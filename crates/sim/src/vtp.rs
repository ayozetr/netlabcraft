//! VTP: switches in one domain share their VLAN database over trunks.
//!
//! Like spanning tree, the outcome is computed at once instead of by timed advertisements:
//! within each group of switches that hear each other (same domain and password, joined by up
//! trunks, possibly through transparent switches that pass advertisements on), the database
//! with the highest revision is copied to every server and client with a lower one. That
//! includes a client: plugging in a switch with a higher revision overwrites the domain's
//! VLANs, as on real networks. A switch with no domain yet takes the first one it hears.
//!
//! **Pruning** (`vtp pruning` on a server; the setting travels with the database): a switch
//! stops flooding a VLAN (broadcasts, multicasts, unknown unicasts) out of a trunk when the
//! switch at the other end, pruning in the same domain, has no up access port or VLAN
//! interface in it and no trunk leading on to one that needs it. That is what the pruning
//! join messages of VTP version 2 tell a neighbour; their format is a vendor's own, so, like
//! the advertisements, they are computed rather than drawn on the wire. VLAN 1 and VLANs above
//! 1001 are never pruned; `switchport trunk pruning vlan …` narrows the eligible ones per trunk.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use netlabcraft_core::{DeviceId, VlanId, VlanSet};
use netlabcraft_model::{Device, DeviceKind, End, Topology, VTP_MAX_VLAN, VtpMode};

use crate::port_up;

/// Switch neighbours over up trunk links.
fn trunk_neighbours(t: &Topology) -> BTreeMap<DeviceId, BTreeSet<DeviceId>> {
    let mut adj: BTreeMap<DeviceId, BTreeSet<DeviceId>> = BTreeMap::new();
    let trunk = |e: netlabcraft_model::End| {
        t.device(e.device).filter(|d| d.kind == DeviceKind::Switch).and_then(|d| d.ports.get(e.port)).is_some() && t.is_trunk(e)
    };
    for l in &t.links {
        if l.a.device != l.b.device && trunk(l.a) && trunk(l.b) && port_up(t, l.a) {
            adj.entry(l.a.device).or_default().insert(l.b.device);
            adj.entry(l.b.device).or_default().insert(l.a.device);
        }
    }
    adj
}

/// Whether switch `relay` passes on advertisements of `domain` without taking part.
fn relays(t: &Topology, relay: DeviceId, domain: &str) -> bool {
    t.device(relay).is_some_and(|d| d.vtp.mode == VtpMode::Transparent && (d.vtp.version >= 2 || d.vtp.domain == domain))
}

/// Brings every switch's VLAN database in line with its VTP domain. Returns whether anything
/// changed.
pub fn sync(t: &mut Topology) -> bool {
    let adj = trunk_neighbours(t);
    let empty = BTreeSet::new();
    let mut changed = false;
    // A switch without a domain joins the first one it hears (through transparent switches too).
    let lonely: Vec<DeviceId> = t
        .devices
        .iter()
        .filter(|d| d.kind == DeviceKind::Switch && d.vtp.mode != VtpMode::Transparent && d.vtp.domain.is_empty())
        .map(|d| d.id)
        .collect();
    for id in lonely {
        let mut seen = BTreeSet::from([id]);
        let mut queue = VecDeque::from([id]);
        let mut heard: Option<String> = None;
        while let Some(u) = queue.pop_front() {
            for &v in adj.get(&u).unwrap_or(&empty) {
                if !seen.insert(v) {
                    continue;
                }
                let Some(dv) = t.device(v) else { continue };
                if dv.vtp.mode == VtpMode::Transparent {
                    if dv.vtp.version >= 2 {
                        queue.push_back(v);
                    }
                } else if !dv.vtp.domain.is_empty() {
                    heard = Some(heard.map_or(dv.vtp.domain.clone(), |h: String| h.min(dv.vtp.domain.clone())));
                }
            }
        }
        if let (Some(domain), Some(d)) = (heard, t.device_mut(id)) {
            d.vtp.domain = domain;
            changed = true;
        }
    }
    // Groups that hear each other, each converging on its highest revision.
    let mut done: BTreeSet<DeviceId> = BTreeSet::new();
    let starts: Vec<DeviceId> = t
        .devices
        .iter()
        .filter(|d| d.kind == DeviceKind::Switch && d.vtp.mode != VtpMode::Transparent && !d.vtp.domain.is_empty())
        .map(|d| d.id)
        .collect();
    for start in starts {
        if done.contains(&start) {
            continue;
        }
        let Some(s) = t.device(start) else { continue };
        let (domain, password) = (s.vtp.domain.clone(), s.vtp.password.clone());
        let mut members = vec![start];
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(u) = queue.pop_front() {
            for &v in adj.get(&u).unwrap_or(&empty) {
                if !seen.insert(v) {
                    continue;
                }
                let Some(dv) = t.device(v) else { continue };
                if dv.vtp.mode == VtpMode::Transparent {
                    if relays(t, v, &domain) {
                        queue.push_back(v);
                    }
                } else if dv.vtp.domain == domain && dv.vtp.password == password {
                    members.push(v);
                    queue.push_back(v);
                }
            }
        }
        done.extend(members.iter().copied());
        let Some(best) = members.iter().filter_map(|id| t.device(*id)).max_by_key(|d| (d.vtp.revision, std::cmp::Reverse(d.id))) else { continue };
        let (revision, vlans, pruning) = (best.vtp.revision, best.vlans.clone(), best.vtp.pruning);
        let vlans: BTreeMap<_, _> = vlans.into_iter().filter(|(v, _)| v.get() <= VTP_MAX_VLAN).collect();
        for id in members {
            if let Some(d) = t.device_mut(id)
                && d.vtp.revision < revision
            {
                d.vlans = vlans.clone();
                d.vtp.revision = revision;
                d.vtp.pruning = pruning;
                changed = true;
            }
        }
    }
    changed
}

/// The VLANs pruning may stop on a trunk by default (VLAN 1 and the reserved and extended
/// ranges never are).
pub fn default_prune_eligible(v: VlanId) -> bool {
    (2..=1001).contains(&v.get())
}

/// Whether VLAN `v` may be pruned on a switch's trunk port: eligible by default or as listed
/// with `switchport trunk pruning vlan`.
pub fn prune_eligible(d: &Device, port: usize, v: VlanId) -> bool {
    match d.ports.get(port).and_then(|p| p.l2.prune_eligible.as_ref()) {
        Some(set) => set.contains(v) && v.get() != 1,
        None => default_prune_eligible(v),
    }
}

/// Whether a switch takes part in VTP pruning: pruning on in its domain, not transparent.
fn prunes(d: &Device) -> bool {
    d.kind == DeviceKind::Switch && d.vtp.pruning && d.vtp.mode != VtpMode::Transparent && !d.vtp.domain.is_empty()
}

/// Whether switch `d` needs VLAN `v` from the trunk it hears on `from`: it has an up access
/// port (or VLAN interface) in it, or another trunk leads on to a switch that needs it (or that
/// doesn't prune, so it can't say it doesn't). This is what VTP pruning's join messages tell
/// the neighbour, computed here instead of sent.
fn needs(t: &Topology, d: &Device, from: usize, v: VlanId, seen: &mut BTreeSet<DeviceId>) -> bool {
    if !seen.insert(d.id) {
        return false;
    }
    if d.svis.iter().any(|s| s.vlan == v && !s.shutdown) {
        return true;
    }
    for (i, p) in d.ports.iter().enumerate() {
        if i == from || !port_up(t, End { device: d.id, port: i }) {
            continue;
        }
        let end = End { device: d.id, port: i };
        if !t.is_trunk(end) {
            if p.switchport.access_carries(v) {
                return true;
            }
            continue;
        }
        if !p.switchport.allowed.contains(v) {
            continue;
        }
        let Some(peer) = t.peer(end) else { continue };
        let Some(n) = t.device(peer.device) else { continue };
        if !prunes(n) || n.vtp.domain != d.vtp.domain || needs(t, n, peer.port, v, seen) {
            return true;
        }
    }
    false
}

/// Whether VTP pruning stops flooding of VLAN `v` out of a switch's trunk port: pruning is on,
/// the VLAN is eligible there, and the switch at the other end (pruning in the same domain)
/// has no use for it.
pub fn pruned(t: &Topology, d: &Device, port: usize, v: VlanId) -> bool {
    if !prunes(d) || !prune_eligible(d, port, v) {
        return false;
    }
    let end = End { device: d.id, port };
    if !t.is_trunk(end) {
        return false;
    }
    let Some(peer) = t.peer(end) else { return false };
    let Some(n) = t.device(peer.device) else { return false };
    if !prunes(n) || n.vtp.domain != d.vtp.domain {
        return false;
    }
    let mut seen = BTreeSet::from([d.id]);
    !needs(t, n, peer.port, v, &mut seen)
}

/// The VLANs pruned on a trunk port, out of those it carries.
pub fn pruned_vlans(t: &Topology, d: &Device, port: usize) -> VlanSet {
    let mut set = VlanSet::none();
    let Some(p) = d.ports.get(port) else { return set };
    for v in d.vlans.keys().filter(|v| p.switchport.allowed.contains(**v)) {
        if pruned(t, d, port, *v) {
            set.insert(*v);
        }
    }
    set
}
