//! EIGRP for IPv6: `ipv6 router eigrp N` and its mode (`eigrp router-id`, `passive-interface`,
//! `maximum-paths`, `shutdown`), `ipv6 eigrp N` on interfaces, and the `show ipv6 eigrp` views.

use std::fmt::Write as _;

use netlabcraft_core::parse_ip;
use netlabcraft_model::{Device, DeviceKind, Eigrp6, MAX_NETWORKS, Topology, ifname};
use netlabcraft_sim::eigrp6::Eigrp6State;

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn asn(text: &str) -> Result<u16, String> {
    text.trim().parse::<u16>().ok().filter(|n| *n >= 1).ok_or_else(|| "% The autonomous system number is 1-65535".to_string())
}

fn process<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut Eigrp6, String> {
    c.dm()?.eigrp6.as_mut().ok_or_else(|| "% Not in EIGRP for IPv6 mode".to_string())
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::Ipv6RouterEigrp => {
            let n = asn(arg(0))?;
            let d = c.dm()?;
            if d.kind == DeviceKind::Switch && !d.ip_routing {
                return Err("% EIGRP needs `ip routing` (multilayer switches)".into());
            }
            match &d.eigrp6 {
                Some(e) if e.asn != n => {
                    return Err(format!("% EIGRP for IPv6 already runs here in AS {}; NetLabCraft runs one such process per router", e.asn));
                }
                Some(_) => {}
                None => d.eigrp6 = Some(Eigrp6 { asn: n, ..Default::default() }),
            }
            let mut notes = Vec::new();
            if !d.ipv6_routing {
                notes.push("IPv6 routing is off: EIGRP for IPv6 waits for `ipv6 unicast-routing`.");
            }
            if netlabcraft_sim::eigrp6::router_id(d).is_none() {
                notes.push("EIGRP for IPv6 needs a router id: give one with `eigrp router-id A.B.C.D` (there's no IPv4 address to take it from).");
            }
            c.cli.mode = Mode::Eigrp6;
            Ok(notes.join("\n"))
        }
        Id::NoIpv6RouterEigrp => {
            let n = asn(arg(0))?;
            let d = c.dm()?;
            if d.eigrp6.as_ref().is_none_or(|e| e.asn != n) {
                return Err(format!("% No EIGRP for IPv6 process for AS {n}"));
            }
            d.eigrp6 = None;
            Ok(String::new())
        }
        Id::Ipv6EigrpIf | Id::NoIpv6EigrpIf => {
            let n = asn(arg(0))?;
            let add = id == Id::Ipv6EigrpIf;
            c.each_v6(|v6, _, _| {
                if add {
                    v6.eigrp = Some(n);
                } else if v6.eigrp == Some(n) {
                    v6.eigrp = None;
                }
                Ok(())
            })?;
            let d = c.d()?;
            Ok(if add && d.eigrp6.as_ref().is_none_or(|e| e.asn != n) {
                format!("Saved; the interface joins once `ipv6 router eigrp {n}` runs here.")
            } else {
                String::new()
            })
        }
        Id::EigrpShutdown | Id::NoEigrpShutdown => {
            if c.cli.mode != Mode::Eigrp6 {
                return Err(
                    "% `shutdown` applies to EIGRP for IPv6 here (`ipv6 router eigrp N`); remove an IPv4 process with `no router eigrp N`".into()
                );
            }
            process(c)?.shutdown = id == Id::EigrpShutdown;
            Ok(String::new())
        }
        Id::EigrpRouterId | Id::NoEigrpRouterId => {
            let rid = if id == Id::EigrpRouterId { Some(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?) } else { None };
            process(c)?.router_id = rid;
            Ok(String::new())
        }
        Id::EigrpPassive | Id::NoEigrpPassive => {
            let name =
                ifname::parse(arg(0).trim()).map(|n| n.to_string()).ok_or_else(|| format!("% `{}` is not an interface name", arg(0).trim()))?;
            let e = process(c)?;
            e.passive.retain(|p| *p != name);
            if id == Id::EigrpPassive {
                if e.passive.len() >= MAX_NETWORKS {
                    return Err("% Too many passive interfaces".into());
                }
                e.passive.push(name);
            }
            Ok(String::new())
        }
        Id::EigrpMaximumPaths | Id::NoEigrpMaximumPaths => {
            let n = if id == Id::EigrpMaximumPaths {
                Some(arg(0).parse::<u8>().ok().filter(|n| (1..=16).contains(n)).ok_or("% Maximum paths is 1-16")?)
            } else {
                None
            };
            process(c)?.maximum_paths = n;
            Ok(String::new())
        }
        Id::EigrpNetwork | Id::NoEigrpNetwork => {
            Err("% EIGRP for IPv6 has no network statements: put `ipv6 eigrp N` on each interface instead".into())
        }
        Id::EigrpVariance | Id::NoEigrpVariance => {
            let n = if id == Id::EigrpVariance {
                Some(arg(0).parse::<u8>().ok().filter(|n| (1..=128).contains(n)).ok_or("% The variance is 1-128")?)
            } else {
                None
            };
            process(c)?.variance = n.filter(|v| *v > 1);
            Ok(String::new())
        }
        Id::EigrpAutoSummary | Id::NoEigrpAutoSummary => {
            Err("% EIGRP for IPv6 never summarises automatically; use `ipv6 summary-address eigrp N PREFIX` on an interface".into())
        }
        Id::ShowIpv6EigrpNeighbors => {
            let st = c.sim.eigrp6_state(c.t);
            Ok(neighbors(c.t, c.d()?, &st))
        }
        Id::ShowIpv6EigrpTopology => {
            let st = c.sim.eigrp6_state(c.t);
            Ok(topology(c.d()?, &st))
        }
        Id::ShowIpv6EigrpInterfaces => {
            let st = c.sim.eigrp6_state(c.t);
            Ok(interfaces(c.d()?, &st))
        }
        _ => Err("% Not an EIGRP for IPv6 command".into()),
    }
}

/// Why the process isn't running, if it isn't.
fn stopped(d: &Device) -> Option<String> {
    let e = d.eigrp6.as_ref()?;
    if e.shutdown {
        Some(format!("EIGRP for IPv6, AS {}, is shut down (`no shutdown` in `ipv6 router eigrp {}`).", e.asn, e.asn))
    } else if !d.ipv6_routing {
        Some(format!("EIGRP for IPv6, AS {}, waits for `ipv6 unicast-routing`.", e.asn))
    } else if netlabcraft_sim::eigrp6::router_id(d).is_none() {
        Some(format!("EIGRP for IPv6, AS {}, has no router id (`eigrp router-id A.B.C.D`).", e.asn))
    } else {
        None
    }
}

const NOT_RUNNING: &str = "EIGRP for IPv6 is not running on this router.";

/// `show ipv6 eigrp neighbors`.
fn neighbors(t: &Topology, d: &Device, st: &Eigrp6State) -> String {
    let Some(e) = &d.eigrp6 else { return NOT_RUNNING.into() };
    if let Some(why) = stopped(d) {
        return why;
    }
    let list = st.neighbors.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        return format!(
            "No EIGRP for IPv6 neighbours in AS {} (check `ipv6 eigrp {}` on the interfaces, passive interfaces and cables).",
            e.asn, e.asn
        );
    }
    let mut s = format!("EIGRP for IPv6 neighbours, AS {}\n", e.asn);
    let _ = writeln!(s, "{:<3} {:<28} {:<12} {:<9} Device", "#", "Link-local address", "Interface", "Hold (s)");
    for (k, n) in list.iter().enumerate() {
        let name = t.device(n.device).map_or("?".to_string(), |x| x.name.clone());
        let _ = writeln!(s, "{:<3} {:<28} {:<12} {:<9} {name}", k, n.address.to_string(), ifname::short(&n.iface), n.hold_secs);
    }
    s.trim_end().to_string()
}

/// `show ipv6 eigrp topology`.
fn topology(d: &Device, st: &Eigrp6State) -> String {
    let Some(e) = &d.eigrp6 else { return NOT_RUNNING.into() };
    if let Some(why) = stopped(d) {
        return why;
    }
    let rid = st.router_ids.get(&d.id).map_or("none".to_string(), |r| r.to_string());
    let mut s = format!("EIGRP for IPv6 topology, AS {}, router id {rid}\n", e.asn);
    s.push_str("Every destination is passive (DUAL has converged). Paths show (distance / reported distance).\n");
    let connected: Vec<String> = st.interfaces.get(&d.id).into_iter().flatten().map(|i| i.name.clone()).collect();
    let entries = st.topology.get(&d.id).cloned().unwrap_or_default();
    if entries.is_empty() {
        let _ = write!(
            s,
            "\n(no learned destinations; EIGRP interfaces: {})",
            if connected.is_empty() { "none".to_string() } else { connected.join(", ") }
        );
        return s;
    }
    for t in &entries {
        let succ = t.paths.iter().filter(|p| p.successor).count();
        let _ = writeln!(s, "\n{}  FD {}, {succ} successor{}", t.net, t.feasible_distance, if succ == 1 { "" } else { "s" });
        for p in &t.paths {
            let role = if p.successor { "successor" } else { "feasible successor" };
            let _ = writeln!(s, "    {role:<19} via {}, {} ({} / {})", p.via, ifname::short(&p.iface), p.distance, p.reported);
        }
    }
    s.trim_end().to_string()
}

/// `show ipv6 eigrp interfaces`.
fn interfaces(d: &Device, st: &Eigrp6State) -> String {
    let Some(e) = &d.eigrp6 else { return NOT_RUNNING.into() };
    if let Some(why) = stopped(d) {
        return why;
    }
    let ifs = st.interfaces.get(&d.id).cloned().unwrap_or_default();
    if ifs.is_empty() {
        return format!("No interface runs EIGRP for IPv6 in AS {} (`ipv6 eigrp {}` on an interface that is up).", e.asn, e.asn);
    }
    let mut s = format!("EIGRP for IPv6 interfaces, AS {}\n", e.asn);
    let _ = writeln!(s, "{:<12} {:<6} {:<12} {:<12} Passive", "Interface", "Peers", "Bandwidth", "Delay (µs)");
    for i in &ifs {
        let peers = st.neighbors.get(&d.id).into_iter().flatten().filter(|n| n.iface == i.name).count();
        let _ = writeln!(
            s,
            "{:<12} {:<6} {:<12} {:<12} {}",
            ifname::short(&i.name),
            peers,
            format!("{} kbit/s", i.bandwidth),
            u64::from(i.delay) * 10,
            if i.passive { "yes" } else { "no" }
        );
    }
    s.trim_end().to_string()
}

/// The `ipv6 router eigrp` block of the running configuration.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let Some(e) = &d.eigrp6 else { return };
    let _ = writeln!(s, "ipv6 router eigrp {}", e.asn);
    if let Some(id) = e.router_id {
        let _ = writeln!(s, " eigrp router-id {id}");
    }
    for p in &e.passive {
        let _ = writeln!(s, " passive-interface {p}");
    }
    if let Some(n) = e.maximum_paths {
        let _ = writeln!(s, " maximum-paths {n}");
    }
    if let Some(n) = e.variance {
        let _ = writeln!(s, " variance {n}");
    }
    if e.shutdown {
        let _ = writeln!(s, " shutdown");
    }
    let _ = writeln!(s, "!");
}
