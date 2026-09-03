//! Генератор паролей — экран 1h макета, и он же стоит за кнопкой
//! «Сгенерировать» в редакторе записи (1g).
//!
//! Случайность берётся только у системного ГСЧ, а выбор символа делается
//! отбраковкой (rejection sampling), а не остатком от деления: остаток даёт
//! перекос в сторону первых символов алфавита и молча съедает часть энтропии.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::crypto::fill_random;
use crate::error::{Error, Result};

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?";
/// Пары, которые путаются в большинстве шрифтов, — флажок «Исключить похожие».
const LOOKALIKE: &str = "0O1lI|";

pub const MIN_LEN: usize = 8;
pub const MAX_LEN: usize = 64;
pub const MIN_WORDS: usize = 3;
pub const MAX_WORDS: usize = 12;
pub const MIN_PIN: usize = 4;
pub const MAX_PIN: usize = 12;

static WORDS: &str = include_str!("../data/wordlist_en.txt");

fn wordlist() -> Vec<&'static str> {
    WORDS
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Три режима сегментированного переключателя в макете 1h.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenMode {
    Password,
    Passphrase,
    Pin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenOptions {
    pub mode: GenMode,
    /// Длина пароля, число слов для фразы или число цифр для PIN.
    pub length: usize,
    #[serde(default = "yes")]
    pub uppercase: bool,
    #[serde(default = "yes")]
    pub digits: bool,
    #[serde(default = "yes")]
    pub symbols: bool,
    #[serde(default)]
    pub exclude_lookalike: bool,
    /// Разделитель слов во фразе.
    #[serde(default = "dash")]
    pub separator: String,
}

fn yes() -> bool {
    true
}
fn dash() -> String {
    "-".into()
}

impl Default for GenOptions {
    /// Умолчания макета: пароль на 16 символов, все три набора включены,
    /// похожие символы не исключаются.
    fn default() -> Self {
        Self {
            mode: GenMode::Password,
            length: 16,
            uppercase: true,
            digits: true,
            symbols: true,
            exclude_lookalike: false,
            separator: "-".into(),
        }
    }
}

/// Результат генерации вместе с энтропией — той самой строкой «96 бит»
/// под полосой надёжности.
#[derive(Debug, Clone, Serialize)]
pub struct Generated {
    pub value: String,
    pub entropy_bits: f64,
    /// Подпись рядом с полосой: «слабый» … «надёжный».
    pub label: &'static str,
    /// Заполнение полосы, 0.0…1.0.
    pub fill: f64,
}

impl Drop for Generated {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// Равномерный выбор индекса в `0..n` без смещения.
///
/// Берётся байт (или два, если алфавит длиннее 256) и отбрасывается всё,
/// что попадает в «хвост» за последним полным периодом, — тогда каждый
/// остаток равновероятен.
fn uniform_index(n: usize) -> Result<usize> {
    if n == 0 {
        return Err(Error::Other("пустой алфавит генератора".into()));
    }
    if n == 1 {
        return Ok(0);
    }
    if n <= 256 {
        let limit = 256 - (256 % n); // последний полный период
        let mut b = [0u8; 1];
        loop {
            fill_random(&mut b)?;
            let v = b[0] as usize;
            if v < limit {
                return Ok(v % n);
            }
        }
    } else {
        let space = 65536usize;
        let limit = space - (space % n);
        let mut b = [0u8; 2];
        loop {
            fill_random(&mut b)?;
            let v = u16::from_le_bytes(b) as usize;
            if v < limit {
                return Ok(v % n);
            }
        }
    }
}

/// Алфавит по флажкам. Строчные буквы включены всегда: в макете для них нет
/// флажка, они — основа набора.
fn alphabet(opts: &GenOptions) -> Vec<char> {
    let mut s = String::from(LOWER);
    if opts.uppercase {
        s.push_str(UPPER);
    }
    if opts.digits {
        s.push_str(DIGITS);
    }
    if opts.symbols {
        s.push_str(SYMBOLS);
    }
    s.chars()
        .filter(|c| !(opts.exclude_lookalike && LOOKALIKE.contains(*c)))
        .collect()
}

/// Наборы, которые пользователь потребовал: в готовом пароле должен быть
/// хотя бы один символ из каждого.
fn required_classes(opts: &GenOptions) -> Vec<Vec<char>> {
    let keep = |src: &str| -> Vec<char> {
        src.chars()
            .filter(|c| !(opts.exclude_lookalike && LOOKALIKE.contains(*c)))
            .collect()
    };
    let mut v = vec![keep(LOWER)];
    if opts.uppercase {
        v.push(keep(UPPER));
    }
    if opts.digits {
        v.push(keep(DIGITS));
    }
    if opts.symbols {
        v.push(keep(SYMBOLS));
    }
    v.retain(|c| !c.is_empty());
    v
}

/// Перемешивание Фишера — Йейтса на системном ГСЧ.
fn shuffle(v: &mut [char]) -> Result<()> {
    for i in (1..v.len()).rev() {
        let j = uniform_index(i + 1)?;
        v.swap(i, j);
    }
    Ok(())
}

pub fn generate(opts: &GenOptions) -> Result<Generated> {
    match opts.mode {
        GenMode::Password => generate_password(opts),
        GenMode::Passphrase => generate_passphrase(opts),
        GenMode::Pin => generate_pin(opts),
    }
}

fn generate_password(opts: &GenOptions) -> Result<Generated> {
    let len = opts.length.clamp(MIN_LEN, MAX_LEN);
    let alpha = alphabet(opts);
    if alpha.is_empty() {
        return Err(Error::Other("не выбран ни один набор символов".into()));
    }

    // Сначала по одному символу из каждого обязательного набора, остальное —
    // из общего алфавита, затем всё перемешивается. Без перемешивания
    // обязательные символы всегда стояли бы в начале.
    let classes = required_classes(opts);
    let mut out: Vec<char> = Vec::with_capacity(len);
    for class in classes.iter().take(len) {
        out.push(class[uniform_index(class.len())?]);
    }
    while out.len() < len {
        out.push(alpha[uniform_index(alpha.len())?]);
    }
    shuffle(&mut out)?;

    // Энтропия считается по общему алфавиту. Требование «хотя бы один из
    // каждого набора» на деле её чуть снижает; разница для длин от восьми
    // символов — доли бита, и оценка остаётся консервативной по смыслу
    // (она не завышает стойкость сверх log2(|A|) * L).
    let bits = (alpha.len() as f64).log2() * len as f64;
    Ok(finish(out.into_iter().collect(), bits))
}

fn generate_passphrase(opts: &GenOptions) -> Result<Generated> {
    let words = wordlist();
    let n = opts.length.clamp(MIN_WORDS, MAX_WORDS);
    let sep = if opts.separator.is_empty() {
        "-"
    } else {
        opts.separator.as_str()
    };

    let mut parts: Vec<String> = Vec::with_capacity(n);
    for i in 0..n {
        let w = words[uniform_index(words.len())?];
        // Прописная первая буква у части слов — читаемости ради; выбор
        // детерминирован позицией, поэтому энтропию он не добавляет и в
        // расчёт не входит.
        if opts.uppercase && i % 2 == 1 {
            let mut c = w.chars();
            let first = c.next().unwrap_or('a').to_ascii_uppercase();
            parts.push(format!("{first}{}", c.as_str()));
        } else {
            parts.push(w.to_string());
        }
    }
    let mut value = parts.join(sep);
    let mut bits = (words.len() as f64).log2() * n as f64;

    if opts.digits {
        let d = uniform_index(10)?;
        value.push_str(&d.to_string());
        bits += (10f64).log2();
    }
    Ok(finish(value, bits))
}

fn generate_pin(opts: &GenOptions) -> Result<Generated> {
    let len = opts.length.clamp(MIN_PIN, MAX_PIN);
    let digits: Vec<char> = DIGITS.chars().collect();
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        out.push(digits[uniform_index(digits.len())?]);
    }
    let bits = (10f64).log2() * len as f64;
    Ok(finish(out, bits))
}

fn finish(value: String, bits: f64) -> Generated {
    let (label, fill) = grade(bits);
    Generated {
        value,
        entropy_bits: (bits * 10.0).round() / 10.0,
        label,
        fill,
    }
}

/// Подписи полосы надёжности — те же четыре, что в макете.
/// Порог «надёжный» стоит на 80 битах: это запас против перебора,
/// который не устареет за срок жизни пароля.
pub fn grade(bits: f64) -> (&'static str, f64) {
    let label = if bits < 40.0 {
        "слабый"
    } else if bits < 60.0 {
        "средний"
    } else if bits < 80.0 {
        "хороший"
    } else {
        "надёжный"
    };
    // Полоса заполняется целиком к 120 битам — дальше расти незачем.
    (label, (bits / 120.0).clamp(0.05, 1.0))
}

/// Оценка стойкости уже существующего пароля — для полосы в карточке записи
/// и столбца «Надёжность» в таблице 1e.
///
/// Это приближение по составу символов, а не полноценный анализ словарей:
/// «Пароль123» получит здесь незаслуженно много. Подключение zxcvbn
/// заменит только эту функцию.
pub fn estimate_strength(password: &str) -> Generated {
    if password.is_empty() {
        return Generated {
            value: String::new(),
            entropy_bits: 0.0,
            label: "слабый",
            fill: 0.05,
        };
    }
    let mut pool = 0usize;
    if password.chars().any(|c| c.is_ascii_lowercase()) {
        pool += 26;
    }
    if password.chars().any(|c| c.is_ascii_uppercase()) {
        pool += 26;
    }
    if password.chars().any(|c| c.is_ascii_digit()) {
        pool += 10;
    }
    if password
        .chars()
        .any(|c| c.is_ascii_punctuation() || c == ' ')
    {
        pool += 33;
    }
    if !password.is_ascii() {
        pool += 64; // грубая оценка для не-ASCII
    }
    let unique = password
        .chars()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    // Повторы снижают реальное разнообразие: «aaaaaaaa» не стоит 8 символов.
    let effective = password.chars().count().min(unique * 2);
    let bits = (pool.max(2) as f64).log2() * effective as f64;
    let (label, fill) = grade(bits);
    Generated {
        value: String::new(),
        entropy_bits: (bits * 10.0).round() / 10.0,
        label,
        fill,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_has_requested_length_and_classes() {
        let o = GenOptions {
            length: 16,
            ..Default::default()
        };
        for _ in 0..40 {
            let g = generate(&o).unwrap();
            assert_eq!(g.value.chars().count(), 16);
            assert!(g.value.chars().any(|c| c.is_ascii_lowercase()));
            assert!(g.value.chars().any(|c| c.is_ascii_uppercase()));
            assert!(g.value.chars().any(|c| c.is_ascii_digit()));
            assert!(g.value.chars().any(|c| SYMBOLS.contains(c)));
        }
    }

    #[test]
    fn lookalikes_are_excluded_when_asked() {
        let o = GenOptions {
            length: 64,
            exclude_lookalike: true,
            ..Default::default()
        };
        for _ in 0..20 {
            let g = generate(&o).unwrap();
            assert!(
                !g.value.chars().any(|c| LOOKALIKE.contains(c)),
                "нашёлся похожий символ в {}",
                g.value
            );
        }
    }

    #[test]
    fn length_is_clamped_to_the_slider_range() {
        let short = generate(&GenOptions {
            length: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(short.value.chars().count(), MIN_LEN);
        let long = generate(&GenOptions {
            length: 9999,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(long.value.chars().count(), MAX_LEN);
    }

    #[test]
    fn sixteen_chars_of_the_default_alphabet_clears_ninety_bits() {
        // Строка «96 бит энтропии» в макете — при полном алфавите выходит больше.
        let g = generate(&GenOptions {
            length: 16,
            ..Default::default()
        })
        .unwrap();
        assert!(g.entropy_bits > 90.0, "получилось {} бит", g.entropy_bits);
        assert_eq!(g.label, "надёжный");
    }

    #[test]
    fn passphrase_uses_the_separator_and_counts_words() {
        let o = GenOptions {
            mode: GenMode::Passphrase,
            length: 5,
            digits: false,
            separator: "-".into(),
            ..Default::default()
        };
        let g = generate(&o).unwrap();
        assert_eq!(g.value.split('-').count(), 5);
        // 1296 слов → 10.34 бита на слово
        assert!(
            g.entropy_bits > 51.0 && g.entropy_bits < 52.0,
            "{} бит",
            g.entropy_bits
        );
    }

    #[test]
    fn pin_is_digits_only() {
        let g = generate(&GenOptions {
            mode: GenMode::Pin,
            length: 6,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.value.len(), 6);
        assert!(g.value.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn empty_alphabet_is_impossible_because_lowercase_is_always_on() {
        let o = GenOptions {
            uppercase: false,
            digits: false,
            symbols: false,
            ..Default::default()
        };
        assert!(generate(&o).is_ok());
    }

    #[test]
    fn uniform_index_stays_in_range_and_covers_it() {
        let mut seen = [false; 7];
        for _ in 0..1000 {
            let i = uniform_index(7).unwrap();
            assert!(i < 7);
            seen[i] = true;
        }
        assert!(seen.iter().all(|s| *s), "не все значения встретились");
    }

    #[test]
    fn strength_labels_match_the_mockup_vocabulary() {
        assert_eq!(estimate_strength("").label, "слабый");
        assert_eq!(estimate_strength("qwerty").label, "слабый");
        assert_eq!(estimate_strength("k7$Rm2-vQx9Lp!Zt").label, "надёжный");
    }

    #[test]
    fn repeated_characters_do_not_count_as_full_length() {
        let repeated = estimate_strength("aaaaaaaaaaaaaaaa");
        let varied = estimate_strength("abcdefghijklmnop");
        assert!(repeated.entropy_bits < varied.entropy_bits);
    }
}
