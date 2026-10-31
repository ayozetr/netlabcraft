//! Router security commands: the zone-based policy firewall (`zone security`, `class-map type
//! inspect`, `policy-map type inspect`, `zone-pair security`, `zone-member security`), classic
//! inspection (`ip inspect`), and their reports.

use std::fmt::Write as _;

use netlabcraft_model::security::{
    CLASS_DEFAULT, InspectAction, InspectBinding, InspectClass, InspectMatch, InspectPolicy, InspectPolicyClass, InspectRule, MAX_IFACES, MAX_ITEMS,
    MAX_LINES, MAX_MSS, MAX_MTU, MAX_NAME, MIN_MSS, MIN_MTU, PROTOCOLS, SELF_ZONE, Zone, ZonePair, protocol,
};
use netlabcraft_model::{Device, DeviceKind, MAX_DESCRIPTION};
use netlabcraft_sim::Sim;
use netlabcraft_sim::zfw::{Owner, Session};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn router_only(c: &Ctx) -> Result<(), String> {
    if c.d()?.kind != DeviceKind::Router {
        return Err("% Zones and inspection are router features".into());
    }
    Ok(())
}

fn name_ok(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME || !t.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% Give a name of 1-{MAX_NAME} printable characters"));
    }
    Ok(t.to_string())
}

/// The names of the interfaces the current interface mode configures.
pub(super) fn mode_ifaces(c: &Ctx) -> Result<Vec<String>, String> {
    match &c.cli.mode {
        Mode::Iface(ifs) => {
            let d = c.d()?;
            Ok(ifs.iter().filter_map(|r| d.ports.get(r.port).map(|p| r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))).collect())
        }
        Mode::Tunnel(n) => Ok(vec![format!("Tunnel{n}")]),
        _ => Err("% Not in interface mode".into()),
    }
}

fn protocol_name(text: &str) -> Result<String, String> {
    let t = text.trim().to_ascii_lowercase();
    if protocol(&t).is_none() {
        let known: Vec<&str> = PROTOCOLS.iter().map(|p| p.0).collect();
        return Err(format!("% Unknown protocol `{}`; one of {}", text.trim(), known.join(", ")));
    }
    Ok(t)
}

fn describe_match(m: &InspectMatch) -> String {
    match m {
        InspectMatch::Protocol(p) => format!("protocol {p}"),
        InspectMatch::AccessGroup(a) if a.chars().all(|ch| ch.is_ascii_digit()) => format!("access-group {a}"),
        InspectMatch::AccessGroup(a) => format!("access-group name {a}"),
        InspectMatch::ClassMap(n) => format!("class-map {n}"),
    }
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim().to_string();
    router_only(c)?;
    match id {
        Id::ZoneSecurity => {
            let name = name_ok(&arg(0))?;
            if name.eq_ignore_ascii_case(SELF_ZONE) {
                return Err("% `self` is the router's own zone; it always exists".into());
            }
            let sec = &mut c.dm()?.security;
            if sec.zone(&name).is_none() {
                if sec.zones.len() >= MAX_ITEMS {
                    return Err(format!("% At most {MAX_ITEMS} zones"));
                }
                sec.zones.push(Zone { name: name.clone(), description: String::new() });
            }
            c.cli.mode = Mode::Zone(name);
            Ok(String::new())
        }
        Id::NoZoneSecurity => {
            let name = arg(0);
            let sec = &mut c.dm()?.security;
            if sec.zone(&name).is_none() {
                return Err(format!("% There is no zone {name}"));
            }
            if sec.members.values().any(|z| *z == name) {
                return Err(format!("% Zone {name} still has member interfaces; take them out first (no zone-member security)"));
            }
            if let Some(p) = sec.pairs.iter().find(|p| p.source == name || p.destination == name) {
                return Err(format!("% Zone {name} is used by zone pair {}; delete it first", p.name));
            }
            sec.zones.retain(|z| z.name != name);
            Ok(String::new())
        }
        Id::ZoneDescription | Id::NoZoneDescription => {
            let text: String = if id == Id::ZoneDescription { arg(0).chars().take(MAX_DESCRIPTION).collect() } else { String::new() };
            match c.cli.mode.clone() {
                Mode::Zone(z) => {
                    c.dm()?.security.zones.iter_mut().find(|x| x.name == z).ok_or("% The zone was deleted")?.description = text;
                }
                Mode::ZonePair(_) => return Ok(String::new()),
                _ => return Err("% Not in zone mode".into()),
            }
            Ok(String::new())
        }
        Id::InspectClassMap | Id::NoInspectClassMap => {
            let words: Vec<String> = arg(0).split_whitespace().map(str::to_string).collect();
            let (match_all, name) = match words.as_slice() {
                [kind, n] if kind.eq_ignore_ascii_case("match-any") => (false, n.clone()),
                [kind, n] if kind.eq_ignore_ascii_case("match-all") => (true, n.clone()),
                [n] => (true, n.clone()),
                _ => return Err("% Usage: class-map type inspect [match-any|match-all] NAME".into()),
            };
            let name = name_ok(&name)?;
            let sec = &mut c.dm()?.security;
            if id == Id::NoInspectClassMap {
                if sec.class(&name).is_none() {
                    return Err(format!("% There is no inspect class {name}"));
                }
                if let Some(p) = sec.policies.iter().find(|p| p.classes.iter().any(|x| x.class == name)) {
                    return Err(format!("% Class {name} is used by policy {}; remove it there first", p.name));
                }
                sec.classes.retain(|x| x.name != name);
                return Ok(String::new());
            }
            let full = sec.classes.len() >= MAX_ITEMS;
            match sec.classes.iter_mut().find(|x| x.name == name) {
                Some(x) => x.match_all = match_all,
                None if full => return Err(format!("% At most {MAX_ITEMS} inspect classes")),
                None => sec.classes.push(InspectClass { name: name.clone(), match_all, matches: Vec::new() }),
            }
            c.cli.mode = Mode::InspectClass(name);
            Ok(String::new())
        }
        Id::InspectMatchProtocol | Id::InspectMatchAcl | Id::InspectMatchClass | Id::NoInspectMatch => {
            let Mode::InspectClass(class) = c.cli.mode.clone() else { return Err("% Not in an inspect class map".into()) };
            let m = match id {
                Id::InspectMatchProtocol => InspectMatch::Protocol(protocol_name(&arg(0))?),
                Id::InspectMatchClass => InspectMatch::ClassMap(name_ok(&arg(0))?),
                // `match access-group N` and `match access-group name NAME` share the id: a
                // number is a numbered list, anything else a named one.
                Id::InspectMatchAcl => acl_match(&arg(0))?,
                _ => {
                    let line = arg(0);
                    let words: Vec<&str> = line.split_whitespace().collect();
                    parse_match_words(&words)?
                }
            };
            if let InspectMatch::ClassMap(n) = &m {
                if *n == class {
                    return Err("% A class cannot match itself".into());
                }
                if c.d()?.security.class(n).is_none() {
                    return Err(format!("% There is no inspect class {n}"));
                }
            }
            let sec = &mut c.dm()?.security;
            let cl = sec.classes.iter_mut().find(|x| x.name == class).ok_or("% The class was deleted")?;
            if id == Id::NoInspectMatch {
                let before = cl.matches.len();
                cl.matches.retain(|x| *x != m);
                if cl.matches.len() == before {
                    return Err(format!("% Class {class} has no `match {}` line", describe_match(&m)));
                }
                return Ok(String::new());
            }
            if !cl.matches.contains(&m) {
                if cl.matches.len() >= MAX_LINES {
                    return Err(format!("% At most {MAX_LINES} match lines"));
                }
                cl.matches.push(m);
            }
            Ok(String::new())
        }
        Id::InspectPolicyMap | Id::NoInspectPolicyMap => {
            let name = name_ok(&arg(0))?;
            let sec = &mut c.dm()?.security;
            if id == Id::NoInspectPolicyMap {
                if sec.policy(&name).is_none() {
                    return Err(format!("% There is no inspect policy {name}"));
                }
                if let Some(p) = sec.pairs.iter().find(|p| p.policy.as_deref() == Some(name.as_str())) {
                    return Err(format!("% Policy {name} is used by zone pair {}; remove it there first", p.name));
                }
                sec.policies.retain(|p| p.name != name);
                return Ok(String::new());
            }
            if sec.policy(&name).is_none() {
                if sec.policies.len() >= MAX_ITEMS {
                    return Err(format!("% At most {MAX_ITEMS} inspect policies"));
                }
                sec.policies.push(InspectPolicy { name: name.clone(), classes: Vec::new() });
            }
            c.cli.mode = Mode::InspectPolicy(name);
            Ok(String::new())
        }
        Id::InspectPolicyClass | Id::NoInspectPolicyClass => {
            let policy = match c.cli.mode.clone() {
                Mode::InspectPolicy(p) | Mode::InspectPolicyClass(p, _) => p,
                _ => return Err("% Not in an inspect policy map".into()),
            };
            let class = if args.is_empty() { CLASS_DEFAULT.to_string() } else { name_ok(&arg(0))? };
            let default = class.eq_ignore_ascii_case(CLASS_DEFAULT);
            if !default && c.d()?.security.class(&class).is_none() {
                return Err(format!("% There is no inspect class {class} (create it with class-map type inspect)"));
            }
            let sec = &mut c.dm()?.security;
            let p = sec.policies.iter_mut().find(|x| x.name == policy).ok_or("% The policy was deleted")?;
            if id == Id::NoInspectPolicyClass {
                let before = p.classes.len();
                p.classes.retain(|x| x.class != class);
                if p.classes.len() == before {
                    return Err(format!("% Policy {policy} has no class {class}"));
                }
                c.cli.mode = Mode::InspectPolicy(policy);
                return Ok(String::new());
            }
            if !p.classes.iter().any(|x| x.class == class) {
                if p.classes.len() >= MAX_LINES {
                    return Err(format!("% At most {MAX_LINES} classes in a policy"));
                }
                p.classes.push(InspectPolicyClass { class: class.clone(), action: None, log: false });
                p.order();
            }
            c.cli.mode = Mode::InspectPolicyClass(policy, class);
            Ok(String::new())
        }
        Id::InspectActionInspect
        | Id::InspectActionPass
        | Id::InspectActionPassLog
        | Id::InspectActionDrop
        | Id::InspectActionDropLog
        | Id::NoInspectAction => {
            let Mode::InspectPolicyClass(policy, class) = c.cli.mode.clone() else { return Err("% Not in a policy's class".into()) };
            let (action, log) = match id {
                Id::InspectActionInspect => (InspectAction::Inspect, false),
                Id::InspectActionPass => (InspectAction::Pass, false),
                Id::InspectActionPassLog => (InspectAction::Pass, true),
                Id::InspectActionDropLog => (InspectAction::Drop, true),
                _ => (InspectAction::Drop, false),
            };
            if action == InspectAction::Inspect && class.eq_ignore_ascii_case(CLASS_DEFAULT) && id != Id::NoInspectAction {
                return Err("% class-default can only pass or drop (inspection needs a class that names the traffic)".into());
            }
            let sec = &mut c.dm()?.security;
            let p = sec.policies.iter_mut().find(|x| x.name == policy).ok_or("% The policy was deleted")?;
            let pc = p.classes.iter_mut().find(|x| x.class == class).ok_or("% The class was removed from the policy")?;
            if id == Id::NoInspectAction {
                pc.action = None;
                pc.log = false;
            } else {
                pc.action = Some(action);
                pc.log = log && action != InspectAction::Inspect;
            }
            Ok(String::new())
        }
        Id::ZonePairSecurity => {
            let name = name_ok(&arg(0))?;
            let (src, dst) = (name_ok(&arg(1))?, name_ok(&arg(2))?);
            let norm = |z: &str| if z.eq_ignore_ascii_case(SELF_ZONE) { SELF_ZONE.to_string() } else { z.to_string() };
            let (src, dst) = (norm(&src), norm(&dst));
            let sec = &c.d()?.security;
            for z in [&src, &dst] {
                if *z != SELF_ZONE && sec.zone(z).is_none() {
                    return Err(format!("% There is no zone {z} (create it with zone security {z})"));
                }
            }
            if src == dst && src == SELF_ZONE {
                return Err("% A zone pair needs two different zones".into());
            }
            if let Some(p) = sec.pair_between(&src, &dst).filter(|p| p.name != name) {
                return Err(format!("% Zone pair {} already joins {src} to {dst}", p.name));
            }
            let sec = &mut c.dm()?.security;
            let full = sec.pairs.len() >= MAX_ITEMS;
            match sec.pairs.iter_mut().find(|p| p.name == name) {
                Some(p) if p.source != src || p.destination != dst => {
                    return Err(format!("% Zone pair {name} joins {} to {}; delete it to change its zones", p.source, p.destination));
                }
                Some(_) => {}
                None if full => return Err(format!("% At most {MAX_ITEMS} zone pairs")),
                None => sec.pairs.push(ZonePair { name: name.clone(), source: src, destination: dst, policy: None }),
            }
            c.cli.mode = Mode::ZonePair(name);
            Ok(String::new())
        }
        Id::NoZonePairSecurity => {
            let name = arg(0);
            let sec = &mut c.dm()?.security;
            let before = sec.pairs.len();
            sec.pairs.retain(|p| p.name != name);
            if sec.pairs.len() == before {
                return Err(format!("% There is no zone pair {name}"));
            }
            Ok(String::new())
        }
        Id::ZonePairServicePolicy | Id::NoZonePairServicePolicy => {
            let Mode::ZonePair(pair) = c.cli.mode.clone() else { return Err("% Not in zone pair mode".into()) };
            let policy = if id == Id::ZonePairServicePolicy {
                let p = name_ok(&arg(0))?;
                if c.d()?.security.policy(&p).is_none() {
                    return Err(format!("% There is no inspect policy {p} (create it with policy-map type inspect {p})"));
                }
                Some(p)
            } else {
                None
            };
            let sec = &mut c.dm()?.security;
            sec.pairs.iter_mut().find(|p| p.name == pair).ok_or("% The zone pair was deleted")?.policy = policy;
            Ok(String::new())
        }
        Id::ZoneMember | Id::NoZoneMember => {
            let names = mode_ifaces(c)?;
            if id == Id::ZoneMember {
                let zone = name_ok(&arg(0))?;
                if zone.eq_ignore_ascii_case(SELF_ZONE) {
                    return Err("% Interfaces cannot join the self zone (it is the router itself)".into());
                }
                if c.d()?.security.zone(&zone).is_none() {
                    return Err(format!("% There is no zone {zone} (create it with zone security {zone})"));
                }
                let sec = &mut c.dm()?.security;
                for n in names {
                    if sec.members.len() >= MAX_IFACES && !sec.members.contains_key(&n) {
                        return Err("% Too many zone members".into());
                    }
                    sec.members.insert(n, zone.clone());
                }
            } else {
                let sec = &mut c.dm()?.security;
                for n in names {
                    sec.members.remove(&n);
                }
            }
            Ok(String::new())
        }
        Id::IpInspectName => {
            let name = name_ok(&arg(0))?;
            let proto = protocol_name(&arg(1))?;
            let sec = &mut c.dm()?.security;
            let full = sec.inspect_rules.len() >= MAX_ITEMS;
            match sec.inspect_rules.iter_mut().find(|r| r.name == name) {
                Some(r) if r.protocols.contains(&proto) => {}
                Some(r) if r.protocols.len() >= MAX_LINES => return Err(format!("% At most {MAX_LINES} protocols in a rule")),
                Some(r) => r.protocols.push(proto),
                None if full => return Err(format!("% At most {MAX_ITEMS} inspection rules")),
                None => sec.inspect_rules.push(InspectRule { name, protocols: vec![proto] }),
            }
            Ok(String::new())
        }
        Id::NoIpInspectName => {
            let name = arg(0);
            let sec = &mut c.dm()?.security;
            let Some(i) = sec.inspect_rules.iter().position(|r| r.name == name) else { return Err(format!("% There is no inspection rule {name}")) };
            if args.len() > 1 {
                let proto = arg(1).to_ascii_lowercase();
                let r = sec.inspect_rules.get_mut(i).ok_or("% The rule was deleted")?;
                let before = r.protocols.len();
                r.protocols.retain(|p| *p != proto);
                if r.protocols.len() == before {
                    return Err(format!("% Rule {name} does not inspect {proto}"));
                }
                if !r.protocols.is_empty() {
                    return Ok(String::new());
                }
            }
            sec.inspect_rules.remove(i);
            Ok(String::new())
        }
        Id::IpInspectIf | Id::NoIpInspectIf => {
            let names = mode_ifaces(c)?;
            let name = name_ok(&arg(0))?;
            let inbound = match arg(1).to_ascii_lowercase().as_str() {
                "in" => true,
                "out" => false,
                _ => return Err("% Say in or out".into()),
            };
            let sec = &mut c.dm()?.security;
            for n in names {
                if id == Id::IpInspectIf {
                    if sec.inspect_applied.len() >= MAX_IFACES && !sec.inspect_applied.contains_key(&n) {
                        return Err("% Too many interfaces with inspection".into());
                    }
                    let b = sec.inspect_applied.entry(n).or_insert(InspectBinding { inbound: None, outbound: None });
                    if inbound {
                        b.inbound = Some(name.clone());
                    } else {
                        b.outbound = Some(name.clone());
                    }
                } else if let Some(b) = sec.inspect_applied.get_mut(&n) {
                    let slot = if inbound { &mut b.inbound } else { &mut b.outbound };
                    if slot.as_deref() == Some(name.as_str()) {
                        *slot = None;
                    }
                }
            }
            sec.inspect_applied.retain(|_, b| b.inbound.is_some() || b.outbound.is_some());
            Ok(String::new())
        }
        Id::ShowZoneSecurity => Ok(zones(c.d()?, args.first().map(String::as_str))),
        Id::ShowZonePairSecurity => Ok(zone_pairs(c.d()?)),
        Id::ShowInspectSessions | Id::ShowInspectZonePair => {
            let d = c.d()?.clone();
            Ok(zone_pair_report(c.sim, &d, id == Id::ShowInspectSessions))
        }
        Id::ShowInspectClassMaps => Ok(class_maps(c.d()?)),
        Id::ShowInspectPolicyMaps => Ok(policy_maps(c.d()?)),
        Id::ClearInspectSessions => {
            c.sim.zfw_clear(c.dev);
            Ok(String::new())
        }
        Id::ShowIpInspectSessions | Id::ShowIpInspectAll => {
            let d = c.d()?.clone();
            Ok(inspect_report(c.sim, &d, id == Id::ShowIpInspectAll))
        }
        Id::IpMtu | Id::NoIpMtu | Id::TcpAdjustMss | Id::NoTcpAdjustMss => {
            let names = mode_ifaces(c)?;
            let value = match id {
                Id::IpMtu => Some(
                    arg(0)
                        .parse::<u16>()
                        .ok()
                        .filter(|m| (MIN_MTU..=MAX_MTU).contains(m))
                        .ok_or(format!("% The IP MTU is {MIN_MTU}-{MAX_MTU} bytes"))?,
                ),
                Id::TcpAdjustMss => Some(
                    arg(0)
                        .parse::<u16>()
                        .ok()
                        .filter(|m| (MIN_MSS..=MAX_MSS).contains(m))
                        .ok_or(format!("% The MSS is {MIN_MSS}-{MAX_MSS} bytes"))?,
                ),
                _ => None,
            };
            let sec = &mut c.dm()?.security;
            let map = if matches!(id, Id::IpMtu | Id::NoIpMtu) { &mut sec.ip_mtu } else { &mut sec.adjust_mss };
            for n in names {
                match value {
                    Some(v) if map.len() < MAX_IFACES || map.contains_key(&n) => {
                        map.insert(n, v);
                    }
                    Some(_) => return Err("% Too many interfaces with this setting".into()),
                    None => {
                        map.remove(&n);
                    }
                }
            }
            Ok(String::new())
        }
        _ => Err("% Not a security command".into()),
    }
}

/// `match access-group N` (1-2699) or `match access-group name NAME`.
fn acl_match(text: &str) -> Result<InspectMatch, String> {
    if text.chars().all(|ch| ch.is_ascii_digit()) {
        let n: u32 = text.parse().map_err(|_| "% A numbered access list is 1-2699")?;
        if !(1..=2699).contains(&n) {
            return Err("% A numbered access list is 1-2699".into());
        }
        return Ok(InspectMatch::AccessGroup(n.to_string()));
    }
    Ok(InspectMatch::AccessGroup(name_ok(text)?))
}

/// `protocol tcp`, `access-group 101`, `access-group name X`, `class-map C` (keywords may be
/// abbreviated).
fn parse_match_words(words: &[&str]) -> Result<InspectMatch, String> {
    let kw = |w: &str, full: &str| !w.is_empty() && full.starts_with(&w.to_ascii_lowercase());
    match words {
        [k, p] if kw(k, "protocol") => Ok(InspectMatch::Protocol(protocol_name(p)?)),
        [k, n, a] if kw(k, "access-group") && kw(n, "name") => Ok(InspectMatch::AccessGroup(name_ok(a)?)),
        [k, a] if kw(k, "access-group") => acl_match(a),
        [k, n] if kw(k, "class-map") => Ok(InspectMatch::ClassMap(name_ok(n)?)),
        _ => Err("% Usage: match {protocol NAME | access-group {N | name NAME} | class-map NAME}".into()),
    }
}

// ---- configuration ----

/// The global lines: inspect classes and policies, zones, zone pairs and inspection rules.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let sec = &d.security;
    for cl in &sec.classes {
        let _ = writeln!(s, "class-map type inspect {} {}", if cl.match_all { "match-all" } else { "match-any" }, cl.name);
        for m in &cl.matches {
            let _ = writeln!(s, " match {}", describe_match(m));
        }
        let _ = writeln!(s, "!");
    }
    for p in &sec.policies {
        let _ = writeln!(s, "policy-map type inspect {}", p.name);
        for pc in &p.classes {
            if pc.class.eq_ignore_ascii_case(CLASS_DEFAULT) {
                let _ = writeln!(s, " class class-default");
            } else {
                let _ = writeln!(s, " class type inspect {}", pc.class);
            }
            if let Some(a) = pc.action {
                let _ = writeln!(s, "  {}{}", a.word(), if pc.log { " log" } else { "" });
            }
        }
        let _ = writeln!(s, "!");
    }
    for z in &sec.zones {
        let _ = writeln!(s, "zone security {}", z.name);
        if !z.description.is_empty() {
            let _ = writeln!(s, " description {}", z.description);
        }
        let _ = writeln!(s, "!");
    }
    for p in &sec.pairs {
        let _ = writeln!(s, "zone-pair security {} source {} destination {}", p.name, p.source, p.destination);
        if let Some(pol) = &p.policy {
            let _ = writeln!(s, " service-policy type inspect {pol}");
        }
        let _ = writeln!(s, "!");
    }
    for r in &sec.inspect_rules {
        for p in &r.protocols {
            let _ = writeln!(s, "ip inspect name {} {p}", r.name);
        }
    }
    if !sec.inspect_rules.is_empty() {
        let _ = writeln!(s, "!");
    }
}

/// The lines under one interface: IP MTU, MSS clamping, zone membership and inspection.
pub(crate) fn iface_lines(s: &mut String, d: &Device, name: &str) {
    let sec = &d.security;
    if let Some(m) = sec.ip_mtu.get(name) {
        let _ = writeln!(s, " ip mtu {m}");
    }
    if let Some(m) = sec.adjust_mss.get(name) {
        let _ = writeln!(s, " ip tcp adjust-mss {m}");
    }
    if let Some(z) = sec.members.get(name) {
        let _ = writeln!(s, " zone-member security {z}");
    }
    if let Some(b) = sec.inspect_applied.get(name) {
        if let Some(r) = &b.inbound {
            let _ = writeln!(s, " ip inspect {r} in");
        }
        if let Some(r) = &b.outbound {
            let _ = writeln!(s, " ip inspect {r} out");
        }
    }
}

// ---- reports ----

/// `show zone security [NAME]`.
pub(crate) fn zones(d: &Device, only: Option<&str>) -> String {
    let sec = &d.security;
    let mut s = String::new();
    let members = |z: &str| -> Vec<String> { sec.members.iter().filter(|(_, m)| *m == z).map(|(i, _)| i.clone()).collect() };
    if only.is_none_or(|o| o.eq_ignore_ascii_case(SELF_ZONE)) {
        let _ = writeln!(s, "Zone self\n  The router itself: traffic to and from its own addresses");
    }
    let mut found = only.is_some_and(|o| o.eq_ignore_ascii_case(SELF_ZONE));
    for z in sec.zones.iter().filter(|z| only.is_none_or(|o| o == z.name)) {
        found = true;
        let _ = writeln!(s, "Zone {}", z.name);
        if !z.description.is_empty() {
            let _ = writeln!(s, "  Description: {}", z.description);
        }
        let m = members(&z.name);
        let _ = writeln!(s, "  Members: {}", if m.is_empty() { "none".to_string() } else { m.join(", ") });
    }
    if let Some(o) = only.filter(|_| !found) {
        return format!("% There is no zone {o}");
    }
    s.trim_end().to_string()
}

/// `show zone-pair security`.
pub(crate) fn zone_pairs(d: &Device) -> String {
    let sec = &d.security;
    if sec.pairs.is_empty() {
        return "No zone pairs (traffic between two different zones is dropped)".into();
    }
    let mut s = String::new();
    for p in &sec.pairs {
        let _ = writeln!(s, "Zone pair {}: from {} to {}", p.name, p.source, p.destination);
        let _ = writeln!(s, "  Inspect policy: {}", p.policy.as_deref().unwrap_or("none (everything is dropped)"));
    }
    s.trim_end().to_string()
}

fn session_line(sim: &Sim, s: &Session) -> String {
    let now = sim.now_us();
    // ICMP (protocol 1) sessions are keyed by the echo id, not ports.
    let ports = s.protocol != 1;
    let at = |a: std::net::Ipv4Addr, p: u16| if ports { format!("{a}:{p}") } else { a.to_string() };
    format!(
        "{:<5} {:<22} → {:<22} up {} s, idle {} s, {}/{} packets{}{}",
        s.protocol_name(),
        if s.expected { format!("{}:any", s.src) } else { at(s.src, s.src_port) },
        at(s.dst, s.dst_port),
        now.saturating_sub(s.started_us) / 1_000_000,
        now.saturating_sub(s.last_us) / 1_000_000,
        s.packets.0,
        s.packets.1,
        match (s.app, s.expected) {
            (_, true) => ", FTP data connection expected".to_string(),
            (Some(app), _) => format!(", {} inspected", app.name().to_uppercase()),
            _ => String::new(),
        },
        if s.closing { ", closing" } else { "" }
    )
}

/// `show policy-map type inspect zone-pair [sessions]`.
pub(crate) fn zone_pair_report(sim: &mut Sim, d: &Device, sessions_only: bool) -> String {
    let sec = &d.security;
    if sec.pairs.is_empty() {
        return "No zone pairs".into();
    }
    let st = sim.zfw_state(d);
    let mut s = String::new();
    for p in &sec.pairs {
        let _ = writeln!(s, "Zone pair {} ({} → {}), policy {}", p.name, p.source, p.destination, p.policy.as_deref().unwrap_or("none"));
        if !sessions_only && let Some(pol) = p.policy.as_deref().and_then(|n| sec.policy(n)) {
            let mut listed: Vec<&InspectPolicyClass> = pol.classes.iter().collect();
            let implicit = InspectPolicyClass { class: CLASS_DEFAULT.into(), action: None, log: false };
            if !listed.iter().any(|c| c.class.eq_ignore_ascii_case(CLASS_DEFAULT)) {
                listed.push(&implicit);
            }
            for pc in listed {
                let hits = st.class_hits.get(&(p.name.clone(), pc.class.clone())).copied().unwrap_or(0);
                let how = sec.class(&pc.class).map_or(String::new(), |cl| {
                    let m: Vec<String> = cl.matches.iter().map(describe_match).collect();
                    format!(
                        " ({}: {})",
                        if cl.match_all { "match-all" } else { "match-any" },
                        if m.is_empty() { "nothing".into() } else { m.join(", ") }
                    )
                });
                let _ = writeln!(
                    s,
                    "  Class {}{how}: {}{}, {hits} packet{}",
                    pc.class,
                    pc.effective().word(),
                    if pc.log { " and log" } else { "" },
                    if hits == 1 { "" } else { "s" }
                );
            }
        }
        let mine: Vec<&Session> = st.sessions.iter().filter(|x| matches!(&x.owner, Owner::Pair { pair, .. } if *pair == p.name)).collect();
        if mine.is_empty() {
            let _ = writeln!(s, "  No sessions");
        } else {
            let _ = writeln!(s, "  Sessions:");
            for x in mine {
                let class = match &x.owner {
                    Owner::Pair { class, .. } => class.as_str(),
                    Owner::Rule { .. } => "",
                };
                let _ = writeln!(s, "    {} (class {class})", session_line(sim, x));
            }
        }
    }
    if !sessions_only && !st.drops.is_empty() {
        let _ = writeln!(s, "Dropped:");
        for (why, n) in &st.drops {
            let _ = writeln!(s, "  {n} × {why}");
        }
    }
    s.trim_end().to_string()
}

/// `show class-map type inspect`.
pub(crate) fn class_maps(d: &Device) -> String {
    let sec = &d.security;
    if sec.classes.is_empty() {
        return "No inspect classes".into();
    }
    let mut s = String::new();
    for cl in &sec.classes {
        let _ = writeln!(s, "Inspect class {} ({})", cl.name, if cl.match_all { "every line must match" } else { "any line may match" });
        if cl.matches.is_empty() {
            let _ = writeln!(s, "  (no match lines: takes nothing)");
        }
        for m in &cl.matches {
            let _ = writeln!(s, "  match {}", describe_match(m));
        }
    }
    s.trim_end().to_string()
}

/// `show policy-map type inspect`.
pub(crate) fn policy_maps(d: &Device) -> String {
    let sec = &d.security;
    if sec.policies.is_empty() {
        return "No inspect policies".into();
    }
    let mut s = String::new();
    for p in &sec.policies {
        let _ = writeln!(s, "Inspect policy {}", p.name);
        for pc in &p.classes {
            let _ = writeln!(s, "  Class {}: {}{}", pc.class, pc.effective().word(), if pc.log { " and log" } else { "" });
        }
        if !p.classes.iter().any(|c| c.class.eq_ignore_ascii_case(CLASS_DEFAULT)) {
            let _ = writeln!(s, "  Class class-default: drop (implicit)");
        }
    }
    s.trim_end().to_string()
}

/// `show ip inspect sessions|all`.
pub(crate) fn inspect_report(sim: &mut Sim, d: &Device, all: bool) -> String {
    let sec = &d.security;
    let mut s = String::new();
    if all {
        if sec.inspect_rules.is_empty() {
            let _ = writeln!(s, "No inspection rules");
        }
        for r in &sec.inspect_rules {
            let _ = writeln!(s, "Inspection rule {}: {}", r.name, r.protocols.join(", "));
        }
        for (i, b) in &sec.inspect_applied {
            if let Some(r) = &b.inbound {
                let _ = writeln!(s, "  {i}: {r} inbound");
            }
            if let Some(r) = &b.outbound {
                let _ = writeln!(s, "  {i}: {r} outbound");
            }
        }
    }
    let st = sim.zfw_state(d);
    let mine: Vec<&Session> = st.sessions.iter().filter(|x| matches!(x.owner, Owner::Rule { .. })).collect();
    if mine.is_empty() {
        let _ = writeln!(s, "No inspected sessions");
    } else {
        let _ = writeln!(s, "Inspected sessions:");
        for x in mine {
            let (rule, iface) = match &x.owner {
                Owner::Rule { rule, iface } => (rule.as_str(), iface.as_str()),
                Owner::Pair { .. } => ("", ""),
            };
            let _ = writeln!(s, "  {} (rule {rule} on {iface})", session_line(sim, x));
        }
    }
    s.trim_end().to_string()
}
