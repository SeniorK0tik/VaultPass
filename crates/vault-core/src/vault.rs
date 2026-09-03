//! Файл хранилища «формата v3» — тот, что подписан в углу экрана настроек.
//!
//! Конверт на диске — JSON, но секретного в нём нет ничего: только параметры
//! Argon2id, соли, нонсы, две обёртки ключа данных и один шифротекст.
//! Названия записей, логины, адреса и теги лежат внутри шифротекста, поэтому
//! по файлу нельзя узнать даже, сколько в сейфе записей.

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{
    self, KdfParams, SecretKey, KEY_LEN, MIN_MASTER_PASSWORD_LEN, NONCE_LEN, SALT_LEN,
};
use crate::error::{Error, Result};
use crate::model::{Entry, EntryKind, Folder};

pub const FORMAT_VERSION: u32 = 3;
const MAGIC: &str = "seif-vault";
/// Сколько прошлых копий хранить рядом с сейфом (раздел «Резервные копии»).
pub const BACKUP_KEEP: usize = 5;
/// Через сколько дней запись из корзины исчезает окончательно.
pub const TRASH_RETENTION_DAYS: i64 = 30;

// ─── конверт на диске ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct WrappedKey {
    salt: String,
    nonce: String,
    ct: String,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    magic: String,
    format: u32,
    kdf: KdfParams,
    master: WrappedKey,
    #[serde(default)]
    recovery: Option<WrappedKey>,
    payload_nonce: String,
    payload: String,
    created_at: DateTime<Utc>,
    modified_at: DateTime<Utc>,
}

/// Расшифрованное содержимое.
#[derive(Default, Serialize, Deserialize)]
struct VaultData {
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    folders: Vec<Folder>,
}

// ─── связывание шифротекста с заголовком ─────────────────────────────────────

/// Дополнительные данные для обёртки ключа. Включают параметры Argon2id,
/// поэтому попытка понизить их (скажем, до одного прохода и мегабайта памяти,
/// чтобы удешевить перебор) ломает проверку тега, а не открывает сейф дешевле.
fn aad_wrap(kdf: KdfParams) -> Vec<u8> {
    format!(
        "seif/v{FORMAT_VERSION}/wrap|{}|{}|{}",
        kdf.m_cost, kdf.t_cost, kdf.p_cost
    )
    .into_bytes()
}

fn aad_payload() -> &'static [u8] {
    b"seif/v3/payload"
}

fn b64d(s: &str, what: &str) -> Result<Vec<u8>> {
    B64.decode(s)
        .map_err(|_| Error::Corrupt(format!("поле «{what}» не разбирается")))
}

fn fixed<const N: usize>(v: Vec<u8>, what: &str) -> Result<[u8; N]> {
    v.try_into()
        .map_err(|_| Error::Corrupt(format!("поле «{what}» неверной длины")))
}

// ─── сейф в памяти ───────────────────────────────────────────────────────────

/// Открытый сейф. Ключ данных живёт только здесь и затирается вместе со
/// структурой, поэтому «заблокировать» — это буквально `drop(vault)`.
pub struct Vault {
    path: PathBuf,
    dk: SecretKey,
    kdf: KdfParams,
    master: (Vec<u8>, Vec<u8>, Vec<u8>), // соль, нонс, обёртка
    recovery: Option<(Vec<u8>, Vec<u8>, Vec<u8>)>,
    data: VaultData,
    created_at: DateTime<Utc>,
    dirty: bool,
}

impl Vault {
    // ── создание и открытие ─────────────────────────────────────────────────

    /// Создаёт новый сейф с параметрами Argon2id по умолчанию и сразу пишет
    /// его на диск.
    pub fn create(path: impl Into<PathBuf>, master_password: &str) -> Result<Self> {
        Self::create_with_params(path, master_password, KdfParams::default())
    }

    /// То же, но с заданной стоимостью Argon2id. Нужно там, где умолчание не
    /// подходит: слабая машина, где 64 МиБ памяти на разблокировку заметны, —
    /// и тесты, которым четверть секунды на вызов обошлась бы в минуты.
    pub fn create_with_params(
        path: impl Into<PathBuf>,
        master_password: &str,
        kdf: KdfParams,
    ) -> Result<Self> {
        if master_password.chars().count() < MIN_MASTER_PASSWORD_LEN {
            return Err(Error::WeakMasterPassword(MIN_MASTER_PASSWORD_LEN));
        }
        let path = path.into();
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
            recovery: None,
            data: VaultData::default(),
            created_at: Utc::now(),
            dirty: true,
        };
        v.save()?;
        Ok(v)
    }

    /// Открывает сейф мастер-паролем.
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

        Self::finish_open(path, env, dk)
    }

    /// Открывает сейф ключом восстановления — «Забыли мастер-пароль?» на 1f.
    pub fn open_with_recovery(path: impl Into<PathBuf>, recovery_key: &str) -> Result<Self> {
        let path = path.into();
        let env = read_envelope(&path)?;
        let rec = env.recovery.as_ref().ok_or(Error::NoRecoveryKey)?;

        let raw = crypto::parse_recovery_key(recovery_key)?;
        let salt = b64d(&rec.salt, "recovery.salt")?;
        let nonce: [u8; NONCE_LEN] = fixed(b64d(&rec.nonce, "recovery.nonce")?, "recovery.nonce")?;
        let ct = b64d(&rec.ct, "recovery.ct")?;

        let kek = crypto::derive_recovery_kek(&raw, &salt)?;
        let dk_bytes =
            crypto::open(&kek, &nonce, &aad_wrap(env.kdf), &ct).ok_or(Error::BadRecoveryKey)?;
        let dk = SecretKey::from_bytes(fixed(dk_bytes, "ключ данных")?);

        Self::finish_open(path, env, dk)
    }

    fn finish_open(path: PathBuf, env: Envelope, dk: SecretKey) -> Result<Self> {
        let p_nonce: [u8; NONCE_LEN] =
            fixed(b64d(&env.payload_nonce, "payload_nonce")?, "payload_nonce")?;
        let p_ct = b64d(&env.payload, "payload")?;

        // Ключ данных уже подтверждён обёрткой, поэтому неудача здесь означает
        // именно порчу файла, а не неверный пароль.
        let plain = crypto::open(&dk, &p_nonce, aad_payload(), &p_ct)
            .ok_or_else(|| Error::Corrupt("не сходится тег полезной нагрузки".into()))?;
        let data: VaultData = serde_json::from_slice(&plain)?;

        let master = (
            b64d(&env.master.salt, "master.salt")?,
            b64d(&env.master.nonce, "master.nonce")?,
            b64d(&env.master.ct, "master.ct")?,
        );
        let recovery = match env.recovery {
            Some(r) => Some((
                b64d(&r.salt, "recovery.salt")?,
                b64d(&r.nonce, "recovery.nonce")?,
                b64d(&r.ct, "recovery.ct")?,
            )),
            None => None,
        };

        let mut v = Self {
            path,
            dk,
            kdf: env.kdf,
            master,
            recovery,
            data,
            created_at: env.created_at,
            dirty: false,
        };
        v.purge_expired_trash();
        Ok(v)
    }

    // ── запись на диск ──────────────────────────────────────────────────────

    /// Пишет сейф целиком. Запись атомарна: новый файл готовится рядом,
    /// сбрасывается на диск и только потом занимает место старого — обрыв
    /// питания посреди сохранения оставляет прежнюю версию, а не огрызок.
    pub fn save(&mut self) -> Result<()> {
        let plain = serde_json::to_vec(&self.data)?;
        let p_nonce = crypto::random_nonce()?;
        let p_ct = crypto::seal(&self.dk, &p_nonce, aad_payload(), &plain)?;

        let env = Envelope {
            magic: MAGIC.into(),
            format: FORMAT_VERSION,
            kdf: self.kdf,
            master: WrappedKey {
                salt: B64.encode(&self.master.0),
                nonce: B64.encode(&self.master.1),
                ct: B64.encode(&self.master.2),
            },
            recovery: self.recovery.as_ref().map(|r| WrappedKey {
                salt: B64.encode(&r.0),
                nonce: B64.encode(&r.1),
                ct: B64.encode(&r.2),
            }),
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

        let tmp = self.path.with_extension("vault.tmp");
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

    /// Копирует текущий файл в `backups/` и оставляет там последние
    /// [`BACKUP_KEEP`] штук.
    fn rotate_backup(&self) -> Result<()> {
        let dir = self.backup_dir();
        std::fs::create_dir_all(&dir)?;
        let stamp = Utc::now().format("%Y%m%d-%H%M%S");
        std::fs::copy(&self.path, dir.join(format!("seif-{stamp}.vault")))?;

        let mut old: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".vault"))
            .collect();
        old.sort_by_key(|e| e.file_name());
        while old.len() > BACKUP_KEEP {
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

    fn touch(&mut self) {
        self.dirty = true;
    }

    // ── ключи ───────────────────────────────────────────────────────────────

    /// Смена мастер-пароля переписывает только обёртку ключа данных: сами
    /// записи не перешифровываются, поэтому операция мгновенна и не может
    /// испортить содержимое.
    pub fn change_master_password(&mut self, current: &str, new: &str) -> Result<()> {
        if new.chars().count() < MIN_MASTER_PASSWORD_LEN {
            return Err(Error::WeakMasterPassword(MIN_MASTER_PASSWORD_LEN));
        }
        // Текущий пароль проверяется заново, а не «раз сейф открыт — значит можно»:
        // открытый сейф мог остаться от предыдущего пользователя за тем же столом.
        let kek_old = crypto::derive_kek(current, &self.master.0, self.kdf)?;
        let nonce_old: [u8; NONCE_LEN] = fixed(self.master.1.clone(), "master.nonce")?;
        crypto::open(&kek_old, &nonce_old, &aad_wrap(self.kdf), &self.master.2)
            .ok_or(Error::BadMasterPassword)?;

        // Новая соль и новый нонс — переиспользовать нонс с новым ключом нельзя.
        let salt = crypto::random_salt()?;
        let nonce = crypto::random_nonce()?;
        let kek = crypto::derive_kek(new, &salt, self.kdf)?;
        let wrap = crypto::seal(&kek, &nonce, &aad_wrap(self.kdf), self.dk.expose())?;
        self.master = (salt.to_vec(), nonce.to_vec(), wrap);
        self.touch();
        self.save()
    }

    /// Создаёт (или пересоздаёт) ключ восстановления и возвращает его один раз.
    /// В сейфе остаётся только обёртка ключа данных — сам ключ нигде не хранится.
    pub fn create_recovery_key(&mut self) -> Result<String> {
        let mut raw = [0u8; KEY_LEN];
        crypto::fill_random(&mut raw)?;
        let salt = crypto::random_salt()?;
        let nonce = crypto::random_nonce()?;
        let kek = crypto::derive_recovery_kek(&raw, &salt)?;
        let wrap = crypto::seal(&kek, &nonce, &aad_wrap(self.kdf), self.dk.expose())?;
        self.recovery = Some((salt.to_vec(), nonce.to_vec(), wrap));
        self.touch();
        self.save()?;
        Ok(crypto::format_recovery_key(&raw))
    }

    pub fn has_recovery_key(&self) -> bool {
        self.recovery.is_some()
    }

    // ── записи ──────────────────────────────────────────────────────────────

    pub fn entries(&self) -> &[Entry] {
        &self.data.entries
    }

    /// Все живые записи — то есть всё, кроме корзины.
    pub fn active(&self) -> impl Iterator<Item = &Entry> {
        self.data.entries.iter().filter(|e| !e.is_deleted())
    }

    pub fn get(&self, id: Uuid) -> Result<&Entry> {
        self.data
            .entries
            .iter()
            .find(|e| e.id == id)
            .ok_or(Error::NoSuchEntry(id))
    }

    pub fn get_mut(&mut self, id: Uuid) -> Result<&mut Entry> {
        self.dirty = true;
        self.data
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(Error::NoSuchEntry(id))
    }

    pub fn add(&mut self, entry: Entry) -> Uuid {
        let id = entry.id;
        self.data.entries.push(entry);
        self.touch();
        id
    }

    /// Заменяет запись целиком, сохраняя историю пароля и счётчик обращений:
    /// форма редактирования их не присылает, а терять их нельзя.
    pub fn update(&mut self, mut entry: Entry) -> Result<()> {
        let old = self.get(entry.id)?;
        let old_password = old.password.clone();
        let old_pw_modified = old.password_modified_at;
        entry.created_at = old.created_at;
        entry.usage_count = old.usage_count;
        entry.last_used_at = old.last_used_at;
        entry.history = old.history.clone();
        entry.modified_at = Utc::now();

        if entry.password != old_password {
            let new_password = std::mem::take(&mut entry.password);
            entry.password = old_password;
            entry.set_password(new_password);
        } else {
            entry.password_modified_at = old_pw_modified;
        }

        let slot = self
            .data
            .entries
            .iter_mut()
            .find(|e| e.id == entry.id)
            .ok_or(Error::NoSuchEntry(entry.id))?;
        *slot = entry;
        self.touch();
        Ok(())
    }

    /// Мягкое удаление — запись уезжает в корзину.
    pub fn trash(&mut self, id: Uuid) -> Result<()> {
        self.get_mut(id)?.deleted_at = Some(Utc::now());
        Ok(())
    }

    pub fn restore(&mut self, id: Uuid) -> Result<()> {
        self.get_mut(id)?.deleted_at = None;
        Ok(())
    }

    /// Необратимое удаление.
    pub fn purge(&mut self, id: Uuid) -> Result<()> {
        let before = self.data.entries.len();
        self.data.entries.retain(|e| e.id != id);
        if self.data.entries.len() == before {
            return Err(Error::NoSuchEntry(id));
        }
        self.touch();
        Ok(())
    }

    pub fn empty_trash(&mut self) {
        self.data.entries.retain(|e| !e.is_deleted());
        self.touch();
    }

    /// Вычищает корзину от записей старше [`TRASH_RETENTION_DAYS`].
    /// Вызывается при открытии сейфа.
    fn purge_expired_trash(&mut self) {
        let cutoff = Utc::now() - Duration::days(TRASH_RETENTION_DAYS);
        let before = self.data.entries.len();
        self.data
            .entries
            .retain(|e| e.deleted_at.map(|d| d > cutoff).unwrap_or(true));
        if self.data.entries.len() != before {
            self.dirty = true;
        }
    }

    /// Отмечает обращение к записи — на этом держится раздел
    /// «Часто используемые» и порядок в мини-окне.
    pub fn mark_used(&mut self, id: Uuid) -> Result<()> {
        let e = self.get_mut(id)?;
        e.usage_count = e.usage_count.saturating_add(1);
        e.last_used_at = Some(Utc::now());
        Ok(())
    }

    // ── папки ───────────────────────────────────────────────────────────────

    pub fn folders(&self) -> &[Folder] {
        &self.data.folders
    }

    pub fn add_folder(&mut self, name: impl Into<String>) -> Uuid {
        let f = Folder {
            id: Uuid::new_v4(),
            name: name.into(),
            created_at: Some(Utc::now()),
        };
        let id = f.id;
        self.data.folders.push(f);
        self.touch();
        id
    }

    pub fn rename_folder(&mut self, id: Uuid, name: impl Into<String>) -> Result<()> {
        let f = self
            .data
            .folders
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or(Error::NoSuchFolder(id))?;
        f.name = name.into();
        self.touch();
        Ok(())
    }

    /// Удаляет папку; записи из неё не пропадают, а просто теряют папку.
    pub fn remove_folder(&mut self, id: Uuid) -> Result<()> {
        let before = self.data.folders.len();
        self.data.folders.retain(|f| f.id != id);
        if self.data.folders.len() == before {
            return Err(Error::NoSuchFolder(id));
        }
        for e in &mut self.data.entries {
            if e.folder == Some(id) {
                e.folder = None;
            }
        }
        self.touch();
        Ok(())
    }

    // ── поиск и подсчёты ────────────────────────────────────────────────────

    /// Поиск для палитры (1a/1b) и строки поиска главного окна.
    /// Возвращает идентификаторы в порядке ранга.
    pub fn search(&self, needle: &str, limit: usize) -> Vec<Uuid> {
        let n = needle.trim().to_lowercase();
        let mut hits: Vec<&Entry> = self.active().filter(|e| e.matches(&n)).collect();
        hits.sort_by(|a, b| {
            a.match_rank(&n)
                .cmp(&b.match_rank(&n))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        hits.into_iter().take(limit).map(|e| e.id).collect()
    }

    /// Недавние записи для мини-окна у трея (1c).
    pub fn recent(&self, limit: usize) -> Vec<Uuid> {
        let mut v: Vec<&Entry> = self.active().filter(|e| e.quick_access).collect();
        v.sort_by(|a, b| {
            b.last_used_at
                .cmp(&a.last_used_at)
                .then_with(|| b.usage_count.cmp(&a.usage_count))
        });
        v.into_iter().take(limit).map(|e| e.id).collect()
    }

    /// Счётчики слева на экране 1d: всего, по типам, избранное, корзина.
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for e in &self.data.entries {
            if e.is_deleted() {
                c.trash += 1;
                continue;
            }
            c.total += 1;
            match e.kind {
                EntryKind::Password => c.passwords += 1,
                EntryKind::Note => c.notes += 1,
                EntryKind::ApiKey => c.api_keys += 1,
                EntryKind::Document => c.documents += 1,
            }
            if e.favorite {
                c.favorites += 1;
            }
        }
        c
    }
}

impl std::fmt::Debug for Vault {
    /// Ключ данных и содержимое записей в отладочный вывод не попадают:
    /// строка `{vault:?}` в логе не должна раскрывать сейф.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("path", &self.path)
            .field("entries", &self.data.entries.len())
            .field("folders", &self.data.folders.len())
            .field("has_recovery_key", &self.recovery.is_some())
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Counts {
    pub total: usize,
    pub passwords: usize,
    pub notes: usize,
    pub api_keys: usize,
    pub documents: usize,
    pub favorites: usize,
    pub trash: usize,
}

fn read_envelope(path: &Path) -> Result<Envelope> {
    let raw = std::fs::read(path)?;
    let env: Envelope = serde_json::from_slice(&raw).map_err(|_| Error::NotAVault)?;
    if env.magic != MAGIC {
        return Err(Error::NotAVault);
    }
    if env.format != FORMAT_VERSION {
        return Err(Error::UnsupportedFormat(env.format));
    }
    if env.master.salt.is_empty() || env.master.ct.is_empty() {
        return Err(Error::Corrupt("нет обёртки мастер-ключа".into()));
    }
    let _ = SALT_LEN;
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryKind;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("seif-test-{}-{}", std::process::id(), name));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn file(&self) -> PathBuf {
            self.0.join("seif.vault")
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Боевые параметры Argon2id в тестах стоили бы четверть секунды на вызов.
    fn fast() -> KdfParams {
        KdfParams {
            m_cost: 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn make(tmp: &Tmp, pw: &str) -> Vault {
        Vault::create_with_params(tmp.file(), pw, fast()).unwrap()
    }

    #[test]
    fn create_open_roundtrip() {
        let tmp = Tmp::new("roundtrip");
        let mut v = make(&tmp, "мастер-пароль");
        let mut e = Entry::new(EntryKind::Password, "GitHub");
        e.username = "annakuz".into();
        e.set_password("k7$Rm2-vQx9Lp!Zt".into());
        let id = v.add(e);
        v.save().unwrap();
        drop(v);

        let v2 = Vault::open(tmp.file(), "мастер-пароль").unwrap();
        let got = v2.get(id).unwrap();
        assert_eq!(got.title, "GitHub");
        assert_eq!(got.password, "k7$Rm2-vQx9Lp!Zt");
    }

    #[test]
    fn wrong_password_is_rejected() {
        let tmp = Tmp::new("wrongpw");
        let v = make(&tmp, "правильный пароль");
        drop(v);
        let err = Vault::open(tmp.file(), "неправильный пароль").unwrap_err();
        assert_eq!(err.code(), "bad_master_password");
    }

    #[test]
    fn secrets_never_appear_in_the_file() {
        let tmp = Tmp::new("opaque");
        let mut v = make(&tmp, "мастер-пароль");
        let mut e = Entry::new(EntryKind::Password, "СекретноеНазвание");
        e.username = "секретный-логин".into();
        e.set_password("СекретныйПароль".into());
        v.add(e);
        v.save().unwrap();

        let raw = std::fs::read_to_string(tmp.file()).unwrap();
        for needle in ["СекретноеНазвание", "секретный-логин", "СекретныйПароль"]
        {
            assert!(
                !raw.contains(needle),
                "«{needle}» видно в файле открытым текстом"
            );
        }
    }

    #[test]
    fn downgrading_kdf_params_breaks_the_tag() {
        let tmp = Tmp::new("aad");
        let v = make(&tmp, "мастер-пароль");
        drop(v);

        // Атакующий переписывает параметры на самые дешёвые.
        let mut env: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(tmp.file()).unwrap()).unwrap();
        env["kdf"]["m_cost"] = serde_json::json!(8);
        env["kdf"]["t_cost"] = serde_json::json!(1);
        env["kdf"]["p_cost"] = serde_json::json!(1);
        std::fs::write(tmp.file(), serde_json::to_vec(&env).unwrap()).unwrap();

        assert!(Vault::open(tmp.file(), "мастер-пароль").is_err());
    }

    #[test]
    fn recovery_key_opens_the_vault_and_wrong_one_does_not() {
        let tmp = Tmp::new("recovery");
        let mut v = make(&tmp, "мастер-пароль");
        let id = v.add(Entry::new(EntryKind::Note, "Коды"));
        let key = v.create_recovery_key().unwrap();
        drop(v);

        let v2 = Vault::open_with_recovery(tmp.file(), &key).unwrap();
        assert!(v2.get(id).is_ok());

        let bogus = crypto::format_recovery_key(&[0u8; KEY_LEN]);
        assert_eq!(
            Vault::open_with_recovery(tmp.file(), &bogus)
                .unwrap_err()
                .code(),
            "bad_recovery_key"
        );
    }

    #[test]
    fn recovery_missing_is_its_own_error() {
        let tmp = Tmp::new("norecovery");
        let v = make(&tmp, "мастер-пароль");
        drop(v);
        let key = crypto::format_recovery_key(&[1u8; KEY_LEN]);
        assert_eq!(
            Vault::open_with_recovery(tmp.file(), &key)
                .unwrap_err()
                .code(),
            "no_recovery_key"
        );
    }

    #[test]
    fn changing_master_password_keeps_the_data_and_the_recovery_key() {
        let tmp = Tmp::new("changepw");
        let mut v = make(&tmp, "старый пароль");
        let id = v.add(Entry::new(EntryKind::Password, "Запись"));
        let key = v.create_recovery_key().unwrap();
        v.change_master_password("старый пароль", "новый пароль")
            .unwrap();
        drop(v);

        assert!(Vault::open(tmp.file(), "старый пароль").is_err());
        assert!(Vault::open(tmp.file(), "новый пароль")
            .unwrap()
            .get(id)
            .is_ok());
        assert!(Vault::open_with_recovery(tmp.file(), &key)
            .unwrap()
            .get(id)
            .is_ok());
    }

    #[test]
    fn changing_master_password_requires_the_current_one() {
        let tmp = Tmp::new("changepw-guard");
        let mut v = make(&tmp, "текущий пароль");
        assert_eq!(
            v.change_master_password("не тот", "новый пароль")
                .unwrap_err()
                .code(),
            "bad_master_password"
        );
    }

    #[test]
    fn short_master_password_is_refused() {
        let tmp = Tmp::new("weak");
        assert_eq!(
            Vault::create_with_params(tmp.file(), "1234", fast())
                .unwrap_err()
                .code(),
            "weak_master_password"
        );
    }

    #[test]
    fn update_preserves_history_usage_and_creation_date() {
        let tmp = Tmp::new("update");
        let mut v = make(&tmp, "мастер-пароль");
        let mut e = Entry::new(EntryKind::Password, "Сайт");
        e.set_password("первый".into());
        let id = v.add(e);
        v.mark_used(id).unwrap();

        let mut edited = v.get(id).unwrap().clone();
        edited.password = "второй".into();
        edited.created_at = Utc::now(); // форма прислала мусор — он должен быть отброшен
        edited.usage_count = 0;
        v.update(edited).unwrap();

        let got = v.get(id).unwrap();
        assert_eq!(got.password, "второй");
        assert_eq!(got.history.len(), 1);
        assert_eq!(got.history[0].password, "первый");
        assert_eq!(
            got.usage_count, 1,
            "счётчик обращений не сбрасывается формой"
        );
    }

    #[test]
    fn update_without_password_change_keeps_the_password_date() {
        let tmp = Tmp::new("update-nochange");
        let mut v = make(&tmp, "мастер-пароль");
        let mut e = Entry::new(EntryKind::Password, "Сайт");
        e.set_password("пароль".into());
        let id = v.add(e);
        let when = v.get(id).unwrap().password_modified_at;

        let mut edited = v.get(id).unwrap().clone();
        edited.title = "Сайт (переименован)".into();
        v.update(edited).unwrap();

        assert_eq!(v.get(id).unwrap().password_modified_at, when);
        assert!(v.get(id).unwrap().history.is_empty());
    }

    #[test]
    fn trash_restore_and_purge() {
        let tmp = Tmp::new("trash");
        let mut v = make(&tmp, "мастер-пароль");
        let id = v.add(Entry::new(EntryKind::Note, "Черновик"));

        v.trash(id).unwrap();
        assert_eq!(v.counts().total, 0);
        assert_eq!(v.counts().trash, 1);
        assert_eq!(
            v.search("Черновик", 10).len(),
            0,
            "корзина не попадает в поиск"
        );

        v.restore(id).unwrap();
        assert_eq!(v.counts().total, 1);

        v.trash(id).unwrap();
        v.empty_trash();
        assert!(v.get(id).is_err());
    }

    #[test]
    fn expired_trash_is_purged_on_open() {
        let tmp = Tmp::new("trash-expiry");
        let mut v = make(&tmp, "мастер-пароль");
        let fresh = v.add(Entry::new(EntryKind::Note, "Свежая"));
        let stale = v.add(Entry::new(EntryKind::Note, "Древняя"));
        v.trash(fresh).unwrap();
        v.trash(stale).unwrap();
        v.get_mut(stale).unwrap().deleted_at =
            Some(Utc::now() - Duration::days(TRASH_RETENTION_DAYS + 1));
        v.save().unwrap();
        drop(v);

        let v2 = Vault::open(tmp.file(), "мастер-пароль").unwrap();
        assert!(v2.get(fresh).is_ok());
        assert!(v2.get(stale).is_err());
    }

    #[test]
    fn removing_a_folder_orphans_its_entries_instead_of_deleting_them() {
        let tmp = Tmp::new("folders");
        let mut v = make(&tmp, "мастер-пароль");
        let f = v.add_folder("Работа");
        let mut e = Entry::new(EntryKind::Password, "Запись");
        e.folder = Some(f);
        let id = v.add(e);

        v.remove_folder(f).unwrap();
        assert!(v.get(id).is_ok());
        assert_eq!(v.get(id).unwrap().folder, None);
    }

    #[test]
    fn search_orders_by_rank_then_usage() {
        let tmp = Tmp::new("search");
        let mut v = make(&tmp, "мастер-пароль");
        let exact = v.add(Entry::new(EntryKind::Password, "GitHub"));
        let contains = v.add(Entry::new(EntryKind::Password, "Мой GitHub"));
        v.mark_used(contains).unwrap();
        v.mark_used(contains).unwrap();

        assert_eq!(v.search("gith", 10), vec![exact, contains]);
    }

    #[test]
    fn recent_respects_the_quick_access_flag() {
        let tmp = Tmp::new("recent");
        let mut v = make(&tmp, "мастер-пароль");
        let shown = v.add(Entry::new(EntryKind::Password, "Видна"));
        let mut hidden_e = Entry::new(EntryKind::Password, "Скрыта");
        hidden_e.quick_access = false;
        let hidden = v.add(hidden_e);
        v.mark_used(shown).unwrap();
        v.mark_used(hidden).unwrap();

        let r = v.recent(10);
        assert!(r.contains(&shown));
        assert!(!r.contains(&hidden));
    }

    #[test]
    fn counts_split_by_kind() {
        let tmp = Tmp::new("counts");
        let mut v = make(&tmp, "мастер-пароль");
        v.add(Entry::new(EntryKind::Password, "п"));
        v.add(Entry::new(EntryKind::Note, "з"));
        v.add(Entry::new(EntryKind::ApiKey, "к"));
        v.add(Entry::new(EntryKind::Document, "д"));
        let mut fav = Entry::new(EntryKind::Password, "и");
        fav.favorite = true;
        v.add(fav);

        let c = v.counts();
        assert_eq!(
            (
                c.total,
                c.passwords,
                c.notes,
                c.api_keys,
                c.documents,
                c.favorites
            ),
            (5, 2, 1, 1, 1, 1)
        );
    }

    #[test]
    fn saving_rotates_backups_and_keeps_only_the_last_few() {
        let tmp = Tmp::new("backups");
        let mut v = make(&tmp, "мастер-пароль");
        for i in 0..(BACKUP_KEEP + 4) {
            v.add(Entry::new(EntryKind::Note, format!("запись {i}")));
            v.save().unwrap();
            // отметки времени в именах идут посекундно
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let n = std::fs::read_dir(tmp.0.join("backups")).unwrap().count();
        assert!(n <= BACKUP_KEEP, "копий накопилось {n}");
    }

    #[test]
    fn a_foreign_file_is_not_mistaken_for_a_vault() {
        let tmp = Tmp::new("foreign");
        std::fs::write(tmp.file(), b"{\"hello\":\"world\"}").unwrap();
        assert_eq!(
            Vault::open(tmp.file(), "x").unwrap_err().code(),
            "not_a_vault"
        );
    }
}
