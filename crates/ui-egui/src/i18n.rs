//! Interface translations.
//!
//! Catalogs use the craft-i18n row format: one `English<TAB>translation` per line, `#` comments;
//! `\n` in a row stands for a line break (for texts of several lines).
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

/// A row's text with its `\n` escapes turned into line breaks (kept for the program's life: the
/// catalog is built once).
fn unescape(s: &'static str) -> &'static str {
    if s.contains("\\n") { Box::leak(s.replace("\\n", "\n").into_boxed_str()) } else { s }
}

fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| (unescape(k), unescape(v)))
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

/// A message with parts: the catalog key is the English template with `{}` for each part, and
/// the translation puts the same number of `{}` where its language wants them.
pub fn trf(template: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let t = tl(template);
    let mut out = String::with_capacity(t.len() + 16);
    let mut parts = args.iter();
    let mut rest = t;
    while let Some(at) = rest.find("{}") {
        out.push_str(rest.get(..at).unwrap_or_default());
        if let Some(a) = parts.next() {
            out.push_str(&a.to_string());
        }
        rest = rest.get(at + 2..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// A message from the engine or the simulator, whole or built from parts: an exact row, else a
/// row whose `{}` stand for the parts (`no route to {}` matches `no route to 10.0.0.1`). The
/// parts are kept as they are; English when nothing matches.
pub fn tr_any(message: &str) -> String {
    if lang() == Lang::English {
        return message.to_string();
    }
    if let Some(t) = spanish().get(message) {
        return (*t).to_string();
    }
    for (key, value) in spanish().iter().filter(|(k, _)| k.contains("{}")) {
        if let Some(parts) = match_template(key, message) {
            let mut out = String::new();
            let mut rest: &str = value;
            let mut it = parts.iter();
            while let Some(at) = rest.find("{}") {
                out.push_str(rest.get(..at).unwrap_or_default());
                out.push_str(it.next().map_or("", |s| s.as_str()));
                rest = rest.get(at + 2..).unwrap_or_default();
            }
            out.push_str(rest);
            return out;
        }
    }
    message.to_string()
}

const ES_EVENTS: &str = include_str!("../i18n/es-events.tsv");

/// The event-list phrases, longest first.
fn event_phrases() -> &'static [(&'static str, &'static str)] {
    static PHRASES: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    PHRASES.get_or_init(|| {
        let mut v: Vec<_> = parse(ES_EVENTS).into_iter().collect();
        v.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));
        v
    })
}

/// An event summary in the interface language: known phrases replaced where they stand as
/// whole words, everything else (addresses, names, numbers, payload text) left as it is.
pub fn tr_event(summary: &str) -> String {
    if lang() == Lang::English { summary.to_string() } else { spanish_event(summary) }
}

fn spanish_event(summary: &str) -> String {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
    let mut out = String::with_capacity(summary.len() + 16);
    let mut i = 0;
    while let Some(rest) = summary.get(i..) {
        let Some(ch) = rest.chars().next() else { break };
        let before = summary.get(..i).and_then(|b| b.chars().next_back());
        let hit = (!word(before))
            .then(|| event_phrases().iter().find(|(en, _)| rest.starts_with(en) && !word(rest.get(en.len()..).and_then(|r| r.chars().next()))))
            .flatten();
        match hit {
            Some((en, es)) => {
                out.push_str(es);
                i += en.len();
            }
            None => {
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// The parts of `message` that fill the `{}` of `template`, if it fits.
fn match_template(template: &str, message: &str) -> Option<Vec<String>> {
    let pieces: Vec<&str> = template.split("{}").collect();
    let first = pieces.first()?;
    let mut rest = message.strip_prefix(first)?;
    let mut parts = Vec::new();
    for (i, lit) in pieces.iter().enumerate().skip(1) {
        let last = i == pieces.len() - 1;
        let at = if last {
            if lit.is_empty() { Some(rest.len()) } else { rest.ends_with(lit).then(|| rest.len() - lit.len()) }
        } else if lit.is_empty() {
            return None;
        } else {
            rest.find(lit)
        }?;
        let part = rest.get(..at)?;
        if part.is_empty() {
            return None;
        }
        parts.push(part.to_string());
        rest = rest.get(at + lit.len()..)?;
    }
    rest.is_empty().then_some(parts)
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
    fn every_sample_lab_reads_in_spanish() {
        for s in netlabcraft_engine::sample::SAMPLES {
            assert!(spanish().contains_key(s.title), "title of {}", s.id);
            assert!(spanish().contains_key(s.description), "description of {}", s.id);
            let mut session = netlabcraft_engine::Session::new();
            session.execute("file.new", serde_json::json!({"sample": s.id})).unwrap();
            let t = &session.topology;
            assert!(t.notes.is_empty() || spanish().contains_key(t.notes.as_str()), "notes of {}", s.id);
            if let Some(a) = &t.activity {
                assert!(spanish().contains_key(a.title.as_str()) && spanish().contains_key(a.instructions.as_str()), "activity of {}", s.id);
            }
        }
    }

    #[test]
    fn event_summaries_read_in_spanish() {
        // (The language itself is global and other tests run alongside: this checks the
        // Spanish path directly.)
        assert_eq!(spanish_event("ARP who has 10.0.0.1? tell 10.0.0.2"), "ARP quién tiene 10.0.0.1? responde a 10.0.0.2");
        assert_eq!(spanish_event("ICMP echo request seq 1 10.0.0.1 → 10.0.0.2 ttl 64"), "ICMP solicitud de eco sec 1 10.0.0.1 → 10.0.0.2 ttl 64");
        // Only whole words: "Rootbridge" and "online" keep their letters.
        assert_eq!(spanish_event("Rootbridge online from R1"), "Rootbridge online de R1");
        assert_eq!(spanish_event(""), "");
        assert_eq!(spanish_event("ñ on ñ"), "ñ en ñ");
        let mut seen = std::collections::BTreeSet::new();
        for l in ES_EVENTS.lines().filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty()) {
            let (k, v) = l.split_once('\t').unwrap_or_else(|| panic!("row without a tab: {l}"));
            assert!(!v.trim().is_empty() && seen.insert(k), "{l}");
        }
    }

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
        assert_eq!(parse("one\\ntwo\tuno\\ndos\n").get("one\ntwo"), Some(&"uno\ndos"));
        // Templates keep their parts, in order, and a translation keeps their number.
        assert_eq!(trf("Opened {}", &[&"lab.nlcraft"]), "Opened lab.nlcraft");
        assert_eq!(trf("{} of {}", &[&1, &2]), "1 of 2");
        assert_eq!(match_template("no route to {}", "no route to 10.0.0.1"), Some(vec!["10.0.0.1".to_string()]));
        assert_eq!(match_template("denied by access list {} ({})", "denied by access list 10 (in)"), Some(vec!["10".into(), "in".into()]));
        assert_eq!(match_template("no route to {}", "no route"), None);
        assert_eq!(match_template("ARP for {}, not this device", "ARP for 1.2.3.4, not this device!"), None);
        for (k, v) in spanish() {
            assert_eq!(k.matches("{}").count(), v.matches("{}").count(), "placeholders differ in {k}");
        }
    }
}
