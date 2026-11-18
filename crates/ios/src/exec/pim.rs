//! Multicast routing: `ip multicast-routing`, PIM on interfaces, rendezvous points, groups a
//! router interface joins, and the `show` commands for the multicast table, PIM and IGMP.

use std::fmt::Write as _;

use netlabcraft_core::parse_ip;
use netlabcraft_model::Device;
use netlabcraft_model::knobs::Scope;
use netlabcraft_model::pim::{MAX_IFACES, MAX_JOINED, MAX_RPS, Pim, PimIface, PimMode, RpLine};
use netlabcraft_sim::pim::MrouteRow;

use super::{Ctx, R};
use crate::grammar::Id;
use crate::render::hms;

const OFF: &str = "% Multicast routing is not on (`ip multicast-routing`)";

fn unicast(text: &str) -> Result<std::net::Ipv4Addr, String> {
    let a = parse_ip(text).map_err(|e| format!("% {e}"))?;
    if a.is_unspecified() || a.is_multicast() || a.is_broadcast() {
        return Err("% That is not a unicast address".into());
    }
    Ok(a)
}

fn routing_on(c: &Ctx) -> Result<(), String> {
    if c.d()?.pim.routing { Ok(()) } else { Err(OFF.into()) }
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim().to_string();
    match id {
        Id::McRouting | Id::NoMcRouting => {
            let d = c.dm()?;
            d.pim.routing = id == Id::McRouting;
            // The older "kept only" setting of the same name is replaced by this one.
            d.knobs.clear(Scope::Global, "ip multicast-routing");
            Ok(String::new())
        }
        Id::McRpAddress => {
            let rp = unicast(&arg(0))?;
            let acl = Some(arg(1)).filter(|a| !a.is_empty());
            let pim = &mut c.dm()?.pim;
            pim.rps.retain(|r| r.rp != rp);
            if pim.rps.len() >= MAX_RPS {
                return Err(format!("% At most {MAX_RPS} rendezvous point lines"));
            }
            pim.rps.push(RpLine { rp, acl });
            Ok(String::new())
        }
        Id::NoMcRpAddress => {
            let rp = unicast(&arg(0))?;
            let acl = Some(arg(1)).filter(|a| !a.is_empty());
            let pim = &mut c.dm()?.pim;
            let before = pim.rps.len();
            pim.rps.retain(|r| !(r.rp == rp && (acl.is_none() || r.acl == acl)));
            if pim.rps.len() == before { Err("% No such rendezvous point line".into()) } else { Ok(String::new()) }
        }
        Id::McPimSparse | Id::McPimDense | Id::McPimSparseDense => {
            let mode = match id {
                Id::McPimSparse => PimMode::Sparse,
                Id::McPimDense => PimMode::Dense,
                _ => PimMode::SparseDense,
            };
            let names = super::routing::if_names(c)?;
            let d = c.dm()?;
            for n in names {
                if !d.pim.ifaces.contains_key(&n) && d.pim.ifaces.len() >= MAX_IFACES {
                    return Err("% Too many interfaces run PIM".into());
                }
                d.pim.ifaces.entry(n).and_modify(|i| i.mode = mode).or_insert_with(|| PimIface::new(mode));
            }
            Ok(if d.pim.routing { String::new() } else { "PIM acts once `ip multicast-routing` is on.".into() })
        }
        Id::NoMcPim => {
            let names = super::routing::if_names(c)?;
            let d = c.dm()?;
            for n in names {
                d.pim.ifaces.remove(&n);
            }
            Ok(String::new())
        }
        Id::McDrPriority => {
            let p: u32 = arg(0).parse().ok().filter(|p| *p < u32::MAX).ok_or("% The priority is 0-4294967294")?;
            let names = super::routing::if_names(c)?;
            let d = c.dm()?;
            for n in &names {
                d.pim.ifaces.get_mut(n).ok_or("% Run PIM on the interface first (`ip pim sparse-mode`)")?.dr_priority = p;
            }
            Ok(String::new())
        }
        Id::NoMcDrPriority => {
            let names = super::routing::if_names(c)?;
            let d = c.dm()?;
            for n in &names {
                if let Some(i) = d.pim.ifaces.get_mut(n) {
                    i.dr_priority = 1;
                }
            }
            Ok(String::new())
        }
        Id::McJoinGroup => {
            let g = parse_ip(&arg(0)).map_err(|e| format!("% {e}"))?;
            if !Pim::joinable(g) {
                return Err("% That is not a group an interface can join (224.0.0.0/24 belongs to protocols on the link)".into());
            }
            let names = super::routing::if_names(c)?;
            let d = c.dm()?;
            for n in names {
                let set = d.pim.joins.entry(n).or_default();
                if set.len() >= MAX_JOINED && !set.contains(&g) {
                    return Err(format!("% At most {MAX_JOINED} groups per interface"));
                }
                set.insert(g);
            }
            Ok(String::new())
        }
        Id::NoMcJoinGroup => {
            let g = parse_ip(&arg(0)).map_err(|e| format!("% {e}"))?;
            let names = super::routing::if_names(c)?;
            let d = c.dm()?;
            for n in names {
                if let Some(set) = d.pim.joins.get_mut(&n) {
                    set.remove(&g);
                    if set.is_empty() {
                        d.pim.joins.remove(&n);
                    }
                }
            }
            Ok(String::new())
        }
        Id::ClearMroute => {
            routing_on(c)?;
            c.sim.clear_mroute(c.dev);
            Ok(String::new())
        }
        Id::ShowMroute | Id::ShowMrouteCount | Id::ShowMrouteSummary | Id::ShowMrouteGroup => {
            routing_on(c)?;
            let group = if id == Id::ShowMrouteGroup { Some(parse_ip(&arg(0)).map_err(|e| format!("% {e}"))?) } else { None };
            let t = &*c.t;
            let rows: Vec<MrouteRow> = c.sim.mroute_table(t, c.dev).into_iter().filter(|r| group.is_none_or(|g| r.group == g)).collect();
            Ok(match id {
                Id::ShowMrouteCount => counts(&rows),
                Id::ShowMrouteSummary => summary(&rows),
                _ => table(&rows),
            })
        }
        Id::ShowPimNeighbor => {
            routing_on(c)?;
            let t = &*c.t;
            let rows = c.sim.pim_neighbors(t, c.dev);
            if rows.is_empty() {
                return Ok("No PIM neighbours.".into());
            }
            let mut s = format!("{:<16} {:<24} {:<9} DR priority\n", "Neighbour", "Interface", "Up for");
            for r in rows {
                let _ = writeln!(
                    s,
                    "{:<16} {:<24} {:<9} {}{}",
                    r.addr.to_string(),
                    r.iface,
                    hms(r.up_us),
                    r.priority,
                    if r.dr { " (designated router)" } else { "" }
                );
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowPimInterface => {
            routing_on(c)?;
            let t = &*c.t;
            let rows = c.sim.pim_interfaces(t, c.dev);
            if rows.is_empty() {
                return Ok("No interface runs PIM.".into());
            }
            let mut s =
                format!("{:<16} {:<24} {:<13} {:<10} {:<12} Designated router\n", "Address", "Interface", "Mode", "Neighbours", "DR priority");
            for r in rows {
                let _ =
                    writeln!(s, "{:<16} {:<24} {:<13} {:<10} {:<12} {}", r.addr.to_string(), r.iface, r.mode.label(), r.neighbors, r.priority, r.dr);
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowPimRp => {
            let d = c.d()?;
            if d.pim.rps.is_empty() {
                return Ok("No rendezvous point is configured (`ip pim rp-address`).".into());
            }
            let mut s = String::from("Group-to-rendezvous-point mappings (static):\n");
            for r in &d.pim.rps {
                let groups = r.acl.as_ref().map_or("all groups (224.0.0.0/4)".to_string(), |a| format!("the groups access list {a} permits"));
                let _ = writeln!(s, "  RP {:<15} {groups}", r.rp.to_string());
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowRouterIgmpGroups => {
            routing_on(c)?;
            let t = &*c.t;
            let rows = c.sim.router_igmp_groups(t, c.dev);
            if rows.is_empty() {
                return Ok("No group has a member on these interfaces.".into());
            }
            let mut s = format!("{:<16} {:<24} Reported by\n", "Group", "Interface");
            for r in rows {
                let _ = writeln!(
                    s,
                    "{:<16} {:<24} {}",
                    r.group.to_string(),
                    r.iface,
                    r.reporter.map_or("this router (ip igmp join-group)".to_string(), |a| a.to_string())
                );
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowRouterIgmpInterface => {
            routing_on(c)?;
            let t = &*c.t;
            let ifs = c.sim.pim_interfaces(t, c.dev);
            let groups = c.sim.router_igmp_groups(t, c.dev);
            if ifs.is_empty() {
                return Ok("No interface runs PIM, so none sends IGMP queries.".into());
            }
            let mut s = String::new();
            for i in ifs {
                let n = groups.iter().filter(|g| g.iface == i.iface).count();
                let _ = writeln!(
                    s,
                    "{} ({}): IGMP version 2, general query every 60 s, {n} group{} with members",
                    i.iface,
                    i.addr,
                    if n == 1 { "" } else { "s" }
                );
            }
            Ok(s.trim_end().to_string())
        }
        _ => Err("% That command is not implemented".into()),
    }
}

fn flags(r: &MrouteRow) -> String {
    let mut f = String::new();
    f.push(if r.sparse { 'S' } else { 'D' });
    if r.source.is_some() && r.sparse {
        f.push('T');
    }
    if r.members {
        f.push('C');
    }
    if r.first_hop {
        f.push('F');
    }
    if r.oifs.is_empty() {
        f.push('P');
    }
    f
}

fn name(r: &MrouteRow) -> String {
    match r.source {
        Some(s) => format!("({s}, {})", r.group),
        None => format!("(*, {})", r.group),
    }
}

fn table(rows: &[MrouteRow]) -> String {
    if rows.is_empty() {
        return "The multicast routing table is empty.".into();
    }
    let mut s = String::from(
        "Flags: S sparse, D dense, C a member is on a connected link, T shortest-path tree, F first-hop router, P pruned (no outgoing interface)\n\n",
    );
    for r in rows {
        let rp = r.rp.filter(|_| r.sparse).map_or(String::new(), |rp| format!(", RP {rp}"));
        let _ = writeln!(s, "{}, up {}{rp}, flags: {}", name(r), hms(r.age_us), flags(r));
        let incoming = r.iif.clone().unwrap_or_else(|| "Null".into());
        let via = match (r.rpf_nbr, r.owner, r.source.is_some()) {
            (Some(n), ..) => format!(", RPF neighbour {n}"),
            (None, true, _) => ", this router is the root of the tree".to_string(),
            (None, _, true) if r.first_hop => ", the source is on this link".to_string(),
            _ => ", no route to the root of the tree".to_string(),
        };
        let _ = writeln!(s, "  Incoming interface: {incoming}{via}");
        if r.oifs.is_empty() {
            let _ = writeln!(s, "  Outgoing interface list: Null");
        } else {
            let _ = writeln!(s, "  Outgoing interface list:");
            for o in &r.oifs {
                let _ = writeln!(s, "    {}, Forward/{}", o.iface, if o.sparse { "Sparse" } else { "Dense" });
            }
        }
    }
    s.trim_end().to_string()
}

fn summary(rows: &[MrouteRow]) -> String {
    let stars = rows.iter().filter(|r| r.source.is_none()).count();
    let sources = rows.len() - stars;
    let mut s = format!(
        "Multicast routing table: {stars} (*,G) entr{} and {sources} (S,G) entr{}\n",
        if stars == 1 { "y" } else { "ies" },
        if sources == 1 { "y" } else { "ies" }
    );
    for r in rows {
        let _ = writeln!(
            s,
            "{:<34} in {:<22} out {}  flags {:<4} up {}",
            name(r),
            r.iif.clone().unwrap_or_else(|| "Null".into()),
            r.oifs.len(),
            flags(r),
            hms(r.age_us)
        );
    }
    s.trim_end().to_string()
}

fn counts(rows: &[MrouteRow]) -> String {
    let mine: Vec<&MrouteRow> = rows.iter().filter(|r| r.source.is_some()).collect();
    if mine.is_empty() {
        return "No multicast traffic has been handled.".into();
    }
    let mut s = String::from("Multicast traffic handled by this router:\n");
    let mut last = None;
    for r in mine {
        if last != Some(r.group) {
            let _ = writeln!(s, "Group {}", r.group);
            last = Some(r.group);
        }
        let src = r.source.map_or(String::new(), |a| a.to_string());
        let _ =
            writeln!(s, "  Source {src}: {} packets forwarded ({} bytes), {} dropped by the reverse path check", r.forwarded, r.bytes, r.rpf_failed);
    }
    s.trim_end().to_string()
}

/// The global lines of the running configuration.
pub(crate) fn global_lines(s: &mut String, d: &Device) {
    if d.pim.routing {
        let _ = writeln!(s, "ip multicast-routing");
    }
    for r in &d.pim.rps {
        match &r.acl {
            Some(a) => {
                let _ = writeln!(s, "ip pim rp-address {} {a}", r.rp);
            }
            None => {
                let _ = writeln!(s, "ip pim rp-address {}", r.rp);
            }
        }
    }
}

/// The lines under one interface.
pub(crate) fn iface_lines(s: &mut String, d: &Device, name: &str) {
    if let Some(i) = d.pim.iface(name) {
        let _ = writeln!(s, " ip pim {}", i.mode.word());
        if i.dr_priority != 1 {
            let _ = writeln!(s, " ip pim dr-priority {}", i.dr_priority);
        }
    }
    for g in d.pim.joins.get(name).into_iter().flatten() {
        let _ = writeln!(s, " ip igmp join-group {g}");
    }
}
