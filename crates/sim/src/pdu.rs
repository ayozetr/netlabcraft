//! User PDUs (Simulation mode's simple and complex PDUs): one packet of a chosen application,
//! built field by field (ports, TTL, type of service, sequence number, size), sent from a
//! device and followed until its answer comes back or the wait runs out.

use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr};
use netlabcraft_model::Topology;
use netlabcraft_model::scenario::PduApp;
use netlabcraft_wire::dhcp::{self, Dhcp, DhcpKind};
use netlabcraft_wire::tcp::{self, Tcp};
use netlabcraft_wire::{Arp, ArpOp, ETHERTYPE_ARP, ETHERTYPE_IPV4, Frame, Icmp, Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP, Udp, dns};

use crate::{L3If, Sim, SimError, TraceEvent, default_ttl, l3_ifaces};

/// How long a PDU waits for its answer (simulated µs).
pub const PDU_WAIT_US: u64 = 2_000_000;

/// A PDU to send.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PduSpec {
    pub app: PduApp,
    /// The source port (index); none: the routed one (hosts: their first port).
    pub port: Option<usize>,
    /// The destination address (ignored by DHCP, which broadcasts).
    pub dst: Option<Ipv4Addr>,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
    pub ttl: Option<u8>,
    /// The IPv4 type-of-service byte.
    pub tos: u8,
    pub seq: Option<u32>,
    pub size: u16,
    /// DNS: the name to look up; TFTP: the file; HTTP: the page; SNMP: the community.
    pub text: String,
}

/// What became of a PDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PduOutcome {
    pub ok: bool,
    /// One line for people ("reply from 10.0.2.10").
    pub note: String,
    /// Events from this id on belong to it.
    pub first_event: u64,
}

/// What a frame delivered back to the source means for the PDU.
enum Answer {
    Good(String),
    Bad(String),
}

impl Sim {
    /// Sends one user PDU from `dev` and waits for what answers it.
    pub fn send_pdu(&mut self, t: &Topology, dev: DeviceId, spec: &PduSpec) -> Result<PduOutcome, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        if d.powered_off {
            return Err(SimError::NoSource(format!("{} is switched off", d.name)));
        }
        let first_event = self.next_event.saturating_add(1);
        let ifs = l3_ifaces(d);
        let from_port = spec.port.and_then(|p| ifs.iter().find(|i| i.port == p).cloned());
        if spec.port.is_some() && from_port.is_none() && spec.app != PduApp::Dhcp {
            return Err(SimError::NoSource("that port has no IP address".into()));
        }
        let held = std::mem::replace(&mut self.hold_for_arp, true);
        // Its answers are watched for whatever the capture filter keeps in the event list.
        self.watch(dev);
        let r = match spec.app {
            PduApp::Dhcp => self.pdu_dhcp(t, dev, spec),
            PduApp::Arp => self.pdu_arp(t, dev, spec, from_port.as_ref()),
            _ => self.pdu_ip(t, dev, spec, from_port.as_ref()),
        };
        self.unwatch();
        self.hold_for_arp = held;
        let (ok, note) = r?;
        Ok(PduOutcome { ok, note, first_event })
    }

    /// Runs the network until a frame delivered to `dev` since `since` answers the PDU (or the
    /// wait runs out).
    fn pdu_wait(&mut self, t: &Topology, dev: DeviceId, since: u64, judge: impl Fn(&TraceEvent) -> Option<Answer>) -> Option<Answer> {
        let found = |s: &Sim| s.watched(since).filter(|e| e.to.device == dev).last().and_then(&judge);
        if let Some(a) = found(self) {
            return Some(a);
        }
        let deadline = self.now.saturating_add(PDU_WAIT_US);
        self.advance(t, deadline, |s| {
            s.mode.watch.as_ref().and_then(|(_, v)| v.back()).is_some_and(|e| e.id >= since && e.to.device == dev && judge(e).is_some())
        });
        self.watched(since).filter(|e| e.to.device == dev).find_map(&judge)
    }

    fn pdu_arp(&mut self, t: &Topology, dev: DeviceId, spec: &PduSpec, from: Option<&L3If>) -> Result<(bool, String), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let dst = spec.dst.ok_or_else(|| SimError::NoSource("an ARP request needs the address to ask for".into()))?;
        let out = match from {
            Some(i) => i.clone(),
            None => {
                let ifs = l3_ifaces(d);
                match ifs.iter().find(|i| i.net.contains(dst)).cloned() {
                    Some(i) => i,
                    None => self.route(t, d, dst).map(|(i, _)| i).ok_or_else(|| SimError::NoSource(format!("{} has no route to {dst}", d.name)))?,
                }
            }
        };
        let since = self.next_event.saturating_add(1);
        let req = Arp { op: ArpOp::Request, sender_mac: out.mac, sender_ip: out.net.addr, target_mac: MacAddr::ZERO, target_ip: dst };
        let frame = Frame { dst: MacAddr::BROADCAST, src: out.mac, vlan: out.vlan, ethertype: ETHERTYPE_ARP, payload: req.encode() };
        let origin = self.begin_origin();
        self.l3_emit(t, d, &out, frame);
        self.end_origin(origin);
        let answer = self.pdu_wait(t, dev, since, |e| {
            let f = Frame::decode(&e.frame).ok().filter(|f| f.ethertype == ETHERTYPE_ARP)?;
            let a = Arp::decode(&f.payload).ok().filter(|a| a.op == ArpOp::Reply && a.sender_ip == dst)?;
            Some(Answer::Good(format!("the ARP reply says {dst} is at {}", a.sender_mac)))
        });
        Ok(verdict(answer, format!("nobody answered for {dst}")))
    }

    fn pdu_dhcp(&mut self, t: &Topology, dev: DeviceId, spec: &PduSpec) -> Result<(bool, String), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let at = spec.port.unwrap_or(0);
        let port = d.ports.get(at).ok_or_else(|| SimError::NoSource("no such port".into()))?;
        self.seq += 1;
        let xid = 0x4e4c_0000 ^ (self.seq as u32);
        let since = self.next_event.saturating_add(1);
        let msg = Dhcp::new(DhcpKind::Discover, xid, port.mac);
        let src = Ipv4Addr::UNSPECIFIED;
        let udp =
            Udp { src_port: self.pdu_sport(spec, dhcp::CLIENT_PORT), dst_port: spec.dst_port.unwrap_or(dhcp::SERVER_PORT), payload: msg.encode() };
        let ip = Ipv4 {
            id: self.ip_id(),
            ttl: spec.ttl.unwrap_or(128).max(1),
            protocol: PROTO_UDP,
            src,
            dst: Ipv4Addr::BROADCAST,
            payload: udp.encode(src, Ipv4Addr::BROADCAST),
        };
        let frame = Frame { dst: MacAddr::BROADCAST, src: port.mac, vlan: None, ethertype: ETHERTYPE_IPV4, payload: with_tos(ip.encode(), spec.tos) };
        if !self.emit(t, netlabcraft_model::End { device: dev, port: at }, frame.encode()) {
            return Ok((false, "the port is down".into()));
        }
        let answer = self.pdu_wait(t, dev, since, |e| {
            let (ip, _) = ipv4_of(e)?;
            let u = Udp::decode(&ip.payload, ip.src, ip.dst).ok().filter(|u| u.dst_port == dhcp::CLIENT_PORT)?;
            let m = Dhcp::decode(&u.payload).ok().filter(|m| m.xid == xid)?;
            Some(match m.kind {
                DhcpKind::Offer => Answer::Good(format!("{} offered the address {}", ip.src, m.your_ip)),
                k => Answer::Bad(format!("{} answered with DHCP {}", ip.src, k.name())),
            })
        });
        Ok(verdict(answer, "no DHCP server answered".into()))
    }

    fn pdu_sport(&mut self, spec: &PduSpec, default: u16) -> u16 {
        match spec.src_port {
            Some(p) if p != 0 => p,
            _ if default != 0 => default,
            _ => {
                self.seq += 1;
                49_152 + (self.seq % 16_000) as u16
            }
        }
    }

    /// Routes and sends a datagram with the PDU's type of service.
    fn pdu_send(&mut self, t: &Topology, dev: DeviceId, ip: Ipv4, tos: u8) -> Result<(), String> {
        let d = t.device(dev).ok_or("device removed")?;
        self.bump(d.id, "sent");
        let (out, nh) = self.route_flow(t, d, ip.src, ip.dst).ok_or_else(|| format!("no route to {}", ip.dst))?;
        let origin = self.begin_origin();
        self.note_sends(ip.dst, &out, nh, ip.ttl);
        let r = self.transmit(t, d, &out, nh, with_tos(ip.encode(), tos));
        self.end_origin(origin);
        r
    }

    fn pdu_ip(&mut self, t: &Topology, dev: DeviceId, spec: &PduSpec, from: Option<&L3If>) -> Result<(bool, String), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let dst = spec.dst.ok_or_else(|| SimError::NoSource("the PDU needs a destination address".into()))?;
        if l3_ifaces(d).iter().any(|i| i.net.addr == dst) {
            return Ok((true, format!("{dst} is this device's own address")));
        }
        let src = match from {
            Some(i) => i.net.addr,
            None => self.source_for(t, d, dst)?,
        };
        let ttl = spec.ttl.unwrap_or(default_ttl(d.kind)).max(1);
        let size = usize::from(spec.size.min(netlabcraft_model::scenario::MAX_PDU_SIZE));
        let since = self.next_event.saturating_add(1);
        let ip = |s: &mut Sim, protocol: u8, payload: Vec<u8>| Ipv4 { id: s.ip_id(), ttl, protocol, src, dst, payload };
        // ICMP errors about this PDU coming back.
        let icmp_error = move |ip: &Ipv4| -> Option<Answer> {
            if ip.protocol != PROTO_ICMP {
                return None;
            }
            match Icmp::decode(&ip.payload).ok()? {
                Icmp::Unreachable { code, .. } => Some(Answer::Bad(format!("{} says destination unreachable (code {code})", ip.src))),
                Icmp::TimeExceeded { .. } => Some(Answer::Bad(format!("TTL ran out at {}", ip.src))),
                _ => None,
            }
        };
        match spec.app {
            PduApp::Ping => {
                self.next_echo_id = self.next_echo_id.wrapping_add(1);
                let id = self.next_echo_id;
                let seq = spec.seq.map_or(1, |s| s as u16);
                let pkt = ip(self, PROTO_ICMP, Icmp::EchoRequest { id, seq, data: vec![0xab; size] }.encode());
                if let Err(e) = self.pdu_send(t, dev, pkt, spec.tos) {
                    return Ok((false, e));
                }
                let answer = self.pdu_wait(t, dev, since, |e| {
                    let (ip, _) = ipv4_of(e)?;
                    match Icmp::decode(&ip.payload).ok() {
                        Some(Icmp::EchoReply { id: rid, seq: rseq, .. }) if ip.protocol == PROTO_ICMP && rid == id && rseq == seq => {
                            Some(Answer::Good(format!("reply from {}", ip.src)))
                        }
                        _ => icmp_error(&ip),
                    }
                });
                Ok(verdict(answer, format!("no answer from {dst}")))
            }
            PduApp::Dns | PduApp::Tftp | PduApp::Snmp | PduApp::Udp => {
                let sport = self.pdu_sport(spec, 0);
                let dport = spec.dst_port.or(spec.app.default_port()).unwrap_or(1_024);
                let payload = match spec.app {
                    PduApp::Dns => {
                        let name = if spec.text.trim().is_empty() { "www.netlabcraft.lab" } else { spec.text.trim() };
                        dns::Dns::query(spec.seq.map_or(1, |s| s as u16), name).encode()
                    }
                    PduApp::Tftp => {
                        let file = if spec.text.trim().is_empty() { "startup-config" } else { spec.text.trim() };
                        netlabcraft_wire::mgmt::Tftp::Read(file.to_string()).encode()
                    }
                    PduApp::Snmp => {
                        use netlabcraft_wire::snmp::{PduKind, Snmp, Value};
                        let community = if spec.text.trim().is_empty() { "public" } else { spec.text.trim() };
                        Snmp {
                            community: community.to_string(),
                            kind: PduKind::Get,
                            request_id: spec.seq.map_or(1, |s| (s & 0x7fff_ffff) as i32),
                            error_status: 0,
                            error_index: 0,
                            bindings: vec![(vec![1, 3, 6, 1, 2, 1, 1, 5, 0], Value::Null)],
                        }
                        .encode()
                    }
                    _ => vec![0u8; size],
                };
                let udp = Udp { src_port: sport, dst_port: dport, payload };
                let pkt = ip(self, PROTO_UDP, udp.encode(src, dst));
                if let Err(e) = self.pdu_send(t, dev, pkt, spec.tos) {
                    return Ok((false, e));
                }
                let answer = self.pdu_wait(t, dev, since, |e| {
                    let (ip, _) = ipv4_of(e)?;
                    if ip.protocol == PROTO_UDP && ip.src == dst {
                        let u = Udp::decode(&ip.payload, ip.src, ip.dst).ok()?;
                        return (u.dst_port == sport).then(|| Answer::Good(format!("{dst} answered from UDP port {}", u.src_port)));
                    }
                    icmp_error(&ip)
                });
                Ok(verdict(answer, format!("no answer from {dst}")))
            }
            _ => self.pdu_tcp(t, dev, spec, src, dst, ttl),
        }
    }

    /// A TCP PDU: the handshake with the chosen ports and sequence number, the application's
    /// first message (HTTP's GET), then a FIN.
    fn pdu_tcp(&mut self, t: &Topology, dev: DeviceId, spec: &PduSpec, src: Ipv4Addr, dst: Ipv4Addr, ttl: u8) -> Result<(bool, String), SimError> {
        let sport = self.pdu_sport(spec, 0);
        let dport = spec.dst_port.or(spec.app.default_port()).unwrap_or(1_024);
        let isn = spec.seq.unwrap_or(1_000);
        let seg =
            |flags: u8, seq: u32, ack: u32, payload: Vec<u8>| Tcp { src_port: sport, dst_port: dport, seq, ack, flags, window: 65_535, payload };
        let send = |s: &mut Sim, seg: Tcp| {
            let ip = Ipv4 { id: s.ip_id(), ttl, protocol: PROTO_TCP, src, dst, payload: seg.encode(src, dst) };
            s.pdu_send(t, dev, ip, spec.tos)
        };
        self.st(dev).tcp_wait.push((sport, dport));
        let since = self.next_event.saturating_add(1);
        let result = (|| {
            if let Err(e) = send(self, seg(tcp::SYN, isn, 0, Vec::new())) {
                return Ok((false, e));
            }
            let answer = self.pdu_wait(t, dev, since, |e| {
                let (ip, _) = ipv4_of(e)?;
                if ip.protocol == PROTO_TCP && ip.src == dst {
                    let s = Tcp::decode(&ip.payload, ip.src, ip.dst).ok().filter(|s| s.dst_port == sport)?;
                    if s.flags & tcp::RST != 0 {
                        return Some(Answer::Bad(format!("{dst} refused the connection to TCP port {dport}")));
                    }
                    return (s.flags & tcp::SYN != 0).then(|| Answer::Good(format!("{dst} accepted the connection to TCP port {dport}")));
                }
                None
            });
            let Some(Answer::Good(note)) = answer else { return Ok(verdict(answer, format!("{dst} did not answer on TCP port {dport}"))) };
            let synack = self.st(dev).tcp_inbox.iter().position(|s| s.dst_port == sport && s.flags & tcp::SYN != 0);
            let srv = synack.map(|i| self.st(dev).tcp_inbox.remove(i)).map_or(0, |s| s.seq.wrapping_add(1));
            let _ = send(self, seg(tcp::ACK, isn.wrapping_add(1), srv, Vec::new()));
            let mut sent = 0u32;
            if spec.app == PduApp::Http {
                let page = if spec.text.trim().is_empty() { "/" } else { spec.text.trim() };
                let req = format!("GET {page} HTTP/1.1\r\nHost: {dst}\r\nConnection: close\r\n\r\n").into_bytes();
                sent = req.len() as u32;
                let _ = send(self, seg(tcp::PSH | tcp::ACK, isn.wrapping_add(1), srv, req));
                let _ = self.tcp_wait(t, dev, |s| s.dst_port == sport && !s.payload.is_empty());
            }
            let _ = send(self, seg(tcp::FIN | tcp::ACK, isn.wrapping_add(1).wrapping_add(sent), srv, Vec::new()));
            self.run_for(t, 50_000);
            Ok((true, note))
        })();
        let st = self.st(dev);
        st.tcp_wait.retain(|w| *w != (sport, dport));
        st.tcp_inbox.retain(|s| s.dst_port != sport);
        result
    }
}

/// The datagram with its type-of-service byte set.
fn with_tos(mut datagram: Vec<u8>, tos: u8) -> Vec<u8> {
    if tos != 0 {
        netlabcraft_wire::qos::set_ds_byte(&mut datagram, tos);
    }
    datagram
}

/// The IPv4 datagram an event carries.
fn ipv4_of(e: &TraceEvent) -> Option<(Ipv4, Frame)> {
    let f = Frame::decode(&e.frame).ok().filter(|f| f.ethertype == ETHERTYPE_IPV4)?;
    Some((Ipv4::decode(&f.payload).ok()?, f))
}

fn verdict(a: Option<Answer>, silent: String) -> (bool, String) {
    match a {
        Some(Answer::Good(n)) => (true, n),
        Some(Answer::Bad(n)) => (false, n),
        None => (false, silent),
    }
}
