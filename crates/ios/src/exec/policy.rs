//! Prefix lists (`ip prefix-list`) and route maps (`route-map`, with its mode): the commands, the
//! `show ip prefix-list` and `show route-map` views and their sections of the running
//! configuration.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::Device;
use netlabcraft_model::policy::{
    Clause, FIRST_CLAUSE, MAX_MATCH_NAMES, MAX_NAME, MAX_PREFIX_LISTS, MAX_PREPEND, MAX_ROUTE_MAPS, MAX_SEQ, PrefixEntry, PrefixList, RouteMap,
    origin_word,
};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// A list, map or access-list name: one word, bounded, no control characters.
fn name_ok(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME || t.chars().any(char::is_control) {
        return Err(format!("% A name is 1-{MAX_NAME} characters"));
    }
    Ok(t.to_string())
}

fn number(text: &str, what: &str, max: u64) -> Result<u32, String> {
    text.trim().parse::<u64>().ok().filter(|n| *n <= max).and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("% The {what} is 0-{max}"))
}

fn sequence(text: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|n| (1..=MAX_SEQ).contains(n)).ok_or_else(|| format!("% The sequence number is 1-{MAX_SEQ}"))
}

/// What a prefix-list line says after its name.
struct Line {
    seq: Option<u32>,
    /// Absent when only `seq N` was typed (a `no` form).
    body: Option<Body>,
}

struct Body {
    permit: bool,
    net: Ipv4Net,
    ge: Option<u8>,
    le: Option<u8>,
}

fn length(text: Option<&&str>, which: &str) -> Result<u8, String> {
    text.and_then(|t| t.parse::<u8>().ok()).filter(|n| *n <= 32).ok_or_else(|| format!("% `{which}` takes a prefix length, 0-32"))
}

/// `[seq N] permit|deny A.B.C.D/LEN [ge N] [le N]`.
fn parse_line(rest: &str) -> Result<Line, String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let mut at = 0usize;
    let mut seq = None;
    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("seq")) {
        seq = Some(sequence(words.get(1).copied().unwrap_or(""))?);
        at = 2;
    }
    let Some(action) = words.get(at) else {
        return Ok(Line { seq, body: None });
    };
    let permit = match action.to_ascii_lowercase().as_str() {
        "permit" => true,
        "deny" => false,
        _ => return Err("% Expected permit or deny".into()),
    };
    let prefix = words.get(at + 1).copied().unwrap_or("");
    if !prefix.contains('/') {
        return Err("% Write the prefix as A.B.C.D/LEN, like 10.0.0.0/8".into());
    }
    let net = prefix.parse::<Ipv4Net>().map_err(|e| format!("% {e}"))?.network();
    let (mut ge, mut le) = (None, None);
    let mut i = at + 2;
    while let Some(w) = words.get(i) {
        match w.to_ascii_lowercase().as_str() {
            "ge" if ge.is_none() => ge = Some(length(words.get(i + 1), "ge")?),
            "le" if le.is_none() => le = Some(length(words.get(i + 1), "le")?),
            _ => return Err(format!("% Unexpected `{w}`: only ge and le may follow the prefix, once each")),
        }
        i += 2;
    }
    let e = PrefixEntry { seq: 0, permit, net, ge, le };
    if !e.valid() {
        return Err(format!("% Invalid range for {net}: the lengths must satisfy {} < ge <= le <= 32", net.prefix));
    }
    Ok(Line { seq, body: Some(Body { permit, net, ge, le }) })
}

/// The `permit|deny [seq]` part of a route-map command: the action when typed, the sequence
/// number when typed.
fn clause_words(rest: &str) -> Result<(Option<bool>, Option<u32>), String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let permit = match words.first().map(|w| w.to_ascii_lowercase()).as_deref() {
        None => None,
        Some("permit") => Some(true),
        Some("deny") => Some(false),
        Some(_) => return Err("% Expected permit or deny, then an optional sequence number".into()),
    };
    if words.len() > 2 {
        return Err("% route-map NAME {permit|deny} [<sequence number>]".into());
    }
    let seq = match words.get(1) {
        Some(w) => Some(sequence(w)?),
        None => None,
    };
    Ok((permit, seq))
}

/// The clause the console is inside.
fn clause<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut Clause, String> {
    let Mode::RouteMap(name, seq) = c.cli.mode.clone() else { return Err("% Not in route-map mode".into()) };
    c.dm()?
        .route_maps
        .iter_mut()
        .find(|m| m.name == name)
        .and_then(|m| m.clauses.iter_mut().find(|x| x.seq == seq))
        .ok_or_else(|| "% This route map clause no longer exists".to_string())
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::PrefixListLine => {
            let name = name_ok(arg(0))?;
            let line = parse_line(arg(1))?;
            let Some(b) = line.body else { return Err("% Name permit or deny after the sequence number".into()) };
            let d = c.dm()?;
            let at = match d.prefix_lists.iter().position(|l| l.name == name) {
                Some(i) => i,
                None => {
                    if d.prefix_lists.len() >= MAX_PREFIX_LISTS {
                        return Err(format!("% At most {MAX_PREFIX_LISTS} prefix lists"));
                    }
                    d.prefix_lists.push(PrefixList::new(name.clone()));
                    d.prefix_lists.len().saturating_sub(1)
                }
            };
            let result = match d.prefix_lists.get_mut(at) {
                Some(list) => match line.seq.or_else(|| list.next_seq()) {
                    Some(seq) => list.add(PrefixEntry { seq, permit: b.permit, net: b.net, ge: b.ge, le: b.le }),
                    None => Err("% No sequence number is left after the last line".to_string()),
                },
                None => Err("% No such prefix list".to_string()),
            };
            d.prefix_lists.retain(|l| !l.entries.is_empty());
            result.map(|()| String::new())
        }
        Id::NoPrefixList => {
            let name = arg(0);
            let d = c.dm()?;
            let before = d.prefix_lists.len();
            d.prefix_lists.retain(|l| l.name != name);
            if d.prefix_lists.len() == before { Err(format!("% No prefix list named {name}")) } else { Ok(String::new()) }
        }
        Id::NoPrefixListLine => {
            let name = arg(0);
            let line = parse_line(arg(1))?;
            let d = c.dm()?;
            let list = d.prefix_lists.iter_mut().find(|l| l.name == name).ok_or_else(|| format!("% No prefix list named {name}"))?;
            let before = list.entries.len();
            match (&line.body, line.seq) {
                (Some(b), _) => list.entries.retain(|e| !(e.permit == b.permit && e.net == b.net && e.ge == b.ge && e.le == b.le)),
                (None, Some(s)) => list.entries.retain(|e| e.seq != s),
                (None, None) => return Err("% Name a sequence number or the whole line".into()),
            }
            let removed = list.entries.len() != before;
            d.prefix_lists.retain(|l| !l.entries.is_empty());
            if removed { Ok(String::new()) } else { Err(format!("% No such line in prefix list {name}")) }
        }
        Id::ShowIpPrefixList => show_prefix_lists(c.d()?, args.first().map(String::as_str)),
        Id::ShowRouteMap => show_route_maps(c.d()?, args.first().map(String::as_str)),
        Id::RouteMapEnter => {
            let name = name_ok(arg(0))?;
            let (permit, seq) = clause_words(arg(1))?;
            let d = c.dm()?;
            let at = match d.route_maps.iter().position(|m| m.name == name) {
                Some(i) => i,
                None => {
                    if d.route_maps.len() >= MAX_ROUTE_MAPS {
                        return Err(format!("% At most {MAX_ROUTE_MAPS} route maps"));
                    }
                    d.route_maps.push(RouteMap::new(name.clone()));
                    d.route_maps.len().saturating_sub(1)
                }
            };
            let map = d.route_maps.get_mut(at).ok_or("% No such route map")?;
            let seq = match (seq, map.clauses.as_slice()) {
                (Some(s), _) => s,
                (None, []) => FIRST_CLAUSE,
                (None, [only]) => only.seq,
                (None, _) => return Err("% This route map has several clauses: name the sequence number".into()),
            };
            let cl = map
                .clause_mut(seq, permit.unwrap_or(true))
                .ok_or_else(|| format!("% A route map holds at most {} clauses", netlabcraft_model::policy::MAX_CLAUSES))?;
            if let Some(p) = permit {
                cl.permit = p;
            }
            c.cli.mode = Mode::RouteMap(name, seq);
            Ok(String::new())
        }
        Id::NoRouteMap => {
            let name = arg(0);
            let d = c.dm()?;
            if arg(1).is_empty() {
                let before = d.route_maps.len();
                d.route_maps.retain(|m| m.name != name);
                return if d.route_maps.len() == before { Err(format!("% No route map named {name}")) } else { Ok(String::new()) };
            }
            let (_, seq) = clause_words(arg(1))?;
            let map = d.route_maps.iter_mut().find(|m| m.name == name).ok_or_else(|| format!("% No route map named {name}"))?;
            let seq = seq.or_else(|| if map.clauses.len() == 1 { map.clauses.first().map(|x| x.seq) } else { None }).unwrap_or(FIRST_CLAUSE);
            let before = map.clauses.len();
            map.clauses.retain(|x| x.seq != seq);
            let removed = map.clauses.len() != before;
            d.route_maps.retain(|m| !m.clauses.is_empty());
            if removed { Ok(String::new()) } else { Err(format!("% Route map {name} has no clause {seq}")) }
        }
        Id::RmMatchAddress | Id::NoRmMatchAddress => {
            let add = id == Id::RmMatchAddress;
            let mut words: Vec<&str> = arg(0).split_whitespace().collect();
            let lists = words.first().is_some_and(|w| w.eq_ignore_ascii_case("prefix-list"));
            if lists {
                words.remove(0);
            }
            if add && words.is_empty() {
                return Err("% Name at least one prefix list or access list".into());
            }
            let names: Vec<String> = words.iter().map(|w| name_ok(w)).collect::<Result<_, _>>()?;
            let cl = clause(c)?;
            if !add && names.is_empty() {
                cl.prefix_lists.clear();
                cl.acls.clear();
                return Ok(String::new());
            }
            let target = if lists { &mut cl.prefix_lists } else { &mut cl.acls };
            for n in names {
                if add {
                    if !target.contains(&n) {
                        if target.len() >= MAX_MATCH_NAMES {
                            return Err(format!("% A match line holds at most {MAX_MATCH_NAMES} names"));
                        }
                        target.push(n);
                    }
                } else {
                    target.retain(|x| *x != n);
                }
            }
            Ok(String::new())
        }
        Id::RmMatchMetric => {
            let v = number(arg(0), "metric", u64::from(u32::MAX))?;
            clause(c)?.metric = Some(v);
            Ok(String::new())
        }
        Id::NoRmMatchMetric => {
            clause(c)?.metric = None;
            Ok(String::new())
        }
        Id::RmSetLocalPref => {
            let v = number(arg(0), "local preference", u64::from(u32::MAX))?;
            clause(c)?.sets.local_pref = Some(v);
            Ok(String::new())
        }
        Id::NoRmSetLocalPref => {
            clause(c)?.sets.local_pref = None;
            Ok(String::new())
        }
        Id::RmSetMetric => {
            let v = number(arg(0), "metric", u64::from(u32::MAX))?;
            clause(c)?.sets.metric = Some(v);
            Ok(String::new())
        }
        Id::NoRmSetMetric => {
            clause(c)?.sets.metric = None;
            Ok(String::new())
        }
        Id::RmSetWeight => {
            let v = number(arg(0), "weight", 65_535)?;
            clause(c)?.sets.weight = Some(v);
            Ok(String::new())
        }
        Id::NoRmSetWeight => {
            clause(c)?.sets.weight = None;
            Ok(String::new())
        }
        Id::RmSetPrepend => {
            let mut list = Vec::new();
            for w in arg(0).split_whitespace() {
                if list.len() >= MAX_PREPEND {
                    return Err(format!("% At most {MAX_PREPEND} AS numbers"));
                }
                list.push(w.parse::<u32>().ok().filter(|n| *n >= 1).ok_or("% An AS number is 1-4294967295")?);
            }
            if list.is_empty() {
                return Err("% Name at least one AS number".into());
            }
            clause(c)?.sets.prepend = list;
            Ok(String::new())
        }
        Id::NoRmSetPrepend => {
            clause(c)?.sets.prepend.clear();
            Ok(String::new())
        }
        Id::RmSetNextHop => {
            let a: Ipv4Addr = parse_ip(arg(0).trim()).map_err(|e| format!("% {e}"))?;
            if a.is_unspecified() || a.is_multicast() || a == Ipv4Addr::BROADCAST {
                return Err(format!("% {a} is not an address a next hop can have"));
            }
            clause(c)?.sets.next_hop = Some(a);
            Ok(String::new())
        }
        Id::NoRmSetNextHop => {
            clause(c)?.sets.next_hop = None;
            Ok(String::new())
        }
        Id::RmSetOrigin => {
            let o = match arg(0).to_ascii_lowercase().as_str() {
                "igp" => 0,
                "egp" => 1,
                "incomplete" => 2,
                _ => return Err("% The origin is igp, egp or incomplete".into()),
            };
            clause(c)?.sets.origin = Some(o);
            Ok(String::new())
        }
        Id::NoRmSetOrigin => {
            clause(c)?.sets.origin = None;
            Ok(String::new())
        }
        _ => Err("% Not a prefix-list or route-map command".into()),
    }
}

fn show_prefix_lists(d: &Device, only: Option<&str>) -> R {
    let lists: Vec<&PrefixList> = d.prefix_lists.iter().filter(|l| only.is_none_or(|n| l.name == n)).collect();
    if lists.is_empty() {
        return match only {
            Some(n) => Err(format!("% No prefix list named {n}")),
            None => Ok("(no prefix lists: add one with `ip prefix-list NAME permit A.B.C.D/LEN`)".into()),
        };
    }
    let mut s = String::new();
    for l in lists {
        let _ = writeln!(s, "Prefix list {}: {} line{}", l.name, l.entries.len(), if l.entries.len() == 1 { "" } else { "s" });
        for e in &l.entries {
            let _ = writeln!(s, "  seq {} {}", e.seq, e.text());
        }
        s.push_str("  (a prefix no line matches is denied)\n");
    }
    Ok(s.trim_end().to_string())
}

fn show_route_maps(d: &Device, only: Option<&str>) -> R {
    let maps: Vec<&RouteMap> = d.route_maps.iter().filter(|m| only.is_none_or(|n| m.name == n)).collect();
    if maps.is_empty() {
        return match only {
            Some(n) => Err(format!("% No route map named {n}")),
            None => Ok("(no route maps: add one with `route-map NAME permit 10`)".into()),
        };
    }
    let mut s = String::new();
    for m in maps {
        let _ = writeln!(s, "Route map {}: {} clause{}", m.name, m.clauses.len(), if m.clauses.len() == 1 { "" } else { "s" });
        for c in &m.clauses {
            let _ = writeln!(s, "  {}, sequence {}", if c.permit { "permit" } else { "deny" }, c.seq);
            if !c.prefix_lists.is_empty() {
                let _ = writeln!(s, "    match prefix list: {}", c.prefix_lists.join(", "));
            }
            if !c.acls.is_empty() {
                let _ = writeln!(s, "    match access list: {}", c.acls.join(", "));
            }
            if let Some(v) = c.metric {
                let _ = writeln!(s, "    match metric: {v}");
            }
            for line in set_lines(c) {
                let _ = writeln!(s, "    set {line}");
            }
            if c.prefix_lists.is_empty() && c.acls.is_empty() && c.metric.is_none() {
                s.push_str("    (no match lines: every route matches)\n");
            }
        }
        s.push_str("  (a route no clause matches is dropped)\n");
    }
    Ok(s.trim_end().to_string())
}

/// The `set` lines of a clause, as the configuration writes them (after `set `).
fn set_lines(c: &Clause) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(x) = c.sets.local_pref {
        v.push(format!("local-preference {x}"));
    }
    if let Some(x) = c.sets.metric {
        v.push(format!("metric {x}"));
    }
    if let Some(x) = c.sets.weight {
        v.push(format!("weight {x}"));
    }
    if !c.sets.prepend.is_empty() {
        v.push(format!("as-path prepend {}", c.sets.prepend.iter().map(u32::to_string).collect::<Vec<_>>().join(" ")));
    }
    if let Some(x) = c.sets.next_hop {
        v.push(format!("ip next-hop {x}"));
    }
    if let Some(x) = c.sets.origin {
        v.push(format!("origin {}", origin_word(x)));
    }
    v
}

/// The prefix lists and route maps of `show running-config`.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    for l in &d.prefix_lists {
        for e in &l.entries {
            let _ = writeln!(s, "ip prefix-list {} seq {} {}", l.name, e.seq, e.text());
        }
    }
    if !d.prefix_lists.is_empty() {
        let _ = writeln!(s, "!");
    }
    for m in &d.route_maps {
        for c in &m.clauses {
            let _ = writeln!(s, "route-map {} {} {}", m.name, if c.permit { "permit" } else { "deny" }, c.seq);
            if !c.prefix_lists.is_empty() {
                let _ = writeln!(s, " match ip address prefix-list {}", c.prefix_lists.join(" "));
            }
            if !c.acls.is_empty() {
                let _ = writeln!(s, " match ip address {}", c.acls.join(" "));
            }
            if let Some(v) = c.metric {
                let _ = writeln!(s, " match metric {v}");
            }
            for line in set_lines(c) {
                let _ = writeln!(s, " set {line}");
            }
            let _ = writeln!(s, "!");
        }
    }
}
