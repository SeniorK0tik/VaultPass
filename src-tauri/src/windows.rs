//! Окна приложения.
//!
//! Каждый экран макета, у которого нарисована своя полоса заголовка,
//! — отдельное окно: редактор (1g), генератор (1h), настройки (1i) и
//! мини-окно у трея (1c). Главное окно и быстрое окно объявлены в
//! `tauri.conf.json`, остальные создаются при первом обращении и дальше
//! просто прячутся и показываются: держать шесть готовых вебвью с первой
//! секунды — это впустую занятая память.
//!
//! Рамки везде свои: `decorations: false`, а полосу заголовка с кнопками
//! рисует разметка. Так окна выглядят одинаково в Windows и в Debian,
//! и ровно так, как в макете.

use tauri::{
    AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub const MAIN: &str = "main";
pub const QUICK: &str = "quick";
pub const TRAY: &str = "tray";
pub const EDITOR: &str = "editor";
pub const GENERATOR: &str = "generator";
pub const SETTINGS: &str = "settings";

/// Размеры взяты из макета один в один.
struct Spec {
    url: &'static str,
    title: &'static str,
    width: f64,
    height: f64,
    always_on_top: bool,
    skip_taskbar: bool,
    resizable: bool,
}

fn spec(label: &str) -> Option<Spec> {
    Some(match label {
        TRAY => Spec {
            url: "tray.html",
            title: "Сейф",
            width: 330.0,
            height: 372.0,
            always_on_top: true,
            skip_taskbar: true,
            resizable: false,
        },
        EDITOR => Spec {
            url: "editor.html",
            title: "Редактирование",
            width: 900.0,
            height: 634.0,
            always_on_top: false,
            skip_taskbar: false,
            resizable: true,
        },
        GENERATOR => Spec {
            url: "generator.html",
            title: "Генератор",
            width: 470.0,
            height: 500.0,
            always_on_top: false,
            skip_taskbar: false,
            resizable: false,
        },
        SETTINGS => Spec {
            url: "settings.html",
            title: "Настройки",
            width: 900.0,
            height: 630.0,
            always_on_top: false,
            skip_taskbar: false,
            resizable: true,
        },
        _ => return None,
    })
}

/// Возвращает окно, создавая его при первом обращении.
pub fn ensure<R: Runtime>(app: &AppHandle<R>, label: &str) -> tauri::Result<WebviewWindow<R>> {
    if let Some(w) = app.get_webview_window(label) {
        return Ok(w);
    }
    let Some(s) = spec(label) else {
        // main и quick объявлены в конфигурации; если их нет — приложение
        // не поднялось, и придумывать их на ходу нельзя.
        return Err(tauri::Error::WebviewNotFound);
    };

    WebviewWindowBuilder::new(app, label, WebviewUrl::App(s.url.into()))
        .title(s.title)
        .inner_size(s.width, s.height)
        .decorations(false)
        .resizable(s.resizable)
        .always_on_top(s.always_on_top)
        .skip_taskbar(s.skip_taskbar)
        .visible(false)
        .background_color(tauri::window::Color(0x16, 0x18, 0x26, 0xff))
        .center()
        .build()
}

/// Показывает окно и отдаёт ему фокус.
pub fn show<R: Runtime>(app: &AppHandle<R>, label: &str) -> tauri::Result<WebviewWindow<R>> {
    let w = ensure(app, label)?;
    w.show()?;
    w.unminimize().ok();
    w.set_focus()?;
    Ok(w)
}

pub fn hide<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
}

/// Прячет все окна, кроме главного, и закрывает быстрый доступ.
/// Вызывается при блокировке сейфа: на экране не должно остаться ни одной
/// открытой карточки.
pub fn hide_all_but_main<R: Runtime>(app: &AppHandle<R>) {
    for label in [QUICK, TRAY, EDITOR, GENERATOR, SETTINGS] {
        hide(app, label);
    }
}

/// Показывает окно и одновременно сообщает ему, с чем работать, —
/// например, какую запись открыл пользователь.
pub fn show_with<R: Runtime, T: serde::Serialize + Clone>(
    app: &AppHandle<R>,
    label: &str,
    event: &str,
    payload: T,
) -> tauri::Result<()> {
    let w = show(app, label)?;
    // Окно могло только что родиться и ещё не подписаться на события,
    // поэтому разметка при загрузке дополнительно сама спрашивает состояние.
    w.emit(event, payload)?;
    Ok(())
}

// ─── создание окон и главный поток ───────────────────────────────────────────
//
// На Windows создание вебвью блокирует вызывающий поток, пока WebView2 не
// ответит. Если этот поток — главный, отвечать некому: цикл событий занят
// нашим же кодом, и приложение встаёт намертво. Документация Tauri про
// `WebviewWindowBuilder::new` говорит прямо: «On Windows, this function
// deadlocks when used in a synchronous command and event handlers… You should
// use `async` commands and separate threads when creating windows».
//
// На главном потоке у нас оказываются обработчик горячей клавиши, меню трея
// и повторный запуск. Поэтому там окна не создаются напрямую — работа уходит
// в асинхронную среду выполнения, то есть на другой поток.
//
// Исключение — `setup`: он выполняется до запуска цикла событий, и Tauri сам
// создаёт там окна из конфигурации.

/// Показать окно, не блокируя поток вызова.
pub fn spawn_show<R: Runtime>(app: &AppHandle<R>, label: &'static str) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = show(&app, label) {
            log::warn!("не удалось открыть окно «{label}»: {e}");
        }
    });
}

/// Показать окно и передать ему полезную нагрузку, не блокируя поток вызова.
pub fn spawn_show_with<R: Runtime, T>(
    app: &AppHandle<R>,
    label: &'static str,
    event: &'static str,
    payload: T,
) where
    T: serde::Serialize + Clone + Send + 'static,
{
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = show_with(&app, label, event, payload) {
            log::warn!("не удалось открыть окно «{label}»: {e}");
        }
    });
}

/// Переключить быстрое окно, не блокируя поток вызова.
pub fn spawn_toggle_quick<R: Runtime>(app: &AppHandle<R>, label: &'static str) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = toggle_quick(&app, label) {
            log::warn!("не удалось открыть быстрое окно «{label}»: {e}");
        }
    });
}

/// Переключает быстрый доступ: открыт — спрятать, спрятан — показать.
/// Это и есть поведение горячей клавиши.
pub fn toggle_quick<R: Runtime>(app: &AppHandle<R>, label: &str) -> tauri::Result<bool> {
    let w = ensure(app, label)?;
    if w.is_visible().unwrap_or(false) {
        w.hide()?;
        Ok(false)
    } else {
        w.center().ok();
        w.show()?;
        w.set_focus()?;
        w.emit("quick-opened", ())?;
        Ok(true)
    }
}
