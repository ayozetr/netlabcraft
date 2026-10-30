//! Loopback interfaces: layer-3 interfaces with no cable behind them.
//!
//! A loopback is up whenever it is configured and not shut down. Its address is one of the
//! device's own (pings to it answer), its network is a connected route, and routing protocols
//! advertise it like any other interface their `network` statements cover. Nothing is ever
//! sent out of it: a packet routed to another address of its subnet is dropped.

use std::net::Ipv4Addr;

use netlabcraft_core::MacAddr;
use netlabcraft_model::routing::MAX_LOOPBACKS;
use netlabcraft_model::{Device, DeviceKind};

use crate::L3If;
use crate::v6::L3If6;

/// The loopback at index `i` of a device's list is reported as port `LOOPBACK_BASE + i`: far
/// beyond any real port, below the tunnels' range ends on every target, and below
/// [`crate::SVI_PORT`].
pub const LOOPBACK_BASE: usize = usize::MAX - 1_024;

/// Whether a pseudo port index stands for a loopback.
pub fn is_loopback(port: usize) -> bool {
    port.checked_sub(LOOPBACK_BASE).is_some_and(|i| i < MAX_LOOPBACKS)
}

/// The MAC address loopbacks report (they never send frames).
fn loopback_mac(d: &Device) -> MacAddr {
    MacAddr::local(d.id, 0xfffc)
}

/// Whether a device kind takes loopback interfaces.
pub fn has_loopbacks(d: &Device) -> bool {
    matches!(d.kind, DeviceKind::Router | DeviceKind::Switch)
}

/// The loopbacks with an IPv4 address and not shut down.
pub(crate) fn loopback_ifaces(d: &Device) -> Vec<L3If> {
    if !has_loopbacks(d) {
        return Vec::new();
    }
    let mac = loopback_mac(d);
    d.loopbacks
        .iter()
        .enumerate()
        .take(MAX_LOOPBACKS)
        .filter(|(_, l)| !l.shutdown)
        .filter_map(|(i, l)| l.ip.map(|net| L3If { port: LOOPBACK_BASE.saturating_add(i), sub: None, vlan: None, net, mac, name: l.name() }))
        .collect()
}

/// The loopbacks with IPv6 on and not shut down.
pub(crate) fn loopback_ifaces6(d: &Device) -> Vec<L3If6> {
    if !has_loopbacks(d) {
        return Vec::new();
    }
    let mac = loopback_mac(d);
    d.loopbacks
        .iter()
        .enumerate()
        .take(MAX_LOOPBACKS)
        .filter(|(_, l)| !l.shutdown && l.ipv6.active())
        .map(|(i, l)| L3If6 {
            port: LOOPBACK_BASE.saturating_add(i),
            vlan: None,
            name: l.name(),
            mac,
            link_local: l.ipv6.link_local.unwrap_or_else(|| netlabcraft_core::link_local(mac)),
            addresses: l.ipv6.addresses.clone(),
        })
        .collect()
}

/// The IPv6 settings of the loopback behind a pseudo port.
pub(crate) fn conf6(d: &Device, port: usize) -> Option<&netlabcraft_model::Ipv6Conf> {
    let i = port.checked_sub(LOOPBACK_BASE)?;
    d.loopbacks.get(i).map(|l| &l.ipv6)
}

/// The router ID a routing process picks when none is configured: the highest loopback
/// address, else the highest address of any interface.
pub fn default_router_id(d: &Device) -> Option<Ipv4Addr> {
    loopback_ifaces(d).iter().map(|i| i.net.addr).max().or_else(|| crate::l3_ifaces(d).iter().map(|i| i.net.addr).max())
}
