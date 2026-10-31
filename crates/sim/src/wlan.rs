//! Wi-Fi on the air and wireless LAN controllers on the wire.
//!
//! Every hop between a Wi-Fi host and its access point carries a real IEEE 802.11 frame: data
//! frames (LLC/SNAP, then the Ethernet payload) and the management frames of association
//! (beacons, probe, open-system authentication, association, and reassociation when a host
//! roams to another access point of its network), plus EAPOL: the WPA2 4-way handshake (with
//! MICs from the real PBKDF2/PRF key hierarchy) and, on WPA2-Enterprise WLANs, first the EAP
//! identity exchange that the controller checks with its RADIUS server.
//!
//! After the handshake, data frames on an RSN network are protected with CCMP (AES-CCM) under
//! the temporal key the handshake derived, with per-direction packet numbers checked against
//! replays. The event list and the Ethernet capture show their plaintext equivalents; the
//! 802.11 capture shows them protected. Each access point radio holds a group temporal key (GTK,
//! derived from a group master key as IEEE 802.11 §12.7.1.4 has it) and hands it to every host
//! in message 3 of the handshake, inside the key data wrapped with the KEK (AES key wrap, RFC
//! 3394). Group-addressed frames from the access point (broadcasts, multicasts) are protected
//! with the GTK (key id 1) and a group packet number each host checks for replays; frames from
//! a host stay under its pairwise key. On WPA2-Enterprise WLANs the host logs in with PEAP
//! (see [`crate::peap`]): the controller passes each EAP message between the host (EAPOL, via
//! the AP over CAPWAP data) and its RADIUS server (EAP-Message with Message-Authenticator,
//! RFC 3579); the server runs the tunnel and the inner EAP-GTC login, and its Access-Accept
//! hands the controller the session key (MPPE key attribute), which keys the 4-way handshake
//! as the host's own end of the tunnel does.
//!
//! Access points beacon every [`BEACON_TU`] time units (about two seconds, slower than real
//! ones, so the event list stays readable) to the Wi-Fi hosts in range.
//!
//! A WLAN can map to a VLAN (`wlan N SSID ... vlan V`): its hosts' frames enter the access
//! point in that VLAN and leave on its uplink tagged, so each WLAN can have its own subnet.
//!
//! Lightweight APs join their controller with a CAPWAP exchange (discovery, join, configuration
//! status, then the IEEE 802.11 WLAN configuration that pushes the WLANs) over UDP 5246, and
//! relay EAPOL to it over the CAPWAP data channel (UDP 5247). Host traffic itself is bridged
//! locally by the AP onto its wired port, or, on a centrally switched WLAN, tunnelled to the
//! controller over CAPWAP data and bridged there (see [`crate::central`]).
//!
//! Who may join and associate is worked out from the lab (`Topology::wifi_attempt`,
//! `Topology::lap_controller`), as the routing protocols are; these exchanges show it on the
//! air and the wire. A host's frames wait until its association (and handshake) completes.

use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr, VlanId};
use netlabcraft_model::wlan::{Attempt, WlanSecurity, is_lap, is_wlc, mgmt_addr};
use netlabcraft_model::{Device, End, Topology, ifname};
use netlabcraft_wire::capwap::{self, Control};
use netlabcraft_wire::radius::{ACCESS_ACCEPT, ACCESS_CHALLENGE, RADIUS_PORT, Radius};
use netlabcraft_wire::wlan::{
    self, Akm, Body, Bss, Dot11, EAP_FAILURE, EAP_IDENTITY, EAP_PEAP, EAP_REQUEST, EAP_RESPONSE, EAP_SUCCESS, ETHERTYPE_EAPOL, Eap, Eapol, EapolKey,
    KEY_ACK, KEY_INSTALL, KEY_MIC, KEY_PAIRWISE, KEY_SECURE, KEY_VERSION_2, PEAP_START,
};
use netlabcraft_wire::{Frame, Ipv4, PROTO_UDP, Udp};

use crate::{Queued, Sim, default_ttl, is_ap_radio};

/// Most frames a host's association keeps waiting.
const MAX_HELD: usize = 64;
/// The beacon interval access points advertise and keep, in time units of 1024 µs.
pub const BEACON_TU: u16 = 2048;
pub const BEACON_EVERY_US: u64 = BEACON_TU as u64 * 1024;
/// A long run only replays the last few beacons.
const BEACON_CATCH_UP_US: u64 = 10 * BEACON_EVERY_US;
/// Most beacon frames queued per beacon interval.
const MAX_BEACONS: usize = 64;

/// What the simulator has shown of one host's association.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assoc {
    pub ap: Option<End>,
    /// The attempt this association plays out (access point, network, outcome).
    key: String,
    /// Associated and through any handshake: its frames flow.
    pub done: bool,
    aid: u16,
    anonce: [u8; 32],
    snonce: [u8; 32],
    replay: u64,
    eap_id: u8,
    /// Frames waiting for the association: (from, to, frame, hops).
    held: Vec<(End, End, Vec<u8>, u8)>,
    /// The network joined (to tell a roam from a change of network).
    ssid: String,
    /// Moving from this access point: the host reassociates instead of associating.
    roam_from: Option<End>,
    /// The 802.1X identity the host gave (WPA2-Enterprise), for the controller's RADIUS request.
    identity: String,
    /// The CCMP temporal key, once the 4-way handshake installed it.
    tk: Option<[u8; 16]>,
    /// The group key message 3 delivered, and the last group packet number accepted.
    gtk: Option<[u8; 16]>,
    rx_group: u64,
    /// Packet numbers sent by the host and by the AP, and the last each accepted.
    pn_up: u64,
    pn_down: u64,
    rx_up: u64,
    rx_down: u64,
    /// WPA2-Enterprise: the host's end of its PEAP tunnel.
    peap: Option<crate::peap::ClientTunnel>,
    /// The controller's view: the RADIUS server's State for this login, and the master key
    /// its Access-Accept delivered.
    radius_state: Option<Vec<u8>>,
    pmk_ap: Option<[u8; 32]>,
}

impl Assoc {
    /// Whether data frames to and from the host are CCMP-protected.
    pub fn protected(&self) -> bool {
        self.tk.is_some()
    }
}

/// A WPA2-Enterprise login the controller asked its RADIUS server about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dot1xWait {
    wlc: DeviceId,
    lap: DeviceId,
    client: End,
    id: u8,
    authenticator: [u8; 16],
    secret: String,
}

/// What the simulator has shown of a lightweight AP's join: its controller and the WLANs pushed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Joined {
    wlc: Option<DeviceId>,
    wlans: Vec<netlabcraft_model::wlan::Wlan>,
}

fn port_mac(t: &Topology, e: End) -> MacAddr {
    t.device(e.device).and_then(|d| d.ports.get(e.port)).map_or(MacAddr::ZERO, |p| p.mac)
}

fn is_radio(t: &Topology, e: End) -> bool {
    t.device(e.device).and_then(|d| d.ports.get(e.port)).is_some_and(|p| ifname::is_wireless(&p.name))
}

fn is_host(t: &Topology, e: End) -> bool {
    t.device(e.device).is_some_and(|d| d.kind.is_host())
}

fn akm(s: &WlanSecurity) -> Option<Akm> {
    match s {
        WlanSecurity::Open => None,
        WlanSecurity::Psk(_) => Some(Akm::Psk),
        WlanSecurity::Enterprise => Some(Akm::Dot1x),
    }
}

/// A hop through the air: between a host's radio and an access point's radio.
pub fn air_hop(t: &Topology, a: End, b: End) -> bool {
    is_radio(t, a) && is_radio(t, b)
}

/// The VLAN a frame from `src` enters an access point's radio in: the sender's WLAN's.
pub(crate) fn radio_vlan(t: &Topology, ap: End, src: MacAddr) -> Option<VlanId> {
    if !is_ap_radio(t, ap) {
        return None;
    }
    let client = host_with_mac(t, src)?;
    if t.wifi_ap_of(client) != Some(ap) {
        return None;
    }
    t.wifi_vlan(client)
}

/// The key id group frames are sent under.
const GTK_KEY_ID: u8 = 1;

/// An access point radio's group key: PRF-128 of a group master key over its address and a
/// group nonce (IEEE 802.11 §12.7.1.4), both made up from the radio's address.
fn group_key(t: &Topology, ap: End) -> [u8; 16] {
    let aa = port_mac(t, ap).0;
    let gmk = netlabcraft_wire::sha1::sha1(&[b"group master key".as_slice(), &aa].concat());
    let gnonce = nonce(&format!("gnonce{:?}", aa));
    let mut data = aa.to_vec();
    data.extend_from_slice(&gnonce);
    let k = netlabcraft_wire::sha1::prf(&gmk, "Group key expansion", &data, 16);
    let mut out = [0u8; 16];
    for (o, b) in out.iter_mut().zip(k) {
        *o = b;
    }
    out
}

fn nonce(seed: &str) -> [u8; 32] {
    let a = netlabcraft_wire::sha1::sha1(seed.as_bytes());
    let b = netlabcraft_wire::sha1::sha1(&a);
    let mut out = [0u8; 32];
    out[..20].copy_from_slice(&a);
    out[20..].copy_from_slice(&b[..12]);
    out
}

impl Sim {
    /// The association a host's radio has, as far as the air has shown.
    pub fn association(&self, client: End) -> Option<&Assoc> {
        self.assoc.get(&client)
    }

    fn air_seq(&mut self) -> u16 {
        self.air_seq = (self.air_seq + 1) & 0x0fff;
        self.air_seq
    }

    /// Puts bytes on the air (or a cable) as they are.
    fn enqueue(&mut self, at: u64, from: End, to: End, frame: Vec<u8>, hops: u8) {
        self.seq += 1;
        self.origin_queued(self.seq);
        self.queue.push(std::cmp::Reverse(Queued { at, seq: self.seq, from, to, frame, hops, cause: self.cause }));
    }

    fn send_air(&mut self, from: End, to: End, f: Dot11) {
        let at = self.now + self.hop_delay();
        self.enqueue(at, from, to, f.encode(), 0);
    }

    /// Queues an Ethernet frame for one hop, at `at`. Across the air it becomes an 802.11 data
    /// frame, and it waits while the host is still associating.
    pub(crate) fn push_hop(&mut self, t: &Topology, at: u64, from: End, to: End, frame: Vec<u8>, hops: u8) {
        if !air_hop(t, from, to) {
            self.enqueue(at, from, to, frame, hops);
            return;
        }
        let Ok(eth) = Frame::decode(&frame) else { return };
        let (client, ap) = if is_host(t, from) { (from, to) } else { (to, from) };
        let bssid = port_mac(t, ap);
        let seq = self.air_seq();
        let air = if client == from {
            Dot11::to_ap(bssid, eth.src, eth.dst, seq, eth.ethertype, eth.payload)
        } else {
            Dot11::from_ap(eth.dst, bssid, eth.src, seq, eth.ethertype, eth.payload)
        }
        .encode();
        if self.assoc.get(&client).is_none_or(|a| !a.done || a.ap != Some(ap)) {
            self.wlan_sync(t);
            if let Some(a) = self.assoc.get_mut(&client).filter(|a| a.ap == Some(ap) && !a.done) {
                if a.held.len() < MAX_HELD {
                    a.held.push((from, to, air, hops));
                }
                return;
            }
        }
        let air = if client != from && eth.dst.is_group() { self.seal_group(client, from, air) } else { self.seal(client, client == from, air) };
        self.enqueue(at, from, to, air, hops);
    }

    /// Protects a group-addressed frame from an access point with its group key, under the
    /// radio's group packet number, when the host's handshake gave it that key.
    fn seal_group(&mut self, client: End, ap: End, air: Vec<u8>) -> Vec<u8> {
        // The host's copy is the radio's group key (message 3 delivered it).
        let Some(gtk) = self.assoc.get(&client).and_then(|a| a.gtk) else { return air };
        let pn = self.group_pn.entry(ap).or_insert(0);
        *pn = pn.saturating_add(1).min(wlan::MAX_PN);
        wlan::ccmp_encrypt(&gtk, &air, *pn, GTK_KEY_ID).unwrap_or(air)
    }

    /// Protects a data frame with CCMP when the host's handshake installed a key.
    fn seal(&mut self, client: End, up: bool, air: Vec<u8>) -> Vec<u8> {
        let Some(a) = self.assoc.get_mut(&client) else { return air };
        let Some(tk) = a.tk else { return air };
        let pn = if up { &mut a.pn_up } else { &mut a.pn_down };
        *pn = pn.saturating_add(1).min(wlan::MAX_PN);
        wlan::ccmp_encrypt(&tk, &air, *pn, 0).unwrap_or(air)
    }

    /// The plaintext of a protected frame on the air, for the event list (no replay check).
    pub(crate) fn air_plain(&self, t: &Topology, from: End, to: End, bytes: &[u8]) -> Option<Dot11> {
        let f = Dot11::decode(bytes).ok()?;
        if !f.is_protected() {
            return Some(f);
        }
        let client = if is_host(t, from) { from } else { to };
        let a = self.assoc.get(&client)?;
        let key = if !is_host(t, from) && f.addr1.0[0] & 1 == 1 { a.gtk? } else { a.tk? };
        let (plain, _) = wlan::ccmp_decrypt(&key, bytes).ok()?;
        Dot11::decode(&plain).ok()
    }

    /// Opens a protected frame arriving at `at`: checks the MIC with the host's key and that
    /// the packet number moved on.
    fn unprotect(&mut self, t: &Topology, from: End, at: End, bytes: &[u8]) -> Result<Dot11, String> {
        let up = !is_host(t, at);
        let client = if up { from } else { at };
        let group = !up && Dot11::decode(bytes).is_ok_and(|f| f.addr1.0[0] & 1 == 1);
        let a = self.assoc.get_mut(&client).ok_or("protected frame from a host that is not associated")?;
        if group {
            let gtk = a.gtk.ok_or("group-addressed frame, but no group key installed")?;
            let (plain, pn) = wlan::ccmp_decrypt(&gtk, bytes).map_err(|e| format!("group key: {e}"))?;
            if pn <= a.rx_group {
                return Err(format!("CCMP replay: group packet number {pn} is not above the last one ({})", a.rx_group));
            }
            a.rx_group = pn;
            return Dot11::decode(&plain).map_err(|e| e.to_string());
        }
        let tk = a.tk.ok_or("protected frame, but no key installed")?;
        let (plain, pn) = wlan::ccmp_decrypt(&tk, bytes).map_err(|e| e.to_string())?;
        let last = if up { &mut a.rx_up } else { &mut a.rx_down };
        if pn <= *last {
            return Err(format!("CCMP replay: packet number {pn} is not above the last one ({last})"));
        }
        *last = pn;
        Dot11::decode(&plain).map_err(|e| e.to_string())
    }

    /// Queues the beacons due in `(self.beacon_until, deadline]`: every access point radio
    /// that offers networks beacons each of them to the Wi-Fi hosts in range.
    pub(crate) fn schedule_beacons(&mut self, t: &Topology, deadline: u64) {
        let from = self.beacon_until.max(deadline.saturating_sub(BEACON_CATCH_UP_US));
        if deadline <= from {
            return;
        }
        self.beacon_until = deadline;
        let hosts: Vec<End> = t
            .devices
            .iter()
            .filter(|d| d.kind.is_host())
            .flat_map(|d| {
                d.ports
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| ifname::is_wireless(&p.name) && !p.shutdown)
                    .map(move |(i, _)| End { device: d.id, port: i })
            })
            .collect();
        if hosts.is_empty() {
            return;
        }
        let mut plan: Vec<(End, End, Body)> = Vec::new();
        for d in t.devices.iter().filter(|d| d.kind == netlabcraft_model::DeviceKind::Switch) {
            for p in (0..d.ports.len()).filter(|p| is_ap_radio(t, End { device: d.id, port: *p })) {
                let ap = End { device: d.id, port: p };
                let offers = t.offers(ap);
                for h in &hosts {
                    if t.device(h.device).is_none_or(|hd| !t.wifi_in_range(hd, d)) {
                        continue;
                    }
                    for o in &offers {
                        let bss = Bss { ssid: o.ssid.clone(), channel: o.channel, rsn: akm(&o.security), interval: BEACON_TU, timestamp: 0 };
                        plan.push((ap, *h, Body::Beacon(bss)));
                    }
                }
            }
        }
        plan.truncate(MAX_BEACONS);
        let mut at = (from / BEACON_EVERY_US + 1) * BEACON_EVERY_US;
        while at <= deadline {
            for (ap, h, body) in &plan {
                let mut body = body.clone();
                if let Body::Beacon(b) = &mut body {
                    b.timestamp = at;
                }
                let seq = self.air_seq();
                let bssid = port_mac(t, *ap);
                let f = Dot11::mgmt(MacAddr::BROADCAST, bssid, bssid, seq, body);
                self.enqueue(at + self.hop_delay(), *ap, *h, f.encode(), 0);
            }
            at += BEACON_EVERY_US;
        }
    }

    /// Plays out what changed since last time: lightweight APs joining (or reconfigured by)
    /// their controller, and hosts associating with an access point.
    pub(crate) fn wlan_sync(&mut self, t: &Topology) {
        // ---- CAPWAP ----
        let laps: Vec<DeviceId> = t.devices.iter().filter(|d| is_lap(d)).map(|d| d.id).collect();
        self.joined.retain(|id, _| laps.contains(id));
        for lap in laps {
            let wlc = t.lap_controller(lap);
            let wlans = wlc.and_then(|w| t.device(w)).map(|w| w.wlan.wlans.clone()).unwrap_or_default();
            let before = self.joined.get(&lap).cloned().unwrap_or_default();
            if before.wlc == wlc && before.wlans == wlans {
                continue;
            }
            self.joined.insert(lap, Joined { wlc, wlans: wlans.clone() });
            let (Some(w), Some(ld), Some(wd)) = (wlc, t.device(lap), wlc.and_then(|w| t.device(w))) else { continue };
            if before.wlc == Some(w) {
                // Same controller, WLANs changed: it pushes the difference.
                let removed: Vec<u8> = before.wlans.iter().filter(|o| !wlans.iter().any(|n| n.id == o.id)).map(|o| o.id).collect();
                let msg = self.wlan_config(wd, &wlans, &removed);
                let _ = self.capwap_send(t, wd, ld, msg.encode(), capwap::CONTROL_PORT);
            } else {
                let msg = Control::new(capwap::DISCOVERY_REQUEST, self.capwap_seq()).with(capwap::WTP_NAME, ld.name.as_bytes().to_vec());
                let _ = self.capwap_send(t, ld, wd, msg.encode(), capwap::CONTROL_PORT);
            }
        }
        // ---- association ----
        let clients: Vec<End> = t
            .devices
            .iter()
            .filter(|d| d.kind.is_host())
            .flat_map(|d| d.ports.iter().enumerate().filter(|(_, p)| ifname::is_wireless(&p.name)).map(move |(i, _)| End { device: d.id, port: i }))
            .collect();
        self.assoc.retain(|c, _| clients.contains(c));
        for client in clients {
            let attempt = t.wifi_attempt(client);
            let key =
                attempt.as_ref().map(|a| format!("{:?}/{}/{:?}/{}", a.ap, a.offer.ssid, a.offer.security, a.outcome.is_ok())).unwrap_or_default();
            if self.assoc.get(&client).is_some_and(|a| a.key == key) {
                continue;
            }
            match attempt {
                None => {
                    self.assoc.remove(&client);
                }
                Some(a) => self.start_association(t, client, &a, key),
            }
        }
    }

    fn start_association(&mut self, t: &Topology, client: End, a: &Attempt, key: String) {
        let old = self.assoc.get(&client).filter(|x| x.done).and_then(|x| x.ap.filter(|ap| *ap != a.ap).map(|ap| (ap, x.ssid.clone())));
        let old = old.filter(|(ap, _)| t.device(ap.device).is_some());
        // Same network, another access point: a roam. The host reassociates (and keeps its
        // address); leaving for another network, it tells the old AP first.
        let roam_from = old.as_ref().filter(|(_, ssid)| *ssid == a.offer.ssid && a.outcome.is_ok()).map(|(ap, _)| *ap);
        if let (Some((ap, _)), None) = (&old, roam_from) {
            let seq = self.air_seq();
            let bssid = port_mac(t, *ap);
            self.send_air(client, *ap, Dot11::mgmt(bssid, port_mac(t, client), bssid, seq, Body::Deauth { reason: wlan::REASON_LEAVING }));
        }
        let aid = 1 + self.assoc.values().filter(|x| x.ap == Some(a.ap)).count() as u16;
        self.assoc.insert(client, Assoc { ap: Some(a.ap), key, aid, ssid: a.offer.ssid.clone(), roam_from, ..Assoc::default() });
        let seq = self.air_seq();
        let me = port_mac(t, client);
        let first = if roam_from.is_some() {
            // It knows the network already: straight to authentication with the new AP.
            let bssid = port_mac(t, a.ap);
            Dot11::mgmt(bssid, me, bssid, seq, Body::Auth { algorithm: 0, transaction: 1, status: 0 })
        } else {
            Dot11::mgmt(MacAddr::BROADCAST, me, MacAddr::BROADCAST, seq, Body::ProbeRequest { ssid: a.offer.ssid.clone() })
        };
        self.send_air(client, a.ap, first);
    }

    /// Lets a host's waiting frames go.
    fn associated(&mut self, client: End) {
        let now = self.now;
        let held = match self.assoc.get_mut(&client) {
            Some(a) => {
                a.done = true;
                std::mem::take(&mut a.held)
            }
            None => return,
        };
        for (from, to, frame, hops) in held {
            let group = client != from && Dot11::decode(&frame).is_ok_and(|f| f.addr1.0[0] & 1 == 1);
            let frame = if group { self.seal_group(client, from, frame) } else { self.seal(client, client == from, frame) };
            self.enqueue(now + self.hop_delay(), from, to, frame, hops);
        }
    }

    /// An 802.11 frame arriving at a radio.
    pub(crate) fn air_in(&mut self, t: &Topology, from: End, at: End, bytes: &[u8], hops: u8) -> Result<(), String> {
        let mut f = Dot11::decode(bytes).map_err(|e| e.to_string())?;
        let me = port_mac(t, at);
        if f.addr1 != me && f.addr1.0[0] & 1 == 0 {
            return Err(format!("802.11 frame for {}, not this radio", f.addr1));
        }
        if t.device(at.device).and_then(|d| d.ports.get(at.port)).is_none_or(|p| p.shutdown) {
            return Err("radio is off".into());
        }
        if f.is_protected() {
            f = self.unprotect(t, from, at, bytes)?;
        } else if f.is_data()
            && f.ethernet().is_some_and(|e| e.ethertype != ETHERTYPE_EAPOL)
            && self.assoc.get(&if is_host(t, at) { at } else { from }).is_some_and(|a| a.done && a.protected())
        {
            return Err("unprotected data on a WPA2 network (CCMP expected)".into());
        }
        match &f.body {
            Body::Data { ethertype, payload } if *ethertype == ETHERTYPE_EAPOL => {
                let e = Eapol::decode(payload).map_err(|e| e.to_string())?;
                if is_host(t, at) { self.eapol_client_in(t, at, from, e) } else { self.eapol_ap_in(t, at, from, &f, e) }
            }
            Body::Data { .. } => {
                let eth = f.ethernet().ok_or("802.11 data without addresses")?;
                if self.assoc.get(&if is_host(t, at) { at } else { from }).is_none_or(|a| !a.done) {
                    return Err("data from a host that is not associated".into());
                }
                self.receive(t, at, &eth.encode(), hops)
            }
            // Hosts hear the beacons of every access point in range.
            Body::Beacon(_) if is_host(t, at) => Ok(()),
            _ if is_host(t, at) => self.mgmt_client_in(t, at, from, &f),
            _ => self.mgmt_ap_in(t, at, from, &f),
        }
    }

    fn attempt_of(&self, t: &Topology, client: End, ap: End) -> Option<Attempt> {
        t.wifi_attempt(client).filter(|a| a.ap == ap)
    }

    /// Management frames at an access point: answer probes, authentication and association.
    fn mgmt_ap_in(&mut self, t: &Topology, ap: End, client: End, f: &Dot11) -> Result<(), String> {
        let a = self.attempt_of(t, client, ap).ok_or("from a host that is not joining this access point")?;
        let (me, sta) = (port_mac(t, ap), f.addr2);
        let seq = self.air_seq();
        let reply = match &f.body {
            Body::ProbeRequest { ssid } if ssid.is_empty() || *ssid == a.offer.ssid => Body::ProbeResponse(Bss {
                ssid: a.offer.ssid.clone(),
                channel: a.offer.channel,
                rsn: akm(&a.offer.security),
                interval: BEACON_TU,
                timestamp: self.now,
            }),
            Body::ProbeRequest { .. } => return Err("probe for a network this access point doesn't offer".into()),
            Body::Auth { transaction: 1, .. } => Body::Auth { algorithm: 0, transaction: 2, status: wlan::STATUS_SUCCESS },
            Body::AssocRequest { ssid, .. } if *ssid == a.offer.ssid => {
                let aid = self.assoc.get(&client).map_or(1, |x| x.aid);
                Body::AssocResponse { status: wlan::STATUS_SUCCESS, aid }
            }
            Body::AssocRequest { .. } => Body::AssocResponse { status: wlan::STATUS_REFUSED, aid: 0 },
            Body::ReassocRequest { ssid, .. } if *ssid == a.offer.ssid => {
                let aid = self.assoc.get(&client).map_or(1, |x| x.aid);
                Body::ReassocResponse { status: wlan::STATUS_SUCCESS, aid }
            }
            Body::ReassocRequest { .. } => Body::ReassocResponse { status: wlan::STATUS_REFUSED, aid: 0 },
            Body::Deauth { .. } | Body::Disassoc { .. } => return Ok(()),
            _ => return Err("unexpected management frame".into()),
        };
        let accepted =
            matches!(reply, Body::AssocResponse { status: wlan::STATUS_SUCCESS, .. } | Body::ReassocResponse { status: wlan::STATUS_SUCCESS, .. });
        self.send_air(ap, client, Dot11::mgmt(sta, me, me, seq, reply));
        if !accepted {
            return Ok(());
        }
        // After association: the WPA2 4-way handshake, or an 802.1X login.
        match &a.offer.security {
            WlanSecurity::Open => {}
            WlanSecurity::Psk(_) => self.start_handshake(t, ap, client),
            WlanSecurity::Enterprise => {
                let id = {
                    let st = self.assoc.entry(client).or_default();
                    st.eap_id = st.eap_id.wrapping_add(1);
                    st.eap_id
                };
                self.send_eapol(t, ap, client, Eapol::Eap(Eap { code: EAP_REQUEST, id, kind: Some(EAP_IDENTITY), data: Vec::new() }));
            }
        }
        Ok(())
    }

    /// Message 1 of the 4-way handshake, from the access point.
    fn start_handshake(&mut self, t: &Topology, ap: End, client: End) {
        let anonce = nonce(&format!("a{}{}{:?}", self.now, self.seq, client));
        let replay = {
            let st = self.assoc.entry(client).or_default();
            st.anonce = anonce;
            st.replay += 1;
            st.replay
        };
        let key = EapolKey { info: KEY_VERSION_2 | KEY_PAIRWISE | KEY_ACK, key_len: 16, replay, nonce: anonce, mic: [0; 16], data: Vec::new() };
        self.send_eapol(t, ap, client, Eapol::Key(key));
    }

    /// Management frames at a host: the next step of joining.
    fn mgmt_client_in(&mut self, t: &Topology, client: End, ap: End, f: &Dot11) -> Result<(), String> {
        let a = self.attempt_of(t, client, ap).ok_or("from an access point this host isn't joining")?;
        let (me, bssid) = (port_mac(t, client), port_mac(t, ap));
        let seq = self.air_seq();
        let next = match &f.body {
            Body::ProbeResponse(_) | Body::Beacon(_) => Body::Auth { algorithm: 0, transaction: 1, status: 0 },
            Body::Auth { transaction: 2, status: wlan::STATUS_SUCCESS, .. } => match self.assoc.get(&client).and_then(|x| x.roam_from) {
                Some(old) => Body::ReassocRequest { ssid: a.offer.ssid.clone(), rsn: akm(&a.offer.security), current_ap: port_mac(t, old) },
                None => Body::AssocRequest { ssid: a.offer.ssid.clone(), rsn: akm(&a.offer.security) },
            },
            Body::AssocResponse { status: wlan::STATUS_SUCCESS, .. } | Body::ReassocResponse { status: wlan::STATUS_SUCCESS, .. } => {
                if a.offer.security == WlanSecurity::Open {
                    self.associated(client);
                }
                return Ok(());
            }
            Body::Deauth { reason } | Body::Disassoc { reason } => {
                if let Some(st) = self.assoc.get_mut(&client) {
                    st.done = false;
                    st.held.clear();
                }
                return Err(format!("the access point turned the host away (reason {reason})"));
            }
            Body::Auth { status, .. } | Body::AssocResponse { status, .. } | Body::ReassocResponse { status, .. } => {
                return Err(format!("refused (status {status})"));
            }
            _ => return Err("unexpected management frame".into()),
        };
        self.send_air(client, ap, Dot11::mgmt(bssid, me, bssid, seq, next));
        Ok(())
    }

    fn send_eapol(&mut self, t: &Topology, from: End, to: End, e: Eapol) {
        let seq = self.air_seq();
        let (sm, tm) = (port_mac(t, from), port_mac(t, to));
        let f = if is_host(t, from) {
            Dot11::to_ap(tm, sm, tm, seq, ETHERTYPE_EAPOL, e.encode())
        } else {
            Dot11::from_ap(tm, sm, sm, seq, ETHERTYPE_EAPOL, e.encode())
        };
        self.send_air(from, to, f);
    }

    /// The pairwise master key of a WPA2-Personal network (cached: PBKDF2 is slow on purpose).
    fn pmk(&mut self, pass: &str, ssid: &str) -> [u8; 32] {
        let k = (pass.to_string(), ssid.to_string());
        if let Some(p) = self.pmks.get(&k) {
            return *p;
        }
        let p = netlabcraft_wire::sha1::wpa_pmk(pass, ssid);
        if self.pmks.len() < 64 {
            self.pmks.insert(k, p);
        }
        p
    }

    /// The PMK a host and its access point share: from the passphrase (WPA2-Personal), or on
    /// WPA2-Enterprise the key the PEAP tunnel exported (the host's end), as the RADIUS server
    /// delivered it to the controller (the access point's). Without one, each side's stand-in
    /// differs, so the handshake fails.
    /// `ap_side`: the access point's view (its own passphrase) rather than the host's.
    fn pmk_for(&mut self, t: &Topology, offer: &netlabcraft_model::wlan::Offer, client: End, ap_side: bool) -> [u8; 32] {
        let mine = t.device(client.device).and_then(|d| d.ports.get(client.port)).and_then(|p| p.wifi.clone()).unwrap_or_default();
        match &offer.security {
            WlanSecurity::Enterprise => {
                let a = self.assoc.get(&client);
                if ap_side {
                    a.and_then(|a| a.pmk_ap).unwrap_or([0; 32])
                } else {
                    a.and_then(|a| a.peap.as_ref()).and_then(|p| p.pmk).unwrap_or([0xff; 32])
                }
            }
            WlanSecurity::Psk(pass) if ap_side => self.pmk(pass, &offer.ssid),
            _ => self.pmk(mine.key.as_deref().unwrap_or(""), &offer.ssid),
        }
    }

    /// The pairwise transient key: KCK (bytes 0-15), KEK (16-31), TK (32-47).
    fn ptk(&mut self, t: &Topology, pmk: &[u8; 32], ap: End, client: End, anonce: &[u8; 32], snonce: &[u8; 32]) -> Vec<u8> {
        netlabcraft_wire::sha1::wpa_ptk(pmk, &port_mac(t, ap).0, &port_mac(t, client).0, anonce, snonce)
    }

    fn kck(&mut self, t: &Topology, pmk: &[u8; 32], ap: End, client: End, anonce: &[u8; 32], snonce: &[u8; 32]) -> Vec<u8> {
        self.ptk(t, pmk, ap, client, anonce, snonce).get(..16).unwrap_or_default().to_vec()
    }

    /// EAPOL at a host: the handshake from its side, or the end of its 802.1X login.
    fn eapol_client_in(&mut self, t: &Topology, client: End, ap: End, e: Eapol) -> Result<(), String> {
        let a = self.attempt_of(t, client, ap).ok_or("from an access point this host isn't joining")?;
        let mine = t.device(client.device).and_then(|d| d.ports.get(client.port)).and_then(|p| p.wifi.clone()).unwrap_or_default();
        let rsn = akm(&a.offer.security).unwrap_or(Akm::Psk);
        match e {
            Eapol::Key(k) if k.message() == 1 => {
                let snonce = nonce(&format!("s{}{}{:?}", self.now, self.seq, client));
                let pmk = self.pmk_for(t, &a.offer, client, false);
                let kck = self.kck(t, &pmk, ap, client, &k.nonce, &snonce);
                if let Some(st) = self.assoc.get_mut(&client) {
                    st.snonce = snonce;
                    st.anonce = k.nonce;
                }
                let mut reply = EapolKey {
                    info: KEY_VERSION_2 | KEY_PAIRWISE | KEY_MIC,
                    key_len: 0,
                    replay: k.replay,
                    nonce: snonce,
                    mic: [0; 16],
                    data: wlan::rsn_ie(rsn),
                };
                reply.mic = wlan::key_mic(&kck, &reply);
                self.send_eapol(t, client, ap, Eapol::Key(reply));
                Ok(())
            }
            Eapol::Key(k) if k.message() == 3 => {
                let (anonce, snonce) = self.assoc.get(&client).map(|s| (s.anonce, s.snonce)).ok_or("no handshake under way")?;
                let pmk = self.pmk_for(t, &a.offer, client, false);
                let ptk = self.ptk(t, &pmk, ap, client, &anonce, &snonce);
                let kck = ptk.get(..16).unwrap_or_default();
                if wlan::key_mic(kck, &k) != k.mic {
                    return Err("handshake message 3 has a bad MIC".into());
                }
                let kek: [u8; 16] = ptk.get(16..32).and_then(|x| x.try_into().ok()).ok_or("no KEK")?;
                let gtk = if k.info & wlan::KEY_ENCRYPTED_DATA != 0 {
                    let data = wlan::unwrap_key_data(&kek, &k.data).ok_or("handshake message 3: the key data doesn't unwrap with the KEK")?;
                    Some(wlan::find_gtk(&data).ok_or("handshake message 3 carries no group key")?.1)
                } else {
                    None
                };
                if let Some(st) = self.assoc.get_mut(&client) {
                    st.gtk = gtk;
                    st.rx_group = 0;
                }
                let mut reply = EapolKey {
                    info: KEY_VERSION_2 | KEY_PAIRWISE | KEY_MIC | KEY_SECURE,
                    key_len: 0,
                    replay: k.replay,
                    nonce: [0; 32],
                    mic: [0; 16],
                    data: Vec::new(),
                };
                reply.mic = wlan::key_mic(kck, &reply);
                self.send_eapol(t, client, ap, Eapol::Key(reply));
                // Both ends install the temporal key: data is CCMP-protected from now on.
                if let (Some(st), Some(tk)) = (self.assoc.get_mut(&client), ptk.get(32..48).and_then(|k| <[u8; 16]>::try_from(k).ok())) {
                    st.tk = Some(tk);
                }
                self.associated(client);
                Ok(())
            }
            Eapol::Eap(q) if q.code == EAP_REQUEST && q.kind == Some(EAP_IDENTITY) => {
                let user = mine.login.map(|l| l.user).unwrap_or_default();
                self.send_eapol(t, client, ap, Eapol::Eap(Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_IDENTITY), data: user.into_bytes() }));
                Ok(())
            }
            // PEAP: the host's end of the tunnel answers (see `crate::peap`); a start opens a new one.
            Eapol::Eap(q) if q.code == EAP_REQUEST && q.kind == Some(EAP_PEAP) => {
                let login = mine.login.unwrap_or_default();
                let start = q.data.first().is_some_and(|f| f & PEAP_START != 0);
                let seed = format!("peap{}{}{:?}", self.now, self.seq, client);
                let st = self.assoc.get_mut(&client).ok_or("no association under way")?;
                if start || st.peap.is_none() {
                    st.peap = Some(crate::peap::ClientTunnel::new(seed.as_bytes()));
                }
                let tunnel = st.peap.as_mut().ok_or("no tunnel")?;
                let data = tunnel.answer(&q.data, &login.user, &login.password)?;
                self.send_eapol(t, client, ap, Eapol::Eap(Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_PEAP), data }));
                Ok(())
            }
            // Logged in: the access point starts the 4-way handshake next.
            Eapol::Eap(q) if q.code == EAP_SUCCESS => Ok(()),
            Eapol::Eap(q) if q.code == EAP_FAILURE => Err("802.1X login refused".into()),
            _ => Err("unexpected EAPOL frame".into()),
        }
    }

    /// EAPOL at an access point: the handshake from its side, or an 802.1X identity to pass to
    /// the controller.
    fn eapol_ap_in(&mut self, t: &Topology, ap: End, client: End, f: &Dot11, e: Eapol) -> Result<(), String> {
        let a = self.attempt_of(t, client, ap).ok_or("from a host that is not joining this access point")?;
        let keyed = matches!(a.offer.security, WlanSecurity::Psk(_) | WlanSecurity::Enterprise);
        match (e, &a.offer.security) {
            (Eapol::Key(k), _) if keyed && k.message() == 2 => {
                let anonce = self.assoc.get(&client).map(|s| s.anonce).ok_or("no handshake under way")?;
                let pmk = self.pmk_for(t, &a.offer, client, true);
                let kck = self.kck(t, &pmk, ap, client, &anonce, &k.nonce);
                if wlan::key_mic(&kck, &k) != k.mic {
                    // A wrong passphrase: the MIC doesn't check out, and the host is sent away.
                    let (me, seq) = (port_mac(t, ap), self.air_seq());
                    self.send_air(ap, client, Dot11::mgmt(f.addr2, me, me, seq, Body::Deauth { reason: wlan::REASON_HANDSHAKE_TIMEOUT }));
                    return Err("handshake message 2 has a bad MIC (wrong passphrase)".into());
                }
                let replay = k.replay.saturating_add(1);
                // The key data: the RSN element and the group key, wrapped with the KEK.
                let ptk = self.ptk(t, &pmk, ap, client, &anonce, &k.nonce);
                let kek: [u8; 16] = ptk.get(16..32).and_then(|x| x.try_into().ok()).ok_or("no KEK")?;
                let mut plain = wlan::rsn_ie(akm(&a.offer.security).unwrap_or(Akm::Psk));
                plain.extend_from_slice(&wlan::gtk_kde(GTK_KEY_ID, &group_key(t, ap)));
                let data = wlan::wrap_key_data(&kek, &plain).ok_or("key data too long to wrap")?;
                let mut m3 = EapolKey {
                    info: KEY_VERSION_2 | KEY_PAIRWISE | KEY_INSTALL | KEY_ACK | KEY_MIC | KEY_SECURE | wlan::KEY_ENCRYPTED_DATA,
                    key_len: 16,
                    replay,
                    nonce: anonce,
                    mic: [0; 16],
                    data,
                };
                m3.mic = wlan::key_mic(&kck, &m3);
                self.send_eapol(t, ap, client, Eapol::Key(m3));
                Ok(())
            }
            (Eapol::Key(k), _) if keyed && k.message() == 4 => Ok(()),
            (Eapol::Eap(r), WlanSecurity::Enterprise) if r.code == EAP_RESPONSE && matches!(r.kind, Some(EAP_IDENTITY | EAP_PEAP)) => {
                // The lightweight AP hands the login to its controller over CAPWAP data.
                let lap = t.device(ap.device).ok_or("access point removed")?;
                let wlc = a.offer.controller.and_then(|w| t.device(w)).ok_or("no controller")?;
                self.capwap_send(t, lap, wlc, capwap::data_encode(&f.encode()), capwap::DATA_PORT)
            }
            (Eapol::Start, WlanSecurity::Enterprise) => Ok(()),
            _ => Err("unexpected EAPOL frame".into()),
        }
    }

    // ---- CAPWAP and RADIUS on the wire ----

    fn capwap_seq(&mut self) -> u8 {
        self.capwap_seq = self.capwap_seq.wrapping_add(1);
        self.capwap_seq
    }

    fn udp_send(&mut self, t: &Topology, d: &Device, dst: Ipv4Addr, sport: u16, dport: u16, payload: Vec<u8>) -> Result<(), String> {
        let (_, src) = mgmt_addr(d).ok_or_else(|| format!("{} has no management address", d.name))?;
        let u = Udp { src_port: sport, dst_port: dport, payload };
        let ip = Ipv4 { id: self.ip_id(), ttl: default_ttl(d.kind), protocol: PROTO_UDP, src: src.addr, dst, payload: u.encode(src.addr, dst) };
        self.send_ip(t, d, ip)
    }

    pub(crate) fn capwap_send(&mut self, t: &Topology, from: &Device, to: &Device, payload: Vec<u8>, port: u16) -> Result<(), String> {
        let (_, dst) = mgmt_addr(to).ok_or_else(|| format!("{} has no management address", to.name))?;
        self.udp_send(t, from, dst.addr, port, port, payload)
    }

    fn wlan_config(&mut self, wlc: &Device, wlans: &[netlabcraft_model::wlan::Wlan], removed: &[u8]) -> Control {
        let mut msg = Control::new(capwap::WLAN_CONFIG_REQUEST, self.capwap_seq());
        for id in removed {
            msg = msg.with(capwap::DELETE_WLAN, vec![1, *id]);
        }
        for w in wlans {
            let key = match &w.security {
                WlanSecurity::Psk(p) => self.pmk(p, &w.ssid).to_vec(),
                _ => Vec::new(),
            };
            let add = capwap::AddWlan { radio: 1, wlan_id: w.id, key, akm: akm(&w.security), ssid: w.ssid.clone(), central: w.central };
            msg = msg.with(capwap::ADD_WLAN, add.encode());
        }
        let _ = wlc;
        msg
    }

    /// UDP for CAPWAP and for the RADIUS answers of 802.1X logins; `None` when it is neither.
    pub(crate) fn wlan_udp_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, u: &Udp) -> Option<Result<(), String>> {
        if u.src_port == RADIUS_PORT && self.dot1x.contains_key(&u.dst_port) {
            return Some(self.radius_answer_in(t, d, u));
        }
        if ![capwap::CONTROL_PORT, capwap::DATA_PORT].contains(&u.dst_port) || !(is_wlc(d) || is_lap(d)) {
            return None;
        }
        let peer = t.devices.iter().find(|x| mgmt_addr(x).is_some_and(|(_, n)| n.addr == ip.src)).cloned();
        let Some(peer) = peer else { return Some(Err(format!("CAPWAP from {}, which is no access point or controller here", ip.src))) };
        Some(if u.dst_port == capwap::DATA_PORT {
            self.capwap_data_in(t, d, &peer, &u.payload)
        } else {
            self.capwap_control_in(t, d, &peer, &u.payload)
        })
    }

    fn capwap_control_in(&mut self, t: &Topology, me: &Device, peer: &Device, payload: &[u8]) -> Result<(), String> {
        let msg = Control::decode(payload).map_err(|e| e.to_string())?;
        let seq = msg.seq;
        let reply = if is_wlc(me) {
            if !is_lap(peer) {
                return Err("CAPWAP request from a device that is no lightweight AP".into());
            }
            let ours = t.lap_controller(peer.id) == Some(me.id);
            let name = me.name.as_bytes().to_vec();
            match msg.kind {
                capwap::DISCOVERY_REQUEST => Control::new(capwap::DISCOVERY_RESPONSE, seq).with(capwap::AC_NAME, name),
                capwap::JOIN_REQUEST => {
                    let code = if ours { capwap::RESULT_SUCCESS } else { capwap::RESULT_JOIN_REFUSED };
                    Control::new(capwap::JOIN_RESPONSE, seq).with(capwap::AC_NAME, name).with(capwap::RESULT_CODE, capwap::result_code(code))
                }
                capwap::CONFIG_STATUS_REQUEST if ours => {
                    let resp =
                        Control::new(capwap::CONFIG_STATUS_RESPONSE, seq).with(capwap::RESULT_CODE, capwap::result_code(capwap::RESULT_SUCCESS));
                    self.capwap_send(t, me, peer, resp.encode(), capwap::CONTROL_PORT)?;
                    // Then the WLANs.
                    self.wlan_config(me, &me.wlan.wlans, &[])
                }
                capwap::ECHO_REQUEST => Control::new(capwap::ECHO_RESPONSE, seq),
                capwap::WLAN_CONFIG_RESPONSE => return Ok(()),
                _ => return Err(format!("unexpected CAPWAP {}", msg.name())),
            }
        } else {
            match msg.kind {
                capwap::DISCOVERY_RESPONSE => {
                    Control::new(capwap::JOIN_REQUEST, self.capwap_seq()).with(capwap::WTP_NAME, me.name.as_bytes().to_vec())
                }
                capwap::JOIN_RESPONSE if msg.result() == Some(capwap::RESULT_SUCCESS) => {
                    Control::new(capwap::CONFIG_STATUS_REQUEST, self.capwap_seq()).with(capwap::WTP_NAME, me.name.as_bytes().to_vec())
                }
                capwap::JOIN_RESPONSE => return Err("the controller refused the join".into()),
                capwap::WLAN_CONFIG_REQUEST => {
                    Control::new(capwap::WLAN_CONFIG_RESPONSE, seq).with(capwap::RESULT_CODE, capwap::result_code(capwap::RESULT_SUCCESS))
                }
                capwap::CONFIG_STATUS_RESPONSE | capwap::ECHO_RESPONSE => return Ok(()),
                _ => return Err(format!("unexpected CAPWAP {}", msg.name())),
            }
        };
        self.capwap_send(t, me, peer, reply.encode(), capwap::CONTROL_PORT)
    }

    /// CAPWAP data: at a controller, a host's 802.1X identity to check; at an AP, the answer to
    /// put on the air.
    fn capwap_data_in(&mut self, t: &Topology, me: &Device, peer: &Device, payload: &[u8]) -> Result<(), String> {
        let (inner, native) = capwap::data_frame(payload).map_err(|e| e.to_string())?;
        // An 802.3 frame: a centrally switched host's traffic (see `crate::central`).
        if !native {
            return if is_wlc(me) { self.central_from_ap(t, me, peer, &inner) } else { self.central_to_air(t, me, &inner) };
        }
        let f = Dot11::decode(&inner).map_err(|e| e.to_string())?;
        if is_lap(me) {
            // Out of the radio, to the host it names.
            let radio = me.ports.iter().position(|p| ifname::is_wireless(&p.name)).ok_or("no radio")?;
            let client = host_with_mac(t, f.addr1).ok_or("the host left")?;
            let radio = End { device: me.id, port: radio };
            let success = matches!(&f.body, Body::Data { ethertype: ETHERTYPE_EAPOL, payload }
                if matches!(Eapol::decode(payload), Ok(Eapol::Eap(Eap { code: EAP_SUCCESS, .. }))));
            self.send_air(radio, client, f);
            // Logged in: the AP keys the host with the 4-way handshake.
            if success {
                self.start_handshake(t, radio, client);
            }
            return Ok(());
        }
        let Body::Data { ethertype: ETHERTYPE_EAPOL, payload } = &f.body else { return Err("CAPWAP data that is not EAPOL".into()) };
        let Ok(Eapol::Eap(r)) = Eapol::decode(payload) else { return Err("not an EAP packet".into()) };
        let client = host_with_mac(t, f.addr2).ok_or("from an unknown host")?;
        if r.code != EAP_RESPONSE {
            return Err("EAP from a host that is not a response".into());
        }
        // A pass-through authenticator: each EAP response goes to the RADIUS server inside an
        // Access-Request (RFC 3579), with the server's State; the identity starts a new login.
        if r.kind == Some(EAP_IDENTITY) {
            let user: String = String::from_utf8_lossy(&r.data).chars().take(64).collect();
            if let Some(st) = self.assoc.get_mut(&client) {
                st.identity = user;
                st.radius_state = None;
                st.pmk_ap = None;
            }
        }
        let (user, state) = self.assoc.get(&client).map(|a| (a.identity.clone(), a.radius_state.clone())).unwrap_or_default();
        let Some((server, secret)) = me.access.radius_servers.first().cloned() else {
            return self.dot1x_result(t, me.id, peer.id, client, r.id, false);
        };
        let (_, src) = mgmt_addr(me).ok_or("the controller has no management address")?;
        let port = self.next_port();
        self.seq += 1;
        let id = (self.seq % 256) as u8;
        let authenticator = netlabcraft_wire::md5::md5(format!("{}:{}:{user}", self.now, self.seq).as_bytes());
        let req = Radius::eap_request(id, authenticator, &user, &r.encode(), state.as_deref(), &secret, src.addr);
        if self.dot1x.len() >= 64 {
            self.dot1x.clear();
        }
        self.dot1x.insert(port, Dot1xWait { wlc: me.id, lap: peer.id, client, id: r.id, authenticator, secret });
        self.udp_send(t, me, server, port, RADIUS_PORT, req.encode())
    }

    fn radius_answer_in(&mut self, t: &Topology, d: &Device, u: &Udp) -> Result<(), String> {
        let w = self.dot1x.remove(&u.dst_port).ok_or("no login waiting")?;
        if w.wlc != d.id {
            return Err("RADIUS answer for another device".into());
        }
        let r = Radius::decode(&u.payload).map_err(|e| e.to_string())?;
        if !r.verifies(&w.authenticator, &w.secret) {
            return Err("RADIUS answer signed with another secret".into());
        }
        if !r.checks_message(&w.authenticator, &w.secret) {
            return Err("RADIUS answer without a valid Message-Authenticator: discarded".into());
        }
        let eap = r.eap_message().and_then(|m| Eap::decode(&m).ok());
        match r.code {
            // Another round: the server's EAP request goes on to the host.
            ACCESS_CHALLENGE => {
                let eap = eap.ok_or("an Access-Challenge without EAP")?;
                if let Some(st) = self.assoc.get_mut(&w.client) {
                    st.radius_state = r.state().map(<[u8]>::to_vec);
                }
                self.eap_to_host(t, w.wlc, w.lap, w.client, eap)
            }
            // Logged in: the session key keys the access point's side of the 4-way handshake.
            ACCESS_ACCEPT => {
                let key = r.mppe_recv_key(&w.authenticator, &w.secret).and_then(|k| <[u8; 32]>::try_from(k.get(..32)?).ok());
                if let Some(st) = self.assoc.get_mut(&w.client) {
                    st.pmk_ap = key;
                    st.radius_state = None;
                }
                self.dot1x_result(t, w.wlc, w.lap, w.client, eap.map_or(w.id, |e| e.id), true)
            }
            _ => self.dot1x_result(t, w.wlc, w.lap, w.client, eap.map_or(w.id, |e| e.id), false),
        }
    }

    /// An EAP packet from the controller to a host, through its AP (CAPWAP data, then the air).
    fn eap_to_host(&mut self, t: &Topology, wlc: DeviceId, lap: DeviceId, client: End, eap: Eap) -> Result<(), String> {
        let (Some(wd), Some(ld)) = (t.device(wlc), t.device(lap)) else { return Err("controller or AP removed".into()) };
        let radio = ld.ports.iter().position(|p| ifname::is_wireless(&p.name)).ok_or("no radio")?;
        let bssid = port_mac(t, End { device: lap, port: radio });
        let seq = self.air_seq();
        let f = Dot11::from_ap(port_mac(t, client), bssid, bssid, seq, ETHERTYPE_EAPOL, Eapol::Eap(eap).encode());
        let (wd, ld) = (wd.clone(), ld.clone());
        self.capwap_send(t, &wd, &ld, capwap::data_encode(&f.encode()), capwap::DATA_PORT)
    }

    /// The controller tells the host (through its AP) how its login went.
    fn dot1x_result(&mut self, t: &Topology, wlc: DeviceId, lap: DeviceId, client: End, id: u8, ok: bool) -> Result<(), String> {
        let (Some(wd), Some(ld)) = (t.device(wlc), t.device(lap)) else { return Err("controller or AP removed".into()) };
        let radio = ld.ports.iter().position(|p| ifname::is_wireless(&p.name)).ok_or("no radio")?;
        let bssid = port_mac(t, End { device: lap, port: radio });
        let seq = self.air_seq();
        let eap = Eap { code: if ok { EAP_SUCCESS } else { EAP_FAILURE }, id, kind: None, data: Vec::new() };
        let f = Dot11::from_ap(port_mac(t, client), bssid, bssid, seq, ETHERTYPE_EAPOL, Eapol::Eap(eap).encode());
        let (wd, ld) = (wd.clone(), ld.clone());
        self.capwap_send(t, &wd, &ld, capwap::data_encode(&f.encode()), capwap::DATA_PORT)?;
        if !ok {
            let seq = self.air_seq();
            let deauth = Dot11::mgmt(port_mac(t, client), bssid, bssid, seq, Body::Deauth { reason: wlan::REASON_8021X_FAILED });
            self.capwap_send(t, &wd, &ld, capwap::data_encode(&deauth.encode()), capwap::DATA_PORT)?;
        }
        Ok(())
    }

    /// A capture of the air hops only, as IEEE 802.11 frames (link type 105).
    pub fn pcap_wlan(&self) -> Vec<u8> {
        netlabcraft_wire::pcap::write_link(
            netlabcraft_wire::pcap::LINKTYPE_IEEE802_11,
            self.trace.iter().filter_map(|e| Some((e.at_us, e.air.as_deref()?))),
        )
    }
}

/// The Wi-Fi host port with this MAC address.
pub(crate) fn host_with_mac(t: &Topology, mac: MacAddr) -> Option<End> {
    t.devices
        .iter()
        .filter(|d| d.kind.is_host())
        .find_map(|d| d.ports.iter().position(|p| p.mac == mac && ifname::is_wireless(&p.name)).map(|i| End { device: d.id, port: i }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PingOptions, Probe};
    use netlabcraft_model::{Wifi, models};
    use netlabcraft_wire::Protocol;

    /// A PC and an access point on one cable, a Wi-Fi laptop near the AP.
    fn lab(key: Option<&str>, laptop_key: Option<&str>) -> (Topology, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let pc = t.add_model(models::find("PC").unwrap(), Some("PC1"), 0.0, 0.0).unwrap();
        let ap = t.add_model(models::find("AccessPoint").unwrap(), Some("AP1"), 100.0, 0.0).unwrap();
        let lt = t.add_model(models::find("Laptop-WiFi").unwrap(), Some("L1"), 200.0, 100.0).unwrap();
        t.connect(End { device: pc, port: 0 }, End { device: ap, port: 0 }).unwrap();
        t.device_mut(pc).unwrap().ports[0].ip = Some("10.0.0.1/24".parse().unwrap());
        t.device_mut(lt).unwrap().ports[0].ip = Some("10.0.0.2/24".parse().unwrap());
        t.device_mut(ap).unwrap().ports[1].wifi = Some(Wifi { ssid: "Lab".into(), key: key.map(str::to_string), channel: 6, login: None });
        t.device_mut(lt).unwrap().ports[0].wifi = Some(Wifi { ssid: "Lab".into(), key: laptop_key.map(str::to_string), channel: 6, login: None });
        (t, pc, lt)
    }

    #[test]
    fn a_host_associates_before_its_frames_cross_the_air_as_802_11() {
        let (t, _, lt) = lab(Some("secreto123"), Some("secreto123"));
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        let r = sim.ping(&t, lt, "10.0.0.1".parse().unwrap(), PingOptions::default()).unwrap();
        assert!(r.iter().all(|p| matches!(p, Probe::Reply { .. })), "{r:?}");
        let client = End { device: lt, port: 0 };
        assert!(sim.association(client).is_some_and(|a| a.done));
        let air: Vec<&crate::TraceEvent> = sim.trace().iter().filter(|e| e.air.is_some()).collect();
        let first_data = air.iter().position(|e| !e.frame.is_empty() && e.protocol != Protocol::Eapol).unwrap();
        let handshake_done = air.iter().position(|e| e.summary.contains("message 4 of 4")).unwrap();
        // Probe, authentication and association first; no data before the handshake ends.
        assert_eq!(air[0].protocol, Protocol::Wlan);
        assert!(handshake_done < first_data, "{:?}", air.iter().map(|e| &e.summary).collect::<Vec<_>>());
        for e in &air {
            let f = sim.air_plain(&t, e.from, e.to, e.air.as_deref().unwrap()).unwrap();
            if f.is_data() && e.from == client {
                assert!(f.to_ds() && !f.from_ds());
                assert_eq!(f.ethernet().unwrap().encode(), e.frame);
            }
        }
        assert!(sim.trace().iter().all(|e| e.dropped.as_deref().is_none_or(|d| !d.contains("MIC"))));
    }

    #[test]
    fn a_wrong_passphrase_fails_the_handshake_and_nothing_flows() {
        let (t, pc, _) = lab(Some("secreto123"), Some("otraclave9"));
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.run_for(&t, 100_000);
        assert!(sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.contains("bad MIC"))));
        assert!(sim.trace().iter().any(|e| e.summary.contains("deauthentication")));
        let r = sim.ping(&t, pc, "10.0.0.2".parse().unwrap(), PingOptions { count: 1, ..PingOptions::default() }).unwrap();
        assert!(!matches!(r.first(), Some(Probe::Reply { .. })));
    }

    fn air_data(sim: &Sim, from: End) -> Vec<(Vec<u8>, Vec<u8>)> {
        sim.trace()
            .iter()
            .filter(|e| e.from == from && e.air.is_some() && e.protocol != Protocol::Eapol && !e.frame.is_empty())
            .map(|e| (e.air.clone().unwrap(), e.frame.clone()))
            .collect()
    }

    #[test]
    fn data_after_the_handshake_is_ccmp_protected_and_replays_are_refused() {
        let (t, _, lt) = lab(Some("secreto123"), Some("secreto123"));
        let client = End { device: lt, port: 0 };
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        let r = sim.ping(&t, lt, "10.0.0.1".parse().unwrap(), PingOptions::default()).unwrap();
        assert!(r.iter().all(|p| matches!(p, Probe::Reply { .. })), "{r:?}");
        assert!(sim.association(client).is_some_and(|a| a.protected()));
        let up = air_data(&sim, client);
        assert!(!up.is_empty());
        let tk = sim.association(client).unwrap().tk.unwrap();
        let mut last = 0;
        for (air, eth) in &up {
            // On the air: protected, its plaintext nowhere in it; listed as the plain Ethernet frame.
            let f = Dot11::decode(air).unwrap();
            assert!(f.is_protected(), "{}", f.describe());
            assert!(!air.windows(eth.len().min(20)).any(|w| w == &eth[eth.len() - 20..]));
            let (plain, pn) = wlan::ccmp_decrypt(&tk, air).unwrap();
            assert!(pn > last);
            last = pn;
            assert_eq!(Dot11::decode(&plain).unwrap().ethernet().unwrap().encode(), *eth);
        }
        assert!(sim.trace().iter().any(|e| e.summary.contains("[802.11, CCMP]")));
        // The Ethernet capture has the plaintext; the 802.11 capture the protected frames.
        assert!(sim.pcap().windows(up[0].1.len()).any(|w| w == up[0].1.as_slice()));
        assert!(sim.pcap_wlan().windows(up[0].0.len()).any(|w| w == up[0].0.as_slice()));
        // A frame played again is refused; so is one with a changed byte or sent in the clear.
        let ap = End { device: t.resolve("AP1").unwrap(), port: 1 };
        let (old, _) = up[0].clone();
        let mut bad = up.last().unwrap().0.clone();
        let n = bad.len() - 3;
        bad[n] ^= 1;
        let clear = Dot11::to_ap(port_mac(&t, ap), port_mac(&t, client), MacAddr::BROADCAST, 9, 0x0800, vec![0x45; 20]).encode();
        let before = sim.trace().len();
        for f in [old, bad, clear] {
            sim.enqueue(sim.now + 1, client, ap, f, 0);
        }
        sim.run_for(&t, 10_000);
        let drops: Vec<String> = sim.trace().iter().skip(before).filter_map(|e| e.dropped.clone()).collect();
        assert!(drops.iter().any(|d| d.contains("replay")), "{drops:?}");
        assert!(drops.iter().any(|d| d.contains("MIC check failed")), "{drops:?}");
        assert!(drops.iter().any(|d| d.contains("CCMP expected")), "{drops:?}");
    }

    #[test]
    fn an_open_network_stays_in_the_clear() {
        let (t, _, lt) = lab(None, None);
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.ping(&t, lt, "10.0.0.1".parse().unwrap(), PingOptions { count: 1, ..PingOptions::default() }).unwrap();
        let up = air_data(&sim, End { device: lt, port: 0 });
        assert!(!up.is_empty() && up.iter().all(|(a, _)| !Dot11::decode(a).unwrap().is_protected()));
    }

    #[test]
    fn message_3_carries_the_group_key_and_broadcasts_use_it() {
        let (t, pc, lt) = lab(Some("secreto123"), Some("secreto123"));
        let client = End { device: lt, port: 0 };
        let ap = End { device: t.resolve("AP1").unwrap(), port: 1 };
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        // The PC's ARP request is a broadcast: the access point sends it over the air under
        // the group key.
        let r = sim.ping(&t, pc, "10.0.0.2".parse().unwrap(), PingOptions::default()).unwrap();
        assert!(r.iter().all(|p| matches!(p, Probe::Reply { .. })), "{r:?}");
        let a = sim.association(client).unwrap().clone();
        let gtk = a.gtk.expect("message 3 delivered a group key");
        assert_eq!(gtk, group_key(&t, ap));
        assert_ne!(Some(gtk), a.tk);
        // Message 3 on the air: encrypted key data, and the group key nowhere in the clear.
        let m3 = sim.trace().iter().find(|e| e.summary.contains("message 3 of 4")).unwrap();
        assert!(m3.summary.contains("group key wrapped"), "{}", m3.summary);
        let raw = m3.air.clone().unwrap();
        assert!(!raw.windows(16).any(|w| w == gtk));
        let Body::Data { payload, .. } = Dot11::decode(&raw).unwrap().body else { panic!() };
        let Eapol::Key(k) = Eapol::decode(&payload).unwrap() else { panic!() };
        assert!(k.info & wlan::KEY_ENCRYPTED_DATA != 0 && k.data.len().is_multiple_of(8));
        // Unwrapped with the KEK (from the same key hierarchy) it holds the GTK KDE.
        let (anonce, snonce) = (a.anonce, a.snonce);
        let pmk = sim.pmk("secreto123", "Lab");
        let ptk = sim.ptk(&t, &pmk, ap, client, &anonce, &snonce);
        let kek: [u8; 16] = ptk[16..32].try_into().unwrap();
        let plain = wlan::unwrap_key_data(&kek, &k.data).unwrap();
        assert_eq!(wlan::find_gtk(&plain), Some((1, gtk)));
        assert!(wlan::unwrap_key_data(&[0; 16], &k.data).is_none());
        // Group frames down: protected under the GTK with key id 1; unicast under the pairwise key.
        let down: Vec<Vec<u8>> =
            sim.trace().iter().filter(|e| e.from == ap && e.to == client && e.protocol != Protocol::Eapol).filter_map(|e| e.air.clone()).collect();
        let group: Vec<&Vec<u8>> = down.iter().filter(|b| Dot11::decode(b).is_ok_and(|f| f.is_protected() && f.addr1.0[0] & 1 == 1)).collect();
        assert!(!group.is_empty(), "no group-addressed frame went down");
        for g in &group {
            let (plain, _) = wlan::ccmp_decrypt(&gtk, g).unwrap();
            assert!(Dot11::decode(&plain).unwrap().ethernet().unwrap().dst.is_group());
            // The CCMP header's key id byte: bits 6-7 (24-byte header, then PN0 PN1 rsvd keyid).
            assert_eq!(g[24 + 3] >> 6, 1);
            assert!(wlan::ccmp_decrypt(&a.tk.unwrap(), g).is_err());
        }
        assert!(sim.trace().iter().all(|e| e.dropped.as_deref().is_none_or(|d| !d.contains("group"))));
        // A group frame played again is a replay.
        let before = sim.trace().len();
        sim.enqueue(sim.now + 1, ap, client, group[0].clone(), 0);
        sim.run_for(&t, 10_000);
        let drops: Vec<String> = sim.trace().iter().skip(before).filter_map(|e| e.dropped.clone()).collect();
        assert!(drops.iter().any(|d| d.contains("group packet number")), "{drops:?}");
    }

    #[test]
    fn walls_decide_wifi_range_once_the_lab_has_a_physical_layout() {
        let (mut t, pc, lt) = lab(Some("secreto123"), Some("secreto123"));
        let client = End { device: lt, port: 0 };
        // No layout yet: the logical distance decides (the laptop is near).
        assert!(t.wifi_ap_of(client).is_some());
        // A second building: the laptop moves there, out of reach of the AP in the first.
        let annex = t.add_place(netlabcraft_model::physical::PlaceKind::Building, Some("Annex"), Some(1), 600.0, 220.0).unwrap();
        t.set_home(lt, annex, 10.0, 10.0).unwrap();
        assert!(t.wifi_ap_of(client).is_none());
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.run_for(&t, 3 * BEACON_EVERY_US);
        assert!(!sim.trace().iter().any(|e| e.summary.starts_with("802.11 beacon")));
        let r = sim.ping(&t, pc, "10.0.0.2".parse().unwrap(), PingOptions { count: 1, ..PingOptions::default() }).unwrap();
        assert!(!matches!(r.first(), Some(Probe::Reply { .. })));
        // An access point in the annex's wiring closet reaches it, however far on the map.
        let closet = t.add_place(netlabcraft_model::physical::PlaceKind::Closet, Some("Annex closet"), Some(annex), 0.0, 0.0).unwrap();
        let ap2 = t.add_model(models::find("AccessPoint").unwrap(), Some("AP2"), 9000.0, 9000.0).unwrap();
        t.device_mut(ap2).unwrap().ports[1].wifi = Some(Wifi { ssid: "Lab".into(), key: Some("secreto123".into()), channel: 11, login: None });
        t.set_home(ap2, closet, 5.0, 5.0).unwrap();
        assert_eq!(t.wifi_ap_of(client), Some(End { device: ap2, port: 1 }));
        // Back in the main building (whatever the logical distance), AP1 again.
        t.device_mut(lt).unwrap().x = 5000.0;
        t.set_home(lt, 2, 0.0, 0.0).unwrap();
        assert_eq!(t.wifi_ap_of(client).map(|e| e.device), t.resolve("AP1").ok());
    }

    #[test]
    fn access_points_beacon_to_hosts_in_range() {
        let (mut t, _, lt) = lab(Some("secreto123"), Some("secreto123"));
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.run_for(&t, 10 * BEACON_EVERY_US);
        let beacons: Vec<&crate::TraceEvent> = sim.trace().iter().filter(|e| e.summary.starts_with("802.11 beacon")).collect();
        assert!((9..=11).contains(&beacons.len()), "{}", beacons.len());
        assert!(beacons.iter().all(|e| e.dropped.is_none() && e.to == End { device: lt, port: 0 }));
        assert!(beacons.windows(2).all(|w| w[1].at_us - w[0].at_us == BEACON_EVERY_US));
        let Body::Beacon(b) = Dot11::decode(beacons[0].air.as_deref().unwrap()).unwrap().body else { panic!() };
        assert_eq!((b.ssid.as_str(), b.interval, b.rsn), ("Lab", BEACON_TU, Some(Akm::Psk)));
        // Beacons don't disturb the association.
        assert!(sim.association(End { device: lt, port: 0 }).is_some_and(|a| a.done));
        // A long run only replays the last few; a host out of range hears none.
        sim.run_for(&t, 3_600_000_000);
        assert!(sim.trace().iter().filter(|e| e.summary.starts_with("802.11 beacon")).count() < 30);
        t.device_mut(lt).unwrap().x = 5000.0;
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.run_for(&t, 10 * BEACON_EVERY_US);
        assert!(!sim.trace().iter().any(|e| e.summary.starts_with("802.11 beacon")));
    }

    /// Two access points of one WPA2 network on a switch, a PC, and a laptop near AP1.
    fn roam_lab() -> (Topology, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let sw = t.add_model(models::find("Switch-24").unwrap(), Some("S1"), 400.0, 0.0).unwrap();
        let pc = t.add_model(models::find("PC").unwrap(), Some("PC1"), 400.0, -200.0).unwrap();
        let ap1 = t.add_model(models::find("AccessPoint").unwrap(), Some("AP1"), 0.0, 0.0).unwrap();
        let ap2 = t.add_model(models::find("AccessPoint").unwrap(), Some("AP2"), 800.0, 0.0).unwrap();
        let lt = t.add_model(models::find("Laptop-WiFi").unwrap(), Some("L1"), 50.0, 100.0).unwrap();
        for (i, d) in [pc, ap1, ap2].into_iter().enumerate() {
            t.connect(End { device: d, port: 0 }, End { device: sw, port: i }).unwrap();
        }
        t.device_mut(pc).unwrap().ports[0].ip = Some("10.0.0.1/24".parse().unwrap());
        t.device_mut(lt).unwrap().ports[0].ip = Some("10.0.0.2/24".parse().unwrap());
        for ap in [ap1, ap2] {
            t.device_mut(ap).unwrap().ports[1].wifi = Some(Wifi { ssid: "Lab".into(), key: Some("secreto123".into()), channel: 6, login: None });
        }
        t.device_mut(lt).unwrap().ports[0].wifi = Some(Wifi { ssid: "Lab".into(), key: Some("secreto123".into()), channel: 6, login: None });
        (t, pc, lt)
    }

    #[test]
    fn a_host_moving_to_another_access_point_reassociates_and_keeps_its_address() {
        let (mut t, pc, lt) = roam_lab();
        let client = End { device: lt, port: 0 };
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        let r = sim.ping(&t, lt, "10.0.0.1".parse().unwrap(), PingOptions { count: 1, ..PingOptions::default() }).unwrap();
        assert!(matches!(r.first(), Some(Probe::Reply { .. })), "{r:?}");
        let ap1 = sim.association(client).unwrap().ap.unwrap();
        // Over to AP2.
        t.device_mut(lt).unwrap().x = 760.0;
        sim.topology_changed(&t);
        let before = sim.trace().len();
        let r = sim.ping(&t, lt, "10.0.0.1".parse().unwrap(), PingOptions { count: 2, ..PingOptions::default() }).unwrap();
        let shown: Vec<String> = sim.trace().iter().skip(before).map(|e| format!("{:?}>{:?} {} {:?}", e.from, e.to, e.summary, e.dropped)).collect();
        assert!(r.iter().all(|p| matches!(p, Probe::Reply { .. })), "{r:?} {shown:#?}");
        let a = sim.association(client).unwrap();
        assert!(a.ap != Some(ap1) && a.done && a.protected());
        let tr: Vec<String> = sim.trace().iter().skip(before).map(|e| e.summary.clone()).collect();
        let req = tr.iter().position(|s| s.starts_with("802.11 reassociation request: \"Lab\", moving from")).expect("reassociation request");
        let resp = tr.iter().position(|s| s.starts_with("802.11 reassociation response: success")).expect("reassociation response");
        assert!(req < resp && !tr.iter().any(|s| s.contains("probe request") || s.contains("deauthentication")), "{tr:?}");
        let Body::ReassocRequest { current_ap, .. } =
            Dot11::decode(sim.trace().iter().skip(before).nth(req).unwrap().air.as_deref().unwrap()).unwrap().body
        else {
            panic!()
        };
        assert_eq!(current_ap, port_mac(&t, ap1));
        // A fresh handshake keys the new access point; the address stays.
        assert!(tr.iter().any(|s| s.contains("message 4 of 4")));
        assert_eq!(t.device(lt).unwrap().ports[0].ip, Some("10.0.0.2/24".parse().unwrap()));
        let r = sim.ping(&t, pc, "10.0.0.2".parse().unwrap(), PingOptions { count: 1, ..PingOptions::default() }).unwrap();
        assert!(matches!(r.first(), Some(Probe::Reply { .. })), "{r:?}");
    }

    #[test]
    fn hostile_air_frames_are_dropped_not_fatal() {
        let (t, _, lt) = lab(None, None);
        let ap = End { device: t.resolve("AP1").unwrap(), port: 1 };
        let client = End { device: lt, port: 0 };
        let mut sim = Sim::new();
        let good = Dot11::mgmt(MacAddr::BROADCAST, port_mac(&t, client), MacAddr::BROADCAST, 1, Body::ProbeRequest { ssid: "Lab".into() }).encode();
        for n in 0..good.len() {
            sim.enqueue(sim.now + 1, client, ap, good[..n].to_vec(), 0);
            sim.enqueue(sim.now + 1, ap, client, good[..n].to_vec(), 0);
        }
        for seed in 0u32..200 {
            let junk: Vec<u8> = (0..(seed % 90)).map(|i| (seed.wrapping_mul(2_654_435_761).wrapping_add(i * 31) >> 9) as u8).collect();
            sim.enqueue(sim.now + 1, client, ap, junk, 0);
        }
        sim.run_for(&t, 1_000_000);
        assert!(sim.trace().iter().any(|e| e.dropped.is_some()));
        assert!(sim.pcap_wlan().len() > 24);
    }
}
