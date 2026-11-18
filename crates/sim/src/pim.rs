//! Multicast routing with PIM (RFC 7761): neighbours, the trees that join and prune messages
//! would build, and multicast data forwarded over them.
//!
//! Like the unicast protocols here, the trees are worked out from the lab as if converged: a
//! group's receivers are the hosts that joined it on a router's interface (found through the
//! cables and switches, whatever order things were configured in) and the interfaces with
//! `ip igmp join-group`; every router with a receiver below it joins towards the rendezvous
//! point (a tree for `(*,G)`) or towards the source (a tree for `(S,G)`) along the path the
//! unicast routing table gives, and a router with no interest below it is pruned. Hellos,
//! join/prune messages and registers are sent for real so captures show them; the state is not
//! kept from them.
//!
//! What the shortcut leaves out: a router starts on the shortest-path tree at the first packet
//! (the switchover threshold is zero), so packets never go through the rendezvous point on the
//! shared tree and the register only tells the point a source exists; there is no assert
//! election (two routers forwarding to one LAN), no bootstrap router or auto-RP, and the DR
//! election needs no hello to have been heard.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::pim::{PimIface, PimMode};
use netlabcraft_model::{Device, End, Topology};
use netlabcraft_wire::control::ipv4_multicast_mac;
use netlabcraft_wire::igmp::{ALL_HOSTS, Igmp, PROTO_IGMP};
use netlabcraft_wire::pim::{ALL_PIM_ROUTERS, Hello, JOIN_PRUNE_HOLDTIME_SECS, JoinPrune, JpGroup, JpSource, PROTO_PIM, Pim};
use netlabcraft_wire::{ETHERTYPE_IPV4, Frame, Ipv4};

use crate::{DebugKind, L3If, SVI_PORT, Sim, l3_ifaces, up_ifaces};

/// Hellos go out this often (RFC 7761 §4.11).
pub const HELLO_EVERY_US: u64 = 30_000_000;
/// Joins are repeated this often.
pub const JOIN_EVERY_US: u64 = 60_000_000;
/// Most (source, group) pairs tracked.
pub const MAX_SOURCES: usize = 1_024;
/// Rounds of joins that travel upstream at most (a path is shorter than this).
const MAX_ROUNDS: usize = 32;

/// What a router has counted for one (source, group).
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    forwarded: u64,
    bytes: u64,
    rpf_failed: u64,
}

/// What the simulation remembers about multicast routing.
#[derive(Debug, Clone, Default)]
pub struct State {
    sources: BTreeSet<(Ipv4Addr, Ipv4Addr)>,
    counts: BTreeMap<(DeviceId, Ipv4Addr, Ipv4Addr), Counts>,
    /// When each entry was first seen: (router, source, group).
    born: BTreeMap<(DeviceId, Option<Ipv4Addr>, Ipv4Addr), u64>,
    /// When each neighbour was first seen: (router, neighbour address).
    nbr_born: BTreeMap<(DeviceId, Ipv4Addr), u64>,
}

/// A neighbour on an interface.
#[derive(Debug, Clone)]
struct Nbr {
    dev: DeviceId,
    addr: Ipv4Addr,
    iface: String,
    prio: u32,
}

/// An interface that runs PIM.
#[derive(Debug, Clone)]
struct IfInfo {
    l3: L3If,
    cfg: PimIface,
    nbrs: Vec<Nbr>,
    /// Groups with a member on the link, and who reported them (`None`: this router's own join).
    members: BTreeMap<Ipv4Addr, Option<Ipv4Addr>>,
    /// The designated router of the link.
    dr: Ipv4Addr,
}

/// Every router that routes multicast, with its PIM interfaces.
#[derive(Debug, Clone, Default)]
struct Net {
    ifs: BTreeMap<DeviceId, Vec<IfInfo>>,
}

impl Net {
    fn iface(&self, d: DeviceId, name: &str) -> Option<&IfInfo> {
        self.ifs.get(&d)?.iter().find(|i| i.l3.name == name)
    }

    fn groups(&self) -> BTreeSet<Ipv4Addr> {
        self.ifs.values().flatten().flat_map(|i| i.members.keys().copied()).collect()
    }
}

/// One router's place in a tree towards a target (the rendezvous point or a source).
#[derive(Debug, Clone, Default)]
struct Node {
    /// The interface the target is reached through.
    iif: Option<String>,
    /// The neighbour towards the target: its device, address and the name of its interface.
    upstream: Option<(DeviceId, Ipv4Addr, String)>,
    /// The router has the target's address.
    owner: bool,
    /// The target is a host on a link of this router.
    connected: bool,
    oifs: BTreeSet<String>,
}

/// One row of `show ip pim neighbor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborRow {
    pub iface: String,
    pub addr: Ipv4Addr,
    pub up_us: u64,
    pub priority: u32,
    pub dr: bool,
}

/// One row of `show ip pim interface`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfaceRow {
    pub iface: String,
    pub addr: Ipv4Addr,
    pub mode: PimMode,
    pub neighbors: usize,
    pub priority: u32,
    pub dr: Ipv4Addr,
}

/// One outgoing interface of a multicast route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oif {
    pub iface: String,
    pub sparse: bool,
}

/// One entry of the multicast routing table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MrouteRow {
    /// `None` for a `(*,G)` entry.
    pub source: Option<Ipv4Addr>,
    pub group: Ipv4Addr,
    pub rp: Option<Ipv4Addr>,
    pub iif: Option<String>,
    pub rpf_nbr: Option<Ipv4Addr>,
    pub oifs: Vec<Oif>,
    /// The interface the entry arrived on runs sparse mode.
    pub sparse: bool,
    pub members: bool,
    pub first_hop: bool,
    pub owner: bool,
    pub age_us: u64,
    pub forwarded: u64,
    pub bytes: u64,
    pub rpf_failed: u64,
}

/// What `show ip rpf` says about an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpfRow {
    /// The interface multicast from the address must arrive on.
    pub iface: String,
    /// The neighbour behind it (none when the address is on the link).
    pub neighbor: Option<Ipv4Addr>,
    /// The router has the address itself.
    pub owner: bool,
    /// PIM runs on the interface.
    pub pim: bool,
}

/// One row of `show ip igmp groups` on a router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgmpRow {
    pub iface: String,
    pub group: Ipv4Addr,
    pub reporter: Option<Ipv4Addr>,
}

/// Whether a group runs in sparse mode on an interface: its own mode, or for sparse-dense mode
/// whether the router knows a rendezvous point for the group.
fn sparse_on(d: &Device, cfg: &PimIface, group: Ipv4Addr) -> bool {
    match cfg.mode {
        PimMode::Sparse => true,
        PimMode::Dense => false,
        PimMode::SparseDense => d.pim.rp_for(&d.acls, group).is_some(),
    }
}

/// The routers that route multicast, their PIM interfaces, neighbours and members.
fn pim_net(t: &Topology) -> Net {
    let mut ifs: BTreeMap<DeviceId, Vec<IfInfo>> = BTreeMap::new();
    let routers: Vec<&Device> = t.devices.iter().filter(|d| d.routes_ip() && d.pim.routing).collect();
    for d in &routers {
        let mut list = Vec::new();
        for l3 in up_ifaces(t, d) {
            if crate::loopback::is_loopback(l3.port) || crate::vpn::is_tunnel(l3.port) {
                // A loopback runs PIM so that it can hold a rendezvous point address.
                if let Some(cfg) = d.pim.iface(&l3.name)
                    && crate::loopback::is_loopback(l3.port)
                {
                    let members = d.pim.joins.get(&l3.name).into_iter().flatten().map(|g| (*g, None)).collect();
                    list.push(IfInfo { dr: l3.net.addr, l3, cfg: cfg.clone(), nbrs: Vec::new(), members });
                }
                continue;
            }
            let Some(cfg) = d.pim.iface(&l3.name) else { continue };
            list.push(IfInfo { dr: l3.net.addr, l3, cfg: cfg.clone(), nbrs: Vec::new(), members: BTreeMap::new() });
        }
        ifs.insert(d.id, list);
    }
    // Neighbours, members and the designated router, link by link.
    let snapshot = ifs.clone();
    let hosts: Vec<&Device> = t.devices.iter().filter(|h| h.kind.is_host() && !h.igmp.joined.is_empty()).collect();
    for d in &routers {
        let Some(mine) = ifs.get_mut(&d.id) else { continue };
        for info in mine.iter_mut() {
            if crate::loopback::is_loopback(info.l3.port) {
                continue;
            }
            let reach = crate::ospf::iface_reach(t, d, &info.l3);
            for b in &routers {
                if b.id == d.id {
                    continue;
                }
                for ib in snapshot.get(&b.id).into_iter().flatten() {
                    if ib.l3.net.network() == info.l3.net.network()
                        && (ib.l3.vlan == info.l3.vlan || info.l3.port == SVI_PORT || ib.l3.port == SVI_PORT)
                        && crate::ospf::reaches(t, &reach, b, &ib.l3)
                    {
                        info.nbrs.push(Nbr { dev: b.id, addr: ib.l3.net.addr, iface: ib.l3.name.clone(), prio: ib.cfg.dr_priority });
                    }
                }
            }
            for g in d.pim.joins.get(&info.l3.name).into_iter().flatten() {
                info.members.insert(*g, None);
            }
            for h in &hosts {
                for hi in up_ifaces(t, h) {
                    if hi.net.network() == info.l3.net.network() && hi.port != SVI_PORT && reach.contains(&End { device: h.id, port: hi.port }) {
                        for g in h.igmp.joined.iter().filter(|g| netlabcraft_model::igmp::Igmp::joinable(**g)) {
                            info.members.entry(*g).or_insert(Some(hi.net.addr));
                        }
                    }
                }
            }
            let best = info.nbrs.iter().map(|n| (n.prio, n.addr)).chain(std::iter::once((info.cfg.dr_priority, info.l3.net.addr))).max();
            if let Some((_, a)) = best {
                info.dr = a;
            }
        }
    }
    Net { ifs }
}

impl Sim {
    /// The routers' trees towards `target` for `group`: where each reaches it, and which of their
    /// interfaces lead to a receiver. `star` is the shared tree towards a rendezvous point.
    fn pim_tree(&mut self, t: &Topology, net: &Net, target: Ipv4Addr, group: Ipv4Addr, star: bool) -> BTreeMap<DeviceId, Node> {
        let mut nodes: BTreeMap<DeviceId, Node> = BTreeMap::new();
        for (id, ifs) in &net.ifs {
            let Some(d) = t.device(*id) else { continue };
            let mut n = Node::default();
            if l3_ifaces(d).iter().any(|i| i.net.addr == target) {
                n.owner = true;
            } else if let Some((out, nh)) = self.route_flow(t, d, Ipv4Addr::UNSPECIFIED, target)
                && ifs.iter().any(|i| i.l3.name == out.name)
            {
                n.iif = Some(out.name.clone());
                let up = ifs.iter().find(|i| i.l3.name == out.name).and_then(|i| i.nbrs.iter().find(|x| x.addr == nh));
                match up {
                    Some(x) => n.upstream = Some((x.dev, x.addr, x.iface.clone())),
                    None => n.connected = !star && out.net.contains(target),
                }
            }
            for info in ifs {
                if n.iif.as_deref() == Some(info.l3.name.as_str()) || info.dr != info.l3.net.addr || !info.members.contains_key(&group) {
                    continue;
                }
                let sparse = sparse_on(d, &info.cfg, group);
                if (star && !sparse) || (sparse && d.pim.rp_for(&d.acls, group).is_none()) {
                    continue;
                }
                n.oifs.insert(info.l3.name.clone());
            }
            nodes.insert(*id, n);
        }
        // Joins travel upstream until nothing new is joined.
        for _ in 0..MAX_ROUNDS {
            let ups: Vec<(DeviceId, String)> = nodes
                .values()
                .filter(|n| !n.owner && !n.oifs.is_empty())
                .filter_map(|n| n.upstream.as_ref().map(|(u, _, name)| (*u, name.clone())))
                .collect();
            let mut changed = false;
            for (u, name) in ups {
                if let Some(un) = nodes.get_mut(&u)
                    && un.iif.as_deref() != Some(name.as_str())
                {
                    changed |= un.oifs.insert(name);
                }
            }
            if !changed {
                break;
            }
        }
        nodes
    }

    /// One line of `debug ip mpacket`.
    fn mdebug(&mut self, d: &Device, text: &str) {
        if self.debug_on(d.id, DebugKind::Mpacket) {
            self.debug_log(d.id, DebugKind::Mpacket, text);
        }
    }

    /// Multicast data arriving at a router: checked against the reverse path, delivered to the
    /// router's own joins and forwarded out of the interfaces its tree has.
    pub(crate) fn mcast_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: Ipv4) -> Result<(), String> {
        if ip.protocol == PROTO_IGMP {
            return Ok(());
        }
        if !d.pim.routing {
            self.bump(d.id, "multicast dropped: routing is off");
            self.mdebug(d, &format!("{} to {} dropped: multicast routing is off", ip.src, ip.dst));
            return Err("multicast routing is off here (`ip multicast-routing`)".into());
        }
        let net = pim_net(t);
        let Some(info) = net.iface(d.id, &ifc.name) else {
            self.bump(d.id, "multicast dropped: no PIM on the interface");
            let why = format!("PIM is not running on {}", ifc.name);
            self.mdebug(d, &format!("{} to {} dropped: {why}", ip.src, ip.dst));
            return Err(why);
        };
        let (src, group) = (ip.src, ip.dst);
        let sparse = sparse_on(d, &info.cfg, group);
        let fresh = !self.pim.sources.contains(&(src, group));
        if fresh && self.pim.sources.len() < MAX_SOURCES {
            self.pim.sources.insert((src, group));
        }
        let nodes = self.pim_tree(t, &net, src, group, false);
        let node = nodes.get(&d.id).cloned().unwrap_or_default();
        let key = (d.id, src, group);
        if node.iif.as_deref() != Some(ifc.name.as_str()) {
            let n = self.pim.counts.entry(key).or_default();
            n.rpf_failed = n.rpf_failed.saturating_add(1);
            self.bump(d.id, "multicast dropped: reverse path check");
            let way = node.iif.map_or("no route".to_string(), |i| format!("through {i}"));
            let why = format!("reverse path check failed: {src} is reached {way}, not through {}", ifc.name);
            self.mdebug(d, &format!("{src} to {group} dropped on {}: {why}", ifc.name));
            return Err(why);
        }
        if sparse && node.connected {
            let rp = d.pim.rp_for(&d.acls, group);
            let Some(rp) = rp else {
                self.bump(d.id, "multicast dropped: no rendezvous point");
                let why = format!("no rendezvous point for {group}: the source cannot register");
                self.mdebug(d, &format!("{src} to {group} dropped on {}: {why}", ifc.name));
                return Err(why);
            };
            let reachable = l3_ifaces(d).iter().any(|i| i.net.addr == rp) || self.route_flow(t, d, Ipv4Addr::UNSPECIFIED, rp).is_some();
            if !reachable {
                self.bump(d.id, "multicast dropped: rendezvous point unreachable");
                let why = format!("the rendezvous point {rp} is unreachable");
                self.mdebug(d, &format!("{src} to {group} dropped on {}: {why}", ifc.name));
                return Err(why);
            }
            if fresh {
                self.pim_register(t, d, ifc, rp, &ip);
            }
        }
        let wire_len = u64::try_from(ip.payload.len().saturating_add(20)).unwrap_or(u64::MAX);
        let n = self.pim.counts.entry(key).or_default();
        n.forwarded = n.forwarded.saturating_add(1);
        n.bytes = n.bytes.saturating_add(wire_len);
        if d.pim.has_joined(group) {
            let _ = self.local_in(t, d, ifc, ip.clone());
        }
        if ip.ttl <= 1 {
            self.bump(d.id, "TTL expired");
            self.mdebug(d, &format!("{src} to {group} dropped on {}: TTL expired", ifc.name));
            return Err("TTL expired in transit".into());
        }
        let up = up_ifaces(t, d);
        let mut sent = 0;
        for name in &node.oifs {
            let Some(out) = up.iter().find(|i| i.name == *name) else { continue };
            if d.pim.iface(name).is_some_and(|c| c.ttl_threshold > 0 && ip.ttl <= c.ttl_threshold) {
                self.bump(d.id, "multicast held back by a TTL threshold");
                continue;
            }
            let fwd = Ipv4 { ttl: ip.ttl - 1, ..ip.clone() };
            let f = Frame { dst: ipv4_multicast_mac(group), src: out.mac, vlan: out.vlan, ethertype: ETHERTYPE_IPV4, payload: fwd.encode() };
            self.l3_emit(t, d, out, f);
            sent += 1;
        }
        if sent > 0 {
            self.bump(d.id, "multicast forwarded");
        }
        let outs: Vec<&str> = node.oifs.iter().map(String::as_str).collect();
        let text = if outs.is_empty() {
            format!("{src} to {group} arrived on {}, no interface wants it", ifc.name)
        } else {
            format!("{src} to {group} arrived on {}, forwarded out {}", ifc.name, outs.join(", "))
        };
        self.mdebug(d, &text);
        Ok(())
    }

    /// The first router of a source tells the rendezvous point the source exists.
    fn pim_register(&mut self, t: &Topology, d: &Device, ifc: &L3If, rp: Ipv4Addr, ip: &Ipv4) {
        if l3_ifaces(d).iter().any(|i| i.net.addr == rp) {
            return;
        }
        let msg = Pim::Register { border: false, null: false, data: ip.encode() };
        let reg = Ipv4 { id: self.ip_id(), ttl: 64, protocol: PROTO_PIM, src: ifc.net.addr, dst: rp, payload: msg.encode() };
        let _ = self.send_ip(t, d, reg);
    }

    /// A PIM message addressed to this router: a register is answered with a register-stop when
    /// no receiver is behind the rendezvous point.
    pub(crate) fn pim_unicast_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4) -> Result<(), String> {
        let Ok(msg) = Pim::decode(&ip.payload) else { return Err("malformed PIM message".into()) };
        self.bump(d.id, "PIM messages received");
        if self.debug_on(d.id, DebugKind::Pim) {
            let line = format!("{} from {}", msg.describe(), ip.src);
            self.debug_log(d.id, DebugKind::Pim, &line);
        }
        let Pim::Register { null: false, data, .. } = msg else { return Ok(()) };
        let inner = Ipv4::decode(&data).map_err(|e| e.to_string())?;
        if !d.pim.routing || !inner.dst.is_multicast() {
            return Ok(());
        }
        let net = pim_net(t);
        let rp = d.pim.rp_for(&d.acls, inner.dst);
        let wanted = rp.is_some_and(|rp| self.pim_tree(t, &net, rp, inner.dst, true).get(&d.id).is_some_and(|n| !n.oifs.is_empty()));
        if !wanted {
            let stop = Pim::RegisterStop { group: inner.dst, source: inner.src };
            let reply = Ipv4 { id: self.ip_id(), ttl: 64, protocol: PROTO_PIM, src: ip.dst, dst: ip.src, payload: stop.encode() };
            let _ = self.send_ip(t, d, reply);
        }
        Ok(())
    }

    // ---- what the show commands print ----

    /// `show ip pim neighbor`.
    pub fn pim_neighbors(&mut self, t: &Topology, dev: DeviceId) -> Vec<NeighborRow> {
        let net = pim_net(t);
        let now = self.now;
        let mut rows = Vec::new();
        for info in net.ifs.get(&dev).into_iter().flatten() {
            for n in &info.nbrs {
                let born = *self.pim.nbr_born.entry((dev, n.addr)).or_insert(now);
                rows.push(NeighborRow {
                    iface: info.l3.name.clone(),
                    addr: n.addr,
                    up_us: now.saturating_sub(born),
                    priority: n.prio,
                    dr: info.dr == n.addr,
                });
            }
        }
        rows
    }

    /// `show ip pim interface`.
    pub fn pim_interfaces(&self, t: &Topology, dev: DeviceId) -> Vec<IfaceRow> {
        let net = pim_net(t);
        net.ifs
            .get(&dev)
            .into_iter()
            .flatten()
            .map(|i| IfaceRow {
                iface: i.l3.name.clone(),
                addr: i.l3.net.addr,
                mode: i.cfg.mode,
                neighbors: i.nbrs.len(),
                priority: i.cfg.dr_priority,
                dr: i.dr,
            })
            .collect()
    }

    /// `show ip rpf ADDRESS`: where multicast from an address has to arrive.
    pub fn pim_rpf(&mut self, t: &Topology, dev: DeviceId, target: Ipv4Addr) -> Option<RpfRow> {
        let d = t.device(dev)?;
        if l3_ifaces(d).iter().any(|i| i.net.addr == target) {
            return Some(RpfRow { iface: String::new(), neighbor: None, owner: true, pim: true });
        }
        let net = pim_net(t);
        let (out, nh) = self.route_flow(t, d, Ipv4Addr::UNSPECIFIED, target)?;
        let info = net.iface(dev, &out.name);
        let neighbor = info.and_then(|i| i.nbrs.iter().find(|x| x.addr == nh)).map(|x| x.addr);
        Some(RpfRow { iface: out.name, neighbor, owner: false, pim: info.is_some() })
    }

    /// `show ip igmp groups` on a router: the groups with a member on each interface.
    pub fn router_igmp_groups(&self, t: &Topology, dev: DeviceId) -> Vec<IgmpRow> {
        let net = pim_net(t);
        net.ifs
            .get(&dev)
            .into_iter()
            .flatten()
            .flat_map(|i| i.members.iter().map(|(g, r)| IgmpRow { iface: i.l3.name.clone(), group: *g, reporter: *r }))
            .collect()
    }

    /// The router's multicast routing table: a `(*,G)` entry for each group with a receiver below
    /// the router, and an `(S,G)` entry for each source whose packets it has handled.
    pub fn mroute_table(&mut self, t: &Topology, dev: DeviceId) -> Vec<MrouteRow> {
        let net = pim_net(t);
        let Some(d) = t.device(dev) else { return Vec::new() };
        let Some(mine) = net.ifs.get(&dev) else { return Vec::new() };
        let now = self.now;
        let mut rows = Vec::new();
        let has_members = |oifs: &BTreeSet<String>, g: Ipv4Addr| {
            mine.iter().any(|i| oifs.contains(&i.l3.name) && i.members.contains_key(&g) && i.dr == i.l3.net.addr)
        };
        for g in net.groups() {
            let Some(rp) = d.pim.rp_for(&d.acls, g) else { continue };
            let nodes = self.pim_tree(t, &net, rp, g, true);
            let Some(n) = nodes.get(&dev) else { continue };
            if n.oifs.is_empty() {
                continue;
            }
            let sparse_oifs =
                n.oifs.iter().filter_map(|o| mine.iter().find(|i| i.l3.name == *o)).map(|i| Oif { iface: i.l3.name.clone(), sparse: true }).collect();
            rows.push(MrouteRow {
                source: None,
                group: g,
                rp: Some(rp),
                iif: n.iif.clone(),
                rpf_nbr: n.upstream.as_ref().map(|u| u.1),
                oifs: sparse_oifs,
                sparse: true,
                members: has_members(&n.oifs, g),
                first_hop: false,
                owner: n.owner,
                age_us: 0,
                forwarded: 0,
                bytes: 0,
                rpf_failed: 0,
            });
        }
        let seen: Vec<(Ipv4Addr, Ipv4Addr)> =
            self.pim.sources.iter().copied().filter(|(s, g)| self.pim.counts.contains_key(&(dev, *s, *g))).collect();
        for (s, g) in seen {
            let nodes = self.pim_tree(t, &net, s, g, false);
            let Some(n) = nodes.get(&dev) else { continue };
            let c = self.pim.counts.get(&(dev, s, g)).copied().unwrap_or_default();
            let sparse = n.iif.as_deref().and_then(|name| mine.iter().find(|i| i.l3.name == name)).is_some_and(|i| sparse_on(d, &i.cfg, g));
            let oifs = n
                .oifs
                .iter()
                .filter_map(|o| mine.iter().find(|i| i.l3.name == *o))
                .map(|i| Oif { iface: i.l3.name.clone(), sparse: sparse_on(d, &i.cfg, g) })
                .collect();
            rows.push(MrouteRow {
                source: Some(s),
                group: g,
                rp: d.pim.rp_for(&d.acls, g),
                iif: n.iif.clone(),
                rpf_nbr: n.upstream.as_ref().map(|u| u.1),
                oifs,
                sparse,
                members: has_members(&n.oifs, g),
                first_hop: n.connected,
                owner: n.owner,
                age_us: 0,
                forwarded: c.forwarded,
                bytes: c.bytes,
                rpf_failed: c.rpf_failed,
            });
        }
        // How long each entry has existed; entries that went away start again.
        let present: BTreeSet<(Option<Ipv4Addr>, Ipv4Addr)> = rows.iter().map(|r| (r.source, r.group)).collect();
        self.pim.born.retain(|(r, s, g), _| *r != dev || present.contains(&(*s, *g)));
        for r in &mut rows {
            let born = *self.pim.born.entry((dev, r.source, r.group)).or_insert(now);
            r.age_us = now.saturating_sub(born);
        }
        rows
    }

    /// `clear ip mroute`: forgets the sources and counters this router holds.
    pub fn clear_mroute(&mut self, dev: DeviceId) {
        self.pim.counts.retain(|(r, _, _), _| *r != dev);
        self.pim.born.retain(|(r, _, _), _| *r != dev);
        let still: BTreeSet<(Ipv4Addr, Ipv4Addr)> = self.pim.counts.keys().map(|(_, s, g)| (*s, *g)).collect();
        self.pim.sources.retain(|k| still.contains(k));
    }

    // ---- control traffic ----

    /// Hellos from every PIM interface.
    pub(crate) fn send_pim_hellos(&mut self, t: &Topology, at: u64) {
        let net = pim_net(t);
        let mut jobs: Vec<(DeviceId, L3If, Vec<u8>)> = Vec::new();
        for (id, ifs) in &net.ifs {
            let Some(d) = t.device(*id) else { continue };
            let generation = d.name.bytes().fold(0x5049_4d00u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
            for info in ifs.iter().filter(|i| !crate::loopback::is_loopback(i.l3.port)) {
                let hello = Pim::Hello(Hello::new(info.cfg.dr_priority, generation));
                let ip = Ipv4 { id: 0, ttl: 1, protocol: PROTO_PIM, src: info.l3.net.addr, dst: ALL_PIM_ROUTERS, payload: hello.encode() };
                jobs.push((*id, info.l3.clone(), ip.encode()));
            }
        }
        for (id, ifc, bytes) in jobs {
            if let Some(d) = t.device(id) {
                self.queue_l3(t, d, &ifc, ifc.mac, ipv4_multicast_mac(ALL_PIM_ROUTERS), bytes, at);
            }
        }
    }

    /// General queries from every router interface that runs PIM (the router with the lowest
    /// address on a link is its querier).
    pub(crate) fn send_router_igmp_queries(&mut self, t: &Topology, at: u64) {
        let net = pim_net(t);
        let mut jobs: Vec<(DeviceId, L3If, Vec<u8>)> = Vec::new();
        for (id, ifs) in &net.ifs {
            for info in ifs.iter().filter(|i| !crate::loopback::is_loopback(i.l3.port)) {
                if info.nbrs.iter().any(|n| n.addr < info.l3.net.addr) {
                    continue;
                }
                let ip =
                    Ipv4 { id: 0, ttl: 1, protocol: PROTO_IGMP, src: info.l3.net.addr, dst: ALL_HOSTS, payload: Igmp::general_query(100).encode() };
                jobs.push((*id, info.l3.clone(), ip.encode()));
            }
        }
        for (id, ifc, bytes) in jobs {
            if let Some(d) = t.device(id) {
                self.queue_l3(t, d, &ifc, ifc.mac, ipv4_multicast_mac(ALL_HOSTS), bytes, at);
            }
        }
    }

    /// Join/prune messages: each router repeats the joins it has sent towards the neighbour that
    /// leads to the rendezvous point or the source.
    pub(crate) fn send_pim_joins(&mut self, t: &Topology, at: u64) {
        // (router, interface, upstream neighbour) to group to joined entries.
        type Key = (DeviceId, String, Ipv4Addr);
        let net = pim_net(t);
        if net.ifs.is_empty() {
            return;
        }
        let mut plan: BTreeMap<Key, BTreeMap<Ipv4Addr, Vec<JpSource>>> = BTreeMap::new();
        let sources: Vec<(Ipv4Addr, Ipv4Addr)> = self.pim.sources.iter().copied().collect();
        for g in net.groups() {
            let rps: BTreeSet<Ipv4Addr> = net.ifs.keys().filter_map(|id| t.device(*id)).filter_map(|d| d.pim.rp_for(&d.acls, g)).collect();
            for rp in rps {
                for (id, n) in self.pim_tree(t, &net, rp, g, true) {
                    if let (false, false, Some(iif), Some((_, addr, _))) = (n.owner, n.oifs.is_empty(), &n.iif, &n.upstream) {
                        plan.entry((id, iif.clone(), *addr)).or_default().entry(g).or_default().push(JpSource::star(rp));
                    }
                }
            }
            for (s, _) in sources.iter().filter(|(_, sg)| *sg == g) {
                for (id, n) in self.pim_tree(t, &net, *s, g, false) {
                    let sparse = t.device(id).zip(n.iif.as_deref().and_then(|i| net.iface(id, i))).is_some_and(|(d, i)| sparse_on(d, &i.cfg, g));
                    if let (true, false, false, Some(iif), Some((_, addr, _))) = (sparse, n.owner, n.oifs.is_empty(), &n.iif, &n.upstream) {
                        plan.entry((id, iif.clone(), *addr)).or_default().entry(g).or_default().push(JpSource::source(*s));
                    }
                }
            }
        }
        for ((id, iface, upstream), groups) in plan {
            let (Some(d), Some(info)) = (t.device(id), net.iface(id, &iface)) else { continue };
            let jp = JoinPrune {
                upstream,
                holdtime: JOIN_PRUNE_HOLDTIME_SECS,
                groups: groups.into_iter().map(|(group, joins)| JpGroup { group, joins, prunes: Vec::new() }).collect(),
            };
            let ip = Ipv4 { id: 0, ttl: 1, protocol: PROTO_PIM, src: info.l3.net.addr, dst: ALL_PIM_ROUTERS, payload: Pim::JoinPrune(jp).encode() };
            self.queue_l3(t, d, &info.l3, info.l3.mac, ipv4_multicast_mac(ALL_PIM_ROUTERS), ip.encode(), at);
        }
    }
}
