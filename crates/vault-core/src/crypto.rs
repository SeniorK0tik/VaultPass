//! Криптографический слой хранилища «Сейф».
//!
//! Схема с промежуточным ключом данных:
//!
//! ```text
//!   мастер-пароль ──Argon2id(salt_m, m/t/p)──► KEK_m ─┐
//!                                                     ├─► обёртка DK (XChaCha20-Poly1305)
//!   ключ восстановления ──HKDF-SHA256(salt_r)──► KEK_r ┘
//!
//!   DK (32 случайных байта) ──XChaCha20-Poly1305(nonce, AAD=заголовок)──► полезная нагрузка
//! ```
//!
//! Смысл промежуточного `DK`: смена мастер-пароля переписывает только 48 байт
//! обёртки, а не весь сейф, и оба способа входа (пароль и ключ восстановления)
//! ведут к одному и тому же ключу данных.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Error, Result};

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;

/// Минимальная длина мастер-пароля. Ниже этого порога Argon2id уже не спасает.
pub const MIN_MASTER_PASSWORD_LEN: usize = 8;

/// 32 байта ключа, которые затираются при уничтожении значения.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretKey([u8; KEY_LEN]);

impl SecretKey {
    pub fn from_bytes(b: [u8; KEY_LEN]) -> Self {
        Self(b)
    }

    pub fn random() -> Result<Self> {
        let mut b = [0u8; KEY_LEN];
        fill_random(&mut b)?;
        Ok(Self(b))
    }

    pub fn expose(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(<скрыт>)")
    }
}

/// Параметры Argon2id, записанные в заголовок файла: старое хранилище
/// открывается своими параметрами, даже если умолчания в программе выросли.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KdfParams {
    /// Память в КиБ.
    pub m_cost: u32,
    /// Число проходов.
    pub t_cost: u32,
    /// Степень параллелизма.
    pub p_cost: u32,
}

impl Default for KdfParams {
    /// 64 МиБ / 3 прохода / 4 потока — выше рекомендации OWASP для Argon2id
    /// и всё ещё около четверти секунды на настольной машине.
    fn default() -> Self {
        Self {
            m_cost: 65536,
            t_cost: 3,
            p_cost: 4,
        }
    }
}

impl KdfParams {
    fn build(&self) -> Result<argon2::Argon2<'static>> {
        let params = argon2::Params::new(self.m_cost, self.t_cost, self.p_cost, Some(KEY_LEN))
            .map_err(|e| Error::Corrupt(format!("недопустимые параметры Argon2id: {e}")))?;
        Ok(argon2::Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params,
        ))
    }
}

/// Заполняет буфер байтами системного ГСЧ. Любая ошибка здесь фатальна:
/// генерировать ключи из непроверенного источника нельзя.
pub fn fill_random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| Error::Random(e.to_string()))
}

pub fn random_salt() -> Result<[u8; SALT_LEN]> {
    let mut s = [0u8; SALT_LEN];
    fill_random(&mut s)?;
    Ok(s)
}

pub fn random_nonce() -> Result<[u8; NONCE_LEN]> {
    let mut n = [0u8; NONCE_LEN];
    fill_random(&mut n)?;
    Ok(n)
}

/// Argon2id: мастер-пароль → ключ шифрования ключа.
pub fn derive_kek(password: &str, salt: &[u8], params: KdfParams) -> Result<SecretKey> {
    let mut out = [0u8; KEY_LEN];
    params
        .build()?
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| Error::Other(format!("Argon2id: {e}")))?;
    Ok(SecretKey::from_bytes(out))
}

/// HKDF-SHA256 для ключа восстановления. Он и так состоит из 256 бит
/// системной энтропии, растягивать его Argon2id незачем — нужен только
/// домен-разделитель, чтобы ключ нельзя было переиспользовать в другой роли.
pub fn derive_recovery_kek(recovery_key: &[u8; KEY_LEN], salt: &[u8]) -> Result<SecretKey> {
    type H = Hmac<Sha256>;

    // extract
    let mut mac =
        <H as Mac>::new_from_slice(salt).map_err(|e| Error::Other(format!("HKDF extract: {e}")))?;
    mac.update(recovery_key);
    let prk = mac.finalize().into_bytes();

    // expand (одна итерация — нужен ровно один блок в 32 байта)
    let mut mac =
        <H as Mac>::new_from_slice(&prk).map_err(|e| Error::Other(format!("HKDF expand: {e}")))?;
    mac.update(b"seif/recovery-kek/v3");
    mac.update(&[0x01]);
    let okm = mac.finalize().into_bytes();

    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&okm[..KEY_LEN]);
    Ok(SecretKey::from_bytes(out))
}

/// XChaCha20-Poly1305. `aad` привязывает шифротекст к заголовку файла:
/// подменённые параметры Argon2id или номер формата ломают проверку тега.
pub fn seal(
    key: &SecretKey,
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.expose().into());
    cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Other("шифрование не удалось".into()))
}

/// Расшифровка. Неотличимость «не тот ключ» от «файл испорчен» — свойство AEAD,
/// поэтому вызывающая сторона сама решает, каким текстом объяснить неудачу.
pub fn open(
    key: &SecretKey,
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.expose().into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .ok()
}

/// Ключ восстановления в человекочитаемом виде: 52 символа Base32 без
/// смешиваемых глифов, разбитые по 4 — `XXXX-XXXX-…`. Ровно те «••••-••••-••••-••••»,
/// что показаны в макете.
pub fn format_recovery_key(key: &[u8; KEY_LEN]) -> String {
    let enc = data_encoding::BASE32_NOPAD.encode(key);
    enc.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or_default().to_string())
        .collect::<Vec<_>>()
        .join("-")
}

/// Разбор ключа восстановления: дефисы, пробелы и регистр значения не имеют.
pub fn parse_recovery_key(text: &str) -> Result<[u8; KEY_LEN]> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let bytes = data_encoding::BASE32_NOPAD
        .decode(cleaned.as_bytes())
        .map_err(|_| Error::BadRecoveryKey)?;
    if bytes.len() != KEY_LEN {
        return Err(Error::BadRecoveryKey);
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Быстрые параметры: боевые 64 МиБ в тестах не нужны.
    fn fast() -> KdfParams {
        KdfParams {
            m_cost: 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn kek_is_deterministic_and_salt_dependent() {
        let salt_a = [7u8; SALT_LEN];
        let salt_b = [9u8; SALT_LEN];
        let a1 = derive_kek("пароль", &salt_a, fast()).unwrap();
        let a2 = derive_kek("пароль", &salt_a, fast()).unwrap();
        let b = derive_kek("пароль", &salt_b, fast()).unwrap();
        assert_eq!(a1.expose(), a2.expose());
        assert_ne!(a1.expose(), b.expose());
    }

    #[test]
    fn seal_open_roundtrip() {
        let key = SecretKey::random().unwrap();
        let nonce = random_nonce().unwrap();
        let ct = seal(&key, &nonce, b"aad", "тайна".as_bytes()).unwrap();
        assert_eq!(open(&key, &nonce, b"aad", &ct).unwrap(), "тайна".as_bytes());
    }

    #[test]
    fn tampered_aad_fails() {
        let key = SecretKey::random().unwrap();
        let nonce = random_nonce().unwrap();
        let ct = seal(&key, &nonce, "заголовок".as_bytes(), "данные".as_bytes()).unwrap();
        assert!(open(&key, &nonce, "подменённый".as_bytes(), &ct).is_none());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = SecretKey::random().unwrap();
        let nonce = random_nonce().unwrap();
        let mut ct = seal(&key, &nonce, b"", "данные".as_bytes()).unwrap();
        ct[0] ^= 1;
        assert!(open(&key, &nonce, b"", &ct).is_none());
    }

    #[test]
    fn recovery_key_roundtrip() {
        let mut raw = [0u8; KEY_LEN];
        fill_random(&mut raw).unwrap();
        let text = format_recovery_key(&raw);
        assert!(text.contains('-'));
        assert_eq!(parse_recovery_key(&text).unwrap(), raw);
        // регистр и лишние разделители не мешают
        assert_eq!(
            parse_recovery_key(&text.to_lowercase().replace('-', " ")).unwrap(),
            raw
        );
    }

    #[test]
    fn recovery_kek_is_domain_separated() {
        let raw = [3u8; KEY_LEN];
        let salt = [1u8; SALT_LEN];
        let k = derive_recovery_kek(&raw, &salt).unwrap();
        assert_ne!(k.expose(), &raw);
    }
}
