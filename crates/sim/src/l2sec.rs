//! Switch edge protection on the frames themselves: DHCP snooping (untrusted ports never pass
//! a server's answers, and the switch keeps a table of who got which address), dynamic ARP
//! inspection (ARP from untrusted ports must match that table), BPDU guard, rate limits that
//! err-disable a port, errdisable recovery on simulated time, port-security aging, and the
//! per-port counters `show interfaces counters` reads.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr, VlanId};
use netlabcraft_model::l2sec::{AgingType, ErrCause};
use netlabcraft_model::{Device, DeviceKind, End, Topology, ifname};
use netlabcraft_wire::dhcp::{self, Dhcp, DhcpKind};
use netlabcraft_wire::{Arp, ArpOp, ETHERTYPE_ARP, ETHERTYPE_IPV4, Frame, Ipv4, PROTO_UDP, Udp};

use crate::{MAC_AGING_US, PortEvent, Sim, port_up};

/// LACP state bits, for reports.
pub use netlabcraft_wire::lacp;

/// Most snooping bindings one switch keeps.
pub const MAX_BINDINGS: usize = 4_096;

/// One DHCP snooping binding: a client, the address it was given, and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub vlan: VlanId,
    pub port: usize,
    /// The lease length (seconds) and when it was acknowledged (simulated µs).
    pub lease_s: u32,
    pub at_us: u64,
}

/// What snooping and inspection did on one switch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub dhcp_forwarded: u64,
    pub dhcp_dropped: u64,
}

/// ARP inspection counts for one VLAN.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArpCounts {
    pub forwarded: u64,
    pub dropped: u64,
    pub dropped_validation: u64,
}

/// Traffic through one port.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PortCounters {
    pub in_octets: u64,
    pub in_ucast: u64,
    pub in_mcast: u64,
    pub in_bcast: u64,
    pub out_octets: u64,
    pub out_ucast: u64,
    pub out_mcast: u64,
    pub out_bcast: u64,
}

/// The simulator's state for all of the above.
#[derive(Debug, Clone, Default)]
pub struct L2State {
    bindings: BTreeMap<DeviceId, Vec<Binding>>,
    counts: BTreeMap<DeviceId, Counts>,
    arp_counts: BTreeMap<(DeviceId, u16), ArpCounts>,
    /// Rate limits: (switch, port, ARP?) → (the second being counted, messages in it).
    rate: BTreeMap<(DeviceId, usize, bool), (u64, u32)>,
    /// When each err-disabled port went down (simulated µs).
    errdis_at: BTreeMap<(DeviceId, usize), u64>,
    /// Port security: when each learned address was learned and last seen.
    psec_seen: BTreeMap<(DeviceId, usize, MacAddr), (u64, u64)>,
    counters: BTreeMap<(DeviceId, usize), PortCounters>,
}

impl L2State {
    pub(crate) fn forget(&mut self, d: DeviceId) {
        self.bindings.remove(&d);
        self.counts.remove(&d);
        self.arp_counts.retain(|k, _| k.0 != d);
        self.rate.retain(|k, _| k.0 != d);
        self.psec_seen.retain(|k, _| k.0 != d);
        self.counters.retain(|k, _| k.0 != d);
    }
}

/// Whether BPDU guard is on for a port: set on the port, or an edge port with the default on.
pub fn bpduguard_on(t: &Topology, d: &Device, port: usize) -> bool {
    let Some(p) = d.ports.get(port) else { return false };
    p.l2.bpduguard.unwrap_or_else(|| d.l2.bpduguard_default && edge_port(t, d, port))
}

/// Whether a port is an edge port: `spanning-tree portfast`, or an access port with
/// `spanning-tree portfast default`.
pub fn edge_port(t: &Topology, d: &Device, port: usize) -> bool {
    let Some(p) = d.ports.get(port) else { return false };
    p.portfast || (d.l2.portfast_default && !p.routed && !t.is_trunk(End { device: d.id, port }))
}

/// The DHCP message in a frame, either direction.
fn dhcp_of(f: &Frame) -> Option<Dhcp> {
    if f.ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    let ip = Ipv4::decode(&f.payload).ok()?;
    if ip.protocol != PROTO_UDP {
        return None;
    }
    let u = Udp::decode(&ip.payload, ip.src, ip.dst).ok()?;
    if u.dst_port != dhcp::CLIENT_PORT && u.dst_port != dhcp::SERVER_PORT {
        return None;
    }
    Dhcp::decode(&u.payload).ok()
}

fn kind_name(k: DhcpKind) -> &'static str {
    match k {
        DhcpKind::Discover => "DISCOVER",
        DhcpKind::Offer => "OFFER",
        DhcpKind::Request => "REQUEST",
        DhcpKind::Ack => "ACK",
        DhcpKind::Nak => "NAK",
        DhcpKind::Release => "RELEASE",
    }
}

fn host_address(a: Ipv4Addr) -> bool {
    !(a.is_unspecified() || a.is_broadcast() || a.is_multicast())
}

impl Sim {
    /// Shuts a port down for `cause` (once), logging why.
    pub(crate) fn err_disable(&mut self, d: &Device, port: usize, cause: ErrCause, why: &str) {
        if self.err_disabled_now(d.id, port) {
            return;
        }
        self.events.push(PortEvent::ErrDisabledBy { device: d.id, port, cause });
        self.l2.errdis_at.insert((d.id, port), self.now);
        let name = d.ports.get(port).map_or(String::new(), |p| p.name.clone());
        self.log(d.id, &format!("{name}: {why}; port shut down (err-disabled, cause {})", cause.name()));
    }

    /// Counts a message against a per-second limit; true when it goes over.
    fn over_rate(&mut self, d: DeviceId, port: usize, arp: bool, limit: u32) -> bool {
        let sec = self.now / 1_000_000;
        let e = self.l2.rate.entry((d, port, arp)).or_insert((sec, 0));
        if e.0 != sec {
            *e = (sec, 0);
        }
        e.1 = e.1.saturating_add(1);
        e.1 > limit
    }

    /// A frame for spanning tree (a BPDU) on a switch port: BPDU guard err-disables the port.
    pub(crate) fn l2_bpdu_in(&mut self, t: &Topology, d: &Device, port: usize, f: &Frame) -> Result<(), String> {
        if f.dst == netlabcraft_wire::control::STP_GROUP && bpduguard_on(t, d, port) {
            let short = d.ports.get(port).map_or(String::new(), |p| ifname::short(&p.name));
            self.err_disable(d, port, ErrCause::Bpduguard, "BPDU guard: a BPDU arrived on an edge port");
            return Err(format!("BPDU guard: a BPDU on {short}, which expects only hosts; the port is err-disabled"));
        }
        Ok(())
    }

    /// DHCP snooping and ARP inspection on a frame that entered switch port `port` in `vlan`.
    pub(crate) fn l2_switch_in(&mut self, d: &Device, port: usize, vlan: VlanId, f: &Frame) -> Result<(), String> {
        let Some(p) = d.ports.get(port) else { return Ok(()) };
        let short = ifname::short(&p.name);
        if d.l2.snoops(vlan)
            && let Some(msg) = dhcp_of(f)
        {
            if let Some(limit) = p.l2.snoop_rate
                && self.over_rate(d.id, port, false, limit)
            {
                self.err_disable(d, port, ErrCause::DhcpRateLimit, &format!("DHCP snooping: more than {limit} DHCP messages a second"));
                return Err(format!("DHCP snooping: {short} went over {limit} DHCP messages a second; the port is err-disabled"));
            }
            let server_msg = matches!(msg.kind, DhcpKind::Offer | DhcpKind::Ack | DhcpKind::Nak);
            if !p.l2.snoop_trust {
                if server_msg {
                    self.l2.counts.entry(d.id).or_default().dhcp_dropped += 1;
                    return Err(format!(
                        "DHCP snooping: a server's {} on untrusted port {short} (VLAN {vlan}); dropped, only trusted ports may answer",
                        kind_name(msg.kind)
                    ));
                }
                if msg.client_mac != f.src {
                    self.l2.counts.entry(d.id).or_default().dhcp_dropped += 1;
                    return Err(format!("DHCP snooping: client address {} in a frame from {}; dropped", msg.client_mac.dotted(), f.src.dotted()));
                }
            }
            match msg.kind {
                DhcpKind::Ack if p.l2.snoop_trust => self.bind(d, vlan, &msg),
                DhcpKind::Release => {
                    if let Some(b) = self.l2.bindings.get_mut(&d.id) {
                        b.retain(|x| x.mac != msg.client_mac);
                    }
                }
                _ => {}
            }
            self.l2.counts.entry(d.id).or_default().dhcp_forwarded += 1;
        }
        if f.ethertype == ETHERTYPE_ARP && d.l2.inspects(vlan) && !p.l2.dai_trust {
            if let Some(limit) = p.l2.dai_rate
                && self.over_rate(d.id, port, true, limit)
            {
                self.err_disable(d, port, ErrCause::ArpInspection, &format!("ARP inspection: more than {limit} ARP messages a second"));
                return Err(format!("ARP inspection: {short} went over {limit} ARP messages a second; the port is err-disabled"));
            }
            let a = Arp::decode(&f.payload).map_err(|e| e.to_string())?;
            let v = d.l2.dai_validate;
            let invalid = if v.src_mac && f.src != a.sender_mac {
                Some(format!("Ethernet source {} is not the ARP sender {}", f.src.dotted(), a.sender_mac.dotted()))
            } else if v.dst_mac && a.op == ArpOp::Reply && f.dst != a.target_mac {
                Some(format!("Ethernet destination {} is not the ARP target {}", f.dst.dotted(), a.target_mac.dotted()))
            } else if v.ip && (!host_address(a.sender_ip) || (a.op == ArpOp::Reply && !host_address(a.target_ip))) {
                Some(format!("{} → {} is not a pair of host addresses", a.sender_ip, a.target_ip))
            } else {
                None
            };
            let c = self.l2.arp_counts.entry((d.id, vlan.get())).or_default();
            if let Some(why) = invalid {
                c.dropped += 1;
                c.dropped_validation += 1;
                return Err(format!("ARP inspection: {why} (validate check, {short}, VLAN {vlan}); dropped"));
            }
            let known =
                self.l2.bindings.get(&d.id).is_some_and(|bs| bs.iter().any(|b| b.vlan == vlan && b.mac == a.sender_mac && b.ip == a.sender_ip));
            let c = self.l2.arp_counts.entry((d.id, vlan.get())).or_default();
            if !known {
                c.dropped += 1;
                let name = d.name.clone();
                self.log(
                    d.id,
                    &format!(
                        "{short}: ARP inspection dropped {} claiming {} (VLAN {vlan}): no DHCP snooping binding on {name}",
                        a.sender_mac.dotted(),
                        a.sender_ip
                    ),
                );
                return Err(format!(
                    "ARP inspection: {} says it is {}, which no DHCP snooping binding in VLAN {vlan} confirms ({short} untrusted); dropped",
                    a.sender_mac.dotted(),
                    a.sender_ip
                ));
            }
            c.forwarded += 1;
        }
        Ok(())
    }

    /// Records the binding an ACK makes: the client is where the switch learned its address.
    fn bind(&mut self, d: &Device, vlan: VlanId, msg: &Dhcp) {
        if !host_address(msg.your_ip) {
            return;
        }
        let now = self.now;
        let port = self
            .state
            .get(&d.id)
            .and_then(|st| st.macs.get(&(vlan.get(), msg.client_mac)))
            .filter(|e| now.saturating_sub(e.learned_us) < MAC_AGING_US)
            .map(|e| e.port);
        let Some(port) = port else { return };
        // Only an untrusted port is checked later; a client behind a trusted port needs none.
        if d.ports.get(port).is_none_or(|p| p.l2.snoop_trust) {
            return;
        }
        let list = self.l2.bindings.entry(d.id).or_default();
        list.retain(|b| !(b.mac == msg.client_mac && b.vlan == vlan) && b.ip != msg.your_ip);
        if list.len() < MAX_BINDINGS {
            list.push(Binding { mac: msg.client_mac, ip: msg.your_ip, vlan, port, lease_s: msg.lease_secs.unwrap_or(86_400), at_us: now });
        }
    }

    /// The DHCP snooping bindings of a switch (leases still running).
    pub fn snooping_bindings(&self, d: DeviceId) -> Vec<Binding> {
        let now = self.now;
        self.l2
            .bindings
            .get(&d)
            .map(|bs| bs.iter().filter(|b| now.saturating_sub(b.at_us) < u64::from(b.lease_s).saturating_mul(1_000_000)).cloned().collect())
            .unwrap_or_default()
    }

    pub fn snooping_counts(&self, d: DeviceId) -> Counts {
        self.l2.counts.get(&d).copied().unwrap_or_default()
    }

    pub fn arp_inspection_counts(&self, d: DeviceId, vlan: VlanId) -> ArpCounts {
        self.l2.arp_counts.get(&(d, vlan.get())).copied().unwrap_or_default()
    }

    /// Forgets a switch's snooping bindings (`clear ip dhcp snooping binding`).
    pub fn clear_snooping(&mut self, d: DeviceId) {
        self.l2.bindings.remove(&d);
    }

    /// Traffic counted through a port.
    pub fn port_counters(&self, d: DeviceId, port: usize) -> PortCounters {
        self.l2.counters.get(&(d, port)).copied().unwrap_or_default()
    }

    /// When an err-disabled port went down (simulated µs), if the simulator saw it.
    pub fn err_disabled_since(&self, d: DeviceId, port: usize) -> Option<u64> {
        self.l2.errdis_at.get(&(d, port)).copied()
    }

    /// Counts a frame crossing a cable: out of `from`, into `to`.
    pub(crate) fn l2_count(&mut self, from: End, to: End, frame: &[u8]) {
        let dst = frame.get(0..6).map(|b| b[0] & 1 == 1 && b.iter().all(|x| *x == 0xff));
        let group = frame.first().is_some_and(|b| b & 1 == 1);
        let len = u64::try_from(frame.len()).unwrap_or(0);
        for (end, out) in [(from, true), (to, false)] {
            if self.l2.counters.len() >= crate::MAX_MAC_ENTRIES && !self.l2.counters.contains_key(&(end.device, end.port)) {
                continue;
            }
            let c = self.l2.counters.entry((end.device, end.port)).or_default();
            let (octets, ucast, mcast, bcast) = if out {
                (&mut c.out_octets, &mut c.out_ucast, &mut c.out_mcast, &mut c.out_bcast)
            } else {
                (&mut c.in_octets, &mut c.in_ucast, &mut c.in_mcast, &mut c.in_bcast)
            };
            *octets = octets.saturating_add(len);
            let n = if dst == Some(true) {
                bcast
            } else if group {
                mcast
            } else {
                ucast
            };
            *n = n.saturating_add(1);
        }
    }

    /// Port security aging: forgets learned addresses whose time is up, and notes `src` as seen.
    pub(crate) fn psec_age(&mut self, d: &Device, port: usize, src: MacAddr) {
        let Some(ps) = d.ports.get(port).and_then(|p| p.port_security.as_ref()) else { return };
        let now = self.now;
        if ps.aging_time > 0 {
            let limit = u64::from(ps.aging_time).saturating_mul(60_000_000);
            let kind = ps.aging_type;
            let mut expired = Vec::new();
            for ((dev, p, mac), (learned, last)) in &self.l2.psec_seen {
                let since = if kind == AgingType::Absolute { *learned } else { *last };
                if *dev == d.id && *p == port && now.saturating_sub(since) >= limit {
                    expired.push(*mac);
                }
            }
            if !expired.is_empty() {
                if let Some(list) = self.state.get_mut(&d.id).and_then(|st| st.secure.get_mut(&port)) {
                    list.retain(|m| !expired.contains(m));
                }
                for m in expired {
                    self.l2.psec_seen.remove(&(d.id, port, m));
                }
            }
        }
        if self.l2.psec_seen.len() < crate::MAX_MAC_ENTRIES || self.l2.psec_seen.contains_key(&(d.id, port, src)) {
            let e = self.l2.psec_seen.entry((d.id, port, src)).or_insert((now, now));
            e.1 = now;
        }
    }

    /// Once per run (start and end): BPDU guard on edge ports cabled to a switch (a switch
    /// sends BPDUs as soon as its port comes up), and errdisable recovery.
    pub(crate) fn l2_tick(&mut self, t: &Topology) {
        let now = self.now;
        let mut guard: Vec<(DeviceId, usize)> = Vec::new();
        let mut down: Vec<(DeviceId, usize)> = Vec::new();
        for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Switch) {
            for (i, p) in d.ports.iter().enumerate() {
                let end = End { device: d.id, port: i };
                if p.err_disabled {
                    down.push((d.id, i));
                    continue;
                }
                if p.routed || !port_up(t, end) || !bpduguard_on(t, d, i) {
                    continue;
                }
                let switch_peer = t
                    .peer(end)
                    .and_then(|q| t.device(q.device).map(|pd| (q, pd)))
                    .is_some_and(|(q, pd)| pd.kind == DeviceKind::Switch && pd.ports.get(q.port).is_some_and(|x| !x.routed) && port_up(t, q));
                if switch_peer {
                    guard.push((d.id, i));
                }
            }
        }
        for (dev, i) in guard {
            if let Some(d) = t.device(dev) {
                self.err_disable(d, i, ErrCause::Bpduguard, "BPDU guard: BPDUs from another switch on an edge port");
            }
        }
        // Ports that came back (by hand or by recovery) are forgotten; new ones are timed.
        let pending: Vec<(DeviceId, usize)> =
            self.events.iter().filter_map(|e| if let PortEvent::Recovered { device, port } = e { Some((*device, *port)) } else { None }).collect();
        let events_down: Vec<(DeviceId, usize)> = self
            .events
            .iter()
            .filter_map(|e| match e {
                PortEvent::ErrDisabled { device, port } | PortEvent::ErrDisabledBy { device, port, .. } => Some((*device, *port)),
                _ => None,
            })
            .collect();
        self.l2.errdis_at.retain(|k, _| (down.contains(k) || events_down.contains(k)) && !pending.contains(k));
        for k in &down {
            if !pending.contains(k) {
                self.l2.errdis_at.entry(*k).or_insert(now);
            }
        }
        let mut back: Vec<(DeviceId, usize, ErrCause)> = Vec::new();
        for ((dev, i), at) in &self.l2.errdis_at {
            let Some(d) = t.device(*dev) else { continue };
            let Some(p) = d.ports.get(*i) else { continue };
            if !p.err_disabled {
                continue;
            }
            let cause = p.l2.err_cause.unwrap_or(ErrCause::PsecureViolation);
            let wait = u64::from(d.l2.recovery_interval).saturating_mul(1_000_000);
            if d.l2.recovery.contains(&cause) && now.saturating_sub(*at) >= wait {
                back.push((*dev, *i, cause));
            }
        }
        for (dev, i, cause) in back {
            self.l2.errdis_at.remove(&(dev, i));
            self.events.push(PortEvent::Recovered { device: dev, port: i });
            let name = t.device(dev).and_then(|d| d.ports.get(i)).map_or(String::new(), |p| p.name.clone());
            self.log(dev, &format!("{name}: errdisable recovery ({}): the port comes back up", cause.name()));
        }
    }
}
