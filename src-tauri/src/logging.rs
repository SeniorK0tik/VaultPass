//! Журнал приложения.
//!
//! Пишется в файл рядом с данными приложения и одновременно в stdout, если
//! программа запущена из терминала. Смысл файла простой: когда что-то ломается
//! у пользователя, ему нечего рассказывать словами — достаточно приложить
//! последние строки журнала.
//!
//! Про секреты. В журнал не попадают ни пароли, ни логины, ни названия
//! записей, ни мастер-пароль: пишутся только имена команд, коды ошибок и
//! их тексты. Тексты ошибок ядра составлены так, что содержимого сейфа в них
//! нет — исключение одно, `NoSuchEntry`, где стоит UUID, а по нему без самого
//! сейфа ничего не узнать.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_log::{Target, TargetKind};

/// Имя файла журнала без расширения.
pub const LOG_FILE: &str = "seif";

/// Сколько файлов держать: текущий плюс предыдущий. Журнал нужен, чтобы
/// разобрать свежую поломку, а не как архив.
const MAX_FILE_SIZE: u128 = 4 * 1024 * 1024;

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_log::Builder::new()
        // Именно `targets`, а не `target`: у построителя уже есть цели по
        // умолчанию, и добавление к ним давало второй файл журнала
        // (с именем из productName) и удвоенные строки в stdout.
        .clear_targets()
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::LogDir {
                file_name: Some(LOG_FILE.into()),
            }),
        ])
        // Записи ниже Info в файле только мешают: интересны действия и сбои.
        .level(log::LevelFilter::Info)
        // Болтливость чужих крейтов гасим отдельно — иначе webkit и zbus
        // забивают журнал так, что своих строк в нём не найти.
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .level_for("zbus", log::LevelFilter::Warn)
        .max_file_size(MAX_FILE_SIZE)
        .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
        .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
        .build()
}

/// Каталог с файлом журнала — его показывает экран настроек.
pub fn log_dir<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_log_dir().ok()
}

/// Полный путь к текущему файлу журнала.
pub fn log_file<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    log_dir(app).map(|d| d.join(format!("{LOG_FILE}.log")))
}

/// Строки, которые пишутся при запуске. По ним сразу видно окружение:
/// без них половина сообщений об ошибках требует переспрашивать,
/// какая система и какой сеанс.
pub fn log_startup<R: Runtime>(app: &AppHandle<R>) {
    log::info!(
        "Сейф {} · формат хранилища v{} · {} {}",
        app.package_info().version,
        vault_core::FORMAT_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if cfg!(target_os = "linux") {
        log::info!(
            "сеанс: {} · рабочий стол: {}",
            std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "неизвестен".into()),
            std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "неизвестен".into())
        );
    }
    if let Some(p) = log_file(app) {
        log::info!("журнал: {}", p.display());
    }
}
