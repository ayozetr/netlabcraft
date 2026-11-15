//! Port mirroring (SPAN): `monitor session N source interface|vlan …`, `monitor session N
//! destination interface …`, their `no` forms, `show monitor` and the running configuration.
//!
//! A session names what to watch and one port that receives the copies (`netlabcraft_model::span`).
//! The checks keep a switch's sessions sane: a port is the source of one session or the
//! destination of one (never both), a session watches ports or VLANs (not both), and a switch
//! holds [`MAX_SESSIONS`] sessions at once.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use netlabcraft_core::{VlanId, VlanSet};
use netlabcraft_model::l2sec::vlan_list;
use netlabcraft_model::span::{MAX_SESSION_ID, MAX_SESSIONS, MAX_SOURCE_PORTS, SpanDir, SpanSession};
use netlabcraft_model::{Device, DeviceKind, End, Topology, ifname};
use netlabcraft_sim::port_up;

use super::{Ctx, R};
use crate::grammar::Id;

fn session_id(text: &str) -> Result<u8, String> {
    text.trim().parse::<u8>().ok().filter(|n| (1..=MAX_SESSION_ID).contains(n)).ok_or_else(|| format!("% The session number is 1-{MAX_SESSION_ID}"))
}

/// `fa0/1 - 3, fa0/5 rx` into the list and the direction (`both` when none is typed).
fn split_direction(text: &str) -> Result<(String, SpanDir), String> {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    let dir = match words.last().and_then(|w| SpanDir::parse(*w)) {
        Some(d) => {
            words.pop();
            d
        }
        None => SpanDir::Both,
    };
    if words.is_empty() { Err("% Name what to watch before the direction".into()) } else { Ok((words.join(" "), dir)) }
}

/// The VLANs a list names, none of them reserved.
fn vlan_sources(text: &str) -> Result<Vec<VlanId>, String> {
    if !text.chars().all(|c| c.is_ascii_digit() || matches!(c, ',' | '-' | ' ')) {
        return Err("% Give VLAN numbers such as 10 or 10,20-30".into());
    }
    let set = VlanSet::parse_list(text).map_err(|e| format!("% {e}"))?;
    let ids: Vec<VlanId> = set.iter().collect();
    if ids.is_empty() {
        return Err("% No VLANs given".into());
    }
    if let Some(v) = ids.iter().find(|v| (1002..=1005).contains(&v.get())) {
        return Err(format!("% VLAN {v} is reserved and cannot be a monitor source"));
    }
    Ok(ids)
}

fn short(name: &str) -> String {
    ifname::short(name)
}

/// The full names of the switch ports a list selects (no subinterfaces).
fn port_names(c: &Ctx, text: &str) -> Result<Vec<String>, String> {
    let refs = c.iface_range(text)?;
    let d = c.d()?;
    let mut names = Vec::new();
    for r in refs {
        if r.sub.is_some() {
            return Err("% A monitor session watches whole ports, not subinterfaces".into());
        }
        let p = d.ports.get(r.port).ok_or("% No such port")?;
        names.push(p.name.clone());
    }
    Ok(names)
}

/// Makes sure a new session fits: it exists already, or the switch has room for another.
fn room_for(d: &Device, id: u8) -> Result<(), String> {
    if d.l2.span.iter().any(|s| s.id == id) || d.l2.span.len() < MAX_SESSIONS {
        Ok(())
    } else {
        Err(format!("% This switch holds {MAX_SESSIONS} monitor sessions at once: delete one with `no monitor session N` first"))
    }
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    use Id::*;
    match id {
        MonitorSource => {
            let n = session_id(arg(0))?;
            let (list, dir) = split_direction(arg(1))?;
            let names = port_names(c, &list)?;
            let d = c.d()?;
            room_for(d, n)?;
            if d.l2.span.iter().any(|s| s.id == n && !s.vlans.is_empty()) {
                return Err(format!("% Session {n} watches VLANs: a session takes ports or VLANs as sources, not both"));
            }
            for name in &names {
                if let Some(other) = d.l2.span_destination_of(name) {
                    return Err(format!("% {} is the destination of session {other}: a destination cannot also be a source", short(name)));
                }
                if let Some(other) = d.l2.span.iter().find(|s| s.id != n && s.ports.contains_key(name)) {
                    return Err(format!("% {} already feeds session {}: a port is the source of one session only", short(name), other.id));
                }
            }
            let held = d.l2.span.iter().find(|s| s.id == n).map(|s| s.ports.keys().cloned().collect::<BTreeSet<_>>()).unwrap_or_default();
            if held.iter().chain(names.iter()).collect::<BTreeSet<_>>().len() > MAX_SOURCE_PORTS {
                return Err(format!("% A session watches at most {MAX_SOURCE_PORTS} ports"));
            }
            let s = c.dm()?.l2.span_entry(n).ok_or("% No such session")?;
            for name in names {
                s.ports.insert(name, dir);
            }
            Ok(String::new())
        }
        MonitorSourceVlan => {
            let n = session_id(arg(0))?;
            let (list, dir) = split_direction(arg(1))?;
            let vlans = vlan_sources(&list)?;
            let d = c.d()?;
            room_for(d, n)?;
            if d.l2.span.iter().any(|s| s.id == n && !s.ports.is_empty()) {
                return Err(format!("% Session {n} watches ports: a session takes ports or VLANs as sources, not both"));
            }
            for v in &vlans {
                if let Some(other) = d.l2.span.iter().find(|s| s.id != n && s.vlans.contains_key(v)) {
                    return Err(format!("% VLAN {v} already feeds session {}: a VLAN is the source of one session only", other.id));
                }
            }
            let s = c.dm()?.l2.span_entry(n).ok_or("% No such session")?;
            for v in vlans {
                s.vlans.insert(v, dir);
            }
            Ok(String::new())
        }
        MonitorDestination => {
            let n = session_id(arg(0))?;
            let name = port_names(c, arg(1))?;
            let [name] = name.as_slice() else { return Err("% Name one port: a session has a single destination".into()) };
            let d = c.d()?;
            room_for(d, n)?;
            if let Some(s) = d.l2.span.iter().find(|s| s.ports.contains_key(name)) {
                return Err(format!("% {} is a source of session {}: a source cannot also be the destination", short(name), s.id));
            }
            if let Some(other) = d.l2.span_destination_of(name).filter(|o| *o != n) {
                return Err(format!("% {} already receives session {other}: a port is the destination of one session only", short(name)));
            }
            if d.ports.iter().any(|p| &p.name == name && p.routed) {
                return Err(format!("% {} is a routed port: a destination must be a switch port", short(name)));
            }
            c.dm()?.l2.span_entry(n).ok_or("% No such session")?.destination = Some(name.clone());
            Ok(String::new())
        }
        NoMonitorSession => {
            let all = matches!(arg(0).to_ascii_lowercase().as_str(), "all" | "local");
            if all {
                c.dm()?.l2.span.clear();
                return Ok(String::new());
            }
            let n = session_id(arg(0))?;
            let d = c.dm()?;
            let before = d.l2.span.len();
            d.l2.span.retain(|s| s.id != n);
            if d.l2.span.len() == before { Err(format!("% Monitor session {n} is not configured")) } else { Ok(String::new()) }
        }
        NoMonitorSource | NoMonitorSourceVlan => {
            let n = session_id(arg(0))?;
            let (list, _) = split_direction(arg(1))?;
            let ports = if id == NoMonitorSource { port_names(c, &list)? } else { Vec::new() };
            let vlans = if id == NoMonitorSourceVlan { vlan_sources(&list)? } else { Vec::new() };
            let d = c.dm()?;
            let s = d.l2.span.iter_mut().find(|s| s.id == n).ok_or_else(|| format!("% Monitor session {n} is not configured"))?;
            let before = s.ports.len() + s.vlans.len();
            for p in &ports {
                s.ports.remove(p);
            }
            for v in &vlans {
                s.vlans.remove(v);
            }
            if s.ports.len() + s.vlans.len() == before {
                return Err(format!("% None of those is a source of session {n}"));
            }
            d.l2.span.retain(|s| !s.is_empty());
            Ok(String::new())
        }
        NoMonitorDestination => {
            let n = session_id(arg(0))?;
            let name = port_names(c, arg(1))?;
            let [name] = name.as_slice() else { return Err("% Name one port".into()) };
            let d = c.dm()?;
            let s = d.l2.span.iter_mut().find(|s| s.id == n).ok_or_else(|| format!("% Monitor session {n} is not configured"))?;
            if s.destination.as_ref() != Some(name) {
                return Err(format!("% {} is not the destination of session {n}", short(name)));
            }
            s.destination = None;
            d.l2.span.retain(|s| !s.is_empty());
            Ok(String::new())
        }
        ShowMonitor => show(&*c.t, c.d()?, None),
        ShowMonitorSession => {
            let which = if arg(0).eq_ignore_ascii_case("all") { None } else { Some(session_id(arg(0))?) };
            show(&*c.t, c.d()?, which)
        }
        _ => Err("% Not a monitor command".into()),
    }
}

/// `Fa0/1 - 3, Fa0/5` for the ports at these indexes (a run of neighbours becomes a range).
fn port_list(d: &Device, idx: &[usize], full: bool) -> String {
    let names: Vec<&str> = idx.iter().filter_map(|i| d.ports.get(*i)).map(|p| p.name.as_str()).collect();
    let numbered = |name: &str| name.rsplit_once('/').and_then(|(pre, n)| n.parse::<u32>().ok().map(|n| (pre.to_string(), n)));
    let label = |name: &str| if full { name.to_string() } else { short(name) };
    let mut out: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(&first) = names.get(at) {
        let mut end = at;
        let mut last = 0;
        if let Some((pre, n0)) = numbered(first) {
            last = n0;
            while let Some((p, n)) = names.get(end + 1).and_then(|x| numbered(*x))
                && p == pre
                && n == last.saturating_add(1)
            {
                last = n;
                end += 1;
            }
        }
        out.push(if end > at { format!("{} - {last}", label(first)) } else { label(first) });
        at = end + 1;
    }
    out.join(", ")
}

/// The indexes of the session's source ports that watch in this direction, in port order.
fn ports_of(d: &Device, s: &SpanSession, dir: SpanDir) -> Vec<usize> {
    d.ports.iter().enumerate().filter(|(_, p)| s.ports.get(&p.name) == Some(&dir)).map(|(i, _)| i).collect()
}

fn vlans_of(s: &SpanSession, dir: SpanDir) -> BTreeSet<VlanId> {
    s.vlans.iter().filter(|(_, d)| **d == dir).map(|(v, _)| *v).collect()
}

/// `show monitor [session N]`.
fn show(t: &Topology, d: &Device, which: Option<u8>) -> R {
    if let Some(n) = which
        && !d.l2.span.iter().any(|s| s.id == n)
    {
        return Err(format!("% Monitor session {n} is not configured"));
    }
    let mut out = String::new();
    for s in d.l2.span.iter().filter(|s| which.is_none_or(|n| n == s.id)) {
        let _ = writeln!(out, "Monitor session {} (local)", s.id);
        let mut sources: Vec<String> = Vec::new();
        for dir in SpanDir::ALL {
            let ports = ports_of(d, s, dir);
            if !ports.is_empty() {
                sources.push(format!("{} {}", dir.name(), port_list(d, &ports, false)));
            }
        }
        let mut vlans: Vec<String> = Vec::new();
        for dir in SpanDir::ALL {
            let set = vlans_of(s, dir);
            if !set.is_empty() {
                vlans.push(format!("{} {}", dir.name(), vlan_list(&set)));
            }
        }
        let _ = writeln!(out, "  Source ports:  {}", if sources.is_empty() { "none".to_string() } else { sources.join("; ") });
        let _ = writeln!(out, "  Source VLANs:  {}", if vlans.is_empty() { "none".to_string() } else { vlans.join("; ") });
        let dest = s.destination.as_deref().and_then(|n| d.port_index(n));
        match dest {
            Some(p) => {
                let up = port_up(t, End { device: d.id, port: p });
                let name = d.ports.get(p).map_or_else(String::new, |x| short(&x.name));
                let _ = writeln!(out, "  Destination:   {name} (link {})", if up { "up" } else { "down" });
            }
            None => {
                let _ = writeln!(out, "  Destination:   none");
            }
        }
        let state = match (s.ports.is_empty() && s.vlans.is_empty(), dest.is_some()) {
            (true, _) => "waiting: nothing to watch yet",
            (false, false) => "waiting: no destination yet",
            (false, true) => "copying",
        };
        let _ = writeln!(out, "  State:         {state}");
    }
    if out.is_empty() {
        out.push_str("No monitor session is configured.\n");
    }
    Ok(out.trim_end().to_string())
}

/// `monitor session …` lines, for `show running-config`.
pub(crate) fn global_lines(s: &mut String, d: &Device) {
    if d.kind != DeviceKind::Switch || d.l2.span.is_empty() {
        return;
    }
    for sess in &d.l2.span {
        let suffix = |dir: SpanDir| if dir == SpanDir::Both { String::new() } else { format!(" {}", dir.name()) };
        for dir in SpanDir::ALL {
            let ports = ports_of(d, sess, dir);
            if !ports.is_empty() {
                let _ = writeln!(s, "monitor session {} source interface {}{}", sess.id, port_list(d, &ports, true), suffix(dir));
            }
            let vlans = vlans_of(sess, dir);
            if !vlans.is_empty() {
                let _ = writeln!(s, "monitor session {} source vlan {}{}", sess.id, vlan_list(&vlans), suffix(dir));
            }
        }
        if let Some(name) = sess.destination.as_deref().and_then(|n| d.port_index(n)).and_then(|p| d.ports.get(p)) {
            let _ = writeln!(s, "monitor session {} destination interface {}", sess.id, name.name);
        }
    }
    let _ = writeln!(s, "!");
}
