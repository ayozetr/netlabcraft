//! Policy-based routing: `ip policy route-map` on an interface, `ip local policy route-map`, the
//! `match length` and `set` lines that only policy routing reads, and `show ip policy`. The
//! route map is an ordinary one (`exec/policy.rs`); the decision is taken by
//! `netlabcraft_sim::pbr` before the routing table is asked.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::parse_ip;
use netlabcraft_model::Device;
use netlabcraft_model::pbr::MAX_POLICIES;

use super::policy::clause;
use super::{Ctx, R};
use crate::grammar::Id;

fn map_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > netlabcraft_model::policy::MAX_NAME || t.chars().any(char::is_control) {
        return Err(format!("% A route-map name is 1-{} characters", netlabcraft_model::policy::MAX_NAME));
    }
    Ok(t.to_string())
}

/// A next hop that can be one: not 0.0.0.0, a multicast or the broadcast address.
fn next_hop(text: &str) -> Result<Ipv4Addr, String> {
    let a = parse_ip(text.trim()).map_err(|e| format!("% {e}"))?;
    if a.is_unspecified() || a.is_multicast() || a == Ipv4Addr::BROADCAST {
        return Err(format!("% {a} is not an address a next hop can have"));
    }
    Ok(a)
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::IpPolicyRouteMap => {
            let name = map_name(arg(0))?;
            let ifaces = c.mode_iface_names()?;
            let d = c.dm()?;
            for n in ifaces {
                if !d.pbr.interfaces.contains_key(&n) && d.pbr.interfaces.len() >= MAX_POLICIES {
                    return Err("% Too many interfaces have a policy".into());
                }
                d.pbr.interfaces.insert(n, name.clone());
            }
            Ok(if d.route_maps.iter().any(|m| m.name == name) {
                String::new()
            } else {
                format!("Note: there is no route map {name} yet; until it exists the packets are routed by the table")
            })
        }
        Id::NoIpPolicyRouteMap => {
            let ifaces = c.mode_iface_names()?;
            let want = arg(0).trim().to_string();
            let d = c.dm()?;
            for n in ifaces {
                if want.is_empty() || d.pbr.interfaces.get(&n) == Some(&want) {
                    d.pbr.interfaces.remove(&n);
                }
            }
            Ok(String::new())
        }
        Id::IpLocalPolicy => {
            let name = map_name(arg(0))?;
            c.dm()?.pbr.local = Some(name);
            Ok(String::new())
        }
        Id::NoIpLocalPolicy => {
            c.dm()?.pbr.local = None;
            Ok(String::new())
        }
        Id::ShowIpPolicy => Ok(show_policy(c.d()?, c.sim)),
        Id::RmMatchLength => {
            let lo = arg(0).trim().parse::<u16>().map_err(|_| "% The length is 0-65535 bytes")?;
            let hi = arg(1).trim().parse::<u16>().map_err(|_| "% The length is 0-65535 bytes")?;
            clause(c)?.length = Some((lo.min(hi), lo.max(hi)));
            Ok(String::new())
        }
        Id::NoRmMatchLength => {
            clause(c)?.length = None;
            Ok(String::new())
        }
        Id::RmSetDefaultNextHop => {
            let a = next_hop(arg(0))?;
            clause(c)?.sets.default_next_hop = Some(a);
            Ok(String::new())
        }
        Id::NoRmSetDefaultNextHop => {
            clause(c)?.sets.default_next_hop = None;
            Ok(String::new())
        }
        Id::RmSetInterface => {
            let name = super::track::interface_name(c, arg(0))?;
            clause(c)?.sets.interface = Some(name);
            Ok(String::new())
        }
        Id::NoRmSetInterface => {
            clause(c)?.sets.interface = None;
            Ok(String::new())
        }
        Id::RmSetPrecedence => {
            let p = arg(0).trim().parse::<u8>().ok().filter(|p| *p <= 7).ok_or("% The precedence is 0-7")?;
            let cl = clause(c)?;
            cl.sets.precedence = Some(p);
            cl.sets.dscp = None;
            Ok(String::new())
        }
        Id::NoRmSetPrecedence => {
            clause(c)?.sets.precedence = None;
            Ok(String::new())
        }
        Id::RmSetDscp => {
            let v = netlabcraft_wire::qos::parse_dscp(arg(0)).filter(|v| *v <= 63).ok_or("% The DSCP is 0-63 or a name such as af21, cs3 or ef")?;
            let cl = clause(c)?;
            cl.sets.dscp = Some(v);
            cl.sets.precedence = None;
            Ok(String::new())
        }
        Id::NoRmSetDscp => {
            clause(c)?.sets.dscp = None;
            Ok(String::new())
        }
        Id::RmSetNextHopVerify => {
            let a = next_hop(arg(0))?;
            let track = arg(2)
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=netlabcraft_model::track::MAX_ID).contains(n))
                .ok_or("% The tracked object is 1-1000")?;
            let cl = clause(c)?;
            cl.sets.next_hop = Some(a);
            cl.sets.next_hop_track = Some(track);
            Ok(String::new())
        }
        _ => Err("% Not a policy-routing command".into()),
    }
}

/// `show ip policy`: the interfaces and the route maps that route their packets, with what each
/// clause took.
fn show_policy(d: &Device, sim: &netlabcraft_sim::Sim) -> String {
    let mut rows: Vec<(String, &str)> = d.pbr.interfaces.iter().map(|(i, m)| (i.clone(), m.as_str())).collect();
    if let Some(m) = &d.pbr.local {
        rows.push(("(this router's own packets)".into(), m.as_str()));
    }
    if rows.is_empty() {
        return "No policy routing is set up (apply a route map with `ip policy route-map NAME` on an interface).".into();
    }
    let mut s = String::new();
    let _ = writeln!(s, "{:<30} {}", "Interface", "Route map");
    for (iface, map) in &rows {
        let _ = writeln!(s, "{iface:<30} {map}");
    }
    for map in rows.iter().map(|(_, m)| *m).collect::<std::collections::BTreeSet<_>>() {
        let Some(m) = d.route_maps.iter().find(|x| x.name == map) else {
            let _ = writeln!(s, "Route map {map} does not exist: its packets are routed by the table.");
            continue;
        };
        for cl in &m.clauses {
            let (packets, bytes) = sim.pbr_hits(d.id, &m.name, cl.seq);
            let _ = writeln!(s, "  {} {} {}: {packets} packets, {bytes} bytes", m.name, if cl.permit { "permit" } else { "deny" }, cl.seq);
        }
    }
    s.trim_end().to_string()
}

/// ` ip policy route-map NAME` under an interface of `show running-config`.
pub(crate) fn iface_line(s: &mut String, d: &Device, name: &str) {
    if let Some(m) = d.pbr.for_interface(name) {
        let _ = writeln!(s, " ip policy route-map {m}");
    }
}

/// `ip local policy route-map NAME` of `show running-config`.
pub(crate) fn local_line(s: &mut String, d: &Device) {
    if let Some(m) = &d.pbr.local {
        let _ = writeln!(s, "ip local policy route-map {m}\n!");
    }
}
