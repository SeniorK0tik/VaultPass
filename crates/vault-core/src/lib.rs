//! Ядро приложения «Сейф»: формат хранилища, криптография, модель записей,
//! поиск, генератор паролей и локальная проверка их здоровья.
//!
//! Крейт намеренно ничего не знает ни про Tauri, ни про операционную систему:
//! в нём нет ни буфера обмена, ни окон, ни горячих клавиш. Благодаря этому он
//! собирается и проверяется тестами одинаково на Windows и на Debian, а всё
//! платформенное живёт в `src-tauri`.

#![forbid(unsafe_code)]

pub mod audit;
pub mod bip39;
pub mod crypto;
pub mod error;
pub mod generator;
pub mod model;
pub mod seed;
pub mod vault;

pub use error::{Error, Result};
pub use model::{CustomField, Entry, EntryKind, Folder, PasswordHistoryItem};
pub use seed::{SeedEntry, SeedVault, SEED_FORMAT_VERSION};
pub use vault::{Counts, Vault, FORMAT_VERSION};
