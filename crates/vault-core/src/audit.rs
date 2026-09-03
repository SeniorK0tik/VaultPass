//! Локальная проверка здоровья паролей — раздел «Аудит» в левой панели и
//! плашка внизу экрана настроек.
//!
//! Всё считается на месте: наружу не уходит ни одного байта. Проверки утечек
//! по базам вроде HIBP здесь намеренно нет — она требует сетевого запроса,
//! а это отдельное решение с отдельным согласием пользователя.

use serde::Serialize;
use uuid::Uuid;

use crate::generator::estimate_strength;
use crate::model::{Entry, EntryKind};

/// Порог, ниже которого пароль считается слабым.
const WEAK_BITS: f64 = 60.0;
/// Возраст пароля, после которого стоит его сменить.
const STALE_DAYS: i64 = 365;
/// За сколько дней до истечения предупреждать о ключе API.
const EXPIRY_WARN_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Issue {
    Weak,
    Reused,
    Stale,
    Expiring,
    Expired,
}

impl Issue {
    pub fn title_ru(self) -> &'static str {
        match self {
            Issue::Weak => "слабый пароль",
            Issue::Reused => "повтор",
            Issue::Stale => "давно не менялся",
            Issue::Expiring => "скоро истекает",
            Issue::Expired => "истёк",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub entry_id: Uuid,
    pub title: String,
    pub issue: Issue,
    pub detail: String,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct AuditReport {
    pub findings: Vec<Finding>,
    pub weak: usize,
    pub reused: usize,
    pub stale: usize,
    pub expiring: usize,
}

impl AuditReport {
    pub fn total(&self) -> usize {
        self.findings.len()
    }
}

/// Прогоняет живые записи через все локальные проверки.
pub fn audit<'a>(entries: impl Iterator<Item = &'a Entry>) -> AuditReport {
    let live: Vec<&Entry> = entries.filter(|e| !e.is_deleted()).collect();
    let now = chrono::Utc::now();
    let mut report = AuditReport::default();

    // Повторы ищутся по количеству записей с тем же паролем. Сравнение идёт
    // по самому значению, но наружу оно не выходит — только число совпадений.
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for e in &live {
        if !e.password.is_empty() {
            *seen.entry(e.password.as_str()).or_insert(0) += 1;
        }
    }

    for e in &live {
        if !e.password.is_empty() {
            let s = estimate_strength(&e.password);
            if s.entropy_bits < WEAK_BITS {
                report.weak += 1;
                report.findings.push(Finding {
                    entry_id: e.id,
                    title: e.title.clone(),
                    issue: Issue::Weak,
                    detail: format!("{:.0} бит · {}", s.entropy_bits, s.label),
                });
            }
            if seen.get(e.password.as_str()).copied().unwrap_or(0) > 1 {
                report.reused += 1;
                report.findings.push(Finding {
                    entry_id: e.id,
                    title: e.title.clone(),
                    issue: Issue::Reused,
                    detail: "тот же пароль есть в другой записи".into(),
                });
            }
            let changed = e.password_modified_at.unwrap_or(e.created_at);
            let age = (now - changed).num_days();
            if age > STALE_DAYS {
                report.stale += 1;
                report.findings.push(Finding {
                    entry_id: e.id,
                    title: e.title.clone(),
                    issue: Issue::Stale,
                    detail: format!("не менялся {age} дн."),
                });
            }
        }

        if let Some(exp) = e.expires_at {
            let left = (exp - now).num_days();
            if left < 0 {
                report.expiring += 1;
                report.findings.push(Finding {
                    entry_id: e.id,
                    title: e.title.clone(),
                    issue: Issue::Expired,
                    detail: format!("истёк {} дн. назад", -left),
                });
            } else if left <= EXPIRY_WARN_DAYS {
                report.expiring += 1;
                report.findings.push(Finding {
                    entry_id: e.id,
                    title: e.title.clone(),
                    issue: Issue::Expiring,
                    detail: format!("истекает через {left} дн."),
                });
            }
        }

        let _ = EntryKind::ALL;
    }

    // Самое срочное — наверх.
    report.findings.sort_by_key(|f| match f.issue {
        Issue::Expired => 0,
        Issue::Weak => 1,
        Issue::Reused => 2,
        Issue::Expiring => 3,
        Issue::Stale => 4,
    });
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Entry;
    use chrono::{Duration, Utc};

    fn pw(title: &str, password: &str) -> Entry {
        let mut e = Entry::new(EntryKind::Password, title);
        e.set_password(password.into());
        e
    }

    #[test]
    fn weak_passwords_are_flagged() {
        let e = pw("Слабая", "qwerty");
        let r = audit([&e].into_iter());
        assert_eq!(r.weak, 1);
        assert_eq!(r.findings[0].issue, Issue::Weak);
    }

    #[test]
    fn strong_unique_fresh_password_is_clean() {
        let e = pw("Хорошая", "k7$Rm2-vQx9Lp!Zt");
        let r = audit([&e].into_iter());
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn reuse_is_reported_for_both_entries() {
        let a = pw("Первая", "k7$Rm2-vQx9Lp!Zt");
        let b = pw("Вторая", "k7$Rm2-vQx9Lp!Zt");
        let r = audit([&a, &b].into_iter());
        assert_eq!(r.reused, 2);
    }

    #[test]
    fn empty_passwords_are_not_treated_as_a_reuse() {
        let a = Entry::new(EntryKind::Note, "Заметка");
        let b = Entry::new(EntryKind::Note, "Другая заметка");
        let r = audit([&a, &b].into_iter());
        assert_eq!(r.reused, 0);
        assert_eq!(r.weak, 0);
    }

    #[test]
    fn stale_password_is_flagged() {
        let mut e = pw("Старая", "k7$Rm2-vQx9Lp!Zt");
        e.password_modified_at = Some(Utc::now() - Duration::days(STALE_DAYS + 5));
        let r = audit([&e].into_iter());
        assert_eq!(r.stale, 1);
    }

    #[test]
    fn expiring_and_expired_keys_are_separated() {
        let mut soon = Entry::new(EntryKind::ApiKey, "Скоро");
        soon.expires_at = Some(Utc::now() + Duration::days(21));
        let mut gone = Entry::new(EntryKind::ApiKey, "Уже");
        gone.expires_at = Some(Utc::now() - Duration::days(2));

        let r = audit([&soon, &gone].into_iter());
        assert_eq!(r.expiring, 2);
        assert_eq!(r.findings[0].issue, Issue::Expired, "истёкшее идёт первым");
    }

    #[test]
    fn deleted_entries_are_skipped() {
        let mut e = pw("В корзине", "qwerty");
        e.deleted_at = Some(Utc::now());
        assert_eq!(audit([&e].into_iter()).total(), 0);
    }
}
