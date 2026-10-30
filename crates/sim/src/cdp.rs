//! CDP-style neighbour discovery: which router or switch is at the other end of each cable.
//!
//! Computed from the topology like the other protocols here: a neighbour is listed when both
//! devices run discovery (on by default; `no cdp run` turns it off for a device, `no cdp enable`
//! for one port) and the cable is up. Hosts don't take part. The advertisement frames are not
//! drawn in the event list yet.

use netlabcraft_core::Ipv4Net;
use netlabcraft_model::{Device, DeviceKind, End, Topology};

use crate::port_up;

/// One neighbour seen on one of our ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub name: String,
    pub kind: DeviceKind,
    pub platform: String,
    /// Our port.
    pub local: String,
    /// Their port.
    pub remote: String,
    /// The address they advertise: their port's, else their first interface address.
    pub address: Option<Ipv4Net>,
}

fn speaks(d: &Device) -> bool {
    matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) && !d.cdp_off
}

/// The neighbours a device hears.
pub fn neighbors(t: &Topology, d: &Device) -> Vec<Neighbor> {
    if !speaks(d) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, p) in d.ports.iter().enumerate() {
        let end = End { device: d.id, port: i };
        if p.cdp_off || !port_up(t, end) {
            continue;
        }
        let Some(q) = t.peer(end) else { continue };
        let Some(nd) = t.device(q.device).filter(|nd| speaks(nd)) else { continue };
        let Some(np) = nd.ports.get(q.port).filter(|np| !np.cdp_off) else { continue };
        let address = np.ip.or_else(|| crate::l3_ifaces(nd).first().map(|i| i.net));
        out.push(Neighbor {
            name: nd.name.clone(),
            kind: nd.kind,
            platform: nd.catalogue_model().label.to_string(),
            local: p.name.clone(),
            remote: np.name.clone(),
            address,
        });
    }
    out
}
