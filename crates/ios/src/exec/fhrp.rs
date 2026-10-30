//! VRRP and GLBP on interfaces (`vrrp N …`, `glbp N …`), `fhrp version vrrp v3`, and the
//! `show vrrp` and `show glbp` views.

use std::fmt::Write as _;

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::fhrp::{Glbp, GlbpBalance, MAX_GROUPS, MAX_IFACES, Vrrp};
use netlabcraft_model::{Device, ifname};
use netlabcraft_sim::fhrp::{FhrpRoles, VrrpState};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// The interfaces the mode configures, with their IPv4 subnets.
fn targets(c: &Ctx) -> Result<Vec<(String, Option<Ipv4Net>)>, String> {
    let d = c.d()?;
    match &c.cli.mode {
        Mode::Svi(v) => Ok(vec![(format!("Vlan{v}"), d.svis.iter().find(|s| s.vlan == *v).and_then(|s| s.ip))]),
        Mode::Iface(ifs) => Ok(ifs
            .iter()
            .filter_map(|r| {
                let p = d.ports.get(r.port)?;
                Some(match r.sub {
                    Some(n) => (format!("{}.{n}", p.name), p.subif(n).and_then(|s| s.ip)),
                    None => (p.name.clone(), p.ip),
                })
            })
            .collect()),
        _ => Err("% Not in interface mode".into()),
    }
}

fn vrrp_group(text: &str) -> Result<u8, String> {
    text.trim().parse::<u8>().ok().filter(|n| *n >= 1).ok_or_else(|| "% The VRRP group is 1-255".to_string())
}

fn glbp_group(text: &str) -> Result<u16, String> {
    text.trim().parse::<u16>().ok().filter(|n| *n <= 1023).ok_or_else(|| "% The GLBP group is 0-1023".to_string())
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::FhrpVrrpV3 | Id::NoFhrpVrrpV3 => {
            c.dm()?.fhrp.vrrp_v3 = id == Id::FhrpVrrpV3;
            Ok(String::new())
        }
        Id::VrrpIp | Id::VrrpPriority | Id::VrrpPreempt | Id::NoVrrpPreempt | Id::NoVrrpPriority | Id::NoVrrp => {
            let group = vrrp_group(arg(0))?;
            let ip = if id == Id::VrrpIp { Some(parse_ip(arg(1)).map_err(|e| format!("% {e}"))?) } else { None };
            let priority = match id {
                Id::VrrpPriority => Some(arg(1).trim().parse::<u8>().ok().filter(|p| (1..=254).contains(p)).ok_or("% The VRRP priority is 1-254")?),
                Id::NoVrrpPriority => Some(100),
                _ => None,
            };
            let list = targets(c)?;
            let d = c.dm()?;
            for (name, net) in list {
                if id == Id::NoVrrp {
                    if let Some(v) = d.fhrp.vrrp.get_mut(&name) {
                        v.retain(|g| g.group != group);
                    }
                    d.fhrp.vrrp.retain(|_, v| !v.is_empty());
                    continue;
                }
                if let (Some(v), Some(n)) = (ip, net)
                    && !n.contains(v)
                {
                    return Err(format!("% {v} is not on {}", n.network()));
                }
                if !d.fhrp.vrrp.contains_key(&name) && d.fhrp.vrrp.len() >= MAX_IFACES {
                    return Err("% Too many interfaces with VRRP".into());
                }
                let groups = d.fhrp.vrrp.entry(name).or_default();
                if !groups.iter().any(|g| g.group == group) {
                    if groups.len() >= MAX_GROUPS {
                        return Err("% Too many VRRP groups on this interface".into());
                    }
                    groups.push(Vrrp::new(group));
                }
                if let Some(g) = groups.iter_mut().find(|g| g.group == group) {
                    if ip.is_some() {
                        g.ip = ip;
                    }
                    if let Some(p) = priority {
                        g.priority = p;
                    }
                    match id {
                        Id::VrrpPreempt => g.preempt = true,
                        Id::NoVrrpPreempt => g.preempt = false,
                        _ => {}
                    }
                }
            }
            Ok(String::new())
        }
        Id::GlbpIp | Id::GlbpPriority | Id::GlbpPreempt | Id::NoGlbpPreempt | Id::NoGlbpPriority | Id::GlbpBalancing | Id::NoGlbp => {
            let group = glbp_group(arg(0))?;
            let ip = if id == Id::GlbpIp { Some(parse_ip(arg(1)).map_err(|e| format!("% {e}"))?) } else { None };
            let priority = match id {
                Id::GlbpPriority => Some(arg(1).trim().parse::<u8>().ok().filter(|p| *p >= 1).ok_or("% The GLBP priority is 1-255")?),
                Id::NoGlbpPriority => Some(100),
                _ => None,
            };
            let balance = if id == Id::GlbpBalancing { Some(GlbpBalance::parse(arg(1)).ok_or("% `round-robin` or `host-dependent`")?) } else { None };
            let list = targets(c)?;
            let d = c.dm()?;
            for (name, net) in list {
                if id == Id::NoGlbp {
                    if let Some(v) = d.fhrp.glbp.get_mut(&name) {
                        v.retain(|g| g.group != group);
                    }
                    d.fhrp.glbp.retain(|_, v| !v.is_empty());
                    continue;
                }
                if let (Some(v), Some(n)) = (ip, net)
                    && !n.contains(v)
                {
                    return Err(format!("% {v} is not on {}", n.network()));
                }
                if !d.fhrp.glbp.contains_key(&name) && d.fhrp.glbp.len() >= MAX_IFACES {
                    return Err("% Too many interfaces with GLBP".into());
                }
                let groups = d.fhrp.glbp.entry(name).or_default();
                if !groups.iter().any(|g| g.group == group) {
                    if groups.len() >= MAX_GROUPS {
                        return Err("% Too many GLBP groups on this interface".into());
                    }
                    groups.push(Glbp::new(group));
                }
                if let Some(g) = groups.iter_mut().find(|g| g.group == group) {
                    if ip.is_some() {
                        g.ip = ip;
                    }
                    if let Some(p) = priority {
                        g.priority = p;
                    }
                    if let Some(b) = balance {
                        g.balance = b;
                    }
                    match id {
                        Id::GlbpPreempt => g.preempt = true,
                        Id::NoGlbpPreempt => g.preempt = false,
                        _ => {}
                    }
                }
            }
            Ok(String::new())
        }
        Id::ShowVrrp => {
            let roles = c.sim.fhrp_roles(c.t);
            Ok(show_vrrp(c.d()?, &roles, arg(0).eq_ignore_ascii_case("brief")))
        }
        Id::ShowGlbp => {
            let roles = c.sim.fhrp_roles(c.t);
            Ok(show_glbp(c.d()?, &roles, arg(0).eq_ignore_ascii_case("brief")))
        }
        _ => Err("% Not a VRRP or GLBP command".into()),
    }
}

fn show_vrrp(d: &Device, roles: &FhrpRoles, brief: bool) -> String {
    if d.fhrp.vrrp.is_empty() {
        return "No VRRP group is configured here.".into();
    }
    let list = roles.vrrp.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        return "VRRP groups are configured, but none is running (interface down, or no virtual address in the group).".into();
    }
    let version = if d.fhrp.vrrp_v3 { 3 } else { 2 };
    let mut s = String::new();
    if brief {
        let _ = writeln!(s, "{:<12} {:<5} {:<4} {:<8} {:<7} {:<16} Virtual address", "Interface", "Group", "Pri", "Preempt", "State", "Master");
    }
    for r in &list {
        let master = if r.state == VrrpState::Master { "this router".to_string() } else { r.master.map_or("none".into(), |m| m.to_string()) };
        if brief {
            let _ = writeln!(
                s,
                "{:<12} {:<5} {:<4} {:<8} {:<7} {:<16} {}",
                ifname::short(&r.iface),
                r.group,
                r.priority,
                if r.preempt || r.owner { "yes" } else { "no" },
                r.state.name(),
                master,
                r.virtual_ip
            );
            continue;
        }
        let owner = if r.owner { ", address owner" } else { "" };
        let _ = writeln!(
            s,
            "Group {} on {}: {} (priority {}{owner}{})",
            r.group,
            r.iface,
            r.state.name(),
            r.priority,
            if r.preempt { ", preempt" } else { "" }
        );
        let _ = writeln!(s, "  VRRP version {version}, virtual address {}, virtual MAC {}", r.virtual_ip, r.virtual_mac().dotted());
        let _ = writeln!(s, "  Master: {master}; advertisements every 1 s");
    }
    s.trim_end().to_string()
}

fn show_glbp(d: &Device, roles: &FhrpRoles, brief: bool) -> String {
    if d.fhrp.glbp.is_empty() {
        return "No GLBP group is configured here.".into();
    }
    let list = roles.glbp.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        return "GLBP groups are configured, but none is running (interface down, or no virtual address in the group).".into();
    }
    let mut s = String::new();
    if brief {
        let _ =
            writeln!(s, "{:<12} {:<5} {:<4} {:<9} {:<10} {:<16} Virtual address", "Interface", "Group", "Pri", "Gateway", "Forwarder", "Virtual MAC");
    }
    for r in &list {
        let own_mac = r.forwarder.and_then(|n| r.forwarders.iter().find(|f| f.0 == n)).map_or("-".to_string(), |f| f.2.dotted());
        if brief {
            let _ = writeln!(
                s,
                "{:<12} {:<5} {:<4} {:<9} {:<10} {:<16} {}",
                ifname::short(&r.iface),
                r.group,
                r.priority,
                if r.avg { "active" } else { "standby" },
                r.forwarder.map_or("-".to_string(), |n| n.to_string()),
                own_mac,
                r.virtual_ip
            );
            continue;
        }
        let avg = if r.avg { "this router".to_string() } else { r.avg_address.map_or("none".into(), |a| a.to_string()) };
        let _ = writeln!(
            s,
            "Group {} on {}: {} (priority {}{})",
            r.group,
            r.iface,
            if r.avg { "active gateway" } else { "standby gateway" },
            r.priority,
            if r.preempt { ", preempt" } else { "" }
        );
        let _ = writeln!(s, "  Virtual address {}; active virtual gateway: {avg}; load balancing {}", r.virtual_ip, r.balance.name());
        for (n, addr, mac) in &r.forwarders {
            let mine = if Some(*n) == r.forwarder { " (this router)" } else { "" };
            let _ = writeln!(s, "  Forwarder {n}: {} on {addr}{mine}", mac.dotted());
        }
    }
    s.trim_end().to_string()
}

/// Interface lines for `show running-config`: the VRRP and GLBP groups on `iface`.
pub(crate) fn iface_lines(s: &mut String, d: &Device, iface: &str) {
    for g in d.fhrp.vrrp_on(iface) {
        if let Some(ip) = g.ip {
            let _ = writeln!(s, " vrrp {} ip {ip}", g.group);
        }
        if g.priority != 100 {
            let _ = writeln!(s, " vrrp {} priority {}", g.group, g.priority);
        }
        if !g.preempt {
            let _ = writeln!(s, " no vrrp {} preempt", g.group);
        }
    }
    for g in d.fhrp.glbp_on(iface) {
        if let Some(ip) = g.ip {
            let _ = writeln!(s, " glbp {} ip {ip}", g.group);
        }
        if g.priority != 100 {
            let _ = writeln!(s, " glbp {} priority {}", g.group, g.priority);
        }
        if g.preempt {
            let _ = writeln!(s, " glbp {} preempt", g.group);
        }
        if g.balance != GlbpBalance::default() {
            let _ = writeln!(s, " glbp {} load-balancing {}", g.group, g.balance.name());
        }
    }
}

/// Global lines for `show running-config`.
pub(crate) fn global_lines(s: &mut String, d: &Device) {
    if d.fhrp.vrrp_v3 {
        let _ = writeln!(s, "fhrp version vrrp v3");
    }
}
