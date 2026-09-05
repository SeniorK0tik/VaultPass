//! Модель данных сейфа. Всё, что здесь описано, живёт внутри зашифрованной
//! полезной нагрузки — на диске в открытом виде не остаётся ни названий, ни
//! логинов, ни адресов сайтов.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

/// Четыре типа записей из макета: категории в левой панели экрана 1d.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Password,
    Note,
    ApiKey,
    Document,
}

impl EntryKind {
    pub const ALL: [EntryKind; 4] = [
        EntryKind::Password,
        EntryKind::Note,
        EntryKind::ApiKey,
        EntryKind::Document,
    ];

    /// Подпись категории — та же, что в макете.
    pub fn title_ru(self) -> &'static str {
        match self {
            EntryKind::Password => "Пароли",
            EntryKind::Note => "Заметки",
            EntryKind::ApiKey => "Ключи API",
            EntryKind::Document => "Документы",
        }
    }

    /// Имя иконки Phosphor по умолчанию для записи этого типа.
    pub fn icon(self) -> &'static str {
        match self {
            EntryKind::Password => "key",
            EntryKind::Note => "note",
            EntryKind::ApiKey => "code",
            EntryKind::Document => "identification-card",
        }
    }
}

/// Своё поле записи («Ключ восстановления» в макете 1g).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomField {
    pub id: Uuid,
    pub label: String,
    pub value: String,
    /// Скрывать значение точками и не показывать без явного запроса.
    #[serde(default)]
    pub secret: bool,
}

impl CustomField {
    pub fn new(label: impl Into<String>, value: impl Into<String>, secret: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            value: value.into(),
            secret,
        }
    }
}

impl Drop for CustomField {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// Прошлый пароль. Панель «История пароля» в макете 1g.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordHistoryItem {
    pub password: String,
    pub replaced_at: DateTime<Utc>,
}

impl Drop for PasswordHistoryItem {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

/// Папка («Работа» в макете). Плоский список: вложенность в макете не показана.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// Запись сейфа.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: Uuid,
    pub kind: EntryKind,
    pub title: String,

    #[serde(default)]
    pub username: String,
    /// Почта учётной записи. Секретом не считается: её видно в списках,
    /// по ней ищут — как по логину.
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub note: String,

    /// Строка `otpauth://…`. В этой версии хранится и переносится как есть,
    /// коды не вычисляются.
    #[serde(default)]
    pub totp: Option<String>,

    #[serde(default)]
    pub custom: Vec<CustomField>,
    #[serde(default)]
    pub folder: Option<Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub favorite: bool,
    /// «Показывать в мини-окне» — флажок из макета 1g.
    #[serde(default = "yes")]
    pub quick_access: bool,
    /// Переопределение иконки Phosphor (`github-logo`, `bank`, …).
    #[serde(default)]
    pub icon: Option<String>,

    #[serde(default)]
    pub history: Vec<PasswordHistoryItem>,

    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    #[serde(default)]
    pub password_modified_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub usage_count: u32,
    /// Срок действия — для ключей API («истекает через 21 день»).
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// Непусто — запись в корзине.
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
}

fn yes() -> bool {
    true
}

impl Entry {
    pub fn new(kind: EntryKind, title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            kind,
            title: title.into(),
            username: String::new(),
            email: String::new(),
            password: String::new(),
            url: String::new(),
            note: String::new(),
            totp: None,
            custom: Vec::new(),
            folder: None,
            tags: Vec::new(),
            favorite: false,
            quick_access: true,
            icon: None,
            history: Vec::new(),
            created_at: now,
            modified_at: now,
            password_modified_at: None,
            last_used_at: None,
            usage_count: 0,
            expires_at: None,
            deleted_at: None,
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// Имя иконки: своё, если задано, иначе угаданное по адресу, иначе — по типу.
    pub fn resolved_icon(&self) -> String {
        if let Some(i) = &self.icon {
            if !i.is_empty() {
                return i.clone();
            }
        }
        if let Some(i) = icon_for_url(&self.url).or_else(|| icon_for_url(&self.title)) {
            return i.to_string();
        }
        self.kind.icon().to_string()
    }

    /// Строка второй строки в списке: то, чем запись опознаётся с одного взгляда.
    pub fn subtitle(&self) -> String {
        if !self.username.is_empty() {
            return self.username.clone();
        }
        if self.kind == EntryKind::Password && !self.email.is_empty() {
            return self.email.clone();
        }
        match self.kind {
            EntryKind::Password => host_of(&self.url),
            EntryKind::Note => "заметка".into(),
            EntryKind::ApiKey => "ключ API".into(),
            EntryKind::Document => "документ".into(),
        }
    }

    /// Переписывает пароль, складывая прежний в историю. История ограничена
    /// десятью значениями: она нужна для «а какой был раньше», а не как архив.
    pub fn set_password(&mut self, new_password: String) {
        if self.password == new_password {
            return;
        }
        if !self.password.is_empty() {
            self.history.insert(
                0,
                PasswordHistoryItem {
                    password: std::mem::take(&mut self.password),
                    replaced_at: Utc::now(),
                },
            );
            self.history.truncate(10);
        }
        self.password = new_password;
        let now = Utc::now();
        self.password_modified_at = Some(now);
        self.modified_at = now;
    }

    /// Совпадение с поисковым запросом. Поиск идёт по названию, логину, адресу,
    /// тегам и меткам своих полей — но никогда по значениям секретов.
    pub fn matches(&self, needle_lower: &str) -> bool {
        if needle_lower.is_empty() {
            return true;
        }
        let hay = [
            self.title.as_str(),
            self.username.as_str(),
            self.email.as_str(),
            self.url.as_str(),
            self.note.as_str(),
        ];
        if hay.iter().any(|h| h.to_lowercase().contains(needle_lower)) {
            return true;
        }
        if self
            .tags
            .iter()
            .any(|t| t.to_lowercase().contains(needle_lower))
        {
            return true;
        }
        self.custom
            .iter()
            .any(|f| f.label.to_lowercase().contains(needle_lower))
    }

    /// Ранг совпадения: чем меньше, тем выше в списке. Точное начало названия
    /// бьёт вхождение в середину, а частота использования — всё остальное.
    pub fn match_rank(&self, needle_lower: &str) -> (u8, std::cmp::Reverse<u32>) {
        let title = self.title.to_lowercase();
        let tier = if needle_lower.is_empty() {
            3
        } else if title == needle_lower {
            0
        } else if title.starts_with(needle_lower) {
            1
        } else if title.contains(needle_lower) {
            2
        } else if self.username.to_lowercase().contains(needle_lower)
            || self.email.to_lowercase().contains(needle_lower)
            || self.url.to_lowercase().contains(needle_lower)
        {
            3
        } else {
            4
        };
        (tier, std::cmp::Reverse(self.usage_count))
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.password.zeroize();
        self.note.zeroize();
        if let Some(t) = self.totp.as_mut() {
            t.zeroize();
        }
    }
}

/// Хост из адреса, без схемы и `www.` — то, что показано под названием в макете.
pub fn host_of(url: &str) -> String {
    let s = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let s = s.split(['/', '?', '#']).next().unwrap_or(s);
    s.trim_start_matches("www.").to_string()
}

/// Иконка Phosphor по узнаваемому домену. Список короткий и намеренно
/// консервативный: всё незнакомое получает иконку своего типа записи.
fn icon_for_url(url: &str) -> Option<&'static str> {
    let h = url.to_lowercase();
    const MAP: &[(&str, &str)] = &[
        ("github", "github-logo"),
        ("gitlab", "gitlab-logo"),
        ("google", "google-logo"),
        ("gmail", "google-logo"),
        ("figma", "figma-logo"),
        ("notion", "notion-logo"),
        ("dropbox", "dropbox-logo"),
        ("twitter", "twitter-logo"),
        ("telegram", "telegram-logo"),
        ("discord", "discord-logo"),
        ("linkedin", "linkedin-logo"),
        ("apple", "apple-logo"),
        ("microsoft", "windows-logo"),
        ("amazon", "amazon-logo"),
        ("aws", "code"),
        ("openai", "code"),
        ("банк", "bank"),
        ("bank", "bank"),
        ("тинькофф", "bank"),
        ("сбер", "bank"),
        ("паспорт", "identification-card"),
    ];
    MAP.iter().find(|(k, _)| h.contains(k)).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_strips_scheme_and_www() {
        assert_eq!(host_of("https://www.github.com/login?x=1"), "github.com");
        assert_eq!(host_of("github.com"), "github.com");
        assert_eq!(host_of(""), "");
    }

    #[test]
    fn set_password_pushes_history_and_dedupes() {
        let mut e = Entry::new(EntryKind::Password, "GitHub");
        e.set_password("первый".into());
        assert!(e.history.is_empty(), "первая установка не создаёт истории");

        e.set_password("второй".into());
        assert_eq!(e.history.len(), 1);
        assert_eq!(e.history[0].password, "первый");

        e.set_password("второй".into());
        assert_eq!(e.history.len(), 1, "повтор того же пароля ничего не меняет");
    }

    #[test]
    fn history_is_capped_at_ten() {
        let mut e = Entry::new(EntryKind::Password, "x");
        for i in 0..20 {
            e.set_password(format!("p{i}"));
        }
        assert_eq!(e.history.len(), 10);
        assert_eq!(
            e.history[0].password, "p18",
            "самый свежий прежний — первым"
        );
    }

    #[test]
    fn search_never_matches_the_secret_itself() {
        let mut e = Entry::new(EntryKind::Password, "GitHub");
        e.password = "k7$Rm2-vQx9Lp!Zt".into();
        assert!(!e.matches("k7$rm2"));
        assert!(e.matches("gith"));
    }

    #[test]
    fn rank_prefers_title_prefix() {
        let a = Entry::new(EntryKind::Password, "GitHub");
        let mut b = Entry::new(EntryKind::Password, "Мой GitHub");
        b.usage_count = 99;
        assert!(a.match_rank("gith") < b.match_rank("gith"));
    }

    #[test]
    fn search_finds_an_entry_by_its_email() {
        let mut e = Entry::new(EntryKind::Password, "Notion");
        e.email = "anna.k@fastmail.com".into();
        assert!(e.matches("fastmail"));
        assert!(!e.matches("gmail"));
    }

    #[test]
    fn subtitle_falls_back_to_email_when_there_is_no_login() {
        let mut e = Entry::new(EntryKind::Password, "Notion");
        e.email = "anna.k@fastmail.com".into();
        e.url = "https://notion.so".into();
        assert_eq!(e.subtitle(), "anna.k@fastmail.com");

        // Логин, если он есть, остаётся главным: почта — только замена.
        e.username = "annakuz".into();
        assert_eq!(e.subtitle(), "annakuz");
    }

    #[test]
    fn icon_falls_back_to_kind() {
        let e = Entry::new(EntryKind::Note, "Просто заметка");
        assert_eq!(e.resolved_icon(), "note");
        let mut g = Entry::new(EntryKind::Password, "GitHub");
        g.url = "https://github.com/login".into();
        assert_eq!(g.resolved_icon(), "github-logo");
    }
}
