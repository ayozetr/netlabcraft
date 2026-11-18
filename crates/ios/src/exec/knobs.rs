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
    "validate-update-source",
];

/// Settings the simulator acts on: they are kept like the others, but the console does not say
/// they change nothing.
const ACTIVE: &[&str] = &[
    "absolute-timeout",
    "cdp holdtime",
    "cdp timer",
    "exec-timeout",
    "ip classless",
    "ip icmp rate-limit unreachable",
    "ip unreachables",
    "lldp holdtime",
    "lldp timer",
    "logging rate-limit",
    "mac address-table aging-time",
    "mac-address-table aging-time",
    "service sequence-numbers",
    "session-timeout",
];

/// What the console says after it keeps a setting.
const KEPT: &str = "Accepted: kept in the configuration; it does not change what the simulator does.";

/// A value checked against its placeholder: `{a|b}` is one of the words, `<lo-hi>` a number in the
/// range, anything else one word. Returns the value as it is kept.
pub(super) fn check(placeholder: &str, value: &str) -> Result<String, String> {
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

/// Whether the argument at `i` numbers a group or an autonomous system, so that two of the same
/// setting for different numbers (`standby 1 timers …`, `standby 2 timers …`) are two settings.
fn keyed_arg(spec: &Spec, start: usize, i: usize) -> bool {
    let first = spec.path.get(start);
    let group = matches!(first, Some(Tok::K("standby" | "vrrp" | "glbp"))) && i == start + 1;
    let eigrp_as = matches!(first, Some(Tok::K("ip"))) && matches!(i.checked_sub(1).and_then(|j| spec.path.get(j)), Some(Tok::K("eigrp")));
    let alias = matches!(first, Some(Tok::K("alias"))) && i == start + 2;
    group || eigrp_as || alias
}

/// Runs `Knob` and `NoKnob`.
pub(super) fn command(c: &mut Ctx, spec: &Spec, args: &[String]) -> R {
    let no = spec.id == Id::NoKnob;
    let mut words: Vec<String> = Vec::new();
    let mut key: Vec<String> = Vec::new();
    let mut next = args.iter();
    let start = usize::from(no);
    for (i, t) in spec.path.iter().enumerate().skip(start) {
        match t {
            Tok::K(k) => {
                words.push((*k).to_string());
                key.push((*k).to_string());
            }
            Tok::Arg(p) => {
                let v = check(p, next.next().map_or("", String::as_str))?;
                if p.starts_with('{') || keyed_arg(spec, start, i) {
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
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Global,
        Iface,
        Line,
        Proto,
    }
    // `line 1 8`: the settings belong to those asynchronous lines (`tty1` to `tty8`).
    let async_scopes: Vec<String> = c.cli.async_lines.map(|(a, b)| (a..=b).map(|n| format!("tty{n}")).collect()).unwrap_or_default();
    let (kind, scope_names): (Kind, Vec<String>) = match &c.cli.mode {
        Mode::Config => (Kind::Global, Vec::new()),
        Mode::Line(false) if !async_scopes.is_empty() => (Kind::Line, async_scopes.clone()),
        Mode::Line(vty) => (Kind::Line, vec![(if *vty { "vty" } else { "con" }).to_string()]),
        Mode::Rip => (Kind::Proto, vec!["rip".to_string()]),
        Mode::Eigrp => (Kind::Proto, vec!["eigrp".to_string()]),
        Mode::Loopback(n) => (Kind::Iface, vec![format!("Loopback{n}")]),
        Mode::Iface(_) | Mode::Svi(_) => (Kind::Iface, c.mode_iface_names()?),
        _ => return Err("% That setting is not available in this mode".into()),
    };
    let d = c.dm()?;
    let mut scopes: Vec<Scope> = Vec::new();
    if kind == Kind::Global {
        scopes.push(Scope::Global);
    }
    for n in &scope_names {
        scopes.push(match kind {
            Kind::Line => Scope::Line(n),
            Kind::Proto => Scope::Proto(n),
            _ => Scope::Iface(n),
        });
    }
    for s in scopes {
        match &keep {
            Some(l) => d.knobs.set(s, &key, l.clone())?,
            None => d.knobs.clear(s, &key),
        }
    }
    Ok(if keep.is_some() && !key.starts_with("alias ") && !ACTIVE.contains(&key.as_str()) { KEPT.to_string() } else { String::new() })
}

/// `show aliases`: what each alias stands for, by mode.
pub(super) fn aliases(d: &netlabcraft_model::Device) -> String {
    let mut s = String::new();
    for scope in ["exec", "configure", "interface"] {
        let mine: Vec<(&str, &str)> = d
            .knobs
            .global
            .iter()
            .filter_map(|(k, l)| k.strip_prefix(&format!("alias {scope} ")).map(|name| (name, l.splitn(4, ' ').nth(3).unwrap_or(""))))
            .collect();
        if mine.is_empty() {
            continue;
        }
        s.push_str(&format!("{scope} mode aliases:\n"));
        for (name, text) in mine {
            s.push_str(&format!("  {name:<10} {text}\n"));
        }
    }
    if s.is_empty() { "No aliases are configured (`alias exec NAME command`).".to_string() } else { s.trim_end().to_string() }
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

/// The settings under `router rip` or `router eigrp`, written inside the process block.
pub(crate) fn proto_lines(s: &mut String, d: &Device, process: &str) {
    for l in d.knobs.written(Scope::Proto(process)) {
        let _ = writeln!(s, " {l}");
    }
}

/// A line group's settings (`con` or `vty`), written inside its block.
pub(crate) fn line_lines(s: &mut String, d: &Device, group: &str) {
    for l in d.knobs.written(Scope::Line(group)) {
        let _ = writeln!(s, " {l}");
    }
}
