//! Команды раздела сид-фраз — вторая, более узкая граница IPC.
//!
//! Чего здесь нет и не будет:
//!
//! * команды, кладущей сид-фразу в буфер обмена, — «скопировать» для этого
//!   раздела не существует ни в каком виде;
//! * автозаполнения, открытия ссылок и участия в поиске, палитре, мини-окне
//!   и аудите — сид-волт не виден ниоткуда, кроме своего раздела;
//! * формы редактирования, которая отдавала бы фразу наверх «на всякий
//!   случай»: сведения правятся отдельно, фраза заменяется целиком.
//!
//! Наверх фраза уходит ровно одной командой — [`seed_reveal`], по явному
//! действию пользователя и (по умолчанию) с повторным вводом мастер-пароля.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;
use vault_core::bip39;
use vault_core::seed::{self, SeedVault, MIN_SEED_PASSWORD_LEN, SEED_FORMAT_VERSION};
use zeroize::Zeroize;

use crate::commands::CmdError;
use crate::seed_dto::{RevealedPhrase, SeedDetails, SeedDraft, SeedEntryView, SeedStatus};
use crate::state::AppState;

type R<T> = Result<T, CmdError>;
type St<'a> = State<'a, Arc<AppState>>;

/// После стольких промахов подряд раздел закрывается сам.
const MAX_FAILED_REVEALS: u32 = 5;

// ─── состояние раздела ───────────────────────────────────────────────────────

#[tauri::command]
pub fn seed_status(state: St<'_>) -> SeedStatus {
    let s = state.settings();
    let path = s.seed_vault_path.clone();
    SeedStatus {
        configured: path.is_some(),
        exists: path.as_ref().map(|p| p.exists()).unwrap_or(false),
        unlocked: state.is_seed_unlocked(),
        path: path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        entry_count: state.with_seed(|v| Ok(v.len())).ok(),
        format_version: SEED_FORMAT_VERSION,
        require_password_on_reveal: s.seed_require_password_on_reveal,
        hide_after_secs: s.seed_hide_after_secs,
        autolock_secs: s.seed_autolock_secs,
        min_password_len: MIN_SEED_PASSWORD_LEN,
    }
}

/// Сохраняет выбранный путь и закрывает то, что было открыто раньше:
/// переключение файла не должно оставлять в памяти ключ от предыдущего.
fn remember_path<Rt: Runtime>(
    app: &AppHandle<Rt>,
    state: &AppState,
    path: Option<PathBuf>,
) -> R<()> {
    state.lock_seed();
    let mut s = state.settings();
    s.seed_vault_path = path;
    state.set_settings(s)?;
    let _ = app.emit("seed-changed", ());
    Ok(())
}

#[derive(Serialize)]
pub struct PickedFile {
    pub path: String,
}

/// Выбор файла системным диалогом. `mode` — «create» или «open».
///
/// Команда асинхронная: диалог блокирует поток, на котором его открыли, а
/// делать это на главном нельзя (там крутится цикл событий — см. пояснение
/// в `windows.rs`).
#[tauri::command]
pub async fn seed_pick_file<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    mode: String,
) -> R<Option<PickedFile>> {
    let creating = mode == "create";
    let dialog = app
        .dialog()
        .file()
        .set_title(if creating {
            "Новый файл с сид-фразами"
        } else {
            "Подключить файл с сид-фразами"
        })
        .add_filter("Хранилище сид-фраз", &["seed"]);

    let picked = if creating {
        dialog.set_file_name("wallets.seed").blocking_save_file()
    } else {
        dialog.blocking_pick_file()
    };

    let Some(picked) = picked else {
        return Ok(None); // пользователь закрыл диалог — это не ошибка
    };
    let path = picked
        .simplified()
        .into_path()
        .map_err(|e| CmdError::new("bad_path", e.to_string()))?;

    if creating {
        // Диалог сохранения уже спросил про замену, но здесь «заменить» значит
        // «стереть кошельки», и согласие на это не может быть случайным.
        if path.exists() {
            return Err(CmdError::new(
                "seed_file_exists",
                "Файл с таким именем уже есть. Выберите другое имя или подключите этот файл как существующий.",
            ));
        }
    } else {
        // Чужой файл опознаётся до пароля: иначе на выбор не того файла
        // пользователь получил бы «неверный мастер-пароль» и искал бы ошибку
        // в пароле.
        seed::peek(&path)?;
    }

    remember_path(&app, &state, Some(path.clone()))?;
    log::info!("раздел сид-фраз: выбран файл ({} режим)", mode);
    Ok(Some(PickedFile {
        path: path.display().to_string(),
    }))
}

/// Отвязывает файл от приложения. Сам файл остаётся на диске: «забыть» — это
/// про запись в настройках, а не про удаление кошельков.
#[tauri::command]
pub fn seed_forget<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>) -> R<SeedStatus> {
    remember_path(&app, &state, None)?;
    log::info!("раздел сид-фраз: файл отключён");
    Ok(seed_status(state))
}

fn configured_path(state: &AppState) -> R<PathBuf> {
    state.settings().seed_vault_path.ok_or_else(|| {
        CmdError::new(
            "seed_not_configured",
            "Файл с сид-фразами не выбран — подключите или создайте его.",
        )
    })
}

/// Раздел живёт внутри разблокированного приложения: при закрытом сейфе он
/// недоступен вовсе.
fn require_main_vault(state: &AppState) -> R<()> {
    if state.is_unlocked() {
        Ok(())
    } else {
        Err(CmdError::from(vault_core::Error::Locked))
    }
}

#[tauri::command]
pub fn seed_create<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    master_password: String,
) -> R<SeedStatus> {
    require_main_vault(&state)?;
    let mut master_password = master_password;
    let path = configured_path(&state)?;
    let created = SeedVault::create(path, &master_password);
    master_password.zeroize();

    state.set_seed_vault(created?);
    log::info!("создано хранилище сид-фраз");
    let _ = app.emit("seed-unlocked", ());
    Ok(seed_status(state))
}

#[tauri::command]
pub fn seed_unlock<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    master_password: String,
) -> R<SeedStatus> {
    require_main_vault(&state)?;
    let mut master_password = master_password;
    let path = configured_path(&state)?;
    let opened = SeedVault::open(path, &master_password);
    master_password.zeroize();

    state.set_seed_vault(opened?);
    state.reset_failed_reveals();
    log::info!("хранилище сид-фраз открыто");
    let _ = app.emit("seed-unlocked", ());
    Ok(seed_status(state))
}

#[tauri::command]
pub fn seed_lock<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>) -> R<()> {
    if state.lock_seed() {
        log::info!("хранилище сид-фраз закрыто");
        let _ = app.emit("seed-locked", ());
    }
    Ok(())
}

/// Смена мастер-пароля хранилища сид-фраз.
///
/// Работает и при закрытом разделе: тогда файл открывается на время операции
/// и закрывается сразу. Так смена пароля не превращается в способ открыть
/// раздел — после неё хранилище остаётся ровно в том состоянии, в каком было.
#[tauri::command]
pub fn seed_change_password(state: St<'_>, current: String, new: String) -> R<()> {
    require_main_vault(&state)?;
    let (mut current, mut new) = (current, new);

    let out = if state.is_seed_unlocked() {
        state.with_seed_mut(|v| v.change_master_password(&current, &new))
    } else {
        let path = configured_path(&state)?;
        SeedVault::open(path, &current).and_then(|mut v| v.change_master_password(&current, &new))
    };

    current.zeroize();
    new.zeroize();
    out?;
    log::info!("мастер-пароль хранилища сид-фраз изменён");
    Ok(())
}

/// Отметка активности раздела — отдельная от общей. Общий `ping` эти часы не
/// трогает, иначе работа с паролями держала бы сид-фразы открытыми.
#[tauri::command]
pub fn seed_ping(state: St<'_>) -> u64 {
    state.touch_seed();
    state.settings().seed_autolock_secs
}

#[tauri::command]
pub fn seed_countdown(state: St<'_>) -> Option<u64> {
    if !state.is_seed_unlocked() {
        return None;
    }
    Some(
        state
            .settings()
            .seed_autolock_secs
            .saturating_sub(state.seed_idle_secs()),
    )
}

// ─── чтение записей ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn seed_list(state: St<'_>) -> R<Vec<SeedEntryView>> {
    Ok(state.with_seed(|v| {
        let mut out: Vec<SeedEntryView> = v.entries().iter().map(SeedEntryView::of).collect();
        out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        Ok(out)
    })?)
}

#[tauri::command]
pub fn seed_get(state: St<'_>, id: Uuid) -> R<SeedEntryView> {
    Ok(state.with_seed(|v| Ok(SeedEntryView::of(v.get(id)?)))?)
}

// ─── правка ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn seed_add<Rt: Runtime>(app: AppHandle<Rt>, state: St<'_>, draft: SeedDraft) -> R<Uuid> {
    if draft.title.trim().is_empty() {
        return Err(CmdError::new(
            "empty_title",
            "У записи должно быть название.",
        ));
    }
    let id = state.with_seed_mut(|v| v.add(draft.into_entry()))?;
    log::info!("сид-фраза добавлена: {id}");
    let _ = app.emit("seed-changed", ());
    Ok(id)
}

#[tauri::command]
pub fn seed_update_details<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    details: SeedDetails,
) -> R<()> {
    if details.title.trim().is_empty() {
        return Err(CmdError::new(
            "empty_title",
            "У записи должно быть название.",
        ));
    }
    state.with_seed_mut(|v| {
        v.update_details(
            id,
            details.title.clone(),
            details.wallet.clone(),
            details.derivation.clone(),
            details.network.clone(),
            details.note.clone(),
        )
    })?;
    let _ = app.emit("seed-changed", ());
    Ok(())
}

#[tauri::command]
pub fn seed_replace_phrase<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    words: Vec<String>,
    passphrase: Option<String>,
    standard: Option<bool>,
) -> R<()> {
    let mut words = words;
    let mut passphrase = passphrase.unwrap_or_default();
    let out = state.with_seed_mut(|v| {
        v.replace_phrase(
            id,
            std::mem::take(&mut words),
            std::mem::take(&mut passphrase),
            standard.unwrap_or(true),
        )
    });
    for w in &mut words {
        w.zeroize();
    }
    passphrase.zeroize();
    out?;

    log::info!("сид-фраза заменена: {id}");
    let _ = app.emit("seed-changed", ());
    Ok(())
}

/// Удаление — сразу и навсегда, поэтому подтверждается набранным названием.
/// Корзины в этом хранилище нет: держать сид-фразу «ещё тридцать дней на
/// всякий случай» — ровно то, чего пользователь не просил.
#[tauri::command]
pub fn seed_delete<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    confirm_title: String,
) -> R<()> {
    state.with_seed_mut(|v| {
        let e = v.get(id)?;
        if e.title.trim() != confirm_title.trim() {
            return Err(vault_core::Error::Other(
                "Название не совпадает — запись не удалена.".into(),
            ));
        }
        v.remove(id)
    })?;
    log::info!("сид-фраза удалена: {id}");
    let _ = app.emit("seed-changed", ());
    Ok(())
}

// ─── показ ───────────────────────────────────────────────────────────────────

/// Подтверждение мастер-паролем перед показом.
///
/// Открытый раздел сам по себе права видеть фразу не даёт: сейф мог остаться
/// открытым от предыдущего человека за тем же столом. Несколько промахов
/// подряд закрывают раздел — подбирать пароль на открытом хранилище нельзя.
fn confirm_reveal<Rt: Runtime>(
    app: &AppHandle<Rt>,
    state: &AppState,
    password: Option<String>,
) -> R<()> {
    if !state.settings().seed_require_password_on_reveal {
        return Ok(());
    }
    let mut pw = password.unwrap_or_default();
    if pw.is_empty() {
        return Err(CmdError::new(
            "seed_password_required",
            "Введите мастер-пароль хранилища сид-фраз.",
        ));
    }

    let ok = state.with_seed(|v| v.verify_master_password(&pw));
    pw.zeroize();

    if ok? {
        state.reset_failed_reveals();
        return Ok(());
    }

    let fails = state.note_failed_reveal();
    log::warn!("раздел сид-фраз: пароль не сошёлся при показе (попытка {fails})");
    if fails >= MAX_FAILED_REVEALS {
        state.lock_seed();
        let _ = app.emit("seed-locked", "failed-attempts");
        return Err(CmdError::new(
            "seed_locked_after_failures",
            "Мастер-пароль не сошёлся несколько раз подряд — раздел закрыт.",
        ));
    }
    Err(CmdError::from(vault_core::Error::BadMasterPassword))
}

/// Единственная команда, поднимающая сид-фразу в интерфейс.
#[tauri::command]
pub fn seed_reveal<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    master_password: Option<String>,
) -> R<RevealedPhrase> {
    confirm_reveal(&app, &state, master_password)?;

    let words = state.with_seed(|v| Ok(v.get(id)?.words.clone()))?;
    state.with_seed_mut(|v| v.mark_viewed(id))?;

    // В журнал идёт факт показа и ничего больше: ни слов, ни их числа.
    log::info!("сид-фраза показана: {id}");
    let _ = app.emit("seed-changed", ());

    Ok(RevealedPhrase {
        words,
        hide_after_secs: state.settings().seed_hide_after_secs,
    })
}

/// Дополнительное слово-пароль показывается отдельно от фразы: одно действие
/// пользователя — один секрет на экране.
#[tauri::command]
pub fn seed_reveal_passphrase<Rt: Runtime>(
    app: AppHandle<Rt>,
    state: St<'_>,
    id: Uuid,
    master_password: Option<String>,
) -> R<String> {
    confirm_reveal(&app, &state, master_password)?;
    let value = state.with_seed(|v| Ok(v.get(id)?.passphrase.clone()))?;
    if value.is_empty() {
        return Err(CmdError::new(
            "empty_field",
            "У этой записи нет дополнительного слова.",
        ));
    }
    log::info!("показано дополнительное слово: {id}");
    Ok(value)
}

/// Самопроверка: сверяет набранную заново фразу с сохранённой и отвечает
/// «да» или «нет», ничего не показывая. Пароль здесь не нужен — эта команда
/// не выдаёт секрета, а проверяет чужое знание.
#[tauri::command]
pub fn seed_verify_phrase(state: St<'_>, id: Uuid, words: Vec<String>) -> R<bool> {
    let mut words = words;
    let out = state.with_seed(|v| v.verify_phrase(id, &words));
    for w in &mut words {
        w.zeroize();
    }
    Ok(out?)
}

/// Очищает буфер обмена.
///
/// Нужна ровно в одном месте: фразу разрешено вставлять из буфера при вводе
/// (набирать 24 слова руками — верный способ ошибиться), и сразу после
/// вставки буфер надо стереть. Пустая строка, а не стирание: часть менеджеров
/// буфера на Linux воспринимает очистку как «владелец пропал» и возвращает
/// прежнее значение из своей истории.
#[tauri::command]
pub fn seed_clear_clipboard<Rt: Runtime>(app: AppHandle<Rt>) -> R<()> {
    app.clipboard()
        .write_text(String::new())
        .map_err(|e| CmdError::new("clipboard", e.to_string()))?;
    Ok(())
}

// ─── BIP39 ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn bip39_suggest(prefix: String, limit: Option<usize>) -> Vec<&'static str> {
    bip39::suggest(&prefix, limit.unwrap_or(6).min(24))
}

/// Проверка фразы до сохранения — чтобы форма могла сказать «слово №7 не из
/// списка» раньше, чем пользователь нажмёт «Сохранить».
#[tauri::command]
pub fn bip39_check(words: Vec<String>) -> R<()> {
    let mut words = words;
    let out = bip39::validate(&words);
    for w in &mut words {
        w.zeroize();
    }
    Ok(out?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    const PHRASE: &str =
        "letter advice cage absurd amount doctor acoustic avoid letter advice cage above";

    #[test]
    fn every_command_refuses_to_work_on_a_closed_seed_vault() {
        let state = AppState::new(Settings::default());
        assert_eq!(
            state.with_seed(|v| Ok(v.len())).unwrap_err().code(),
            "seed_locked"
        );
    }

    #[test]
    fn a_few_failed_confirmations_close_the_section() {
        let state = AppState::new(Settings::default());
        for i in 1..MAX_FAILED_REVEALS {
            assert_eq!(state.note_failed_reveal(), i);
        }
        assert_eq!(state.note_failed_reveal(), MAX_FAILED_REVEALS);
        state.reset_failed_reveals();
        assert_eq!(state.note_failed_reveal(), 1);
    }

    #[test]
    fn bip39_check_passes_a_real_phrase_and_names_the_broken_word() {
        assert!(bip39_check(words(PHRASE)).is_ok());

        let mut bad = words(PHRASE);
        bad[6] = "нетакогослова".into();
        let err = bip39_check(bad).unwrap_err();
        assert_eq!(err.code, "unknown_word");
        assert!(!err.message.contains("нетакогослова"));
    }

    #[test]
    fn suggestions_are_capped() {
        assert!(bip39_suggest("a".into(), Some(1000)).len() <= 24);
        assert_eq!(bip39_suggest("aban".into(), None), vec!["abandon"]);
    }
}
