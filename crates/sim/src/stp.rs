//! Spanning tree (IEEE 802.1D, one tree per VLAN as PVST does).
//!
//! The tree is computed from the topology the way the protocol converges: the lowest bridge id
//! is the root; every other bridge picks the port with the lowest cost to the root (ties: lowest
//! sender bridge id, sender port id, own port id); on every switch-to-switch link the end closer
//! to the root is designated; the other end, unless it is its bridge's root port, blocks. Ports
//! facing hosts and routers are designated. Listening and learning periods are timed by the
//! simulator (`Sim::stp_transition`); the BPDUs themselves go on the wire from `control`.
//!
//! A port with `spanning-tree bpdufilter enable` sends and takes no BPDUs, so the link it is on
//! counts as an edge at both ends: each end hears nothing and becomes designated. Loop guard
//! (`spanning-tree guard loop`, or `spanning-tree loopguard default` on switch-to-switch ports)
//! catches the port that stops hearing BPDUs that way: if it would have been a root or
//! alternate port had the BPDUs kept coming, it blocks as loop-inconsistent instead of
//! forwarding (IEEE 802.1D-2004 leaves this out; it is the usual vendor extension).

use std::collections::{BTreeMap, BTreeSet};

use netlabcraft_core::{DeviceId, MacAddr, VlanId};
use netlabcraft_model::{Device, End, STP_DEFAULT_PRIORITY, Topology};

use crate::channel::PortMap;
use crate::port_up;

/// A port's role in one VLAN's tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Root,
    Designated,
    Alternate,
    Disabled,
}

impl Role {
    pub fn forwards(self) -> bool {
        matches!(self, Role::Root | Role::Designated)
    }

    pub fn short(self) -> &'static str {
        match self {
            Role::Root => "Root",
            Role::Designated => "Desg",
            Role::Alternate => "Altn",
            Role::Disabled => "Disb",
        }
    }

    pub fn state(self) -> &'static str {
        match self {
            Role::Root | Role::Designated => "FWD",
            Role::Alternate => "BLK",
            Role::Disabled => "DIS",
        }
    }
}

/// Priority (with the VLAN added, as PVST does) then MAC: lower wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BridgeId {
    pub priority: u32,
    pub mac: MacAddr,
}

/// One bridge in a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bridge {
    pub id: BridgeId,
    /// The root of this bridge's part of the network.
    pub root: BridgeId,
    pub root_cost: u32,
    pub root_port: Option<usize>,
}

/// One port in a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortInfo {
    pub role: Role,
    pub cost: u32,
    /// 128.n
    pub port_id: u32,
}

/// The spanning tree of one VLAN.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VlanTree {
    pub bridges: BTreeMap<DeviceId, Bridge>,
    pub ports: BTreeMap<(DeviceId, usize), PortInfo>,
    /// Root-guard ports that heard a better root: blocked (root-inconsistent) until it goes.
    pub inconsistent: BTreeSet<(DeviceId, usize)>,
    /// Loop-guard ports that stopped hearing BPDUs: blocked (loop-inconsistent).
    pub loop_inconsistent: BTreeSet<(DeviceId, usize)>,
}

impl VlanTree {
    /// Whether a port forwards in this VLAN (ports outside the tree do).
    pub fn forwards(&self, dev: DeviceId, port: usize) -> bool {
        self.ports.get(&(dev, port)).is_none_or(|p| p.role.forwards())
    }
}

/// Port cost by speed (802.1D-1998 short values).
pub fn port_cost(name: &str) -> u32 {
    if name.starts_with("GigabitEthernet") {
        4
    } else if name.starts_with("FastEthernet") {
        19
    } else {
        100
    }
}

pub fn port_id(port: usize) -> u32 {
    128 * 1000 + u32::try_from(port + 1).unwrap_or(u32::MAX - 128_000)
}

/// The port id with the port's configured priority (`spanning-tree [vlan N] port-priority`):
/// priority × 1000 + port number, so a lower priority wins first.
pub fn port_id_in(d: Option<&Device>, port: usize, vlan: VlanId) -> u32 {
    let pri = d.and_then(|d| d.ports.get(port)).map_or(128, |p| p.l2.priority(vlan)).min(240);
    pri * 1000 + u32::try_from(port + 1).unwrap_or(999).min(999)
}

/// Most rounds of root-guard re-election (each round blocks at least one more port).
const MAX_GUARD_ROUNDS: usize = 16;

/// Computes one VLAN's tree; root-guard ports that would hear a better root (they come out
/// root or alternate) are blocked as root-inconsistent and the tree is elected again without
/// them.
pub fn compute(t: &Topology, vlan: VlanId) -> VlanTree {
    let mut excluded: BTreeSet<(DeviceId, usize)> = BTreeSet::new();
    let guarded = |k: &(DeviceId, usize)| t.device(k.0).and_then(|d| d.ports.get(k.1)).is_some_and(|p| p.l2.root_guard);
    // Loop guard: the guarded ports on a filtered link that would be root or alternate if the
    // BPDUs got through.
    let mut looped: BTreeSet<(DeviceId, usize)> = BTreeSet::new();
    if t.devices.iter().any(|d| d.ports.iter().any(|p| p.l2.bpdufilter == Some(true))) {
        let full = compute_with(t, vlan, &excluded, &looped, false);
        looped = full
            .ports
            .iter()
            .filter(|(k, i)| matches!(i.role, Role::Root | Role::Alternate) && loop_guard_on(t, k.0, k.1) && filtered_link(t, k.0, k.1))
            .map(|(k, _)| *k)
            .collect();
    }
    let mut tree = compute_with(t, vlan, &excluded, &looped, true);
    for _ in 0..MAX_GUARD_ROUNDS {
        let bad: Vec<(DeviceId, usize)> = tree
            .ports
            .iter()
            .filter(|(k, i)| matches!(i.role, Role::Root | Role::Alternate) && guarded(k) && !excluded.contains(*k))
            .map(|(k, _)| *k)
            .collect();
        if bad.is_empty() {
            break;
        }
        excluded.extend(bad);
        tree = compute_with(t, vlan, &excluded, &looped, true);
    }
    tree.inconsistent = excluded;
    tree.loop_inconsistent = looped;
    tree
}

/// Whether a port's link is point-to-point for rapid spanning tree: as `spanning-tree
/// link-type` says, else from the duplex (half duplex is a shared segment).
pub fn point_to_point(d: &Device, port: usize) -> bool {
    let Some(p) = d.ports.get(port) else { return false };
    match p.l2.link_type {
        Some(netlabcraft_model::l2sec::LinkType::PointToPoint) => true,
        Some(netlabcraft_model::l2sec::LinkType::Shared) => false,
        None => p.duplex != Some(netlabcraft_model::Duplex::Half),
    }
}

/// Whether a port can use the rapid handshake (802.1w proposal and agreement): both switches
/// run rapid spanning tree, the link is point-to-point at both ends, and BPDUs cross it.
pub fn rapid_link(t: &Topology, dev: DeviceId, port: usize) -> bool {
    let end = End { device: dev, port };
    let Some(d) = t.device(dev) else { return false };
    let Some((q, pd)) = t.peer(end).and_then(|q| t.device(q.device).map(|pd| (q, pd))) else { return false };
    d.rapid_stp && pd.rapid_stp && pd.switches_frames() && point_to_point(d, port) && point_to_point(pd, q.port) && !filtered_link(t, dev, port)
}

/// Whether a port is cabled to a switch port (one that takes part in spanning tree).
pub fn faces_switch(t: &Topology, dev: DeviceId, port: usize) -> bool {
    t.peer(End { device: dev, port })
        .and_then(|q| t.device(q.device).map(|pd| (q, pd)))
        .is_some_and(|(q, pd)| pd.switches_frames() && pd.ports.get(q.port).is_some_and(|x| !x.routed))
}

/// Whether a port has `spanning-tree bpdufilter enable`.
pub fn bpdufilter_on(d: &Device, port: usize) -> bool {
    d.ports.get(port).is_some_and(|p| p.l2.bpdufilter == Some(true))
}

/// Whether BPDUs stop on the link at a port: this end or the far end filters them.
pub fn filtered_link(t: &Topology, dev: DeviceId, port: usize) -> bool {
    let end = End { device: dev, port };
    t.device(dev).is_some_and(|d| bpdufilter_on(d, port))
        || t.peer(end).and_then(|q| t.device(q.device).map(|pd| (q, pd))).is_some_and(|(q, pd)| bpdufilter_on(pd, q.port))
}

/// Whether loop guard is on for a port: set on it, or the switch default on a port cabled to
/// another switch.
pub fn loop_guard_on(t: &Topology, dev: DeviceId, port: usize) -> bool {
    let Some(d) = t.device(dev) else { return false };
    let Some(p) = d.ports.get(port) else { return false };
    p.l2.loop_guard
        || (d.l2.loopguard_default && t.peer(End { device: dev, port }).and_then(|q| t.device(q.device)).is_some_and(|x| x.switches_frames()))
}

pub fn bridge_id(d: &Device, vlan: VlanId) -> BridgeId {
    let pri = u32::from(d.stp_priority.get(&vlan).copied().unwrap_or(STP_DEFAULT_PRIORITY));
    BridgeId { priority: pri + u32::from(vlan.get()), mac: d.ports.first().map(|p| p.mac).unwrap_or_default() }
}

/// Whether a switch port carries a VLAN.
pub fn carries(t: &Topology, d: &Device, port: usize, vlan: VlanId) -> bool {
    d.switches_frames()
        && t.switch_has_vlan(d, vlan)
        && (d.ports.get(port).filter(|_| d.is_switchport(port)).is_some_and(|p| {
            if t.is_trunk(End { device: d.id, port }) { p.switchport.allowed.contains(vlan) } else { p.switchport.access_carries(vlan) }
        }) || t.radio_carries(d, port, vlan))
}

fn compute_with(t: &Topology, vlan: VlanId, excluded: &BTreeSet<(DeviceId, usize)>, looped: &BTreeSet<(DeviceId, usize)>, filters: bool) -> VlanTree {
    let mut tree = VlanTree::default();
    let switches: Vec<&Device> = t.devices.iter().filter(|d| d.switches_frames() && t.switch_has_vlan(d, vlan)).collect();
    // An EtherChannel takes part as one port: its lead member, costed by the bundle's speed.
    let maps: BTreeMap<DeviceId, PortMap> = switches.iter().map(|d| (d.id, PortMap::new(t, d))).collect();
    let logical = |dev: DeviceId, p: usize| maps.get(&dev).map_or(p, |m| m.logical(p));
    let blocked = |dev: DeviceId, p: usize| maps.get(&dev).is_some_and(|m| m.suspended(p));
    let cost_of = |dev: DeviceId, port: usize| {
        let Some(d) = t.device(dev) else { return 100 };
        if let Some(c) = d.ports.get(port).and_then(|p| p.l2.cost(vlan)) {
            return c;
        }
        let members = maps.get(&dev).map_or_else(|| vec![port], |m| m.members(port));
        let mbps: u32 = members.iter().filter_map(|p| d.ports.get(*p)).map(|p| speed_mbps(&p.name)).sum();
        cost_for_speed(mbps)
    };
    let pid = |dev: DeviceId, p: usize| port_id_in(t.device(dev), p, vlan);
    let mut linkset: BTreeSet<(DeviceId, usize, DeviceId, usize)> = BTreeSet::new();
    let ids: BTreeMap<DeviceId, BridgeId> = switches.iter().map(|d| (d.id, bridge_id(d, vlan))).collect();
    // Switch-to-switch links carrying the VLAN: (a, port a, b, port b).
    let mut links: Vec<(DeviceId, usize, DeviceId, usize)> = Vec::new();
    for d in &switches {
        for p in 0..d.ports.len() {
            let end = End { device: d.id, port: p };
            if !carries(t, d, p, vlan) {
                continue;
            }
            let lp = logical(d.id, p);
            if !port_up(t, end) || blocked(d.id, p) {
                if lp == p {
                    tree.ports.insert((d.id, p), PortInfo { role: Role::Disabled, cost: cost_of(d.id, p), port_id: pid(d.id, p) });
                }
                continue;
            }
            // A root- or loop-inconsistent port blocks; the far end sees nothing from it.
            if excluded.contains(&(d.id, lp)) || (filters && looped.contains(&(d.id, lp))) {
                if lp == p {
                    tree.ports.insert((d.id, p), PortInfo { role: Role::Alternate, cost: cost_of(d.id, p), port_id: pid(d.id, p) });
                }
                continue;
            }
            let silent = filters && filtered_link(t, d.id, p);
            match t.peer(end).and_then(|q| t.device(q.device).map(|pd| (q, pd))) {
                Some((q, pd))
                    if !silent
                        && carries(t, pd, q.port, vlan)
                        && !blocked(q.device, q.port)
                        && !excluded.contains(&(q.device, logical(q.device, q.port))) =>
                {
                    let (a, b) = ((d.id, lp), (q.device, logical(q.device, q.port)));
                    let (a, b) = if a < b { (a, b) } else { (b, a) };
                    linkset.insert((a.0, a.1, b.0, b.1));
                }
                _ if lp == p => {
                    // Faces a host, router or hub: an edge, always designated.
                    tree.ports.insert((d.id, p), PortInfo { role: Role::Designated, cost: cost_of(d.id, p), port_id: pid(d.id, p) });
                }
                _ => {}
            }
        }
    }
    links.extend(linkset);

    // Components, each with its own root.
    let mut seen: BTreeSet<DeviceId> = BTreeSet::new();
    for start in ids.keys() {
        if seen.contains(start) {
            continue;
        }
        let mut comp = vec![*start];
        let mut i = 0;
        seen.insert(*start);
        while let Some(&u) = comp.get(i) {
            i += 1;
            for &(a, _, b, _) in &links {
                let next = if a == u {
                    b
                } else if b == u {
                    a
                } else {
                    continue;
                };
                if seen.insert(next) {
                    comp.push(next);
                }
            }
        }
        let Some(root) = comp.iter().filter_map(|d| ids.get(d).map(|b| (*b, *d))).min() else { continue };
        // Dijkstra over the component (costs of receiving ports).
        let mut dist: BTreeMap<DeviceId, u32> = BTreeMap::new();
        dist.insert(root.1, 0);
        let mut done: BTreeSet<DeviceId> = BTreeSet::new();
        while let Some((&u, &du)) = dist.iter().filter(|(d, _)| !done.contains(*d)).min_by_key(|(d, c)| (**c, ids.get(*d).copied())) {
            done.insert(u);
            for &(a, pa, b, pb) in &links {
                let (v, pv) = if a == u {
                    (b, pb)
                } else if b == u {
                    (a, pa)
                } else {
                    continue;
                };
                let c = du.saturating_add(cost_of(v, pv));
                if dist.get(&v).is_none_or(|old| c < *old) && !done.contains(&v) {
                    dist.insert(v, c);
                }
            }
        }
        // Root ports.
        let mut root_port: BTreeMap<DeviceId, usize> = BTreeMap::new();
        for &v in &comp {
            if v == root.1 {
                continue;
            }
            let mut best: Option<((u32, BridgeId, u32, u32), usize)> = None;
            for &(a, pa, b, pb) in &links {
                let (u, pu, pv) = if b == v {
                    (a, pa, pb)
                } else if a == v {
                    (b, pb, pa)
                } else {
                    continue;
                };
                let (Some(du), Some(bu)) = (dist.get(&u), ids.get(&u)) else { continue };
                let key = (du.saturating_add(cost_of(v, pv)), *bu, pid(u, pu), pid(v, pv));
                if best.is_none_or(|(k, _)| key < k) {
                    best = Some((key, pv));
                }
            }
            if let Some((_, p)) = best {
                root_port.insert(v, p);
            }
        }
        for &v in &comp {
            if let (Some(id), Some(c)) = (ids.get(&v), dist.get(&v)) {
                tree.bridges.insert(v, Bridge { id: *id, root: root.0, root_cost: *c, root_port: root_port.get(&v).copied() });
            }
        }
        // Designated and alternate ports on each link of the component.
        for &(a, pa, b, pb) in links.iter().filter(|(a, ..)| comp.contains(a)) {
            let key = |d: DeviceId, p: usize| (dist.get(&d).copied().unwrap_or(u32::MAX), ids.get(&d).copied(), pid(d, p));
            let (des, other) = if key(a, pa) <= key(b, pb) { ((a, pa), (b, pb)) } else { ((b, pb), (a, pa)) };
            tree.ports.insert(des, PortInfo { role: Role::Designated, cost: cost_of(des.0, des.1), port_id: pid(des.0, des.1) });
            let role = if root_port.get(&other.0) == Some(&other.1) { Role::Root } else { Role::Alternate };
            tree.ports.insert(other, PortInfo { role, cost: cost_of(other.0, other.1), port_id: pid(other.0, other.1) });
        }
    }
    tree
}

/// A port's speed from its name.
fn speed_mbps(name: &str) -> u32 {
    if name.starts_with("GigabitEthernet") {
        1_000
    } else if name.starts_with("FastEthernet") {
        100
    } else {
        10
    }
}

/// Path cost for a link (or bundle) speed: faster links cost less.
fn cost_for_speed(mbps: u32) -> u32 {
    match mbps {
        4_000.. => 2,
        2_000.. => 3,
        1_000.. => 4,
        400.. => 9,
        200.. => 12,
        100.. => 19,
        _ => 100,
    }
}

/// A cheap fingerprint of everything the trees depend on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &t.links {
        (l.a.device, l.a.port, l.b.device, l.b.port).hash(&mut h);
        // A cable made too long (or short enough again) changes the tree as unplugging does.
        crate::cable_problem(t, l.a).is_some().hash(&mut h);
    }
    for d in &t.devices {
        d.id.hash(&mut h);
        for p in &d.ports {
            (p.shutdown, p.err_disabled, p.channel, p.routed).hash(&mut h);
            (&p.l2.stp_cost, &p.l2.stp_priority, p.l2.root_guard, p.l2.loop_guard, p.l2.bpdufilter).hash(&mut h);
            if d.switches_frames() {
                format!("{:?}", p.switchport).hash(&mut h);
            }
        }
        d.modules.hash(&mut h);
        if d.switches_frames() {
            format!("{:?}{:?}", d.vlans.keys().collect::<Vec<_>>(), d.stp_priority).hash(&mut h);
            (d.l2.loopguard_default, d.rapid_stp).hash(&mut h);
            // An access point's radio is up while hosts are associated (a host moving in or
            // out of range changes it), and a lightweight AP carries its WLANs' VLANs.
            for p in (0..d.ports.len()).filter(|p| crate::is_ap_radio(t, End { device: d.id, port: *p })) {
                crate::port_up(t, End { device: d.id, port: p }).hash(&mut h);
            }
            if netlabcraft_model::wlan::is_lap(d) {
                t.lap_vlans(d.id).hash(&mut h);
            }
        }
    }
    h.finish()
}
