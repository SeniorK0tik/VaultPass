//! Команды IPC — всё, что интерфейс может попросить у ядра.
//!
//! Через эту границу секреты не ходят. Список записей получает [`EntryView`]
//! без паролей; «скопировать» кладёт значение в буфер прямо из Rust, так что
//! в вебвью оно не попадает вовсе; «показать» отдаёт строку наверх, но только
//! по отдельному запросу и по одному полю за раз.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use uuid::Uuid;
use vault_core::audit::{audit, AuditReport};
use vault_core::generator::{estimate_strength, generate, GenOptions};
use vault_core::model::EntryKind;
use vault_core::{Counts, Entry, Vault};

use crate::dto::{EntryDraft, EntryView, Field, Strength, VaultStatus};
use crate::settings::Settings;
use crate::state::AppState;
use crate::{autofill, clipboard, windows};

// ─── ошибки ──────────────────────────────────────────────────────────────────

/// Ошибка в том виде, в каком её видит интерфейс: машинный код для ветвления
/// и готовый русский текст для показа.
#[derive(Debug, Serialize)]
pub struct CmdError {
    pub code: String,
    pub message: String,
}

impl CmdError {
    /// Единственная точка, где рождается ошибка IPC, — поэтому запись в
    /// журнал стоит здесь. Так ни один отказ не пропадёт молча, даже если
    /// интерфейс решит его не показывать.
    ///
    /// В сообщение не попадает содержимое сейфа: тексты ошибок ядра его
    /// не несут, а значений полей здесь нет вовсе.
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        // `locked` — не поломка, а обычное состояние: интерфейс спрашивает
        // данные у закрытого сейфа при каждой автоблокировке.
        if code == "locked" {
            log::debug!("отказ [{code}]: {message}");
        } else {
            log::warn!("отказ [{code}]: {message}");
        }
        Self {
            code: code.into(),
            message,
        }
    }
}

impl From<vault_core::Error> for CmdError {
    fn from(e: vault_core::Error) -> Self {
        Self::new(e.code(), e.to_string())
    }
}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> Self {
        Self::new("window", e.to_string())
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}

impl From<String> for CmdError {
    fn from(e: String) -> Self {
        Self::new("other", e)
    }
}

type R<T> = Result<T, CmdError>;
type St<'a> = State<'a, Arc<AppState>>;

// ─── состояние сейфа ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn status(state: St<'_>) -> VaultStatus {
    let path = state.settings().vault_path();
    let unlocked = state.is_unlocked();
    VaultStatus {
        exists: path.exists(),
        unlocked,
        path: path.display().to_string(),
        entry_count: state.with_vault(|v| Ok(v.active().count())).ok(),
        has_recovery_key: state
            .with_vault(|v| Ok(v.has_recovery_key()))
            .unwrap_or(false),
        format_version: vault_core::FORMAT_VERSION,
    }
}

/// Первый запуск: создаёт файл хранилища и сразу его открывает.
#[tauri::command]
pub fn create_vault<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    master_password: String,
) -> R<VaultStatus> {
    let path = state.settings().vault_path();
    if path.exists() {
        return Err(CmdError::new(
            "already_exists",
            "Хранилище по этому пути уже есть — его нужно разблокировать, а не создавать заново.",
        ));
    }
    state.set_vault(Vault::create(path, &master_password)?);
    log::info!("создано новое хранилище");
    let _ = app.emit("vault-unlocked", ());
    Ok(status(state))
}

#[tauri::command]
pub fn unlock<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    master_password: String,
) -> R<VaultStatus> {
    let path = state.settings().vault_path();
    state.set_vault(Vault::open(path, &master_password)?);
    log::info!("сейф разблокирован мастер-паролем");
    let _ = app.emit("vault-unlocked", ());
    Ok(status(state))
}

#[tauri::command]
pub fn unlock_with_recovery<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    recovery_key: String,
) -> R<VaultStatus> {
    let path = state.settings().vault_path();
    state.set_vault(Vault::open_with_recovery(path, &recovery_key)?);
    log::info!("сейф разблокирован ключом восстановления");
    let _ = app.emit("vault-unlocked", ());
    Ok(status(state))
}

#[tauri::command]
pub fn lock_vault<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>) -> R<()> {
    // Сначала раздел сид-фраз — и с событием: `AppState::lock_vault` закроет
    // его и сам, но молча, а окнам нужно знать, что показанной фразы больше
    // нет за чем стоять.
    crate::lock_seed_section(&app);
    if state.lock_vault() {
        log::info!("сейф закрыт");
        windows::hide_all_but_main(&app);
        let _ = app.emit("vault-locked", ());
    }
    Ok(())
}

#[tauri::command]
pub fn change_master_password(state: St<'_>, current: String, new: String) -> R<()> {
    state.with_vault_mut(|v| v.change_master_password(&current, &new))?;
    Ok(())
}

/// Создаёт ключ восстановления и возвращает его — единственный раз за всю
/// жизнь ключа. В хранилище остаётся только обёртка, показать ключ ещё раз
/// нельзя, и интерфейс об этом предупреждает.
#[tauri::command]
pub fn create_recovery_key(state: St<'_>) -> R<String> {
    Ok(state.with_vault_mut(|v| v.create_recovery_key())?)
}

// ─── чтение записей ──────────────────────────────────────────────────────────

/// Что показывать в среднем столбце: разделы левой панели макета 1d.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Scope {
    #[default]
    All,
    Kind {
        kind: EntryKind,
    },
    Favorites,
    Folder {
        id: Uuid,
    },
    Tag {
        name: String,
    },
    Trash,
}

fn in_scope(e: &Entry, scope: &Scope) -> bool {
    match scope {
        Scope::Trash => e.is_deleted(),
        _ if e.is_deleted() => false,
        Scope::All => true,
        Scope::Kind { kind } => e.kind == *kind,
        Scope::Favorites => e.favorite,
        Scope::Folder { id } => e.folder == Some(*id),
        Scope::Tag { name } => e.tags.iter().any(|t| t == name),
    }
}

/// Порядок списка. «По изменению» — умолчание, как в макете.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Modified,
    Title,
    Used,
    Created,
}

#[tauri::command]
pub fn list_entries(
    state: St<'_>,
    scope: Option<Scope>,
    query: Option<String>,
    sort: Option<Sort>,
) -> R<Vec<EntryView>> {
    let scope = scope.unwrap_or_default();
    let sort = sort.unwrap_or_default();
    let needle = query.unwrap_or_default().trim().to_lowercase();

    Ok(state.with_vault(|v| {
        let mut hits: Vec<&Entry> = v
            .entries()
            .iter()
            .filter(|e| in_scope(e, &scope) && e.matches(&needle))
            .collect();

        if needle.is_empty() {
            match sort {
                Sort::Modified => hits.sort_by(|a, b| b.modified_at.cmp(&a.modified_at)),
                Sort::Created => hits.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
                Sort::Used => hits.sort_by(|a, b| {
                    b.usage_count
                        .cmp(&a.usage_count)
                        .then_with(|| b.last_used_at.cmp(&a.last_used_at))
                }),
                Sort::Title => hits.sort_by_key(|e| e.title.to_lowercase()),
            }
        } else {
            // При непустом запросе порядок задаёт качество совпадения:
            // выбранная сортировка тут только мешала бы.
            hits.sort_by_key(|e| e.match_rank(&needle));
        }

        Ok(hits.iter().map(|e| EntryView::of(e)).collect())
    })?)
}

#[tauri::command]
pub fn counts(state: St<'_>) -> R<Counts> {
    Ok(state.with_vault(|v| Ok(v.counts()))?)
}

#[tauri::command]
pub fn get_entry(state: St<'_>, id: Uuid) -> R<EntryView> {
    Ok(state.with_vault(|v| Ok(EntryView::of(v.get(id)?)))?)
}

/// Недавние записи для мини-окна у трея (1c).
#[tauri::command]
pub fn recent(state: St<'_>, limit: Option<usize>) -> R<Vec<EntryView>> {
    let limit = limit.unwrap_or(6).min(50);
    Ok(state.with_vault(|v| {
        Ok(v.recent(limit)
            .into_iter()
            .filter_map(|id| v.get(id).ok())
            .map(EntryView::of)
            .collect())
    })?)
}

/// Поиск для палитры (1a/1b).
#[tauri::command]
pub fn search(state: St<'_>, query: String, limit: Option<usize>) -> R<Vec<EntryView>> {
    let limit = limit.unwrap_or(8).min(50);
    Ok(state.with_vault(|v| {
        Ok(v.search(&query, limit)
            .into_iter()
            .filter_map(|id| v.get(id).ok())
            .map(EntryView::of)
            .collect())
    })?)
}

#[derive(Serialize)]
pub struct FolderView {
    pub id: Uuid,
    pub name: String,
    pub count: usize,
}

#[tauri::command]
pub fn folders(state: St<'_>) -> R<Vec<FolderView>> {
    Ok(state.with_vault(|v| {
        Ok(v.folders()
            .iter()
            .map(|f| FolderView {
                id: f.id,
                name: f.name.clone(),
                count: v.active().filter(|e| e.folder == Some(f.id)).count(),
            })
            .collect())
    })?)
}

/// Все теги с числом записей — правая колонка формы редактирования
/// подсказывает уже существующие.
#[tauri::command]
pub fn tags(state: St<'_>) -> R<Vec<(String, usize)>> {
    Ok(state.with_vault(|v| {
        let mut map: std::collections::BTreeMap<String, usize> = Default::default();
        for e in v.active() {
            for t in &e.tags {
                *map.entry(t.clone()).or_insert(0) += 1;
            }
        }
        Ok(map.into_iter().collect())
    })?)
}

#[tauri::command]
pub fn audit_report(state: St<'_>) -> R<AuditReport> {
    Ok(state.with_vault(|v| Ok(audit(v.entries().iter())))?)
}

// ─── правка записей ──────────────────────────────────────────────────────────

/// Отдаёт запись для формы редактирования — вместе с паролем.
/// `None` — форма для новой записи.
#[tauri::command]
pub fn load_draft(state: St<'_>, id: Option<Uuid>) -> R<EntryDraft> {
    match id {
        Some(id) => Ok(state.with_vault(|v| Ok(EntryDraft::of(v.get(id)?)))?),
        None => Ok(EntryDraft::blank(EntryKind::Password)),
    }
}

#[tauri::command]
pub fn save_draft<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, draft: EntryDraft) -> R<Uuid> {
    if draft.title.trim().is_empty() {
        return Err(CmdError::new(
            "empty_title",
            "У записи должно быть название.",
        ));
    }
    let is_new = draft.id.is_none();
    let entry = draft.into_entry();
    let id = entry.id;

    state.with_vault_mut(|v| {
        if is_new {
            v.add(entry);
            Ok(())
        } else {
            v.update(entry)
        }
    })?;

    log::info!(
        "запись {} ({})",
        if is_new {
            "создана"
        } else {
            "изменена"
        },
        id
    );
    let _ = app.emit("entries-changed", id);
    Ok(id)
}

#[tauri::command]
pub fn delete_entry<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, id: Uuid) -> R<()> {
    state.with_vault_mut(|v| v.trash(id))?;
    let _ = app.emit("entries-changed", id);
    Ok(())
}

#[tauri::command]
pub fn restore_entry<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, id: Uuid) -> R<()> {
    state.with_vault_mut(|v| v.restore(id))?;
    let _ = app.emit("entries-changed", id);
    Ok(())
}

#[tauri::command]
pub fn purge_entry<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, id: Uuid) -> R<()> {
    state.with_vault_mut(|v| v.purge(id))?;
    let _ = app.emit("entries-changed", id);
    Ok(())
}

#[tauri::command]
pub fn empty_trash<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>) -> R<()> {
    state.with_vault_mut(|v| {
        v.empty_trash();
        Ok(())
    })?;
    let _ = app.emit("entries-changed", ());
    Ok(())
}

#[tauri::command]
pub fn toggle_favorite<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, id: Uuid) -> R<bool> {
    let now = state.with_vault_mut(|v| {
        let e = v.get_mut(id)?;
        e.favorite = !e.favorite;
        Ok(e.favorite)
    })?;
    let _ = app.emit("entries-changed", id);
    Ok(now)
}

#[tauri::command]
pub fn add_folder<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, name: String) -> R<Uuid> {
    let id = state.with_vault_mut(|v| Ok(v.add_folder(name)))?;
    let _ = app.emit("entries-changed", ());
    Ok(id)
}

#[tauri::command]
pub fn rename_folder<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    name: String,
) -> R<()> {
    state.with_vault_mut(|v| v.rename_folder(id, name))?;
    let _ = app.emit("entries-changed", ());
    Ok(())
}

#[tauri::command]
pub fn remove_folder<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, id: Uuid) -> R<()> {
    state.with_vault_mut(|v| v.remove_folder(id))?;
    let _ = app.emit("entries-changed", ());
    Ok(())
}

// ─── секреты ─────────────────────────────────────────────────────────────────

fn field_value(v: &Vault, id: Uuid, field: Field) -> vault_core::Result<String> {
    let e = v.get(id)?;
    Ok(match field {
        Field::Username => e.username.clone(),
        Field::Email => e.email.clone(),
        Field::Password => e.password.clone(),
        Field::Url => e.url.clone(),
        Field::Note => e.note.clone(),
        Field::Totp => e.totp.clone().unwrap_or_default(),
    })
}

/// Показать одно поле — кнопка с глазом. Значение поднимается в вебвью
/// только здесь и только для запрошенного поля.
#[tauri::command]
pub fn reveal_field(state: St<'_>, id: Uuid, field: Field) -> R<String> {
    Ok(state.with_vault(|v| field_value(v, id, field))?)
}

/// Копировать поле. Значение идёт из Rust прямо в буфер обмена — интерфейс
/// его не видит и, если включена автоочистка, получает только число секунд
/// до стирания.
#[tauri::command]
pub fn copy_field<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    field: Field,
) -> R<Option<u64>> {
    let value = state.with_vault(|v| field_value(v, id, field))?;
    if value.is_empty() {
        return Err(CmdError::new("empty_field", "В этом поле ничего нет."));
    }
    if field == Field::Password {
        state.with_vault_mut(|v| v.mark_used(id))?;
        let _ = app.emit("entries-changed", id);
    }
    clipboard::copy_secret(&app, value).map_err(CmdError::from)
}

#[tauri::command]
pub fn copy_custom_field<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    field_id: Uuid,
) -> R<Option<u64>> {
    let value = state.with_vault(|v| {
        let e = v.get(id)?;
        Ok(e.custom
            .iter()
            .find(|f| f.id == field_id)
            .map(|f| f.value.clone())
            .unwrap_or_default())
    })?;
    if value.is_empty() {
        return Err(CmdError::new("empty_field", "В этом поле ничего нет."));
    }
    clipboard::copy_secret(&app, value).map_err(CmdError::from)
}

#[tauri::command]
pub fn reveal_custom_field(state: St<'_>, id: Uuid, field_id: Uuid) -> R<String> {
    Ok(state.with_vault(|v| {
        let e = v.get(id)?;
        Ok(e.custom
            .iter()
            .find(|f| f.id == field_id)
            .map(|f| f.value.clone())
            .unwrap_or_default())
    })?)
}

/// Копирование произвольной строки — только для сгенерированного пароля,
/// который интерфейс уже держит у себя на экране (1h).
#[tauri::command]
pub fn copy_text<Rt: Runtime>(app: AppHandle<Rt>, text: String) -> R<Option<u64>> {
    if text.is_empty() {
        return Err(CmdError::new("empty_field", "Нечего копировать."));
    }
    clipboard::copy_secret(&app, text).map_err(CmdError::from)
}

/// Автозаполнение: прячет окна «Сейфа» и печатает логин и пароль в то окно,
/// которое станет активным.
#[tauri::command]
pub async fn autofill_entry<Rt: Runtime>(
    app: AppHandle<Rt>,
    id: Uuid,
    submit: Option<bool>,
) -> R<()> {
    // Заимствование состояния держится в блоке: за точкой await оно жило бы
    // через всю паузу на печать, а нужно оно только чтобы прочитать два поля.
    let (login, password) = {
        let state = app.state::<Arc<AppState>>();
        state.with_vault(|v| {
            let e = v.get(id)?;
            // Логин печатается тот, что есть: у части записей вместо него
            // заведена почта, и печатать в форму пустую строку бессмысленно.
            let login = if e.username.is_empty() {
                e.email.clone()
            } else {
                e.username.clone()
            };
            Ok((login, e.password.clone()))
        })?
    };

    windows::hide(&app, windows::QUICK);
    windows::hide(&app, windows::TRAY);

    let submit = submit.unwrap_or(false);
    let result = tauri::async_runtime::spawn_blocking(move || {
        autofill::type_credentials(&login, &password, submit)
    })
    .await
    .map_err(|e| CmdError::new("autofill", e.to_string()))?;

    result.map_err(|e| CmdError::new("autofill", e))?;

    {
        let state = app.state::<Arc<AppState>>();
        state.with_vault_mut(|v| v.mark_used(id))?;
    }
    let _ = app.emit("entries-changed", id);
    Ok(())
}

#[tauri::command]
pub fn open_entry_url(state: St<'_>, id: Uuid) -> R<()> {
    let url = state.with_vault(|v| Ok(v.get(id)?.url.clone()))?;
    let url = url.trim();
    if url.is_empty() {
        return Err(CmdError::new("empty_field", "У записи не указан адрес."));
    }
    // Схема добавляется явно: без неё строка вроде «github.com» на Windows
    // может быть истолкована как путь к файлу.
    let target = if url.contains("://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    if !target.starts_with("https://") && !target.starts_with("http://") {
        return Err(CmdError::new(
            "bad_url",
            "Открывать можно только адреса http и https.",
        ));
    }
    tauri_plugin_opener::open_url(target, None::<&str>)
        .map_err(|e| CmdError::new("open_url", e.to_string()))
}

/// Главное действие быстрого окна: Enter копирует пароль и окно закрывается.
#[tauri::command]
pub fn quick_copy<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, id: Uuid) -> R<Option<u64>> {
    let secs = copy_field(app.clone(), state, id, Field::Password)?;
    windows::hide(&app, windows::QUICK);
    windows::hide(&app, windows::TRAY);
    Ok(secs)
}

// ─── генератор ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct GeneratedView {
    pub value: String,
    pub entropy_bits: f64,
    pub label: &'static str,
    pub fill: f64,
}

#[tauri::command]
pub fn generate_password(options: GenOptions) -> R<GeneratedView> {
    let g = generate(&options)?;
    Ok(GeneratedView {
        value: g.value.clone(),
        entropy_bits: g.entropy_bits,
        label: g.label,
        fill: g.fill,
    })
}

#[tauri::command]
pub fn estimate(password: String) -> Strength {
    let s = estimate_strength(&password);
    Strength {
        bits: s.entropy_bits,
        label: s.label,
        fill: s.fill,
    }
}

// ─── настройки и обслуживание ────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(state: St<'_>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn set_settings<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    settings: Settings,
) -> R<Settings> {
    let before = state.settings();
    state.set_settings(settings)?;
    let after = state.settings();

    if after.hotkey != before.hotkey {
        crate::hotkey::rebind(&app, &before.hotkey, &after.hotkey)
            .map_err(|e| CmdError::new("hotkey", e))?;
    }
    if after.launch_at_startup != before.launch_at_startup {
        crate::set_autostart(&app, after.launch_at_startup)
            .map_err(|e| CmdError::new("autostart", e))?;
    }
    let _ = app.emit("settings-changed", after.clone());
    Ok(after)
}

#[derive(Serialize)]
pub struct BackupView {
    pub name: String,
    pub bytes: u64,
    pub modified: Option<String>,
}

#[tauri::command]
pub fn backups(state: St<'_>) -> R<Vec<BackupView>> {
    let dir = state.settings().backup_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<BackupView> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".vault"))
        .map(|e| {
            let meta = e.metadata().ok();
            BackupView {
                name: e.file_name().to_string_lossy().into_owned(),
                bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta
                    .and_then(|m| m.modified().ok())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
            }
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

#[tauri::command]
pub fn open_backup_dir(state: St<'_>) -> R<()> {
    let dir = state.settings().backup_dir();
    std::fs::create_dir_all(&dir)?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| CmdError::new("open_path", e.to_string()))
}

// ─── журнал ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct LogInfo {
    pub file: String,
    pub dir: String,
    pub exists: bool,
    pub bytes: u64,
}

#[tauri::command]
pub fn log_info<Rt: Runtime>(app: AppHandle<Rt>) -> LogInfo {
    let file = crate::logging::log_file(&app);
    let meta = file.as_ref().and_then(|p| std::fs::metadata(p).ok());
    LogInfo {
        file: file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        dir: crate::logging::log_dir(&app)
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        exists: meta.is_some(),
        bytes: meta.map(|m| m.len()).unwrap_or(0),
    }
}

#[tauri::command]
pub fn open_log_dir<Rt: Runtime>(app: AppHandle<Rt>) -> R<()> {
    let dir = crate::logging::log_dir(&app)
        .ok_or_else(|| CmdError::new("no_log_dir", "Каталог журнала недоступен."))?;
    std::fs::create_dir_all(&dir)?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| CmdError::new("open_path", e.to_string()))
}

/// Последние строки журнала — чтобы приложить их к сообщению об ошибке,
/// не открывая файловый менеджер.
#[tauri::command]
pub fn log_tail<Rt: Runtime>(app: AppHandle<Rt>, lines: Option<usize>) -> R<String> {
    let n = lines.unwrap_or(80).clamp(1, 2000);
    let path = crate::logging::log_file(&app)
        .ok_or_else(|| CmdError::new("no_log_dir", "Каталог журнала недоступен."))?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    Ok(all[all.len().saturating_sub(n)..].join("\n"))
}

/// Приём записи из интерфейса. Через неё в общий журнал попадают ошибки
/// разметки — иначе они остались бы в консоли вебвью, куда пользователь
/// не заглядывает.
#[tauri::command]
pub fn log_ui(level: String, message: String) {
    // Длину ограничиваем: сообщение приходит из вебвью, и раздувать им файл
    // журнала не стоит.
    let msg: String = message.chars().take(4000).collect();
    match level.as_str() {
        "error" => log::error!("[интерфейс] {msg}"),
        "warn" => log::warn!("[интерфейс] {msg}"),
        _ => log::info!("[интерфейс] {msg}"),
    }
}

// ─── окна и активность ───────────────────────────────────────────────────────

// Обе команды асинхронные намеренно: Tauri выполняет такие вне главного
// потока, а создавать окно из главного на Windows нельзя — оно там
// заблокируется насмерть (см. пояснение в `windows.rs`).
#[tauri::command]
pub async fn open_window<Rt: Runtime>(app: AppHandle<Rt>, label: String) -> R<()> {
    windows::show(&app, &label)?;
    Ok(())
}

#[tauri::command]
pub async fn open_editor<Rt: Runtime>(app: AppHandle<Rt>, id: Option<Uuid>) -> R<()> {
    windows::show_with(&app, windows::EDITOR, "open-entry", id)?;
    Ok(())
}

#[tauri::command]
pub fn close_window<Rt: Runtime>(app: AppHandle<Rt>, label: String) -> R<()> {
    // Главное окно не закрывается, а прячется: приложение живёт в трее и
    // должно оставаться готовым к горячей клавише.
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.hide();
    }
    if label == windows::MAIN {
        crate::lock_seed_section(&app);
    }
    Ok(())
}

#[tauri::command]
pub fn minimize_window<Rt: Runtime>(app: AppHandle<Rt>, label: String) -> R<()> {
    if let Some(w) = app.get_webview_window(&label) {
        w.minimize()?;
    }
    if label == windows::MAIN {
        crate::lock_seed_section(&app);
    }
    Ok(())
}

#[tauri::command]
pub fn toggle_maximize<Rt: Runtime>(app: AppHandle<Rt>, label: String) -> R<()> {
    if let Some(w) = app.get_webview_window(&label) {
        if w.is_maximized()? {
            w.unmaximize()?;
        } else {
            w.maximize()?;
        }
    }
    Ok(())
}

/// Отметка активности — сбрасывает часы автоблокировки. Интерфейс шлёт её
/// на нажатия клавиш и движение мыши, но не чаще раза в несколько секунд.
#[tauri::command]
pub fn ping(state: St<'_>) -> u64 {
    state.touch();
    state.settings().autolock_secs.unwrap_or(0)
}

/// Сколько секунд осталось до автоблокировки — строка «блокировка через
/// 8 мин» в левой панели макета 1d.
#[tauri::command]
pub fn lock_countdown(state: St<'_>) -> Option<u64> {
    state
        .settings()
        .autolock_secs
        .map(|limit| limit.saturating_sub(state.idle_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::EntryKind;

    fn entry(kind: EntryKind, title: &str) -> Entry {
        Entry::new(kind, title)
    }

    #[test]
    fn trash_scope_shows_only_deleted_and_others_hide_them() {
        let mut e = entry(EntryKind::Password, "Удалённая");
        e.deleted_at = Some(chrono::Utc::now());

        assert!(in_scope(&e, &Scope::Trash));
        assert!(!in_scope(&e, &Scope::All));
        assert!(!in_scope(
            &e,
            &Scope::Kind {
                kind: EntryKind::Password
            }
        ));
        assert!(!in_scope(&e, &Scope::Favorites));
    }

    #[test]
    fn kind_scope_filters_by_kind() {
        let note = entry(EntryKind::Note, "Заметка");
        assert!(in_scope(
            &note,
            &Scope::Kind {
                kind: EntryKind::Note
            }
        ));
        assert!(!in_scope(
            &note,
            &Scope::Kind {
                kind: EntryKind::ApiKey
            }
        ));
    }

    #[test]
    fn tag_scope_needs_an_exact_tag() {
        let mut e = entry(EntryKind::Password, "Запись");
        e.tags = vec!["работа".into()];
        assert!(in_scope(
            &e,
            &Scope::Tag {
                name: "работа".into()
            }
        ));
        assert!(!in_scope(
            &e,
            &Scope::Tag {
                name: "раб".into()
            }
        ));
    }

    #[test]
    fn error_conversion_keeps_the_machine_code() {
        let e: CmdError = vault_core::Error::Locked.into();
        assert_eq!(e.code, "locked");
        assert!(!e.message.is_empty());
    }
}
