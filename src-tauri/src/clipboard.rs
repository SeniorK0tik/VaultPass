//! Копирование секрета с автоочисткой — «буфер очистится через 0:19»
//! в макете 1b и переключатель на экране настроек.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::state::AppState;

/// Кладёт значение в буфер и, если очистка включена, заводит таймер.
///
/// Возвращает, через сколько секунд буфер будет очищен, — интерфейс рисует
/// по этому числу обратный отсчёт.
pub fn copy_secret<R: Runtime>(app: &AppHandle<R>, value: String) -> Result<Option<u64>, String> {
    app.clipboard()
        .write_text(value.clone())
        .map_err(|e| format!("не удалось записать в буфер обмена: {e}"))?;

    let state = app.state::<Arc<AppState>>();
    let Some(secs) = state.settings().clipboard_clear_secs else {
        return Ok(None);
    };

    // Метка этого копирования. Если пользователь скопирует что-то ещё
    // раньше срока, номер вырастет и наш таймер сам себя отменит.
    let epoch = state.next_clipboard_epoch();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        tokio_sleep(secs).await;

        let state = app.state::<Arc<AppState>>();
        if state.clipboard_epoch() != epoch {
            return; // буфер уже занят более свежим значением
        }

        // Чужое содержимое не трогаем: между копированием и таймером
        // пользователь мог скопировать текст мимо «Сейфа».
        match app.clipboard().read_text() {
            Ok(current) if current == value => {
                // Пустая строка вместо стирания: часть менеджеров буфера
                // на Linux воспринимает очистку как «владелец пропал» и
                // возвращает прежнее значение из своей истории.
                let _ = app.clipboard().write_text(String::new());
                let _ = app.emit("clipboard-cleared", ());
            }
            _ => {}
        }
    });

    Ok(Some(secs))
}

async fn tokio_sleep(secs: u64) {
    tauri::async_runtime::spawn_blocking(move || std::thread::sleep(Duration::from_secs(secs)))
        .await
        .ok();
}
