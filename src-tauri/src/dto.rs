//! Формы данных, пересекающие границу IPC.
//!
//! Главное правило слоя: **пароли не попадают в интерфейс сами по себе**.
//! Списки и карточки получают [`EntryView`], где секретов нет вовсе, — а
//! значение уходит наверх только по отдельной команде «показать» или не
//! уходит вообще, если пользователь нажал «копировать» (тогда строка идёт
//! из Rust прямо в буфер обмена, минуя вебвью).
//!
//! Исключение одно — [`EntryDraft`] для формы редактирования: там пароль
//! нужно видеть и править, и это осознанный, явно запрошенный шаг.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vault_core::generator::estimate_strength;
use vault_core::{CustomField, Entry, EntryKind};
use zeroize::Zeroize;

/// Точки вместо значения. Длина фиксированная: настоящая длина пароля —
/// тоже сведения о пароле.
pub const MASK: &str = "••••••••••••••••";

#[derive(Debug, Clone, Serialize)]
pub struct Strength {
    pub bits: f64,
    pub label: &'static str,
    pub fill: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CustomFieldView {
    pub id: Uuid,
    pub label: String,
    /// Для секретных полей — точки, для обычных — значение.
    pub value: String,
    pub secret: bool,
}

/// Запись в том виде, в каком её видит интерфейс.
#[derive(Debug, Clone, Serialize)]
pub struct EntryView {
    pub id: Uuid,
    pub kind: EntryKind,
    pub kind_title: &'static str,
    pub title: String,
    pub subtitle: String,
    pub username: String,
    pub email: String,
    pub url: String,
    pub host: String,
    pub note: String,
    pub icon: String,
    pub tags: Vec<String>,
    pub folder: Option<Uuid>,
    pub favorite: bool,
    pub quick_access: bool,

    pub has_password: bool,
    pub has_totp: bool,
    /// Всегда маска — настоящее значение приходит только по `reveal_field`.
    pub password_masked: &'static str,
    pub strength: Option<Strength>,

    pub custom: Vec<CustomFieldView>,
    pub history: Vec<HistoryView>,

    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub password_modified_at: Option<DateTime<Utc>>,
    pub password_age_days: Option<i64>,
    pub expires_at: Option<DateTime<Utc>>,
    pub expires_in_days: Option<i64>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub usage_count: u32,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryView {
    /// Прежние пароли не показываются никогда — только точки и дата.
    pub masked: String,
    pub replaced_at: DateTime<Utc>,
}

impl EntryView {
    pub fn of(e: &Entry) -> Self {
        let now = Utc::now();
        Self {
            id: e.id,
            kind: e.kind,
            kind_title: e.kind.title_ru(),
            title: e.title.clone(),
            subtitle: e.subtitle(),
            username: e.username.clone(),
            email: e.email.clone(),
            url: e.url.clone(),
            host: vault_core::model::host_of(&e.url),
            note: e.note.clone(),
            icon: e.resolved_icon(),
            tags: e.tags.clone(),
            folder: e.folder,
            favorite: e.favorite,
            quick_access: e.quick_access,

            has_password: !e.password.is_empty(),
            has_totp: e.totp.as_ref().is_some_and(|t| !t.is_empty()),
            password_masked: MASK,
            strength: (!e.password.is_empty()).then(|| {
                let s = estimate_strength(&e.password);
                Strength {
                    bits: s.entropy_bits,
                    label: s.label,
                    fill: s.fill,
                }
            }),

            custom: e
                .custom
                .iter()
                .map(|f| CustomFieldView {
                    id: f.id,
                    label: f.label.clone(),
                    value: if f.secret {
                        MASK.into()
                    } else {
                        f.value.clone()
                    },
                    secret: f.secret,
                })
                .collect(),
            history: e
                .history
                .iter()
                .map(|h| HistoryView {
                    // Длина показывается приблизительно — по десятку точек на
                    // запись, как в макете, а не символ в символ.
                    masked: "•".repeat(h.password.chars().count().clamp(8, 14)),
                    replaced_at: h.replaced_at,
                })
                .collect(),

            created_at: e.created_at,
            modified_at: e.modified_at,
            password_modified_at: e.password_modified_at,
            password_age_days: e.password_modified_at.map(|d| (now - d).num_days()),
            expires_at: e.expires_at,
            expires_in_days: e.expires_at.map(|d| (d - now).num_days()),
            last_used_at: e.last_used_at,
            usage_count: e.usage_count,
            deleted: e.is_deleted(),
        }
    }
}

/// Какое поле записи запрашивают «показать» или «скопировать».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Username,
    Email,
    Password,
    Url,
    Note,
    Totp,
}

/// Черновик записи: то, что присылает форма редактирования (1g), и то, что
/// она получает при открытии. Здесь пароль настоящий.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryDraft {
    /// `None` — создаётся новая запись.
    pub id: Option<Uuid>,
    pub kind: EntryKind,
    pub title: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub totp: Option<String>,
    #[serde(default)]
    pub custom: Vec<DraftField>,
    #[serde(default)]
    pub folder: Option<Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub quick_access: bool,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftField {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub secret: bool,
}

impl Drop for EntryDraft {
    fn drop(&mut self) {
        self.password.zeroize();
        self.note.zeroize();
    }
}

impl Drop for DraftField {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl EntryDraft {
    /// Пустой черновик для новой записи.
    ///
    /// Отдельный конструктор, а не `of(&Entry::new(...))`: у свежей `Entry`
    /// уже есть свой UUID, и [`EntryDraft::of`] его сохранит — а `save_draft`
    /// по непустому `id` решит, что запись существующая, и попробует её
    /// обновить. `None` здесь означает «этой записи ещё нет».
    pub fn blank(kind: EntryKind) -> Self {
        let mut d = Self::of(&Entry::new(kind, String::new()));
        d.id = None;
        d.quick_access = true;
        d
    }

    pub fn of(e: &Entry) -> Self {
        Self {
            id: Some(e.id),
            kind: e.kind,
            title: e.title.clone(),
            username: e.username.clone(),
            email: e.email.clone(),
            password: e.password.clone(),
            url: e.url.clone(),
            note: e.note.clone(),
            totp: e.totp.clone(),
            custom: e
                .custom
                .iter()
                .map(|f| DraftField {
                    label: f.label.clone(),
                    value: f.value.clone(),
                    secret: f.secret,
                })
                .collect(),
            folder: e.folder,
            tags: e.tags.clone(),
            favorite: e.favorite,
            quick_access: e.quick_access,
            icon: e.icon.clone(),
            expires_at: e.expires_at,
        }
    }

    /// Собирает запись из черновика. Поля, которых форма не знает (история
    /// пароля, счётчик обращений, даты создания), проставит `Vault::update`.
    pub fn into_entry(mut self) -> Entry {
        let mut e = Entry::new(self.kind, std::mem::take(&mut self.title));
        if let Some(id) = self.id {
            e.id = id;
        }
        e.username = std::mem::take(&mut self.username);
        // Почта — принадлежность паролей: у остальных типов форма её не
        // показывает, и оставлять невидимое значение (которое всё равно
        // найдёт поиск) было бы хуже, чем не сохранить его вовсе.
        e.email = if self.kind == EntryKind::Password {
            std::mem::take(&mut self.email).trim().to_string()
        } else {
            String::new()
        };
        e.password = std::mem::take(&mut self.password);
        e.url = std::mem::take(&mut self.url);
        e.note = std::mem::take(&mut self.note);
        e.totp = self.totp.take().filter(|t| !t.trim().is_empty());
        e.custom = self
            .custom
            .iter_mut()
            .filter(|f| !f.label.trim().is_empty())
            .map(|f| {
                CustomField::new(
                    std::mem::take(&mut f.label),
                    std::mem::take(&mut f.value),
                    f.secret,
                )
            })
            .collect();
        e.folder = self.folder;
        e.tags = std::mem::take(&mut self.tags);
        e.favorite = self.favorite;
        e.quick_access = self.quick_access;
        e.icon = self.icon.take().filter(|i| !i.is_empty());
        e.expires_at = self.expires_at;
        e
    }
}

/// Состояние сейфа для экрана разблокировки (1f) и заголовков окон.
#[derive(Debug, Clone, Serialize)]
pub struct VaultStatus {
    /// Файл хранилища существует — значит, показывать «Разблокировать»,
    /// а не «Создать сейф».
    pub exists: bool,
    pub unlocked: bool,
    pub path: String,
    pub entry_count: Option<usize>,
    pub has_recovery_key: bool,
    pub format_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::EntryKind;

    #[test]
    fn view_never_carries_the_password() {
        let mut e = Entry::new(EntryKind::Password, "GitHub");
        e.set_password("k7$Rm2-vQx9Lp!Zt".into());
        let json = serde_json::to_string(&EntryView::of(&e)).unwrap();
        assert!(!json.contains("k7$Rm2"), "пароль просочился в EntryView");
        assert!(json.contains("has_password\":true"));
    }

    #[test]
    fn view_masks_secret_custom_fields_but_not_plain_ones() {
        let mut e = Entry::new(EntryKind::Password, "Сайт");
        e.custom
            .push(CustomField::new("Ключ восстановления", "СЕКРЕТ", true));
        e.custom
            .push(CustomField::new("Отдел", "Бухгалтерия", false));
        let v = EntryView::of(&e);
        assert_eq!(v.custom[0].value, MASK);
        assert_eq!(v.custom[1].value, "Бухгалтерия");
    }

    #[test]
    fn history_shows_dots_not_old_passwords() {
        let mut e = Entry::new(EntryKind::Password, "Сайт");
        e.set_password("старый-пароль".into());
        e.set_password("новый-пароль".into());
        let json = serde_json::to_string(&EntryView::of(&e)).unwrap();
        assert!(!json.contains("старый-пароль"));
        assert_eq!(EntryView::of(&e).history.len(), 1);
    }

    #[test]
    fn draft_roundtrip_keeps_the_identity_and_drops_empty_fields() {
        let mut e = Entry::new(EntryKind::Password, "GitHub");
        e.set_password("пароль".into());
        e.custom.push(CustomField::new("", "мусор", false));
        let back = EntryDraft::of(&e).into_entry();
        assert_eq!(back.id, e.id);
        assert_eq!(back.password, "пароль");
        assert!(back.custom.is_empty(), "поле без названия не сохраняется");
    }

    #[test]
    fn a_blank_draft_has_no_id_so_saving_it_creates_a_record() {
        // Регрессия: `of(&Entry::new(..))` возвращал id уже созданной в памяти
        // записи, и сохранение новой записи уходило в ветку обновления —
        // то есть создать запись было нельзя вообще.
        let d = EntryDraft::blank(EntryKind::Password);
        assert_eq!(d.id, None);
        assert!(d.quick_access);
        assert!(d.title.is_empty());
    }

    #[test]
    fn draft_keeps_the_email_of_a_password_and_drops_it_elsewhere() {
        let mut d = EntryDraft::blank(EntryKind::Password);
        d.email = "  anna.k@fastmail.com  ".into();
        assert_eq!(d.clone().into_entry().email, "anna.k@fastmail.com");

        // Форма показывает почту только у паролей: при другом типе значение
        // не должно тихо остаться в записи.
        d.kind = EntryKind::Note;
        assert!(d.into_entry().email.is_empty());
    }

    #[test]
    fn draft_normalises_a_blank_totp_to_none() {
        let mut d = EntryDraft::of(&Entry::new(EntryKind::Password, "x"));
        d.totp = Some("   ".into());
        assert_eq!(d.into_entry().totp, None);
    }

    /// Повторяет то, что делает команда `save_draft`, включая её развилку
    /// «новая или существующая». Раньше здесь ломалось: черновик приходил с
    /// заполненным `id`, ветка выбиралась неверно, и `update` не находил
    /// записи, которой ещё не было.
    #[test]
    fn a_new_entry_travels_from_a_blank_draft_into_the_vault() {
        use vault_core::crypto::KdfParams;
        use vault_core::Vault;

        let dir = std::env::temp_dir().join(format!("seif-dto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("seif.vault");

        let mut vault = Vault::create_with_params(
            &path,
            "мастер-пароль",
            KdfParams {
                m_cost: 1024,
                t_cost: 1,
                p_cost: 1,
            },
        )
        .unwrap();

        let mut draft = EntryDraft::blank(EntryKind::Password);
        draft.title = "GitHub".into();
        draft.username = "annakuz".into();
        draft.password = "k7$Rm2-vQx9Lp!Zt".into();

        // Развилка команды, дословно.
        let is_new = draft.id.is_none();
        assert!(is_new, "черновик новой записи не должен нести id");

        let entry = draft.into_entry();
        let id = entry.id;
        vault.add(entry);

        let saved = vault.get(id).unwrap();
        assert_eq!(saved.title, "GitHub");
        assert_eq!(saved.password, "k7$Rm2-vQx9Lp!Zt");

        // А правка той же записи должна пойти уже через update и пройти.
        let mut again = EntryDraft::of(vault.get(id).unwrap());
        again.title = "GitHub (работа)".into();
        assert!(again.id.is_some());
        vault.update(again.into_entry()).unwrap();
        assert_eq!(vault.get(id).unwrap().title, "GitHub (работа)");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strength_is_absent_for_entries_without_a_password() {
        let e = Entry::new(EntryKind::Note, "Заметка");
        assert!(EntryView::of(&e).strength.is_none());
    }
}
