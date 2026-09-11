//! Формы данных раздела сид-фраз.
//!
//! Правило здесь строже, чем у паролей: **слова не пересекают границу IPC
//! сами по себе**. У обычной записи есть `EntryDraft`, где пароль приходит в
//! форму редактирования; у сид-фразы такого нет вовсе — фраза поднимается
//! наверх только командой `seed_reveal`, по одному явному действию
//! пользователя, и в интерфейсе живёт секунды.
//!
//! Команды, которая положила бы сид-фразу в буфер обмена, в этом модуле и в
//! `seed_commands.rs` нет — не выключена, а не существует.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vault_core::seed::SeedEntry;
use zeroize::Zeroize;

/// Запись в том виде, в каком её видит интерфейс: всё, кроме самой фразы.
#[derive(Debug, Clone, Serialize)]
pub struct SeedEntryView {
    pub id: Uuid,
    pub title: String,
    pub wallet: String,
    pub network: String,
    pub derivation: String,
    pub note: String,

    /// Сколько в фразе слов. Сами слова — нет.
    pub word_count: usize,
    pub has_passphrase: bool,
    pub standard: bool,

    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub last_viewed_at: Option<DateTime<Utc>>,
    pub view_count: u32,
}

impl SeedEntryView {
    pub fn of(e: &SeedEntry) -> Self {
        Self {
            id: e.id,
            title: e.title.clone(),
            wallet: e.wallet.clone(),
            network: e.network.clone(),
            derivation: e.derivation.clone(),
            note: e.note.clone(),
            word_count: e.word_count(),
            has_passphrase: e.has_passphrase(),
            standard: e.standard,
            created_at: e.created_at,
            modified_at: e.modified_at,
            last_viewed_at: e.last_viewed_at,
            view_count: e.view_count,
        }
    }
}

/// Новая запись: сведения и фраза разом. Единственная форма, в которой слова
/// идут в ядро, — и она живёт ровно один вызов.
#[derive(Debug, Clone, Deserialize)]
pub struct SeedDraft {
    pub title: String,
    #[serde(default)]
    pub wallet: String,
    #[serde(default)]
    pub derivation: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub note: String,
    pub words: Vec<String>,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default = "yes")]
    pub standard: bool,
}

fn yes() -> bool {
    true
}

impl Drop for SeedDraft {
    fn drop(&mut self) {
        for w in &mut self.words {
            w.zeroize();
        }
        self.passphrase.zeroize();
        self.note.zeroize();
    }
}

impl SeedDraft {
    /// Собирает запись ядра. Слова забираются из черновика, а не копируются:
    /// лишней копии фразы в памяти быть не должно.
    pub fn into_entry(mut self) -> SeedEntry {
        let mut e = SeedEntry::new(
            std::mem::take(&mut self.title),
            std::mem::take(&mut self.words),
        );
        e.wallet = std::mem::take(&mut self.wallet);
        e.derivation = std::mem::take(&mut self.derivation);
        e.network = std::mem::take(&mut self.network);
        e.note = std::mem::take(&mut self.note);
        e.passphrase = std::mem::take(&mut self.passphrase);
        e.standard = self.standard;
        e
    }
}

/// Правка сведений: всё, кроме фразы.
#[derive(Debug, Clone, Deserialize)]
pub struct SeedDetails {
    pub title: String,
    #[serde(default)]
    pub wallet: String,
    #[serde(default)]
    pub derivation: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub note: String,
}

impl Drop for SeedDetails {
    fn drop(&mut self) {
        self.note.zeroize();
    }
}

/// Ответ на «показать фразу». Живёт в интерфейсе считанные секунды: столько,
/// сколько сказано в `hide_after_secs`.
#[derive(Debug, Serialize)]
pub struct RevealedPhrase {
    pub words: Vec<String>,
    pub hide_after_secs: u64,
}

impl Drop for RevealedPhrase {
    fn drop(&mut self) {
        for w in &mut self.words {
            w.zeroize();
        }
    }
}

/// Состояние раздела для экрана подключения и разблокировки.
#[derive(Debug, Clone, Serialize)]
pub struct SeedStatus {
    /// Выбран ли файл. `false` — раздел показывает экран подключения.
    pub configured: bool,
    /// Существует ли выбранный файл на диске.
    pub exists: bool,
    pub unlocked: bool,
    pub path: String,
    /// Число записей — только при открытом хранилище.
    pub entry_count: Option<usize>,
    pub format_version: u32,

    // Настройки, которые нужны разделу на каждом шаге, — чтобы интерфейс не
    // ходил за ними отдельным вызовом.
    pub require_password_on_reveal: bool,
    pub hide_after_secs: u64,
    pub autolock_secs: u64,
    pub min_password_len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    const PHRASE: &str =
        "letter advice cage absurd amount doctor acoustic avoid letter advice cage above";

    #[test]
    fn the_view_never_carries_the_phrase() {
        let mut e = SeedEntry::new("Ledger основной", words(PHRASE));
        e.passphrase = "двадцать-пятое".into();
        let json = serde_json::to_string(&SeedEntryView::of(&e)).unwrap();

        for needle in ["letter", "absurd", "acoustic", "двадцать-пятое"] {
            assert!(
                !json.contains(needle),
                "«{needle}» просочилось в SeedEntryView"
            );
        }
        assert!(json.contains("\"word_count\":12"));
        assert!(json.contains("\"has_passphrase\":true"));
    }

    #[test]
    fn a_draft_becomes_an_entry_without_copying_the_words() {
        let d = SeedDraft {
            title: "Ledger".into(),
            wallet: "Nano S".into(),
            derivation: "m/44'/0'/0'".into(),
            network: "BTC".into(),
            note: "в сейфе".into(),
            words: words(PHRASE),
            passphrase: "слово".into(),
            standard: true,
        };
        let e = d.into_entry();
        assert_eq!(e.title, "Ledger");
        assert_eq!(e.words, words(PHRASE));
        assert!(e.has_passphrase());
        assert!(e.standard);
    }

    /// У черновика нет поля `id`: править существующую фразу «заодно» нельзя,
    /// для замены есть отдельная команда. Иначе форма создания стала бы вторым
    /// местом, где фраза лежит открытым текстом.
    #[test]
    fn a_draft_cannot_address_an_existing_entry() {
        let json =
            r#"{"title":"Ledger","words":["a"],"id":"00000000-0000-0000-0000-000000000000"}"#;
        let d: SeedDraft = serde_json::from_str(json).unwrap();
        assert_eq!(d.title, "Ledger");
        // Поле `id` просто не существует в форме — присланное игнорируется.
        let e = d.into_entry();
        assert_ne!(e.id.to_string(), "00000000-0000-0000-0000-000000000000");
    }
}
