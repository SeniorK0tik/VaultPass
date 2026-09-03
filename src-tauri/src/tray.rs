//! Значок в области уведомлений и его меню.
//!
//! «Сейф» живёт в трее: закрытие главного окна прячет его, а не завершает
//! программу, иначе горячая клавиша перестала бы работать до следующего
//! запуска.

use std::sync::Arc;

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::state::AppState;
use crate::windows;

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Открыть «Сейф»", true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", "Быстрый доступ", true, None::<&str>)?;
    let generator = MenuItem::with_id(app, "generator", "Генератор паролей", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "Заблокировать", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Настройки", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &open, &quick, &sep, &generator, &settings, &sep, &lock, &quit,
        ],
    )?;

    TrayIconBuilder::with_id("seif")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or(tauri::Error::WebviewNotFound)?,
        )
        .tooltip("Сейф")
        .menu(&menu)
        // Меню по левой кнопке не открываем: левый клик отдан мини-окну (1c),
        // как принято у менеджеров паролей.
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(on_icon)
        .build(app)?;
    Ok(())
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        "open" => windows::spawn_show(app, windows::MAIN),
        "quick" => crate::hotkey::on_trigger(app),
        "generator" => windows::spawn_show(app, windows::GENERATOR),
        "settings" => windows::spawn_show(app, windows::SETTINGS),
        "lock" => {
            let state = app.state::<Arc<AppState>>();
            if state.lock_vault() {
                windows::hide_all_but_main(app);
                let _ = app.emit("vault-locked", ());
            }
        }
        "quit" => {
            // Перед выходом сейф закрывается штатно: несохранённое
            // дописывается, ключ затирается.
            let state = app.state::<Arc<AppState>>();
            state.lock_vault();
            app.exit(0);
        }
        _ => {}
    }
}

fn on_icon<R: Runtime>(tray: &tauri::tray::TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        crate::hotkey::on_trigger(tray.app_handle());
    }
}
