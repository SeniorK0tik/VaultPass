//! Автозаполнение — кнопка «Автозаполнение» на карточке записи (макет 1d).
//!
//! Приложение печатает логин, жмёт Tab и печатает пароль в то окно, которое
//! окажется активным. Это то же самое, что сделал бы человек руками, поэтому
//! работает с любой формой — и в браузере, и в настольной программе.
//!
//! Ограничение платформы: под Wayland синтетический ввод в чужое окно
//! запрещён самим протоколом (в этом его смысл — клавиатурные шпионы так же
//! не смогут читать чужой ввод). На Windows и на X11 всё работает; на Wayland
//! команда честно сообщает об отказе, вместо того чтобы молча ничего не делать.

use std::time::Duration;

use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};

/// Пауза перед печатью: окно «Сейфа» должно успеть спрятаться, а целевое —
/// получить фокус. Меньше 250 мс — и первые символы уходят в никуда.
const FOCUS_DELAY: Duration = Duration::from_millis(320);

pub fn is_wayland() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    std::env::var("XDG_SESSION_TYPE")
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var("WAYLAND_DISPLAY").is_ok()
}

/// Печатает логин, Tab, пароль. `submit` дополнительно жмёт Enter.
pub fn type_credentials(username: &str, password: &str, submit: bool) -> Result<(), String> {
    if is_wayland() {
        return Err(
            "Автозаполнение недоступно в сеансе Wayland: протокол запрещает \
             отправку нажатий в чужое окно. Скопируйте пароль или войдите в сеанс X11."
                .into(),
        );
    }

    std::thread::sleep(FOCUS_DELAY);

    let mut enigo = Enigo::new(&EnigoSettings::default())
        .map_err(|e| format!("не удалось получить доступ к вводу: {e}"))?;

    if !username.is_empty() {
        enigo
            .text(username)
            .map_err(|e| format!("ошибка ввода логина: {e}"))?;
        enigo
            .key(Key::Tab, Direction::Click)
            .map_err(|e| format!("ошибка нажатия Tab: {e}"))?;
        // Формы с медленной проверкой поля не успевают принять текст,
        // если печатать пароль впритык.
        std::thread::sleep(Duration::from_millis(60));
    }

    if !password.is_empty() {
        enigo
            .text(password)
            .map_err(|e| format!("ошибка ввода пароля: {e}"))?;
    }

    if submit {
        std::thread::sleep(Duration::from_millis(60));
        enigo
            .key(Key::Return, Direction::Click)
            .map_err(|e| format!("ошибка нажатия Enter: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_detection_is_linux_only() {
        if !cfg!(target_os = "linux") {
            assert!(!is_wayland(), "на Windows проверки Wayland быть не должно");
        }
    }
}
