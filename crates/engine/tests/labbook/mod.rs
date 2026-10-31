//! The lab book runner: reads a lab from `docs/labs/*.md` and drives it end to end.
//!
//! A lab's Markdown holds three kinds of fenced blocks the runner executes, in order:
//!
//! - ```` ```lab ```` — engine commands, one per line: `device.add {"model": "Router-2", "name": "R1"}`.
//! - ```` ```console R1 ```` — lines typed at R1's console, one at a time. Any answer that is an
//!   error (`% …`) fails the lab.
//! - ```` ```verify ```` — checks:
//!   - `R1# show ip route` followed by indented `expect: text` / `reject: text` lines: the line is
//!     typed at R1 and its output must (not) contain each text;
//!   - `ping PC1 192.168.1.1 => success` (or `=> failure`): a warm-up ping, then three probes
//!     that must all be answered (or none);
//!   - `events: ARP` (or `no events: …`): some event in the event list (protocol and summary)
//!     contains the text;
//!   - `run browser.get {"device": "PC1", "url": "…"}` followed by `expect:` / `reject:` lines:
//!     an engine command whose JSON answer must (not) contain each text.
//!
//! Other blocks (`text`, `sh`…) are for reading only. The same file is included by the MCP test
//! in `apps/netlabcraft-cli/tests`, which drives the labs through `netlabcraft-cli mcp`.
#![allow(dead_code)]

use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub enum Step {
    Run { line: usize, id: String, params: Value },
    Type { line: usize, device: String, lines: Vec<String> },
    Show { line: usize, device: String, command: String, expect: Vec<String>, reject: Vec<String> },
    Ping { line: usize, from: String, to: String, succeed: bool },
    Events { line: usize, text: String, present: bool },
    Query { line: usize, id: String, params: Value, expect: Vec<String>, reject: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct Lab {
    pub file: String,
    pub title: String,
    pub steps: Vec<Step>,
}

/// The lab book's directory.
pub fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/labs")
}

/// Every lab file (`NN-name.md`), sorted.
pub fn files() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .expect("docs/labs")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".md") && n.as_bytes().first().is_some_and(u8::is_ascii_digit))
        .collect();
    v.sort();
    v
}

pub fn load(file: &str) -> Lab {
    let text = std::fs::read_to_string(dir().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
    parse(file, &text).unwrap_or_else(|e| panic!("{file}: {e}"))
}

pub fn parse(file: &str, text: &str) -> Result<Lab, String> {
    let title = text.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or(file).trim().to_string();
    let mut steps = Vec::new();
    let mut block: Option<(String, usize, Vec<(usize, String)>)> = None;
    for (n, raw) in text.lines().enumerate() {
        let n = n + 1;
        if let Some(info) = raw.trim_start().strip_prefix("```") {
            match block.take() {
                None => block = Some((info.trim().to_string(), n, Vec::new())),
                Some((info, start, lines)) => steps.extend(block_steps(&info, start, &lines)?),
            }
            continue;
        }
        if let Some((_, _, lines)) = block.as_mut() {
            lines.push((n, raw.to_string()));
        }
    }
    if block.is_some() {
        return Err("unclosed code block".into());
    }
    Ok(Lab { file: file.to_string(), title, steps })
}

fn block_steps(info: &str, start: usize, lines: &[(usize, String)]) -> Result<Vec<Step>, String> {
    let mut words = info.split_whitespace();
    let kind = words.next().unwrap_or("");
    let mut out = Vec::new();
    match kind {
        "lab" => {
            for (n, l) in lines.iter().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with("//")) {
                let l = l.trim();
                let (id, rest) = l.split_once(' ').unwrap_or((l, "{}"));
                let params: Value = serde_json::from_str(rest).map_err(|e| format!("line {n}: {e}"))?;
                out.push(Step::Run { line: *n, id: id.to_string(), params });
            }
        }
        "console" => {
            let device = words.next().ok_or(format!("line {start}: console block without a device"))?.to_string();
            let typed: Vec<String> = lines.iter().map(|(_, l)| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            out.push(Step::Type { line: start, device, lines: typed });
        }
        "verify" => {
            for (n, l) in lines.iter().filter(|(_, l)| !l.trim().is_empty()) {
                let t = l.trim();
                if let Some(x) = t.strip_prefix("expect:") {
                    match out.last_mut() {
                        Some(Step::Show { expect, .. } | Step::Query { expect, .. }) => expect.push(x.trim().to_string()),
                        _ => return Err(format!("line {n}: `expect:` without a command before it")),
                    }
                } else if let Some(x) = t.strip_prefix("reject:") {
                    match out.last_mut() {
                        Some(Step::Show { reject, .. } | Step::Query { reject, .. }) => reject.push(x.trim().to_string()),
                        _ => return Err(format!("line {n}: `reject:` without a command before it")),
                    }
                } else if let Some(x) = t.strip_prefix("run ") {
                    let (id, rest) = x.trim().split_once(' ').unwrap_or((x.trim(), "{}"));
                    let params: Value = serde_json::from_str(rest).map_err(|e| format!("line {n}: {e}"))?;
                    out.push(Step::Query { line: *n, id: id.to_string(), params, expect: Vec::new(), reject: Vec::new() });
                } else if let Some(x) = t.strip_prefix("events:") {
                    out.push(Step::Events { line: *n, text: x.trim().to_string(), present: true });
                } else if let Some(x) = t.strip_prefix("no events:") {
                    out.push(Step::Events { line: *n, text: x.trim().to_string(), present: false });
                } else if let Some(x) = t.strip_prefix("ping ") {
                    let (what, want) = x.split_once("=>").ok_or(format!("line {n}: ping without `=> success|failure`"))?;
                    let mut w = what.split_whitespace();
                    let (Some(from), Some(to)) = (w.next(), w.next()) else { return Err(format!("line {n}: ping FROM ADDRESS")) };
                    let succeed = match want.trim() {
                        "success" => true,
                        "failure" => false,
                        o => return Err(format!("line {n}: `{o}` (success or failure)")),
                    };
                    out.push(Step::Ping { line: *n, from: from.to_string(), to: to.to_string(), succeed });
                } else {
                    let (dev, cmd) = t.split_once(' ').ok_or(format!("line {n}: `DEVICE# command`"))?;
                    let device = dev.strip_suffix('#').or_else(|| dev.strip_suffix('>')).ok_or(format!("line {n}: `{dev}` (DEVICE# or DEVICE>)"))?;
                    out.push(Step::Show {
                        line: *n,
                        device: device.to_string(),
                        command: cmd.trim().to_string(),
                        expect: Vec::new(),
                        reject: Vec::new(),
                    });
                }
            }
        }
        _ => {}
    }
    Ok(out)
}

/// What runs the steps: the engine directly, or the MCP server.
pub trait Driver {
    /// Runs an engine command.
    fn run(&mut self, id: &str, params: Value) -> Result<Value, String>;
    /// Types lines at a console; each line's output.
    fn type_lines(&mut self, device: &str, lines: &[String]) -> Result<Vec<String>, String>;
}

/// A console answer that means the line was refused.
pub fn is_error(output: &str) -> bool {
    output.starts_with('%')
        || output.contains("\n%")
        || output.contains("Not understood")
        // What hosts answer to a line they can't use.
        || ["DHCP failed", "Usage:", "is not a command here", "failed:", "can't be", "is not on"].iter().any(|x| output.contains(x))
}

/// Runs a lab from an empty lab; every failure, with its line in the Markdown.
pub fn run(lab: &Lab, d: &mut dyn Driver) -> Vec<String> {
    let mut fails = Vec::new();
    if let Err(e) = d.run("file.new", json!({})) {
        return vec![format!("file.new: {e}")];
    }
    // `LAB_BOOK_TRACE=1 cargo test …`: print every answer, to read a lab's run.
    let trace = std::env::var_os("LAB_BOOK_TRACE").is_some();
    for step in &lab.steps {
        match step {
            Step::Run { line, id, params } => match d.run(id, params.clone()) {
                Ok(v) if trace => println!("[{line}] {id} → {v}"),
                Ok(_) => {}
                Err(e) => fails.push(format!("line {line}: {id}: {e}")),
            },
            Step::Type { line, device, lines } => match d.type_lines(device, lines) {
                Ok(outs) => {
                    for (l, o) in lines.iter().zip(&outs) {
                        if trace {
                            println!("[{line}] {device}: {l}\n{o}");
                        }
                        if is_error(o) {
                            fails.push(format!("block at line {line}: {device}: `{l}` → {o}"));
                        }
                    }
                }
                Err(e) => fails.push(format!("block at line {line}: {device}: {e}")),
            },
            Step::Show { line, device, command, expect, reject } => match d.type_lines(device, std::slice::from_ref(command)) {
                Ok(outs) => {
                    let o = outs.first().cloned().unwrap_or_default();
                    if trace {
                        println!("[{line}] {device}# {command}\n{o}");
                    }
                    for x in expect {
                        if !o.contains(x.as_str()) {
                            fails.push(format!("line {line}: {device}: `{command}` lacks `{x}`:\n{o}"));
                        }
                    }
                    for x in reject {
                        if o.contains(x.as_str()) {
                            fails.push(format!("line {line}: {device}: `{command}` shows `{x}`:\n{o}"));
                        }
                    }
                }
                Err(e) => fails.push(format!("line {line}: {device}: {e}")),
            },
            Step::Query { line, id, params, expect, reject } => {
                // An error is an answer too: the expectations are checked against its text.
                let o = match d.run(id, params.clone()) {
                    Ok(v) => v.to_string(),
                    Err(e) => format!("error: {e}"),
                };
                for x in expect.iter().filter(|x| !o.contains(x.as_str())) {
                    fails.push(format!("line {line}: {id} lacks `{x}`:\n{o}"));
                }
                for x in reject.iter().filter(|x| o.contains(x.as_str())) {
                    fails.push(format!("line {line}: {id} shows `{x}`:\n{o}"));
                }
            }
            Step::Events { line, text, present } => match d.run("sim.events", json!({"limit": 10_000})) {
                Ok(Value::Array(evs)) => {
                    let found = evs.iter().any(|e| {
                        let p = e.get("protocol").and_then(Value::as_str).unwrap_or("");
                        let s = e.get("summary").and_then(Value::as_str).unwrap_or("");
                        let dropped = e.get("dropped").and_then(Value::as_str).unwrap_or("");
                        format!("{p} {s} {dropped}").contains(text.as_str())
                    });
                    if found != *present {
                        let list: Vec<String> = evs
                            .iter()
                            .rev()
                            .take(40)
                            .map(|e| {
                                format!(
                                    "{} {}",
                                    e.get("protocol").and_then(Value::as_str).unwrap_or(""),
                                    e.get("summary").and_then(Value::as_str).unwrap_or("")
                                )
                            })
                            .collect();
                        fails.push(format!(
                            "line {line}: events {} `{text}`; last events:\n{}",
                            if *present { "lack" } else { "show" },
                            list.join("\n")
                        ));
                    }
                }
                Ok(other) => fails.push(format!("line {line}: sim.events gave {other}")),
                Err(e) => fails.push(format!("line {line}: sim.events: {e}")),
            },
            Step::Ping { line, from, to, succeed } => {
                let _ = d.run("net.ping", json!({"from": from, "to": to, "count": 4}));
                match d.run("net.ping", json!({"from": from, "to": to, "count": 3})) {
                    Ok(r) => {
                        let marks = r.get("marks").and_then(Value::as_str).unwrap_or("").to_string();
                        let ok = if *succeed { marks == "!!!" } else { !marks.contains('!') };
                        if !ok {
                            let want = if *succeed { "success" } else { "failure" };
                            fails.push(format!("line {line}: ping {from} → {to}: `{marks}`, expected {want}"));
                        }
                    }
                    // No route is a failure too.
                    Err(_) if !*succeed => {}
                    Err(e) => fails.push(format!("line {line}: ping {from} → {to}: {e}")),
                }
            }
        }
    }
    fails
}
