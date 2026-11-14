//! The catalog checker (the counterpart of craft-i18n's `craft-i18n-tests`): every rule of
//! docs/translating.md, run over every catalog in `locales/`, plus a coverage report.
//!
//! `cargo test -p netlabcraft-ui-egui coverage_report -- --nocapture` prints each catalog's rows
//! against the largest.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::catalog::{KNOWN_CONTEXTS, LANGUAGE_NAME_ID, Line, base_language, lines, plural_forms};

/// The file every locale directory holds.
const FILE: &str = "netlabcraft.tsv";

fn locales_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales")
}

/// A lowercase BCP 47 tag: `es`, `pt-br`, `zh-hans`.
fn is_locale_name(s: &str) -> bool {
    let mut parts = s.split('-');
    let lang = parts.next().unwrap_or_default();
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// The locale a regional one falls back to: `es` for `es-mx` (its last subtag is a region), never
/// for a script variant (`zh-hans`).
fn base_of(locale: &str) -> Option<&str> {
    let (base, last) = locale.rsplit_once('-')?;
    ((last.len() == 2 && last.bytes().all(|b| b.is_ascii_lowercase())) || (last.len() == 3 && last.bytes().all(|b| b.is_ascii_digit())))
        .then_some(base)
}

/// Every `\` in a column starts `\n`, `\t` or `\\`.
fn bad_escape(col: &str) -> Option<String> {
    let mut it = col.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n' | 't' | '\\') => {}
                Some(o) => return Some(format!("`\\{o}`")),
                None => return Some("a `\\` at the end".into()),
            }
        }
    }
    None
}

fn placeholders(s: &str) -> usize {
    s.matches("{}").count()
}

/// The problems of one catalog's text (empty: it passes). `locale` is its directory name.
pub(crate) fn check_catalog(locale: &str, text: &str) -> Vec<String> {
    let mut errs = Vec::new();
    let at = |no: usize, msg: String| format!("{locale}/{FILE}:{no}: {msg}");
    if !text.lines().any(|l| l.trim_start_matches('\u{feff}').starts_with("# Clean room:")) {
        errs.push(format!("{locale}/{FILE}: the header lacks its `# Clean room:` line (see docs/translating.md)"));
    }
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut has_name = false;
    for Line { no, cols } in lines(text) {
        let [context, source, translation] = cols[..] else {
            errs.push(at(no, format!("expected 3 tab-separated columns, found {}", cols.len())));
            continue;
        };
        for (what, col) in [("context", context), ("source text", source), ("translation", translation)] {
            if let Some(e) = bad_escape(col) {
                errs.push(at(no, format!("{what} has {e}; only `\\n`, `\\t` and `\\\\` are escapes")));
            }
        }
        if source.is_empty() {
            errs.push(at(no, "empty source text".into()));
        }
        if translation.trim().is_empty() {
            errs.push(at(no, format!("empty translation for `{source}`: leave the row out and the app shows English")));
        }
        if !seen.insert((context, source)) {
            errs.push(at(no, format!("`{source}` appears twice with context `{context}`")));
        }
        if context.starts_with('@') && !KNOWN_CONTEXTS.contains(&context) {
            errs.push(at(no, format!("unknown context `{context}` (known: {})", KNOWN_CONTEXTS.join(", "))));
        } else if !context.starts_with('@') && !context.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            errs.push(at(no, format!("context `{context}` must be one plain word")));
        }
        match context {
            "@id" => {
                if source == LANGUAGE_NAME_ID {
                    has_name = !translation.trim().is_empty();
                }
            }
            "@plural" => {
                let english: Vec<&str> = source.split('|').collect();
                let forms: Vec<&str> = translation.split('|').collect();
                if english.len() != 2 {
                    errs.push(at(no, format!("the English of a plural row is `one|other`, found {} forms", english.len())));
                    continue;
                }
                if english[0].contains("{}") != english[1].contains("{}") || placeholders(english[0]) != placeholders(english[1]) {
                    errs.push(at(no, "both English forms need the same placeholders".into()));
                }
                match plural_forms(locale) {
                    None => errs.push(at(no, format!("no plural rule for `{}`: add it to `plural_forms` and `plural_index`", base_language(locale)))),
                    Some(n) if n != forms.len() => {
                        errs.push(at(no, format!("`{locale}` has {n} plural form(s), the row has {}", forms.len())));
                    }
                    Some(_) => {}
                }
                for f in &forms {
                    parity(&mut errs, &at, no, english[1], f);
                }
            }
            _ => parity(&mut errs, &at, no, source, translation),
        }
    }
    if !has_name {
        errs.push(format!("{locale}/{FILE}: no `@id\\tlanguage.name\\t<the language's own name>` row"));
    }
    errs
}

/// Placeholders and a trailing `…` agree between a source and its translation.
fn parity(errs: &mut Vec<String>, at: &dyn Fn(usize, String) -> String, no: usize, source: &str, translation: &str) {
    if placeholders(source) != placeholders(translation) {
        errs.push(at(
            no,
            format!("`{{}}` placeholders differ: `{source}` has {}, `{translation}` has {}", placeholders(source), placeholders(translation)),
        ));
    }
    if source.ends_with('…') != translation.ends_with('…') {
        errs.push(at(no, format!("a trailing `…` must be in both or neither: `{source}` / `{translation}`")));
    }
}

/// The rows of a catalog as (context, source) → translation, for the comparisons between locales.
fn rows(text: &str) -> BTreeMap<(&str, &str), &str> {
    lines(text).filter_map(|l| if let [c, s, t] = l.cols[..] { Some(((c, s), t)) } else { None }).collect()
}

/// Problems across catalogs: a regional locale's row that equals its base language's.
pub(crate) fn check_regional(catalogs: &BTreeMap<String, String>) -> Vec<String> {
    let mut errs = Vec::new();
    for (locale, text) in catalogs {
        let Some(base) = base_of(locale).and_then(|b| catalogs.get(b)) else { continue };
        let base_rows = rows(base);
        for (key, tr) in rows(text) {
            if key.0 != "@id" && base_rows.get(&key) == Some(&tr) {
                errs.push(format!("{locale}/{FILE}: `{}` repeats the base language's translation; delete the row (the app falls back)", key.1));
            }
        }
    }
    errs
}

/// Every catalog under `dir`, by locale; the layout's own problems come back as errors.
fn read_tree(dir: &Path) -> (BTreeMap<String, String>, Vec<String>) {
    let mut out = BTreeMap::new();
    let mut errs = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        if !path.is_dir() {
            errs.push(format!("locales/{name}: only locale directories live here"));
            continue;
        }
        if !is_locale_name(&name) {
            errs.push(format!("locales/{name}: locale directories are lowercase BCP 47 (`es`, `pt-br`, `zh-hans`)"));
        }
        for f in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            let file = f.file_name().to_string_lossy().into_owned();
            if file != FILE && !file.starts_with('.') {
                errs.push(format!("locales/{name}/{file}: the only catalog file is {FILE}"));
            }
        }
        match std::fs::read_to_string(path.join(FILE)) {
            Ok(text) => {
                out.insert(name, text);
            }
            Err(e) => errs.push(format!("locales/{name}/{FILE}: {e}")),
        }
    }
    (out, errs)
}

fn check_tree(dir: &Path) -> (BTreeMap<String, String>, Vec<String>) {
    let (catalogs, mut errs) = read_tree(dir);
    for (locale, text) in &catalogs {
        errs.extend(check_catalog(locale, text));
    }
    errs.extend(check_regional(&catalogs));
    (catalogs, errs)
}

#[test]
fn every_catalog_passes() {
    let (catalogs, errs) = check_tree(&locales_dir());
    assert!(!catalogs.is_empty(), "no catalogs under locales/");
    assert!(errs.is_empty(), "{} catalog problem(s):\n{}", errs.len(), errs.join("\n"));
}

/// Rows per catalog against the largest (always passes: it is a report).
#[test]
fn coverage_report() {
    let (catalogs, _) = read_tree(&locales_dir());
    let counts: Vec<(&String, usize)> = catalogs.iter().map(|(l, t)| (l, rows(t).keys().filter(|(c, _)| *c != "@id").count())).collect();
    let largest = counts.iter().map(|(_, n)| *n).max().unwrap_or(0).max(1);
    for (locale, n) in counts {
        println!("{locale:<10} {n:>5} rows  {:>3}% of the largest ({largest})", n * 100 / largest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "# Test (xx) interface catalog.\n# Clean room: written fresh.\n@id\tlanguage.name\tTest\n";

    fn errors(locale: &str, body: &str) -> String {
        check_catalog(locale, &format!("{HEADER}{body}")).join("\n")
    }

    #[test]
    fn a_good_catalog_passes() {
        assert_eq!(
            errors(
                "es",
                "\tSave\tGuardar\n\tOpen…\tAbrir…\nmenu\tOpen\tAbrir\n\tOpened {}\tAbierto {}\n@plural\t{} item|{} items\t{} elemento|{} elementos\n"
            ),
            ""
        );
        assert_eq!(errors("zh-hans", "@plural\t{} item|{} items\t{} 项\n"), "");
        assert_eq!(errors("fr", "@plural\t{} item|{} items\t{} élément|{} éléments\n@msg\tlong message\tmessage long\n"), "");
    }

    #[test]
    fn every_rule_catches_its_case() {
        let cases: [(&str, &str, &str); 14] = [
            ("es", "\tSave\n", "3 tab-separated columns"),
            ("es", "\tSave\tGuardar\ta\n", "3 tab-separated columns"),
            ("es", "\tSave\tGuard\\ar\n", "only `\\n`"),
            ("es", "\tSave\t\n", "empty translation"),
            ("es", "\tSave\tA\n\tSave\tB\n", "appears twice"),
            ("es", "@bogus\tSave\tA\n", "unknown context"),
            ("es", "\tOpened {}\tAbierto\n", "placeholders differ"),
            ("es", "\tOpen…\tAbrir\n", "trailing `…`"),
            ("es", "\tOpen\tAbrir…\n", "trailing `…`"),
            ("es", "@plural\t{} item|{} items\t{} elemento\n", "2 plural form"),
            ("zh-hans", "@plural\t{} item|{} items\t{} 项|{} 项\n", "1 plural form"),
            ("xx", "@plural\t{} item|{} items\t{} a|{} b\n", "no plural rule"),
            ("es", "@plural\t{} item\t{} a|{} b\n", "`one|other`"),
            ("es", "two words\tSave\tA\n", "one plain word"),
        ];
        for (locale, body, want) in cases {
            let e = errors(locale, body);
            assert!(e.contains(want), "{body:?} should report `{want}`, got: {e}");
        }
        assert!(check_catalog("es", "# Clean room: x\n\tSave\tGuardar\n").join("\n").contains("language.name"));
        assert!(check_catalog("es", "@id\tlanguage.name\tEspañol\n").join("\n").contains("Clean room"));
    }

    #[test]
    fn a_regional_row_equal_to_its_base_is_an_error() {
        let mut c = BTreeMap::new();
        c.insert("es".to_string(), format!("{HEADER}\tSave\tGuardar\n\tOpen\tAbrir\n"));
        c.insert("es-mx".to_string(), format!("{HEADER}\tSave\tGuardar\n\tOpen\tAbre\n"));
        c.insert("zh-hans".to_string(), format!("{HEADER}\tSave\tGuardar\n"));
        let e = check_regional(&c);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("es-mx") && e[0].contains("Save"));
    }

    #[test]
    fn locale_names_and_bases() {
        for ok in ["es", "pt-br", "zh-hans", "sr-latn"] {
            assert!(is_locale_name(ok), "{ok}");
        }
        for bad in ["ES", "pt_BR", "e", "pt-", "zh-Hans"] {
            assert!(!is_locale_name(bad), "{bad}");
        }
        assert_eq!(base_of("pt-br"), Some("pt"));
        assert_eq!(base_of("es-419"), Some("es"));
        assert_eq!(base_of("zh-hans"), None);
        assert_eq!(base_of("es"), None);
    }
}
