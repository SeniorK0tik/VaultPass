//! Глобальная горячая клавиша — «Работает поверх любых окон» на экране 1i.
//!
//! Сочетание регистрируется в системе при запуске, ещё до разблокировки:
//! иначе им нельзя было бы вызвать окно, чтобы сейф разблокировать.
//!
//! Под Wayland регистрация идёт через портал XDG и поддерживается не всеми
//! окружениями. Отказ здесь не должен ронять приложение — оно продолжает
//! работать через значок в трее, а интерфейс получает предупреждение.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::settings::QuickView;
use crate::state::AppState;
use crate::windows;

fn parse(spec: &str) -> Result<Shortcut, String> {
    spec.parse::<Shortcut>()
        .map_err(|e| format!("не удалось разобрать сочетание «{spec}»: {e}"))
}

/// Регистрирует сочетание. Всё, что делает обработчик, — решает, какое окно
/// показать; сама логика живёт в [`on_trigger`].
pub fn bind<R: Runtime>(app: &AppHandle<R>, spec: &str) -> Result<(), String> {
    let shortcut = parse(spec)?;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _sc, event| {
            // Реагируем только на нажатие: без этой проверки отпускание
            // клавиши тут же переключало бы окно обратно.
            if event.state() == ShortcutState::Pressed {
                on_trigger(app);
            }
        })
        .map_err(|e| format!("система не отдала сочетание «{spec}»: {e}"))
}

pub fn unbind<R: Runtime>(app: &AppHandle<R>, spec: &str) {
    if let Ok(sc) = parse(spec) {
        let _ = app.global_shortcut().unregister(sc);
    }
}

/// Снимает старое сочетание и ставит новое. Если новое занято другой
/// программой, возвращается старое — остаться совсем без горячей клавиши хуже.
pub fn rebind<R: Runtime>(app: &AppHandle<R>, old: &str, new: &str) -> Result<(), String> {
    unbind(app, old);
    match bind(app, new) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = bind(app, old);
            Err(e)
        }
    }
}

/// Что происходит по нажатию сочетания.
pub fn on_trigger<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<Arc<AppState>>();

    // Сейф закрыт — показываем главное окно с экраном разблокировки (1f).
    // Открывать палитру поиска по закрытому сейфу бессмысленно.
    if !state.is_unlocked() {
        windows::spawn_show(app, windows::MAIN);
        let _ = app.emit("need-unlock", ());
        return;
    }

    let label = match state.settings().quick_view {
        QuickView::Tray => windows::TRAY,
        QuickView::Palette | QuickView::Fields => windows::QUICK,
    };
    windows::spawn_toggle_quick(app, label);
}
