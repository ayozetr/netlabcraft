//! Multiple spanning tree: `spanning-tree mst configuration` (name, revision, instance to VLAN
//! table), per-instance priority, cost and port priority, and `show spanning-tree mst`.
//!
//! Changes apply at once (there is no pending copy). A switch is in the region its name, revision
//! and table say; the trees come from `netlabcraft_sim::stp`.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use netlabcraft_core::{VlanId, VlanSet};
use netlabcraft_model::l2sec::vlan_list;
use netlabcraft_model::mst::{MAX_INSTANCE, MAX_NAME};
use netlabcraft_model::{Device, End, Topology, ifname};
use netlabcraft_sim::stp::Role;

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn instance(text: &str, lo: u16) -> Result<u16, String> {
    text.trim()
        .parse::<u16>()
        .ok()
        .filter(|n| (lo..=MAX_INSTANCE).contains(n))
        .ok_or_else(|| format!("% The instance is a number from {lo} to {MAX_INSTANCE}"))
}

fn to_ids(set: &BTreeSet<u16>) -> BTreeSet<VlanId> {
    set.iter().filter_map(|v| VlanId::new(*v)).collect()
}

/// The VLANs of instance 0: every VLAN no instance claims.
fn cist_vlans(d: &Device) -> BTreeSet<u16> {
    (1u16..=4094).filter(|v| d.mst.instance_of(*v) == 0).collect()
}

fn mapped(d: &Device, inst: u16) -> BTreeSet<u16> {
    if inst == 0 { cist_vlans(d) } else { d.mst.instances.get(&inst).cloned().unwrap_or_default() }
}

/// The VLAN whose tree stands for an instance: the lowest VLAN of the switch it holds.
fn representative(d: &Device, inst: u16) -> Option<VlanId> {
    d.vlans.keys().copied().find(|v| d.mst.instance_of(v.get()) == inst)
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::MstConfiguration => {
            c.cli.mode = Mode::MstConfig;
            Ok(String::new())
        }
        Id::NoMstConfiguration => {
            let d = c.dm()?;
            d.mst.name.clear();
            d.mst.revision = 0;
            d.mst.instances.clear();
            Ok(String::new())
        }
        Id::MstName => {
            let name = arg(0).trim();
            if name.is_empty() || name.chars().count() > MAX_NAME || !name.chars().all(|ch| ch.is_ascii_graphic()) {
                return Err(format!("% The region name is 1-{MAX_NAME} printable characters with no spaces"));
            }
            c.dm()?.mst.name = name.to_string();
            Ok(String::new())
        }
        Id::MstRevision => {
            let n: u16 = arg(0).trim().parse().map_err(|_| "% The revision is a number from 0 to 65535")?;
            c.dm()?.mst.revision = n;
            Ok(String::new())
        }
        Id::MstInstanceVlan => {
            let inst = instance(arg(0), 1)?;
            let set = VlanSet::parse_list(arg(1)).map_err(|e| format!("% {e}"))?;
            let d = c.dm()?;
            let wanted: BTreeSet<u16> = set.iter().map(|v| v.get()).collect();
            for v in d.mst.instances.values_mut() {
                v.retain(|x| !wanted.contains(x));
            }
            d.mst.instances.retain(|_, v| !v.is_empty());
            d.mst.instances.entry(inst).or_default().extend(wanted);
            Ok(String::new())
        }
        Id::NoMstInstance => {
            let inst = instance(arg(0), 1)?;
            c.dm()?.mst.instances.remove(&inst);
            Ok(String::new())
        }
        Id::MstPriority | Id::MstRoot | Id::NoMstPriority => {
            let inst = instance(arg(0), 0)?;
            let pri = match id {
                Id::MstPriority => {
                    let p: u16 = arg(1).trim().parse().map_err(|_| "% The priority is a number, 0-61440")?;
                    if p > 61_440 || !p.is_multiple_of(4096) {
                        return Err("% The priority is a multiple of 4096 from 0 to 61440".into());
                    }
                    Some(p)
                }
                Id::MstRoot => match arg(1).to_ascii_lowercase().as_str() {
                    "primary" => Some(24_576),
                    "secondary" => Some(28_672),
                    _ => return Err("% `root primary` or `root secondary`".into()),
                },
                _ => None,
            };
            let d = c.dm()?;
            match pri {
                Some(p) => {
                    d.mst.priority.insert(inst, p);
                }
                None => {
                    d.mst.priority.remove(&inst);
                }
            }
            Ok(String::new())
        }
        Id::MstCost | Id::NoMstCost | Id::MstPortPriority | Id::NoMstPortPriority => {
            let inst = instance(arg(0), 0)?;
            let cost = matches!(id, Id::MstCost | Id::NoMstCost);
            let set = matches!(id, Id::MstCost | Id::MstPortPriority);
            let value: Option<u32> = if !set {
                None
            } else {
                let n: u32 = arg(1).trim().parse().map_err(|_| "% The value is a number")?;
                if cost && !(1..=200_000_000).contains(&n) {
                    return Err("% The cost is a number from 1 to 200000000".into());
                }
                if !cost && (n > 240 || !n.is_multiple_of(16)) {
                    return Err("% The port priority is a multiple of 16 from 0 to 240".into());
                }
                Some(n)
            };
            c.each_if(|p, _| {
                let map = if cost { &mut p.l2.mst_cost } else { &mut p.l2.mst_priority };
                match value {
                    Some(v) => {
                        map.insert(inst, v);
                    }
                    None => {
                        map.remove(&inst);
                    }
                }
                Ok(())
            })
        }
        Id::ShowMstConfig => {
            let d = c.d()?;
            if !d.mst.on {
                return Ok("Multiple spanning tree is not running (`spanning-tree mode mst` turns it on).".into());
            }
            Ok(configuration(d))
        }
        Id::ShowMst | Id::ShowMstInstance => {
            let d = c.d()?.clone();
            if !d.mst.on {
                return Ok("Multiple spanning tree is not running (`spanning-tree mode mst` turns it on).".into());
            }
            let list: Vec<u16> =
                if id == Id::ShowMstInstance { vec![instance(arg(0), 0)?] } else { std::iter::once(0).chain(d.mst.used_instances()).collect() };
            let mut out = Vec::new();
            for inst in list {
                out.push(instance_view(c, &d, inst));
            }
            Ok(out.join("\n\n"))
        }
        _ => Err("% That command is not implemented".into()),
    }
}

/// `show spanning-tree mst configuration`.
pub(crate) fn configuration(d: &Device) -> String {
    let digest = netlabcraft_wire::mst::config_digest(|v| d.mst.instance_of(v));
    let mut s = String::new();
    let _ = writeln!(s, "Region name:  {}", if d.mst.name.is_empty() { "(not set)" } else { &d.mst.name });
    let _ = writeln!(s, "Revision:     {}", d.mst.revision);
    let _ = writeln!(s, "Digest:       {}", digest.iter().map(|b| format!("{b:02X}")).collect::<String>());
    let _ = writeln!(s, "Instances:    {} besides the common tree", d.mst.used_instances().len());
    let _ = writeln!(s, "\n{:<9} VLANs", "Instance");
    for inst in std::iter::once(0).chain(d.mst.used_instances()) {
        let set = mapped(d, inst);
        let text = if set.is_empty() { "none".to_string() } else { vlan_list(&to_ids(&set)) };
        let _ = writeln!(s, "{inst:<9} {text}");
    }
    s.trim_end().to_string()
}

/// What the link at a port faces, for the Type column: another switch in the region, or the
/// edge of the region towards a switch outside it.
fn link_kind(t: &Topology, d: &Device, p: usize) -> String {
    let Some(q) = t.peer(End { device: d.id, port: p }) else { return "Edge".into() };
    let Some(pd) = t.device(q.device).filter(|x| x.switches_frames()) else { return "Edge".into() };
    if d.mst.same_region(&pd.mst) {
        "Internal".into()
    } else if pd.mst.on {
        "Boundary(MST)".into()
    } else if pd.rapid_stp {
        "Boundary(RSTP)".into()
    } else {
        "Boundary(STP)".into()
    }
}

fn instance_view(c: &mut Ctx, d: &Device, inst: u16) -> String {
    let set = mapped(d, inst);
    let list = if set.is_empty() { "none".to_string() } else { vlan_list(&to_ids(&set)) };
    let name = if inst == 0 { "MST0 (the common tree)".to_string() } else { format!("MST{inst}") };
    let mut s = format!("{name}: VLANs {list}\n");
    let Some(v) = representative(d, inst) else {
        let _ = write!(s, "  This switch has none of these VLANs, so it has no tree for them.");
        return s;
    };
    let tree = c.sim.spanning_tree(c.t, v).clone();
    let Some(me) = tree.bridges.get(&d.id) else {
        let _ = write!(s, "  This switch has no port in the tree.");
        return s;
    };
    let _ = writeln!(s, "  Bridge: priority {}, address {}", me.id.priority, me.id.mac.dotted());
    if me.root == me.id {
        let _ = writeln!(s, "  Root:   this switch");
    } else {
        let rp = me.root_port.map_or("-".to_string(), |p| netlabcraft_sim::channel::port_label(c.t, d, p, true));
        let _ = writeln!(s, "  Root:   priority {}, address {}; path cost {} through {rp}", me.root.priority, me.root.mac.dotted(), me.root_cost);
    }
    let _ = writeln!(s, "\n  {:<10} {:<5} {:<4} {:<10} {:<8} Type", "Interface", "Role", "Sts", "Cost", "Prio.Nbr");
    for ((_, p), info) in tree.ports.iter().filter(|((dev, _), i)| *dev == d.id && i.role != Role::Disabled) {
        let state = c.sim.stp_transition(d.id, *p, v).unwrap_or(info.role.state());
        let label = d.ports.get(*p).map_or_else(String::new, |x| ifname::short(&x.name));
        let _ = writeln!(
            s,
            "  {label:<10} {:<5} {state:<4} {:<10} {:<8} {}",
            info.role.short(),
            info.cost,
            format!("{}.{}", info.port_id / 1000, info.port_id % 1000),
            link_kind(c.t, d, *p)
        );
    }
    s.trim_end().to_string()
}

/// The lines of a running configuration that belong to MST: the region and the priorities.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    for (i, p) in &d.mst.priority {
        let _ = writeln!(s, "spanning-tree mst {i} priority {p}");
    }
    let _ = writeln!(s, "spanning-tree mst configuration");
    if !d.mst.name.is_empty() {
        let _ = writeln!(s, " name {}", d.mst.name);
    }
    let _ = writeln!(s, " revision {}", d.mst.revision);
    for (i, v) in d.mst.instances.iter().filter(|(_, v)| !v.is_empty()) {
        let _ = writeln!(s, " instance {i} vlan {}", vlan_list(&to_ids(v)));
    }
    let _ = writeln!(s, "!");
}
