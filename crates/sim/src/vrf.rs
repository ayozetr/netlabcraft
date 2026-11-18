//! VRF-lite: a routing table, an ARP table and routing protocols for each VRF of a router.
//!
//! The interfaces of a VRF are left out of [`crate::l3_ifaces`], so everything that works on the
//! global table (connected routes, OSPF, RIP, EIGRP and BGP, local addresses) never sees them.
//! Frames that reach such an interface are still handled by the simulator, with the VRF as the
//! packet's context: the ARP table is the VRF's, an address is "ours" only in the VRF that holds it,
//! and the next hop comes from the VRF's own routing table.
//!
//! A VRF's routing table is computed by a simulation of its own, run on a copy of the lab as that
//! VRF sees it ([`view`]): the VRF's interfaces keep their addresses, the router's other
//! interfaces have none, and the router's static routes, OSPF process and BGP address family are
//! the VRF's. Neighbours that have no VRFs keep their own configuration, so a router that does not
//! know about VRFs can still be the other end of a VRF's link. That simulation moves the VRF's
//! hellos, adjacencies and sessions on the same cables, in step with the clock of the main one.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use netlabcraft_model::{Device, Topology};

use crate::{L3If, RouteEntry, Sim};

/// The simulation of one VRF and the lab as the VRF sees it.
#[derive(Debug, Clone)]
pub(crate) struct VrfSim {
    sim: Sim,
    topo: Topology,
}

/// The lab as the VRF `name` sees it: every device that has VRFs keeps the addresses of that
/// VRF's interfaces only and runs that VRF's routes and protocols; the others are unchanged.
pub fn view(t: &Topology, name: &str) -> Topology {
    let mut v = t.clone();
    for d in &mut v.devices {
        if d.vrfs.is_empty() {
            continue;
        }
        let mine = std::mem::take(&mut d.vrfs).into_iter().find(|x| x.name == name);
        let keep = |n: &str| mine.as_ref().is_some_and(|m| m.members.iter().any(|x| x == n));
        for p in &mut d.ports {
            if !keep(&p.name) {
                p.ip = None;
            }
            let base = p.name.clone();
            for s in &mut p.subifs {
                if !keep(&format!("{base}.{}", s.number)) {
                    s.ip = None;
                }
            }
        }
        for s in &mut d.svis {
            if !keep(&format!("Vlan{}", s.vlan)) {
                s.ip = None;
            }
        }
        for l in &mut d.loopbacks {
            if !keep(&l.name()) {
                l.ip = None;
            }
        }
        d.tunnels.clear();
        d.default_networks.clear();
        d.rip = None;
        d.eigrp = None;
        let (routes, ospf, bgp) = match mine {
            Some(m) => (m.routes, m.ospf, m.bgp),
            None => (Vec::new(), None, None),
        };
        d.routes = routes;
        d.ospf = ospf;
        d.bgp = bgp;
    }
    v
}

impl Sim {
    /// Keeps one simulation per VRF in step with the lab: made when a VRF appears, given the
    /// new lab when anything changed, dropped when the VRF is gone.
    pub(crate) fn vrf_sync(&mut self, t: &Topology) {
        let names: BTreeSet<String> = t.devices.iter().flat_map(|d| d.vrfs.iter().map(|v| v.name.clone())).collect();
        if names.is_empty() {
            self.vrf_subs.clear();
            self.vrf_src = None;
            return;
        }
        if self.vrf_src.as_ref() == Some(t) && names.iter().all(|n| self.vrf_subs.contains_key(n)) {
            return;
        }
        self.vrf_src = Some(t.clone());
        self.vrf_subs.retain(|n, _| names.contains(n));
        for name in names {
            let topo = view(t, &name);
            let mut sim = self.vrf_subs.remove(&name).map(|s| s.sim).unwrap_or_default();
            sim.topology_changed(&topo);
            self.vrf_subs.insert(name, VrfSim { sim, topo });
        }
    }

    /// Runs every VRF's simulation up to the main clock.
    pub(crate) fn vrf_follow(&mut self) {
        let now = self.now;
        for vs in self.vrf_subs.values_mut() {
            let VrfSim { sim, topo } = vs;
            if sim.now < now {
                let span = now.saturating_sub(sim.now);
                sim.run_for(topo, span);
            }
        }
    }

    /// Where a datagram to `dst` leaves a device in a VRF: the interface (marked with the VRF)
    /// and the next hop to ask for.
    pub(crate) fn vrf_route(&mut self, d: &Device, vrf: &str, src: Ipv4Addr, dst: Ipv4Addr) -> Option<(L3If, Ipv4Addr)> {
        let VrfSim { sim, topo } = self.vrf_subs.get_mut(vrf)?;
        let vd = topo.device(d.id)?;
        let (mut out, nh) = sim.route_flow(topo, vd, src, dst)?;
        out.vrf = Some(vrf.to_string());
        Some((out, nh))
    }

    /// A VRF's routing table, as `show ip route vrf NAME` lists it; `None` when the device has
    /// no such VRF.
    pub fn vrf_routing_table(&mut self, t: &Topology, d: &Device, vrf: &str) -> Option<Vec<RouteEntry>> {
        d.vrf(vrf)?;
        self.vrf_sync(t);
        let VrfSim { sim, topo } = self.vrf_subs.get_mut(vrf)?;
        let vd = topo.device(d.id)?;
        Some(sim.routing_table(topo, vd))
    }

    /// The VRF's own simulation and lab, for the commands that show what its protocols know.
    pub fn vrf_inner(&mut self, t: &Topology, vrf: &str) -> Option<(&mut Sim, &Topology)> {
        self.vrf_sync(t);
        let vs = self.vrf_subs.get_mut(vrf)?;
        Some((&mut vs.sim, &vs.topo))
    }
}
