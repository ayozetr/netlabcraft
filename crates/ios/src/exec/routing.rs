//! Routing commands kept apart from the big dispatcher: loopback interfaces
//! (`interface Loopback N`).

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::MAX_DESCRIPTION;
use netlabcraft_model::routing::loopback_number;

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// The loopback number an interface name gives, if it names a loopback.
pub(super) fn loopback(name: &str) -> Option<u32> {
    loopback_number(name)
}

fn loopback_ok(c: &Ctx) -> Result<(), String> {
    if !netlabcraft_sim::loopback::has_loopbacks(c.d()?) {
        return Err("% Loopback interfaces are for routers and switches".into());
    }
    Ok(())
}

/// `interface Loopback N`: created on first use, up at once.
pub(super) fn enter_loopback(c: &mut Ctx, n: u32) -> R {
    loopback_ok(c)?;
    c.dm()?.loopback_mut(n).ok_or("% Too many loopback interfaces")?;
    c.cli.mode = Mode::Loopback(n);
    Ok(String::new())
}

/// `no interface Loopback N`: gone, with its per-interface routing settings.
pub(super) fn delete_loopback(c: &mut Ctx, n: u32) -> R {
    let d = c.dm()?;
    let before = d.loopbacks.len();
    d.loopbacks.retain(|l| l.number != n);
    if d.loopbacks.len() == before {
        return Err(format!("% There is no interface Loopback{n}"));
    }
    let name = format!("Loopback{n}");
    d.ospf_if.remove(&name);
    d.eigrp_if.remove(&name);
    Ok(String::new())
}

/// The other interface whose subnet overlaps `net`, if any (`except` is the one being set).
pub(super) fn overlap(d: &netlabcraft_model::Device, net: Ipv4Net, except: &str) -> Option<String> {
    let mut all: Vec<(String, Ipv4Net)> = Vec::new();
    for p in &d.ports {
        if let Some(ip) = p.ip {
            all.push((p.name.clone(), ip));
        }
        for s in &p.subifs {
            if let Some(ip) = s.ip {
                all.push((format!("{}.{}", p.name, s.number), ip));
            }
        }
    }
    all.extend(d.svis.iter().filter_map(|s| s.ip.map(|ip| (format!("Vlan{}", s.vlan), ip))));
    all.extend(d.tunnels.iter().filter_map(|t| t.ip.map(|ip| (t.name(), ip))));
    all.extend(d.loopbacks.iter().filter_map(|l| l.ip.map(|ip| (l.name(), ip))));
    all.into_iter().find(|(n, ip)| n != except && ip.overlaps(net)).map(|(n, ip)| format!("% {} overlaps {} on {n}", net.network(), ip.network()))
}

/// `ip address`, `shutdown`, `description` and their `no` forms on a loopback.
pub(super) fn loopback_iface(c: &mut Ctx, id: Id, a0: &str, a1: &str) -> R {
    let Mode::Loopback(n) = c.cli.mode else { return Err("% Not in a loopback interface".into()) };
    let name = format!("Loopback{n}");
    let ip = if id == Id::IpAddress {
        let net = Ipv4Net::with_mask(parse_ip(a0).map_err(|e| format!("% {e}"))?, parse_ip(a1).map_err(|e| format!("% {e}"))?)
            .map_err(|e| format!("% {e}"))?;
        if net.is_network_or_broadcast() {
            return Err(format!("% {} is the network or broadcast address of {}", net.addr, net.network()));
        }
        if let Some(why) = overlap(c.d()?, net, &name) {
            return Err(why);
        }
        Some(net)
    } else {
        None
    };
    let text: String = a0.chars().take(MAX_DESCRIPTION).collect();
    let lo = c.dm()?.loopback_mut(n).ok_or("% Too many loopback interfaces")?;
    match id {
        Id::IpAddress => lo.ip = ip,
        Id::NoIpAddress => lo.ip = None,
        Id::Shutdown => lo.shutdown = true,
        Id::NoShutdown => lo.shutdown = false,
        Id::Description => lo.description = text,
        _ => lo.description.clear(),
    }
    Ok(match id {
        Id::NoShutdown => format!("{name} is now up"),
        Id::Shutdown => format!("{name} is now administratively down"),
        _ => String::new(),
    })
}

// ---- what the show commands print about loopbacks ----

/// The loopbacks' blocks of the running configuration.
pub(crate) fn loopback_config(s: &mut String, d: &netlabcraft_model::Device) {
    use std::fmt::Write as _;
    for l in &d.loopbacks {
        let _ = writeln!(s, "interface {}", l.name());
        if !l.description.is_empty() {
            let _ = writeln!(s, " description {}", l.description);
        }
        match l.ip {
            Some(ip) => {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            None => s.push_str(" no ip address\n"),
        }
        crate::render::ipv6_lines(s, &l.ipv6);
        crate::render::ospf_if_lines(s, d, &l.name());
        if l.shutdown {
            s.push_str(" shutdown\n");
        }
        s.push_str("!\n");
    }
}

/// The loopbacks' rows of `show ip interface brief` (same columns as the other rows).
pub(crate) fn loopback_brief(s: &mut String, d: &netlabcraft_model::Device) {
    use std::fmt::Write as _;
    for l in &d.loopbacks {
        let addr = l.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let state = if l.shutdown { "down" } else { "up" };
        let _ = writeln!(s, "{:<24} {:<18} {:<7} {}", l.name(), addr, state, state);
    }
}

/// One loopback's `show interfaces` block.
pub(crate) fn loopback_block(l: &netlabcraft_model::routing::Loopback) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let state = if l.shutdown { "administratively down, line protocol down" } else { "up, line protocol up" };
    let _ = writeln!(s, "{} is {state}", l.name());
    if !l.description.is_empty() {
        let _ = writeln!(s, "  Description: {}", l.description);
    }
    match l.ip {
        Some(ip) => {
            let _ = writeln!(s, "  Internet address {ip}");
        }
        None => s.push_str("  No address\n"),
    }
    s.push_str("  Virtual interface: no cable, always up unless shut down; nothing is sent out of it\n");
    s
}

/// Every loopback's `show interfaces` block.
pub(crate) fn loopback_blocks(d: &netlabcraft_model::Device) -> String {
    d.loopbacks.iter().map(loopback_block).collect()
}

/// `show interfaces loopback N`.
pub(super) fn loopback_interface(d: &netlabcraft_model::Device, n: u32) -> R {
    let l = d.loopbacks.iter().find(|x| x.number == n).ok_or_else(|| format!("% There is no interface Loopback{n}"))?;
    Ok(loopback_block(l).trim_end().to_string())
}
