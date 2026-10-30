//! IP telephony: phones register with a router's call control, call each other, and send
//! voice packets while a call is up. Also the phone's built-in switch and voice VLANs.
//!
//! **Signalling** is NetLabCraft's own small protocol, not any vendor's: one line of ASCII
//! text per UDP datagram, to and from port 2000, `PCV1 <VERB> <arguments>`.
//!
//! | From | Message | Meaning |
//! |---|---|---|
//! | phone | `REGISTER <mac>` | let me in (sent to the TFTP server DHCP option 150 named) |
//! | phone | `DIAL <number>` | call this directory number |
//! | phone | `ANSWER <call>` | pick up the ringing call |
//! | phone | `HANGUP <call>` | end (or refuse) the call |
//! | router | `REGISTERED <number>` | you are this number |
//! | router | `REJECTED <reason>` | registration refused |
//! | router | `RINGBACK <call> <number>` | the far phone is ringing |
//! | router | `RING <call> <number>` | someone (this number) is calling you |
//! | router | `CONNECT <call> <address>` | talk to the phone at this address |
//! | router | `RELEASE <call> <reason>` | the call is over |
//!
//! **Voice** travels phone to phone (not through the router's call control) as RTP
//! (RFC 3550) over UDP port 16384, payload type 0 (G.711 µ-law), each packet 20 ms of
//! silence. A real call sends 50 a second each way; here one packet per simulated second
//! stands for them, so the event list stays readable.
//!
//! **The built-in switch**: the phone's network port (`FastEthernet0`) faces the switch, its
//! PC port (`FastEthernet1`) a computer. The PC's frames pass through untouched (untagged:
//! the access VLAN); the phone's own frames go tagged in the voice VLAN when the switch port
//! has `switchport voice vlan V`. The phone learns V from the switch port it is cabled to,
//! standing for the discovery protocols (CDP, LLDP-MED) real phones learn it with, which are
//! computed here like the other control protocols.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr};
use netlabcraft_model::voice::SIGNAL_PORT;
use netlabcraft_model::{Device, DeviceKind, End, Topology};
use netlabcraft_wire::{Frame, Ipv4, PROTO_UDP, Udp};

use crate::{LINK_DELAY_US, MAX_SWITCH_HOPS, Sim, SimError, default_ttl, port_up};

/// The UDP port voice packets go to.
pub const RTP_PORT: u16 = 16_384;
/// How long a phone waits before trying to register again.
const REGISTER_RETRY_US: u64 = 5_000_000;
/// One voice packet per second of simulated time stands for the 50 a real call sends.
const RTP_EVERY_US: u64 = 1_000_000;
/// Most registrations and calls one router keeps.
const MAX_REGISTRATIONS: usize = 256;
const MAX_CALLS: usize = 128;
/// The network port and the PC port of a phone.
pub const NETWORK_PORT: usize = 0;
pub const PC_PORT: usize = 1;

/// Where a phone's call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallState {
    /// Dialled; waiting for the router.
    Dialing,
    /// The far phone is ringing.
    Ringback,
    /// This phone is ringing.
    Ringing,
    /// Talking.
    Connected,
}

impl CallState {
    pub fn name(self) -> &'static str {
        match self {
            CallState::Dialing => "dialing",
            CallState::Ringback => "ringback",
            CallState::Ringing => "ringing",
            CallState::Connected => "connected",
        }
    }
}

/// A phone's call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneCall {
    pub id: u32,
    /// The other phone's number.
    pub peer: String,
    pub state: CallState,
    /// The other phone's address, once connected.
    pub peer_ip: Option<Ipv4Addr>,
    pub rtp_sent: u32,
    pub rtp_received: u32,
    last_rtp_us: u64,
}

/// What a phone knows at run time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhoneState {
    /// The router it registered with, and the number it was given.
    pub agent: Option<Ipv4Addr>,
    pub number: Option<String>,
    /// The address it registered from (a new address means registering again).
    reg_ip: Option<Ipv4Addr>,
    /// Why the router refused it, or why the last call ended.
    pub message: Option<String>,
    last_try: Option<u64>,
    pub call: Option<PhoneCall>,
    rtp_seq: u16,
}

/// A phone the router has registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub number: String,
    /// The `ephone` entry (0: given a number by `auto assign`).
    pub ephone: u16,
}

/// A call the router is connecting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCall {
    pub id: u32,
    pub caller: Ipv4Addr,
    pub callee: Ipv4Addr,
    pub from: String,
    pub to: String,
    pub connected: bool,
}

/// What a router's call control knows at run time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentState {
    pub registrations: Vec<Registration>,
    pub calls: Vec<AgentCall>,
    next_call: u32,
}

/// Every phone's and router's voice state.
#[derive(Debug, Clone, Default)]
pub struct VoiceState {
    phones: BTreeMap<DeviceId, PhoneState>,
    agents: BTreeMap<DeviceId, AgentState>,
}

impl VoiceState {
    pub(crate) fn forget(&mut self, d: DeviceId) {
        self.phones.remove(&d);
        self.agents.remove(&d);
    }
}

/// The voice VLAN of the switch port a phone's network port is cabled to.
pub fn voice_vlan(t: &Topology, phone: DeviceId) -> Option<u16> {
    let peer = t.peer(End { device: phone, port: NETWORK_PORT })?;
    let sw = t.device(peer.device).filter(|d| d.kind == DeviceKind::Switch)?;
    let p = sw.ports.get(peer.port).filter(|p| !p.routed)?;
    if t.is_trunk(peer) {
        return None;
    }
    p.switchport.voice_vlan.map(|v| v.get())
}

/// One signalling message.
fn message(verb: &str, args: &[&str]) -> Vec<u8> {
    let mut s = format!("PCV1 {verb}");
    for a in args {
        s.push(' ');
        s.push_str(a);
    }
    s.into_bytes()
}

/// `PCV1 VERB args…` → (verb, args).
fn parse(payload: &[u8]) -> Option<(String, Vec<String>)> {
    let text = std::str::from_utf8(payload.get(..payload.len().min(512))?).ok()?;
    let mut words = text.split_whitespace();
    if words.next()? != "PCV1" {
        return None;
    }
    let verb = words.next()?.to_ascii_uppercase();
    Some((verb, words.take(8).map(|w| w.chars().take(64).collect()).collect()))
}

/// An RTP packet (RFC 3550): version 2, payload type 0, 160 bytes of µ-law silence.
fn rtp_packet(seq: u16, ssrc: u32) -> Vec<u8> {
    let mut b = vec![0x80, 0x00];
    b.extend_from_slice(&seq.to_be_bytes());
    b.extend_from_slice(&(u32::from(seq).wrapping_mul(160)).to_be_bytes());
    b.extend_from_slice(&ssrc.to_be_bytes());
    b.extend(std::iter::repeat_n(0xff, 160));
    b
}

/// Whether a payload is an RTP packet as phones send them.
fn is_rtp(b: &[u8]) -> bool {
    b.len() >= 12 && b.first().is_some_and(|v| v >> 6 == 2) && b.get(1).is_some_and(|pt| pt & 0x7f == 0)
}

impl Sim {
    /// A phone's run-time state (registration, call).
    pub fn phone(&self, d: DeviceId) -> Option<&PhoneState> {
        self.voice.phones.get(&d)
    }

    /// A router's registered phones and calls.
    pub fn call_agent(&self, d: DeviceId) -> Option<&AgentState> {
        self.voice.agents.get(&d)
    }

    /// Frames a phone itself sends out of its network port go tagged in the voice VLAN.
    pub(crate) fn phone_tag(&self, t: &Topology, from: End, frame: Vec<u8>) -> Vec<u8> {
        if from.port != NETWORK_PORT || !t.device(from.device).is_some_and(|d| d.kind == DeviceKind::Phone) {
            return frame;
        }
        match (voice_vlan(t, from.device), Frame::decode(&frame)) {
            (Some(v), Ok(f)) if f.vlan.is_none() => f.retagged(Some(v)).encode(),
            _ => frame,
        }
    }

    /// The built-in switch: frames between the PC port and the network port pass through;
    /// the phone takes what is its own.
    pub(crate) fn phone_in(&mut self, t: &Topology, d: &Device, port: usize, f: Frame, hops: u8) -> Result<(), String> {
        if hops >= MAX_SWITCH_HOPS {
            self.storm = true;
            return Err(format!("switching loop: the frame crossed {MAX_SWITCH_HOPS} switches"));
        }
        let own = d.ports.get(NETWORK_PORT).map(|p| p.mac).unwrap_or_default();
        let voice = voice_vlan(t, d.id);
        let other = if port == PC_PORT { NETWORK_PORT } else { PC_PORT };
        if port == NETWORK_PORT
            && let Some(tag) = f.vlan
        {
            // Tagged frames are the phone's own, in its voice VLAN.
            if voice != Some(tag) {
                return Err(format!("tagged for VLAN {tag}, and the phone's voice VLAN is {}", voice.map_or("none".into(), |v| v.to_string())));
            }
            return self.l3_in(t, d, NETWORK_PORT, f.retagged(None));
        }
        // Untagged: the access VLAN, shared with the PC unless the phone has a voice VLAN.
        let mine = voice.is_none() && (f.dst == own || f.dst.is_group());
        if mine {
            let r = self.l3_in(t, d, NETWORK_PORT, f.clone());
            if f.dst == own {
                return r;
            }
        }
        self.bridge(t, End { device: d.id, port: other }, f.encode(), hops.saturating_add(1))
    }

    /// Sends a frame out of the phone's other port, untouched.
    fn bridge(&mut self, t: &Topology, from: End, frame: Vec<u8>, hops: u8) -> Result<(), String> {
        if !port_up(t, from) {
            return Err("the phone's other port is not connected".into());
        }
        let Some(to) = t.peer(from) else { return Err("the phone's other port is not connected".into()) };
        self.push_hop(t, self.now + LINK_DELAY_US, from, to, frame, hops);
        Ok(())
    }

    /// Signalling and voice arriving at a phone or a router with `telephony-service`.
    pub(crate) fn voice_udp_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, u: &Udp) -> Option<Result<(), String>> {
        let agent_port = d.voice.service.as_ref().map(|s| s.port);
        match d.kind {
            DeviceKind::Router if agent_port == Some(u.dst_port) => Some(self.agent_in(t, d, ip, &u.payload)),
            DeviceKind::Phone if u.dst_port == SIGNAL_PORT => Some(self.phone_signal_in(t, d, ip, &u.payload)),
            DeviceKind::Phone if u.dst_port == RTP_PORT => Some(self.rtp_in(d, ip, &u.payload)),
            _ => None,
        }
    }

    fn send_udp(&mut self, t: &Topology, d: &Device, src: Ipv4Addr, dst: Ipv4Addr, ports: (u16, u16), payload: Vec<u8>) -> Result<(), String> {
        let udp = Udp { src_port: ports.0, dst_port: ports.1, payload };
        let ip = Ipv4 { id: self.ip_id(), ttl: default_ttl(d.kind), protocol: PROTO_UDP, src, dst, payload: udp.encode(src, dst) };
        self.send_ip(t, d, ip)
    }

    /// The router answers from the address the phone wrote to.
    fn agent_send(&mut self, t: &Topology, d: &Device, from: Ipv4Addr, to: Ipv4Addr, msg: Vec<u8>) {
        let port = d.voice.service.as_ref().map_or(SIGNAL_PORT, |s| s.port);
        let _ = self.send_udp(t, d, from, to, (port, SIGNAL_PORT), msg);
    }

    fn agent_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        let (verb, args) = parse(payload).ok_or("not a call-control message")?;
        let Some(service) = d.voice.service.clone() else { return Err("no telephony service".into()) };
        let me = ip.dst;
        let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
        match verb.as_str() {
            "REGISTER" => {
                let mac: MacAddr = arg(0).parse().map_err(|_| "REGISTER without a MAC address")?;
                let refuse = |why: &str| message("REJECTED", &[why]);
                if service.source.is_some_and(|s| s != me) {
                    let reply = refuse("register-with-the-source-address");
                    self.agent_send(t, d, me, ip.src, reply);
                    return Ok(());
                }
                let st = self.voice.agents.entry(d.id).or_default();
                st.registrations.retain(|r| r.mac != mac && r.ip != ip.src);
                let taken: Vec<String> = st.registrations.iter().map(|r| r.number.clone()).collect();
                let given = d.voice.number_for(mac).or_else(|| {
                    // `auto assign A to B`: the first number in the range nobody holds.
                    let (a, b) = service.auto_assign?;
                    let configured: Vec<u16> = d.voice.ephones.iter().flat_map(|e| e.buttons.iter().map(|(_, dn)| *dn)).collect();
                    d.voice
                        .dns
                        .iter()
                        .filter(|dn| (a..=b).contains(&dn.tag) && !configured.contains(&dn.tag))
                        .find_map(|dn| dn.number.clone().filter(|n| !taken.contains(n)))
                        .map(|n| (0, n))
                });
                let full = st.registrations.len() >= usize::from(service.max_ephones).min(MAX_REGISTRATIONS);
                let reply = match given {
                    _ if full => refuse("max-ephones-reached"),
                    None => refuse("no-ephone-or-free-number-for-this-phone"),
                    Some((_, ref n)) if taken.contains(n) => refuse("number-in-use"),
                    Some((ephone, number)) => {
                        st.registrations.push(Registration { mac, ip: ip.src, number: number.clone(), ephone });
                        self.log(d.id, &format!("Phone {mac} registered from {} as {number}", ip.src));
                        message("REGISTERED", &[&number])
                    }
                };
                self.agent_send(t, d, me, ip.src, reply);
                Ok(())
            }
            "DIAL" => {
                let st = self.voice.agents.entry(d.id).or_default();
                let Some(caller) = st.registrations.iter().find(|r| r.ip == ip.src).cloned() else {
                    let reply = message("RELEASE", &["0", "not-registered"]);
                    self.agent_send(t, d, me, ip.src, reply);
                    return Ok(());
                };
                let to = arg(0).to_string();
                let callee = st.registrations.iter().find(|r| r.number == to).cloned();
                let busy = |a: Ipv4Addr| st.calls.iter().any(|c| c.caller == a || c.callee == a);
                let outcome = match callee {
                    None => Err(format!("number-{to}-unknown")),
                    Some(c) if c.ip == caller.ip => Err("that-is-your-own-number".into()),
                    Some(c) if busy(c.ip) => Err("busy".into()),
                    Some(_) if st.calls.len() >= MAX_CALLS => Err("no-free-lines".into()),
                    Some(c) => Ok(c),
                };
                match outcome {
                    Err(why) => {
                        self.agent_send(t, d, me, ip.src, message("RELEASE", &["0", &why]));
                    }
                    Ok(callee) => {
                        st.next_call = st.next_call.wrapping_add(1).max(1);
                        let id = st.next_call;
                        st.calls.push(AgentCall {
                            id,
                            caller: caller.ip,
                            callee: callee.ip,
                            from: caller.number.clone(),
                            to: to.clone(),
                            connected: false,
                        });
                        let sid = id.to_string();
                        self.agent_send(t, d, me, callee.ip, message("RING", &[&sid, &caller.number]));
                        self.agent_send(t, d, me, caller.ip, message("RINGBACK", &[&sid, &to]));
                    }
                }
                Ok(())
            }
            "ANSWER" | "HANGUP" => {
                let id: u32 = arg(0).parse().map_err(|_| "no call number")?;
                let st = self.voice.agents.entry(d.id).or_default();
                let Some(i) = st.calls.iter().position(|c| c.id == id && (c.caller == ip.src || c.callee == ip.src)) else {
                    return Err(format!("no call {id} for {}", ip.src));
                };
                let sid = id.to_string();
                if verb == "ANSWER" {
                    let Some(c) = st.calls.get_mut(i).filter(|c| c.callee == ip.src) else { return Err("only the called phone answers".into()) };
                    c.connected = true;
                    let (a, b) = (c.caller, c.callee);
                    self.agent_send(t, d, me, a, message("CONNECT", &[&sid, &b.to_string()]));
                    self.agent_send(t, d, me, b, message("CONNECT", &[&sid, &a.to_string()]));
                } else {
                    let c = st.calls.remove(i);
                    let other = if c.caller == ip.src { c.callee } else { c.caller };
                    self.agent_send(t, d, me, other, message("RELEASE", &[&sid, "hung-up"]));
                    self.agent_send(t, d, me, ip.src, message("RELEASE", &[&sid, "hung-up"]));
                }
                Ok(())
            }
            other => Err(format!("unknown call-control message {other}")),
        }
    }

    fn phone_signal_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        let (verb, args) = parse(payload).ok_or("not a call-control message")?;
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        let id: u32 = arg(0).parse().unwrap_or(0);
        let now = self.now;
        let st = self.voice.phones.entry(d.id).or_default();
        if st.agent.is_some_and(|a| a != ip.src) && verb != "REGISTERED" && verb != "REJECTED" {
            return Err(format!("call-control message from {}, not this phone's router", ip.src));
        }
        let new_call = |state: CallState| PhoneCall { id, peer: arg(1), state, peer_ip: None, rtp_sent: 0, rtp_received: 0, last_rtp_us: 0 };
        match verb.as_str() {
            "REGISTERED" => {
                st.agent = Some(ip.src);
                st.number = Some(arg(0));
                st.message = None;
            }
            "REJECTED" => {
                st.agent = None;
                st.number = None;
                st.message = Some(format!("registration refused: {}", arg(0).replace('-', " ")));
            }
            "RINGBACK" => st.call = Some(new_call(CallState::Ringback)),
            "RING" => {
                if st.call.is_some() {
                    return Err("already in a call".into());
                }
                st.call = Some(new_call(CallState::Ringing));
            }
            "CONNECT" => {
                let peer: Ipv4Addr = arg(1).parse().map_err(|_| "CONNECT without an address")?;
                let Some(c) = st.call.as_mut().filter(|c| c.id == id) else { return Err(format!("no call {id} on this phone")) };
                c.state = CallState::Connected;
                c.peer_ip = Some(peer);
                c.last_rtp_us = now;
                // The first voice packet goes at once.
                self.send_rtp(t, d);
            }
            "RELEASE" => {
                if st.call.as_ref().is_some_and(|c| c.id == id || id == 0) {
                    st.call = None;
                }
                st.message = Some(format!("call ended: {}", arg(1).replace('-', " ")));
            }
            other => return Err(format!("unknown call-control message {other}")),
        }
        Ok(())
    }

    fn rtp_in(&mut self, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        if !is_rtp(payload) {
            return Err("not an RTP packet".into());
        }
        let c = self.voice.phones.get_mut(&d.id).and_then(|s| s.call.as_mut()).ok_or("voice packet with no call up")?;
        if c.state != CallState::Connected || c.peer_ip != Some(ip.src) {
            return Err("voice packet from a phone this one isn't talking to".into());
        }
        c.rtp_received = c.rtp_received.saturating_add(1);
        Ok(())
    }

    fn send_rtp(&mut self, t: &Topology, d: &Device) {
        let Some(src) = d.ports.get(NETWORK_PORT).and_then(|p| p.ip).map(|n| n.addr) else { return };
        let st = self.voice.phones.entry(d.id).or_default();
        let Some(c) = st.call.as_mut().filter(|c| c.state == CallState::Connected) else { return };
        let Some(peer) = c.peer_ip else { return };
        st.rtp_seq = st.rtp_seq.wrapping_add(1);
        c.rtp_sent = c.rtp_sent.saturating_add(1);
        let ssrc = u32::from_be_bytes([0x50, 0x43, (d.id.0 >> 8) as u8, d.id.0 as u8]);
        let pkt = rtp_packet(st.rtp_seq, ssrc);
        let _ = self.send_udp(t, d, src, peer, (RTP_PORT, RTP_PORT), pkt);
    }

    fn phone_send(&mut self, t: &Topology, d: &Device, to: Ipv4Addr, msg: Vec<u8>) -> Result<(), String> {
        let src = d.ports.get(NETWORK_PORT).and_then(|p| p.ip).map(|n| n.addr).ok_or("the phone has no address")?;
        self.send_udp(t, d, src, to, (SIGNAL_PORT, SIGNAL_PORT), msg)
    }

    /// Before each run: phones with an address and a TFTP server register (again, after a
    /// change of address or a refusal); phones in a call send their voice packet.
    pub(crate) fn voice_tick(&mut self, t: &Topology) {
        let now = self.now;
        for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Phone && !d.powered_off) {
            let ip = d.ports.get(NETWORK_PORT).and_then(|p| p.ip).map(|n| n.addr);
            let st = self.voice.phones.entry(d.id).or_default();
            if st.number.is_some() && (st.reg_ip != ip || d.voice.tftp.is_none_or(|a| st.agent != Some(a))) {
                // New address, or another server: register again.
                st.number = None;
                st.agent = None;
                st.call = None;
            }
            let due = st.last_try.is_none_or(|at| now.saturating_sub(at) >= REGISTER_RETRY_US);
            if let (None, Some(_), Some(server), true) = (&st.number, ip, d.voice.tftp, due) {
                st.last_try = Some(now);
                st.reg_ip = ip;
                let mac = d.ports.get(NETWORK_PORT).map(|p| p.mac).unwrap_or_default();
                let _ = self.phone_send(t, d, server, message("REGISTER", &[&mac.to_string()]));
            }
            let rtp_due = self
                .voice
                .phones
                .get(&d.id)
                .and_then(|s| s.call.as_ref())
                .is_some_and(|c| c.state == CallState::Connected && now.saturating_sub(c.last_rtp_us) >= RTP_EVERY_US);
            if rtp_due {
                if let Some(c) = self.voice.phones.get_mut(&d.id).and_then(|s| s.call.as_mut()) {
                    c.last_rtp_us = now;
                }
                self.send_rtp(t, d);
            }
        }
    }

    fn phone_wait(&mut self, t: &Topology, dev: DeviceId, done: impl Fn(&PhoneState) -> bool) {
        let deadline = self.now.saturating_add(2_000_000);
        self.advance(t, deadline, |s| s.voice.phones.get(&dev).is_some_and(&done));
    }

    fn phone_device<'a>(&self, t: &'a Topology, dev: DeviceId) -> Result<&'a Device, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        if d.kind != DeviceKind::Phone {
            return Err(SimError::NoSource(format!("{} is not an IP phone", d.name)));
        }
        if d.powered_off {
            return Err(SimError::NoSource(format!("{} is switched off", d.name)));
        }
        Ok(d)
    }

    /// Lets the phones register (a second of simulated time), then reports.
    pub fn phone_sync(&mut self, t: &Topology) {
        self.run_for(t, 1_000_000);
    }

    /// Dials a number from a phone and waits for the router's answer.
    pub fn phone_dial(&mut self, t: &Topology, dev: DeviceId, number: &str) -> Result<(), SimError> {
        let d = self.phone_device(t, dev)?;
        if !netlabcraft_model::voice::valid_number(number) {
            return Err(SimError::NoSource(format!("`{number}` is not a directory number (digits only)")));
        }
        self.phone_sync(t);
        let st = self.voice.phones.get(&dev).cloned().unwrap_or_default();
        let Some(agent) = st.agent.filter(|_| st.number.is_some()) else {
            return Err(SimError::NoSource(st.message.unwrap_or_else(|| "the phone is not registered with a call server".into())));
        };
        if st.call.is_some() {
            return Err(SimError::NoSource("the phone is already in a call: hang up first".into()));
        }
        if let Some(s) = self.voice.phones.get_mut(&dev) {
            s.call = Some(PhoneCall {
                id: 0,
                peer: number.to_string(),
                state: CallState::Dialing,
                peer_ip: None,
                rtp_sent: 0,
                rtp_received: 0,
                last_rtp_us: 0,
            });
            s.message = None;
        }
        self.phone_send(t, d, agent, message("DIAL", &[number])).map_err(SimError::NoSource)?;
        self.phone_wait(t, dev, |s| s.call.as_ref().is_none_or(|c| c.state != CallState::Dialing));
        if let Some(s) = self.voice.phones.get_mut(&dev)
            && s.call.as_ref().is_some_and(|c| c.state == CallState::Dialing)
        {
            s.call = None;
            s.message = Some("the call server did not answer".into());
        }
        Ok(())
    }

    /// Answers a ringing phone (or with `hangup`, ends its call).
    pub fn phone_answer(&mut self, t: &Topology, dev: DeviceId, hangup: bool) -> Result<(), SimError> {
        let d = self.phone_device(t, dev)?;
        let st = self.voice.phones.get(&dev).cloned().unwrap_or_default();
        let (Some(agent), Some(call)) = (st.agent, st.call) else {
            return Err(SimError::NoSource(if hangup { "no call to hang up".into() } else { "the phone is not ringing".into() }));
        };
        if !hangup && call.state != CallState::Ringing {
            return Err(SimError::NoSource("the phone is not ringing".into()));
        }
        if hangup && call.id == 0 {
            if let Some(s) = self.voice.phones.get_mut(&dev) {
                s.call = None;
            }
            return Ok(());
        }
        let msg = message(if hangup { "HANGUP" } else { "ANSWER" }, &[&call.id.to_string()]);
        self.phone_send(t, d, agent, msg).map_err(SimError::NoSource)?;
        if hangup {
            self.phone_wait(t, dev, |s| s.call.is_none());
        } else {
            self.phone_wait(t, dev, |s| s.call.as_ref().is_none_or(|c| c.state == CallState::Connected));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_and_hostile_ones_are_refused() {
        let m = message("RING", &["7", "1001"]);
        assert_eq!(parse(&m), Some(("RING".into(), vec!["7".into(), "1001".into()])));
        assert_eq!(parse(b"HELLO"), None);
        assert_eq!(parse(&[0xff, 0xfe]), None);
        assert_eq!(parse(b""), None);
        let r = rtp_packet(3, 9);
        assert!(is_rtp(&r) && r.len() == 172);
        for n in 0..12 {
            assert!(!is_rtp(&r[..n]));
        }
    }
}
