//! Interface translations.
//!
//! Catalogs use the craft-i18n row format: one `English<TAB>translation` per line, `#` comments.
//! The English text is the key, so a string with no translation simply shows in English. The
//! device consoles stay in English on purpose: commands and their output are what students
//! meet on real equipment.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// An interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    English,
    Spanish,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::English, Lang::Spanish];

    /// The language's own name, as the language menu shows it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::Spanish => "Español",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::Spanish => "es",
        }
    }

    /// The language for a locale name such as `es_ES.UTF-8`, `es-419` or `en-GB`.
    pub fn from_locale(locale: &str) -> Lang {
        if locale.to_ascii_lowercase().starts_with("es") { Lang::Spanish } else { Lang::English }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(l: Lang) {
    CURRENT.store(if l == Lang::Spanish { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 { Lang::Spanish } else { Lang::English }
}

const ES: &str = include_str!("../i18n/es.tsv");

fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, v)| !v.trim().is_empty())
        .collect()
}

fn spanish() -> &'static HashMap<&'static str, &'static str> {
    static ES_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    ES_MAP.get_or_init(|| parse(ES))
}

/// The interface text in the current language (English when there is no translation).
pub fn tl(english: &'static str) -> &'static str {
    match lang() {
        Lang::English => english,
        Lang::Spanish => spanish().get(english).copied().unwrap_or(english),
    }
}

/// [`tl`] for text that isn't a literal (labels passed through helpers).
pub fn tr(english: &str) -> &str {
    match lang() {
        Lang::English => english,
        Lang::Spanish => spanish().get(english).copied().unwrap_or(english),
    }
}

/// How many catalog rows a language has (for the localization report).
pub fn catalog_size(l: Lang) -> usize {
    match l {
        Lang::English => 0,
        Lang::Spanish => spanish().len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_falls_back_to_english() {
        assert_eq!(Lang::from_locale("es_ES.UTF-8"), Lang::Spanish);
        assert_eq!(Lang::from_locale("en-GB"), Lang::English);
        // Every row has a key and a translation, and no key repeats.
        let mut seen = std::collections::BTreeSet::new();
        for l in ES.lines().filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty()) {
            let (k, v) = l.split_once('\t').unwrap_or_else(|| panic!("row without a tab: {l}"));
            assert!(!v.trim().is_empty(), "empty translation for {k}");
            assert!(seen.insert(k), "duplicate key {k}");
        }
        assert!(catalog_size(Lang::Spanish) > 100);
        assert_eq!(parse("a\tb\n# c\td\nx\t\n").get("a"), Some(&"b"));
    }
}
