//! A small, bounded pattern matcher for the regular expressions routers use on AS paths and
//! community lists (`ip as-path access-list`, `ip community-list expanded`).
//!
//! It supports what those lists use in practice: `^` and `$` anchors, `_` (a boundary: the start
//! or the end of the text, a space, a comma, a brace or a parenthesis), `.` for any character,
//! the repeats `*`, `+` and `?` after one character, a bracket class like `[0-9]` or `[^5]`, and
//! `\` to take the next character literally. Alternation and groups are not supported (`(` and
//! `)` match themselves). A pattern is not anchored unless it says so, and a search that takes
//! more than [`MAX_STEPS`] steps counts as no match, so a hostile pattern cannot hang the
//! simulator.

/// Longest pattern and text looked at.
pub const MAX_PATTERN: usize = 64;
pub const MAX_TEXT: usize = 512;
/// Most steps one search may take.
pub const MAX_STEPS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Atom {
    Start,
    End,
    Boundary,
    Any,
    Char(char),
    /// Ranges (inclusive) and whether the class is negated.
    Class(Vec<(char, char)>, bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rep {
    One,
    Star,
    Plus,
    Opt,
}

fn is_separator(c: char) -> bool {
    matches!(c, ' ' | ',' | '{' | '}' | '(' | ')')
}

/// Parses a pattern; `None` when it is empty, too long or malformed (an unclosed `[`).
fn parse(pattern: &str) -> Option<Vec<(Atom, Rep)>> {
    let chars: Vec<char> = pattern.chars().collect();
    if chars.is_empty() || chars.len() > MAX_PATTERN {
        return None;
    }
    let mut out: Vec<(Atom, Rep)> = Vec::new();
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        i += 1;
        let atom = match c {
            '^' => Atom::Start,
            '$' => Atom::End,
            '_' => Atom::Boundary,
            '.' => Atom::Any,
            '\\' => {
                let n = *chars.get(i)?;
                i += 1;
                Atom::Char(n)
            }
            '[' => {
                let mut negate = false;
                if chars.get(i) == Some(&'^') {
                    negate = true;
                    i += 1;
                }
                let mut ranges = Vec::new();
                loop {
                    let lo = *chars.get(i)?;
                    i += 1;
                    if lo == ']' && !ranges.is_empty() {
                        break;
                    }
                    if chars.get(i) == Some(&'-') && chars.get(i + 1).is_some_and(|x| *x != ']') {
                        let hi = *chars.get(i + 1)?;
                        i += 2;
                        ranges.push((lo.min(hi), lo.max(hi)));
                    } else {
                        ranges.push((lo, lo));
                    }
                }
                Atom::Class(ranges, negate)
            }
            other => Atom::Char(other),
        };
        let rep = match chars.get(i) {
            Some('*') => Rep::Star,
            Some('+') => Rep::Plus,
            Some('?') => Rep::Opt,
            _ => Rep::One,
        };
        let zero_width = matches!(atom, Atom::Start | Atom::End);
        if rep != Rep::One && !zero_width {
            i += 1;
            out.push((atom, rep));
        } else {
            if rep != Rep::One {
                // A repeat after an anchor means nothing; skip it.
                i += 1;
            }
            out.push((atom, Rep::One));
        }
    }
    Some(out)
}

/// Whether a one-character atom accepts `c`.
fn takes(atom: &Atom, c: char) -> bool {
    match atom {
        Atom::Any => true,
        Atom::Char(x) => *x == c,
        Atom::Class(ranges, negate) => ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)) != *negate,
        _ => false,
    }
}

fn run(items: &[(Atom, Rep)], text: &[char], pos: usize, steps: &mut usize) -> bool {
    *steps += 1;
    if *steps > MAX_STEPS {
        return false;
    }
    let Some(((atom, rep), rest)) = items.split_first() else { return true };
    match atom {
        Atom::Start => pos == 0 && run(rest, text, pos, steps),
        Atom::End => pos == text.len() && run(rest, text, pos, steps),
        Atom::Boundary => {
            if (pos == 0 || pos == text.len()) && run(rest, text, pos, steps) {
                return true;
            }
            text.get(pos).is_some_and(|c| is_separator(*c)) && run(rest, text, pos + 1, steps)
        }
        _ => {
            let (min, max) = match rep {
                Rep::One => (1usize, 1usize),
                Rep::Star => (0, usize::MAX),
                Rep::Plus => (1, usize::MAX),
                Rep::Opt => (0, 1),
            };
            let mut n = 0usize;
            while n < max && text.get(pos + n).is_some_and(|c| takes(atom, *c)) {
                n += 1;
            }
            loop {
                if n < min {
                    return false;
                }
                if run(rest, text, pos + n, steps) {
                    return true;
                }
                if n == 0 {
                    return false;
                }
                n -= 1;
            }
        }
    }
}

/// Whether `pattern` matches somewhere in `text` (anchors aside). A pattern that is empty, too
/// long or malformed matches nothing.
pub fn is_match(pattern: &str, text: &str) -> bool {
    let Some(items) = parse(pattern) else { return false };
    let text: Vec<char> = text.chars().take(MAX_TEXT).collect();
    let mut steps = 0usize;
    (0..=text.len()).any(|start| run(&items, &text, start, &mut steps))
}

/// Whether a pattern is one `is_match` can use.
pub fn valid(pattern: &str) -> bool {
    parse(pattern).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_and_boundaries() {
        assert!(is_match("^65001$", "65001"));
        assert!(!is_match("^65001$", "65001 65002"));
        assert!(is_match("^65001_", "65001 65002"));
        assert!(is_match("_65002$", "65001 65002"));
        assert!(is_match("_65002_", "65001 65002 65003"));
        assert!(!is_match("_6500_", "65001"));
        assert!(is_match("^$", ""));
        assert!(!is_match("^$", "65001"));
        assert!(is_match("_65001_65002_", "65001 65002"));
        assert!(is_match("_65001_", "{65001,65002}"));
    }

    #[test]
    fn dots_repeats_and_classes() {
        assert!(is_match(".*", ""));
        assert!(is_match(".*", "anything at all"));
        assert!(is_match("^[0-9]+$", "65001"));
        assert!(!is_match("^[0-9]+$", "65x01"));
        assert!(is_match("^6500[12]$", "65002"));
        assert!(!is_match("^6500[^2]$", "65002"));
        assert!(is_match("^65.*3$", "65001 65003"));
        assert!(is_match("^650?1$", "651") && is_match("^650?1$", "6501"));
        assert!(is_match(r"^1\.2$", "1.2") && !is_match(r"^1\.2$", "132"));
    }

    #[test]
    fn a_bad_or_hostile_pattern_matches_nothing_and_stops() {
        assert!(!is_match("", "x"));
        assert!(!is_match("[abc", "a"));
        assert!(!is_match(&"a".repeat(MAX_PATTERN + 1), "a"));
        assert!(!valid("[") && valid("^1_"));
        // Nested repeats of the same character would explode; the step budget ends the search.
        let text = "a".repeat(MAX_TEXT);
        let _ = is_match(&format!("{}b", "a*".repeat(30)), &text);
    }
}
