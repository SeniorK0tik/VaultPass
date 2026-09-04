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
//!
//! Где окно появляется — тоже забота этого модуля, а не разметки: у мини-окна
//! место привязано к значку в трее, и вычислять его в вебвью значило бы
//! просить у интерфейса право двигать окна по экрану. См. `place_tray`.

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Rect, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::state::AppState;

pub const MAIN: &str = "main";
pub const QUICK: &str = "quick";
pub const TRAY: &str = "tray";
pub const EDITOR: &str = "editor";
pub const GENERATOR: &str = "generator";
pub const SETTINGS: &str = "settings";

/// Отступ мини-окна от значка в трее и от краёв экрана, логические пиксели.
const GAP: f64 = 10.0;

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
    place(&w, label);
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

// ─── где показывать мини-окно у трея ─────────────────────────────────────────
//
// Значок в трее — единственная точка, к которой мини-окно (1c) имеет смысл
// привязывать, и свой прямоугольник он сообщает сам, вместе с событием мыши.
// Windows его отдаёт, GTK — нет («Linux: Unsupported» в документации
// `TrayIcon::rect`), поэтому там остаётся запасной вариант: угол рабочей
// области, где панель стоит почти всегда.
//
// Отдельно запоминается место, куда пользователь перетащил окно за полосу
// заголовка: если он один раз решил, где окну стоять, дальше оно открывается
// там же.
//
// Wayland: положение окна там назначает композитор, а не программа, и
// `set_position` не делает ничего — GNOME открывает мини-окно посреди экрана.
// Передвинуть его можно только перетаскиванием (оно идёт через композитор и
// работает), а чтобы передвинутое окно не исчезло при первом же щелчке мимо,
// есть булавка — см. `Settings::tray_pinned`.

#[derive(Clone, Copy, Default)]
struct Placement {
    /// Прямоугольник значка в трее.
    icon: Option<Rect>,
    /// Куда пользователь перетащил окно.
    spot: Option<PhysicalPosition<i32>>,
    /// Куда окно поставили мы сами. Нужно, чтобы отличить наш собственный
    /// переезд от пользовательского: иначе окно «запоминало» бы место,
    /// которое само же и вычислило.
    placed: Option<PhysicalPosition<i32>>,
}

static PLACEMENT: Mutex<Placement> = Mutex::new(Placement {
    icon: None,
    spot: None,
    placed: None,
});

/// Запоминает, где нарисован значок в трее.
pub fn remember_tray_icon(rect: Rect) {
    PLACEMENT.lock().icon = Some(rect);
}

/// Запоминает, куда переехало мини-окно. Наши собственные перестановки
/// пропускаются: запоминать нужно только то, что сделал пользователь.
pub fn remember_tray_spot(pos: PhysicalPosition<i32>) {
    let mut p = PLACEMENT.lock();
    if p.placed == Some(pos) {
        return;
    }
    p.spot = Some(pos);
}

/// Прямоугольник в физических пикселях — общий язык для значка, рабочей
/// области и окна. `tauri::Rect` для этого не годится: он хранит то
/// логические единицы, то физические, а здесь всё уже приведено к одним.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Area {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Area {
    fn right(self) -> i32 {
        self.x + self.w
    }

    fn bottom(self) -> i32 {
        self.y + self.h
    }
}

/// Считает, где показать окно размером `win`.
///
/// Значок известен — окно встаёт вплотную к нему: по горизонтали серединой к
/// середине значка, по вертикали с той стороны, где экран, а не край. Значка
/// нет — нижний правый угол рабочей области: там панель и в Windows, и в
/// GNOME, и в KDE.
///
/// Отдельная функция без окна и монитора — чтобы эту арифметику можно было
/// проверить тестами: ошибка здесь выражается в окне, наполовину уехавшем за
/// край экрана, и заметить её на глаз получается не на каждой раскладке.
fn spot_for(icon: Option<Area>, area: Area, win: (i32, i32), gap: i32) -> PhysicalPosition<i32> {
    let (win_w, win_h) = win;

    let (x, y) = match icon {
        Some(icon) => {
            let panel_on_top = icon.y < (area.y + area.bottom()) / 2;
            (
                icon.x + icon.w / 2 - win_w / 2,
                if panel_on_top {
                    icon.bottom() + gap
                } else {
                    icon.y - win_h - gap
                },
            )
        }
        None => (area.right() - win_w - gap, area.bottom() - win_h - gap),
    };

    // Окно целиком помещается в рабочую область: значок у самого края экрана
    // иначе увёл бы выровненное по нему окно за границу. `max` нужен на
    // случай, когда окно шире рабочей области: без него границы `clamp`
    // поменялись бы местами, а это паника.
    PhysicalPosition::new(
        x.clamp(area.x + gap, (area.right() - win_w - gap).max(area.x + gap)),
        y.clamp(
            area.y + gap,
            (area.bottom() - win_h - gap).max(area.y + gap),
        ),
    )
}

/// Ставит мини-окно к значку в трее — или туда, куда его перетащили.
fn place_tray<R: Runtime>(w: &WebviewWindow<R>) -> tauri::Result<()> {
    let (icon, spot) = {
        let p = PLACEMENT.lock();
        (p.icon, p.spot)
    };

    if let Some(spot) = spot {
        return w.set_position(spot);
    }

    // Монитор берётся тот, на котором значок; если о значке ничего не
    // известно — тот, где окно оказалось сейчас.
    let monitor = icon
        // Масштаб ещё неизвестен, а нужны только координаты для выбора
        // монитора: трей присылает их физическими, и множитель 1.0 их не
        // портит.
        .map(|r| r.position.to_physical::<f64>(1.0))
        .and_then(|p| w.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.current_monitor().ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        // Про экраны ничего не известно — пусть будет хотя бы центр.
        return w.center();
    };

    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let size = w.outer_size()?;

    let area = Area {
        x: work.position.x,
        y: work.position.y,
        w: work.size.width as i32,
        h: work.size.height as i32,
    };
    let icon = icon.map(|rect| {
        let pos = rect.position.to_physical::<f64>(scale);
        let size = rect.size.to_physical::<u32>(scale);
        Area {
            x: pos.x as i32,
            y: pos.y as i32,
            w: size.width as i32,
            h: size.height as i32,
        }
    });

    let pos = spot_for(
        icon,
        area,
        (size.width as i32, size.height as i32),
        (GAP * scale).round() as i32,
    );
    PLACEMENT.lock().placed = Some(pos);
    w.set_position(pos)
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
        // Закреплённое окно не прячется, а выходит вперёд: булавка поставлена
        // ровно затем, чтобы окно оставалось на глазах, и горячая клавиша при
        // ней должна возвращать его, а не убирать. Убрать по-прежнему можно
        // тем же сочетанием — из самого окна, — или клавишей Esc.
        if is_pinned(app) && label == TRAY && !w.is_focused().unwrap_or(false) {
            w.set_focus()?;
            return Ok(true);
        }
        w.hide()?;
        Ok(false)
    } else {
        let w = show(app, label)?;
        w.emit("quick-opened", ())?;
        Ok(true)
    }
}

/// Закреплено ли мини-окно у трея.
fn is_pinned<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.state::<Arc<AppState>>().settings().tray_pinned
}

/// Ставит окно на место перед показом.
///
/// Своё место есть только у двух окон: палитра (1a, 1b) всегда посреди
/// экрана, мини-окно (1c) — у значка в трее. Остальные окна пользователь
/// двигает сам, и возвращать их в центр при каждом открытии нельзя.
fn place<R: Runtime>(w: &WebviewWindow<R>, label: &str) {
    let placed = match label {
        QUICK => w.center(),
        TRAY => place_tray(w),
        _ => return,
    };
    if let Err(e) = placed {
        log::warn!("не удалось поставить окно «{label}» на место: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Экран 1920×1080 с панелью в 40 пикселей внизу.
    const SCREEN: Area = Area {
        x: 0,
        y: 0,
        w: 1920,
        h: 1040,
    };
    const WIN: (i32, i32) = (330, 372);
    const GAP_PX: i32 = 10;

    #[test]
    fn without_the_icon_the_window_goes_to_the_corner_by_the_tray() {
        let pos = spot_for(None, SCREEN, WIN, GAP_PX);
        assert_eq!(pos.x, 1920 - 330 - 10);
        assert_eq!(pos.y, 1040 - 372 - 10);
    }

    #[test]
    fn with_a_bottom_panel_the_window_stands_above_the_icon() {
        let icon = Area {
            x: 1700,
            y: 1040,
            w: 24,
            h: 40,
        };
        let pos = spot_for(Some(icon), SCREEN, WIN, GAP_PX);
        assert_eq!(pos.x, 1700 + 12 - 165, "выровнено по середине значка");
        assert_eq!(pos.y, 1040 - 372 - 10, "над значком, не под ним");
    }

    #[test]
    fn with_a_top_panel_the_window_hangs_under_the_icon() {
        // Панель сверху: рабочая область начинается ниже неё.
        let area = Area {
            x: 0,
            y: 40,
            w: 1920,
            h: 1040,
        };
        let icon = Area {
            x: 1700,
            y: 4,
            w: 24,
            h: 32,
        };
        let pos = spot_for(Some(icon), area, WIN, GAP_PX);
        // Значок нарисован на самой панели, поэтому «под значком» и «под
        // панелью» — одно и то же место: верхний край рабочей области.
        assert_eq!(pos.y, 40 + 10);
    }

    #[test]
    fn the_window_never_leaves_the_work_area() {
        // Значок у самого левого края: выровненное по нему окно уехало бы
        // за границу экрана.
        let icon = Area {
            x: 2,
            y: 1040,
            w: 24,
            h: 40,
        };
        let pos = spot_for(Some(icon), SCREEN, WIN, GAP_PX);
        assert_eq!(pos.x, 10, "прижато к левому краю с отступом");
    }

    #[test]
    fn a_window_wider_than_the_screen_still_gets_a_position() {
        // Не выдуманный случай: на маленьком экране с крупным масштабом
        // мини-окно шире рабочей области. Границы `clamp` при этом
        // переворачиваются, и без `max` здесь была бы паника.
        let narrow = Area {
            x: 0,
            y: 0,
            w: 200,
            h: 200,
        };
        let pos = spot_for(None, narrow, WIN, GAP_PX);
        assert_eq!((pos.x, pos.y), (10, 10));
    }
}
