//! Settings the console keeps and shows without acting on them: `service sequence-numbers`,
//! `ip classless`, `load-interval`, line options and the like. Each is stored under the words that
//! name it (`netlabcraft_model::knobs`), written back by `show running-config`, and answered with a
//! line saying it changes nothing in the simulation (the list is in `docs/gaps.md`).

use std::fmt::Write as _;

use netlabcraft_model::Device;
use netlabcraft_model::knobs::{Scope, clean_value};

use super::{Ctx, Mode, R};
use crate::grammar::{Id, Spec, Tok};

/// Settings that are on until turned off: typing the setting removes its `no` line, and the `no`
/// form is what gets written.
const DEFAULT_ON: &[&str] = &[
    "cdp advertise-v2",
    "exec",
    "exec-banner",
    "ip cef",
    "ip classless",
    "ip dhcp conflict logging",
    "ip gratuitous-arps",
    "ip mroute-cache",
    "ip redirects",
    "ip route-cache",
    "ip source-route",
    "ip split-horizon",
    "ip subnet-zero",
    "ip unreachables",
    "ipv6 redirects",
    "ipv6 unreachables",
    "motd-banner",
    "service dhcp",
    "service pad",
    "spanning-tree etherchannel guard misconfig",
    "spanning-tree extend system-id",
];

/// What the console says after it keeps a setting.
const KEPT: &str = "Accepted: kept in the configuration; it does not change what the simulator does.";

/// A value checked against its placeholder: `{a|b}` is one of the words, `<lo-hi>` a number in the
/// range, anything else one word. Returns the value as it is kept.
fn check(placeholder: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if v.is_empty() {
        return Err("% Incomplete command.".into());
    }
    if let Some(options) = placeholder.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
        return options
            .split('|')
            .find(|o| o.eq_ignore_ascii_case(v))
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| format!("% Say one of: {}", options.replace('|', ", ")));
    }
    if let Some((lo, hi)) = placeholder.strip_prefix('<').and_then(|p| p.strip_suffix('>')).and_then(|r| r.split_once('-')) {
        let (Ok(lo), Ok(hi)) = (lo.parse::<f64>(), hi.parse::<f64>()) else { return Ok(clean_value(v)) };
        if !v.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(format!("% The value is a number from {lo} to {hi}"));
        }
        return match v.parse::<f64>() {
            Ok(n) if (lo..=hi).contains(&n) => Ok(v.to_string()),
            _ => Err(format!("% The value is a number from {lo} to {hi}")),
        };
    }
    let one = clean_value(v);
    if one.is_empty() || one.contains(char::is_whitespace) {
        return Err("% One word is expected here".into());
    }
    Ok(one)
}

/// Runs `Knob` and `NoKnob`.
pub(super) fn command(c: &mut Ctx, spec: &Spec, args: &[String]) -> R {
    let no = spec.id == Id::NoKnob;
    let mut words: Vec<String> = Vec::new();
    let mut key: Vec<String> = Vec::new();
    let mut next = args.iter();
    for t in spec.path.iter().skip(usize::from(no)) {
        match t {
            Tok::K(k) => {
                words.push((*k).to_string());
                key.push((*k).to_string());
            }
            Tok::Arg(p) => {
                let v = check(p, next.next().map_or("", String::as_str))?;
                if p.starts_with('{') {
                    key.push(v.clone());
                }
                words.push(v);
            }
            Tok::Rest(_) => {
                let v = clean_value(next.next().map_or("", String::as_str));
                if v.is_empty() {
                    return Err("% Incomplete command.".into());
                }
                words.push(v);
            }
        }
    }
    let key = key.join(" ");
    let line = words.join(" ");
    let on_by_default = DEFAULT_ON.contains(&key.as_str());
    // What is kept: the line for a setting that is off by default, the `no` line for one that is on.
    let keep = if on_by_default { no.then(|| format!("no {line}")) } else { (!no).then_some(line) };
    let scope_names: Vec<String> = match &c.cli.mode {
        Mode::Config => Vec::new(),
        Mode::Line(vty) => vec![(if *vty { "vty" } else { "con" }).to_string()],
        Mode::Loopback(n) => vec![format!("Loopback{n}")],
        Mode::Iface(_) | Mode::Svi(_) => c.mode_iface_names()?,
        _ => return Err("% That setting is not available in this mode".into()),
    };
    let line_mode = matches!(c.cli.mode, Mode::Line(_));
    let global = matches!(c.cli.mode, Mode::Config);
    let d = c.dm()?;
    let mut scopes: Vec<Scope> = Vec::new();
    if global {
        scopes.push(Scope::Global);
    }
    for n in &scope_names {
        scopes.push(if line_mode { Scope::Line(n) } else { Scope::Iface(n) });
    }
    for s in scopes {
        match &keep {
            Some(l) => d.knobs.set(s, &key, l.clone())?,
            None => d.knobs.clear(s, &key),
        }
    }
    Ok(if keep.is_some() { KEPT.to_string() } else { String::new() })
}

/// The global settings, for `show running-config`.
pub(crate) fn global_config(s: &mut String, d: &Device) {
    let lines = d.knobs.written(Scope::Global);
    for l in &lines {
        let _ = writeln!(s, "{l}");
    }
    if !lines.is_empty() {
        s.push_str("!\n");
    }
}

/// An interface's settings, written inside its block.
pub(crate) fn iface_lines(s: &mut String, d: &Device, name: &str) {
    for l in d.knobs.written(Scope::Iface(name)) {
        let _ = writeln!(s, " {l}");
    }
}

/// A line group's settings (`con` or `vty`), written inside its block.
pub(crate) fn line_lines(s: &mut String, d: &Device, group: &str) {
    for l in d.knobs.written(Scope::Line(group)) {
        let _ = writeln!(s, " {l}");
    }
}
