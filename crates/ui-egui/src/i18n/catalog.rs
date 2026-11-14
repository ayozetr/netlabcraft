//! The catalog format (docs/translating.md): rows, escapes, plural rules.
//!
//! A catalog is `locales/<locale>/netlabcraft.tsv`, one row per line, three columns separated by a
//! TAB: `context<TAB>English<TAB>translation`. Lines starting with `#` and blank lines are ignored.

use std::collections::HashMap;

/// A row of a catalog file, columns as written (escapes not yet resolved).
pub(crate) struct Line<'a> {
    /// 1-based line number in the file (for the checker's messages).
    #[cfg_attr(not(test), allow(dead_code))]
    pub no: usize,
    pub cols: Vec<&'a str>,
}

/// The data lines of a catalog: comments and blank lines skipped, a byte-order mark and Windows
/// line endings ignored.
pub(crate) fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.split('\n').enumerate().filter_map(|(i, l)| {
        let l = l.strip_suffix('\r').unwrap_or(l);
        (!l.starts_with('#') && !l.trim().is_empty()).then(|| Line { no: i + 1, cols: l.split('\t').collect() })
    })
}

#[cfg(test)]
/// Contexts that start with `@` and mean something to the program.
pub(crate) const KNOWN_CONTEXTS: [&str; 3] = ["@plural", "@id", "@msg"];

/// The `@id` source whose translation is the language's own name.
pub(crate) const LANGUAGE_NAME_ID: &str = "language.name";

/// A column with its `\n`, `\t` and `\\` escapes resolved (kept for the program's life: the
/// catalogs are parsed once).
pub(crate) fn unescape(s: &'static str) -> &'static str {
    if !s.contains('\\') {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Box::leak(out.into_boxed_str())
}

/// One locale's parsed rows.
#[derive(Default)]
pub(crate) struct Catalog {
    /// Rows with no context, and `@msg` rows: English → translation.
    pub plain: HashMap<&'static str, &'static str>,
    /// Rows with a context (`event`, `menu`…): (context, English) → translation.
    pub context: HashMap<(&'static str, &'static str), &'static str>,
    /// `@plural` rows: `one|other` English → the language's forms.
    pub plural: HashMap<&'static str, Vec<&'static str>>,
    /// `@id` rows: id → text.
    pub ids: HashMap<&'static str, &'static str>,
}

impl Catalog {
    pub fn parse(text: &'static str) -> Catalog {
        let mut c = Catalog::default();
        for line in lines(text) {
            let [context, source, translation] = line.cols[..] else { continue };
            if translation.trim().is_empty() || source.is_empty() {
                continue;
            }
            let (source, translation) = (unescape(source), unescape(translation));
            match context {
                "" | "@msg" => {
                    c.plain.insert(source, translation);
                }
                "@id" => {
                    c.ids.insert(source, translation);
                }
                "@plural" => {
                    c.plural.insert(source, translation.split('|').collect());
                }
                other if other.starts_with('@') => {}
                other => {
                    c.context.insert((other, source), translation);
                }
            }
        }
        c
    }

    /// The language's own name (`@id language.name`).
    pub fn name(&self) -> Option<&'static str> {
        self.ids.get(LANGUAGE_NAME_ID).copied()
    }

    /// How many translated rows the catalog has.
    pub fn len(&self) -> usize {
        self.plain.len() + self.context.len() + self.plural.len()
    }
}

#[cfg(test)]
/// How many plural forms a locale lists in an `@plural` row (CLDR cardinal categories that whole
/// numbers 0 to 1000 reach), for the locales NetLabCraft knows. `None`: unknown, so add its rule
/// here before shipping a plural row for it.
pub(crate) fn plural_forms(locale: &str) -> Option<usize> {
    match base_language(locale) {
        "es" | "fr" | "it" | "pt" | "de" | "en" | "nl" | "ca" | "gl" | "sv" | "nb" | "da" | "fi" | "el" | "tr" | "hu" | "bg" | "et" => Some(2),
        "zh" | "ja" | "ko" | "vi" | "th" | "id" => Some(1),
        _ => None,
    }
}

/// Which plural form (index into the `@plural` translation) a count takes.
pub(crate) fn plural_index(locale: &str, n: u64) -> usize {
    let singular = match locale {
        "pt-pt" => n == 1,
        _ => match base_language(locale) {
            // French and Portuguese call 0 singular too.
            "fr" | "pt" => n <= 1,
            "zh" | "ja" | "ko" | "vi" | "th" | "id" => return 0,
            _ => n == 1,
        },
    };
    usize::from(!singular)
}

/// `pt` for `pt-br`.
pub(crate) fn base_language(locale: &str) -> &str {
    locale.split('-').next().unwrap_or(locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_comments_and_escapes() {
        let text: &'static str = "\u{feff}# header\r\n\r\n\ta\tb\r\nmenu\tOpen\tAbrir\n\tone\\ntwo\tuno\\ndos\n\tx\t\n@id\tlanguage.name\tEsp\n@plural\t{} page|{} pages\t{} pág|{} págs\n@future\ta\tb\n\tt\\\\n\ttab\\t\n";
        let c = Catalog::parse(text);
        assert_eq!(c.plain.get("a"), Some(&"b"));
        assert_eq!(c.plain.get("one\ntwo"), Some(&"uno\ndos"));
        assert_eq!(c.context.get(&("menu", "Open")), Some(&"Abrir"));
        assert_eq!(c.name(), Some("Esp"));
        assert_eq!(c.plural.get("{} page|{} pages").map(Vec::len), Some(2));
        assert_eq!(c.plain.get("x"), None, "an empty translation is no row");
        assert_eq!(c.plain.get("t\\n"), Some(&"tab\t"), "an escaped backslash stays one");
        assert_eq!(c.len(), 5);
    }

    #[test]
    fn plural_rules() {
        assert_eq!([0, 1, 2].map(|n| plural_index("es", n)), [1, 0, 1]);
        assert_eq!([0, 1, 2].map(|n| plural_index("fr", n)), [0, 0, 1]);
        assert_eq!([0, 1, 2].map(|n| plural_index("pt-br", n)), [0, 0, 1]);
        assert_eq!([0, 1, 2].map(|n| plural_index("pt-pt", n)), [1, 0, 1]);
        assert_eq!([0, 1, 5].map(|n| plural_index("zh-hans", n)), [0, 0, 0]);
        assert_eq!(plural_forms("pt-br"), Some(2));
        assert_eq!(plural_forms("zh-hans"), Some(1));
        assert_eq!(plural_forms("xx"), None);
    }
}
