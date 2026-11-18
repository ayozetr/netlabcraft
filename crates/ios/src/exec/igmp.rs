//! IGMP snooping on switches: `ip igmp snooping [vlan N] [querier [address A]]`,
//! `show ip igmp snooping [groups|mrouter|querier]` and `clear ip igmp snooping`.

use std::fmt::Write as _;

use netlabcraft_core::{VlanSet, parse_ip};
use netlabcraft_model::{Device, ifname};

use super::{Ctx, R};
use crate::grammar::Id;

fn short(d: &Device, port: usize) -> String {
    d.ports.get(port).map_or_else(|| "?".to_string(), |p| ifname::short(&p.name))
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::IgmpSnooping | Id::NoIgmpSnooping => {
            c.dm()?.igmp.on = id == Id::IgmpSnooping;
            Ok(String::new())
        }
        Id::IgmpSnoopingVlan | Id::NoIgmpSnoopingVlan => {
            let set = VlanSet::parse_list(arg(0)).map_err(|e| format!("% {e}"))?;
            let d = c.dm()?;
            for v in set.iter() {
                if id == Id::IgmpSnoopingVlan {
                    d.igmp.vlan_off.remove(&v.get());
                } else {
                    d.igmp.vlan_off.insert(v.get());
                }
            }
            Ok(if id == Id::IgmpSnoopingVlan && !d.igmp.on {
                "Saved; snooping acts once `ip igmp snooping` turns it on.".into()
            } else {
                String::new()
            })
        }
        Id::IgmpQuerier | Id::NoIgmpQuerier => {
            c.dm()?.igmp.querier = id == Id::IgmpQuerier;
            Ok(String::new())
        }
        Id::IgmpQuerierAddress => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            if a.is_unspecified() || a.is_multicast() || a.is_broadcast() {
                return Err("% The querier address is a unicast address".into());
            }
            c.dm()?.igmp.querier_address = Some(a);
            Ok(String::new())
        }
        Id::NoIgmpQuerierAddress => {
            c.dm()?.igmp.querier_address = None;
            Ok(String::new())
        }
        Id::ClearIgmpSnooping => {
            c.sim.clear_igmp(c.dev);
            Ok(String::new())
        }
        Id::ShowIgmpSnooping => {
            let d = c.d()?;
            let mut s = String::new();
            let _ = writeln!(s, "IGMP snooping:  {}", if d.igmp.on { "enabled" } else { "disabled" });
            let _ = writeln!(
                s,
                "Querier:        {}{}",
                if d.igmp.querier { "enabled" } else { "disabled" },
                d.igmp.querier_address.map_or(String::new(), |a| format!(" (address {a})"))
            );
            let _ = writeln!(s, "\n{:<6} Snooping", "VLAN");
            for v in d.vlans.keys() {
                let _ = writeln!(s, "{:<6} {}", v.get(), if d.igmp.active(v.get()) { "enabled" } else { "disabled" });
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowIgmpGroups => {
            let d = c.d()?.clone();
            let rows = c.sim.igmp_groups(c.dev);
            if rows.is_empty() {
                return Ok("No groups learned.".into());
            }
            let mut s = format!("{:<6} {:<16} Ports\n", "VLAN", "Group");
            for r in rows {
                let ports: Vec<String> = r.ports.iter().map(|p| short(&d, *p)).collect();
                let _ = writeln!(s, "{:<6} {:<16} {}", r.vlan, r.group, ports.join(", "));
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowIgmpMrouter => {
            let d = c.d()?.clone();
            let rows = c.sim.igmp_mrouters(c.dev);
            if rows.is_empty() {
                return Ok("No ports lead to a multicast router.".into());
            }
            let mut s = format!("{:<6} Port\n", "VLAN");
            for (v, p) in rows {
                let _ = writeln!(s, "{v:<6} {}", short(&d, p));
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowIgmpQuerier => {
            let now = c.sim.now_us();
            let rows = c.sim.igmp_queries(c.dev);
            if rows.is_empty() {
                return Ok("This switch has sent no queries.".into());
            }
            let mut s = format!("{:<6} {:<16} Last query\n", "VLAN", "Address");
            for (v, a, at) in rows {
                let _ = writeln!(s, "{v:<6} {a:<16} {}", crate::render::hms(now.saturating_sub(at)));
            }
            Ok(s.trim_end().to_string())
        }
        _ => Err("% That command is not implemented".into()),
    }
}

/// The lines of a running configuration that belong to IGMP snooping.
pub(crate) fn global_lines(s: &mut String, d: &Device) {
    let i = &d.igmp;
    if i.on {
        let _ = writeln!(s, "ip igmp snooping");
    }
    for v in &i.vlan_off {
        let _ = writeln!(s, "no ip igmp snooping vlan {v}");
    }
    if i.querier {
        let _ = writeln!(s, "ip igmp snooping querier");
    }
    if let Some(a) = i.querier_address {
        let _ = writeln!(s, "ip igmp snooping querier address {a}");
    }
}
