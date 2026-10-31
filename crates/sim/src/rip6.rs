//! RIPng (RFC 2080): RIP for IPv6, computed from the topology like RIP (`rip`).
//!
//! A router runs it with `ipv6 router rip NAME` (and `ipv6 unicast-routing`), and interfaces
//! join with `ipv6 rip NAME enable`. Neighbours meet over link-local addresses on a shared
//! link; each router advertises the global prefixes of its RIPng interfaces and takes from each
//! neighbour every prefix one hop further, keeping the fewest hops (ties: lowest next hop),
//! never past 15; split horizon as in RIP. Routes lead to the neighbour's link-local address,
//! with RIPng's metric (hops + 1: a neighbour's own prefix is 2). Responses go to `FF02::9`,
//! UDP port 521, every 30 s (`control`), with poisoned reverse.

use std::collections::BTreeMap;
use std::net::Ipv6Addr;

use netlabcraft_core::{DeviceId, Ipv6Net};
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::ripng::{self, RIPNG_PORT, RIPNG_ROUTERS, RipngEntry};

use crate::v6::{L3If6, conf6, routes6, up_ifaces6};
use crate::{SVI_PORT, Sim};

/// Hop count meaning unreachable.
pub const INFINITY: u8 = 16;

/// A route RIPng installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rip6Route {
    pub net: Ipv6Net,
    pub via: Ipv6Addr,
    pub iface: String,
    /// RIPng's metric: hops + 1.
    pub metric: u8,
}

/// RIPng's view of the lab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rip6State {
    pub routes: BTreeMap<DeviceId, Vec<Rip6Route>>,
    /// Neighbours per router: (their device, their link-local address, our interface).
    pub neighbors: BTreeMap<DeviceId, Vec<(DeviceId, Ipv6Addr, String)>>,
    /// Interfaces running RIPng per router, with their global prefixes.
    pub interfaces: BTreeMap<DeviceId, Vec<(String, Vec<Ipv6Net>)>>,
}

/// Whether a router runs RIPng: configured, forwarding IPv6.
pub fn running(d: &Device) -> bool {
    d.rip6.is_some() && routes6(d)
}

fn ifaces(t: &Topology, d: &Device) -> Vec<L3If6> {
    let Some(name) = d.rip6.as_ref().map(|r| r.name.clone()) else { return Vec::new() };
    up_ifaces6(t, d).into_iter().filter(|i| conf6(d, i).and_then(|c| c.rip.as_ref()) == Some(&name)).collect()
}

/// One entry while computing: hops, and (our interface, next hop, neighbour) unless connected.
type Entry = (u8, Option<(String, Ipv6Addr, DeviceId)>);

pub fn compute(t: &Topology) -> Rip6State {
    let mut st = Rip6State::default();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| running(d)).collect();
    let mut ifs: BTreeMap<DeviceId, Vec<L3If6>> = BTreeMap::new();
    for d in &routers {
        let list = ifaces(t, d);
        st.interfaces.insert(
            d.id,
            list.iter()
                .map(|i| (i.name.clone(), i.addresses.iter().map(|a| a.network()).filter(|n| !Ipv6Net::is_link_local(n.addr)).collect()))
                .collect(),
        );
        ifs.insert(d.id, list);
    }
    let mut adj: BTreeMap<DeviceId, Vec<(String, Ipv6Addr, DeviceId)>> = BTreeMap::new();
    for a in &routers {
        for ia in ifs.get(&a.id).into_iter().flatten() {
            let r = crate::ospf6::reach(t, a, ia);
            for b in routers.iter().filter(|b| b.id != a.id) {
                for ib in ifs.get(&b.id).into_iter().flatten() {
                    let same_vlan = ib.vlan == ia.vlan || ia.port == SVI_PORT || ib.port == SVI_PORT;
                    if same_vlan && crate::ospf6::reaches(t, &r, b, ib) {
                        adj.entry(a.id).or_default().push((ia.name.clone(), ib.link_local, b.id));
                        st.neighbors.entry(a.id).or_default().push((b.id, ib.link_local, ia.name.clone()));
                    }
                }
            }
        }
    }
    let mut tables: BTreeMap<DeviceId, BTreeMap<Ipv6Net, Entry>> = BTreeMap::new();
    for (id, list) in &st.interfaces {
        let mut tbl = BTreeMap::new();
        for (_, nets) in list {
            for n in nets {
                tbl.insert(*n, (0, None));
            }
        }
        tables.insert(*id, tbl);
    }
    for _ in 0..=INFINITY {
        let snapshot = tables.clone();
        let mut changed = false;
        for d in &routers {
            let Some(tbl) = tables.get_mut(&d.id) else { continue };
            for (ifc, addr, from) in adj.get(&d.id).into_iter().flatten() {
                for (net, (hops, learned)) in snapshot.get(from).into_iter().flatten() {
                    if learned.as_ref().is_some_and(|(_, _, back)| *back == d.id) {
                        continue;
                    }
                    let h = hops.saturating_add(1);
                    if h >= INFINITY {
                        continue;
                    }
                    let better = match tbl.get(net) {
                        None => true,
                        Some((old, None)) => h < *old,
                        Some((old, Some((_, oa, _)))) => h < *old || (h == *old && *addr < *oa),
                    };
                    if better {
                        tbl.insert(*net, (h, Some((ifc.clone(), *addr, *from))));
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    for (id, tbl) in tables {
        let routes = tbl
            .into_iter()
            .filter_map(|(net, (hops, via))| via.map(|(iface, addr, _)| Rip6Route { net, via: addr, iface, metric: hops.saturating_add(1) }))
            .collect();
        st.routes.insert(id, routes);
    }
    st
}

/// A fingerprint of everything RIPng depends on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    crate::eigrp6::fingerprint(t).hash(&mut h);
    for d in &t.devices {
        format!("{:?}", d.rip6).hash(&mut h);
    }
    h.finish()
}

impl Sim {
    /// RIPng responses: every RIPng interface's prefixes and routes, from its link-local
    /// address to `FF02::9` (routes learned through the interface at metric 16).
    pub(crate) fn send_ripng(&mut self, t: &Topology, at: u64) {
        use netlabcraft_wire::ipv6::{ETHERTYPE_IPV6, Ipv6, multicast_mac};
        use netlabcraft_wire::{Frame, PROTO_UDP};
        if !t.devices.iter().any(|d| d.rip6.is_some()) {
            return;
        }
        let st = self.rip6_state(t);
        for (id, ifs) in &st.interfaces {
            let Some(d) = t.device(*id) else { continue };
            let up = up_ifaces6(t, d);
            for (name, _) in ifs {
                let Some(ifc) = up.iter().find(|i| i.name == *name) else { continue };
                if crate::loopback::is_loopback(ifc.port) {
                    continue;
                }
                let mut entries: Vec<RipngEntry> = ifs
                    .iter()
                    .filter(|(n, _)| n != name)
                    .flat_map(|(_, nets)| nets.iter().map(|n| RipngEntry { net: *n, tag: 0, metric: 1 }))
                    .collect();
                for r in st.routes.get(id).into_iter().flatten() {
                    entries.push(RipngEntry { net: r.net, tag: 0, metric: if r.iface == *name { INFINITY } else { r.metric } });
                }
                for chunk in entries.chunks(ripng::MAX_ENTRIES).take(16) {
                    let payload = netlabcraft_wire::dhcp6::udp6_encode(ifc.link_local, RIPNG_ROUTERS, RIPNG_PORT, RIPNG_PORT, &ripng::encode(chunk));
                    let ip = Ipv6 { hop_limit: 255, next_header: PROTO_UDP, src: ifc.link_local, dst: RIPNG_ROUTERS, payload };
                    let frame =
                        Frame { dst: multicast_mac(RIPNG_ROUTERS), src: ifc.mac, vlan: ifc.vlan, ethertype: ETHERTYPE_IPV6, payload: ip.encode() };
                    self.queue_frame(t, d, ifc.port, frame, at);
                }
            }
        }
    }
}
