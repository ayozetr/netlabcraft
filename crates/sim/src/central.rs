//! Central switching: on a WLAN set to it (`wlan N SSID ... central`), a lightweight AP doesn't
//! bridge its hosts' frames onto its own uplink. It tunnels each one to its controller inside
//! CAPWAP data (UDP 5247, an IEEE 802.3 frame with the T bit clear, RFC 5415 §4.3), and the
//! controller bridges it into the WLAN's VLAN on its own ports. Frames for those hosts come
//! back the same way: the controller, seeing one for a centrally switched host (or a broadcast
//! in its VLAN), tunnels it to the host's AP, which puts it on the air. The tunnelled frame
//! keeps an 802.1Q tag naming the VLAN, so the AP knows which of its hosts a broadcast is for.
//! Local switching (the AP bridges) stays the default.

use netlabcraft_core::{DeviceId, VlanId};
use netlabcraft_model::wlan::is_wlc;
use netlabcraft_model::{Device, End, Topology, ifname};
use netlabcraft_wire::Frame;
use netlabcraft_wire::capwap;

use crate::{Sim, is_ap_radio, wlan};

/// The centrally switched hosts a controller serves in `vlan`: (host radio, its AP).
fn central_hosts(t: &Topology, wlc: DeviceId, vlan: VlanId) -> Vec<(End, DeviceId)> {
    t.devices
        .iter()
        .filter(|d| d.kind.is_host())
        .flat_map(|d| d.ports.iter().enumerate().filter(|(_, p)| ifname::is_wireless(&p.name)).map(move |(i, _)| End { device: d.id, port: i }))
        .filter_map(|c| t.central_client(c).filter(|(w, _, v)| *w == wlc && *v == vlan).map(|(_, ap, _)| (c, ap.device)))
        .collect()
}

fn port_mac(t: &Topology, e: End) -> netlabcraft_core::MacAddr {
    t.device(e.device).and_then(|d| d.ports.get(e.port)).map_or(netlabcraft_core::MacAddr::ZERO, |p| p.mac)
}

impl Sim {
    /// At a lightweight AP: a frame off its radio from a centrally switched host goes to the
    /// controller in CAPWAP data instead of being bridged. `None` when it isn't one.
    pub(crate) fn central_in(&mut self, t: &Topology, d: &Device, in_port: usize, f: &Frame) -> Option<Result<(), String>> {
        if !netlabcraft_model::wlan::is_lap(d) || !is_ap_radio(t, End { device: d.id, port: in_port }) {
            return None;
        }
        let client = wlan::host_with_mac(t, f.src)?;
        let (wlc, ap, vlan) = t.central_client(client)?;
        if ap != (End { device: d.id, port: in_port }) {
            return None;
        }
        let Some(w) = t.device(wlc).cloned() else { return Some(Err("controller removed".into())) };
        let inner = f.clone().retagged(Some(vlan.get())).encode();
        Some(self.capwap_send(t, d, &w, capwap::data_encode_8023(&inner), capwap::DATA_PORT))
    }

    /// At a controller: a host's frame tunnelled by its AP, bridged into the host's VLAN.
    pub(crate) fn central_from_ap(&mut self, t: &Topology, me: &Device, ap: &Device, frame: &[u8]) -> Result<(), String> {
        let f = Frame::decode(frame).map_err(|e| e.to_string())?;
        let client = wlan::host_with_mac(t, f.src).ok_or("tunnelled frame from a host that isn't on the air")?;
        let (wlc, radio, vlan) = t.central_client(client).ok_or("tunnelled frame from a host that isn't centrally switched")?;
        if wlc != me.id || radio.device != ap.id {
            return Err("tunnelled frame for another controller or from another AP".into());
        }
        if f.vlan.is_some_and(|v| v != vlan.get()) {
            return Err(format!("tunnelled frame tagged for VLAN {}, the host's WLAN is in VLAN {vlan}", f.vlan.unwrap_or(0)));
        }
        self.switch_forward(t, me, None, vlan, f.retagged(None), 0)
    }

    /// At a controller forwarding in `vlan`: frames for centrally switched hosts are tunnelled
    /// to their APs. Returns (handled: a unicast that needs nothing else, tunnelled any).
    pub(crate) fn central_out(&mut self, t: &Topology, d: &Device, vlan: VlanId, f: &Frame) -> (bool, bool) {
        if !is_wlc(d) || !d.wlan.wlans.iter().any(|w| w.central) {
            return (false, false);
        }
        let hosts = central_hosts(t, d.id, vlan);
        let targets: Vec<DeviceId> = if f.dst.is_group() {
            let mut aps: Vec<DeviceId> = hosts.iter().map(|(_, ap)| *ap).collect();
            aps.sort();
            aps.dedup();
            aps
        } else {
            hosts.iter().filter(|(c, _)| port_mac(t, *c) == f.dst).map(|(_, ap)| *ap).take(1).collect()
        };
        if targets.is_empty() {
            return (false, false);
        }
        let inner = f.clone().retagged(Some(vlan.get())).encode();
        let mut any = false;
        for ap in targets {
            if let Some(a) = t.device(ap).cloned() {
                any |= self.capwap_send(t, d, &a, capwap::data_encode_8023(&inner), capwap::DATA_PORT).is_ok();
            }
        }
        (!f.dst.is_group() && any, any)
    }

    /// At a lightweight AP: a frame its controller tunnelled back, put on the air to the
    /// centrally switched hosts of its VLAN (the one it names, or all of them for a broadcast).
    pub(crate) fn central_to_air(&mut self, t: &Topology, me: &Device, frame: &[u8]) -> Result<(), String> {
        let f = Frame::decode(frame).map_err(|e| e.to_string())?;
        let vlan = f.vlan.and_then(VlanId::new).ok_or("tunnelled frame without its VLAN")?;
        let radio = me.ports.iter().position(|p| ifname::is_wireless(&p.name)).ok_or("no radio")?;
        let radio = End { device: me.id, port: radio };
        let plain = f.clone().retagged(None).encode();
        let mut sent = 0;
        for c in t.wifi_clients(radio) {
            if t.central_client(c).is_none_or(|(_, ap, v)| ap != radio || v != vlan) {
                continue;
            }
            let mac = port_mac(t, c);
            if mac == f.src || (!f.dst.is_group() && mac != f.dst) {
                continue;
            }
            self.push_hop(t, self.now + self.hop_delay(), radio, c, plain.clone(), 0);
            sent += 1;
        }
        if sent == 0 { Err(format!("no centrally switched host of VLAN {vlan} on this radio for it")) } else { Ok(()) }
    }
}
