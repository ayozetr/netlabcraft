//! IP source guard: `ip verify source [port-security]` on a port, `ip source binding` for hosts
//! without DHCP, `show ip verify source` and `show ip source binding`. The check itself is in
//! `netlabcraft_sim::l2sec`: an IPv4 frame from an untrusted port of a snooped VLAN passes only
//! when a binding gives its source address (and with `port-security`, its MAC address) to the
//! port.

use std::fmt::Write as _;

use netlabcraft_core::{MacAddr, parse_ip};
use netlabcraft_model::l2sec::{MAX_STATIC_BINDINGS, SourceGuard, StaticBinding};
use netlabcraft_model::{Device, ifname};
use netlabcraft_sim::Sim;

use super::{Ctx, R};
use crate::grammar::Id;

fn short(d: &Device, port: usize) -> String {
    d.ports.get(port).map_or_else(|| "?".to_string(), |p| ifname::short(&p.name))
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::IpSgOn | Id::IpSgPortSecurity | Id::IpSgOff => {
            let mode = match id {
                Id::IpSgOn => Some(SourceGuard::Ip),
                Id::IpSgPortSecurity => Some(SourceGuard::IpMac),
                _ => None,
            };
            c.each_if(|p, _| {
                p.l2.source_guard = mode;
                Ok(())
            })?;
            let d = c.d()?;
            Ok(if mode.is_some() && !d.l2.snooping {
                "Saved; IP source guard acts once DHCP snooping is on (`ip dhcp snooping`).".into()
            } else {
                String::new()
            })
        }
        Id::IpSgBinding | Id::IpSgNoBinding => {
            let mac: MacAddr = arg(0).parse().map_err(|_| "% Give the MAC address as H.H.H".to_string())?;
            let vlan: u16 = arg(1).trim().parse().ok().filter(|v| (1..=4094).contains(v)).ok_or("% The VLAN is a number from 1 to 4094")?;
            if id == Id::IpSgNoBinding {
                let l = &mut c.dm()?.l2.static_bindings;
                let before = l.len();
                l.retain(|b| !(b.mac == mac && b.vlan == vlan));
                return if l.len() == before { Err("% No such binding".into()) } else { Ok(String::new()) };
            }
            let ip = parse_ip(arg(2)).map_err(|e| format!("% {e}"))?;
            if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
                return Err("% The binding's address is a unicast address".into());
            }
            let r = c.iface_ref(arg(3).trim_start_matches("interface").trim())?;
            if !c.d()?.is_switchport(r.port) {
                return Err("% That is not a switch port".into());
            }
            let l = &mut c.dm()?.l2.static_bindings;
            l.retain(|b| !(b.mac == mac && b.vlan == vlan));
            if l.len() >= MAX_STATIC_BINDINGS {
                return Err(format!("% At most {MAX_STATIC_BINDINGS} bindings typed by hand"));
            }
            l.push(StaticBinding { mac, vlan, ip, port: r.port });
            Ok(String::new())
        }
        Id::IpSgShowBinding => Ok(show_bindings(c.sim, c.d()?)),
        Id::IpSgShowVerify => Ok(show_verify(c.sim, c.d()?)),
        _ => Err("% That command is not implemented".into()),
    }
}

/// Every binding: learned by snooping, or typed.
fn show_bindings(sim: &Sim, d: &Device) -> String {
    let now = sim.now_us();
    let mut s = format!("{:<15} {:<16} {:<10} {:<15} {:<5} Interface\n", "MAC address", "IP address", "Lease (s)", "Type", "VLAN");
    let mut n = 0;
    for b in sim.snooping_bindings(d.id) {
        let left = u64::from(b.lease_s).saturating_sub(now.saturating_sub(b.at_us) / 1_000_000);
        let _ = writeln!(
            s,
            "{:<15} {:<16} {:<10} {:<15} {:<5} {}",
            b.mac.dotted(),
            b.ip.to_string(),
            left,
            "dhcp-snooping",
            b.vlan.to_string(),
            short(d, b.port)
        );
        n += 1;
    }
    for b in &d.l2.static_bindings {
        let _ = writeln!(s, "{:<15} {:<16} {:<10} {:<15} {:<5} {}", b.mac.dotted(), b.ip.to_string(), "infinite", "static", b.vlan, short(d, b.port));
        n += 1;
    }
    let _ = write!(s, "{n} binding{}", if n == 1 { "" } else { "s" });
    s
}

/// `show ip verify source`: for each guarded port, what it lets through.
fn show_verify(sim: &Sim, d: &Device) -> String {
    let guarded: Vec<usize> = (0..d.ports.len()).filter(|i| d.ports.get(*i).is_some_and(|p| p.l2.source_guard.is_some())).collect();
    if guarded.is_empty() {
        return "No port has IP source guard (`ip verify source` on a port).".into();
    }
    let learned = sim.snooping_bindings(d.id);
    let mut s = format!("{:<10} {:<12} {:<12} {:<16} {:<15} VLAN\n", "Interface", "Filter-type", "Filter-mode", "IP address", "MAC address");
    for i in guarded {
        let Some(p) = d.ports.get(i) else { continue };
        let mode = p.l2.source_guard.unwrap_or(SourceGuard::Ip);
        let mut rows: Vec<(String, String, u16)> =
            learned.iter().filter(|b| b.port == i).map(|b| (b.ip.to_string(), b.mac.dotted(), b.vlan.get())).collect();
        rows.extend(d.l2.static_bindings.iter().filter(|b| b.port == i).map(|b| (b.ip.to_string(), b.mac.dotted(), b.vlan)));
        let name = short(d, i);
        if rows.is_empty() {
            let _ = writeln!(s, "{name:<10} {:<12} {:<12} {:<16} {:<15} {}", mode.name(), "active", "deny-all", "", p.switchport.access_vlan.get());
        }
        for (ip, mac, vlan) in rows {
            let mac = if mode == SourceGuard::IpMac { mac } else { String::new() };
            let _ = writeln!(s, "{name:<10} {:<12} {:<12} {ip:<16} {mac:<15} {vlan}", mode.name(), "active");
        }
    }
    s.trim_end().to_string()
}

/// The global lines of a running configuration: bindings typed by hand.
pub(crate) fn global_lines(s: &mut String, d: &Device) {
    for b in &d.l2.static_bindings {
        let _ = writeln!(s, "ip source binding {} vlan {} {} interface {}", b.mac.dotted(), b.vlan, b.ip, short(d, b.port));
    }
}
