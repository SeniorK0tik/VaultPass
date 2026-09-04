//! Настройки приложения — экран 1i.
//!
//! Файл лежит рядом с сейфом в открытом виде и намеренно не содержит ничего
//! секретного: только длительности, режимы отображения и сочетание клавиш.
//! Иначе горячая клавиша не могла бы работать до разблокировки — а именно она
//! и нужна, чтобы сейф разблокировать.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Какой из двух вариантов главного окна показывать: 1d или 1e.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MainView {
    /// 1d — три панели: категории, список, карточка.
    Panels,
    /// 1e — верхняя навигация и плотная таблица со здоровьем паролей.
    Table,
}

/// Что показывать по горячей клавише: 1a, 1b или 1c.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickView {
    /// 1a — палитра поиска, Enter копирует пароль.
    Palette,
    /// 1b — палитра с полями записи справа.
    Fields,
    /// 1c — мини-окно у трея с недавними записями.
    Tray,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Путь к файлу хранилища. `None` — путь по умолчанию.
    pub vault_path: Option<PathBuf>,

    /// Автоблокировка при бездействии, секунды. `None` — «Никогда».
    pub autolock_secs: Option<u64>,
    /// Очистка буфера обмена после копирования, секунды. `None` — «Не чистить».
    pub clipboard_clear_secs: Option<u64>,

    /// Сочетание клавиш мини-окна в записи Tauri: `CmdOrCtrl+Shift+Space`.
    pub hotkey: String,
    /// Вход по биометрии. Переключатель есть на экране, поддержки пока нет —
    /// см. раздел «Чего в этой версии нет» в README.
    pub biometrics: bool,
    /// «Скрывать секреты» — показывать значения только по явному запросу.
    pub mask_secrets: bool,
    /// Показывать нижнюю строку подсказок в быстром окне.
    pub show_key_hints: bool,
    /// Булавка мини-окна у трея: закреплённое окно не прячется, когда
    /// пользователь щёлкает мимо, и остаётся поверх остальных.
    pub tray_pinned: bool,

    pub main_view: MainView,
    pub quick_view: QuickView,
    pub launch_at_startup: bool,
}

impl Default for Settings {
    /// Умолчания — ровно те, что отмечены в макете 1i: блокировка через
    /// 10 минут, буфер чистится через 30 секунд, секреты скрыты.
    fn default() -> Self {
        Self {
            vault_path: None,
            autolock_secs: Some(600),
            clipboard_clear_secs: Some(30),
            hotkey: "CmdOrCtrl+Shift+Space".into(),
            biometrics: false,
            mask_secrets: true,
            show_key_hints: true,
            tray_pinned: false,
            main_view: MainView::Panels,
            quick_view: QuickView::Fields,
            launch_at_startup: false,
        }
    }
}

impl Settings {
    /// Каталог данных приложения: `%APPDATA%\Seif` на Windows,
    /// `~/.local/share/seif` на Debian.
    pub fn data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(if cfg!(windows) { "Seif" } else { "seif" })
    }

    pub fn config_path() -> PathBuf {
        Self::data_dir().join("config.json")
    }

    pub fn default_vault_path() -> PathBuf {
        Self::data_dir().join("seif.vault")
    }

    pub fn vault_path(&self) -> PathBuf {
        self.vault_path
            .clone()
            .unwrap_or_else(Self::default_vault_path)
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.vault_path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups")
    }

    /// Читает настройки, а при любой беде возвращает умолчания: испорченный
    /// config.json не должен мешать войти в сейф.
    pub fn load() -> Self {
        std::fs::read(Self::config_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self).unwrap_or_default())
    }

    /// Приводит присланные интерфейсом значения к допустимым: сегментированные
    /// переключатели на экране 1i предлагают ровно эти варианты, но команда
    /// IPC может принести что угодно.
    pub fn sanitize(&mut self) {
        const LOCK_CHOICES: [u64; 3] = [60, 600, 3600];
        const CLIP_CHOICES: [u64; 3] = [10, 30, 90];

        if let Some(v) = self.autolock_secs {
            if !LOCK_CHOICES.contains(&v) {
                self.autolock_secs = Some(v.clamp(15, 86_400));
            }
        }
        if let Some(v) = self.clipboard_clear_secs {
            if !CLIP_CHOICES.contains(&v) {
                self.clipboard_clear_secs = Some(v.clamp(5, 600));
            }
        }
        if self.hotkey.trim().is_empty() {
            self.hotkey = Self::default().hotkey;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_settings_mockup() {
        let s = Settings::default();
        assert_eq!(s.autolock_secs, Some(600));
        assert_eq!(s.clipboard_clear_secs, Some(30));
        assert!(s.mask_secrets);
    }

    #[test]
    fn sanitize_clamps_odd_values_and_restores_an_empty_hotkey() {
        let mut s = Settings {
            autolock_secs: Some(0),
            clipboard_clear_secs: Some(99_999),
            hotkey: "   ".into(),
            ..Default::default()
        };
        s.sanitize();
        assert_eq!(s.autolock_secs, Some(15));
        assert_eq!(s.clipboard_clear_secs, Some(600));
        assert_eq!(s.hotkey, Settings::default().hotkey);
    }

    #[test]
    fn never_stays_never() {
        let mut s = Settings {
            autolock_secs: None,
            clipboard_clear_secs: None,
            ..Default::default()
        };
        s.sanitize();
        assert_eq!(s.autolock_secs, None);
        assert_eq!(s.clipboard_clear_secs, None);
    }

    #[test]
    fn unknown_fields_do_not_break_loading() {
        let json = r#"{"autolock_secs":60,"что-то-новое":true}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.autolock_secs, Some(60));
        assert_eq!(s.hotkey, Settings::default().hotkey);
    }
}
