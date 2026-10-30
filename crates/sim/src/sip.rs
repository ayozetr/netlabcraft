//! SIP phones (RFC 3261, the subset a basic call needs) and a router's SIP call control
//! (`voice register global`, `voice register dn`, `voice register pool`).
//!
//! A SIP phone registers with `REGISTER`, its user part being its MAC address (12 hex
//! digits); the router finds the `voice register pool` with that `id mac` and answers `200 OK`
//! naming the phone's number in `P-Associated-URI` (RFC 3455). A call goes through the router,
//! which acts as a stateful proxy keyed by `Call-ID`:
//!
//! ```text
//! caller            router             callee
//!   INVITE (SDP) ->   100 Trying
//!                     INVITE (SDP) ->
//!                            <- 180 Ringing
//!   <- 180 Ringing
//!                            <- 200 OK (SDP)
//!   <- 200 OK (SDP)
//!   ACK ->            ACK ->
//!   ... RTP phone to phone ...
//!   BYE ->            BYE ->
//!                            <- 200 OK
//!   <- 200 OK
//! ```
//!
//! A caller that gives up before the answer sends `CANCEL` (the router answers `487 Request
//! Terminated` for the call); a callee that refuses answers `603 Decline`.

use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, MacAddr};
use netlabcraft_model::voice::SIP_PORT;
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::Ipv4;
use netlabcraft_wire::sip::{Message, sdp};

use crate::Sim;
use crate::voice::{CallState, NETWORK_PORT, PhoneCall, RTP_PORT, Registration};

/// Most SIP calls one router keeps.
const MAX_SIP_CALLS: usize = 128;

/// A call the router's SIP call control is connecting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SipCall {
    pub call_id: String,
    pub caller: Ipv4Addr,
    pub callee: Ipv4Addr,
    pub from: String,
    pub to: String,
    pub connected: bool,
}

/// A phone's SIP identity: its MAC address as 12 hex digits.
pub fn mac_user(mac: MacAddr) -> String {
    mac.0.iter().map(|b| format!("{b:02x}")).collect()
}

fn user_mac(user: &str) -> Option<MacAddr> {
    if user.len() != 12 || !user.is_ascii() {
        return None;
    }
    let mut b = [0u8; 6];
    for (i, x) in b.iter_mut().enumerate() {
        *x = u8::from_str_radix(user.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(MacAddr(b))
}

/// A response to `req`, copying the headers that tie it to the request.
fn reply(req: &Message, code: u16, reason: &str) -> Message {
    let mut m = Message::response(code, reason);
    for h in ["Via", "From", "To", "Call-ID", "CSeq"] {
        if let Some(v) = req.header(h) {
            m = m.with(h, v);
        }
    }
    m
}

impl Sim {
    fn sip_send(&mut self, t: &Topology, d: &Device, src: Ipv4Addr, dst: Ipv4Addr, sport: u16, m: &Message) -> Result<(), String> {
        self.send_udp(t, d, src, dst, (sport, SIP_PORT), m.encode())
    }

    /// The router's SIP port (`source-address … port P`).
    pub(crate) fn sip_agent_port(d: &Device) -> Option<u16> {
        d.voice.register.as_ref().map(|r| r.port)
    }

    /// SIP arriving at the router's call control.
    pub(crate) fn sip_agent_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        let m = Message::decode(payload).map_err(|e| format!("not a SIP message: {e}"))?;
        let Some(reg) = d.voice.register.clone() else { return Err("no voice register global".into()) };
        let me = ip.dst;
        let port = reg.port;
        let call_id = m.header("Call-ID").unwrap_or_default().to_string();
        // Responses from phones: relay them to the other end of the call.
        if let Some(code) = m.code() {
            let st = self.voice.agents.entry(d.id).or_default();
            let Some(i) = st.sip_calls.iter().position(|c| c.call_id == call_id) else { return Ok(()) };
            let Some(c) = st.sip_calls.get_mut(i) else { return Ok(()) };
            let other = if ip.src == c.callee { c.caller } else { c.callee };
            if m.cseq_method() == Some("INVITE") {
                if code == 200 {
                    c.connected = true;
                } else if code >= 300 {
                    st.sip_calls.remove(i);
                }
            }
            if m.cseq_method() == Some("BYE") || m.cseq_method() == Some("CANCEL") {
                return Ok(());
            }
            return self.sip_send(t, d, me, other, port, &m);
        }
        let method = m.method().unwrap_or_default().to_string();
        match method.as_str() {
            "REGISTER" => {
                let answer = |code: u16, reason: &str| reply(&m, code, reason);
                if reg.source.is_some_and(|s| s != me) {
                    return self.sip_send(t, d, me, ip.src, port, &answer(403, "Forbidden - register with the source address"));
                }
                let Some(mac) = m.user_of("From").and_then(|u| user_mac(&u)) else {
                    return self.sip_send(t, d, me, ip.src, port, &answer(400, "Bad Request"));
                };
                let st = self.voice.agents.entry(d.id).or_default();
                st.registrations.retain(|r| !(r.sip && (r.mac == mac || r.ip == ip.src)));
                let sip_count = st.registrations.iter().filter(|r| r.sip).count();
                let taken: Vec<String> = st.registrations.iter().map(|r| r.number.clone()).collect();
                let out = match d.voice.sip_number_for(mac) {
                    _ if sip_count >= usize::from(reg.max_pool) => answer(503, "Service Unavailable - max-pool reached"),
                    None => answer(403, "Forbidden - no voice register pool for this phone"),
                    Some((_, None)) => answer(404, "Not Found - the pool has no number"),
                    Some((_, Some(n))) if taken.contains(&n) => answer(409, "Conflict - number in use"),
                    Some((pool, Some(number))) => {
                        st.registrations.push(Registration { mac, ip: ip.src, number: number.clone(), ephone: pool, sip: true });
                        self.log(d.id, &format!("SIP phone {mac} registered from {} as {number}", ip.src));
                        answer(200, "OK")
                            .with("Contact", m.header("Contact").unwrap_or_default())
                            .with("Expires", "3600")
                            .with("P-Associated-URI", format!("<sip:{number}@{me}>"))
                    }
                };
                self.sip_send(t, d, me, ip.src, port, &out)
            }
            "INVITE" => {
                let st = self.voice.agents.entry(d.id).or_default();
                let Some(caller) = st.registrations.iter().find(|r| r.sip && r.ip == ip.src).cloned() else {
                    return self.sip_send(t, d, me, ip.src, port, &reply(&m, 403, "Forbidden - not registered"));
                };
                let to = m.user_of("To").unwrap_or_default();
                let callee = st.registrations.iter().find(|r| r.sip && r.number == to).cloned();
                let busy = |a: Ipv4Addr| st.sip_calls.iter().any(|c| c.caller == a || c.callee == a);
                let refuse = match &callee {
                    None => Some((404, "Not Found")),
                    Some(c) if c.ip == caller.ip => Some((482, "Loop Detected")),
                    Some(c) if busy(c.ip) => Some((486, "Busy Here")),
                    Some(_) if st.sip_calls.len() >= MAX_SIP_CALLS => Some((503, "Service Unavailable")),
                    Some(_) => None,
                };
                if let Some((code, reason)) = refuse {
                    return self.sip_send(t, d, me, ip.src, port, &reply(&m, code, reason));
                }
                let Some(callee) = callee else { return Ok(()) };
                st.sip_calls.push(SipCall { call_id, caller: caller.ip, callee: callee.ip, from: caller.number, to: to.clone(), connected: false });
                self.sip_send(t, d, me, ip.src, port, &reply(&m, 100, "Trying"))?;
                let mut fwd = Message { start: Message::request("INVITE", &format!("sip:{to}@{}", callee.ip)).start, ..m.clone() };
                fwd.headers.insert(0, ("Via".into(), format!("SIP/2.0/UDP {me}:{port};branch=z9hG4bK{}", self.now)));
                self.sip_send(t, d, me, callee.ip, port, &fwd)
            }
            "ACK" | "BYE" | "CANCEL" => {
                let st = self.voice.agents.entry(d.id).or_default();
                let Some(i) = st.sip_calls.iter().position(|c| c.call_id == call_id && (c.caller == ip.src || c.callee == ip.src)) else {
                    return if method == "ACK" { Ok(()) } else { self.sip_send(t, d, me, ip.src, port, &reply(&m, 481, "Call Does Not Exist")) };
                };
                let Some(c) = st.sip_calls.get(i).cloned() else { return Ok(()) };
                let other = if c.caller == ip.src { c.callee } else { c.caller };
                if method != "ACK" {
                    st.sip_calls.remove(i);
                    self.sip_send(t, d, me, ip.src, port, &reply(&m, 200, "OK"))?;
                }
                if method == "CANCEL" {
                    // The caller gave up: the INVITE ends with 487 for it.
                    let ended = Message::response(487, "Request Terminated").with("Call-ID", call_id.clone()).with("CSeq", "1 INVITE");
                    self.sip_send(t, d, me, ip.src, port, &ended)?;
                }
                self.sip_send(t, d, me, other, port, &m)
            }
            other => self.sip_send(t, d, me, ip.src, port, &reply(&m, 405, &format!("Method Not Allowed ({other})"))),
        }
    }

    /// SIP arriving at a SIP phone.
    pub(crate) fn sip_phone_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        let m = Message::decode(payload).map_err(|e| format!("not a SIP message: {e}"))?;
        let now = self.now;
        let me = d.ports.get(NETWORK_PORT).and_then(|p| p.ip).map(|n| n.addr).ok_or("the phone has no address")?;
        let st = self.voice.phones.entry(d.id).or_default();
        if st.agent.is_some_and(|a| a != ip.src) && m.cseq_method() != Some("REGISTER") {
            return Err(format!("SIP from {}, not this phone's registrar", ip.src));
        }
        let call_id = m.header("Call-ID").unwrap_or_default().to_string();
        if let Some(code) = m.code() {
            match m.cseq_method() {
                Some("REGISTER") => {
                    if code == 200 {
                        st.agent = Some(ip.src);
                        st.number = m.user_of("P-Associated-URI");
                        st.message = None;
                    } else {
                        st.agent = None;
                        st.number = None;
                        let reason = match &m.start {
                            netlabcraft_wire::sip::Start::Response { reason, .. } => reason.clone(),
                            netlabcraft_wire::sip::Start::Request { .. } => String::new(),
                        };
                        st.message = Some(format!("registration refused: {code} {reason}"));
                    }
                }
                Some("INVITE") => {
                    let mine = st.call.as_ref().is_some_and(|c| c.sip_id.as_deref() == Some(call_id.as_str()));
                    if !mine {
                        return Ok(());
                    }
                    match code {
                        100 => {}
                        180 => {
                            if let Some(c) = st.call.as_mut() {
                                c.state = CallState::Ringback;
                            }
                        }
                        200 => {
                            let peer = m.sdp_address();
                            if let Some(c) = st.call.as_mut() {
                                c.state = CallState::Connected;
                                c.peer_ip = peer;
                                c.last_rtp_us = now;
                            }
                            let ack = Message::request("ACK", &format!("sip:{}@{}", st.call.as_ref().map_or("", |c| c.peer.as_str()), ip.src))
                                .with("Via", format!("SIP/2.0/UDP {me}:{SIP_PORT};branch=z9hG4bKa{now}"))
                                .with("From", m.header("From").unwrap_or_default())
                                .with("To", m.header("To").unwrap_or_default())
                                .with("Call-ID", call_id.clone())
                                .with("CSeq", "1 ACK");
                            self.sip_send(t, d, me, ip.src, SIP_PORT, &ack)?;
                            self.send_rtp(t, d);
                        }
                        _ => {
                            let reason = match &m.start {
                                netlabcraft_wire::sip::Start::Response { reason, .. } => reason.clone(),
                                netlabcraft_wire::sip::Start::Request { .. } => String::new(),
                            };
                            st.call = None;
                            st.message = Some(format!("call ended: {code} {reason}"));
                        }
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        match m.method().unwrap_or_default() {
            "INVITE" => {
                if st.call.is_some() {
                    return self.sip_send(t, d, me, ip.src, SIP_PORT, &reply(&m, 486, "Busy Here"));
                }
                let from = m.user_of("From").unwrap_or_default();
                st.call = Some(PhoneCall {
                    id: 0,
                    peer: from,
                    state: CallState::Ringing,
                    peer_ip: m.sdp_address(),
                    rtp_sent: 0,
                    rtp_received: 0,
                    last_rtp_us: 0,
                    sip_id: Some(call_id),
                });
                st.sip_invite = Some(m.clone());
                self.sip_send(t, d, me, ip.src, SIP_PORT, &reply(&m, 180, "Ringing"))
            }
            "ACK" => {
                if let Some(c) = st.call.as_mut().filter(|c| c.sip_id.as_deref() == Some(call_id.as_str())) {
                    c.state = CallState::Connected;
                    c.last_rtp_us = now;
                    self.send_rtp(t, d);
                }
                Ok(())
            }
            "BYE" | "CANCEL" => {
                if st.call.as_ref().is_some_and(|c| c.sip_id.as_deref() == Some(call_id.as_str())) {
                    st.call = None;
                    st.sip_invite = None;
                    st.message = Some("call ended: hung up".into());
                }
                self.sip_send(t, d, me, ip.src, SIP_PORT, &reply(&m, 200, "OK"))
            }
            other => Err(format!("SIP {other} not handled by a phone")),
        }
    }

    /// A SIP phone registers (from `voice_tick`).
    pub(crate) fn sip_register(&mut self, t: &Topology, d: &Device, server: Ipv4Addr) {
        let Some(me) = d.ports.get(NETWORK_PORT).and_then(|p| p.ip).map(|n| n.addr) else { return };
        let user = mac_user(d.ports.get(NETWORK_PORT).map(|p| p.mac).unwrap_or_default());
        let m = Message::request("REGISTER", &format!("sip:{server}"))
            .with("Via", format!("SIP/2.0/UDP {me}:{SIP_PORT};branch=z9hG4bKr{}", self.now))
            .with("From", format!("<sip:{user}@{server}>;tag={}", d.id.0))
            .with("To", format!("<sip:{user}@{server}>"))
            .with("Call-ID", format!("reg-{user}@{me}"))
            .with("CSeq", "1 REGISTER")
            .with("Contact", format!("<sip:{user}@{me}:{SIP_PORT}>"))
            .with("Expires", "3600");
        let _ = self.sip_send(t, d, me, server, SIP_PORT, &m);
    }

    /// A SIP phone dials: INVITE with its SDP offer.
    pub(crate) fn sip_dial(&mut self, t: &Topology, d: &Device, agent: Ipv4Addr, number: &str) -> Result<(), String> {
        let me = d.ports.get(NETWORK_PORT).and_then(|p| p.ip).map(|n| n.addr).ok_or("the phone has no address")?;
        let st = self.voice.phones.entry(d.id).or_default();
        st.sip_seq = st.sip_seq.wrapping_add(1);
        let call_id = format!("{}-{}@{me}", d.id.0, st.sip_seq);
        let own = st.number.clone().unwrap_or_default();
        if let Some(c) = st.call.as_mut() {
            c.sip_id = Some(call_id.clone());
        }
        let m = Message::request("INVITE", &format!("sip:{number}@{agent}"))
            .with("Via", format!("SIP/2.0/UDP {me}:{SIP_PORT};branch=z9hG4bKi{}", self.now))
            .with("From", format!("<sip:{own}@{agent}>;tag={}", d.id.0))
            .with("To", format!("<sip:{number}@{agent}>"))
            .with("Call-ID", call_id)
            .with("CSeq", "1 INVITE")
            .with("Contact", format!("<sip:{own}@{me}:{SIP_PORT}>"))
            .with("Content-Type", "application/sdp");
        let m = Message { body: sdp(me, RTP_PORT, d.id.0), ..m };
        self.sip_send(t, d, me, agent, SIP_PORT, &m)
    }

    /// A SIP phone answers (200 OK with its SDP) or hangs up (603 Decline while ringing, BYE
    /// once talking, CANCEL while its own call rings).
    pub(crate) fn sip_answer(&mut self, t: &Topology, d: &Device, agent: Ipv4Addr, call: &PhoneCall, hangup: bool) -> Result<(), String> {
        let me = d.ports.get(NETWORK_PORT).and_then(|p| p.ip).map(|n| n.addr).ok_or("the phone has no address")?;
        let invite = self.voice.phones.get(&d.id).and_then(|s| s.sip_invite.clone());
        let call_id = call.sip_id.clone().unwrap_or_default();
        let own = self.voice.phones.get(&d.id).and_then(|s| s.number.clone()).unwrap_or_default();
        let msg = match (call.state, hangup, invite) {
            (CallState::Ringing, false, Some(inv)) => {
                let m = reply(&inv, 200, "OK").with("Contact", format!("<sip:{own}@{me}:{SIP_PORT}>")).with("Content-Type", "application/sdp");
                Message { body: sdp(me, RTP_PORT, d.id.0), ..m }
            }
            (CallState::Ringing, true, Some(inv)) => reply(&inv, 603, "Decline"),
            (state, true, _) => {
                let method = if state == CallState::Connected { "BYE" } else { "CANCEL" };
                Message::request(method, &format!("sip:{}@{agent}", call.peer))
                    .with("Via", format!("SIP/2.0/UDP {me}:{SIP_PORT};branch=z9hG4bKb{}", self.now))
                    .with("From", format!("<sip:{own}@{agent}>;tag={}", d.id.0))
                    .with("To", format!("<sip:{}@{agent}>", call.peer))
                    .with("Call-ID", call_id)
                    .with("CSeq", format!("2 {method}"))
            }
            _ => return Err("nothing to answer".into()),
        };
        if hangup && let Some(s) = self.voice.phones.get_mut(&d.id) {
            s.call = None;
            s.sip_invite = None;
            s.message = Some("call ended: hung up".into());
        }
        self.sip_send(t, d, me, agent, SIP_PORT, &msg)
    }

    /// A router's SIP calls.
    pub fn sip_calls(&self, d: DeviceId) -> &[SipCall] {
        self.voice.agents.get(&d).map_or(&[], |a| a.sip_calls.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_users_round_trip_and_hostile_ones_are_refused() {
        let mac = MacAddr([0x02, 0, 0, 0x0a, 0xbc, 0xff]);
        assert_eq!(mac_user(mac), "0200000abcff");
        assert_eq!(user_mac("0200000abcff"), Some(mac));
        for bad in ["", "02", "0200000abcfg", "0200000abcffaa", "ñ200000abcf"] {
            assert_eq!(user_mac(bad), None, "{bad}");
        }
    }
}
