//! VRRP (RFC 5798, RFC 3768) and GLBP: routers sharing a gateway address, computed from the
//! topology like HSRP (`hsrp`).
//!
//! **VRRP.** Among the up interfaces of one subnet carrying the same virtual router id, the one
//! whose own address is the virtual address is its owner (priority 255) and always master;
//! otherwise the highest priority, then the highest address, wins. Preemption is on by default
//! (§6.1): the simulator remembers the master and keeps it while it stays unless a better router
//! may preempt (`no vrrp N preempt` stops that; an owner always preempts). The master answers
//! ARP for the virtual address with `00-00-5E-00-01-{VRID}` and takes frames sent to it; it
//! accepts packets for the virtual address itself only when it owns it (accept mode off,
//! §6.4.3). Advertisements go every second to `224.0.0.18` (`control`), in RFC 3768's version 2
//! format, or RFC 5798's version 3 with `fhrp version vrrp v3`, at the group's advertisement
//! interval (`vrrp N timers advertise`; drawn at most once a second).
//!
//! **Timers** run in simulated time: when the master changes, the new one takes over only
//! after the Master_Down_Interval (3 × the advertisement interval plus the skew time, which is
//! shorter the higher the priority) has passed without advertisements from the old one; when
//! the old master said goodbye (priority 0: its interface shut down or the group removed), after
//! the skew time alone (RFC 5798 §6.4.2). Until then the old master keeps forwarding if it is
//! still there, else the group has none. A preempting backup also waits that long. `vrrp N
//! track INTERFACE [decrement D]` lowers the priority while the interface is down.
//!
//! **VRRPv3 for IPv6** (`vrrp N ipv6 FE80::…` with `fhrp version vrrp v3`): the master answers
//! neighbour solicitations for the virtual link-local address with `00-00-5E-00-02-{VRID}`
//! and forwards what hosts send there; advertisements go to `FF02::12`.
//!
//! **GLBP.** One gateway address, several routers forwarding. The highest priority (then the
//! highest address) is the active virtual gateway (AVG; preemption off unless `glbp N
//! preempt`, as with VRRP's memory of who held the role); up to four members are virtual
//! forwarders (AVF), each with its own virtual MAC `0007.b4GG.GGFF` (group, forwarder number,
//! numbered by address). The AVG answers each ARP request for the gateway address with one of
//! the forwarders' MACs, in turn (round-robin) or by the asking host (host-dependent), so hosts
//! spread over the routers; each forwarder takes the frames sent to its MAC. GLBP's message
//! format is not public, so its hellos are not drawn on the wire.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use netlabcraft_core::{DeviceId, Ipv4Net, Ipv6Net, MacAddr};
use netlabcraft_model::fhrp::{Glbp, GlbpBalance, Vrrp};
use netlabcraft_model::{Device, DeviceKind, End, Topology};

use crate::hsrp::{Actives, keep_active};
use crate::v6::L3If6;
use crate::{L3If, Sim, port_up, up_ifaces};

/// Most GLBP forwarders per group.
pub const MAX_FORWARDERS: usize = 4;
/// Most VRRP groups waiting for a master at once.
const MAX_WAITS: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VrrpState {
    Master,
    Backup,
}

impl VrrpState {
    pub fn name(self) -> &'static str {
        match self {
            VrrpState::Master => "master",
            VrrpState::Backup => "backup",
        }
    }
}

/// One router's place in one VRRP group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrrpRole {
    pub iface: String,
    pub group: u8,
    pub virtual_ip: Ipv4Addr,
    pub state: VrrpState,
    /// 255 for the owner; lowered by tracked interfaces that are down.
    pub priority: u8,
    /// The configured priority.
    pub configured: u8,
    pub preempt: bool,
    pub owner: bool,
    pub master: Option<Ipv4Addr>,
    /// The interface's own address (the advertisements' source).
    pub address: Ipv4Addr,
    pub subnet: Ipv4Net,
    /// The advertisement interval, centiseconds.
    pub interval_cs: u16,
    /// Tracked interfaces that are down.
    pub tracked_down: Vec<String>,
    /// The group is waiting for a new master until then (µs of simulated time).
    pub waiting_until: Option<u64>,
}

impl VrrpRole {
    pub fn virtual_mac(&self) -> MacAddr {
        netlabcraft_wire::vrrp::virtual_mac(self.group)
    }
}

/// One router's place in one VRRPv3 group for IPv6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vrrp6Role {
    pub iface: String,
    pub group: u8,
    /// The virtual link-local address.
    pub virtual_ip: Ipv6Addr,
    /// The link's first global prefix (tells groups on different links apart).
    pub prefix: Option<Ipv6Net>,
    pub state: VrrpState,
    pub priority: u8,
    pub configured: u8,
    pub preempt: bool,
    pub owner: bool,
    /// The master's link-local address.
    pub master: Option<Ipv6Addr>,
    /// This interface's link-local address (the advertisements' source).
    pub address: Ipv6Addr,
    pub interval_cs: u16,
    pub tracked_down: Vec<String>,
    pub waiting_until: Option<u64>,
}

impl Vrrp6Role {
    pub fn virtual_mac(&self) -> MacAddr {
        netlabcraft_wire::vrrp::virtual_mac6(self.group)
    }
}

/// One router's place in one GLBP group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlbpRole {
    pub iface: String,
    pub group: u16,
    pub virtual_ip: Ipv4Addr,
    /// Whether this router is the active virtual gateway.
    pub avg: bool,
    pub priority: u8,
    pub preempt: bool,
    pub balance: GlbpBalance,
    pub avg_address: Option<Ipv4Addr>,
    /// The forwarder number this router holds (1-4), if any.
    pub forwarder: Option<u8>,
    /// Every forwarder of the group: (number, its router's address, virtual MAC).
    pub forwarders: Vec<(u8, Ipv4Addr, MacAddr)>,
}

/// The virtual MAC of GLBP forwarder `n` in group `g`: `0007.b4` then the group's 10 bits and
/// the forwarder's number.
pub fn glbp_mac(group: u16, n: u8) -> MacAddr {
    let g = (group & 0x03ff).to_be_bytes();
    MacAddr([0x00, 0x07, 0xb4, g[0], g[1], n])
}

/// Every device's VRRP and GLBP roles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FhrpRoles {
    pub vrrp: BTreeMap<DeviceId, Vec<VrrpRole>>,
    pub vrrp6: BTreeMap<DeviceId, Vec<Vrrp6Role>>,
    pub glbp: BTreeMap<DeviceId, Vec<GlbpRole>>,
}

/// Who held each role at the last election: VRRP masters by (subnet, VRID), GLBP gateways by
/// (subnet, group) (the group number folded into the same key space above 255).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    pub vrrp: Actives,
    /// VRRPv3 masters for IPv6 by (virtual address, link prefix, VRID).
    pub vrrp6: BTreeMap<(Ipv6Addr, Option<Ipv6Net>, u8), Ipv6Addr>,
    pub glbp: BTreeMap<(Ipv4Net, u16), Ipv4Addr>,
}

fn routers(t: &Topology) -> impl Iterator<Item = &netlabcraft_model::Device> {
    t.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) && !d.fhrp.is_empty())
}

/// The priority a member runs with: 255 for the address owner, else the configured one less
/// the decrements of tracked interfaces that are down (at least 1); and those interfaces.
pub fn vrrp_priority(t: &Topology, d: &Device, g: &Vrrp, owner: bool) -> (u8, Vec<String>) {
    if owner {
        return (255, Vec::new());
    }
    let mut p = g.priority;
    let mut down = Vec::new();
    for (name, dec) in &g.track {
        if !crate::hsrp::tracked_up(t, d, name) {
            p = p.saturating_sub(*dec);
            down.push(name.clone());
        }
    }
    (p.max(1), down)
}

/// RFC 5798 §6.1: Skew_Time and Master_Down_Interval (3 × the advertisement interval plus the
/// skew), in µs, for a backup of `priority` with an advertisement interval of `cs`
/// centiseconds. Version 2 (RFC 3768 §6.1) counts the skew in whole seconds' parts:
/// (256 − priority) / 256 s.
pub fn master_down_us(priority: u8, cs: u16, v3: bool) -> (u64, u64) {
    let adv = u64::from(cs) * 10_000;
    let spare = 256 - u64::from(priority);
    let skew = if v3 { spare * adv / 256 } else { spare * 1_000_000 / 256 };
    (skew, adv.saturating_mul(3).saturating_add(skew))
}

/// Elects every group, given who held the roles before; and who holds them now.
pub fn compute(t: &Topology, held: &Held) -> (FhrpRoles, Held) {
    type VMember = (DeviceId, L3If, Vrrp, bool, u8, Vec<String>);
    type V6Member = (DeviceId, L3If6, Vrrp, bool, u8, Vec<String>);
    type GMember = (DeviceId, L3If, Glbp);
    let mut vgroups: BTreeMap<(Ipv4Net, u8), Vec<VMember>> = BTreeMap::new();
    let mut v6groups: BTreeMap<(Ipv6Addr, Option<Ipv6Net>, u8), Vec<V6Member>> = BTreeMap::new();
    let mut ggroups: BTreeMap<(Ipv4Net, u16), Vec<GMember>> = BTreeMap::new();
    for d in routers(t) {
        for ifc in up_ifaces(t, d) {
            for g in d.fhrp.vrrp_on(&ifc.name).iter().filter(|g| g.ip.is_some() || g.ipv6.is_none()) {
                let owner = g.ip == Some(ifc.net.addr);
                let (prio, down) = vrrp_priority(t, d, g, owner);
                vgroups.entry((ifc.net.network(), g.group)).or_default().push((d.id, ifc.clone(), g.clone(), owner, prio, down));
            }
            for g in d.fhrp.glbp_on(&ifc.name) {
                ggroups.entry((ifc.net.network(), g.group)).or_default().push((d.id, ifc.clone(), g.clone()));
            }
        }
        // VRRPv3 for IPv6: a group is told apart by its virtual address and the link's first
        // global prefix.
        if d.fhrp.vrrp_v3 && crate::v6::routes6(d) {
            for ifc in crate::v6::up_ifaces6(t, d) {
                for g in d.fhrp.vrrp_on(&ifc.name) {
                    let Some(vip) = g.ipv6 else { continue };
                    let owner = vip == ifc.link_local;
                    let (prio, down) = vrrp_priority(t, d, g, owner);
                    let prefix = ifc.addresses.first().map(|n| n.network());
                    v6groups.entry((vip, prefix, g.group)).or_default().push((d.id, ifc.clone(), g.clone(), owner, prio, down));
                }
            }
        }
    }
    let mut roles = FhrpRoles::default();
    let mut now = Held::default();
    for ((subnet, group), mut members) in vgroups {
        let Some(vip) = members.iter().find_map(|m| m.2.ip) else { continue };
        members.sort_by_key(|m| std::cmp::Reverse((m.4, m.1.net.addr)));
        keep_active(&mut members, held.vrrp.get(&(subnet, group)).copied(), |m| m.1.net.addr, |m| m.2.preempt || m.3);
        let master = members.first().map(|m| m.1.net.addr);
        if let Some(m) = master {
            now.vrrp.insert((subnet, group), m);
        }
        for (i, m) in members.iter().enumerate() {
            roles.vrrp.entry(m.0).or_default().push(VrrpRole {
                iface: m.1.name.clone(),
                group,
                virtual_ip: vip,
                state: if i == 0 { VrrpState::Master } else { VrrpState::Backup },
                priority: m.4,
                configured: m.2.priority,
                preempt: m.2.preempt,
                owner: m.3,
                master,
                address: m.1.net.addr,
                subnet,
                interval_cs: m.2.interval_cs(),
                tracked_down: m.5.clone(),
                waiting_until: None,
            });
        }
    }
    for ((vip, prefix, group), mut members) in v6groups {
        members.sort_by_key(|m| std::cmp::Reverse((m.4, m.1.link_local)));
        keep_active(&mut members, held.vrrp6.get(&(vip, prefix, group)).copied(), |m| m.1.link_local, |m| m.2.preempt || m.3);
        let master = members.first().map(|m| m.1.link_local);
        if let Some(m) = master {
            now.vrrp6.insert((vip, prefix, group), m);
        }
        for (i, m) in members.iter().enumerate() {
            roles.vrrp6.entry(m.0).or_default().push(Vrrp6Role {
                iface: m.1.name.clone(),
                group,
                virtual_ip: vip,
                prefix,
                state: if i == 0 { VrrpState::Master } else { VrrpState::Backup },
                priority: m.4,
                configured: m.2.priority,
                preempt: m.2.preempt,
                owner: m.3,
                master,
                address: m.1.link_local,
                interval_cs: m.2.interval_cs(),
                tracked_down: m.5.clone(),
                waiting_until: None,
            });
        }
    }
    for ((subnet, group), mut members) in ggroups {
        let Some(vip) = members.iter().find_map(|m| m.2.ip) else { continue };
        // Forwarders are numbered by address, up to four.
        let mut by_addr: Vec<Ipv4Addr> = members.iter().map(|m| m.1.net.addr).collect();
        by_addr.sort();
        by_addr.truncate(MAX_FORWARDERS);
        let forwarders: Vec<(u8, Ipv4Addr, MacAddr)> = by_addr
            .iter()
            .enumerate()
            .map(|(k, a)| {
                let n = u8::try_from(k + 1).unwrap_or(1);
                (n, *a, glbp_mac(group, n))
            })
            .collect();
        members.sort_by_key(|m| std::cmp::Reverse((m.2.priority, m.1.net.addr)));
        keep_active(&mut members, held.glbp.get(&(subnet, group)).copied(), |m| m.1.net.addr, |m| m.2.preempt);
        let avg = members.first().map(|m| m.1.net.addr);
        if let Some(a) = avg {
            now.glbp.insert((subnet, group), a);
        }
        let balance = members.first().map_or(GlbpBalance::default(), |m| m.2.balance);
        for (i, m) in members.iter().enumerate() {
            roles.glbp.entry(m.0).or_default().push(GlbpRole {
                iface: m.1.name.clone(),
                group,
                virtual_ip: vip,
                avg: i == 0,
                priority: m.2.priority,
                preempt: m.2.preempt,
                balance,
                avg_address: avg,
                forwarder: forwarders.iter().find(|f| f.1 == m.1.net.addr).map(|f| f.0),
                forwarders: forwarders.clone(),
            });
        }
    }
    (roles, now)
}

/// A fingerprint of what the elections depend on.
pub fn fingerprint(t: &Topology) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &t.links {
        (l.a.device, l.a.port, l.b.device, l.b.port).hash(&mut h);
    }
    for d in routers(t) {
        d.id.hash(&mut h);
        d.fhrp.hash(&mut h);
        for (i, p) in d.ports.iter().enumerate() {
            (p.shutdown, p.err_disabled, p.ip, port_up(t, End { device: d.id, port: i })).hash(&mut h);
            format!("{:?}", p.ipv6).hash(&mut h);
            for s in &p.subifs {
                (s.ip, s.vlan, s.shutdown, format!("{:?}", s.ipv6)).hash(&mut h);
            }
        }
        for s in &d.svis {
            (s.vlan, s.ip, s.shutdown, format!("{:?}", s.ipv6)).hash(&mut h);
        }
        (d.ipv6_routing, d.ip_routing).hash(&mut h);
    }
    h.finish()
}

/// A VRRP group waiting for a master: from a change of master until the master down interval
/// (or, after a master said goodbye, the skew time) runs out (RFC 5798 §6.4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrrpWait {
    /// The group (its subnet or virtual IPv6 address, and number).
    pub key: String,
    pub until: u64,
    /// The master before, and whether it still takes part (it keeps the role meanwhile:
    /// preemption also waits for the interval).
    pub prev: String,
    pub prev_member: bool,
    /// The router that takes over when the wait ends.
    pub next: String,
}

/// Who acts as master of a group: its address, router and interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acting {
    pub address: String,
    pub device: DeviceId,
    pub iface: String,
}

/// One member of a group, wherever its role sits.
struct Member {
    dev: DeviceId,
    idx: usize,
    v6: bool,
    address: String,
    iface: String,
    group: u8,
    priority: u8,
    interval_cs: u16,
    master: bool,
}

/// Whether an interface, by name, is shut down (or gone).
fn admin_down(d: &Device, iface: &str) -> bool {
    if let Some(v) = iface.strip_prefix("Vlan").and_then(|v| v.parse::<u16>().ok()) {
        return d.svis.iter().find(|s| s.vlan.get() == v).is_none_or(|s| s.shutdown);
    }
    let (base, sub) = match iface.split_once('.') {
        Some((b, s)) => (b, s.parse::<u32>().ok()),
        None => (iface, None),
    };
    let Some(p) = d.port_index(base).and_then(|i| d.ports.get(i)) else { return true };
    p.shutdown || sub.is_some_and(|n| p.subif(n).is_none_or(|s| s.shutdown))
}

impl Sim {
    /// Applies VRRP's timers to a fresh election: a new master takes over only when the master
    /// down interval has passed without advertisements from the old one (the skew time, when
    /// the old master said goodbye with priority 0: its interface or group was removed or shut
    /// down). Meanwhile the old master keeps the role if it is still there, else nobody has it.
    pub(crate) fn vrrp_timers(&mut self, t: &Topology, roles: &mut FhrpRoles) {
        let now = self.now;
        let v3 = |dev: DeviceId| t.device(dev).is_some_and(|d| d.fhrp.vrrp_v3);
        let mut groups: BTreeMap<String, Vec<Member>> = BTreeMap::new();
        for (dev, list) in &roles.vrrp {
            for (idx, r) in list.iter().enumerate() {
                groups.entry(format!("{}#{}", r.subnet, r.group)).or_default().push(Member {
                    dev: *dev,
                    idx,
                    v6: false,
                    address: r.address.to_string(),
                    iface: r.iface.clone(),
                    group: r.group,
                    priority: r.priority,
                    interval_cs: r.interval_cs,
                    master: r.state == VrrpState::Master,
                });
            }
        }
        for (dev, list) in &roles.vrrp6 {
            for (idx, r) in list.iter().enumerate() {
                let prefix = r.prefix.map_or("-".to_string(), |p| p.to_string());
                groups.entry(format!("{}#{prefix}#{}", r.virtual_ip, r.group)).or_default().push(Member {
                    dev: *dev,
                    idx,
                    v6: true,
                    address: r.address.to_string(),
                    iface: r.iface.clone(),
                    group: r.group,
                    priority: r.priority,
                    interval_cs: r.interval_cs,
                    master: r.state == VrrpState::Master,
                });
            }
        }
        let expired: Vec<String> = self.vrrp_waits.iter().filter(|w| w.until <= now).map(|w| w.key.clone()).collect();
        self.vrrp_waits.retain(|w| w.until > now && groups.contains_key(&w.key));
        let mut acting_next: BTreeMap<String, Acting> = BTreeMap::new();
        let mut logs: Vec<(DeviceId, String)> = Vec::new();
        for (key, members) in &groups {
            let elected = members.iter().find(|m| m.master);
            let prev = self.vrrp_acting.get(key).cloned();
            let at = self.vrrp_waits.iter().position(|w| w.key == *key);
            if let Some(w) = at.and_then(|i| self.vrrp_waits.get_mut(i)) {
                match elected {
                    Some(e) if e.address == w.prev && w.prev_member => w.until = now,
                    Some(e) => w.next = e.address.clone(),
                    None => w.until = now,
                }
            } else if let (Some(p), Some(e)) = (&prev, elected)
                && p.address != e.address
                && !expired.contains(key)
            {
                let prev_member = members.iter().any(|m| m.address == p.address);
                let goodbye = !prev_member
                    && t.device(p.device).is_some_and(|d| {
                        admin_down(d, &p.iface)
                            || !d.fhrp.vrrp_on(&p.iface).iter().any(|g| g.group == e.group && (if e.v6 { g.ipv6.is_some() } else { g.ip.is_some() }))
                    });
                let (skew, mdi) = master_down_us(e.priority, e.interval_cs, e.v6 || v3(e.dev));
                let wait = if goodbye { skew } else { mdi };
                if wait > 0 && self.vrrp_waits.len() < MAX_WAITS {
                    self.vrrp_waits.push(VrrpWait {
                        key: key.clone(),
                        until: now.saturating_add(wait),
                        prev: p.address.clone(),
                        prev_member,
                        next: e.address.clone(),
                    });
                }
            }
            self.vrrp_waits.retain(|w| w.until > now);
            let wait = self.vrrp_waits.iter().find(|w| w.key == *key).cloned();
            let acting = match &wait {
                Some(w) if w.prev_member => members.iter().find(|m| m.address == w.prev),
                Some(_) => None,
                None => elected,
            };
            if let Some(w) = &wait {
                let keep = acting.map(|a| a.address.clone());
                for m in members {
                    let state = if keep.as_deref() == Some(m.address.as_str()) { VrrpState::Master } else { VrrpState::Backup };
                    if m.v6 {
                        if let Some(r) = roles.vrrp6.get_mut(&m.dev).and_then(|l| l.get_mut(m.idx)) {
                            r.state = state;
                            r.master = keep.as_deref().and_then(|a| a.parse().ok());
                            r.waiting_until = Some(w.until);
                        }
                    } else if let Some(r) = roles.vrrp.get_mut(&m.dev).and_then(|l| l.get_mut(m.idx)) {
                        r.state = state;
                        r.master = keep.as_deref().and_then(|a| a.parse().ok());
                        r.waiting_until = Some(w.until);
                    }
                }
            }
            if let Some(a) = acting {
                let now_acting = Acting { address: a.address.clone(), device: a.dev, iface: a.iface.clone() };
                if let Some(p) = &prev
                    && p.address != a.address
                {
                    let family = if a.v6 { "IPv6 " } else { "" };
                    logs.push((a.dev, format!("VRRP: {family}group {} on {}: backup -> master", a.group, a.iface)));
                    if members.iter().any(|m| m.address == p.address) {
                        logs.push((p.device, format!("VRRP: {family}group {} on {}: master -> backup", a.group, p.iface)));
                    }
                }
                acting_next.insert(key.clone(), now_acting);
            } else if let Some(p) = &prev {
                // Nobody has the role while the wait runs; remember who had it.
                if wait.is_some() {
                    acting_next.insert(key.clone(), p.clone());
                }
            }
        }
        self.vrrp_acting = acting_next;
        for (dev, msg) in logs {
            self.log(dev, &msg);
        }
    }

    /// The VRRPv3 IPv6 groups this router is master of on an interface: (virtual address,
    /// virtual MAC).
    pub(crate) fn vrrp6_masters(&mut self, t: &Topology, d: &Device, iface: &str) -> Vec<(Ipv6Addr, MacAddr)> {
        if d.fhrp.vrrp.is_empty() {
            return Vec::new();
        }
        let roles = self.fhrp_roles(t);
        roles
            .vrrp6
            .get(&d.id)
            .into_iter()
            .flatten()
            .filter(|r| r.iface == iface && r.state == VrrpState::Master)
            .map(|r| (r.virtual_ip, r.virtual_mac()))
            .collect()
    }

    /// Every VRRPv3 IPv6 group this router has on an interface, master or not: (virtual
    /// address, virtual MAC, whether it is master).
    pub(crate) fn vrrp6_roles(&mut self, t: &Topology, d: &Device, iface: &str) -> Vec<(Ipv6Addr, MacAddr, bool)> {
        if d.fhrp.vrrp.is_empty() {
            return Vec::new();
        }
        let roles = self.fhrp_roles(t);
        roles
            .vrrp6
            .get(&d.id)
            .into_iter()
            .flatten()
            .filter(|r| r.iface == iface)
            .map(|r| (r.virtual_ip, r.virtual_mac(), r.state == VrrpState::Master))
            .collect()
    }

    /// When the next VRRP wait ends.
    pub(crate) fn vrrp_next_expiry(&self) -> Option<u64> {
        self.vrrp_waits.iter().map(|w| w.until).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glbp_macs_carry_group_and_forwarder() {
        assert_eq!(glbp_mac(1, 2), MacAddr([0x00, 0x07, 0xb4, 0x00, 0x01, 0x02]));
        assert_eq!(glbp_mac(1023, 4), MacAddr([0x00, 0x07, 0xb4, 0x03, 0xff, 0x04]));
        assert_eq!(glbp_mac(0xffff, 1), glbp_mac(1023, 1));
    }
}
