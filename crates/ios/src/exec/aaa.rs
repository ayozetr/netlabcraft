//! AAA beyond the login, and privilege levels: `privilege exec level N COMMAND`, `enable N`,
//! `enable secret level N`, `aaa authorization exec|commands N`, `aaa accounting
//! exec|commands N|connection`, and `show aaa`, `show aaa authorization`, `show aaa accounting`.
//!
//! The lists and rules are kept as console settings (`netlabcraft_model::knobs`, under the
//! command's own words) and written back by `show running-config`; the session reads them
//! (`exec.rs`) when someone logs in, enables, types a command or opens a connection.

use std::fmt::Write as _;

use netlabcraft_model::knobs::Scope;
use netlabcraft_model::{Device, secret_hash};

use super::{Ctx, R};
use crate::grammar::{Id, Modes, SPECS, Spec, Tok};

/// One way to settle an authorization, tried in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    Tacacs,
    Radius,
    Local,
    IfAuthenticated,
    NoCheck,
}

/// The words of a method list, validated: `group tacacs+`, `group radius`, `local`,
/// `if-authenticated`, `none`.
fn parse_methods(words: &[&str], allow_radius: bool) -> Result<Vec<Method>, String> {
    let mut out = Vec::new();
    let mut it = words.iter();
    while let Some(w) = it.next() {
        match w.to_ascii_lowercase().as_str() {
            "group" => match it.next().map(|g| g.to_ascii_lowercase()).as_deref() {
                Some("tacacs+") => out.push(Method::Tacacs),
                Some("radius") if allow_radius => out.push(Method::Radius),
                _ => return Err(format!("% After `group`: tacacs+{}", if allow_radius { " or radius" } else { "" })),
            },
            "local" => out.push(Method::Local),
            "if-authenticated" => out.push(Method::IfAuthenticated),
            "none" => out.push(Method::NoCheck),
            other => return Err(format!("% `{other}` is not a method: group tacacs+, local, if-authenticated or none")),
        }
    }
    if out.is_empty() {
        return Err("% Give at least one method".into());
    }
    Ok(out)
}

fn methods_text(m: &[Method]) -> String {
    m.iter()
        .map(|x| match x {
            Method::Tacacs => "group tacacs+",
            Method::Radius => "group radius",
            Method::Local => "local",
            Method::IfAuthenticated => "if-authenticated",
            Method::NoCheck => "none",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The authorization method list kept under `key` (`aaa authorization exec default`).
pub(super) fn authz_methods(d: &Device, key: &str) -> Option<Vec<Method>> {
    let line = d.knobs.get(Scope::Global, key)?;
    let words: Vec<&str> = line.split_whitespace().skip(key.split_whitespace().count()).collect();
    parse_methods(&words, true).ok()
}

/// An accounting list: record at the start too, send to TACACS+, send to RADIUS. `None` for `none`.
pub(super) fn acct_spec(d: &Device, key: &str) -> Option<(bool, bool, bool)> {
    let line = d.knobs.get(Scope::Global, key)?;
    let words: Vec<&str> = line.split_whitespace().skip(key.split_whitespace().count()).collect();
    let (first, rest) = words.split_first()?;
    if *first == "none" {
        return None;
    }
    let methods = parse_methods(rest, true).ok()?;
    Some((*first == "start-stop", methods.contains(&Method::Tacacs), methods.contains(&Method::Radius)))
}

/// The hashed secret of `enable secret level N`.
pub(super) fn level_secret(d: &Device, level: u8) -> Option<String> {
    d.knobs.get(Scope::Global, &format!("enable secret level {level}"))?.split_whitespace().last().map(str::to_string)
}

/// The scope a privilege rule is kept under for a command mode.
pub(super) fn scope_of(mode: &super::Mode) -> &'static str {
    use super::Mode;
    match mode {
        Mode::User | Mode::Exec => "exec",
        Mode::Iface(_) | Mode::Svi(_) | Mode::Loopback(_) | Mode::Tunnel(_) => "interface",
        Mode::Line(_) => "line",
        Mode::Ospf | Mode::Rip | Mode::Eigrp | Mode::Bgp | Mode::Ospf6 | Mode::Eigrp6 | Mode::Rip6 => "router",
        _ => "configure",
    }
}

/// The keywords a command starts with (up to its first argument).
pub(super) fn keywords(spec: &Spec) -> Vec<&'static str> {
    spec.path.iter().map_while(|t| if let Tok::K(k) = t { Some(*k) } else { None }).collect()
}

/// The level a command needs on this device: the longest `privilege` rule that matches it, else
/// 1 for what a user may type and 15 for the rest.
pub(super) fn command_level(d: &Device, scope: &str, spec: &Spec) -> u8 {
    let kws = keywords(spec);
    let prefix = format!("privilege {scope} ");
    let mut best: Option<(usize, u8)> = None;
    for (key, line) in &d.knobs.global {
        let Some(words) = key.strip_prefix(&prefix) else { continue };
        let rule: Vec<&str> = words.split_whitespace().collect();
        if rule.is_empty() || rule.len() > kws.len() || !rule.iter().zip(&kws).all(|(a, b)| a == b) {
            continue;
        }
        let Some(level) = line.split_whitespace().nth(3).and_then(|n| n.parse::<u8>().ok()) else { continue };
        if best.is_none_or(|(n, _)| rule.len() > n) {
            best = Some((rule.len(), level));
        }
    }
    match best {
        Some((_, level)) => level,
        None if scope == "exec" && spec.modes.has(Modes::USER) => 1,
        None => 15,
    }
}

/// The command words a rule stands for: each typed word completed to the keyword of the
/// commands that start that way. Ambiguous or unknown words are refused.
fn canonical(words: &[&str], scope: &str) -> Result<Vec<String>, String> {
    let wanted = |s: &Spec| {
        if scope == "exec" { s.modes.has(Modes::USER) || s.modes.has(Modes::EXEC) } else { !(s.modes.has(Modes::USER) || s.modes.has(Modes::EXEC)) }
    };
    let mut found: Vec<Vec<&'static str>> = Vec::new();
    for s in SPECS.iter().filter(|s| wanted(s)) {
        let kws = keywords(s);
        if kws.len() < words.len() || !words.iter().zip(&kws).all(|(w, k)| k.starts_with(&w.to_ascii_lowercase())) {
            continue;
        }
        let head: Vec<&'static str> = kws.iter().take(words.len()).copied().collect();
        // A typed word that is a whole keyword wins over longer ones that start with it.
        if !found.contains(&head) {
            found.push(head);
        }
    }
    let exact: Vec<&Vec<&'static str>> = found.iter().filter(|h| h.iter().zip(words).all(|(k, w)| k.eq_ignore_ascii_case(w))).collect();
    match (exact.as_slice(), found.as_slice()) {
        ([one], _) => Ok(one.iter().map(|k| k.to_string()).collect()),
        ([], [one]) => Ok(one.iter().map(|k| k.to_string()).collect()),
        ([], []) => Err(format!("% `{}` is not a {scope} command", words.join(" "))),
        _ => Err(format!("% `{}` could be several commands: type more of it", words.join(" "))),
    }
}

fn need_aaa(c: &Ctx) -> Result<(), String> {
    if c.d()?.access.aaa { Ok(()) } else { Err("% Turn AAA on first: aaa new-model".into()) }
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let level_arg = |i: usize, lo: u8| -> Result<u8, String> {
        arg(i).parse::<u8>().ok().filter(|n| (lo..=15).contains(n)).ok_or_else(|| format!("% The level is a number from {lo} to 15"))
    };
    match id {
        Id::AaaAuthzExec | Id::AaaAuthzCommands | Id::AaaAcctExec | Id::AaaAcctCommands | Id::AaaAcctConnection => {
            need_aaa(c)?;
            let (key, list) = match id {
                Id::AaaAuthzExec => ("aaa authorization exec default".to_string(), arg(0)),
                Id::AaaAuthzCommands => (format!("aaa authorization commands {} default", level_arg(0, 0)?), arg(1)),
                Id::AaaAcctExec => ("aaa accounting exec default".to_string(), arg(0)),
                Id::AaaAcctCommands => (format!("aaa accounting commands {} default", level_arg(0, 0)?), arg(1)),
                _ => ("aaa accounting connection default".to_string(), arg(0)),
            };
            let words: Vec<&str> = list.split_whitespace().collect();
            let text = if matches!(id, Id::AaaAuthzExec | Id::AaaAuthzCommands) {
                methods_text(&parse_methods(&words, false)?)
            } else {
                let (first, rest) = words.split_first().ok_or("% Give start-stop, stop-only or none")?;
                let first = first.to_ascii_lowercase();
                if !["start-stop", "stop-only", "none"].contains(&first.as_str()) {
                    return Err("% Say start-stop, stop-only or none first".into());
                }
                if first == "none" { first } else { format!("{first} {}", methods_text(&parse_methods(rest, true)?)) }
            };
            c.dm()?.knobs.set(Scope::Global, &key, format!("{key} {text}"))?;
            Ok(String::new())
        }
        Id::NoAaaAuthzExec | Id::NoAaaAcctExec | Id::NoAaaAcctConnection | Id::NoAaaAuthzCommands | Id::NoAaaAcctCommands => {
            let key = match id {
                Id::NoAaaAuthzExec => "aaa authorization exec default".to_string(),
                Id::NoAaaAcctExec => "aaa accounting exec default".to_string(),
                Id::NoAaaAcctConnection => "aaa accounting connection default".to_string(),
                Id::NoAaaAuthzCommands => format!("aaa authorization commands {} default", level_arg(0, 0)?),
                _ => format!("aaa accounting commands {} default", level_arg(0, 0)?),
            };
            c.dm()?.knobs.clear(Scope::Global, &key);
            Ok(String::new())
        }
        Id::Privilege | Id::NoPrivilege => {
            let scope = arg(0).to_ascii_lowercase();
            if !["exec", "configure", "interface", "line", "router"].contains(&scope.as_str()) {
                return Err("% Say exec, configure, interface, line or router".into());
            }
            let level = level_arg(1, 0)?;
            let typed: Vec<&str> = arg(2).split_whitespace().collect();
            let words = canonical(&typed, &scope)?;
            let key = format!("privilege {scope} {}", words.join(" "));
            let d = c.dm()?;
            if id == Id::Privilege {
                d.knobs.set(Scope::Global, &key, format!("privilege {scope} level {level} {}", words.join(" ")))?;
            } else {
                d.knobs.clear(Scope::Global, &key);
            }
            Ok(String::new())
        }
        Id::EnableSecretLevel => {
            let level = level_arg(0, 1)?;
            let hash = match arg(1).split_whitespace().collect::<Vec<_>>().as_slice() {
                ["5", h] if h.starts_with("$pc$") => (*h).to_string(),
                ["5", _] => return Err("% Give the secret itself, or a hash NetLabCraft wrote (`$pc$…`)".into()),
                ["0", p] | [p] => {
                    c.check_length(p)?;
                    secret_hash(p)
                }
                _ => return Err("% Give a password".into()),
            };
            if level == 15 {
                c.dm()?.access.enable_secret = Some(hash);
            } else {
                let key = format!("enable secret level {level}");
                c.dm()?.knobs.set(Scope::Global, &key, format!("{key} {hash}"))?;
            }
            Ok(String::new())
        }
        Id::NoEnableSecretLevel => {
            let level = level_arg(0, 1)?;
            if level == 15 {
                c.dm()?.access.enable_secret = None;
            } else {
                c.dm()?.knobs.clear(Scope::Global, &format!("enable secret level {level}"));
            }
            Ok(String::new())
        }
        Id::ShowAaa => {
            let d = c.d()?;
            let mut s = String::new();
            let _ = writeln!(s, "AAA: {}", if d.access.aaa { "on (aaa new-model)" } else { "off" });
            let login: Vec<&str> = d
                .access
                .methods()
                .iter()
                .map(|m| match m {
                    netlabcraft_model::LoginMethod::Radius => "group radius",
                    netlabcraft_model::LoginMethod::Tacacs => "group tacacs+",
                    netlabcraft_model::LoginMethod::Local => "local",
                })
                .collect();
            let _ = writeln!(s, "Login methods: {}", if login.is_empty() { "none".to_string() } else { login.join(" ") });
            for (h, _) in &d.access.radius_servers {
                let _ = writeln!(s, "RADIUS server: {h}");
            }
            for (h, _) in &d.access.tacacs_servers {
                let _ = writeln!(s, "TACACS+ server: {h}");
            }
            let rules: Vec<&String> = d.knobs.global.values().filter(|l| l.starts_with("privilege ")).collect();
            let _ = writeln!(s, "Privilege rules: {}", rules.len());
            for r in rules {
                let _ = writeln!(s, "  {r}");
            }
            let levels: Vec<String> = (1..15u8).filter(|n| level_secret(d, *n).is_some()).map(|n| n.to_string()).collect();
            let _ = writeln!(
                s,
                "Enable secrets for levels: {}",
                if levels.is_empty() { "15 only".to_string() } else { format!("{}, 15", levels.join(", ")) }
            );
            Ok(s.trim_end().to_string())
        }
        Id::ShowAaaAuthorization | Id::ShowAaaAccounting => {
            let d = c.d()?;
            let word = if id == Id::ShowAaaAuthorization { "authorization" } else { "accounting" };
            let prefix = format!("aaa {word} ");
            let lines: Vec<&String> = d.knobs.global.iter().filter(|(k, _)| k.starts_with(&prefix)).map(|(_, l)| l).collect();
            let mut s = String::new();
            if lines.is_empty() {
                let _ = writeln!(s, "No {word} lists are configured.");
            }
            for l in lines {
                let _ = writeln!(s, "{l}");
            }
            if id == Id::ShowAaaAccounting {
                let st = c.sim.aaa_stats(c.dev);
                let _ = writeln!(s, "Accounting records: {} sent, {} acknowledged", st.sent, st.acknowledged);
            }
            Ok(s.trim_end().to_string())
        }
        _ => Err("% Not an AAA command".into()),
    }
}
