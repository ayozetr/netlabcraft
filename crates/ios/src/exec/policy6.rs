//! IPv6 prefix lists (`ipv6 prefix-list`), the IPv6 lines of a route map (`match ipv6 address
//! prefix-list`, `set ipv6 next-hop`) and `show ipv6 prefix-list`.

use std::fmt::Write as _;
use std::net::Ipv6Addr;

use netlabcraft_core::Ipv6Net;
use netlabcraft_model::Device;
use netlabcraft_model::policy::{MAX_MATCH_NAMES, MAX_NAME, MAX_PREFIX_LISTS, MAX_SEQ};
use netlabcraft_model::policy6::{PrefixEntry6, PrefixList6};

use super::policy::clause;
use super::{Ctx, R};
use crate::grammar::Id;

fn name_ok(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME || t.chars().any(char::is_control) {
        return Err(format!("% A name is 1-{MAX_NAME} characters"));
    }
    Ok(t.to_string())
}

fn sequence(text: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|n| (1..=MAX_SEQ).contains(n)).ok_or_else(|| format!("% The sequence number is 1-{MAX_SEQ}"))
}

/// What a prefix-list line says after its name.
struct Line {
    seq: Option<u32>,
    /// Absent when only `seq N` was typed (a `no` form).
    body: Option<PrefixEntry6>,
}

fn length(text: Option<&&str>, which: &str) -> Result<u8, String> {
    text.and_then(|t| t.parse::<u8>().ok()).filter(|n| *n <= 128).ok_or_else(|| format!("% `{which}` takes a prefix length, 0-128"))
}

/// `[seq N] permit|deny X:X::X/LEN [ge N] [le N]`.
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
        return Err("% Write the prefix as X:X::X/LEN, like 2001:db8::/32".into());
    }
    let net = prefix.parse::<Ipv6Net>().map_err(|e| format!("% {e}"))?.network();
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
    let e = PrefixEntry6 { seq: 0, permit, net, ge, le };
    if !e.valid() {
        return Err(format!("% Invalid range for {net}: the lengths must satisfy {} < ge <= le <= 128", net.prefix));
    }
    Ok(Line { seq, body: Some(e) })
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::Prefix6ListLine => {
            let name = name_ok(arg(0))?;
            let line = parse_line(arg(1))?;
            let Some(b) = line.body else { return Err("% Name permit or deny after the sequence number".into()) };
            let d = c.dm()?;
            let at = match d.prefix_lists6.iter().position(|l| l.name == name) {
                Some(i) => i,
                None => {
                    if d.prefix_lists6.len() >= MAX_PREFIX_LISTS {
                        return Err(format!("% At most {MAX_PREFIX_LISTS} prefix lists"));
                    }
                    d.prefix_lists6.push(PrefixList6::new(name.clone()));
                    d.prefix_lists6.len().saturating_sub(1)
                }
            };
            let result = match d.prefix_lists6.get_mut(at) {
                Some(list) => match line.seq.or_else(|| list.next_seq()) {
                    Some(seq) => list.add(PrefixEntry6 { seq, ..b }),
                    None => Err("% No sequence number is left after the last line".to_string()),
                },
                None => Err("% No such prefix list".to_string()),
            };
            d.prefix_lists6.retain(|l| !l.entries.is_empty());
            result.map(|()| String::new())
        }
        Id::NoPrefix6List => {
            let name = arg(0);
            let d = c.dm()?;
            let before = d.prefix_lists6.len();
            d.prefix_lists6.retain(|l| l.name != name);
            if d.prefix_lists6.len() == before { Err(format!("% No IPv6 prefix list named {name}")) } else { Ok(String::new()) }
        }
        Id::NoPrefix6ListLine => {
            let name = arg(0);
            let line = parse_line(arg(1))?;
            let d = c.dm()?;
            let list = d.prefix_lists6.iter_mut().find(|l| l.name == name).ok_or_else(|| format!("% No IPv6 prefix list named {name}"))?;
            let before = list.entries.len();
            match (&line.body, line.seq) {
                (Some(b), _) => list.entries.retain(|e| !e.same_rule(b)),
                (None, Some(s)) => list.entries.retain(|e| e.seq != s),
                (None, None) => return Err("% Name a sequence number or the whole line".into()),
            }
            let removed = list.entries.len() != before;
            d.prefix_lists6.retain(|l| !l.entries.is_empty());
            if removed { Ok(String::new()) } else { Err(format!("% No such line in IPv6 prefix list {name}")) }
        }
        Id::ShowIpv6PrefixList => show_prefix_lists(c.d()?, args.first().map(String::as_str)),
        Id::RmMatchAddress6 | Id::NoRmMatchAddress6 => {
            let add = id == Id::RmMatchAddress6;
            let mut words: Vec<&str> = arg(0).split_whitespace().collect();
            if words.first().is_some_and(|w| w.eq_ignore_ascii_case("prefix-list")) {
                words.remove(0);
            } else if !words.is_empty() {
                return Err("% Only IPv6 prefix lists can be matched: match ipv6 address prefix-list NAME".into());
            }
            if add && words.is_empty() {
                return Err("% Name at least one IPv6 prefix list".into());
            }
            let names: Vec<String> = words.iter().map(|w| name_ok(w)).collect::<Result<_, _>>()?;
            let cl = clause(c)?;
            if !add && names.is_empty() {
                cl.prefix_lists6.clear();
                return Ok(String::new());
            }
            for n in names {
                if add {
                    if !cl.prefix_lists6.contains(&n) {
                        if cl.prefix_lists6.len() >= MAX_MATCH_NAMES {
                            return Err(format!("% A match line holds at most {MAX_MATCH_NAMES} names"));
                        }
                        cl.prefix_lists6.push(n);
                    }
                } else {
                    cl.prefix_lists6.retain(|x| *x != n);
                }
            }
            Ok(String::new())
        }
        Id::RmSetNextHop6 => {
            let a: Ipv6Addr = arg(0).trim().parse().map_err(|_| format!("% `{}` is not an IPv6 address", arg(0).trim()))?;
            if a.is_unspecified() || a.is_multicast() || a.is_loopback() {
                return Err(format!("% {a} is not an address a next hop can have"));
            }
            clause(c)?.sets.next_hop6 = Some(a);
            Ok(String::new())
        }
        Id::NoRmSetNextHop6 => {
            clause(c)?.sets.next_hop6 = None;
            Ok(String::new())
        }
        _ => Err("% Not an IPv6 policy command".into()),
    }
}

fn show_prefix_lists(d: &Device, only: Option<&str>) -> R {
    let lists: Vec<&PrefixList6> = d.prefix_lists6.iter().filter(|l| only.is_none_or(|n| l.name == n)).collect();
    if lists.is_empty() {
        return match only {
            Some(n) => Err(format!("% No IPv6 prefix list named {n}")),
            None => Ok("(no IPv6 prefix lists: add one with `ipv6 prefix-list NAME permit X:X::X/LEN`)".into()),
        };
    }
    let mut s = String::new();
    for l in lists {
        let _ = writeln!(s, "IPv6 prefix list {}: {} line{}", l.name, l.entries.len(), if l.entries.len() == 1 { "" } else { "s" });
        for e in &l.entries {
            let _ = writeln!(s, "  seq {} {}", e.seq, e.text());
        }
        s.push_str("  (a prefix no line matches is denied)\n");
    }
    Ok(s.trim_end().to_string())
}

/// The `ipv6 prefix-list` lines of the running configuration.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    for l in &d.prefix_lists6 {
        for e in &l.entries {
            let _ = writeln!(s, "ipv6 prefix-list {} seq {} {}", l.name, e.seq, e.text());
        }
    }
    if !d.prefix_lists6.is_empty() {
        let _ = writeln!(s, "!");
    }
}
