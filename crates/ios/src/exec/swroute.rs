//! Switching and routing leftovers: 802.1p CoS (`mls qos map cos-dscp`, `mls qos cos`,
//! `switchport priority extend cos`, `show mls qos maps`).

use std::fmt::Write as _;

use netlabcraft_model::Device;
use netlabcraft_model::l2sec::DEFAULT_COS_DSCP;

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

/// Interface lines of this module for a switch port.
pub(crate) fn port_lines(s: &mut String, p: &netlabcraft_model::Port) {
    if let Some(n) = p.l2.qos_cos {
        let _ = writeln!(s, " mls qos cos {n}");
    }
    if let Some(n) = p.l2.priority_extend {
        let _ = writeln!(s, " switchport priority extend cos {n}");
    }
}
