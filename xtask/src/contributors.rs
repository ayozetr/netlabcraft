//! `cargo xtask contributors [--check]`: regenerate contributors/contributors.json from the git
//! history (craftrules standards/contributors.md; schema 1, the same fields its contributors.py
//! writes). The About window bakes it in through crates/ui-egui/build.rs.
//!
//! - A commit counts for a GitHub username when its author email is the username's GitHub noreply
//!   address (`12345+login@users.noreply.github.com` or `login@users.noreply.github.com`); other
//!   commits are counted as unattributed rather than guessed.
//! - Co-authors named in `Co-authored-by:` trailers (with a noreply email) are credited with the
//!   commit too: its count, its dates and its lines, as a shared commit can't be split.
//! - `prs` counts merge commits titled `Merge pull request #N from login/…`.
//! - `people_entry` is true only when every commit that added or removed `[people.login]` in
//!   people.toml was authored by that same username: nobody can name someone else.
//! - `github_name` stays null: it needs the network, and only `sync_github_name` uses it.
//!
//! `--check` fails when the committed file is out of date (ignoring head and head_date).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

#[derive(Default)]
struct Stats {
    prs: u64,
    commits: u64,
    lines_added: u64,
    lines_deleted: u64,
    binary_added: u64,
    binary_deleted: u64,
    first: String,
    last: String,
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let path = root.join("contributors/contributors.json");
    let fresh = generate(root)?;
    let text = serde_json::to_string_pretty(&fresh).map_err(|e| e.to_string())? + "\n";
    if args.contains(&"--check") {
        let old: Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let strip = |mut v: Value| {
            if let Some(o) = v.as_object_mut() {
                o.remove("head");
                o.remove("head_date");
            }
            v
        };
        if strip(old) != strip(fresh) {
            return Err("contributors/contributors.json is out of date: run `cargo xtask contributors`".into());
        }
        println!("contributors.json is up to date");
        return Ok(());
    }
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The usernames credited with a commit: its author's, then each `Co-authored-by: Name <email>`
/// trailer's (`trailers` holds the values, \x03-separated), once each, noreply emails only.
fn credited(author: &str, trailers: &str) -> Vec<String> {
    let mut out: Vec<String> = login(author).into_iter().collect();
    for t in trailers.split('\x03') {
        let email = t.rsplit_once('<').and_then(|(_, r)| r.split_once('>')).map_or("", |(e, _)| e.trim());
        if let Some(l) = login(email)
            && !out.iter().any(|o| o.eq_ignore_ascii_case(&l))
        {
            out.push(l);
        }
    }
    out
}

/// The GitHub username in a noreply author email, if it is one.
fn login(email: &str) -> Option<String> {
    // The domain is matched case-insensitively; the username keeps its own case, as GitHub shows it.
    let at = email.len().checked_sub("@users.noreply.github.com".len())?;
    if !email.get(at..)?.eq_ignore_ascii_case("@users.noreply.github.com") {
        return None;
    }
    let local = email.get(..at)?.to_string();
    let name = local.split_once('+').map_or(local.as_str(), |(_, l)| l).to_string();
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then_some(name)
}

fn generate(root: &Path) -> Result<Value, String> {
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    let head = git(root, &["rev-parse", "HEAD"])?.trim().to_string();
    let head_date = git(root, &["log", "-1", "--format=%cI", "--date=iso-strict"])?.trim().to_string();
    let mut people: BTreeMap<String, Stats> = BTreeMap::new();
    let (mut total, mut unattributed) = (0u64, 0u64);
    // One header line per commit (\x01 marks it: author email, date, co-authors), then its
    // --numstat and --summary lines.
    let log = git(
        root,
        &[
            "log",
            "--no-merges",
            "--reverse",
            "--numstat",
            "--summary",
            "--format=\x01%ae\x02%aI\x02%(trailers:key=Co-authored-by,valueonly,separator=%x03)",
        ],
    )?;
    // Everyone credited with the commit being read: its author and its co-authors.
    let mut who: Vec<String> = Vec::new();
    let mut binary: Vec<String> = Vec::new();
    for line in log.lines() {
        if let Some(h) = line.strip_prefix('\x01') {
            total += 1;
            binary.clear();
            let mut parts = h.split('\x02');
            let email = parts.next().unwrap_or("");
            let date = utc(parts.next().unwrap_or(""));
            who = credited(email, parts.next().unwrap_or(""));
            if who.is_empty() {
                unattributed += 1;
            }
            for l in &who {
                let s = people.entry(l.clone()).or_default();
                s.commits += 1;
                if s.first.is_empty() {
                    s.first = date.clone();
                }
                s.last = date.clone();
            }
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        for l in &who {
            let Some(s) = people.get_mut(l) else { continue };
            if let [a, d, file] = cols.as_slice() {
                if *a == "-" && *d == "-" {
                    if !binary.iter().any(|b| b == file) {
                        binary.push((*file).to_string());
                    }
                } else {
                    s.lines_added += a.parse::<u64>().unwrap_or(0);
                    s.lines_deleted += d.parse::<u64>().unwrap_or(0);
                }
            } else if let Some(rest) = line.trim().strip_prefix("create mode ") {
                if binary.iter().any(|b| rest.ends_with(b.as_str())) {
                    s.binary_added += 1;
                }
            } else if let Some(rest) = line.trim().strip_prefix("delete mode ")
                && binary.iter().any(|b| rest.ends_with(b.as_str()))
            {
                s.binary_deleted += 1;
            }
        }
    }
    for subject in git(root, &["log", "--merges", "--format=%s"])?.lines() {
        if let Some(rest) = subject.strip_prefix("Merge pull request #")
            && let Some((_, from)) = rest.split_once(" from ")
            && let Some((l, _)) = from.split_once('/')
        {
            people.entry(l.to_ascii_lowercase()).or_default().prs += 1;
        }
    }
    let mut rows: Vec<(&String, &Stats)> = people.iter().collect();
    rows.sort_by(|a, b| (a.1.first.is_empty(), &a.1.first, a.0).cmp(&(b.1.first.is_empty(), &b.1.first, b.0)));
    let mut contributors = Vec::new();
    for (l, s) in rows {
        contributors.push(json!({
            "login": l,
            "people_entry": verified(root, l)?,
            "github_name": null,
            "prs": s.prs,
            "commits": s.commits,
            "lines_added": s.lines_added,
            "lines_deleted": s.lines_deleted,
            "binary_added": s.binary_added,
            "binary_deleted": s.binary_deleted,
            "first_commit": s.first,
            "last_commit": s.last,
        }));
    }
    Ok(json!({
        "schema": 1,
        "repo": "ayozetr/netlabcraft",
        "branch": branch,
        "head": head,
        "head_date": utc(&head_date),
        "commits": total,
        "unattributed_commits": unattributed,
        "contributors": contributors,
    }))
}

/// Whether `login`'s people.toml entry was only ever written by `login`.
fn verified(root: &Path, login_name: &str) -> Result<bool, String> {
    let needle = format!("[people.{login_name}]");
    let authors = git(root, &["log", "--format=%ae", "-S", &needle, "--", "contributors/people.toml"])?;
    let mut any = false;
    for email in authors.lines() {
        if login(email).as_deref() != Some(login_name) {
            return Ok(false);
        }
        any = true;
    }
    Ok(any)
}

/// `2026-10-30T12:00:00+01:00` → `2026-10-30T11:00:00Z` (whole-hour/minute offsets).
fn utc(iso: &str) -> String {
    let (Some(date), Some(time)) = (iso.get(..10), iso.get(11..19)) else { return String::new() };
    let off = iso.get(19..).unwrap_or("");
    let sign: i64 = if off.starts_with('-') { -1 } else { 1 };
    let mins = off.get(1..3).and_then(|h| h.parse::<i64>().ok()).unwrap_or(0) * 60 + off.get(4..6).and_then(|m| m.parse::<i64>().ok()).unwrap_or(0);
    let parse = |r: std::ops::Range<usize>, s: &str| s.get(r).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
    let (y, mo, d) = (parse(0..4, date), parse(5..7, date), parse(8..10, date));
    let secs = days(y, mo, d) * 86_400 + parse(0..2, time) * 3600 + parse(3..5, time) * 60 + parse(6..8, time) - sign * mins * 60;
    let (day_n, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, mo, d) = civil(day_n);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Days since 1970-01-01 (proleptic Gregorian).
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noreply_logins() {
        assert_eq!(login("53948812+ayozetr@users.noreply.github.com").as_deref(), Some("ayozetr"));
        assert_eq!(login("Someone@Users.Noreply.GitHub.com").as_deref(), Some("Someone"));
        assert_eq!(login("person@example.com"), None);
        assert_eq!(login("1+bad/name@users.noreply.github.com"), None);
    }

    #[test]
    fn co_authors_share_the_credit() {
        let trailers = "Aimar Mendez <93106365+Aimarmndz@users.noreply.github.com>\x03Someone <someone@example.com>\x03Again <1+ayozetr@users.noreply.github.com>";
        assert_eq!(credited("53948812+ayozetr@users.noreply.github.com", trailers), ["ayozetr", "Aimarmndz"]);
        assert_eq!(credited("person@example.com", ""), Vec::<String>::new());
        assert_eq!(credited("person@example.com", "X <2+x@users.noreply.github.com>"), ["x"]);
        assert_eq!(credited("1+a@users.noreply.github.com", "broken <no-close"), ["a"]);
    }

    #[test]
    fn dates_become_utc() {
        assert_eq!(utc("2026-10-30T00:30:00+01:00"), "2026-10-29T23:30:00Z");
        assert_eq!(utc("2026-12-31T23:00:00-02:00"), "2027-01-01T01:00:00Z");
        assert_eq!(utc("2024-02-29T12:00:00Z"), "2024-02-29T12:00:00Z");
        assert_eq!(utc(""), "");
    }
}
