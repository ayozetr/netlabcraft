//! Switching and routing leftovers: 802.1p CoS (`mls qos map cos-dscp`, `mls qos cos`,
//! `switchport priority extend cos`, `show mls qos maps`) and EIGRP summary addresses
//! (`ip summary-address eigrp`, `ipv6 summary-address eigrp`), RIP's timers (`timers basic`)
//! RIPng (`ipv6 router rip`, `ipv6 rip NAME enable`, `show ipv6 rip`) and VTP pruning (`vtp
//! pruning`, `switchport trunk pruning vlan`).

use std::fmt::Write as _;

use netlabcraft_core::{Ipv4Net, Ipv6Net, VlanId, VlanSet, parse_ip};
use netlabcraft_model::Device;
use netlabcraft_model::l2sec::DEFAULT_COS_DSCP;
use netlabcraft_model::routing::MAX_SUMMARIES;

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn number(text: &str, lo: u32, hi: u32, what: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|n| (lo..=hi).contains(n)).ok_or_else(|| format!("% {what} is a number from {lo} to {hi}"))
}

fn cos(text: &str) -> Result<u8, String> {
    number(text, 0, 7, "A CoS value").map(|n| u8::try_from(n).unwrap_or(0))
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    use Id::*;
    match id {
        SrCosDscpMap => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            if words.len() != 8 {
                return Err("% Give eight DSCP values, one for each CoS from 0 to 7".into());
            }
            let mut map = [0u8; 8];
            for (slot, w) in map.iter_mut().zip(&words) {
                *slot = netlabcraft_sim::qos::parse_dscp(w).ok_or_else(|| format!("% `{w}` is not a DSCP value (0-63 or a name such as ef)"))?;
            }
            let d = c.dm()?;
            d.l2.cos_dscp = (map != DEFAULT_COS_DSCP).then_some(map);
            Ok(if d.l2.mls_qos { String::new() } else { "Saved; the map acts once `mls qos` turns QoS on.".into() })
        }
        SrNoCosDscpMap => {
            c.dm()?.l2.cos_dscp = None;
            Ok(String::new())
        }
        SrMlsQosCos | SrNoMlsQosCos => {
            let v = if id == SrMlsQosCos { Some(cos(arg(0))?) } else { None };
            c.each_if(|p, _| {
                p.l2.qos_cos = v.filter(|n| *n != 0);
                Ok(())
            })
        }
        SrPriorityExtend | SrNoPriorityExtend => {
            let v = if id == SrPriorityExtend { Some(cos(arg(0))?) } else { None };
            c.each_if(|p, _| {
                p.l2.priority_extend = v;
                Ok(())
            })?;
            let d = c.d()?;
            Ok(if v.is_some() && !d.l2.mls_qos {
                "Saved; the PC behind the phone gets this CoS once `mls qos` turns QoS on.".into()
            } else {
                String::new()
            })
        }
        SrShowMlsQosMaps => show_maps(c.d()?, arg(0)),
        SrVtpPruning | SrNoVtpPruning => {
            let on = id == SrVtpPruning;
            let d = c.dm()?;
            match d.vtp.mode {
                netlabcraft_model::VtpMode::Client => return Err("% VTP pruning is set on a server; clients take it from the domain".into()),
                netlabcraft_model::VtpMode::Transparent => {
                    return Err("% A transparent switch doesn't take part in VTP pruning (it passes VTP messages on)".into());
                }
                netlabcraft_model::VtpMode::Server => {}
            }
            if d.vtp.pruning != on {
                d.vtp.pruning = on;
                // The change travels to the domain with the next revision.
                d.vtp.revision = d.vtp.revision.saturating_add(1);
            }
            Ok(if on && d.vtp.domain.is_empty() { "Saved; pruning acts once the switch is in a VTP domain.".into() } else { String::new() })
        }
        SrTrunkPruning | SrNoTrunkPruning => {
            let set = if id == SrTrunkPruning { Some(prune_list(c, arg(0))?) } else { None };
            c.each_if(|p, _| {
                p.l2.prune_eligible = match &set {
                    None => None,
                    Some(f) => Some(f(p.l2.prune_eligible.clone())),
                };
                Ok(())
            })
        }
        SrRipTimers | SrNoRipTimers => {
            if c.cli.mode == Mode::Rip6 {
                return Err("% RIPng here keeps RIP's usual timers (30 s updates)".into());
            }
            let timers = if id == SrRipTimers {
                let n = |i: usize, lo: u32| number(arg(i), lo, 65_535, "Each time");
                let v = [n(0, 1)?, n(1, 1)?, n(2, 0)?, n(3, 1)?];
                if v[1] < v[0] || v[3] < v[1] {
                    return Err("% The invalid time must be at least the update time, and the flush time at least the invalid time".into());
                }
                Some(v).filter(|v| *v != netlabcraft_model::RIP_TIMERS)
            } else {
                None
            };
            c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?.timers = timers;
            Ok(String::new())
        }
        SrIpv6RouterRip | SrNoIpv6RouterRip => {
            let name = rip6_name(arg(0))?;
            let d = c.dm()?;
            if id == SrNoIpv6RouterRip {
                if d.rip6.as_ref().is_none_or(|r| r.name != name) {
                    return Err(format!("% No RIPng process {name}"));
                }
                d.rip6 = None;
                return Ok(String::new());
            }
            match &d.rip6 {
                Some(r) if r.name != name => {
                    return Err(format!("% RIPng already runs here as {}; NetLabCraft runs one RIPng process per router", r.name));
                }
                Some(_) => {}
                None => d.rip6 = Some(netlabcraft_model::routing::Rip6 { name }),
            }
            let note = if d.ipv6_routing { String::new() } else { "IPv6 routing is off: RIPng waits for `ipv6 unicast-routing`.".into() };
            c.cli.mode = Mode::Rip6;
            Ok(note)
        }
        SrIpv6RipEnable | SrNoIpv6RipEnable => {
            let name = rip6_name(arg(0))?;
            let add = id == SrIpv6RipEnable;
            c.each_v6(|v6, _, _| {
                if add {
                    v6.rip = Some(name.clone());
                } else if v6.rip.as_ref() == Some(&name) {
                    v6.rip = None;
                }
                Ok(())
            })?;
            let d = c.d()?;
            Ok(if add && d.rip6.as_ref().is_none_or(|r| r.name != name) {
                format!("Saved; the interface joins once `ipv6 router rip {name}` runs here.")
            } else {
                String::new()
            })
        }
        SrShowIpv6Rip | SrShowIpv6RipDatabase => {
            let st = c.sim.rip6_state(c.t);
            Ok(show_rip6(c.t, c.d()?, &st, id == SrShowIpv6RipDatabase))
        }
        SrIpSummaryEigrp | SrNoIpSummaryEigrp | SrIpv6SummaryEigrp | SrNoIpv6SummaryEigrp => {
            let asn = number(arg(0), 1, 65_535, "The autonomous system").map(|n| u16::try_from(n).unwrap_or(1))?;
            let v4 = matches!(id, SrIpSummaryEigrp | SrNoIpSummaryEigrp);
            let add = matches!(id, SrIpSummaryEigrp | SrIpv6SummaryEigrp);
            let (net4, net6) = if v4 {
                let a = parse_ip(arg(1)).map_err(|e| format!("% {e}"))?;
                let m = parse_ip(arg(2)).map_err(|e| format!("% {e}"))?;
                let n = Ipv4Net::with_mask(a, m).map_err(|e| format!("% {e}"))?;
                (Some(n.network()), None)
            } else {
                let n: Ipv6Net = arg(1).trim().parse().map_err(|_| format!("% `{}` is not an IPv6 prefix (X:X::X/N)", arg(1).trim()))?;
                (None, Some(n.network()))
            };
            let names = c.mode_iface_names()?;
            let d = c.dm()?;
            for name in names {
                let e = d.eigrp_summary.entry(name.clone()).or_default();
                if let Some(n) = net4 {
                    e.v4.retain(|x| *x != (asn, n));
                    if add {
                        if e.v4.len() >= MAX_SUMMARIES {
                            return Err("% Too many summary addresses on this interface".into());
                        }
                        e.v4.push((asn, n));
                    }
                }
                if let Some(n) = net6 {
                    e.v6.retain(|x| *x != (asn, n));
                    if add {
                        if e.v6.len() >= MAX_SUMMARIES {
                            return Err("% Too many summary addresses on this interface".into());
                        }
                        e.v6.push((asn, n));
                    }
                }
                if e.is_empty() {
                    d.eigrp_summary.remove(&name);
                }
            }
            let d = c.d()?;
            let running = if v4 { d.eigrp.as_ref().is_some_and(|e| e.asn == asn) } else { d.eigrp6.as_ref().is_some_and(|e| e.asn == asn) };
            Ok(if add && !running { format!("Saved; it acts once EIGRP for AS {asn} runs here.") } else { String::new() })
        }
        _ => Err("% Not handled here".into()),
    }
}

/// The eligible VLANs `switchport trunk pruning vlan …` asks for, from the current ones.
#[allow(clippy::type_complexity)]
fn prune_list(_c: &Ctx, text: &str) -> Result<Box<dyn Fn(Option<VlanSet>) -> VlanSet>, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let parse = |w: Option<&&str>| VlanSet::parse_list(w.copied().unwrap_or("")).map_err(|e| format!("% {e}"));
    let default = || {
        let mut s = VlanSet::none();
        for v in 2..=1001u16 {
            if let Some(v) = VlanId::new(v) {
                s.insert(v);
            }
        }
        s
    };
    let cur = move |c: Option<VlanSet>| c.unwrap_or_else(default);
    Ok(match words.first().map(|w| w.to_ascii_lowercase()).as_deref() {
        Some("none") => Box::new(|_| VlanSet::none()),
        Some("add") => {
            let add = parse(words.get(1))?;
            Box::new(move |c| {
                let mut s = cur(c);
                s.union(&add);
                s
            })
        }
        Some("remove") => {
            let rm = parse(words.get(1))?;
            Box::new(move |c| {
                let mut s = cur(c);
                s.subtract(&rm);
                s
            })
        }
        Some("except") => {
            let ex = parse(words.get(1))?;
            Box::new(move |_| {
                let mut s = default();
                s.subtract(&ex);
                s
            })
        }
        Some(_) => {
            let only = parse(words.first())?;
            Box::new(move |_| only.clone())
        }
        None => return Err("% Give the VLANs, or add, remove, except or none".into()),
    })
}

fn rip6_name(text: &str) -> Result<String, String> {
    let n = text.trim();
    if n.is_empty()
        || n.chars().count() > netlabcraft_model::routing::MAX_RIP6_NAME
        || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("% A RIPng process name is 1-{} letters, digits, - or _", netlabcraft_model::routing::MAX_RIP6_NAME));
    }
    Ok(n.to_string())
}

/// `show ipv6 rip` and `show ipv6 rip database`.
fn show_rip6(t: &netlabcraft_model::Topology, d: &Device, st: &netlabcraft_sim::rip6::Rip6State, database: bool) -> String {
    use netlabcraft_model::ifname;
    let Some(r) = &d.rip6 else { return "RIPng is not running on this router.".into() };
    if !d.ipv6_routing {
        return format!("RIPng process {} waits for `ipv6 unicast-routing`.", r.name);
    }
    let ifs = st.interfaces.get(&d.id).cloned().unwrap_or_default();
    let mut s = String::new();
    if database {
        let _ = writeln!(s, "RIPng process {} database", r.name);
        let _ = writeln!(s, "{:<28} {:<7} {:<28} Interface", "Prefix", "Metric", "Next hop");
        for (name, nets) in &ifs {
            for n in nets {
                let _ = writeln!(s, "{:<28} {:<7} {:<28} {}", n.to_string(), 1, "directly", ifname::short(name));
            }
        }
        for x in st.routes.get(&d.id).into_iter().flatten() {
            let _ = writeln!(s, "{:<28} {:<7} {:<28} {}", x.net.to_string(), x.metric, x.via.to_string(), ifname::short(&x.iface));
        }
        return s.trim_end().to_string();
    }
    let _ = writeln!(s, "RIPng process {}: UDP port 521, group FF02::9, updates every 30 s, administrative distance 120", r.name);
    let names: Vec<String> = ifs.iter().map(|(n, _)| ifname::short(n)).collect();
    let _ = writeln!(
        s,
        "  Interfaces: {}",
        if names.is_empty() { "none (`ipv6 rip NAME enable` on an interface that is up)".to_string() } else { names.join(", ") }
    );
    let heard: Vec<String> = st
        .neighbors
        .get(&d.id)
        .into_iter()
        .flatten()
        .map(|(dev, a, i)| format!("{a} on {} ({})", ifname::short(i), t.device(*dev).map_or("?", |x| x.name.as_str())))
        .collect();
    let _ = write!(s, "  Neighbours: {}", if heard.is_empty() { "none".to_string() } else { heard.join(", ") });
    s
}

/// The `ipv6 router rip` block of the running configuration.
pub(crate) fn rip6_running_config(s: &mut String, d: &Device) {
    if let Some(r) = &d.rip6 {
        let _ = writeln!(
            s,
            "ipv6 router rip {}
!",
            r.name
        );
    }
}

/// `show mls qos maps [cos-dscp|dscp-cos]`.
fn show_maps(d: &Device, which: &str) -> R {
    let which = which.trim().to_ascii_lowercase();
    let mut s = String::new();
    if which.is_empty() || (which.starts_with('c') && "cos-dscp".starts_with(which.as_str())) {
        let _ = writeln!(s, "CoS to DSCP (ports that trust CoS){}", if d.l2.cos_dscp.is_some() { ", configured" } else { ", default" });
        let _ = writeln!(s, "  CoS:  {}", (0..8u8).map(|c| format!("{c:>3}")).collect::<String>());
        let _ = writeln!(s, "  DSCP: {}", (0..8u8).map(|c| format!("{:>3}", d.l2.cos_to_dscp(c))).collect::<String>());
    }
    if which.is_empty() || (which.starts_with('d') && "dscp-cos".starts_with(which.as_str())) {
        let _ = writeln!(s, "DSCP to CoS (frames leaving tagged; the DSCP's top three bits)");
        for cos in 0..8u8 {
            let lo = cos * 8;
            let _ = writeln!(s, "  DSCP {lo:>2}-{:<2} -> CoS {cos}", lo + 7);
        }
    }
    if s.is_empty() {
        return Err("% `cos-dscp` or `dscp-cos`".into());
    }
    if !d.l2.mls_qos {
        s.push_str("(QoS is disabled: the maps wait for `mls qos`.)");
    }
    Ok(s.trim_end().to_string())
}

/// Global lines of this module for `show running-config` (after `mls qos`).
pub(crate) fn global_lines(s: &mut String, d: &Device) {
    if let Some(m) = d.l2.cos_dscp {
        let _ = writeln!(s, "mls qos map cos-dscp {}", m.iter().map(u8::to_string).collect::<Vec<_>>().join(" "));
    }
}

/// `ip summary-address eigrp` and `ipv6 summary-address eigrp` lines of an interface.
pub(crate) fn eigrp_summary_lines(s: &mut String, d: &Device, iface: &str) {
    let Some(e) = d.eigrp_summary.get(iface) else { return };
    for (asn, n) in &e.v4 {
        let _ = writeln!(s, " ip summary-address eigrp {asn} {} {}", n.addr, n.mask());
    }
    for (asn, n) in &e.v6 {
        let _ = writeln!(s, " ipv6 summary-address eigrp {asn} {n}");
    }
}

/// Interface lines of this module for a switch port.
pub(crate) fn port_lines(s: &mut String, p: &netlabcraft_model::Port) {
    if let Some(n) = p.l2.qos_cos {
        let _ = writeln!(s, " mls qos cos {n}");
    }
    if let Some(n) = p.l2.priority_extend {
        let _ = writeln!(s, " switchport priority extend cos {n}");
    }
    if let Some(set) = &p.l2.prune_eligible {
        if set.is_empty() {
            let _ = writeln!(s, " switchport trunk pruning vlan none");
        } else {
            let _ = writeln!(s, " switchport trunk pruning vlan {set}");
        }
    }
}
