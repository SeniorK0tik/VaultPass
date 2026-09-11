//! Хранилище сид-фраз — отдельный файл, отдельный мастер-пароль, отдельный ключ.
//!
//! Формат устроен как у основного сейфа (`vault.rs`), но это **другой** файл с
//! другой магией и другими дополнительными данными AEAD: сид-файл нельзя
//! открыть кодом обычного сейфа, а обычный сейф — кодом отсюда, и подмена
//! одного другим не проходит проверку тега.
//!
//! Отличий от `vault.rs` три, и все три намеренные:
//!
//! 1. **Argon2id вчетверо дороже** — 256 МиБ против 64. Раздел открывают
//!    редко и осознанно, секунда на разблокировку здесь не цена, а перебор
//!    мастер-пароля дорожает во столько же раз.
//! 2. **Обёртки ключа восстановления нет вовсе** — не выключена, а
//!    отсутствует как поле. Вторая обёртка ключа это вторая цель для атаки, а
//!    бумажка с ключом восстановления рядом с сид-фразами обесценивает всю
//!    затею.
//! 3. **Корзины нет.** Мягкое удаление сид-фразы означало бы хранить её
//!    дольше, чем просил пользователь. Удаление сразу и навсегда; подтверждает
//!    его вызывающая сторона, а прежняя версия файла остаётся в `backups/`.
//!
//! Промежуточные буферы с открытым текстом затираются сразу после
//! использования: `serde_json` выдаёт расшифрованное содержимое обычным
//! `Vec<u8>`, и оставлять его в куче до следующего выделения памяти незачем.

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::bip39;
use crate::crypto::{self, KdfParams, SecretKey, NONCE_LEN};
use crate::error::{Error, Result};

pub const SEED_FORMAT_VERSION: u32 = 1;
const MAGIC: &str = "seif-seed";
/// Сколько прошлых копий держать рядом с файлом.
pub const SEED_BACKUP_KEEP: usize = 5;

/// Мастер-пароль хранилища сид-фраз длиннее, чем у основного сейфа: за ним
/// стоят кошельки, а не учётная запись, которую можно восстановить по почте.
pub const MIN_SEED_PASSWORD_LEN: usize = 12;

/// Argon2id для сид-файла: 256 МиБ / 4 прохода / 4 потока — около секунды на
/// настольной машине.
pub fn seed_kdf() -> KdfParams {
    KdfParams {
        m_cost: 262_144,
        t_cost: 4,
        p_cost: 4,
    }
}

// ─── конверт на диске ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct WrappedKey {
    salt: String,
    nonce: String,
    ct: String,
}

#[derive(Serialize, Deserialize)]
struct SeedEnvelope {
    magic: String,
    format: u32,
    kdf: KdfParams,
    master: WrappedKey,
    payload_nonce: String,
    payload: String,
    created_at: DateTime<Utc>,
    modified_at: DateTime<Utc>,
}

/// Содержимое файла при чтении.
#[derive(Default, Deserialize)]
struct SeedData {
    #[serde(default)]
    entries: Vec<SeedEntry>,
}

/// Оно же при записи — но взаймы.
///
/// Отдельная форма, а не `Serialize` на `SeedData`, чтобы сохранение не
/// вынимало записи из хранилища ради сериализации: при любой неудаче на
/// полпути вынутое пришлось бы возвращать, а забытый возврат означал бы
/// сейф, опустевший в памяти.
#[derive(Serialize)]
struct SeedDataRef<'a> {
    entries: &'a [SeedEntry],
}

fn aad_wrap(kdf: KdfParams) -> Vec<u8> {
    format!(
        "seif-seed/v{SEED_FORMAT_VERSION}/wrap|{}|{}|{}",
        kdf.m_cost, kdf.t_cost, kdf.p_cost
    )
    .into_bytes()
}

fn aad_payload() -> &'static [u8] {
    b"seif-seed/v1/payload"
}

fn b64d(s: &str, what: &str) -> Result<Vec<u8>> {
    B64.decode(s)
        .map_err(|_| Error::Corrupt(format!("поле «{what}» не разбирается")))
}

fn fixed<const N: usize>(v: Vec<u8>, what: &str) -> Result<[u8; N]> {
    v.try_into()
        .map_err(|_| Error::Corrupt(format!("поле «{what}» неверной длины")))
}

// ─── запись ──────────────────────────────────────────────────────────────────

/// Сид-фраза одного кошелька.
///
/// Секретны здесь три поля: слова, дополнительное слово-пароль и заметка.
/// Остальное — сведения, по которым запись опознают в списке, и они нужны с
/// закрытыми глазами на содержимое.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedEntry {
    pub id: Uuid,
    /// Как запись названа в списке: «Ledger основной».
    pub title: String,
    /// Кошелёк или устройство — «Ledger Nano S», «Trezor», «Metamask».
    #[serde(default)]
    pub wallet: String,

    pub words: Vec<String>,
    /// Дополнительное слово-пароль BIP39 («25-е слово»). Секрет, и показывается
    /// отдельно от самой фразы.
    #[serde(default)]
    pub passphrase: String,

    /// Фраза по BIP39 — то есть её проверяют по списку и контрольной сумме.
    /// Снимается для кошельков с собственными словарями (Electrum, Monero):
    /// отказать им в хранении было бы хуже, чем не проверить.
    #[serde(default = "yes")]
    pub standard: bool,

    /// Путь выведения — «m/44'/0'/0'». Не секрет.
    #[serde(default)]
    pub derivation: String,
    /// Сеть или монета — «BTC», «ETH». Не секрет.
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub note: String,

    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    /// Когда фразу в последний раз показывали на экране.
    #[serde(default)]
    pub last_viewed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub view_count: u32,
}

fn yes() -> bool {
    true
}

impl SeedEntry {
    pub fn new(title: impl Into<String>, words: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            wallet: String::new(),
            words,
            passphrase: String::new(),
            standard: true,
            derivation: String::new(),
            network: String::new(),
            note: String::new(),
            created_at: now,
            modified_at: now,
            last_viewed_at: None,
            view_count: 0,
        }
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    pub fn has_passphrase(&self) -> bool {
        !self.passphrase.is_empty()
    }
}

impl Drop for SeedEntry {
    fn drop(&mut self) {
        for w in &mut self.words {
            w.zeroize();
        }
        self.passphrase.zeroize();
        self.note.zeroize();
    }
}

/// Проверяет фразу перед тем, как класть её в хранилище.
///
/// Пустых слов быть не должно: пустая строка в середине фразы — это не
/// «необязательное поле», а потерянное слово.
pub fn check_phrase(words: &[String], standard: bool) -> Result<()> {
    if words.iter().any(|w| w.trim().is_empty()) {
        return Err(Error::Other(
            "в фразе есть пустое слово — заполните все поля".into(),
        ));
    }
    if standard {
        bip39::validate(words)?;
    } else if words.is_empty() {
        return Err(Error::BadWordCount(0));
    }
    Ok(())
}

// ─── хранилище в памяти ──────────────────────────────────────────────────────

/// Открытое хранилище сид-фраз. Как и `Vault`, оно держит ключ только внутри
/// себя: «заблокировать» — это `drop`.
pub struct SeedVault {
    path: PathBuf,
    dk: SecretKey,
    kdf: KdfParams,
    master: (Vec<u8>, Vec<u8>, Vec<u8>),
    entries: Vec<SeedEntry>,
    created_at: DateTime<Utc>,
    dirty: bool,
}

impl SeedVault {
    // ── создание и открытие ─────────────────────────────────────────────────

    pub fn create(path: impl Into<PathBuf>, master_password: &str) -> Result<Self> {
        Self::create_with_params(path, master_password, seed_kdf())
    }

    /// То же с заданной стоимостью Argon2id — нужно тестам: боевые 256 МиБ на
    /// вызов превратили бы их в многоминутные.
    pub fn create_with_params(
        path: impl Into<PathBuf>,
        master_password: &str,
        kdf: KdfParams,
    ) -> Result<Self> {
        if master_password.chars().count() < MIN_SEED_PASSWORD_LEN {
            return Err(Error::WeakMasterPassword(MIN_SEED_PASSWORD_LEN));
        }
        let path = path.into();
        if path.exists() {
            return Err(Error::Other(
                "файл с таким именем уже есть — выберите другое имя".into(),
            ));
        }
        let salt = crypto::random_salt()?;
        let nonce = crypto::random_nonce()?;

        let dk = SecretKey::random()?;
        let kek = crypto::derive_kek(master_password, &salt, kdf)?;
        let wrap = crypto::seal(&kek, &nonce, &aad_wrap(kdf), dk.expose())?;

        let mut v = Self {
            path,
            dk,
            kdf,
            master: (salt.to_vec(), nonce.to_vec(), wrap),
            entries: Vec::new(),
            created_at: Utc::now(),
            dirty: true,
        };
        v.save()?;
        Ok(v)
    }

    pub fn open(path: impl Into<PathBuf>, master_password: &str) -> Result<Self> {
        let path = path.into();
        let env = read_envelope(&path)?;

        let salt = b64d(&env.master.salt, "master.salt")?;
        let nonce: [u8; NONCE_LEN] =
            fixed(b64d(&env.master.nonce, "master.nonce")?, "master.nonce")?;
        let ct = b64d(&env.master.ct, "master.ct")?;

        let kek = crypto::derive_kek(master_password, &salt, env.kdf)?;
        let dk_bytes =
            crypto::open(&kek, &nonce, &aad_wrap(env.kdf), &ct).ok_or(Error::BadMasterPassword)?;
        let dk = SecretKey::from_bytes(fixed(dk_bytes, "ключ данных")?);

        let p_nonce: [u8; NONCE_LEN] =
            fixed(b64d(&env.payload_nonce, "payload_nonce")?, "payload_nonce")?;
        let p_ct = b64d(&env.payload, "payload")?;

        let mut plain = crypto::open(&dk, &p_nonce, aad_payload(), &p_ct)
            .ok_or_else(|| Error::Corrupt("не сходится тег полезной нагрузки".into()))?;
        let data: SeedData = serde_json::from_slice(&plain)?;
        // Расшифрованный JSON со всеми фразами больше не нужен.
        plain.zeroize();

        Ok(Self {
            path,
            dk,
            kdf: env.kdf,
            master: (salt, nonce.to_vec(), ct),
            entries: data.entries,
            created_at: env.created_at,
            dirty: false,
        })
    }

    // ── запись на диск ──────────────────────────────────────────────────────

    pub fn save(&mut self) -> Result<()> {
        let mut plain = serde_json::to_vec(&SeedDataRef {
            entries: &self.entries,
        })?;

        let p_nonce = crypto::random_nonce()?;
        let sealed = crypto::seal(&self.dk, &p_nonce, aad_payload(), &plain);
        plain.zeroize();
        let p_ct = sealed?;

        let env = SeedEnvelope {
            magic: MAGIC.into(),
            format: SEED_FORMAT_VERSION,
            kdf: self.kdf,
            master: WrappedKey {
                salt: B64.encode(&self.master.0),
                nonce: B64.encode(&self.master.1),
                ct: B64.encode(&self.master.2),
            },
            payload_nonce: B64.encode(p_nonce),
            payload: B64.encode(&p_ct),
            created_at: self.created_at,
            modified_at: Utc::now(),
        };

        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if self.path.exists() {
            self.rotate_backup()?;
        }

        let tmp = self.path.with_extension("seed.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(&env)?)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        self.dirty = false;
        Ok(())
    }

    fn backup_dir(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups")
    }

    fn rotate_backup(&self) -> Result<()> {
        let dir = self.backup_dir();
        std::fs::create_dir_all(&dir)?;
        let stamp = Utc::now().format("%Y%m%d-%H%M%S");
        std::fs::copy(&self.path, dir.join(format!("seif-seed-{stamp}.seed")))?;

        let mut old: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with("seif-seed-") && name.ends_with(".seed")
            })
            .collect();
        old.sort_by_key(|e| e.file_name());
        while old.len() > SEED_BACKUP_KEEP {
            let victim = old.remove(0);
            let _ = std::fs::remove_file(victim.path());
        }
        Ok(())
    }

    pub fn save_if_dirty(&mut self) -> Result<()> {
        if self.dirty {
            self.save()?;
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    // ── пароль ──────────────────────────────────────────────────────────────

    /// Сходится ли мастер-пароль с тем, которым открыт файл.
    ///
    /// Нужна не для входа, а для подтверждения перед показом фразы: открытое
    /// хранилище за чужим столом не должно выдавать сид-фразу. Сравнение идёт
    /// за постоянное время — «почти верный» пароль не должен отличаться от
    /// неверного ни ответом, ни временем ответа.
    pub fn verify_master_password(&self, password: &str) -> Result<bool> {
        let nonce: [u8; NONCE_LEN] = fixed(self.master.1.clone(), "master.nonce")?;
        let kek = crypto::derive_kek(password, &self.master.0, self.kdf)?;
        let Some(mut dk_bytes) = crypto::open(&kek, &nonce, &aad_wrap(self.kdf), &self.master.2)
        else {
            return Ok(false);
        };
        let same = dk_bytes.ct_eq(self.dk.expose().as_slice()).into();
        dk_bytes.zeroize();
        Ok(same)
    }

    pub fn change_master_password(&mut self, current: &str, new: &str) -> Result<()> {
        if new.chars().count() < MIN_SEED_PASSWORD_LEN {
            return Err(Error::WeakMasterPassword(MIN_SEED_PASSWORD_LEN));
        }
        if !self.verify_master_password(current)? {
            return Err(Error::BadMasterPassword);
        }
        let salt = crypto::random_salt()?;
        let nonce = crypto::random_nonce()?;
        let kek = crypto::derive_kek(new, &salt, self.kdf)?;
        let wrap = crypto::seal(&kek, &nonce, &aad_wrap(self.kdf), self.dk.expose())?;
        self.master = (salt.to_vec(), nonce.to_vec(), wrap);
        self.dirty = true;
        self.save()
    }

    // ── записи ──────────────────────────────────────────────────────────────

    pub fn entries(&self) -> &[SeedEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: Uuid) -> Result<&SeedEntry> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .ok_or(Error::NoSuchSeedEntry(id))
    }

    fn get_mut(&mut self, id: Uuid) -> Result<&mut SeedEntry> {
        self.entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(Error::NoSuchSeedEntry(id))
    }

    pub fn add(&mut self, entry: SeedEntry) -> Result<Uuid> {
        check_phrase(&entry.words, entry.standard)?;
        let id = entry.id;
        self.entries.push(entry);
        self.dirty = true;
        Ok(id)
    }

    /// Меняет всё, кроме самой фразы: название, кошелёк, путь выведения, сеть,
    /// заметку.
    ///
    /// Фраза отдельной операцией намеренно. Иначе правка сведений заставляла бы
    /// форму редактирования держать фразу у себя — то есть появилось бы второе
    /// место, где она лежит открытым текстом, причём при каждом переименовании.
    #[allow(clippy::too_many_arguments)]
    pub fn update_details(
        &mut self,
        id: Uuid,
        title: String,
        wallet: String,
        derivation: String,
        network: String,
        note: String,
    ) -> Result<()> {
        let e = self.get_mut(id)?;
        e.title = title;
        e.wallet = wallet;
        e.derivation = derivation;
        e.network = network;
        e.note.zeroize();
        e.note = note;
        e.modified_at = Utc::now();
        self.dirty = true;
        Ok(())
    }

    /// Заменяет фразу целиком. Прежней версии не остаётся: история сид-фраз —
    /// это хранить старый секрет дольше, чем он нужен.
    pub fn replace_phrase(
        &mut self,
        id: Uuid,
        words: Vec<String>,
        passphrase: String,
        standard: bool,
    ) -> Result<()> {
        check_phrase(&words, standard)?;
        let e = self.get_mut(id)?;
        for w in &mut e.words {
            w.zeroize();
        }
        e.words = words;
        e.passphrase.zeroize();
        e.passphrase = passphrase;
        e.standard = standard;
        e.modified_at = Utc::now();
        self.dirty = true;
        Ok(())
    }

    pub fn remove(&mut self, id: Uuid) -> Result<()> {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() == before {
            return Err(Error::NoSuchSeedEntry(id));
        }
        self.dirty = true;
        Ok(())
    }

    /// Отмечает показ фразы — «последний просмотр» в карточке.
    pub fn mark_viewed(&mut self, id: Uuid) -> Result<()> {
        let e = self.get_mut(id)?;
        e.view_count = e.view_count.saturating_add(1);
        e.last_viewed_at = Some(Utc::now());
        self.dirty = true;
        Ok(())
    }

    /// Самопроверка: совпадает ли набранная заново фраза с сохранённой.
    ///
    /// Отвечает «да» или «нет» и ничего не показывает — это единственный
    /// способ сверить бумажную копию, не выводя фразу на экран. Сравнение за
    /// постоянное время, чтобы по задержке нельзя было угадывать слова
    /// по одному.
    pub fn verify_phrase(&self, id: Uuid, words: &[String]) -> Result<bool> {
        let e = self.get(id)?;
        let mut mine = e
            .words
            .iter()
            .map(|w| bip39::normalize(w))
            .collect::<Vec<_>>()
            .join(" ");
        let mut theirs = words
            .iter()
            .map(|w| bip39::normalize(w))
            .collect::<Vec<_>>()
            .join(" ");

        let same = if mine.len() == theirs.len() {
            mine.as_bytes().ct_eq(theirs.as_bytes()).into()
        } else {
            false
        };

        mine.zeroize();
        theirs.zeroize();
        Ok(same)
    }
}

impl std::fmt::Debug for SeedVault {
    /// Ни ключа, ни фраз, ни даже названий записей: строка `{seed:?}` в журнале
    /// не должна рассказывать о содержимом.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeedVault")
            .field("path", &self.path)
            .field("entries", &self.entries.len())
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

/// Что можно узнать о файле, не зная пароля.
///
/// Ровно то, что и так лежит в заголовке открытым текстом: что это вообще
/// хранилище сид-фраз, какой оно версии и когда его последний раз писали.
/// Ни числа записей, ни названий здесь нет — снаружи их не видно.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SeedFileInfo {
    pub format: u32,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

/// Опознаёт файл до ввода пароля — чтобы на выбор чужого файла ответить
/// «это не хранилище сид-фраз», а не «неверный мастер-пароль».
pub fn peek(path: &Path) -> Result<SeedFileInfo> {
    let env = read_envelope(path)?;
    Ok(SeedFileInfo {
        format: env.format,
        created_at: env.created_at,
        modified_at: env.modified_at,
    })
}

fn read_envelope(path: &Path) -> Result<SeedEnvelope> {
    let raw = std::fs::read(path)?;
    let env: SeedEnvelope = serde_json::from_slice(&raw).map_err(|_| Error::NotASeedVault)?;
    if env.magic != MAGIC {
        return Err(Error::NotASeedVault);
    }
    if env.format != SEED_FORMAT_VERSION {
        return Err(Error::UnsupportedSeedFormat(env.format));
    }
    if env.master.salt.is_empty() || env.master.ct.is_empty() {
        return Err(Error::Corrupt("нет обёртки мастер-ключа".into()));
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Настоящая фраза из векторов BIP-0039 — со сходящейся контрольной суммой.
    const PHRASE: &str =
        "letter advice cage absurd amount doctor acoustic avoid letter advice cage above";
    const OTHER: &str =
        "legal winner thank year wave sausage worth useful legal winner thank yellow";

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("seif-seed-test-{}-{}", std::process::id(), name));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn file(&self) -> PathBuf {
            self.0.join("wallets.seed")
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fast() -> KdfParams {
        KdfParams {
            m_cost: 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn make(tmp: &Tmp, pw: &str) -> SeedVault {
        SeedVault::create_with_params(tmp.file(), pw, fast()).unwrap()
    }

    #[test]
    fn create_open_roundtrip() {
        let tmp = Tmp::new("roundtrip");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        let mut e = SeedEntry::new("Ledger основной", words(PHRASE));
        e.wallet = "Ledger Nano S".into();
        e.network = "BTC".into();
        let id = v.add(e).unwrap();
        v.save().unwrap();
        drop(v);

        let v2 = SeedVault::open(tmp.file(), "мастер-пароль сид-волта").unwrap();
        let got = v2.get(id).unwrap();
        assert_eq!(got.title, "Ledger основной");
        assert_eq!(got.words, words(PHRASE));
        assert_eq!(got.word_count(), 12);
    }

    #[test]
    fn the_phrase_never_appears_in_the_file() {
        let tmp = Tmp::new("opaque");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        let mut e = SeedEntry::new("Холодный кошелёк", words(PHRASE));
        e.passphrase = "двадцать-пятое-слово".into();
        e.note = "лежит в сейфе у мамы".into();
        v.add(e).unwrap();
        v.save().unwrap();

        let raw = std::fs::read_to_string(tmp.file()).unwrap();
        for needle in [
            "letter",
            "absurd",
            "acoustic",
            "Холодный кошелёк",
            "двадцать-пятое-слово",
            "лежит в сейфе у мамы",
        ] {
            assert!(
                !raw.contains(needle),
                "«{needle}» видно в файле открытым текстом"
            );
        }
    }

    #[test]
    fn wrong_password_is_rejected() {
        let tmp = Tmp::new("wrongpw");
        drop(make(&tmp, "правильный пароль"));
        assert_eq!(
            SeedVault::open(tmp.file(), "неправильный пароль")
                .unwrap_err()
                .code(),
            "bad_master_password"
        );
    }

    #[test]
    fn a_short_password_is_refused() {
        let tmp = Tmp::new("weak");
        assert_eq!(
            SeedVault::create_with_params(tmp.file(), "коротко", fast())
                .unwrap_err()
                .code(),
            "weak_master_password"
        );
    }

    /// Сид-файл и обычный сейф не должны подменять друг друга: у них разная
    /// магия, и ошибка об этом говорит прямо, а не «неверный пароль».
    #[test]
    fn a_plain_vault_is_not_mistaken_for_a_seed_vault() {
        let tmp = Tmp::new("mixup");
        let plain = tmp.0.join("seif.vault");
        crate::vault::Vault::create_with_params(&plain, "мастер-пароль", fast()).unwrap();
        assert_eq!(
            SeedVault::open(&plain, "мастер-пароль").unwrap_err().code(),
            "not_a_seed_vault"
        );

        drop(make(&tmp, "мастер-пароль сид-волта"));
        assert_eq!(
            crate::vault::Vault::open(tmp.file(), "мастер-пароль сид-волта")
                .unwrap_err()
                .code(),
            "not_a_vault"
        );
    }

    #[test]
    fn downgrading_kdf_params_breaks_the_tag() {
        let tmp = Tmp::new("aad");
        drop(make(&tmp, "мастер-пароль сид-волта"));

        let mut env: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(tmp.file()).unwrap()).unwrap();
        env["kdf"]["m_cost"] = serde_json::json!(8);
        env["kdf"]["t_cost"] = serde_json::json!(1);
        std::fs::write(tmp.file(), serde_json::to_vec(&env).unwrap()).unwrap();

        assert!(SeedVault::open(tmp.file(), "мастер-пароль сид-волта").is_err());
    }

    #[test]
    fn a_phrase_that_does_not_add_up_is_not_stored() {
        let tmp = Tmp::new("badphrase");
        let mut v = make(&tmp, "мастер-пароль сид-волта");

        let mut bad = words(PHRASE);
        bad.swap(0, 1);
        assert_eq!(
            v.add(SeedEntry::new("Кривая", bad)).unwrap_err().code(),
            "bad_checksum"
        );

        let short = words("letter advice cage");
        assert_eq!(
            v.add(SeedEntry::new("Короткая", short)).unwrap_err().code(),
            "bad_word_count"
        );

        let mut empty = words(PHRASE);
        empty[3] = "  ".into();
        assert!(v.add(SeedEntry::new("С дырой", empty)).is_err());
        assert!(v.is_empty(), "ни одна из них не попала в хранилище");
    }

    /// Кошельки с собственными словарями (Electrum, Monero) не проходят
    /// проверку BIP39 — и всё равно должны храниться.
    #[test]
    fn a_non_standard_phrase_is_allowed_when_marked_as_such() {
        let tmp = Tmp::new("nonstandard");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        let mut e = SeedEntry::new(
            "Monero",
            words("абырвалг глокая куздра штеко будланула бокра"),
        );
        e.standard = false;
        assert!(v.add(e).is_ok());
    }

    #[test]
    fn verify_master_password_accepts_only_the_right_one() {
        let tmp = Tmp::new("verifypw");
        let v = make(&tmp, "мастер-пароль сид-волта");
        assert!(v.verify_master_password("мастер-пароль сид-волта").unwrap());
        assert!(!v.verify_master_password("мастер-пароль сид-волт").unwrap());
        assert!(!v.verify_master_password("").unwrap());
    }

    #[test]
    fn changing_the_password_keeps_the_phrases_and_needs_the_old_one() {
        let tmp = Tmp::new("changepw");
        let mut v = make(&tmp, "старый пароль сид-волта");
        let id = v.add(SeedEntry::new("Ledger", words(PHRASE))).unwrap();
        v.save().unwrap();

        assert_eq!(
            v.change_master_password("не тот", "новый пароль сид-волта")
                .unwrap_err()
                .code(),
            "bad_master_password"
        );
        v.change_master_password("старый пароль сид-волта", "новый пароль сид-волта")
            .unwrap();
        drop(v);

        assert!(SeedVault::open(tmp.file(), "старый пароль сид-волта").is_err());
        let v2 = SeedVault::open(tmp.file(), "новый пароль сид-волта").unwrap();
        assert_eq!(v2.get(id).unwrap().words, words(PHRASE));
    }

    #[test]
    fn verify_phrase_compares_without_showing_anything() {
        let tmp = Tmp::new("selfcheck");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        let id = v.add(SeedEntry::new("Ledger", words(PHRASE))).unwrap();

        assert!(v.verify_phrase(id, &words(PHRASE)).unwrap());
        // Регистр и лишние пробелы не должны считаться ошибкой записи.
        assert!(v.verify_phrase(id, &words(&PHRASE.to_uppercase())).unwrap());
        assert!(!v.verify_phrase(id, &words(OTHER)).unwrap());
        assert!(!v.verify_phrase(id, &words("letter advice cage")).unwrap());
    }

    #[test]
    fn details_change_without_touching_the_phrase() {
        let tmp = Tmp::new("details");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        let id = v.add(SeedEntry::new("Было", words(PHRASE))).unwrap();

        v.update_details(
            id,
            "Стало".into(),
            "Trezor".into(),
            "m/44'/0'/0'".into(),
            "BTC".into(),
            "заметка".into(),
        )
        .unwrap();

        let e = v.get(id).unwrap();
        assert_eq!(e.title, "Стало");
        assert_eq!(e.wallet, "Trezor");
        assert_eq!(e.words, words(PHRASE), "фраза осталась прежней");
    }

    #[test]
    fn replacing_a_phrase_leaves_no_previous_version() {
        let tmp = Tmp::new("replace");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        let id = v.add(SeedEntry::new("Ledger", words(PHRASE))).unwrap();

        v.replace_phrase(id, words(OTHER), String::new(), true)
            .unwrap();
        v.save().unwrap();

        assert_eq!(v.get(id).unwrap().words, words(OTHER));
        let raw = std::fs::read_to_string(tmp.file()).unwrap();
        assert!(!raw.contains("absurd"), "прежняя фраза не осталась в файле");

        // Негодная замена не должна затирать то, что уже лежит.
        let mut bad = words(OTHER);
        bad.swap(3, 4);
        assert!(v.replace_phrase(id, bad, String::new(), true).is_err());
        assert_eq!(v.get(id).unwrap().words, words(OTHER));
    }

    #[test]
    fn removing_is_immediate_and_there_is_no_trash() {
        let tmp = Tmp::new("remove");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        let id = v.add(SeedEntry::new("Ledger", words(PHRASE))).unwrap();
        v.remove(id).unwrap();
        assert!(v.get(id).is_err());
        assert!(v.is_empty());
        assert_eq!(v.remove(id).unwrap_err().code(), "no_such_seed_entry");
    }

    #[test]
    fn saving_rotates_backups() {
        let tmp = Tmp::new("backups");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        v.add(SeedEntry::new("Ledger", words(PHRASE))).unwrap();
        v.save().unwrap();
        v.save().unwrap();
        let n = std::fs::read_dir(tmp.0.join("backups")).unwrap().count();
        assert!((1..=SEED_BACKUP_KEEP).contains(&n), "копий накопилось {n}");
    }

    #[test]
    fn creating_over_an_existing_file_is_refused() {
        let tmp = Tmp::new("overwrite");
        drop(make(&tmp, "мастер-пароль сид-волта"));
        // Иначе «создать» поверх подключённого файла стёрло бы все кошельки.
        assert!(SeedVault::create_with_params(tmp.file(), "другой пароль тут", fast()).is_err());
    }

    #[test]
    fn peeking_recognises_the_file_without_a_password() {
        let tmp = Tmp::new("peek");
        drop(make(&tmp, "мастер-пароль сид-волта"));

        let info = peek(&tmp.file()).unwrap();
        assert_eq!(info.format, SEED_FORMAT_VERSION);

        let alien = tmp.0.join("alien.seed");
        std::fs::write(&alien, b"{\"hello\":\"world\"}").unwrap();
        assert_eq!(peek(&alien).unwrap_err().code(), "not_a_seed_vault");
    }

    #[test]
    fn debug_output_keeps_quiet_about_the_contents() {
        let tmp = Tmp::new("debug");
        let mut v = make(&tmp, "мастер-пароль сид-волта");
        v.add(SeedEntry::new("Ledger основной", words(PHRASE)))
            .unwrap();
        let text = format!("{v:?}");
        assert!(!text.contains("Ledger"));
        assert!(!text.contains("letter"));
    }
}
