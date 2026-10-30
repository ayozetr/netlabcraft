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
//! format, or RFC 5798's version 3 with `fhrp version vrrp v3`.
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
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net, MacAddr};
use netlabcraft_model::fhrp::{Glbp, GlbpBalance, Vrrp};
use netlabcraft_model::{DeviceKind, End, Topology};

use crate::hsrp::{Actives, keep_active};
use crate::{L3If, port_up, up_ifaces};

/// Most GLBP forwarders per group.
pub const MAX_FORWARDERS: usize = 4;

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
    /// 255 for the owner.
    pub priority: u8,
    pub preempt: bool,
    pub owner: bool,
    pub master: Option<Ipv4Addr>,
    /// The interface's own address (the advertisements' source).
    pub address: Ipv4Addr,
}

impl VrrpRole {
    pub fn virtual_mac(&self) -> MacAddr {
        netlabcraft_wire::vrrp::virtual_mac(self.group)
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
    pub glbp: BTreeMap<DeviceId, Vec<GlbpRole>>,
}

/// Who held each role at the last election: VRRP masters by (subnet, VRID), GLBP gateways by
/// (subnet, group) (the group number folded into the same key space above 255).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    pub vrrp: Actives,
    pub glbp: BTreeMap<(Ipv4Net, u16), Ipv4Addr>,
}

fn routers(t: &Topology) -> impl Iterator<Item = &netlabcraft_model::Device> {
    t.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) && !d.fhrp.is_empty())
}

/// Elects every group, given who held the roles before; and who holds them now.
pub fn compute(t: &Topology, held: &Held) -> (FhrpRoles, Held) {
    type VMember = (DeviceId, L3If, Vrrp, bool);
    type GMember = (DeviceId, L3If, Glbp);
    let mut vgroups: BTreeMap<(Ipv4Net, u8), Vec<VMember>> = BTreeMap::new();
    let mut ggroups: BTreeMap<(Ipv4Net, u16), Vec<GMember>> = BTreeMap::new();
    for d in routers(t) {
        for ifc in up_ifaces(t, d) {
            for g in d.fhrp.vrrp_on(&ifc.name) {
                let owner = g.ip == Some(ifc.net.addr);
                vgroups.entry((ifc.net.network(), g.group)).or_default().push((d.id, ifc.clone(), g.clone(), owner));
            }
            for g in d.fhrp.glbp_on(&ifc.name) {
                ggroups.entry((ifc.net.network(), g.group)).or_default().push((d.id, ifc.clone(), g.clone()));
            }
        }
    }
    let mut roles = FhrpRoles::default();
    let mut now = Held::default();
    for ((subnet, group), mut members) in vgroups {
        let Some(vip) = members.iter().find_map(|m| m.2.ip) else { continue };
        let prio = |m: &VMember| if m.3 { 255 } else { m.2.priority };
        members.sort_by_key(|m| std::cmp::Reverse((prio(m), m.1.net.addr)));
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
                priority: prio(m),
                preempt: m.2.preempt,
                owner: m.3,
                master,
                address: m.1.net.addr,
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
            for s in &p.subifs {
                (s.ip, s.vlan, s.shutdown).hash(&mut h);
            }
        }
        for s in &d.svis {
            (s.vlan, s.ip, s.shutdown).hash(&mut h);
        }
    }
    h.finish()
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
