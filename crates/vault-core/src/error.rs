use std::fmt;

/// Ошибки хранилища. Ни один вариант не раскрывает содержимое сейфа:
/// при неверном мастер-пароле AEAD не отличает «не тот ключ» от «файл повреждён»,
/// поэтому оба случая приходят как [`Error::BadMasterPassword`] только тогда,
/// когда не сошлась проверка обёртки ключа.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("файл не похож на хранилище «Сейф»")]
    NotAVault,

    #[error("версия формата {0} не поддерживается (нужна 3)")]
    UnsupportedFormat(u32),

    #[error("файл не похож на хранилище сид-фраз «Сейф»")]
    NotASeedVault,

    #[error("версия формата хранилища сид-фраз {0} не поддерживается (нужна 1)")]
    UnsupportedSeedFormat(u32),

    #[error("неверный мастер-пароль")]
    BadMasterPassword,

    #[error("неверный ключ восстановления")]
    BadRecoveryKey,

    #[error("для этого хранилища не создан ключ восстановления")]
    NoRecoveryKey,

    #[error("хранилище повреждено: {0}")]
    Corrupt(String),

    #[error("хранилище заблокировано")]
    Locked,

    #[error("запись {0} не найдена")]
    NoSuchEntry(uuid::Uuid),

    #[error("папка {0} не найдена")]
    NoSuchFolder(uuid::Uuid),

    #[error("сид-фраза {0} не найдена")]
    NoSuchSeedEntry(uuid::Uuid),

    #[error("в сид-фразе {0} слов: их бывает 12, 15, 18, 21 или 24")]
    BadWordCount(usize),

    // Слово, которого нет в списке, называется только номером: сообщение
    // уходит и в интерфейс, и в журнал, а слово сид-фразы — секрет.
    #[error("слово №{0} не из списка BIP39")]
    UnknownWord(usize),

    #[error("фраза не сходится по контрольной сумме — проверьте порядок и написание слов")]
    BadChecksum,

    #[error("хранилище сид-фраз заблокировано")]
    SeedLocked,

    #[error("мастер-пароль слишком короткий: нужно не меньше {0} символов")]
    WeakMasterPassword(usize),

    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("ошибка разбора данных: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("системный генератор случайных чисел недоступен: {0}")]
    Random(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Короткий машинный код — фронтенд по нему выбирает текст и поведение.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotAVault => "not_a_vault",
            Error::UnsupportedFormat(_) => "unsupported_format",
            Error::BadMasterPassword => "bad_master_password",
            Error::BadRecoveryKey => "bad_recovery_key",
            Error::NoRecoveryKey => "no_recovery_key",
            Error::Corrupt(_) => "corrupt",
            Error::Locked => "locked",
            Error::NoSuchEntry(_) => "no_such_entry",
            Error::NoSuchFolder(_) => "no_such_folder",
            Error::NotASeedVault => "not_a_seed_vault",
            Error::UnsupportedSeedFormat(_) => "unsupported_seed_format",
            Error::NoSuchSeedEntry(_) => "no_such_seed_entry",
            Error::BadWordCount(_) => "bad_word_count",
            Error::UnknownWord(_) => "unknown_word",
            Error::BadChecksum => "bad_checksum",
            Error::SeedLocked => "seed_locked",
            Error::WeakMasterPassword(_) => "weak_master_password",
            Error::Io(_) => "io",
            Error::Serde(_) => "serde",
            Error::Random(_) => "random",
            Error::Other(_) => "other",
        }
    }
}

/// Сериализуемое представление ошибки для IPC: `{ code, message }`.
pub struct Wire<'a>(pub &'a Error);

impl fmt::Display for Wire<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Error", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}
