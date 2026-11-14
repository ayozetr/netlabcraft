//! Interface translations.
//!
//! Catalogs are `locales/<locale>/netlabcraft.tsv` at the repository root, in the craft-i18n row
//! format (`context<TAB>English<TAB>translation`; see `docs/translating.md`), embedded at build
//! time by `build.rs`: a release is one self-contained binary. The English text is the key, so a
//! string with no translation simply shows in English. The device consoles stay in English on
//! purpose: commands and their output are what students meet on real equipment.
//!
//! The languages on offer are the embedded catalogs, plus English. A locale looks its text up in
//! its own catalog, then in its base language's (`pt-br` then `pt`), then falls back to English.

mod catalog;
#[cfg(test)]
mod check;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use catalog::Catalog;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/locales.rs"));
}

/// One language on offer.
struct Locale {
    code: &'static str,
    name: &'static str,
    catalog: Catalog,
    /// The catalog tried next (`pt` for `pt-br`), as an index into [`locales`].
    fallback: Option<usize>,
}

/// English (index 0, no catalog) and then every embedded catalog, by locale code.
fn locales() -> &'static [Locale] {
    static LOCALES: OnceLock<Vec<Locale>> = OnceLock::new();
    LOCALES.get_or_init(|| {
        let mut v = vec![Locale { code: "en", name: "English", catalog: Catalog::default(), fallback: None }];
        for (code, text) in embedded::CATALOGS {
            let catalog = Catalog::parse(text);
            v.push(Locale { code, name: catalog.name().unwrap_or(code), catalog, fallback: None });
        }
        // `pt-br` falls back to `pt` when that exists; a script variant (`zh-hans`) never falls back.
        let codes: Vec<&str> = v.iter().map(|l| l.code).collect();
        for l in &mut v {
            l.fallback = l.code.rsplit_once('-').filter(|(_, last)| is_region(last)).and_then(|(base, _)| codes.iter().position(|c| *c == base));
        }
        v
    })
}

/// A region subtag: two letters or three digits.
fn is_region(tag: &str) -> bool {
    (tag.len() == 2 && tag.bytes().all(|b| b.is_ascii_lowercase())) || (tag.len() == 3 && tag.bytes().all(|b| b.is_ascii_digit()))
}

/// A script subtag: four letters.
fn is_script(tag: &str) -> bool {
    tag.len() == 4 && tag.bytes().all(|b| b.is_ascii_lowercase())
}

/// An interface language: English or one of the embedded catalogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lang(usize);

impl Lang {
    pub const ENGLISH: Lang = Lang(0);

    /// Every language on offer, English first, then by locale code.
    pub fn all() -> impl Iterator<Item = Lang> {
        (0..locales().len()).map(Lang)
    }

    fn locale(self) -> Option<&'static Locale> {
        locales().get(self.0)
    }

    /// The language's own name, as the language menu shows it (`@id language.name`).
    pub fn name(self) -> &'static str {
        self.locale().map_or("English", |l| l.name)
    }

    /// The locale code (`en`, `es`, `pt-br`), what is remembered between runs.
    pub fn code(self) -> &'static str {
        self.locale().map_or("en", |l| l.code)
    }

    /// The language with exactly this code.
    pub fn from_code(code: &str) -> Option<Lang> {
        locales().iter().position(|l| l.code == code).map(Lang)
    }

    /// The language for a locale name such as `es_ES.UTF-8`, `pt_BR`, `zh-Hans-CN`, `zh_TW` or
    /// `fr-CA`: the catalog of the region, else of the language. A script decides first (`zh_TW`
    /// is Traditional Chinese, `zh_CN` Simplified) and never falls back to another script's
    /// catalog. English when nothing fits.
    pub fn from_locale(locale: &str) -> Lang {
        let codes: Vec<&str> = locales().iter().map(|l| l.code).collect();
        resolve(&codes, locale).map_or(Lang::ENGLISH, Lang)
    }
}

/// The index in `have` (locale codes) of the catalog a system locale name maps to.
fn resolve(have: &[&str], locale: &str) -> Option<usize> {
    let lowered = locale.split(['.', '@']).next().unwrap_or_default().to_ascii_lowercase().replace('_', "-");
    let tags: Vec<&str> = lowered.split('-').filter(|t| !t.is_empty()).collect();
    let language = *tags.first()?;
    let script = tags.iter().skip(1).copied().find(|t| is_script(t));
    let region = tags.iter().skip(1).copied().find(|t| is_region(t));
    // Languages with more than one script on offer: the script a region implies.
    let script = script.or(match (language, region) {
        ("zh", Some("tw" | "hk" | "mo")) => Some("hant"),
        ("zh", _) => Some("hans"),
        _ => None,
    });
    let find = |code: &str| have.iter().position(|h| *h == code);
    if let Some(script) = script {
        if let Some(i) = find(&format!("{language}-{script}")) {
            return Some(i);
        }
        // A language that has script variants never shows another script's text.
        let prefix = format!("{language}-");
        if have.iter().any(|h| h.strip_prefix(prefix.as_str()).is_some_and(|r| r.split('-').next().is_some_and(is_script))) {
            return None;
        }
    }
    region.and_then(|r| find(&format!("{language}-{r}"))).or_else(|| find(language))
}

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn set_lang(l: Lang) {
    CURRENT.store(l.0, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    #[cfg(test)]
    if let Some(l) = THREAD_LANG.with(std::cell::Cell::get) {
        return l;
    }
    Lang(CURRENT.load(Ordering::Relaxed))
}

#[cfg(test)]
thread_local! {
    /// Tests run in parallel and the language is global: a test that renders in another
    /// language sets it for its own thread only (the kittest harness draws on the test's thread).
    static THREAD_LANG: std::cell::Cell<Option<Lang>> = const { std::cell::Cell::new(None) };
}

/// Renders this test thread in `l` (`None`: back to the global language).
#[cfg(test)]
pub(crate) fn set_thread_lang(l: Option<Lang>) {
    THREAD_LANG.with(|c| c.set(l));
}

/// The catalogs to look in for the current language, own first.
fn chain(l: Lang) -> impl Iterator<Item = &'static Catalog> {
    let first = l.locale().filter(|_| l != Lang::ENGLISH);
    let second = first.and_then(|f| f.fallback).and_then(|i| locales().get(i));
    first.into_iter().chain(second).map(|l| &l.catalog)
}

fn lookup(english: &str) -> Option<&'static str> {
    chain(lang()).find_map(|c| c.plain.get(english).copied())
}

/// The Spanish catalog's plain rows (tests that sweep the interface in Spanish).
#[cfg(test)]
pub(crate) fn spanish() -> &'static std::collections::HashMap<&'static str, &'static str> {
    Lang::from_code("es").and_then(Lang::locale).map(|l| &l.catalog.plain).unwrap_or_else(|| panic!("no es catalog"))
}

/// Spanish, for tests.
#[cfg(test)]
pub(crate) fn spanish_lang() -> Lang {
    Lang::from_code("es").unwrap_or_else(|| panic!("no es catalog"))
}

/// The interface text in the current language (English when there is no translation).
pub fn tl(english: &'static str) -> &'static str {
    lookup(english).unwrap_or(english)
}

/// [`tl`] for text that isn't a literal (labels passed through helpers).
pub fn tr(english: &str) -> &str {
    lookup(english).unwrap_or(english)
}

/// [`tl`] with a context that tells apart two meanings of the same English; the plain row when
/// the language has no row for the context.
pub fn tlc(context: &str, english: &'static str) -> &'static str {
    chain(lang()).find_map(|c| c.context.get(&(context, english)).copied()).or_else(|| lookup(english)).unwrap_or(english)
}

/// A message with parts: the catalog key is the English template with `{}` for each part, and
/// the translation puts the same number of `{}` where its language wants them.
pub fn trf(template: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    fill(tl(template), args)
}

fn fill(t: &str, args: &[&dyn std::fmt::Display]) -> String {
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

/// A message chosen by a count: `one` and `other` are the English forms (`{} page`, `{} pages`,
/// each with `{}` for the count), and the catalog's `@plural` row for `one|other` lists the
/// language's forms. English picks `one` for 1.
pub fn trn(one: &'static str, other: &'static str, n: u64) -> String {
    let l = lang();
    let key = format!("{one}|{other}");
    let forms = (l != Lang::ENGLISH).then(|| chain(l).find_map(|c| c.plural.get(key.as_str()))).flatten();
    let text = match forms {
        Some(f) => f.get(catalog::plural_index(l.code(), n)).or(f.last()).copied().unwrap_or(other),
        None if n == 1 => one,
        None => other,
    };
    fill(text, &[&n])
}

/// A message from the engine or the simulator, whole or built from parts: an exact row, else a
/// row whose `{}` stand for the parts (`no route to {}` matches `no route to 10.0.0.1`). The
/// parts are kept as they are; English when nothing matches.
pub fn tr_any(message: &str) -> String {
    if lang() == Lang::ENGLISH {
        return message.to_string();
    }
    if let Some(t) = lookup(message) {
        return t.to_string();
    }
    // The rows that wrap another message come first (`line 3: {} steps without...` would
    // otherwise fit a row that begins with `{}`), and the wrapped message is translated too.
    // Any other part (an address, a name) stays as it is.
    const WRAPPERS: [&str; 2] = ["line {}: {}", "stopped: {}"];
    let wrappers = WRAPPERS.iter().filter_map(|k| chain(lang()).find_map(|c| c.plain.get_key_value(*k)));
    let others = templates(lang()).iter().filter(|(k, _)| !WRAPPERS.contains(k));
    for (key, value) in wrappers.map(|(k, v)| (*k, *v)).chain(others.map(|(k, v)| (*k, *v))) {
        if let Some(parts) = match_template(key, message) {
            let mut out = String::new();
            let mut rest: &str = value;
            let mut it = parts.iter();
            let wraps = WRAPPERS.contains(&key);
            let last = parts.len().saturating_sub(1);
            let mut n = 0;
            while let Some(at) = rest.find("{}") {
                out.push_str(rest.get(..at).unwrap_or_default());
                let part = it.next().map_or("", |s| s.as_str());
                if wraps && n == last {
                    out.push_str(&tr_any(part))
                } else {
                    out.push_str(part)
                }
                n += 1;
                rest = rest.get(at + 2..).unwrap_or_default();
            }
            out.push_str(rest);
            return out;
        }
    }
    message.to_string()
}

type Rows = Vec<(&'static str, &'static str)>;

/// The rows with `{}` for a language, most literal text first (so the most specific template
/// wins), own rows before its base language's.
fn templates(l: Lang) -> &'static Rows {
    static CACHE: OnceLock<Vec<OnceLock<Rows>>> = OnceLock::new();
    let all = CACHE.get_or_init(|| locales().iter().map(|_| OnceLock::new()).collect());
    static EMPTY: Rows = Vec::new();
    let Some(slot) = all.get(l.0) else { return &EMPTY };
    slot.get_or_init(|| {
        let mut seen = std::collections::HashSet::new();
        let mut v: Rows = Vec::new();
        for c in chain(l) {
            v.extend(c.plain.iter().filter(|(k, _)| k.contains("{}") && seen.insert(**k)).map(|(k, t)| (*k, *t)));
        }
        let literal = |k: &str| k.replace("{}", "").len();
        v.sort_by(|a, b| literal(b.0).cmp(&literal(a.0)).then(a.0.cmp(b.0)));
        v
    })
}

/// The event-list phrases of a language, longest first.
fn event_phrases(l: Lang) -> &'static Rows {
    static CACHE: OnceLock<Vec<OnceLock<Rows>>> = OnceLock::new();
    let all = CACHE.get_or_init(|| locales().iter().map(|_| OnceLock::new()).collect());
    static EMPTY: Rows = Vec::new();
    let Some(slot) = all.get(l.0) else { return &EMPTY };
    slot.get_or_init(|| {
        let mut seen = std::collections::HashSet::new();
        let mut v: Rows = Vec::new();
        for c in chain(l) {
            v.extend(c.context.iter().filter(|((ctx, k), _)| *ctx == "event" && seen.insert(*k)).map(|((_, k), t)| (*k, *t)));
        }
        v.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));
        v
    })
}

/// An event summary in the interface language: known phrases replaced where they stand as
/// whole words, everything else (addresses, names, numbers, payload text) left as it is.
pub fn tr_event(summary: &str) -> String {
    translate_event(lang(), summary)
}

fn translate_event(l: Lang, summary: &str) -> String {
    let phrases = event_phrases(l);
    if phrases.is_empty() {
        return summary.to_string();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
    let mut out = String::with_capacity(summary.len() + 16);
    let mut i = 0;
    while let Some(rest) = summary.get(i..) {
        let Some(ch) = rest.chars().next() else { break };
        let before = summary.get(..i).and_then(|b| b.chars().next_back());
        let hit = (!word(before))
            .then(|| phrases.iter().find(|(en, _)| rest.starts_with(en) && !word(rest.get(en.len()..).and_then(|r| r.chars().next()))))
            .flatten();
        match hit {
            Some((en, tr)) => {
                out.push_str(tr);
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
    l.locale().map_or(0, |l| l.catalog.len())
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

    /// Every BoardScript compile error reads in Spanish, with its line number and the wrapped
    /// message both translated, and the English text is untouched in English.
    #[test]
    fn boardscript_compile_messages_read_in_spanish() {
        let mut s = netlabcraft_engine::Session::new();
        s.execute("file.new", serde_json::json!({"sample": "board"})).unwrap();
        let long = "x = 1\n".repeat(400);
        let cases: Vec<(&str, String)> = [
            "let x = 99999999999999999999",
            "let x = \"abc",
            "let x = 3 $",
            "let x = (3",
            "write(5, 1)",
            "let x =",
            "let x = if",
            "let x = foo(1)",
            "let x = )",
            "5 = 3",
            "else",
            "end",
            "let",
            "x",
            "if 1\nwrite(D0, 1)",
            "let if = 3",
            "while 1\nelse\nend",
            "if 1\nend\nelse\nend",
            "sleep 1 sleep 2",
            "if 1\nelse\nelse\nend",
        ]
        .iter()
        .map(|p| (*p, (*p).to_string()))
        .chain([("a long program", long), ("a huge one", "x".repeat(20_000))])
        .collect();
        let mut refused = 0;
        for (what, src) in cases {
            let e = s.execute("board.program", serde_json::json!({"device": "Board1", "source": src})).err();
            let Some(e) = e else { continue };
            refused += 1;
            let english = e.to_string();
            assert_eq!(tr_any(&english), english, "English stays as it is");
            set_thread_lang(Some(spanish_lang()));
            let spanish = tr_any(&english);
            set_thread_lang(None);
            assert_ne!(spanish, english, "no Spanish row for the message of `{what}`: {english}");
            if english.starts_with("line ") {
                assert!(spanish.starts_with("línea "), "{spanish}");
            }
        }
        assert!(refused >= 18, "most of the broken programs are refused: {refused}");
        // The line number and the wrapped message are both translated.
        set_thread_lang(Some(spanish_lang()));
        let t = tr_any("line 3: `end` without an `if` or `while`");
        let stop = tr_any("stopped: line 2: the board has no pin D9");
        let wired = tr_any("D0 is wired to Temp1, a sensor: read it instead");
        set_thread_lang(None);
        assert_eq!(t, "línea 3: `end` sin un `if` o `while`");
        assert_eq!(stop, "parado: línea 2: la placa no tiene el pin D9");
        assert_eq!(wired, "D0 está cableado a Temp1, un sensor: léelo en su lugar");
    }

    /// A program that stops at run time leaves a message in the machine, translated the same way.
    #[test]
    fn boardscript_run_messages_read_in_spanish() {
        let mut s = netlabcraft_engine::Session::new();
        s.execute("file.new", serde_json::json!({"sample": "board"})).unwrap();
        let mut seen = 0;
        for src in ["write(D9, 1)", "let x = 1 / 0", "let y = z", "let a = \"x\"\nwrite(D0, a)", "while 1\nlet q = 1\nend"] {
            let st = s.execute("board.program", serde_json::json!({"device": "Board1", "source": src, "run": true})).unwrap();
            let Some(m) = st["message"].as_str() else { continue };
            seen += 1;
            set_thread_lang(Some(spanish_lang()));
            let es = tr_any(m);
            let log = st["log"].as_array().and_then(|l| l.last()).and_then(|v| v.as_str()).map(tr_any).unwrap_or_default();
            set_thread_lang(None);
            assert_ne!(es, m, "no Spanish row for `{m}`");
            assert!(es.starts_with("línea ") && log.starts_with("parado: línea "), "{es} / {log}");
        }
        assert!(seen >= 3, "the runtime errors were reached: {seen}");
    }

    fn es_event(summary: &str) -> String {
        translate_event(spanish_lang(), summary)
    }

    #[test]
    fn event_summaries_read_in_spanish() {
        // (The language itself is global and other tests run alongside: this checks the
        // Spanish path directly.)
        assert_eq!(es_event("ARP who has 10.0.0.1? tell 10.0.0.2"), "ARP quién tiene 10.0.0.1? responde a 10.0.0.2");
        assert_eq!(es_event("ICMP echo request seq 1 10.0.0.1 → 10.0.0.2 ttl 64"), "ICMP solicitud de eco sec 1 10.0.0.1 → 10.0.0.2 ttl 64");
        // Only whole words: "Rootbridge" and "online" keep their letters.
        assert_eq!(es_event("Rootbridge online from R1"), "Rootbridge online de R1");
        assert_eq!(es_event(""), "");
        assert_eq!(es_event("ñ on ñ"), "ñ en ñ");
        assert_eq!(translate_event(Lang::ENGLISH, "ARP who has 10.0.0.1?"), "ARP who has 10.0.0.1?");
    }

    /// No row outlives its text: every key is still written somewhere in the workspace's code
    /// (a template by the start of its longest fixed piece, since the code may fill constants in).
    #[test]
    fn every_row_is_still_used() {
        fn rust_files(dir: &std::path::Path, into: &mut String) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    rust_files(&p, into);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    into.push_str(&std::fs::read_to_string(&p).unwrap_or_default().replace("\\\"", "\""));
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut code = String::new();
        rust_files(&root.join("crates"), &mut code);
        rust_files(&root.join("apps"), &mut code);
        let mut stale = Vec::new();
        let keys: std::collections::BTreeSet<&'static str> = embedded::CATALOGS
            .iter()
            .flat_map(|(_, text)| catalog::lines(text))
            .filter_map(|l| match l.cols[..] {
                ["" | "@msg", key, _] => Some(key),
                _ => None,
            })
            .collect();
        for key in keys {
            if code.contains(key) || code.contains(catalog::unescape(key)) {
                continue;
            }
            // `{} via {}` written as `{} via {via}`.
            let pieces: Vec<&str> = key.split("{}").collect();
            let named = |at: usize| {
                let mut rest = code.get(at..).unwrap_or_default();
                for (i, p) in pieces.iter().enumerate() {
                    if i > 0 {
                        let Some(close) = rest.strip_prefix('{').and_then(|r| r.find('}').filter(|c| *c < 24)) else { return false };
                        rest = rest.get(close + 2..).unwrap_or_default();
                    }
                    let Some(r) = rest.strip_prefix(p) else { return false };
                    rest = r;
                }
                true
            };
            if pieces.len() > 1 && code.match_indices('"').any(|(at, _)| named(at + 1)) {
                continue;
            }
            let piece = key.split("{}").max_by_key(|p| p.len()).unwrap_or_default();
            let start: String = piece.chars().take(24).collect();
            if key.contains("{}") && start.chars().count() >= 8 && code.contains(&start) {
                continue;
            }
            stale.push(key);
        }
        assert!(stale.is_empty(), "rows for text the code no longer has: {stale:#?}");
    }

    #[test]
    fn lookup_falls_back_to_english() {
        assert_eq!(Lang::from_locale("es_ES.UTF-8"), spanish_lang());
        assert_eq!(Lang::from_locale("en-GB"), Lang::ENGLISH);
        assert_eq!(Lang::from_locale(""), Lang::ENGLISH);
        assert_eq!(Lang::from_locale("C"), Lang::ENGLISH);
        assert!(catalog_size(spanish_lang()) > 1000);
        assert_eq!(catalog_size(Lang::ENGLISH), 0);
        // Templates keep their parts, in order, and a translation keeps their number.
        assert_eq!(trf("Opened {}", &[&"lab.nlcraft"]), "Opened lab.nlcraft");
        assert_eq!(trf("{} of {}", &[&1, &2]), "1 of 2");
        assert_eq!(match_template("no route to {}", "no route to 10.0.0.1"), Some(vec!["10.0.0.1".to_string()]));
        assert_eq!(match_template("denied by access list {} ({})", "denied by access list 10 (in)"), Some(vec!["10".into(), "in".into()]));
        assert_eq!(match_template("no route to {}", "no route"), None);
        assert_eq!(match_template("ARP for {}, not this device", "ARP for 1.2.3.4, not this device!"), None);
    }

    /// English plurals pick `one` for 1.
    #[test]
    fn counts_choose_a_plural_form() {
        assert_eq!(trn("{} item", "{} items", 1), "1 item");
        assert_eq!(trn("{} item", "{} items", 0), "0 items");
        assert_eq!(trn("{} item", "{} items", 3), "3 items");
    }

    /// The names, codes and menu list come from the catalogs; the saved `en` and `es` still work.
    #[test]
    fn languages_come_from_the_catalogs() {
        let codes: Vec<&str> = Lang::all().map(Lang::code).collect();
        assert_eq!(codes.first(), Some(&"en"));
        assert!(codes.contains(&"es"));
        assert_eq!(spanish_lang().name(), "Español");
        assert_eq!(Lang::ENGLISH.name(), "English");
        assert_eq!(Lang::from_code("en"), Some(Lang::ENGLISH));
        assert_eq!(Lang::from_locale("en"), Lang::ENGLISH);
        assert_eq!(Lang::from_locale("es"), spanish_lang());
        assert_eq!(Lang::from_locale("es-419"), spanish_lang());
        assert_eq!(Lang::from_locale("es_MX.UTF-8@euro"), spanish_lang());
    }

    /// System locales map onto the catalogs that exist; a script with no catalog is English, never
    /// another script's text.
    #[test]
    fn system_locales_map_onto_catalogs() {
        let have = ["en", "es", "fr", "it", "de", "pt-br", "zh-hans"];
        let pick = |l: &str| resolve(&have, l).and_then(|i| have.get(i)).copied();
        for (locale, want) in [
            ("pt_BR.UTF-8", Some("pt-br")),
            ("pt-PT", None),
            ("zh_CN", Some("zh-hans")),
            ("zh-Hans-CN", Some("zh-hans")),
            ("zh_TW", None),
            ("zh-Hant", None),
            ("zh", Some("zh-hans")),
            ("fr_CA", Some("fr")),
            ("de_AT", Some("de")),
            ("es_ES.UTF-8", Some("es")),
            ("es-Latn", Some("es")),
            ("en_US.UTF-8", Some("en")),
            ("C", None),
        ] {
            assert_eq!(pick(locale), want, "{locale}");
        }
        // With a Portuguese base catalog too, `pt-br` falls back to it and `pt_PT` finds it.
        let with_pt = ["en", "pt", "pt-br"];
        assert_eq!(resolve(&with_pt, "pt_PT").and_then(|i| with_pt.get(i)), Some(&"pt"));
    }
}
