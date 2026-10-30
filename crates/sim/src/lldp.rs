//! LLDP neighbour discovery (IEEE 802.1AB): which router or switch is at the other end of each
//! cable, as its advertisements say.
//!
//! Computed from the topology like CDP; the advertisements themselves go on the wire every 30 s
//! (`control`), so Simulation mode and captures show them. A neighbour is listed when both
//! devices run LLDP (`lldp run`), the sender's port transmits, ours receives, and the cable is up.

use std::net::Ipv4Addr;

use packetcraft_core::MacAddr;
use packetcraft_model::{Device, DeviceKind, End, Topology};
pub use packetcraft_wire::lldp::{CAP_BRIDGE, CAP_ROUTER, Lldp};

use crate::port_up;

/// Advertisements every 30 s, kept four times as long.
pub const LLDP_EVERY_US: u64 = 30_000_000;
pub const HOLD_SECS: u16 = 120;

/// One neighbour heard on one of our ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub local: String,
    pub info: Lldp,
}

fn speaks(d: &Device) -> bool {
    matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) && d.lldp_on
}

/// What a device's port advertises, or `None` when it doesn't send.
pub fn advertisement(d: &Device, port: usize) -> Option<Lldp> {
    let p = d.ports.get(port).filter(|p| speaks(d) && !p.lldp_tx_off)?;
    let caps = match d.kind {
        DeviceKind::Router => (CAP_ROUTER, CAP_ROUTER),
        _ if d.ip_routing => (CAP_BRIDGE | CAP_ROUTER, CAP_BRIDGE | CAP_ROUTER),
        _ => (CAP_BRIDGE, CAP_BRIDGE),
    };
    let address: Option<Ipv4Addr> = p.ip.or_else(|| crate::l3_ifaces(d).first().map(|i| i.net)).map(|n| n.addr);
    Some(Lldp {
        chassis: d.ports.first().map_or(MacAddr([0; 6]), |f| f.mac),
        port: p.name.clone(),
        ttl: HOLD_SECS,
        system_name: d.name.clone(),
        description: format!("PacketCraft {} {}", d.catalogue_model().label, env!("CARGO_PKG_VERSION")),
        caps,
        address,
    })
}

/// The neighbours a device hears.
pub fn neighbors(t: &Topology, d: &Device) -> Vec<Neighbor> {
    if !speaks(d) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, p) in d.ports.iter().enumerate() {
        let end = End { device: d.id, port: i };
        if p.lldp_rx_off || !port_up(t, end) {
            continue;
        }
        let Some(q) = t.peer(end) else { continue };
        let Some(info) = t.device(q.device).and_then(|nd| advertisement(nd, q.port)) else { continue };
        out.push(Neighbor { local: p.name.clone(), info });
    }
    out
}
