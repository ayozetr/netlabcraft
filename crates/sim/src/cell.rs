//! The cellular network on the wire: cell towers carrying frames between their sectors and their
//! backhaul port, the attach exchange between a mobile device and the central office server, and
//! what the device then knows of its attach.
//!
//! Who is covered, which tower and sector a device sits on, whether the central office admits it
//! and which address it gets are worked out from the lab (`Topology::cell_refresh`, see
//! `netlabcraft_model::cellular`), as Wi-Fi association is. The simulator shows that outcome as
//! an exchange of real frames (`netlabcraft_wire::cell`): the device sends an attach request out
//! of its cellular port, the tower carries it to the backhaul, the server on the far side
//! answers with an accept or a reject, and the answer comes back the same way. The device's data
//! frames do not wait for the exchange: its address is already in place.
//!
//! A tower never lets one device's frames reach another's sector: sector traffic goes to the
//! backhaul only, and backhaul traffic goes to the sector of the device it is addressed to
//! (every sector for a broadcast).

use std::net::Ipv4Addr;

use netlabcraft_core::MacAddr;
use netlabcraft_model::cellular::Refusal;
use netlabcraft_model::{Device, End, Topology, ifname};
use netlabcraft_wire::Frame;
use netlabcraft_wire::cell::{self, Attach, ETHERTYPE_CELL};

use crate::{MAX_SWITCH_HOPS, Sim};

/// What a device has learned from the attach exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellSession {
    /// The central office accepted it: this address and gateway.
    Attached { address: Ipv4Addr, prefix: u8, gateway: Ipv4Addr },
    /// The central office refused it, with this text.
    Rejected(String),
}

fn reject_code(r: Refusal) -> u8 {
    match r {
        Refusal::NoCore => cell::REJECT_UNKNOWN,
        Refusal::NoPool => cell::REJECT_NO_POOL,
        Refusal::Plan => cell::REJECT_PLAN,
        Refusal::Apn => cell::REJECT_APN,
        Refusal::Exhausted => cell::REJECT_EXHAUSTED,
    }
}

impl Sim {
    /// What a device's attach exchange has shown (`None` until an answer arrived).
    pub fn cell_session(&self, device: End) -> Option<&CellSession> {
        self.cell_state.get(&device)
    }

    /// Starts an attach exchange for every device whose link or admission changed since the
    /// last one, and forgets devices that lost their link.
    pub(crate) fn cell_sync(&mut self, t: &Topology) {
        let has_link = |e: &End| t.cell_link(e.device).is_some();
        self.cell_seen.retain(|e, _| has_link(e));
        self.cell_state.retain(|e, _| has_link(e));
        for (id, l) in &t.cell_links {
            let Some(d) = t.device(*id) else { continue };
            let Some(cp) = d.cell_port() else { continue };
            let end = End { device: *id, port: cp };
            let verdict = match &l.outcome {
                Ok(lease) => lease.address.addr.to_string(),
                Err(r) => r.text().to_string(),
            };
            let key = format!("{}/{}/{}/{}", l.tower.0, l.sector, verdict, d.cell.apn);
            if self.cell_seen.get(&end) == Some(&key) {
                continue;
            }
            self.cell_seen.insert(end, key);
            self.cell_state.remove(&end);
            let src = d.ports.get(cp).map_or(MacAddr::ZERO, |p| p.mac);
            let req = Attach::Request { apn: d.cell.apn.clone() };
            let frame = Frame { dst: MacAddr::BROADCAST, src, vlan: None, ethertype: ETHERTYPE_CELL, payload: req.encode() };
            self.emit(t, end, frame.encode());
        }
    }

    /// A cell tower: frames from a sector go to the backhaul port; frames from the backhaul go
    /// to the sector of the device they are addressed to, or to every attached sector.
    pub(crate) fn tower_in(&mut self, t: &Topology, d: &Device, in_port: usize, bytes: &[u8], hops: u8) -> Result<(), String> {
        if hops >= MAX_SWITCH_HOPS {
            self.storm = true;
            return Err(format!("loop: the frame crossed {MAX_SWITCH_HOPS} hubs or switches"));
        }
        let is_sector = |p: usize| d.ports.get(p).is_some_and(|x| ifname::is_cellular(&x.name));
        let mut sent = 0;
        if is_sector(in_port) {
            let Some(back) = (0..d.ports.len()).find(|p| !is_sector(*p)) else { return Err("the tower has no backhaul port".into()) };
            if self.emit_hops(t, End { device: d.id, port: back }, bytes.to_vec(), hops + 1) {
                sent += 1;
            }
            return if sent == 0 { Err("the backhaul is down".into()) } else { Ok(()) };
        }
        let dst = bytes.get(..6).map(<[u8]>::to_vec).unwrap_or_default();
        let sectors: Vec<(usize, Vec<u8>)> = (0..d.ports.len())
            .filter(|p| is_sector(*p))
            .filter_map(|p| {
                let far = t.cell_peer(End { device: d.id, port: p })?;
                let mac = t.device(far.device)?.ports.get(far.port)?.mac;
                Some((p, mac.0.to_vec()))
            })
            .collect();
        let unicast: Vec<usize> = sectors.iter().filter(|(_, mac)| *mac == dst).map(|(p, _)| *p).collect();
        let targets: Vec<usize> = if unicast.is_empty() { sectors.iter().map(|(p, _)| *p).collect() } else { unicast };
        for p in targets {
            if self.emit_hops(t, End { device: d.id, port: p }, bytes.to_vec(), hops + 1) {
                sent += 1;
            }
        }
        if sent == 0 { Err("no device is attached to this tower".into()) } else { Ok(()) }
    }

    /// An attach message arriving at a device: the central office answers a request, a mobile
    /// device takes the answer.
    pub(crate) fn cell_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: &Frame) -> Result<(), String> {
        let msg = Attach::decode(&f.payload).map_err(|e| e.to_string())?;
        let here = End { device: d.id, port: in_port };
        match msg {
            Attach::Request { apn } => {
                if !d.is_central_office() {
                    return Err("not a central office server".into());
                }
                // The device whose radio sent it, and where the server's side of the lab put it.
                let Some((_, link)) = t
                    .cell_links
                    .iter()
                    .find(|(id, _)| t.device(**id).and_then(|x| x.cell_port().and_then(|cp| x.ports.get(cp))).is_some_and(|p| p.mac == f.src))
                else {
                    return Err("no attached device has that address".into());
                };
                if link.core != Some(here) {
                    return Err("the device attaches through another server".into());
                }
                let reply = match &link.outcome {
                    Ok(lease) => Attach::Accept { address: lease.address.addr, prefix: lease.address.prefix, gateway: lease.gateway, apn },
                    Err(r) => Attach::Reject { code: reject_code(*r), text: r.text().to_string() },
                };
                let src = d.ports.get(in_port).map_or(MacAddr::ZERO, |p| p.mac);
                let frame = Frame { dst: f.src, src, vlan: None, ethertype: ETHERTYPE_CELL, payload: reply.encode() };
                self.emit(t, here, frame.encode());
                Ok(())
            }
            Attach::Accept { address, prefix, gateway, .. } => {
                if d.cell_port() != Some(in_port) {
                    return Err("not a cellular port".into());
                }
                self.cell_state.insert(here, CellSession::Attached { address, prefix, gateway });
                Ok(())
            }
            Attach::Reject { text, .. } => {
                if d.cell_port() != Some(in_port) {
                    return Err("not a cellular port".into());
                }
                self.cell_state.insert(here, CellSession::Rejected(text));
                Ok(())
            }
        }
    }
}
