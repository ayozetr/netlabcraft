//! `ip mtu`: datagrams larger than an interface's IP MTU are fragmented (RFC 791), or, when
//! they say "don't fragment", dropped with an ICMP "fragmentation needed" carrying the MTU
//! back to their source (RFC 1191). Fragments addressed to a device are reassembled there (see
//! `ip_in`); routers forward them as they are. On a tunnel the MTU applies to the inner packet,
//! before encapsulation.

use std::net::Ipv4Addr;

use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::ipfrag::{dont_fragment, frag_needed, fragment};
use netlabcraft_wire::{Icmp, Ipv4, PROTO_ICMP};

use crate::{L3If, Sim, l3_ifaces};

impl Sim {
    /// Whether an encoded datagram's destination is one of this device's addresses (or a
    /// broadcast), so its fragments are reassembled here.
    pub(crate) fn addressed_here(d: &Device, datagram: &[u8]) -> bool {
        let Some(b) = datagram.get(16..20) else { return false };
        let dst = Ipv4Addr::new(b[0], b[1], b[2], b[3]);
        dst == Ipv4Addr::BROADCAST || l3_ifaces(d).iter().any(|i| i.net.addr == dst || (i.net.prefix < 31 && i.net.broadcast() == dst))
    }

    /// Sends a datagram too large for `out`'s MTU: in fragments, or not at all when DF is set.
    pub(crate) fn transmit_mtu(&mut self, t: &Topology, d: &Device, out: &L3If, nh: Ipv4Addr, datagram: Vec<u8>, mtu: u16) -> Result<(), String> {
        if dont_fragment(&datagram) {
            self.bump(d.id, "dropped: fragmentation needed and DF set");
            let Ok(orig) = Ipv4::decode(&datagram) else { return Err("an unreadable datagram is too large for the MTU".into()) };
            // Never an error about an ICMP error.
            let is_error = orig.protocol == PROTO_ICMP && !matches!(orig.payload.first(), Some(0 | 8));
            if !is_error {
                let msg = frag_needed(mtu, &Icmp::quote(&datagram));
                let mine = l3_ifaces(d).iter().any(|i| i.net.addr == orig.src);
                if mine {
                    // Raised here (a ping from this router): told to itself.
                    let err = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_ICMP, src: out.net.addr, dst: orig.src, payload: msg };
                    let _ = self.local_in(t, d, out, err);
                } else if let Ok(src) = self.source_for(t, d, orig.src) {
                    let err = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_ICMP, src, dst: orig.src, payload: msg };
                    let _ = self.send_ip(t, d, err);
                }
            }
            return Err(format!("{} bytes need fragmenting for {} (IP MTU {mtu}) but \"don't fragment\" is set", datagram.len(), out.name));
        }
        let pieces = fragment(&datagram, usize::from(mtu)).map_err(|e| e.to_string())?;
        self.bump(d.id, "fragmented");
        for p in pieces {
            self.transmit(t, d, out, nh, p)?;
        }
        Ok(())
    }
}
