//! Switching and routing leftovers: 802.1p CoS (`mls qos map cos-dscp`, `mls qos cos`,
//! `switchport priority extend cos`, `show mls qos maps`) and EIGRP summary addresses
//! (`ip summary-address eigrp`, `ipv6 summary-address eigrp`).

use std::fmt::Write as _;

use netlabcraft_core::{Ipv4Net, Ipv6Net, parse_ip};
use netlabcraft_model::Device;
use netlabcraft_model::l2sec::DEFAULT_COS_DSCP;
use netlabcraft_model::routing::MAX_SUMMARIES;

use super::{Ctx, R};
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
}
