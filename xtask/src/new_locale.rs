//! `cargo xtask new-locale <code> "<native name>"`: start a language.
//!
//! Writes `locales/<code>/netlabcraft.tsv` with the header (copied from the `es` catalog, so it
//! never drifts), the `@id language.name` row, and every English string the other catalogs have,
//! commented out as `#<context><TAB><English><TAB>`. Delete the `#` and type the translation after
//! the last TAB; rows left commented show English. A catalog that exists is never overwritten.

use std::collections::BTreeSet;
use std::path::Path;

/// Lowercase BCP 47 as the checker wants it: `fr`, `pt-br`, `zh-hans`.
fn valid_code(s: &str) -> bool {
    let mut parts = s.split('-');
    let lang = parts.next().unwrap_or_default();
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// The catalog text of a new locale, from the existing catalogs' texts (the first one lends the header).
pub fn starter(code: &str, name: &str, existing: &[String]) -> String {
    let mut out = String::new();
    let header: Vec<&str> = existing.first().map(|t| t.lines().take_while(|l| l.starts_with('#')).collect()).unwrap_or_default();
    for (i, line) in header.iter().enumerate() {
        if i == 0 {
            out.push_str(&format!("# {name} ({code}) interface catalog for NetLabCraft.\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(&format!("\n@id\tlanguage.name\t{name}\n\n"));
    let mut keys = BTreeSet::new();
    for text in existing {
        for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let mut cols = line.split('\t');
            if let (Some(context), Some(source)) = (cols.next(), cols.next())
                && context != "@id"
            {
                keys.insert((context.to_string(), source.to_string()));
            }
        }
    }
    let mut rows: Vec<_> = keys.into_iter().collect();
    // Plain rows first, then the contexts; each alphabetically by English.
    rows.sort_by(|a, b| (!a.0.is_empty(), a.1.to_lowercase(), &a.1, &a.0).cmp(&(!b.0.is_empty(), b.1.to_lowercase(), &b.1, &b.0)));
    for (context, source) in rows {
        out.push_str(&format!("#{context}\t{source}\t\n"));
    }
    out
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let [code, name] = args else {
        return Err("usage: cargo xtask new-locale <code> \"<the language's name, in that language>\"".into());
    };
    if !valid_code(code) {
        return Err(format!("`{code}`: write the locale as lowercase BCP 47, e.g. fr, pt-br, zh-hans"));
    }
    if name.contains(['\t', '\n']) || name.trim().is_empty() {
        return Err("the language name is one line of text".into());
    }
    let dir = root.join("locales");
    let target = dir.join(code).join("netlabcraft.tsv");
    if target.exists() {
        return Err(format!("{} exists; it is left as it is", target.display()));
    }
    // `es` first: its header is the one every catalog starts with.
    let mut codes: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter(|e| e.path().join("netlabcraft.tsv").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    codes.sort_by_key(|c| (c != "es", c.clone()));
    let existing: Vec<String> = codes.iter().filter_map(|c| std::fs::read_to_string(dir.join(c).join("netlabcraft.tsv")).ok()).collect();
    let text = starter(code, name, &existing);
    let todo = text.lines().filter(|l| l.starts_with('#') && l.ends_with('\t')).count();
    std::fs::create_dir_all(target.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
    std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
    println!("{}: {todo} strings to translate", target.strip_prefix(root).unwrap_or(&target).display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        for ok in ["fr", "pt-br", "zh-hans"] {
            assert!(valid_code(ok), "{ok}");
        }
        for bad in ["FR", "pt_br", "f", "", "fr-", "zh-Hans"] {
            assert!(!valid_code(bad), "{bad}");
        }
    }

    #[test]
    fn the_starter_has_the_header_the_name_and_every_key_commented_out() {
        let es = "# Español (es) interface catalog for NetLabCraft.\n# Columns\n#\n# Clean room: x\n\n@id\tlanguage.name\tEspañol\n\tSave\tGuardar\n\tOpen\tAbrir\nevent\ttell\tresponde a\n@plural\t{} a|{} b\t{} c|{} d\n".to_string();
        let t = starter("fr", "Français", &[es]);
        assert!(t.starts_with("# Français (fr) interface catalog for NetLabCraft.\n# Columns\n#\n# Clean room: x\n"));
        assert!(t.contains("\n@id\tlanguage.name\tFrançais\n"));
        for row in ["#\tOpen\t\n", "#\tSave\t\n", "#event\ttell\t\n", "#@plural\t{} a|{} b\t\n"] {
            assert!(t.contains(row), "{row}");
        }
        assert!(t.find("#\tOpen").unwrap_or(0) < t.find("#\tSave").unwrap_or(0), "sorted by English");
        assert!(!t.contains("Guardar"), "no Spanish text");
    }
}
