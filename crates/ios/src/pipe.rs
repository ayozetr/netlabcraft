//! Output filters after `|`: `include`, `exclude`, `begin`, `section` and `count`.
//!
//! Patterns are matched like the simple regular expressions used in labs: literal text, `^` at
//! the start and `$` at the end anchor it, and `.` stands for any one character.

/// Splits `show … | filter pattern` into the command and the filter (`None` when the line has no
/// filter, or isn't a command whose output can be filtered).
pub fn split(line: &str) -> Option<(&str, &str)> {
    let (cmd, filter) = line.split_once(" | ").or_else(|| line.split_once("| "))?;
    let first = cmd.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    let second = cmd.split_whitespace().nth(1).unwrap_or("").to_ascii_lowercase();
    let shows = |w: &str| w.len() >= 2 && "show".starts_with(w) || w == "more";
    (shows(&first) || (first == "do" && shows(&second))).then_some((cmd.trim_end(), filter.trim()))
}

fn matches_at(text: &[char], pat: &[char]) -> bool {
    text.len() >= pat.len() && text.iter().zip(pat).all(|(c, p)| *p == '.' || c == p)
}

/// Whether a line matches a pattern.
pub fn matches(line: &str, pattern: &str) -> bool {
    let start = pattern.starts_with('^');
    let end = pattern.len() > 1 && pattern.ends_with('$') && !pattern.ends_with("\\$");
    let core = pattern.trim_start_matches('^');
    let core = if end { core.strip_suffix('$').unwrap_or(core) } else { core };
    let text: Vec<char> = line.chars().collect();
    let pat: Vec<char> = core.chars().collect();
    if pat.is_empty() {
        return true;
    }
    let last = text.len().saturating_sub(pat.len());
    let starts: Vec<usize> = match (start, end) {
        (true, _) => vec![0],
        (false, true) => vec![last],
        (false, false) => (0..=last).collect(),
    };
    starts.into_iter().any(|i| {
        text.get(i..).is_some_and(|t| matches_at(t, &pat)) && (!end || i + pat.len() == text.len()) && (!start || !end || pat.len() == text.len())
    })
}

/// Applies a filter (`include ip`, `section interface`, …) to a command's output.
pub fn apply(output: &str, filter: &str) -> Result<String, String> {
    let (word, pattern) = filter.split_once(char::is_whitespace).map_or((filter, ""), |(w, p)| (w, p.trim()));
    let w = word.to_ascii_lowercase();
    let is = |full: &str| !w.is_empty() && full.starts_with(&w);
    if pattern.is_empty() && !is("count") {
        return Err("% Give the text to match after the filter".into());
    }
    let lines: Vec<&str> = output.lines().collect();
    let kept: Vec<&str> = if is("include") {
        lines.into_iter().filter(|l| matches(l, pattern)).collect()
    } else if is("exclude") {
        lines.into_iter().filter(|l| !matches(l, pattern)).collect()
    } else if is("begin") {
        lines.iter().position(|l| matches(l, pattern)).map_or(Vec::new(), |i| lines.get(i..).unwrap_or_default().to_vec())
    } else if is("section") {
        // A matching line and the indented lines under it.
        let mut out = Vec::new();
        let mut inside = false;
        for l in lines {
            let child = l.starts_with(' ');
            if !child {
                inside = matches(l, pattern);
            }
            if inside || (child && matches(l, pattern)) {
                out.push(l);
            }
        }
        out
    } else if is("count") {
        let n = lines.iter().filter(|l| matches(l, pattern)).count();
        return Ok(format!("Number of lines which match the pattern = {n}"));
    } else {
        return Err(format!("% `{word}`: include, exclude, begin, section or count"));
    };
    Ok(kept.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_follow_the_usual_meanings() {
        let run = "hostname R1\n!\ninterface GigabitEthernet0/0\n ip address 10.0.0.1 255.0.0.0\n no shutdown\n!\nline con 0\n login";
        assert_eq!(split("show running-config | include ip"), Some(("show running-config", "include ip")));
        assert_eq!(split("do sh run | sec line"), Some(("do sh run", "sec line")));
        assert_eq!(split("banner motd |x|"), None);
        assert_eq!(apply(run, "include address").unwrap(), " ip address 10.0.0.1 255.0.0.0");
        assert_eq!(apply(run, "i ^line").unwrap(), "line con 0");
        assert_eq!(apply(run, "section interface").unwrap(), "interface GigabitEthernet0/0\n ip address 10.0.0.1 255.0.0.0\n no shutdown");
        assert_eq!(apply(run, "begin line").unwrap(), "line con 0\n login");
        assert!(!apply(run, "exclude !").unwrap().contains('!'));
        assert_eq!(apply(run, "count shut").unwrap(), "Number of lines which match the pattern = 1");
        assert!(matches("GigabitEthernet0/0 up", "up$") && !matches("up GigabitEthernet", "up$"));
        assert!(matches("10.0.0.1", "10.0.0.1") && matches("10x0y0z1", "10.0.0.1"));
        assert!(apply(run, "include").is_err() && apply(run, "grep x").is_err());
    }
}
