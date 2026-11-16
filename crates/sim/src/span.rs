//! Port mirroring (SPAN): a switch copies the frames that enter (rx) and leave (tx) the ports
//! and VLANs of a monitor session to the session's destination port.
//!
//! The copy leaves the destination port as a frame of its own, one hop after the original, so a
//! sniffer cabled there hears it. The destination carries copies only: it forwards no normal
//! traffic and takes no frame in. A copy carries the hop count of the frame it copies plus one,
//! so a destination cabled back into the lab cannot make a loop that runs for ever.

use netlabcraft_core::VlanId;
use netlabcraft_model::{Device, DeviceKind, End, Topology};
use netlabcraft_wire::Frame;

use crate::{MAX_SWITCH_HOPS, Sim, port_up};

/// The VLAN a frame on this port belongs to: its tag when the port tags, else the port's own.
fn frame_vlan(t: &Topology, d: &Device, port: usize, bytes: &[u8]) -> Option<VlanId> {
    let p = d.ports.get(port)?;
    let tag = Frame::decode(bytes).ok().and_then(|f| f.vlan).and_then(VlanId::new);
    if t.is_trunk(End { device: d.id, port }) {
        Some(tag.unwrap_or(p.switchport.native_vlan))
    } else {
        Some(tag.filter(|v| p.switchport.voice_vlan == Some(*v)).unwrap_or(p.switchport.access_vlan))
    }
}

impl Sim {
    /// Whether a switch is mirroring anything at all.
    pub(crate) fn span_active(d: &Device) -> bool {
        d.kind == DeviceKind::Switch && !d.l2.span.is_empty()
    }

    /// Whether `port` of a switch is the destination of a monitor session.
    pub(crate) fn span_destination(d: &Device, port: usize) -> bool {
        Sim::span_active(d) && d.ports.get(port).is_some_and(|p| d.l2.span_destination_of(&p.name).is_some())
    }

    /// Copies a frame received on (`rx`) or leaving (`!rx`) `port` of the switch `d` at `at`, in
    /// simulated microseconds, to the destination of every session that watches that port, or the
    /// frame's VLAN.
    pub(crate) fn span_copy(&mut self, t: &Topology, d: &Device, port: usize, bytes: &[u8], rx: bool, hops: u8, at: u64) {
        if !Sim::span_active(d) || hops >= MAX_SWITCH_HOPS {
            return;
        }
        let Some(p) = d.ports.get(port) else { return };
        let vlan = if d.l2.span.iter().any(|s| !s.vlans.is_empty()) { frame_vlan(t, d, port, bytes) } else { None };
        let dests: Vec<usize> =
            d.l2.span
                .iter()
                .filter(|s| s.watches(&p.name, vlan, rx))
                .filter_map(|s| s.destination.as_deref())
                .filter_map(|name| d.port_index(name))
                .filter(|q| *q != port)
                .collect();
        for q in dests {
            let from = End { device: d.id, port: q };
            if !port_up(t, from) || self.err_disabled_now(d.id, q) {
                continue;
            }
            let Some(to) = t.peer(from) else { continue };
            self.push_hop(t, at + self.hop_delay(), from, to, bytes.to_vec(), hops.saturating_add(1));
        }
    }
}
