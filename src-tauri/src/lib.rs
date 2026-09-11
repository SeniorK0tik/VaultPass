//! Точка сборки приложения «Сейф»: плагины, окна, трей, горячая клавиша и
//! сторож бездействия.
//!
//! Вся работа с данными живёт в крейте `vault-core`, который ничего не знает
//! ни про Tauri, ни про операционную систему. Здесь — только платформенное.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

pub mod autofill;
pub mod clipboard;
pub mod commands;
pub mod dto;
pub mod hotkey;
pub mod logging;
pub mod seed_commands;
pub mod seed_dto;
pub mod settings;
pub mod state;
pub mod tray;
pub mod windows;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, WindowEvent};

use settings::Settings;
use state::AppState;

/// Как часто сторож проверяет бездействие. Секунда — достаточно точно для
/// порогов в минуты и незаметно для процессора.
const IDLE_TICK: Duration = Duration::from_secs(1);

pub fn run() {
    let settings = Settings::load();
    let state = Arc::new(AppState::new(settings.clone()));

    let mut builder = tauri::Builder::default()
        .plugin(logging::plugin())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build());

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))
            // Второй запуск не поднимает второй экземпляр, а выводит окно
            // уже работающего: два процесса на один файл хранилища
            // затирали бы правки друг друга.
            .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
                // Второй запуск завершается молча, и со стороны это выглядит
                // как «приложение не открывается». В журнале должно остаться
                // объяснение — иначе искать причину негде.
                log::info!("повторный запуск: показываю окно уже работающего экземпляра");
                windows::spawn_show(app, windows::MAIN);
            }));
    }

    builder
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::create_vault,
            commands::unlock,
            commands::unlock_with_recovery,
            commands::lock_vault,
            commands::change_master_password,
            commands::create_recovery_key,
            commands::list_entries,
            commands::counts,
            commands::get_entry,
            commands::recent,
            commands::search,
            commands::folders,
            commands::tags,
            commands::audit_report,
            commands::load_draft,
            commands::save_draft,
            commands::delete_entry,
            commands::restore_entry,
            commands::purge_entry,
            commands::empty_trash,
            commands::toggle_favorite,
            commands::add_folder,
            commands::rename_folder,
            commands::remove_folder,
            commands::reveal_field,
            commands::copy_field,
            commands::copy_custom_field,
            commands::reveal_custom_field,
            commands::copy_text,
            commands::autofill_entry,
            commands::open_entry_url,
            commands::quick_copy,
            commands::generate_password,
            commands::estimate,
            commands::get_settings,
            commands::set_settings,
            commands::backups,
            commands::open_backup_dir,
            commands::open_window,
            commands::open_editor,
            commands::close_window,
            commands::minimize_window,
            commands::toggle_maximize,
            commands::ping,
            commands::lock_countdown,
            commands::log_info,
            commands::open_log_dir,
            commands::log_tail,
            commands::log_ui,
            seed_commands::seed_status,
            seed_commands::seed_pick_file,
            seed_commands::seed_forget,
            seed_commands::seed_create,
            seed_commands::seed_unlock,
            seed_commands::seed_lock,
            seed_commands::seed_change_password,
            seed_commands::seed_ping,
            seed_commands::seed_countdown,
            seed_commands::seed_list,
            seed_commands::seed_get,
            seed_commands::seed_add,
            seed_commands::seed_update_details,
            seed_commands::seed_replace_phrase,
            seed_commands::seed_delete,
            seed_commands::seed_reveal,
            seed_commands::seed_reveal_passphrase,
            seed_commands::seed_verify_phrase,
            seed_commands::seed_clear_clipboard,
            seed_commands::bip39_suggest,
            seed_commands::bip39_check,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            logging::log_startup(&handle);

            tray::build(&handle)?;

            if let Err(e) = hotkey::bind(&handle, &settings.hotkey) {
                // Занятое сочетание — не повод не запускаться: приложение
                // остаётся доступным через трей, а интерфейс покажет причину.
                log::warn!("горячая клавиша не зарегистрирована: {e}");
                let _ = handle.emit("hotkey-failed", e);
            }

            if let Err(e) = set_autostart(&handle, settings.launch_at_startup) {
                log::warn!("автозапуск: {e}");
            }

            // Главное окно объявлено скрытым и показывается здесь, когда
            // разметка уже загружена, — иначе в первый кадр видно пустое
            // белое окно поверх тёмной темы.
            windows::show(&handle, windows::MAIN)?;

            spawn_idle_guard(handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                // Закрытие окна прячет его. Программа продолжает жить в трее,
                // иначе горячая клавиша работала бы только до первого клика
                // по крестику.
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                    // Спрятанное главное окно — это уход от стола: раздел
                    // сид-фраз переживать его не должен.
                    if window.label() == windows::MAIN {
                        lock_seed_section(window.app_handle());
                    }
                }
                // Быстрое окно исчезает, как только теряет фокус, —
                // так ведут себя палитры команд, и так задумано в макете.
                // Мини-окно у трея — тоже, но только пока не закреплено
                // булавкой: закрепляют его как раз затем, чтобы оно
                // оставалось на экране рядом с рабочим окном.
                WindowEvent::Focused(false) => {
                    let label = window.label();
                    let pinned = window
                        .app_handle()
                        .state::<Arc<AppState>>()
                        .settings()
                        .tray_pinned;
                    if label == windows::QUICK || (label == windows::TRAY && !pinned) {
                        let _ = window.hide();
                    }
                }
                // Куда пользователь перетащил мини-окно, там оно и должно
                // открываться в следующий раз.
                WindowEvent::Moved(pos) => {
                    if window.label() == windows::TRAY && window.is_visible().unwrap_or(false) {
                        windows::remember_tray_spot(*pos);
                    }
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("не удалось собрать приложение")
        .run(move |app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                // Последний штрих перед выходом: дописать несохранённое и
                // затереть ключ.
                app.state::<Arc<AppState>>().lock_vault();
            }
        });
}

/// Сторож бездействия: раз в секунду смотрит, не пора ли закрыть сейф.
///
/// Отдельный поток, а не таймер в интерфейсе: вебвью может быть спрятано,
/// свёрнуто или занято, а блокировка обязана сработать в срок.
fn spawn_idle_guard<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(IDLE_TICK);
        let state = app.state::<Arc<AppState>>();

        // Раздел сид-фраз закрывается раньше и по своим часам: работа с
        // паролями в соседнем окне его не продлевает.
        if state.should_autolock_seed() && state.lock_seed() {
            log::info!("раздел сид-фраз закрыт по бездействию");
            let _ = app.emit("seed-locked", "autolock");
        }

        if state.should_autolock() {
            // То же и здесь: закрытие сейфа уносит с собой сид-фразы, и
            // сказать об этом надо прежде, чем ключа не станет.
            lock_seed_section(&app);
        }
        if state.should_autolock() && state.lock_vault() {
            log::info!("сейф закрыт по бездействию");
            windows::hide_all_but_main(&app);
            let _ = app.emit("vault-locked", "autolock");
        }
    });
}

/// Закрывает раздел сид-фраз и сообщает об этом окнам.
///
/// Вызывается отовсюду, где пользователь перестал смотреть на главное окно:
/// закрыл его, свернул или заблокировал сейф.
pub fn lock_seed_section<R: Runtime>(app: &AppHandle<R>) {
    if app.state::<Arc<AppState>>().lock_seed() {
        log::info!("раздел сид-фраз закрыт вместе с главным окном");
        let _ = app.emit("seed-locked", ());
    }
}

/// Включает или выключает автозапуск при входе в систему.
pub fn set_autostart<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use tauri_plugin_autostart::ManagerExt;
        let m = app.autolaunch();
        let result = if enabled { m.enable() } else { m.disable() };
        result.map_err(|e| e.to_string())
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (app, enabled);
        Ok(())
    }
}
