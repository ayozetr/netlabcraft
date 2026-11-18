//! Object tracking: `track N interface|ip route|ip sla|list`, `delay`, `object`, `show track`,
//! and the lines of `show running-config`. What an object reports is computed by
//! `netlabcraft_sim::track`; static routes (`ip route … track N`) and HSRP and VRRP groups
//! (`standby N track M decrement D`, `vrrp N track M`) follow it.

use std::fmt::Write as _;

use netlabcraft_core::Ipv4Net;
use netlabcraft_model::Device;
use netlabcraft_model::track::{Kind, MAX_DELAY, MAX_ID, MAX_MEMBERS, MAX_OBJECTS, Member, Object};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn object_number(text: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|n| (1..=MAX_ID).contains(n)).ok_or_else(|| format!("% The object number is 1-{MAX_ID}"))
}

/// The object the session is editing.
fn current(c: &Ctx) -> Result<u32, String> {
    match c.cli.mode {
        Mode::Track(n) => Ok(n),
        _ => Err("% Not in tracked-object mode".into()),
    }
}

/// An interface name as the device spells it: a port, a subinterface, a VLAN interface, a loopback
/// or a tunnel.
fn interface_name(c: &Ctx, text: &str) -> Result<String, String> {
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    let lower = compact.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix("vlan").and_then(|n| n.parse::<u16>().ok()) {
        return Ok(format!("Vlan{n}"));
    }
    if let Some(n) = netlabcraft_model::routing::loopback_number(&compact) {
        return Ok(format!("Loopback{n}"));
    }
    if let Some(n) = netlabcraft_model::vpn::tunnel_number(&compact) {
        return Ok(format!("Tunnel{n}"));
    }
    let r = c.iface_ref(&compact)?;
    let d = c.d()?;
    let p = d.ports.get(r.port).ok_or("% No such port")?;
    Ok(r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))
}

fn put(c: &mut Ctx, n: u32, kind: Kind) -> R {
    let d = c.dm()?;
    if !d.track.objects.contains_key(&n) && d.track.objects.len() >= MAX_OBJECTS {
        return Err(format!("% At most {MAX_OBJECTS} tracked objects"));
    }
    let (delay_up, delay_down) = d.track.objects.get(&n).map_or((0, 0), |o| (o.delay_up, o.delay_down));
    d.track.objects.insert(n, Object { kind, delay_up, delay_down });
    d.track.forget_state(n);
    c.cli.mode = Mode::Track(n);
    Ok(String::new())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::TrackInterface => {
            let n = object_number(arg(0))?;
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            let (name_words, ip_routing) = match words.as_slice() {
                [rest @ .., a, b] if a.eq_ignore_ascii_case("ip") && b.eq_ignore_ascii_case("routing") && !rest.is_empty() => (rest, true),
                [rest @ .., last] if last.eq_ignore_ascii_case("line-protocol") && !rest.is_empty() => (rest, false),
                _ => return Err("% Say `line-protocol` or `ip routing` after the interface".into()),
            };
            let name = interface_name(c, &name_words.join(" "))?;
            put(c, n, Kind::Interface { name, ip_routing })
        }
        Id::TrackIpRoute => {
            let n = object_number(arg(0))?;
            let net = if args.len() > 2 {
                let addr = netlabcraft_core::parse_ip(arg(1)).map_err(|e| format!("% {e}"))?;
                let mask = netlabcraft_core::parse_ip(arg(2)).map_err(|e| format!("% {e}"))?;
                Ipv4Net::with_mask(addr, mask).map_err(|e| format!("% {e}"))?
            } else {
                arg(1).parse::<Ipv4Net>().map_err(|e| format!("% {e}"))?
            };
            put(c, n, Kind::Route { net: net.network() })
        }
        Id::TrackSla => {
            let n = object_number(arg(0))?;
            let op = arg(1).trim().parse::<u32>().ok().filter(|o| (1..=2_147_483_647).contains(o)).ok_or("% The operation number is 1-2147483647")?;
            let state = match arg(2).to_ascii_lowercase().as_str() {
                "" | "state" => true,
                "reachability" => false,
                _ => return Err("% Say `reachability` or `state`".into()),
            };
            put(c, n, Kind::Sla { op, state })
        }
        Id::TrackList => {
            let n = object_number(arg(0))?;
            let and = match arg(1).to_ascii_lowercase().as_str() {
                "and" => true,
                "or" => false,
                _ => return Err("% Say `and` or `or`".into()),
            };
            let members = match c.d()?.track.objects.get(&n).map(|o| &o.kind) {
                Some(Kind::List { members, .. }) => members.clone(),
                _ => Vec::new(),
            };
            put(c, n, Kind::List { and, members })
        }
        Id::TrackEnter => {
            let n = object_number(arg(0))?;
            if !c.d()?.track.objects.contains_key(&n) {
                return Err(format!("% There is no tracked object {n}; create it with `track {n} interface|ip|list …`"));
            }
            c.cli.mode = Mode::Track(n);
            Ok(String::new())
        }
        Id::NoTrack => {
            let n = object_number(arg(0))?;
            if !c.dm()?.track.remove(n) {
                return Err(format!("% There is no tracked object {n}"));
            }
            if c.cli.mode == Mode::Track(n) {
                c.cli.mode = Mode::Config;
            }
            Ok(String::new())
        }
        Id::TrackDelay => {
            let n = current(c)?;
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            if words.len() != 2 && words.len() != 4 {
                return Err("% Say `delay up <seconds>`, `delay down <seconds>` or both".into());
            }
            let (mut up, mut down) = (None, None);
            for pair in words.chunks(2) {
                let [which, secs] = pair else { return Err("% Incomplete command.".into()) };
                let secs = secs.parse::<u32>().ok().filter(|s| *s <= MAX_DELAY).ok_or_else(|| format!("% The delay is 0-{MAX_DELAY} seconds"))?;
                match which.to_ascii_lowercase().as_str() {
                    "up" => up = Some(secs),
                    "down" => down = Some(secs),
                    _ => return Err("% Say `up` or `down`".into()),
                }
            }
            let d = c.dm()?;
            if let Some(o) = d.track.objects.get_mut(&n) {
                if let Some(u) = up {
                    o.delay_up = u;
                }
                if let Some(x) = down {
                    o.delay_down = x;
                }
            }
            d.track.pending.remove(&n);
            Ok(String::new())
        }
        Id::NoTrackDelay => {
            let n = current(c)?;
            let d = c.dm()?;
            if let Some(o) = d.track.objects.get_mut(&n) {
                o.delay_up = 0;
                o.delay_down = 0;
            }
            d.track.pending.remove(&n);
            Ok(String::new())
        }
        Id::TrackObject | Id::TrackObjectNot => {
            let n = current(c)?;
            let m = object_number(arg(0))?;
            if m == n {
                return Err("% A list cannot contain itself".into());
            }
            let not = id == Id::TrackObjectNot;
            let d = c.dm()?;
            let o = d.track.objects.get_mut(&n).ok_or("% That object no longer exists")?;
            let Kind::List { members, .. } = &mut o.kind else { return Err("% Only a list object holds other objects".into()) };
            members.retain(|x| x.id != m);
            if members.len() >= MAX_MEMBERS {
                return Err(format!("% At most {MAX_MEMBERS} objects in a list"));
            }
            members.push(Member { id: m, not });
            d.track.forget_state(n);
            Ok(String::new())
        }
        Id::NoTrackObject => {
            let n = current(c)?;
            let m = object_number(arg(0))?;
            let d = c.dm()?;
            let o = d.track.objects.get_mut(&n).ok_or("% That object no longer exists")?;
            let Kind::List { members, .. } = &mut o.kind else { return Err("% Only a list object holds other objects".into()) };
            let before = members.len();
            members.retain(|x| x.id != m);
            if members.len() == before {
                return Err(format!("% Object {m} is not in this list"));
            }
            d.track.forget_state(n);
            Ok(String::new())
        }
        Id::ShowTrack => {
            let only = if arg(0).is_empty() { None } else { Some(object_number(arg(0))?) };
            let now = c.sim.now_us();
            let d = c.d()?.clone();
            show(c.t, &d, only, now)
        }
        _ => Err("% Not a tracking command".into()),
    }
}

/// What an object watches, in words.
fn describe(k: &Kind) -> String {
    match k {
        Kind::Interface { name, ip_routing: false } => format!("interface {name} line-protocol"),
        Kind::Interface { name, ip_routing: true } => format!("interface {name} ip routing"),
        Kind::Route { net } => format!("ip route {net} reachability"),
        Kind::Sla { op, state: false } => format!("ip sla {op} reachability"),
        Kind::Sla { op, state: true } => format!("ip sla {op} state"),
        Kind::List { and, .. } => format!("list boolean {}", if *and { "and" } else { "or" }),
    }
}

fn age(us: u64) -> String {
    let s = us / 1_000_000;
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// What follows object `id`: static routes, HSRP and VRRP groups, and lists.
fn followers(d: &Device, id: u32) -> Vec<String> {
    let mut out = Vec::new();
    for (key, n) in &d.track.route_track {
        if *n == id {
            out.push(format!("static route {key}"));
        }
    }
    let wanted = |name: &str| name.trim().parse::<u32>().ok() == Some(id);
    let mut hsrp = |iface: &str, list: &[netlabcraft_model::Standby]| {
        for g in list {
            for (name, dec) in &g.track {
                if wanted(name) {
                    out.push(format!("HSRP group {} on {iface}, priority lowered by {dec} while down", g.group));
                }
            }
        }
    };
    for p in &d.ports {
        hsrp(&p.name, &p.standby);
        for s in &p.subifs {
            hsrp(&format!("{}.{}", p.name, s.number), &s.standby);
        }
    }
    for s in &d.svis {
        hsrp(&format!("Vlan{}", s.vlan), &s.standby);
    }
    for (iface, groups) in &d.fhrp.vrrp {
        for g in groups {
            for (name, dec) in &g.track {
                if wanted(name) {
                    out.push(format!("VRRP group {} on {iface}, priority lowered by {dec} while down", g.group));
                }
            }
        }
    }
    for (other, o) in &d.track.objects {
        if let Kind::List { members, .. } = &o.kind
            && members.iter().any(|m| m.id == id)
        {
            out.push(format!("list object {other}"));
        }
    }
    out
}

fn show(t: &netlabcraft_model::Topology, d: &Device, only: Option<u32>, now: u64) -> R {
    let ids: Vec<u32> = d.track.objects.keys().copied().filter(|i| only.is_none_or(|o| o == *i)).collect();
    if ids.is_empty() {
        return Ok(match only {
            Some(n) => format!("There is no tracked object {n}."),
            None => "No object is tracked here.".into(),
        });
    }
    let mut s = String::new();
    for id in ids {
        let Some(o) = d.track.objects.get(&id) else { continue };
        let up = netlabcraft_sim::track::is_up(t, d, id);
        let _ = writeln!(s, "Track {id}: {}", describe(&o.kind));
        let _ = writeln!(s, "  State is {}", if up { "Up" } else { "Down" });
        if let Some(r) = d.track.reported.get(&id) {
            let _ = writeln!(
                s,
                "    {} change{}, last change {} ago",
                r.changes,
                if r.changes == 1 { "" } else { "s" },
                age(now.saturating_sub(r.since_us))
            );
        }
        if let Some((state, since)) = d.track.pending.get(&id) {
            let _ = writeln!(
                s,
                "    Waiting out the delay: it has been {} for {} seconds",
                if *state { "up" } else { "down" },
                now.saturating_sub(*since) / 1_000_000
            );
        }
        if o.delay_up != 0 || o.delay_down != 0 {
            let _ = writeln!(s, "  Delay: up {} s, down {} s", o.delay_up, o.delay_down);
        }
        if let Kind::List { members, .. } = &o.kind {
            for m in members {
                let _ = writeln!(s, "  Member: object {}{}", m.id, if m.not { " (negated)" } else { "" });
            }
        }
        let used = followers(d, id);
        if !used.is_empty() {
            let _ = writeln!(s, "  Followed by:");
            for u in used {
                let _ = writeln!(s, "    {u}");
            }
        }
    }
    Ok(s.trim_end().to_string())
}

/// The `track` blocks of `show running-config`.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    for (id, o) in &d.track.objects {
        let _ = writeln!(s, "track {id} {}", describe(&o.kind));
        if o.delay_up != 0 || o.delay_down != 0 {
            let mut words = String::new();
            if o.delay_up != 0 {
                let _ = write!(words, " up {}", o.delay_up);
            }
            if o.delay_down != 0 {
                let _ = write!(words, " down {}", o.delay_down);
            }
            let _ = writeln!(s, " delay{words}");
        }
        if let Kind::List { members, .. } = &o.kind {
            for m in members {
                let _ = writeln!(s, " object {}{}", m.id, if m.not { " not" } else { "" });
            }
        }
        let _ = writeln!(s, "!");
    }
}

/// The object number an HSRP or VRRP `track` word names, if it is a number.
pub(crate) fn object_word(text: &str) -> Option<u32> {
    text.trim().parse::<u32>().ok().filter(|n| (1..=MAX_ID).contains(n))
}
