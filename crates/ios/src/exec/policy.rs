//! Prefix lists (`ip prefix-list`) and route maps (`route-map`, with its mode): the commands, the
//! `show ip prefix-list` and `show route-map` views and their sections of the running
//! configuration.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::Device;
use netlabcraft_model::policy::{
    AsPathEntry, AsPathList, Clause, CommunityEntry, CommunityList, ExtCommunityEntry, ExtCommunityList, FIRST_CLAUSE, MAX_AS_PATH_LISTS,
    MAX_COMMUNITIES, MAX_COMMUNITY_LISTS, MAX_EXT_COMMUNITIES, MAX_EXT_COMMUNITY_LISTS, MAX_LIST_LINES, MAX_MATCH_NAMES, MAX_NAME, MAX_PREFIX_LISTS,
    MAX_PREPEND, MAX_ROUTE_MAPS, MAX_SEQ, PrefixEntry, PrefixList, RouteMap, community_text, origin_word, parse_community,
};
use netlabcraft_model::textmatch;
use netlabcraft_wire::bgp::{ExtKind, ext_kind, ext_parse, ext_text};

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
pub(super) fn clause<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut Clause, String> {
    let Mode::RouteMap(name, seq) = c.cli.mode.clone() else { return Err("% Not in route-map mode".into()) };
    c.dm()?
        .route_maps
        .iter_mut()
        .find(|m| m.name == name)
        .and_then(|m| m.clauses.iter_mut().find(|x| x.seq == seq))
        .ok_or_else(|| "% This route map clause no longer exists".to_string())
}

/// A community as typed, with a message that teaches the forms.
fn community_word(w: &str) -> Result<u32, String> {
    parse_community(w).ok_or_else(|| format!("% `{w}` is not a community: write AA:NN, a number, or no-export, no-advertise, local-AS or internet"))
}

fn permit_word(w: &str) -> Result<bool, String> {
    match w.to_ascii_lowercase().as_str() {
        "permit" => Ok(true),
        "deny" => Ok(false),
        _ => Err("% Expected permit or deny".into()),
    }
}

/// `standard NAME …`, `expanded NAME …` or `NUMBER …`: whether the list is expanded, its name and
/// the words after it.
fn community_list_head<'a>(words: &'a [&'a str]) -> Result<(bool, String, &'a [&'a str]), String> {
    let first = words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
    match first.as_str() {
        "standard" | "expanded" => {
            let name = name_ok(words.get(1).copied().unwrap_or(""))?;
            Ok((first == "expanded", name, words.get(2..).unwrap_or(&[])))
        }
        _ => {
            let n = first
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=500).contains(n))
                .ok_or("% ip community-list {standard|expanded} NAME ..., or a list number 1-500")?;
            Ok((n >= 100, n.to_string(), words.get(1..).unwrap_or(&[])))
        }
    }
}

/// `standard NAME …`, `expanded NAME …` or `NUMBER …` of an extended community list.
fn ext_list_head<'a>(words: &'a [&'a str]) -> Result<(bool, String, &'a [&'a str]), String> {
    let first = words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
    match first.as_str() {
        "standard" | "expanded" => {
            let name = name_ok(words.get(1).copied().unwrap_or(""))?;
            Ok((first == "expanded", name, words.get(2..).unwrap_or(&[])))
        }
        _ => {
            let n = first
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=500).contains(n))
                .ok_or("% ip extcommunity-list {standard|expanded} NAME ..., or a list number 1-500")?;
            Ok((n >= 100, n.to_string(), words.get(1..).unwrap_or(&[])))
        }
    }
}

/// The values after `rt` or `soo` words (a kind word switches the kind until the next one).
fn ext_words(words: &[&str]) -> Result<Vec<u64>, String> {
    let mut kind: Option<ExtKind> = None;
    let mut out: Vec<u64> = Vec::new();
    for w in words {
        match w.to_ascii_lowercase().as_str() {
            "rt" => kind = Some(ExtKind::RouteTarget),
            "soo" => kind = Some(ExtKind::RouteOrigin),
            _ => {
                let k = kind.ok_or("% Start with rt (a route target) or soo (a site of origin)")?;
                out.push(ext_parse(k, w).ok_or_else(|| format!("% `{w}` is not a route target or site of origin: write ASN:NN or A.B.C.D:NN"))?);
            }
        }
    }
    if out.is_empty() {
        return Err("% Name at least one value after rt or soo".into());
    }
    if out.len() > MAX_EXT_COMMUNITIES {
        return Err(format!("% At most {MAX_EXT_COMMUNITIES} extended communities on a line"));
    }
    Ok(out)
}

/// The words `ext_words` reads, written back: `rt 65001:100 soo 10.0.0.1:5`.
fn ext_words_text(list: &[u64]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut last: Option<ExtKind> = None;
    for c in list {
        let kind = ext_kind(*c);
        if kind.is_some() && kind != last {
            parts.push(kind.map_or("", ExtKind::word).to_string());
            last = kind;
        }
        let text = ext_text(*c);
        parts.push(text.split_once(':').filter(|_| kind.is_some()).map_or(text.clone(), |(_, v)| v.to_string()));
    }
    parts.join(" ")
}

fn ext_entry_text(e: &ExtCommunityEntry, expanded: bool) -> String {
    let word = if e.permit { "permit" } else { "deny" };
    if expanded { format!("{word} {}", e.regex) } else { format!("{word} {}", ext_words_text(&e.communities)) }
}

fn show_ext_community_lists(d: &Device, only: Option<&str>) -> R {
    let lists: Vec<&ExtCommunityList> = d.ext_community_lists.iter().filter(|l| only.is_none_or(|n| l.name == n)).collect();
    if lists.is_empty() {
        return match only {
            Some(n) => Err(format!("% No extended community list named {n}")),
            None => Ok("(no extended community lists: add one with `ip extcommunity-list standard NAME permit rt 65001:100`)".into()),
        };
    }
    let mut s = String::new();
    for l in lists {
        let _ = writeln!(
            s,
            "Extended community list {} ({}): {} line{}",
            l.name,
            if l.expanded { "expanded" } else { "standard" },
            l.entries.len(),
            if l.entries.len() == 1 { "" } else { "s" }
        );
        for e in &l.entries {
            let _ = writeln!(s, "  {}", ext_entry_text(e, l.expanded));
        }
        s.push_str("  (a path no line matches is denied)\n");
    }
    Ok(s.trim_end().to_string())
}

/// An AS-path list name: a number 1-500.
fn as_path_name(w: &str) -> Result<String, String> {
    w.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (1..=500).contains(n))
        .map(|n| n.to_string())
        .ok_or_else(|| "% An AS-path list is numbered 1-500".to_string())
}

/// The names after `match community` or `match as-path`.
fn list_names(text: &str, numbers: bool) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for w in text.split_whitespace() {
        if w.eq_ignore_ascii_case("exact-match") {
            return Err("% exact-match is not simulated: a list matches when any of its lines permits the path".into());
        }
        names.push(if numbers { as_path_name(w)? } else { name_ok(w)? });
    }
    Ok(names)
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
        Id::ShowRouteMap => show_route_maps(c.d()?, args.first().map(String::as_str), c.sim),
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
        Id::RmMatchTag => {
            let mut tags = Vec::new();
            for w in arg(0).split_whitespace() {
                tags.push(number(w, "tag", u64::from(u32::MAX))?);
            }
            let cl = clause(c)?;
            for t in tags {
                if !cl.tags.contains(&t) {
                    if cl.tags.len() >= MAX_MATCH_NAMES {
                        return Err(format!("% A match line holds at most {MAX_MATCH_NAMES} tags"));
                    }
                    cl.tags.push(t);
                }
            }
            cl.tags.sort_unstable();
            Ok(String::new())
        }
        Id::NoRmMatchTag => {
            clause(c)?.tags.clear();
            Ok(String::new())
        }
        Id::RmSetTag => {
            let v = number(arg(0), "tag", u64::from(u32::MAX))?;
            clause(c)?.sets.tag = Some(v);
            Ok(String::new())
        }
        Id::NoRmSetTag => {
            clause(c)?.sets.tag = None;
            Ok(String::new())
        }
        Id::RmSetDampening => {
            clause(c)?.sets.dampening = Some(super::bgp::damp_params(args)?);
            Ok(String::new())
        }
        Id::NoRmSetDampening => {
            clause(c)?.sets.dampening = None;
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
        Id::RmSetMetricType => {
            let t = match arg(0).to_ascii_lowercase().as_str() {
                "type-1" => 1,
                "type-2" => 2,
                _ => return Err("% type-1 or type-2".into()),
            };
            clause(c)?.sets.metric_type = Some(t);
            Ok(String::new())
        }
        Id::NoRmSetMetricType => {
            clause(c)?.sets.metric_type = None;
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
            let cl = clause(c)?;
            cl.sets.next_hop = Some(a);
            cl.sets.next_hop_track = None;
            Ok(String::new())
        }
        Id::NoRmSetNextHop => {
            let cl = clause(c)?;
            cl.sets.next_hop = None;
            cl.sets.next_hop_track = None;
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
        Id::CommunityListLine => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (expanded, name, rest) = community_list_head(&words)?;
            let permit = permit_word(rest.first().copied().unwrap_or(""))?;
            let body = rest.get(1..).unwrap_or(&[]);
            let entry = if expanded {
                let regex = body.join(" ");
                if !textmatch::valid(&regex) {
                    return Err(format!("% `{regex}` is not a usable regular expression (at most {} characters)", textmatch::MAX_PATTERN));
                }
                CommunityEntry { permit, communities: Vec::new(), regex }
            } else {
                if body.is_empty() {
                    return Err("% Name at least one community".into());
                }
                if body.len() > MAX_COMMUNITIES {
                    return Err(format!("% At most {MAX_COMMUNITIES} communities on a line"));
                }
                let communities = body.iter().map(|w| community_word(w)).collect::<Result<Vec<u32>, String>>()?;
                CommunityEntry { permit, communities, regex: String::new() }
            };
            let d = c.dm()?;
            if let Some(l) = d.community_lists.iter_mut().find(|l| l.name == name) {
                if l.expanded != expanded {
                    return Err(format!("% {name} is already a{} list", if l.expanded { "n expanded" } else { " standard" }));
                }
                if l.entries.contains(&entry) {
                    return Ok(String::new());
                }
                if l.entries.len() >= MAX_LIST_LINES {
                    return Err(format!("% A community list holds at most {MAX_LIST_LINES} lines"));
                }
                l.entries.push(entry);
            } else {
                if d.community_lists.len() >= MAX_COMMUNITY_LISTS {
                    return Err(format!("% At most {MAX_COMMUNITY_LISTS} community lists"));
                }
                let mut l = CommunityList::new(name, expanded);
                l.entries.push(entry);
                d.community_lists.push(l);
            }
            Ok(String::new())
        }
        Id::NoCommunityList => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (expanded, name, rest) = community_list_head(&words)?;
            let d = c.dm()?;
            let at = d.community_lists.iter().position(|l| l.name == name).ok_or_else(|| format!("% No community list named {name}"))?;
            if rest.is_empty() {
                d.community_lists.remove(at);
                return Ok(String::new());
            }
            let permit = permit_word(rest.first().copied().unwrap_or(""))?;
            let body: Vec<&str> = rest.get(1..).unwrap_or(&[]).to_vec();
            let l = d.community_lists.get_mut(at).ok_or("% No such community list")?;
            let before = l.entries.len();
            l.entries.retain(|e| {
                let same = if expanded {
                    e.regex == body.join(" ")
                } else {
                    let want: Vec<u32> = body.iter().filter_map(|w| parse_community(w)).collect();
                    e.communities == want
                };
                !(e.permit == permit && same)
            });
            let removed = l.entries.len() != before;
            d.community_lists.retain(|l| !l.entries.is_empty());
            if removed { Ok(String::new()) } else { Err(format!("% No such line in community list {name}")) }
        }
        Id::ExtCommunityListLine => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (expanded, name, rest) = ext_list_head(&words)?;
            let permit = permit_word(rest.first().copied().unwrap_or(""))?;
            let body = rest.get(1..).unwrap_or(&[]);
            let entry = if expanded {
                let regex = body.join(" ");
                if !textmatch::valid(&regex) {
                    return Err(format!("% `{regex}` is not a usable regular expression (at most {} characters)", textmatch::MAX_PATTERN));
                }
                ExtCommunityEntry { permit, communities: Vec::new(), regex }
            } else {
                ExtCommunityEntry { permit, communities: ext_words(body)?, regex: String::new() }
            };
            let d = c.dm()?;
            if let Some(l) = d.ext_community_lists.iter_mut().find(|l| l.name == name) {
                if l.expanded != expanded {
                    return Err(format!("% {name} is already a{} list", if l.expanded { "n expanded" } else { " standard" }));
                }
                if l.entries.contains(&entry) {
                    return Ok(String::new());
                }
                if l.entries.len() >= MAX_LIST_LINES {
                    return Err(format!("% An extended community list holds at most {MAX_LIST_LINES} lines"));
                }
                l.entries.push(entry);
            } else {
                if d.ext_community_lists.len() >= MAX_EXT_COMMUNITY_LISTS {
                    return Err(format!("% At most {MAX_EXT_COMMUNITY_LISTS} extended community lists"));
                }
                let mut l = ExtCommunityList::new(name, expanded);
                l.entries.push(entry);
                d.ext_community_lists.push(l);
            }
            Ok(String::new())
        }
        Id::NoExtCommunityList => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let (expanded, name, rest) = ext_list_head(&words)?;
            let d = c.dm()?;
            let at = d.ext_community_lists.iter().position(|l| l.name == name).ok_or_else(|| format!("% No extended community list named {name}"))?;
            if rest.is_empty() {
                d.ext_community_lists.remove(at);
                return Ok(String::new());
            }
            let permit = permit_word(rest.first().copied().unwrap_or(""))?;
            let body: Vec<&str> = rest.get(1..).unwrap_or(&[]).to_vec();
            let want = if expanded { Vec::new() } else { ext_words(&body)? };
            let l = d.ext_community_lists.get_mut(at).ok_or("% No such extended community list")?;
            let before = l.entries.len();
            l.entries.retain(|e| {
                let same = if expanded { e.regex == body.join(" ") } else { e.communities == want };
                !(e.permit == permit && same)
            });
            let removed = l.entries.len() != before;
            d.ext_community_lists.retain(|l| !l.entries.is_empty());
            if removed { Ok(String::new()) } else { Err(format!("% No such line in extended community list {name}")) }
        }
        Id::ShowIpExtCommunityList => show_ext_community_lists(c.d()?, args.first().map(String::as_str)),
        Id::RmMatchExtCommunity | Id::NoRmMatchExtCommunity => {
            let add = id == Id::RmMatchExtCommunity;
            let names = list_names(arg(0), false)?;
            if add && names.is_empty() {
                return Err("% Name at least one list".into());
            }
            let target = &mut clause(c)?.ext_community_lists;
            if !add && names.is_empty() {
                target.clear();
                return Ok(String::new());
            }
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
        Id::RmSetExtCommunity => {
            let mut words: Vec<&str> = arg(0).split_whitespace().collect();
            let additive = words.last().is_some_and(|w| w.eq_ignore_ascii_case("additive"));
            if additive {
                words.pop();
            }
            let mut list = ext_words(&words)?;
            list.sort_unstable();
            list.dedup();
            let cl = clause(c)?;
            cl.sets.ext_community = Some(list);
            cl.sets.ext_community_additive = additive;
            Ok(String::new())
        }
        Id::NoRmSetExtCommunity => {
            let cl = clause(c)?;
            cl.sets.ext_community = None;
            cl.sets.ext_community_additive = false;
            Ok(String::new())
        }
        Id::AsPathListLine => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let name = as_path_name(words.first().copied().unwrap_or(""))?;
            let permit = permit_word(words.get(1).copied().unwrap_or(""))?;
            let regex = words.get(2..).unwrap_or(&[]).join(" ");
            if !textmatch::valid(&regex) {
                return Err(format!("% `{regex}` is not a usable regular expression (at most {} characters)", textmatch::MAX_PATTERN));
            }
            let entry = AsPathEntry { permit, regex };
            let d = c.dm()?;
            if let Some(l) = d.as_path_lists.iter_mut().find(|l| l.name == name) {
                if l.entries.contains(&entry) {
                    return Ok(String::new());
                }
                if l.entries.len() >= MAX_LIST_LINES {
                    return Err(format!("% An AS-path list holds at most {MAX_LIST_LINES} lines"));
                }
                l.entries.push(entry);
            } else {
                if d.as_path_lists.len() >= MAX_AS_PATH_LISTS {
                    return Err(format!("% At most {MAX_AS_PATH_LISTS} AS-path lists"));
                }
                let mut l = AsPathList::new(name);
                l.entries.push(entry);
                d.as_path_lists.push(l);
            }
            Ok(String::new())
        }
        Id::NoAsPathList => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let name = as_path_name(words.first().copied().unwrap_or(""))?;
            let d = c.dm()?;
            let at = d.as_path_lists.iter().position(|l| l.name == name).ok_or_else(|| format!("% No AS-path list numbered {name}"))?;
            if words.len() == 1 {
                d.as_path_lists.remove(at);
                return Ok(String::new());
            }
            let permit = permit_word(words.get(1).copied().unwrap_or(""))?;
            let regex = words.get(2..).unwrap_or(&[]).join(" ");
            let l = d.as_path_lists.get_mut(at).ok_or("% No such AS-path list")?;
            let before = l.entries.len();
            l.entries.retain(|e| !(e.permit == permit && e.regex == regex));
            let removed = l.entries.len() != before;
            d.as_path_lists.retain(|l| !l.entries.is_empty());
            if removed { Ok(String::new()) } else { Err(format!("% No such line in AS-path list {name}")) }
        }
        Id::BgpCommunityNewFormat | Id::NoBgpCommunityNewFormat => {
            c.dm()?.bgp_community_new_format = id == Id::BgpCommunityNewFormat;
            Ok(String::new())
        }
        Id::ShowIpCommunityList => show_community_lists(c.d()?, args.first().map(String::as_str)),
        Id::ShowIpAsPathList => show_as_path_lists(c.d()?, args.first().map(String::as_str)),
        Id::RmMatchCommunity | Id::NoRmMatchCommunity | Id::RmMatchAsPath | Id::NoRmMatchAsPath => {
            let add = matches!(id, Id::RmMatchCommunity | Id::RmMatchAsPath);
            let communities = matches!(id, Id::RmMatchCommunity | Id::NoRmMatchCommunity);
            let names = list_names(arg(0), !communities)?;
            if add && names.is_empty() {
                return Err("% Name at least one list".into());
            }
            let cl = clause(c)?;
            let target = if communities { &mut cl.community_lists } else { &mut cl.as_path_lists };
            if !add && names.is_empty() {
                target.clear();
                return Ok(String::new());
            }
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
        Id::RmSetCommunity => {
            let mut words: Vec<&str> = arg(0).split_whitespace().collect();
            let additive = words.last().is_some_and(|w| w.eq_ignore_ascii_case("additive"));
            if additive {
                words.pop();
            }
            let none = words.len() == 1 && words.first().is_some_and(|w| w.eq_ignore_ascii_case("none"));
            if none && additive {
                return Err("% `none` cannot be additive".into());
            }
            let mut list: Vec<u32> = if none { Vec::new() } else { words.iter().map(|w| community_word(w)).collect::<Result<_, _>>()? };
            list.sort_unstable();
            list.dedup();
            if list.is_empty() && !none {
                return Err("% Name at least one community, or none".into());
            }
            if list.len() > MAX_COMMUNITIES {
                return Err(format!("% At most {MAX_COMMUNITIES} communities"));
            }
            let cl = clause(c)?;
            cl.sets.community = Some(list);
            cl.sets.community_additive = additive;
            Ok(String::new())
        }
        Id::NoRmSetCommunity => {
            let cl = clause(c)?;
            cl.sets.community = None;
            cl.sets.community_additive = false;
            Ok(String::new())
        }
        _ => Err("% Not a prefix-list or route-map command".into()),
    }
}

fn show_community_lists(d: &Device, only: Option<&str>) -> R {
    let lists: Vec<&CommunityList> = d.community_lists.iter().filter(|l| only.is_none_or(|n| l.name == n)).collect();
    if lists.is_empty() {
        return match only {
            Some(n) => Err(format!("% No community list named {n}")),
            None => Ok("(no community lists: add one with `ip community-list standard NAME permit AA:NN`)".into()),
        };
    }
    let mut s = String::new();
    for l in lists {
        let _ = writeln!(
            s,
            "Community list {} ({}): {} line{}",
            l.name,
            if l.expanded { "expanded" } else { "standard" },
            l.entries.len(),
            if l.entries.len() == 1 { "" } else { "s" }
        );
        for e in &l.entries {
            let _ = writeln!(s, "  {}", community_entry_text(e, l.expanded, d.bgp_community_new_format));
        }
        s.push_str("  (a path no line matches is denied)\n");
    }
    Ok(s.trim_end().to_string())
}

fn community_entry_text(e: &CommunityEntry, expanded: bool, new_format: bool) -> String {
    let word = if e.permit { "permit" } else { "deny" };
    if expanded {
        format!("{word} {}", e.regex)
    } else {
        format!("{word} {}", e.communities.iter().map(|c| community_text(*c, new_format)).collect::<Vec<_>>().join(" "))
    }
}

fn show_as_path_lists(d: &Device, only: Option<&str>) -> R {
    let lists: Vec<&AsPathList> = d.as_path_lists.iter().filter(|l| only.is_none_or(|n| l.name == n)).collect();
    if lists.is_empty() {
        return match only {
            Some(n) => Err(format!("% No AS-path list numbered {n}")),
            None => Ok("(no AS-path lists: add one with `ip as-path access-list 1 permit ^65001_`)".into()),
        };
    }
    let mut s = String::new();
    for l in lists {
        let _ = writeln!(s, "AS-path list {}: {} line{}", l.name, l.entries.len(), if l.entries.len() == 1 { "" } else { "s" });
        for e in &l.entries {
            let _ = writeln!(s, "  {} {}", if e.permit { "permit" } else { "deny" }, e.regex);
        }
        s.push_str("  (a path no line matches is denied)\n");
    }
    Ok(s.trim_end().to_string())
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

fn show_route_maps(d: &Device, only: Option<&str>, sim: &netlabcraft_sim::Sim) -> R {
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
            if !c.community_lists.is_empty() {
                let _ = writeln!(s, "    match community list: {}", c.community_lists.join(", "));
            }
            if !c.ext_community_lists.is_empty() {
                let _ = writeln!(s, "    match extended community list: {}", c.ext_community_lists.join(", "));
            }
            if !c.as_path_lists.is_empty() {
                let _ = writeln!(s, "    match AS-path list: {}", c.as_path_lists.join(", "));
            }
            if let Some(v) = c.metric {
                let _ = writeln!(s, "    match metric: {v}");
            }
            if !c.tags.is_empty() {
                let _ = writeln!(s, "    match tag: {}", c.tags.iter().map(u32::to_string).collect::<Vec<_>>().join(", "));
            }
            if let Some((lo, hi)) = c.length {
                let _ = writeln!(s, "    match length: {lo}-{hi} bytes");
            }
            for line in set_lines(c, d.bgp_community_new_format) {
                let _ = writeln!(s, "    set {line}");
            }
            let (packets, bytes) = sim.pbr_hits(d.id, &m.name, c.seq);
            if packets > 0 {
                let _ = writeln!(s, "    policy routing took {packets} packets, {bytes} bytes");
            }
            if c.prefix_lists.is_empty()
                && c.acls.is_empty()
                && c.length.is_none()
                && c.metric.is_none()
                && c.community_lists.is_empty()
                && c.ext_community_lists.is_empty()
                && c.as_path_lists.is_empty()
                && c.tags.is_empty()
            {
                s.push_str("    (no match lines: every route matches)\n");
            }
        }
        s.push_str("  (a route no clause matches is dropped)\n");
    }
    Ok(s.trim_end().to_string())
}

/// The `set` lines of a clause, as the configuration writes them (after `set `).
fn set_lines(c: &Clause, new_format: bool) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(x) = c.sets.local_pref {
        v.push(format!("local-preference {x}"));
    }
    if let Some(x) = c.sets.metric {
        v.push(format!("metric {x}"));
    }
    if let Some(x) = c.sets.metric_type {
        v.push(format!("metric-type type-{x}"));
    }
    if let Some(x) = c.sets.tag {
        v.push(format!("tag {x}"));
    }
    if let Some(x) = c.sets.dampening {
        v.push(format!("dampening {} {} {} {}", x.half_life, x.reuse, x.suppress, x.max_suppress));
    }
    if let Some(x) = c.sets.weight {
        v.push(format!("weight {x}"));
    }
    if !c.sets.prepend.is_empty() {
        v.push(format!("as-path prepend {}", c.sets.prepend.iter().map(u32::to_string).collect::<Vec<_>>().join(" ")));
    }
    if let Some(x) = c.sets.next_hop {
        match c.sets.next_hop_track {
            Some(n) => v.push(format!("ip next-hop verify-availability {x} 10 track {n}")),
            None => v.push(format!("ip next-hop {x}")),
        }
    }
    if let Some(x) = c.sets.default_next_hop {
        v.push(format!("ip default next-hop {x}"));
    }
    if let Some(x) = &c.sets.interface {
        v.push(format!("interface {x}"));
    }
    if let Some(x) = c.sets.precedence {
        v.push(format!("ip precedence {x}"));
    }
    if let Some(x) = c.sets.dscp {
        v.push(format!("ip dscp {x}"));
    }
    if let Some(x) = c.sets.origin {
        v.push(format!("origin {}", origin_word(x)));
    }
    if let Some(list) = &c.sets.ext_community {
        v.push(format!("extcommunity {}{}", ext_words_text(list), if c.sets.ext_community_additive { " additive" } else { "" }));
    }
    if let Some(list) = &c.sets.community {
        if list.is_empty() {
            v.push("community none".to_string());
        } else {
            let words = list.iter().map(|x| community_text(*x, new_format)).collect::<Vec<_>>().join(" ");
            v.push(format!("community {words}{}", if c.sets.community_additive { " additive" } else { "" }));
        }
    }
    v
}

/// The prefix lists and route maps of `show running-config`.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    if d.bgp_community_new_format {
        let _ = writeln!(s, "ip bgp-community new-format");
    }
    for l in &d.community_lists {
        for e in &l.entries {
            let _ = writeln!(
                s,
                "ip community-list {} {} {}",
                if l.expanded { "expanded" } else { "standard" },
                l.name,
                community_entry_text(e, l.expanded, d.bgp_community_new_format)
            );
        }
    }
    for l in &d.ext_community_lists {
        for e in &l.entries {
            let _ =
                writeln!(s, "ip extcommunity-list {} {} {}", if l.expanded { "expanded" } else { "standard" }, l.name, ext_entry_text(e, l.expanded));
        }
    }
    for l in &d.as_path_lists {
        for e in &l.entries {
            let _ = writeln!(s, "ip as-path access-list {} {} {}", l.name, if e.permit { "permit" } else { "deny" }, e.regex);
        }
    }
    if d.bgp_community_new_format || !d.community_lists.is_empty() || !d.ext_community_lists.is_empty() || !d.as_path_lists.is_empty() {
        let _ = writeln!(s, "!");
    }
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
            if !c.community_lists.is_empty() {
                let _ = writeln!(s, " match community {}", c.community_lists.join(" "));
            }
            if !c.ext_community_lists.is_empty() {
                let _ = writeln!(s, " match extcommunity {}", c.ext_community_lists.join(" "));
            }
            if !c.as_path_lists.is_empty() {
                let _ = writeln!(s, " match as-path {}", c.as_path_lists.join(" "));
            }
            if let Some(v) = c.metric {
                let _ = writeln!(s, " match metric {v}");
            }
            if !c.tags.is_empty() {
                let _ = writeln!(s, " match tag {}", c.tags.iter().map(u32::to_string).collect::<Vec<_>>().join(" "));
            }
            if let Some((lo, hi)) = c.length {
                let _ = writeln!(s, " match length {lo} {hi}");
            }
            for line in set_lines(c, d.bgp_community_new_format) {
                let _ = writeln!(s, " set {line}");
            }
            let _ = writeln!(s, "!");
        }
    }
}
