//! QoS commands: `class-map`, `policy-map` (with its `class` sub-mode), `service-policy` on
//! interfaces, and the `show class-map` / `show policy-map [interface]` reports.

use std::fmt::Write as _;

use netlabcraft_model::qos::{
    AppProto, CLASS_DEFAULT, ClassMap, MAX_APPLIED, MAX_LINES, MAX_MAPS, MAX_NAME, MAX_VALUES, Match, Police, PoliceAction, PolicyClass, PolicyMap,
    Rate,
};
use netlabcraft_model::{Device, DeviceKind, ifname};
use netlabcraft_sim::Sim;
use netlabcraft_sim::qos::{dscp_name, parse_dscp};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// IP precedence names (RFC 791), by value.
const PRECEDENCE_NAMES: [&str; 8] = ["routine", "priority", "immediate", "flash", "flash-override", "critical", "internet", "network"];
/// Highest rate accepted for police and shape (bits per second).
const MAX_BPS: u64 = 100_000_000_000;

fn name_ok(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME || !t.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% Give a name of 1-{MAX_NAME} printable characters"));
    }
    Ok(t.to_string())
}

fn precedence(text: &str) -> Result<u8, String> {
    let t = text.trim().to_ascii_lowercase();
    if let Some(i) = PRECEDENCE_NAMES.iter().position(|n| *n == t) {
        return u8::try_from(i).map_err(|_| "% Precedence is 0-7".to_string());
    }
    t.parse::<u8>().ok().filter(|p| *p <= 7).ok_or_else(|| format!("% `{text}` is not a precedence (0-7, or routine … network)"))
}

fn dscp(text: &str) -> Result<u8, String> {
    parse_dscp(text).ok_or_else(|| format!("% `{text}` is not a DSCP (0-63, or ef, af11-af43, cs0-cs7, default)"))
}

/// Several values on one match line.
fn values(words: &[&str], one: fn(&str) -> Result<u8, String>) -> Result<Vec<u8>, String> {
    if words.is_empty() {
        return Err("% Give at least one value".into());
    }
    if words.len() > MAX_VALUES {
        return Err(format!("% At most {MAX_VALUES} values per line"));
    }
    let mut out = Vec::new();
    for w in words {
        let v = one(w)?;
        if !out.contains(&v) {
            out.push(v);
        }
    }
    Ok(out)
}

/// A match criterion from its words (`dscp ef af41`, `ip precedence 5`, `access-group name X`,
/// `protocol http`, `any`).
fn parse_match(words: &[&str]) -> Result<Match, String> {
    let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
    let l: Vec<&str> = lower.iter().map(String::as_str).collect();
    let rest = |n: usize| words.get(n..).unwrap_or(&[]);
    match l.as_slice() {
        ["any"] => Ok(Match::Any),
        ["dscp", ..] => Ok(Match::Dscp(values(rest(1), dscp)?)),
        ["ip", "dscp", ..] => Ok(Match::Dscp(values(rest(2), dscp)?)),
        ["precedence", ..] => Ok(Match::Precedence(values(rest(1), precedence)?)),
        ["ip", "precedence", ..] => Ok(Match::Precedence(values(rest(2), precedence)?)),
        ["access-group", "name", _] => Ok(Match::AccessGroup(name_ok(words.get(2).copied().unwrap_or(""))?)),
        ["access-group", n] => {
            if netlabcraft_model::acl::numbered_kind(n).is_none() {
                return Err("% A numbered access list is 1-199 or 1300-2699 (or use `match access-group name NAME`)".into());
            }
            Ok(Match::AccessGroup((*n).to_string()))
        }
        ["protocol", p] => AppProto::parse(p)
            .map(Match::Protocol)
            .ok_or_else(|| format!("% `{p}` is not a protocol here: {}", AppProto::ALL.iter().map(|x| x.name()).collect::<Vec<_>>().join(", "))),
        _ => Err("% Match on: dscp, ip precedence, access-group, protocol or any".into()),
    }
}

fn match_text(m: &Match) -> String {
    match m {
        Match::Any => "any".into(),
        Match::Dscp(v) => format!("dscp {}", v.iter().map(|x| dscp_name(*x)).collect::<Vec<_>>().join(" ")),
        Match::Precedence(v) => format!("ip precedence {}", v.iter().map(u8::to_string).collect::<Vec<_>>().join(" ")),
        Match::AccessGroup(n) if netlabcraft_model::acl::numbered_kind(n).is_some() => format!("access-group {n}"),
        Match::AccessGroup(n) => format!("access-group name {n}"),
        Match::Protocol(p) => format!("protocol {}", p.name()),
    }
}

fn action_text(a: PoliceAction) -> String {
    match a {
        PoliceAction::Transmit => "transmit".into(),
        PoliceAction::Drop => "drop".into(),
        PoliceAction::SetDscpTransmit(v) => format!("set-dscp-transmit {}", dscp_name(v)),
    }
}

fn rate_text(word: &str, r: Rate) -> String {
    match r {
        Rate::Kbps(k) => format!("{word} {k}"),
        Rate::Percent(p) => format!("{word} percent {p}"),
    }
}

/// The configuration lines under one `class` of a policy map (two-space indent).
fn class_lines(pc: &PolicyClass) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = pc.set_dscp {
        out.push(format!("set dscp {}", dscp_name(v)));
    }
    if let Some(v) = pc.set_precedence {
        out.push(format!("set ip precedence {v}"));
    }
    if let Some(r) = pc.priority {
        out.push(rate_text("priority", r));
    }
    if let Some(r) = pc.bandwidth {
        out.push(rate_text("bandwidth", r));
    }
    if let Some(p) = pc.police {
        let burst = p.burst.map(|b| format!(" {b}")).unwrap_or_default();
        out.push(format!("police {}{burst} conform-action {} exceed-action {}", p.rate_bps, action_text(p.conform), action_text(p.exceed)));
    }
    if let Some(b) = pc.shape {
        out.push(format!("shape average {b}"));
    }
    out
}

/// The QoS part of the running configuration: class maps, then policy maps.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    for cm in &d.qos.class_maps {
        let _ = writeln!(s, "class-map {} {}", if cm.any { "match-any" } else { "match-all" }, cm.name);
        for m in &cm.matches {
            let _ = writeln!(s, " match {}", match_text(m));
        }
        let _ = writeln!(s, "!");
    }
    for pm in &d.qos.policy_maps {
        let _ = writeln!(s, "policy-map {}", pm.name);
        for pc in &pm.classes {
            let _ = writeln!(s, " class {}", pc.class);
            for l in class_lines(pc) {
                let _ = writeln!(s, "  {l}");
            }
        }
        let _ = writeln!(s, "!");
    }
}

/// ` service-policy input|output NAME` under an interface that has them.
pub(crate) fn service_policy_lines(s: &mut String, d: &Device, iface: &str) {
    if let Some(sp) = d.qos.applied.get(iface) {
        if let Some(p) = &sp.input {
            let _ = writeln!(s, " service-policy input {p}");
        }
        if let Some(p) = &sp.output {
            let _ = writeln!(s, " service-policy output {p}");
        }
    }
}

/// Whether two interface names (any spelling) are the same interface.
fn same_iface(a: &str, b: &str) -> bool {
    use netlabcraft_model::vpn::tunnel_number;
    if let (Some(x), Some(y)) = (tunnel_number(a), tunnel_number(b)) {
        return x == y;
    }
    if let (Some(x), Some(y)) = (ifname::parse(a), ifname::parse(b)) {
        return x == y;
    }
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    compact(a) == compact(b)
}

/// The interfaces the current interface mode configures, checked to carry routed traffic.
fn policy_ifaces(c: &Ctx) -> Result<Vec<String>, String> {
    let d = c.d()?;
    let switch = d.kind == DeviceKind::Switch;
    if switch && !d.ip_routing {
        return Err("% Service policies act on routed traffic: turn on `ip routing` first".into());
    }
    match &c.cli.mode {
        Mode::Svi(v) => Ok(vec![format!("Vlan{v}")]),
        Mode::Tunnel(n) => Ok(vec![format!("Tunnel{n}")]),
        Mode::Iface(ifs) => {
            let mut out = Vec::new();
            for r in ifs {
                let p = d.ports.get(r.port).ok_or("% No such port")?;
                if switch && !p.routed {
                    return Err(format!("% {} is a switch port: service policies need a routed port (`no switchport`) or a VLAN interface", p.name));
                }
                out.push(r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)));
            }
            Ok(out)
        }
        _ => Err("% Not in interface mode".into()),
    }
}

/// `input` or `output` (any unambiguous prefix): `true` for input.
fn direction(text: &str) -> Result<bool, String> {
    let t = text.trim().to_ascii_lowercase();
    if !t.is_empty() && "input".starts_with(&t) {
        Ok(true)
    } else if !t.is_empty() && "output".starts_with(&t) {
        Ok(false)
    } else {
        Err("% `input` or `output`".into())
    }
}

/// The class being configured (policy-map class mode).
fn current<'a>(c: &'a mut Ctx) -> Result<&'a mut PolicyClass, String> {
    let Mode::PolicyClass(p, k) = c.cli.mode.clone() else { return Err("% Not in a policy map's class".into()) };
    c.dm()?
        .qos
        .policy_maps
        .iter_mut()
        .find(|x| x.name == p)
        .and_then(|x| x.classes.iter_mut().find(|y| y.class == k))
        .ok_or_else(|| "% The class was removed from the policy map".to_string())
}

fn kbps(text: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|k| (8..=10_000_000).contains(k)).ok_or_else(|| "% The rate is 8-10000000 kbit/s".to_string())
}

fn percent(text: &str) -> Result<u8, String> {
    text.trim().parse::<u8>().ok().filter(|p| (1..=100).contains(p)).ok_or_else(|| "% The share is 1-100 percent".to_string())
}

fn bps(text: &str) -> Result<u64, String> {
    text.trim().parse::<u64>().ok().filter(|b| (8_000..=MAX_BPS).contains(b)).ok_or_else(|| format!("% The rate is 8000-{MAX_BPS} bit/s"))
}

/// `police [cir] BPS [BURST] [conform-action A] [exceed-action A]`.
fn parse_police(text: &str) -> Result<Police, String> {
    let words: Vec<String> = text.split_whitespace().map(str::to_ascii_lowercase).collect();
    let mut it = words.iter().map(String::as_str).peekable();
    if it.peek() == Some(&"cir") {
        it.next();
    }
    let rate_bps = bps(it.next().ok_or("% Give the rate in bit/s")?)?;
    let mut burst = None;
    if let Some(w) = it.peek().filter(|w| w.chars().all(|ch| ch.is_ascii_digit())) {
        burst = Some(w.parse::<u32>().ok().filter(|b| (1_000..=512_000_000).contains(b)).ok_or("% The burst is 1000-512000000 bytes")?);
        it.next();
    }
    let mut p = Police { rate_bps, burst, conform: PoliceAction::Transmit, exceed: PoliceAction::Drop };
    while let Some(w) = it.next() {
        let slot = match w {
            "conform-action" => &mut p.conform,
            "exceed-action" => &mut p.exceed,
            other => return Err(format!("% `{other}` not understood: conform-action or exceed-action follow the rate")),
        };
        *slot = match it.next() {
            Some("transmit") => PoliceAction::Transmit,
            Some("drop") => PoliceAction::Drop,
            Some("set-dscp-transmit") => PoliceAction::SetDscpTransmit(dscp(it.next().ok_or("% Give the DSCP to mark with")?)?),
            _ => return Err("% An action is transmit, drop or set-dscp-transmit DSCP".into()),
        };
    }
    Ok(p)
}

/// Whether a policy queues (only output policies may).
fn queues(p: &PolicyMap) -> bool {
    p.classes.iter().any(|c| c.priority.is_some() || c.bandwidth.is_some() || c.shape.is_some())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim();
    match id {
        Id::ClassMap | Id::NoClassMap => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (any, name) = match words.as_slice() {
                [t, n] if t.eq_ignore_ascii_case("match-any") => (Some(true), *n),
                [t, n] if t.eq_ignore_ascii_case("match-all") => (Some(false), *n),
                [n] => (None, *n),
                _ => return Err("% Usage: class-map [match-all|match-any] NAME".into()),
            };
            let name = name_ok(name)?;
            if name.eq_ignore_ascii_case(CLASS_DEFAULT) || name.eq_ignore_ascii_case("match-any") || name.eq_ignore_ascii_case("match-all") {
                return Err(format!("% `{name}` is reserved"));
            }
            let qos = &mut c.dm()?.qos;
            if id == Id::NoClassMap {
                if let Some(p) = qos.policy_maps.iter().find(|p| p.classes.iter().any(|k| k.class == name)) {
                    return Err(format!("% Class map {name} is used by policy map {}: remove it there first (`no class {name}`)", p.name));
                }
                let before = qos.class_maps.len();
                qos.class_maps.retain(|x| x.name != name);
                return if qos.class_maps.len() == before { Err(format!("% There is no class map {name}")) } else { Ok(String::new()) };
            }
            match qos.class_maps.iter_mut().find(|x| x.name == name) {
                Some(cm) => {
                    if let Some(a) = any {
                        cm.any = a;
                    }
                }
                None => {
                    if qos.class_maps.len() >= MAX_MAPS {
                        return Err(format!("% At most {MAX_MAPS} class maps"));
                    }
                    qos.class_maps.push(ClassMap { name: name.clone(), any: any.unwrap_or(false), matches: Vec::new() });
                }
            }
            c.cli.mode = Mode::ClassMap(name);
            Ok(String::new())
        }
        Id::PolicyMap | Id::NoPolicyMap => {
            let name = name_ok(arg(0))?;
            let qos = &mut c.dm()?.qos;
            if id == Id::NoPolicyMap {
                if let Some((i, _)) = qos.applied.iter().find(|(_, sp)| sp.input.as_ref() == Some(&name) || sp.output.as_ref() == Some(&name)) {
                    return Err(format!("% Policy map {name} is applied to {i}: remove its `service-policy` there first"));
                }
                let before = qos.policy_maps.len();
                qos.policy_maps.retain(|x| x.name != name);
                return if qos.policy_maps.len() == before { Err(format!("% There is no policy map {name}")) } else { Ok(String::new()) };
            }
            if qos.policy_map(&name).is_none() {
                if qos.policy_maps.len() >= MAX_MAPS {
                    return Err(format!("% At most {MAX_MAPS} policy maps"));
                }
                qos.policy_maps.push(PolicyMap { name: name.clone(), classes: Vec::new() });
            }
            c.cli.mode = Mode::PolicyMap(name);
            Ok(String::new())
        }
        Id::QosMatchDscp
        | Id::QosMatchPrecedence
        | Id::QosMatchAccessGroup
        | Id::QosMatchAccessGroupName
        | Id::QosMatchProtocol
        | Id::QosMatchAny
        | Id::NoQosMatch => {
            let Mode::ClassMap(name) = c.cli.mode.clone() else { return Err("% Not in class map mode".into()) };
            let m = match id {
                Id::QosMatchDscp => Match::Dscp(values(&arg(0).split_whitespace().collect::<Vec<_>>(), dscp)?),
                Id::QosMatchPrecedence => Match::Precedence(values(&arg(0).split_whitespace().collect::<Vec<_>>(), precedence)?),
                Id::QosMatchAccessGroup => parse_match(&["access-group", arg(0)])?,
                Id::QosMatchAccessGroupName => Match::AccessGroup(name_ok(arg(0))?),
                Id::QosMatchProtocol => parse_match(&["protocol", arg(0)])?,
                Id::QosMatchAny => Match::Any,
                _ => parse_match(&arg(0).split_whitespace().collect::<Vec<_>>())?,
            };
            let note = match &m {
                Match::AccessGroup(a) if id != Id::NoQosMatch && c.d()?.acls.iter().all(|x| &x.name != a) => {
                    format!("Note: access list {a} does not exist yet; this line matches nothing until it does")
                }
                _ => String::new(),
            };
            let cm = c.dm()?.qos.class_maps.iter_mut().find(|x| x.name == name).ok_or("% The class map was deleted")?;
            if id == Id::NoQosMatch {
                let before = cm.matches.len();
                cm.matches.retain(|x| *x != m);
                return if cm.matches.len() == before {
                    Err(format!("% Class map {name} has no line `match {}`", match_text(&m)))
                } else {
                    Ok(String::new())
                };
            }
            if !cm.matches.contains(&m) {
                if cm.matches.len() >= MAX_LINES {
                    return Err(format!("% At most {MAX_LINES} match lines"));
                }
                cm.matches.push(m);
            }
            Ok(note)
        }
        Id::PolicyClass | Id::NoPolicyClass => {
            let policy = match &c.cli.mode {
                Mode::PolicyMap(p) | Mode::PolicyClass(p, _) => p.clone(),
                _ => return Err("% Not in policy map mode".into()),
            };
            let class = name_ok(arg(0))?;
            let class = if class.eq_ignore_ascii_case(CLASS_DEFAULT) { CLASS_DEFAULT.to_string() } else { class };
            if id == Id::PolicyClass && class != CLASS_DEFAULT && c.d()?.qos.class_map(&class).is_none() {
                return Err(format!("% There is no class map {class}: define it first with `class-map {class}`"));
            }
            let pm = c.dm()?.qos.policy_maps.iter_mut().find(|x| x.name == policy).ok_or("% The policy map was deleted")?;
            if id == Id::NoPolicyClass {
                let before = pm.classes.len();
                pm.classes.retain(|x| x.class != class);
                if pm.classes.len() == before {
                    return Err(format!("% Policy map {policy} has no class {class}"));
                }
                c.cli.mode = Mode::PolicyMap(policy);
                return Ok(String::new());
            }
            if !pm.classes.iter().any(|x| x.class == class) {
                if pm.classes.len() >= MAX_LINES {
                    return Err(format!("% At most {MAX_LINES} classes per policy map"));
                }
                pm.classes.push(PolicyClass::new(&class));
                pm.order();
            }
            c.cli.mode = Mode::PolicyClass(policy, class);
            Ok(String::new())
        }
        Id::QosSetDscp => {
            let v = dscp(arg(0))?;
            let pc = current(c)?;
            pc.set_dscp = Some(v);
            pc.set_precedence = None;
            Ok(String::new())
        }
        Id::QosSetPrecedence => {
            let v = precedence(arg(0))?;
            let pc = current(c)?;
            pc.set_precedence = Some(v);
            pc.set_dscp = None;
            Ok(String::new())
        }
        Id::NoQosSet => {
            let lower = arg(0).to_ascii_lowercase();
            let words: Vec<&str> = lower.split_whitespace().collect();
            let pc = current(c)?;
            match words.as_slice() {
                ["dscp", ..] | ["ip", "dscp", ..] => pc.set_dscp = None,
                ["precedence", ..] | ["ip", "precedence", ..] => pc.set_precedence = None,
                _ => return Err("% no set dscp, or no set ip precedence".into()),
            }
            Ok(String::new())
        }
        Id::QosPriority | Id::QosPriorityPercent | Id::QosBandwidth | Id::QosBandwidthPercent => {
            let rate = match id {
                Id::QosPriority | Id::QosBandwidth => Rate::Kbps(kbps(arg(0))?),
                _ => Rate::Percent(percent(arg(0))?),
            };
            let priority = matches!(id, Id::QosPriority | Id::QosPriorityPercent);
            let pc = current(c)?;
            if (priority && pc.bandwidth.is_some()) || (!priority && pc.priority.is_some()) {
                return Err("% A class gets either `priority` or `bandwidth`, not both".into());
            }
            if priority && pc.class == CLASS_DEFAULT {
                return Err("% class-default cannot be the priority class".into());
            }
            if priority {
                pc.priority = Some(rate);
            } else {
                pc.bandwidth = Some(rate);
            }
            Ok("Note: queued in an output policy when the interface is congested (it sends at its clock rate, bandwidth or speed)".into())
        }
        Id::NoQosPriority => {
            current(c)?.priority = None;
            Ok(String::new())
        }
        Id::NoQosBandwidth => {
            current(c)?.bandwidth = None;
            Ok(String::new())
        }
        Id::QosPolice => {
            let p = parse_police(arg(0))?;
            current(c)?.police = Some(p);
            Ok(String::new())
        }
        Id::NoQosPolice => {
            current(c)?.police = None;
            Ok(String::new())
        }
        Id::QosShape => {
            let b = bps(arg(0))?;
            current(c)?.shape = Some(b);
            Ok("Note: kept and counted; links here never congest, so shaping delays nothing".into())
        }
        Id::NoQosShape => {
            current(c)?.shape = None;
            Ok(String::new())
        }
        Id::ServicePolicy => {
            let input = direction(arg(0))?;
            let name = name_ok(arg(1))?;
            let pm = c.d()?.qos.policy_map(&name).ok_or_else(|| format!("% There is no policy map {name}: define it with `policy-map {name}`"))?;
            if input && queues(pm) {
                return Err(format!("% Policy map {name} queues (priority, bandwidth or shape): apply it as `output`"));
            }
            let names = policy_ifaces(c)?;
            let qos = &mut c.dm()?.qos;
            for n in &names {
                if !qos.applied.contains_key(n) && qos.applied.len() >= MAX_APPLIED {
                    return Err(format!("% At most {MAX_APPLIED} interfaces with service policies"));
                }
                let sp = qos.applied.entry(n.clone()).or_default();
                if input {
                    sp.input = Some(name.clone());
                } else {
                    sp.output = Some(name.clone());
                }
            }
            Ok(String::new())
        }
        Id::NoServicePolicy => {
            let input = direction(arg(0))?;
            let names = policy_ifaces(c)?;
            let qos = &mut c.dm()?.qos;
            for n in &names {
                if let Some(sp) = qos.applied.get_mut(n) {
                    if input {
                        sp.input = None;
                    } else {
                        sp.output = None;
                    }
                    if sp.input.is_none() && sp.output.is_none() {
                        qos.applied.remove(n);
                    }
                }
            }
            Ok(String::new())
        }
        Id::ShowClassMap => show_class_maps(c.d()?, arg(0)),
        Id::ShowPolicyMap => show_policy_maps(c.d()?, arg(0)),
        Id::ShowPolicyMapInterface => Ok(show_policy_interfaces(c.sim, c.d()?, arg(0))),
        _ => Err("% Not a QoS command".into()),
    }
}

/// `show class-map [NAME]`.
fn show_class_maps(d: &Device, only: &str) -> R {
    let maps: Vec<&ClassMap> = d.qos.class_maps.iter().filter(|m| only.is_empty() || m.name == only).collect();
    if maps.is_empty() {
        return if only.is_empty() { Ok("No class maps".into()) } else { Err(format!("% There is no class map {only}")) };
    }
    let mut s = String::new();
    for m in maps {
        let _ = writeln!(
            s,
            "Class map {} ({}: {})",
            m.name,
            if m.any { "match-any" } else { "match-all" },
            if m.any { "one line is enough" } else { "every line must match" }
        );
        if m.matches.is_empty() {
            let _ = writeln!(s, "  (no match lines: takes no packets)");
        }
        for x in &m.matches {
            let _ = writeln!(s, "  match {}", match_text(x));
        }
    }
    Ok(s.trim_end().to_string())
}

/// `show policy-map [NAME]`.
fn show_policy_maps(d: &Device, only: &str) -> R {
    let maps: Vec<&PolicyMap> = d.qos.policy_maps.iter().filter(|m| only.is_empty() || m.name == only).collect();
    if maps.is_empty() {
        return if only.is_empty() { Ok("No policy maps".into()) } else { Err(format!("% There is no policy map {only}")) };
    }
    let mut s = String::new();
    for m in maps {
        let _ = writeln!(s, "Policy map {}", m.name);
        for pc in &m.classes {
            let _ = writeln!(s, "  class {}", pc.class);
            let lines = class_lines(pc);
            if lines.is_empty() {
                let _ = writeln!(s, "    (no actions: counted only)");
            }
            for l in lines {
                let _ = writeln!(s, "    {l}");
            }
        }
    }
    Ok(s.trim_end().to_string())
}

fn plural(n: u64, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// One policy's per-class report on one interface and direction.
fn policy_report(s: &mut String, sim: &Sim, d: &Device, iface: &str, input: bool, name: &str) {
    let _ = writeln!(s, "  {} policy {name}", if input { "Input" } else { "Output" });
    if !input {
        let _ = writeln!(s, "    Link rate {} kbit/s", netlabcraft_sim::qos::link_rate_bps(d, iface) / 1_000);
    }
    let Some(pm) = d.qos.policy_map(name) else {
        let _ = writeln!(s, "    (the policy map does not exist: nothing is done)");
        return;
    };
    let mut classes: Vec<PolicyClass> = pm.classes.clone();
    if !classes.iter().any(|c| c.class == CLASS_DEFAULT) {
        classes.push(PolicyClass::new(CLASS_DEFAULT));
    }
    for pc in &classes {
        let how = if pc.class == CLASS_DEFAULT {
            "everything else".to_string()
        } else {
            match d.qos.class_map(&pc.class) {
                Some(cm) if cm.matches.is_empty() => "no match lines".to_string(),
                Some(cm) => format!(
                    "{}: {}",
                    if cm.any { "match-any" } else { "match-all" },
                    cm.matches.iter().map(match_text).collect::<Vec<_>>().join("; ")
                ),
                None => "class map missing: takes nothing".to_string(),
            }
        };
        let st = sim.qos_stats(d.id, iface, input, name, &pc.class);
        let _ = writeln!(s, "    Class {} ({how})", pc.class);
        let _ = writeln!(s, "      matched {}, {}", plural(st.packets, "packet"), plural(st.bytes, "byte"));
        if let Some(v) = pc.set_dscp {
            let _ = writeln!(s, "      set dscp {}: {} marked", dscp_name(v), plural(st.marked, "packet"));
        } else if let Some(v) = pc.set_precedence {
            let _ = writeln!(s, "      set ip precedence {v}: {} marked", plural(st.marked, "packet"));
        } else if st.marked > 0 {
            let _ = writeln!(s, "      re-marked by the policer: {}", plural(st.marked, "packet"));
        }
        if let Some(p) = pc.police {
            let _ = writeln!(
                s,
                "      police {} bit/s, bucket {} bytes: {} conformed ({}), {} exceeded ({}), {} dropped",
                p.rate_bps,
                p.depth(),
                st.conformed,
                action_text(p.conform),
                st.exceeded,
                action_text(p.exceed),
                st.dropped
            );
        }
        for (what, r) in [("priority", pc.priority), ("bandwidth", pc.bandwidth)] {
            if let Some(r) = r {
                let amount = match r {
                    Rate::Kbps(k) => format!("{k} kbit/s"),
                    Rate::Percent(p) => format!("{p}% of the link"),
                };
                if input {
                    let _ = writeln!(s, "      {what} {amount}: only output policies queue");
                } else {
                    let _ = writeln!(
                        s,
                        "      {what} {amount}: queue {} now (most {}), {} tail-dropped",
                        plural(st.queued_bytes(), "byte"),
                        plural(st.max_queued, "byte"),
                        plural(st.tail_drops, "packet")
                    );
                }
            }
        }
        if let Some(b) = pc.shape {
            if input {
                let _ = writeln!(s, "      shape average {b} bit/s: only output policies shape");
            } else {
                let _ = writeln!(
                    s,
                    "      shape average {b} bit/s, bucket {} bytes: {} delayed, {} tail-dropped",
                    netlabcraft_sim::qos::shape_bc(b),
                    plural(st.shaped_delayed, "packet"),
                    plural(st.shaped_drops, "packet")
                );
            }
        }
    }
}

/// `show policy-map interface [IFACE]`.
fn show_policy_interfaces(sim: &Sim, d: &Device, only: &str) -> String {
    let mut s = String::new();
    for (iface, sp) in d.qos.applied.iter().filter(|(i, _)| only.is_empty() || same_iface(i, only)) {
        let _ = writeln!(s, "{iface}");
        if let Some(p) = &sp.input {
            policy_report(&mut s, sim, d, iface, true, p);
        }
        if let Some(p) = &sp.output {
            policy_report(&mut s, sim, d, iface, false, p);
        }
    }
    if s.is_empty() {
        return if only.is_empty() { "No service policies applied".into() } else { format!("No service policy on {only}") };
    }
    s.trim_end().to_string()
}
