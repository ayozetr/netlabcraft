//! VTP: switches in one domain share their VLAN database over trunks.
//!
//! Like spanning tree, the outcome is computed at once instead of by timed advertisements:
//! within each group of switches that hear each other (same domain and password, joined by up
//! trunks, possibly through transparent switches that pass advertisements on), the database
//! with the highest revision is copied to every server and client with a lower one. That
//! includes a client: plugging in a switch with a higher revision overwrites the domain's
//! VLANs, as on real networks. A switch with no domain yet takes the first one it hears.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use netlabcraft_core::DeviceId;
use netlabcraft_model::{DeviceKind, Topology, VTP_MAX_VLAN, VtpMode};

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
        let (revision, vlans) = (best.vtp.revision, best.vlans.clone());
        let vlans: BTreeMap<_, _> = vlans.into_iter().filter(|(v, _)| v.get() <= VTP_MAX_VLAN).collect();
        for id in members {
            if let Some(d) = t.device_mut(id)
                && d.vtp.revision < revision
            {
                d.vlans = vlans.clone();
                d.vtp.revision = revision;
                changed = true;
            }
        }
    }
    changed
}
