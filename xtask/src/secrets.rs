//! `cargo xtask secrets [--history]`: no keys, tokens, private keys or local paths in what git would commit.
//!
//! The repository is public. This scans every tracked and untracked-but-not-ignored text file (or, with
//! `--history`, every line added in any commit) for credential shapes and for paths that reveal a person's
//! machine. Only obvious fakes (`sk-test`) belong in tests. A line carrying `secrets:allow` is skipped.

use std::path::Path;
use std::process::Command;

/// File names that are secrets by themselves.
const SECRET_FILES: &[&str] = &[".env", "id_rsa", "id_ed25519", "id_ecdsa"];
const SECRET_EXT: &[&str] = &["pem", "key", "p12", "pfx", "keystore"];
/// Home-directory names that are placeholders, not people.
const PLACEHOLDER_USERS: &[&str] = &["user", "you", "me", "name", "username", "runner", "example", "foo", "<user>", "$user", "yourname"];
const MAX_FILE: u64 = 4 << 20;

/// `prefix` followed by at least `min` characters of `ok`.
fn has_token(line: &str, prefix: &str, min: usize, ok: impl Fn(char) -> bool) -> bool {
    line.match_indices(prefix).any(|(i, _)| line.get(i + prefix.len()..).is_some_and(|rest| rest.chars().take_while(|c| ok(*c)).count() >= min))
}

fn local_path(line: &str) -> bool {
    for marker in ["/home/", "/Users/", "C:\\Users\\", "C:/Users/"] {
        for (i, _) in line.match_indices(marker) {
            let name: String =
                line.get(i + marker.len()..).unwrap_or_default().chars().take_while(|c| !matches!(c, '/' | '\\' | ' ' | '"' | '\'' | '`')).collect();
            if !name.is_empty() && !PLACEHOLDER_USERS.contains(&name.to_ascii_lowercase().as_str()) {
                return true;
            }
        }
    }
    false
}

/// What is wrong with one line of text, if anything.
pub fn scan_line(line: &str) -> Option<&'static str> {
    if line.contains("secrets:allow") {
        return None;
    }
    let alnum = |c: char| c.is_ascii_alphanumeric();
    if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
        return Some("private key");
    }
    if has_token(line, "sk-", 20, |c| alnum(c) || c == '-' || c == '_') {
        return Some("API key (sk-…)");
    }
    if ["ghp_", "gho_", "ghs_", "ghu_", "github_pat_"].iter().any(|p| has_token(line, p, 30, |c| alnum(c) || c == '_')) {
        return Some("GitHub token");
    }
    if has_token(line, "AKIA", 16, |c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        return Some("AWS access key id");
    }
    if ["xoxb-", "xoxp-", "xoxa-"].iter().any(|p| has_token(line, p, 20, |c| alnum(c) || c == '-')) {
        return Some("Slack token");
    }
    if has_token(line, "AIza", 35, |c| alnum(c) || c == '-' || c == '_') {
        return Some("Google API key");
    }
    if local_path(line) {
        return Some("local path of a person's machine");
    }
    None
}

/// A path that must never be committed.
pub fn secret_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = Path::new(name).extension().and_then(std::ffi::OsStr::to_str).unwrap_or_default().to_ascii_lowercase();
    SECRET_FILES.contains(&name) || name.starts_with(".env.") && name != ".env.example" || SECRET_EXT.contains(&ext.as_str())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").current_dir(root).args(args).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let history = args.contains(&"--history");
    let mut hits: Vec<String> = Vec::new();
    if history {
        let log = git(root, &["log", "--all", "-p", "-U0", "--no-color", "--format=commit %h"])?;
        let (mut commit, mut file) = (String::new(), String::new());
        for line in log.lines() {
            if let Some(h) = line.strip_prefix("commit ") {
                commit = h.to_owned();
            } else if let Some(f) = line.strip_prefix("+++ b/") {
                file = f.to_owned();
                if secret_file(&file) {
                    hits.push(format!("{commit} {file}: secret file"));
                }
            } else if let Some(added) = line.strip_prefix('+')
                && !file.starts_with("xtask/src/secrets.rs")
                && let Some(kind) = scan_line(added)
            {
                hits.push(format!("{commit} {file}: {kind}"));
            }
        }
    } else {
        let list = git(root, &["ls-files", "--cached", "--others", "--exclude-standard"])?;
        for file in list.lines() {
            let path = root.join(file);
            if !path.is_file() {
                continue;
            }
            if secret_file(file) {
                hits.push(format!("{file}: secret file"));
                continue;
            }
            if file == "xtask/src/secrets.rs" || file == "Cargo.lock" || path.metadata().map_or(true, |m| m.len() > MAX_FILE) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for (n, line) in text.lines().enumerate() {
                if let Some(kind) = scan_line(line) {
                    hits.push(format!("{file}:{}: {kind}", n + 1));
                }
            }
        }
    }
    if hits.is_empty() {
        println!("secrets: nothing found{}", if history { " in any commit" } else { "" });
        Ok(())
    } else {
        Err(format!(
            "{} possible secret(s) or personal path(s):\n  {}\n(add `secrets:allow` to a line that is a deliberate fake)",
            hits.len(),
            hits.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_credential_shapes() {
        assert!(scan_line(&format!("key = \"sk-{}\"", "a".repeat(30))).is_some());
        assert!(scan_line(&format!("t = {}{}", "ghp_", "b".repeat(36))).is_some());
        assert!(scan_line(&format!("{} RSA PRIVATE KEY-----", "-----BEGIN")).is_some());
        assert!(scan_line(&format!("id {}{}", "AKIA", "A".repeat(16))).is_some());
    }

    #[test]
    fn obvious_fakes_and_placeholders_pass() {
        assert_eq!(scan_line("let key = \"sk-test\";"), None);
        assert_eq!(scan_line("cd /home/user/project"), None);
        assert_eq!(scan_line("/home/runner/work"), None);
        assert_eq!(scan_line("a deliberate fake sk-aaaaaaaaaaaaaaaaaaaaaaaaaaaa secrets:allow"), None);
    }

    #[test]
    fn flags_a_persons_home_directory() {
        assert!(scan_line(&format!("/home/{}/dev/x", "maria")).is_some());
        assert!(scan_line(&format!("C:\\Users\\{}\\Desktop", "Maria")).is_some());
    }

    #[test]
    fn secret_file_names() {
        assert!(secret_file(".env"));
        assert!(secret_file("deploy/server.pem"));
        assert!(!secret_file(".env.example"));
        assert!(!secret_file("src/main.rs"));
    }
}
