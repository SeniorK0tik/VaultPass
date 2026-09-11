//! Состояние приложения: открытый сейф, настройки и часы бездействия.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use vault_core::{Error, Result, SeedVault, Vault};

use crate::settings::Settings;

/// Всё изменяемое состояние процесса.
///
/// Ключ данных живёт внутри `Vault`, а `Vault` — только здесь. Поэтому
/// «заблокировать» — это не флаг, а `take()`: структура уничтожается, и
/// `Drop` затирает ключ и все расшифрованные строки.
pub struct AppState {
    vault: Mutex<Option<Vault>>,
    /// Хранилище сид-фраз — второй, независимый сейф со своим ключом и своими
    /// часами. Живёт рядом, а не внутри `Vault`, потому что это отдельный файл
    /// с отдельным мастер-паролем, и закрываться он должен раньше и чаще.
    seed: Mutex<Option<SeedVault>>,
    settings: Mutex<Settings>,
    last_activity: Mutex<Instant>,
    /// Часы бездействия раздела сид-фраз. Отдельные от общих намеренно: работа
    /// с паролями не должна держать сид-фразы открытыми.
    seed_last_activity: Mutex<Instant>,
    /// Номер последнего копирования в буфер. Отложенная очистка сравнивает
    /// его со своим и молча уходит, если пользователь успел скопировать
    /// что-то ещё, — иначе старый таймер стирал бы свежее значение.
    clipboard_epoch: AtomicU64,
    /// Сколько раз подряд не сошёлся пароль при попытке показать сид-фразу.
    /// Несколько промахов подряд закрывают раздел: открытое хранилище, у
    /// которого подбирают пароль, лучше закрыть, чем дать подбирать дальше.
    seed_failed_reveals: AtomicU32,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            vault: Mutex::new(None),
            seed: Mutex::new(None),
            settings: Mutex::new(settings),
            last_activity: Mutex::new(Instant::now()),
            seed_last_activity: Mutex::new(Instant::now()),
            clipboard_epoch: AtomicU64::new(0),
            seed_failed_reveals: AtomicU32::new(0),
        }
    }

    // ── сейф ────────────────────────────────────────────────────────────────

    pub fn is_unlocked(&self) -> bool {
        self.vault.lock().is_some()
    }

    pub fn set_vault(&self, v: Vault) {
        *self.vault.lock() = Some(v);
        self.touch();
    }

    /// Закрывает сейф. Возвращает `true`, если он был открыт, — по этому
    /// признаку решается, рассылать ли окнам событие блокировки.
    pub fn lock_vault(&self) -> bool {
        // Сид-фразы закрываются первыми и всегда: раздел живёт внутри
        // разблокированного приложения, и оставить его открытым при закрытом
        // сейфе значило бы держать самый дорогой секрет дольше остальных.
        // Замок сейфа при этом ещё не взят — порядок «сначала сид, потом
        // сейф» соблюдается везде, иначе два потока встретились бы здесь
        // накрест.
        self.lock_seed();

        let mut guard = self.vault.lock();
        if let Some(mut v) = guard.take() {
            // Несохранённое дописывается перед уничтожением ключа: после
            // `drop` записать его будет уже нечем.
            let _ = v.save_if_dirty();
            true
        } else {
            false
        }
    }

    /// Даёт замыкание поработать с открытым сейфом. Если сейф закрыт —
    /// [`Error::Locked`], и вызывающему не нужно это проверять самому.
    pub fn with_vault<T>(&self, f: impl FnOnce(&Vault) -> Result<T>) -> Result<T> {
        let guard = self.vault.lock();
        let v = guard.as_ref().ok_or(Error::Locked)?;
        let out = f(v);
        drop(guard);
        self.touch();
        out
    }

    /// То же для изменяющих операций. Сейф сохраняется сразу, если замыкание
    /// его поменяло: терять правку из-за неожиданного выхода незачем.
    pub fn with_vault_mut<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T>) -> Result<T> {
        let mut guard = self.vault.lock();
        let v = guard.as_mut().ok_or(Error::Locked)?;
        let out = f(v);
        if out.is_ok() {
            v.save_if_dirty()?;
        }
        drop(guard);
        self.touch();
        out
    }

    // ── хранилище сид-фраз ──────────────────────────────────────────────────

    pub fn is_seed_unlocked(&self) -> bool {
        self.seed.lock().is_some()
    }

    pub fn set_seed_vault(&self, v: SeedVault) {
        *self.seed.lock() = Some(v);
        self.touch_seed();
    }

    /// Счётчик неудачных подтверждений пароля перед показом фразы.
    pub fn note_failed_reveal(&self) -> u32 {
        self.seed_failed_reveals.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn reset_failed_reveals(&self) {
        self.seed_failed_reveals.store(0, Ordering::SeqCst);
    }

    /// Закрывает хранилище сид-фраз. `true`, если оно было открыто.
    pub fn lock_seed(&self) -> bool {
        self.reset_failed_reveals();
        let mut guard = self.seed.lock();
        if let Some(mut v) = guard.take() {
            let _ = v.save_if_dirty();
            true
        } else {
            false
        }
    }

    pub fn with_seed<T>(&self, f: impl FnOnce(&SeedVault) -> Result<T>) -> Result<T> {
        let guard = self.seed.lock();
        let v = guard.as_ref().ok_or(Error::SeedLocked)?;
        let out = f(v);
        drop(guard);
        self.touch_seed();
        out
    }

    pub fn with_seed_mut<T>(&self, f: impl FnOnce(&mut SeedVault) -> Result<T>) -> Result<T> {
        let mut guard = self.seed.lock();
        let v = guard.as_mut().ok_or(Error::SeedLocked)?;
        let out = f(v);
        if out.is_ok() {
            v.save_if_dirty()?;
        }
        drop(guard);
        self.touch_seed();
        out
    }

    /// Отмечает работу в разделе сид-фраз. Общий `touch` этих часов не
    /// трогает — иначе открытый раздел жил бы, пока пользователь возится с
    /// паролями в соседнем окне.
    pub fn touch_seed(&self) {
        *self.seed_last_activity.lock() = Instant::now();
    }

    pub fn seed_idle_secs(&self) -> u64 {
        self.seed_last_activity.lock().elapsed().as_secs()
    }

    pub fn should_autolock_seed(&self) -> bool {
        self.is_seed_unlocked() && self.seed_idle_secs() >= self.settings().seed_autolock_secs
    }

    // ── настройки ───────────────────────────────────────────────────────────

    pub fn settings(&self) -> Settings {
        self.settings.lock().clone()
    }

    pub fn set_settings(&self, mut s: Settings) -> std::io::Result<()> {
        s.sanitize();
        s.save()?;
        *self.settings.lock() = s;
        Ok(())
    }

    // ── бездействие ─────────────────────────────────────────────────────────

    /// Отмечает активность пользователя. Вызывается из каждой команды IPC и
    /// из движения мыши в окне.
    pub fn touch(&self) {
        *self.last_activity.lock() = Instant::now();
    }

    pub fn idle_secs(&self) -> u64 {
        self.last_activity.lock().elapsed().as_secs()
    }

    /// Пора ли закрывать сейф. `false`, если он и так закрыт или выбрано
    /// «Никогда».
    pub fn should_autolock(&self) -> bool {
        match self.settings().autolock_secs {
            Some(limit) => self.is_unlocked() && self.idle_secs() >= limit,
            None => false,
        }
    }

    // ── буфер обмена ────────────────────────────────────────────────────────

    pub fn next_clipboard_epoch(&self) -> u64 {
        self.clipboard_epoch.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn clipboard_epoch(&self) -> u64 {
        self.clipboard_epoch.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_state_rejects_reads() {
        let s = AppState::new(Settings::default());
        assert!(!s.is_unlocked());
        let e = s.with_vault(|_| Ok(())).unwrap_err();
        assert_eq!(e.code(), "locked");
    }

    #[test]
    fn locking_an_already_locked_vault_reports_no_change() {
        let s = AppState::new(Settings::default());
        assert!(!s.lock_vault());
    }

    #[test]
    fn autolock_never_fires_when_the_vault_is_closed() {
        let s = AppState::new(Settings {
            autolock_secs: Some(0),
            ..Default::default()
        });
        assert!(!s.should_autolock(), "закрытый сейф нечего закрывать");
    }

    #[test]
    fn autolock_is_off_when_the_setting_says_never() {
        let s = AppState::new(Settings {
            autolock_secs: None,
            ..Default::default()
        });
        assert!(!s.should_autolock());
    }

    #[test]
    fn the_seed_vault_starts_closed_and_rejects_reads() {
        let s = AppState::new(Settings::default());
        assert!(!s.is_seed_unlocked());
        assert_eq!(s.with_seed(|_| Ok(())).unwrap_err().code(), "seed_locked");
        assert!(!s.lock_seed());
    }

    #[test]
    fn autolock_of_the_seed_section_never_fires_when_it_is_closed() {
        let s = AppState::new(Settings {
            seed_autolock_secs: 60,
            ..Default::default()
        });
        assert!(!s.should_autolock_seed());
    }

    #[test]
    fn clipboard_epoch_advances_so_a_stale_timer_can_recognise_itself() {
        let s = AppState::new(Settings::default());
        let first = s.next_clipboard_epoch();
        let second = s.next_clipboard_epoch();
        assert!(second > first);
        assert_eq!(s.clipboard_epoch(), second);
    }
}
