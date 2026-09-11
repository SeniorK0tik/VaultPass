//! Список BIP39 и проверка сид-фразы.
//!
//! Здесь ровно две задачи: подсказать слово при вводе и сказать, сходится ли
//! набранная фраза. Ни ключей, ни адресов из фразы не выводится — считать их
//! приложению незачем, а лишний код рядом с сид-фразой это лишняя поверхность.
//!
//! Английский список из 2048 слов лежит рядом, в `data/bip39_en.txt`, дословно
//! таким, каким он записан в BIP-0039. Он отсортирован, поэтому поиск слова —
//! двоичный, а подсказка по началу слова — два `partition_point`.
//!
//! Тексты ошибок называют **номер** слова, но никогда само слово: сообщение
//! уходит в интерфейс и в журнал, а слово сид-фразы — секрет.

use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::error::{Error, Result};

static WORDS: &str = include_str!("../data/bip39_en.txt");

/// Сколько слов бывает в сид-фразе. Другие длины BIP39 не описывает.
pub const LENGTHS: [usize; 5] = [12, 15, 18, 21, 24];

/// Сколько бит несёт одно слово: 2048 = 2¹¹.
const BITS_PER_WORD: usize = 11;

fn wordlist() -> Vec<&'static str> {
    WORDS
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Приводит слово к тому виду, в котором оно лежит в списке: без пробелов
/// вокруг и в нижнем регистре. Раскладка и регистр — забота ввода, а не
/// пользователя, который диктует фразу с бумажки.
pub fn normalize(word: &str) -> String {
    word.trim().to_lowercase()
}

/// Номер слова в списке — он же его 11 бит.
pub fn index_of(word: &str) -> Option<usize> {
    let w = normalize(word);
    wordlist().binary_search(&w.as_str()).ok()
}

pub fn is_word(word: &str) -> bool {
    index_of(word).is_some()
}

/// Слова, начинающиеся с `prefix`, — подсказка под полем ввода.
///
/// Пустой запрос не возвращает ничего: показывать весь список из 2048 слов
/// бессмысленно, а первые его строки («abandon», «ability», …) в подсказке
/// выглядели бы как предложение выбрать одно из них.
pub fn suggest(prefix: &str, limit: usize) -> Vec<&'static str> {
    let p = normalize(prefix);
    if p.is_empty() {
        return Vec::new();
    }
    let list = wordlist();
    let from = list.partition_point(|w| *w < p.as_str());
    let to = list.partition_point(|w| w.starts_with(p.as_str()) || *w < p.as_str());
    list[from..to].iter().take(limit).copied().collect()
}

/// Проверяет фразу целиком: длину, каждое слово и контрольную сумму.
///
/// Контрольная сумма — последние `len·11/33` бит фразы; они должны совпасть с
/// началом SHA-256 от энтропии. Именно она ловит то, чего не ловит сверка со
/// списком: переставленные местами слова, замену одного слова другим, тоже
/// настоящим, и потерянное слово, набранное дважды.
pub fn validate(words: &[String]) -> Result<()> {
    if !LENGTHS.contains(&words.len()) {
        return Err(Error::BadWordCount(words.len()));
    }
    let list = wordlist();

    // Биты фразы: по 11 на слово, старшим вперёд. Хранятся байтами по нулю и
    // единице, чтобы в конце их можно было затереть, — это биты секрета.
    let mut bits: Vec<u8> = Vec::with_capacity(words.len() * BITS_PER_WORD);
    for (i, word) in words.iter().enumerate() {
        let w = normalize(word);
        let idx = list
            .binary_search(&w.as_str())
            .map_err(|_| Error::UnknownWord(i + 1))?;
        for shift in (0..BITS_PER_WORD).rev() {
            bits.push(((idx >> shift) & 1) as u8);
        }
    }

    let checksum_bits = words.len() * BITS_PER_WORD / 33;
    let entropy_bits = words.len() * BITS_PER_WORD - checksum_bits;

    let mut entropy = vec![0u8; entropy_bits / 8];
    for (i, bit) in bits[..entropy_bits].iter().enumerate() {
        if *bit == 1 {
            entropy[i / 8] |= 1 << (7 - i % 8);
        }
    }

    let digest = Sha256::digest(&entropy);
    let ok = bits[entropy_bits..]
        .iter()
        .enumerate()
        .all(|(i, bit)| (digest[i / 8] >> (7 - i % 8)) & 1 == *bit);

    entropy.zeroize();
    bits.zeroize();

    if ok {
        Ok(())
    } else {
        Err(Error::BadChecksum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_list_is_the_one_from_the_standard() {
        let list = wordlist();
        assert_eq!(list.len(), 2048);
        assert_eq!(list[0], "abandon");
        assert_eq!(list[2047], "zoo");
        assert!(list.windows(2).all(|p| p[0] < p[1]), "список отсортирован");
    }

    /// Векторы из самого BIP-0039 (english.json): фразы, которые обязаны
    /// сходиться. Если контрольная сумма посчитана неверно, здесь это видно
    /// сразу, а не на чужой сид-фразе.
    #[test]
    fn official_vectors_pass() {
        for v in [
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "legal winner thank year wave sausage worth useful legal winner thank yellow",
            "letter advice cage absurd amount doctor acoustic avoid letter advice cage above",
            "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong",
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon agent",
            "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal will",
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art",
            "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title",
            "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo vote",
        ] {
            validate(&phrase(v)).unwrap_or_else(|e| panic!("вектор не прошёл: {e} — {v}"));
        }
    }

    #[test]
    fn a_wrong_checksum_is_refused() {
        // Последнее слово заменено на другое, тоже настоящее.
        let bad = phrase(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zoo",
        );
        assert_eq!(validate(&bad).unwrap_err().code(), "bad_checksum");
    }

    #[test]
    fn swapped_words_are_caught_by_the_checksum() {
        let good = phrase(
            "letter advice cage absurd amount doctor acoustic avoid letter advice cage above",
        );
        let mut swapped = good.clone();
        swapped.swap(0, 1);
        assert!(validate(&good).is_ok());
        assert_eq!(validate(&swapped).unwrap_err().code(), "bad_checksum");
    }

    #[test]
    fn an_unknown_word_names_its_position_and_not_itself() {
        let mut words = phrase(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        );
        words[4] = "секрет".into();
        let err = validate(&words).unwrap_err();
        assert_eq!(err.code(), "unknown_word");
        let text = err.to_string();
        assert!(text.contains('5'), "в сообщении есть номер слова: {text}");
        assert!(
            !text.contains("секрет"),
            "само слово в сообщение не попадает"
        );
    }

    #[test]
    fn only_the_five_standard_lengths_are_accepted() {
        assert_eq!(
            validate(&phrase("abandon abandon about"))
                .unwrap_err()
                .code(),
            "bad_word_count"
        );
        assert_eq!(validate(&[]).unwrap_err().code(), "bad_word_count");
    }

    #[test]
    fn case_and_spaces_do_not_matter() {
        let words: Vec<String> = phrase(
            "  ABANDON abandon Abandon abandon abandon abandon abandon abandon abandon abandon abandon ABOUT ",
        );
        assert!(validate(&words).is_ok());
        assert!(is_word("  ZOO  "));
    }

    #[test]
    fn suggestions_follow_the_prefix() {
        assert_eq!(suggest("aban", 5), vec!["abandon"]);
        assert_eq!(
            suggest("ab", 4),
            vec!["abandon", "ability", "able", "about"]
        );
        assert!(
            suggest("", 5).is_empty(),
            "пустой запрос ничего не предлагает"
        );
        assert!(suggest("щщ", 5).is_empty());
        assert_eq!(suggest("zoo", 5), vec!["zoo"]);
    }
}
