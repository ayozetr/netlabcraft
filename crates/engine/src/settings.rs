//! What a session holds besides the lab: the command log (`log.list`, `log.export`, `log.clear`,
//! `log.enable`), the local profile (`profile.get`, `profile.set`) and whether cable lengths
//! count (`physical.length_effects`).
//!
//! The log and the profile belong to the person at the keyboard, not to the lab: opening or
//! starting a lab keeps them. Neither is saved in a lab file. The profile is one display name
//! kept on this computer; it names the student in activity results when the activity has not been
//! started with a name, and it is never sent anywhere.

use std::collections::VecDeque;

use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// Most entries the command log keeps; the oldest go first.
pub const MAX_LOG: usize = 2000;
/// Longest the parameters of one entry are written, in characters.
const MAX_PARAMS_CHARS: usize = 400;
/// Longest the error of one entry is written, in characters.
const MAX_ERROR_CHARS: usize = 300;
/// Longest a string inside the parameters is written.
const MAX_VALUE_CHARS: usize = 120;
/// Most items of an array or keys of an object written.
const MAX_ITEMS: usize = 16;
/// Deepest nesting written.
const MAX_DEPTH: usize = 6;
/// Longest display name, in characters.
pub const MAX_NAME: usize = 64;
/// The name `log.export` suggests for the file.
pub const LOG_FILE_NAME: &str = "netlabcraft-command-log.txt";

/// Commands that fire many times while one thing is dragged: a run of them on the same target is
/// one entry (with a count), so a drag does not push the rest of the history out.
const DRAGS: &[&str] = &["device.move", "place.move", "note.move", "note.resize", "cluster.move", "device.place"];

/// One command that ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Its number in this session, from 1.
    pub n: u64,
    pub command: String,
    /// The parameters as compact JSON, shortened, with passwords and keys hidden.
    pub params: String,
    pub ok: bool,
    /// Why it failed.
    pub error: Option<String>,
    /// The simulated time when it finished, in microseconds.
    pub sim_us: u64,
    /// How many times in a row (a drag is one entry).
    pub count: u32,
    drag: Option<String>,
}

/// The commands run this session, newest last.
#[derive(Debug, Clone)]
pub struct CommandLog {
    /// Whether new commands are recorded.
    pub enabled: bool,
    entries: VecDeque<LogEntry>,
    next: u64,
}

impl Default for CommandLog {
    fn default() -> Self {
        CommandLog { enabled: true, entries: VecDeque::new(), next: 1 }
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// A parameter whose value is a secret: it is written as `***`.
fn secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["password", "secret", "passphrase", "key", "community", "token"].iter().any(|s| k.contains(s))
}

/// The parameters made short and safe to keep: long strings and lists cut, secrets hidden.
fn tidy(v: &Value, depth: usize) -> Value {
    match v {
        _ if depth >= MAX_DEPTH => Value::String("…".into()),
        Value::String(s) => Value::String(clip(s, MAX_VALUE_CHARS)),
        Value::Array(a) => {
            let mut out: Vec<Value> = a.iter().take(MAX_ITEMS).map(|x| tidy(x, depth + 1)).collect();
            if a.len() > MAX_ITEMS {
                out.push(Value::String(format!("… {} more", a.len() - MAX_ITEMS)));
            }
            Value::Array(out)
        }
        Value::Object(m) => Value::Object(
            m.iter()
                .take(MAX_ITEMS * 2)
                .map(|(k, x)| (k.clone(), if secret_key(k) { Value::String("***".into()) } else { tidy(x, depth + 1) }))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// What identifies the thing a drag moves, when `id` is a drag command.
fn drag_key(id: &str, p: &Value) -> Option<String> {
    if !DRAGS.contains(&id) {
        return None;
    }
    let who: Vec<String> = ["device", "place", "note", "cluster", "id"].iter().filter_map(|k| p.get(*k)).map(Value::to_string).collect();
    Some(format!("{id}:{}", who.join(",")))
}

impl CommandLog {
    /// The entries, oldest first.
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> + ExactSizeIterator {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Notes a command that ran (never a `log.*` command, and nothing while logging is off).
    pub(crate) fn record(&mut self, id: &str, params: &Value, result: &Result<Value>, sim_us: u64) {
        if !self.enabled || id.starts_with("log.") {
            return;
        }
        let tidy_params = clip(&tidy(params, 0).to_string(), MAX_PARAMS_CHARS);
        let (ok, error) = match result {
            Ok(_) => (true, None),
            Err(e) => (false, Some(clip(&e.to_string(), MAX_ERROR_CHARS))),
        };
        let drag = drag_key(id, params);
        if let (Some(key), Some(last)) = (&drag, self.entries.back_mut())
            && last.drag.as_ref() == Some(key)
        {
            last.params = tidy_params;
            last.ok = ok;
            last.error = error;
            last.sim_us = sim_us;
            last.count = last.count.saturating_add(1);
            return;
        }
        if self.entries.len() >= MAX_LOG {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry { n: self.next, command: id.to_string(), params: tidy_params, ok, error, sim_us, count: 1, drag });
        self.next += 1;
    }

    /// Forgets every entry (the numbering goes on).
    pub fn clear(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }

    /// One entry as a line of the exported text.
    pub fn line(e: &LogEntry) -> String {
        let secs = e.sim_us / 1_000_000;
        let when = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
        let times = if e.count > 1 { format!(" (x{})", e.count) } else { String::new() };
        let status = match &e.error {
            None => "ok".to_string(),
            Some(why) => format!("failed: {why}"),
        };
        let params = if e.params == "null" { String::new() } else { format!(" {}", e.params) };
        format!("{:>5}  {when}  {}{params}{times}  {status}", e.n, e.command)
    }

    /// The whole log as text: a short header, then one line per command.
    pub fn text(&self) -> String {
        let mut out = format!(
            "# NetLabCraft command log\n# {} command(s); columns: number, simulated time, command, parameters, result\n# Passwords and keys are hidden; long values are cut.\n",
            self.entries.len()
        );
        for e in &self.entries {
            out.push_str(&Self::line(e));
            out.push('\n');
        }
        out
    }
}

/// The person using this computer. Local only: one display name and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub display_name: String,
}

/// A display name made safe: control characters out, spaces trimmed and squeezed, at most
/// [`MAX_NAME`] characters.
pub fn clean_name(name: &str) -> String {
    let squeezed: Vec<&str> = name.split_whitespace().collect();
    let joined: String = squeezed.join(" ").chars().filter(|c| !c.is_control()).collect();
    joined.chars().take(MAX_NAME).collect::<String>().trim().to_string()
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "log.list" => Some(list(s, p)),
        "log.export" => Some(export(s, p)),
        "log.clear" => Some(Ok(json!({"removed": s.cmd_log.clear()}))),
        "log.enable" => Some(enable(s, p)),
        "profile.get" => Some(Ok(json!({"name": s.profile.display_name}))),
        "profile.set" => Some(set_profile(s, p)),
        "physical.length_effects" => Some(length_effects(s, p)),
        _ => None,
    }
}

fn list(s: &Session, p: &Value) -> Result<Value> {
    let since = p.get("since").and_then(Value::as_u64).unwrap_or(0);
    let limit = match p.get("limit") {
        None | Some(Value::Null) => 200,
        Some(v) => v.as_u64().filter(|n| (1..=MAX_LOG as u64).contains(n)).ok_or_else(|| bad(format!("`limit` is 1 to {MAX_LOG}")))? as usize,
    };
    let newer: Vec<&LogEntry> = s.cmd_log.entries().filter(|e| e.n > since).collect();
    let skip = newer.len().saturating_sub(limit);
    let entries: Vec<Value> = newer
        .into_iter()
        .skip(skip)
        .map(|e| json!({"n": e.n, "command": e.command, "params": e.params, "ok": e.ok, "error": e.error, "sim_us": e.sim_us, "count": e.count}))
        .collect();
    Ok(json!({"enabled": s.cmd_log.enabled, "total": s.cmd_log.len(), "entries": entries}))
}

fn export(s: &Session, p: &Value) -> Result<Value> {
    let text = s.cmd_log.text();
    let mut out = json!({"text": text, "file_name": LOG_FILE_NAME, "entries": s.cmd_log.len()});
    if let Some(path) = p.get("path").and_then(Value::as_str) {
        crate::cmd::write_file(path, text.as_bytes())?;
        out["path"] = json!(path);
    }
    Ok(out)
}

fn enable(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad("`on` (true or false) is required"))?;
    s.cmd_log.enabled = on;
    Ok(json!({"on": on}))
}

fn set_profile(s: &mut Session, p: &Value) -> Result<Value> {
    let name = p.get("name").and_then(Value::as_str).ok_or_else(|| bad("`name` (text; empty clears it) is required"))?;
    s.profile.display_name = clean_name(name);
    Ok(json!({"name": s.profile.display_name}))
}

fn length_effects(s: &mut Session, p: &Value) -> Result<Value> {
    match p.get("on") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let on = v.as_bool().ok_or_else(|| bad("`on` is true or false"))?;
            if s.topology.physical.ignore_lengths == on {
                s.checkpoint();
                s.topology.physical.ignore_lengths = !on;
                s.changed();
            }
        }
    }
    Ok(json!({"on": !s.topology.physical.ignore_lengths}))
}
