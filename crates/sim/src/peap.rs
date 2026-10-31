//! PEAP logins of WPA2-Enterprise, end to end: the host's side of the tunnel, and the
//! authentication server's, with the controller passing EAP between them inside RADIUS
//! (EAP-Message and Message-Authenticator, RFC 3579) as a pass-through authenticator.
//!
//! The tunnel is [`netlabcraft_wire::peap`]'s small TLS-like protocol (a real, if toy-sized,
//! Diffie-Hellman exchange and AES-CBC records; no certificates). Inside it the server asks for
//! the identity, then the password with EAP-GTC, checks them against its AAA users and says how
//! it went with a Result TLV; the host's password is only ever on the wire encrypted. The
//! server then accepts with the EAP master session key in an MPPE key attribute, which the
//! controller hands to the access point for the 4-way handshake, as the host derives the same
//! key from its end of the tunnel.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::peap::{
    self, EAP_GTC, EAP_TLV, FLAG_START, HS_CLIENT_HELLO, HS_FINISHED, HS_SERVER_HELLO, Hello, Keys, REC_APP_DATA, REC_HANDSHAKE, Record,
};
use netlabcraft_wire::radius::{ACCESS_ACCEPT, ACCESS_CHALLENGE, ACCESS_REJECT, RADIUS_PORT, Radius};
use netlabcraft_wire::wlan::{EAP_FAILURE, EAP_IDENTITY, EAP_PEAP, EAP_REQUEST, EAP_RESPONSE, EAP_SUCCESS, Eap};
use netlabcraft_wire::{Ipv4, Udp};

use crate::Sim;

/// Most PEAP logins a server keeps under way at once.
const MAX_SESSIONS: usize = 64;

fn seeded(seed: &[u8]) -> [u8; 32] {
    let a = netlabcraft_wire::sha1::sha1(seed);
    let b = netlabcraft_wire::sha1::sha1(&a);
    let mut out = [0u8; 32];
    out[..20].copy_from_slice(&a);
    out[20..].copy_from_slice(&b[..12]);
    out
}

fn iv(seed: &[u8]) -> [u8; 16] {
    netlabcraft_wire::md5::md5(seed)
}

/// The host's end of a PEAP tunnel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientTunnel {
    random: [u8; 32],
    secret: u64,
    server_random: [u8; 32],
    transcript: Vec<u8>,
    keys: Option<Keys>,
    /// The pairwise master key the tunnel exported, once the server said the login was good.
    pub pmk: Option<[u8; 32]>,
    /// Records sealed so far (for their IVs).
    sealed: u64,
}

impl ClientTunnel {
    pub fn new(seed: &[u8]) -> ClientTunnel {
        let random = seeded(seed);
        ClientTunnel { random, secret: peap::dh_secret(&random), ..ClientTunnel::default() }
    }

    fn seal(&mut self, inner: &Eap) -> Result<Vec<u8>, String> {
        self.sealed = self.sealed.saturating_add(1);
        let iv = iv(&[&self.random[..], &self.sealed.to_be_bytes()].concat());
        let keys = self.keys.as_mut().ok_or("no tunnel yet")?;
        Ok(peap::peap_data(0, &[keys.seal(true, &inner.encode(), iv)]))
    }

    /// The host's answer to the data of a PEAP request from the server.
    pub fn answer(&mut self, data: &[u8], user: &str, password: &str) -> Result<Vec<u8>, String> {
        let (flags, records) = peap::parse_peap(data).map_err(|e| e.to_string())?;
        if flags & FLAG_START != 0 {
            let hello = Hello { random: self.random, public: peap::dh_public(self.secret), name: String::new() };
            let hs = peap::handshake(HS_CLIENT_HELLO, &hello.encode());
            self.transcript = hs.clone();
            self.keys = None;
            self.pmk = None;
            return Ok(peap::peap_data(0, &[Record { kind: REC_HANDSHAKE, body: hs }]));
        }
        let r = records.first().ok_or("an empty PEAP request")?;
        match r.kind {
            REC_HANDSHAKE => {
                let msgs = peap::handshakes(&r.body).map_err(|e| e.to_string())?;
                let (Some((HS_SERVER_HELLO, sh)), Some((HS_FINISHED, fin))) = (msgs.first(), msgs.get(1)) else {
                    return Err("the server's handshake is not a server hello and finished".into());
                };
                let hello = Hello::decode(sh).map_err(|e| e.to_string())?;
                self.server_random = hello.random;
                self.transcript.extend(peap::handshake(HS_SERVER_HELLO, sh));
                let shared = peap::dh_shared(hello.public, self.secret);
                if shared == 0 {
                    return Err("the server's key share is not in the group".into());
                }
                let keys = Keys::derive(shared, &self.random, &self.server_random);
                if keys.finished(false, &self.transcript) != *fin {
                    return Err("the server's Finished doesn't check out: not the tunnel this host started".into());
                }
                self.transcript.extend(peap::handshake(HS_FINISHED, fin));
                let mine = peap::handshake(HS_FINISHED, &keys.finished(true, &self.transcript));
                self.keys = Some(keys);
                Ok(peap::peap_data(0, &[Record { kind: REC_HANDSHAKE, body: mine }]))
            }
            REC_APP_DATA => {
                let keys = self.keys.as_mut().ok_or("tunnel data before the handshake")?;
                let plain = keys.open(false, r).map_err(|e| e.to_string())?;
                let q = Eap::decode(&plain).map_err(|e| e.to_string())?;
                if q.code != EAP_REQUEST {
                    return Err("inside the tunnel: not an EAP request".into());
                }
                let reply = match q.kind {
                    Some(EAP_IDENTITY) => Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_IDENTITY), data: user.as_bytes().to_vec() },
                    Some(EAP_GTC) => Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_GTC), data: password.as_bytes().to_vec() },
                    Some(EAP_TLV) => {
                        let ok = peap::tlv_result(&q.data).ok_or("a TLV request without a result")?;
                        if ok {
                            let msk = keys.msk(&self.random, &self.server_random);
                            let mut pmk = [0u8; 32];
                            pmk.copy_from_slice(msk.get(..32).unwrap_or(&[0; 32]));
                            self.pmk = Some(pmk);
                        }
                        Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_TLV), data: peap::result_tlv(ok) }
                    }
                    other => return Err(format!("inside the tunnel: EAP method {} not supported", other.unwrap_or(0))),
                };
                self.seal(&reply)
            }
            _ => Err("a TLS alert from the server".into()),
        }
    }
}

/// Where a server's PEAP login stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Started,
    HelloSent,
    Tunnel,
    AskedPassword,
    SentResult(bool),
}

/// The authentication server's end of one PEAP login.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ServerSession {
    server: Option<netlabcraft_core::DeviceId>,
    client_random: [u8; 32],
    random: [u8; 32],
    transcript: Vec<u8>,
    keys: Option<Keys>,
    stage: Stage,
    user: String,
    sealed: u64,
}

/// The server's answer to one EAP response.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reply {
    Challenge(Eap),
    Accept(Eap, [u8; 32]),
    Reject(Eap),
}

impl ServerSession {
    fn request(id: u8, data: Vec<u8>) -> Reply {
        Reply::Challenge(Eap { code: EAP_REQUEST, id: id.wrapping_add(1), kind: Some(EAP_PEAP), data })
    }

    fn seal(&mut self, id: u8, inner: Eap) -> Result<Reply, String> {
        self.sealed = self.sealed.saturating_add(1);
        let iv = iv(&[&self.random[..], &self.sealed.to_be_bytes()].concat());
        let keys = self.keys.as_mut().ok_or("no tunnel yet")?;
        Ok(ServerSession::request(id, peap::peap_data(0, &[keys.seal(false, &inner.encode(), iv)])))
    }

    /// The next step for a PEAP response; `users`: the server's AAA logins.
    fn step(&mut self, server: &Device, r: &Eap, seed: &[u8]) -> Result<Reply, String> {
        if r.kind != Some(EAP_PEAP) {
            return Err(format!("EAP method {} instead of PEAP", r.kind.unwrap_or(0)));
        }
        let (_, records) = peap::parse_peap(&r.data).map_err(|e| e.to_string())?;
        let rec = records.first().ok_or("an empty PEAP response")?;
        let next_id = r.id.wrapping_add(1);
        match (self.stage, rec.kind) {
            (Stage::Started, REC_HANDSHAKE) => {
                let msgs = peap::handshakes(&rec.body).map_err(|e| e.to_string())?;
                let Some((HS_CLIENT_HELLO, ch)) = msgs.first() else { return Err("expected the client hello".into()) };
                let hello = Hello::decode(ch).map_err(|e| e.to_string())?;
                self.client_random = hello.random;
                self.random = seeded(seed);
                let secret = peap::dh_secret(&[&self.random[..], seed].concat());
                let shared = peap::dh_shared(hello.public, secret);
                if shared == 0 {
                    return Err("the host's key share is not in the group".into());
                }
                let keys = Keys::derive(shared, &self.client_random, &self.random);
                let sh = peap::handshake(
                    HS_SERVER_HELLO,
                    &Hello { random: self.random, public: peap::dh_public(secret), name: server.name.clone() }.encode(),
                );
                self.transcript = [peap::handshake(HS_CLIENT_HELLO, ch), sh.clone()].concat();
                let fin = peap::handshake(HS_FINISHED, &keys.finished(false, &self.transcript));
                self.transcript.extend_from_slice(&fin);
                self.keys = Some(keys);
                self.stage = Stage::HelloSent;
                Ok(ServerSession::request(r.id, peap::peap_data(0, &[Record { kind: REC_HANDSHAKE, body: [sh, fin].concat() }])))
            }
            (Stage::HelloSent, REC_HANDSHAKE) => {
                let msgs = peap::handshakes(&rec.body).map_err(|e| e.to_string())?;
                let Some((HS_FINISHED, fin)) = msgs.first() else { return Err("expected the client's Finished".into()) };
                let keys = self.keys.as_ref().ok_or("no tunnel")?;
                if keys.finished(true, &self.transcript) != *fin {
                    return Err("the host's Finished doesn't check out".into());
                }
                self.stage = Stage::Tunnel;
                self.seal(r.id, Eap { code: EAP_REQUEST, id: next_id, kind: Some(EAP_IDENTITY), data: Vec::new() })
            }
            (stage, REC_APP_DATA) => {
                let keys = self.keys.as_mut().ok_or("tunnel data before the handshake")?;
                let plain = keys.open(true, rec).map_err(|e| e.to_string())?;
                let inner = Eap::decode(&plain).map_err(|e| e.to_string())?;
                match (stage, inner.kind) {
                    (Stage::Tunnel, Some(EAP_IDENTITY)) => {
                        self.user = String::from_utf8_lossy(&inner.data).chars().take(64).collect();
                        self.stage = Stage::AskedPassword;
                        self.seal(r.id, Eap { code: EAP_REQUEST, id: next_id, kind: Some(EAP_GTC), data: b"Password".to_vec() })
                    }
                    (Stage::AskedPassword, Some(EAP_GTC)) => {
                        let password = String::from_utf8_lossy(&inner.data).into_owned();
                        let ok = server.services.aaa.users.iter().any(|u| u.name == self.user && u.password == password);
                        self.stage = Stage::SentResult(ok);
                        self.seal(r.id, Eap { code: EAP_REQUEST, id: next_id, kind: Some(EAP_TLV), data: peap::result_tlv(ok) })
                    }
                    (Stage::SentResult(ok), Some(EAP_TLV)) => {
                        let done = Eap { code: if ok { EAP_SUCCESS } else { EAP_FAILURE }, id: next_id, kind: None, data: Vec::new() };
                        if !ok || peap::tlv_result(&inner.data) != Some(true) {
                            return Ok(Reply::Reject(Eap { code: EAP_FAILURE, ..done }));
                        }
                        let msk = keys.msk(&self.client_random, &self.random);
                        let mut pmk = [0u8; 32];
                        pmk.copy_from_slice(msk.get(..32).unwrap_or(&[0; 32]));
                        Ok(Reply::Accept(done, pmk))
                    }
                    _ => Err("an unexpected message inside the tunnel".into()),
                }
            }
            _ => Err("an unexpected PEAP message".into()),
        }
    }
}

/// The PEAP logins an authentication server has under way, by the RADIUS State it gave.
pub(crate) type Sessions = BTreeMap<Vec<u8>, ServerSession>;

impl Sim {
    /// A RADIUS Access-Request carrying EAP at an AAA server: checked (Message-Authenticator),
    /// played through the server's side of PEAP, and answered with a challenge, an accept (with
    /// the session key) or a reject.
    pub(crate) fn peap_radius_in(&mut self, t: &Topology, d: &Device, me: Ipv4Addr, ip: &Ipv4, u: &Udp, req: &Radius) -> Result<(), String> {
        if d.kind != DeviceKind::Server || !d.services.aaa.on {
            return Err("no AAA service here".into());
        }
        let client = d
            .services
            .aaa
            .clients
            .iter()
            .find(|c| c.address == ip.src)
            .ok_or_else(|| format!("{} isn't one of this AAA server's clients", ip.src))?;
        let secret = client.secret.clone();
        // RFC 3579 §3.2: a request with EAP and no valid Message-Authenticator is discarded.
        if !req.checks_message(&req.authenticator, &secret) {
            return Err("EAP request without a valid Message-Authenticator (wrong secret?): discarded".into());
        }
        let eap = Eap::decode(&req.eap_message().unwrap_or_default()).map_err(|e| e.to_string())?;
        if eap.code != EAP_RESPONSE {
            return Err("EAP from the controller that is not a response".into());
        }
        self.seq += 1;
        let seed = format!("peap-srv{}{}{}{}", d.id.0, self.now, self.seq, ip.src).into_bytes();
        let fail = |id: u8| Reply::Reject(Eap { code: EAP_FAILURE, id: id.wrapping_add(1), kind: None, data: Vec::new() });
        let (reply, state) = if eap.kind == Some(EAP_IDENTITY) {
            // A new login: open a session, and ask the host to start the tunnel.
            let state = netlabcraft_wire::md5::md5(&seed).to_vec();
            if self.peap_srv.len() >= MAX_SESSIONS {
                self.peap_srv.pop_first();
            }
            self.peap_srv.insert(state.clone(), ServerSession { server: Some(d.id), ..ServerSession::default() });
            (ServerSession::request(eap.id, vec![FLAG_START]), Some(state))
        } else {
            let state = req.state().map(<[u8]>::to_vec).unwrap_or_default();
            match self.peap_srv.get_mut(&state).filter(|s| s.server == Some(d.id)) {
                Some(s) => match s.step(d, &eap, &seed) {
                    Ok(r) => (r, Some(state)),
                    Err(_) => (fail(eap.id), Some(state)),
                },
                None => (fail(eap.id), None),
            }
        };
        let (code, out, key) = match &reply {
            Reply::Challenge(e) => (ACCESS_CHALLENGE, e, None),
            Reply::Accept(e, k) => (ACCESS_ACCEPT, e, Some(*k)),
            Reply::Reject(e) => (ACCESS_REJECT, e, None),
        };
        if code != ACCESS_CHALLENGE
            && let Some(s) = &state
        {
            self.peap_srv.remove(s);
        }
        let salt = [seed.first().copied().unwrap_or(0), (self.seq & 0xff) as u8];
        let answer = Radius::eap_answer(
            req,
            code,
            &out.encode(),
            state.as_deref().filter(|_| code == ACCESS_CHALLENGE),
            key.as_ref().map(|k| (&k[..], salt)),
            &secret,
        );
        self.udp_out(t, d, me, ip.src, RADIUS_PORT, u.src_port, answer.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netlabcraft_model::services::AaaUser;

    fn server() -> Device {
        let mut t = Topology::new();
        let id = t.add_model(netlabcraft_model::models::find("Server").unwrap(), Some("RADIUS1"), 0.0, 0.0).unwrap();
        let mut d = t.device(id).unwrap().clone();
        d.services.aaa.on = true;
        d.services.aaa.users.push(AaaUser { name: "ana".into(), password: "ana12345".into() });
        d
    }

    /// Plays a whole login between a host's tunnel and a server session, as the controller
    /// would pass it; returns the server's last reply and every PEAP message seen.
    fn play(password: &str) -> (Reply, ClientTunnel, Vec<Vec<u8>>) {
        let d = server();
        let mut host = ClientTunnel::new(b"host");
        let mut srv = ServerSession { server: Some(d.id), ..ServerSession::default() };
        let mut wire = Vec::new();
        let mut req = ServerSession::request(1, vec![FLAG_START]);
        for n in 0..10u8 {
            let Reply::Challenge(q) = &req else { return (req, host, wire) };
            wire.push(q.data.clone());
            let data = host.answer(&q.data, "ana", password).unwrap();
            wire.push(data.clone());
            let r = Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_PEAP), data };
            req = srv.step(&d, &r, &[n]).unwrap();
        }
        panic!("the login never ended")
    }

    #[test]
    fn a_good_login_ends_in_an_accept_with_the_same_key_on_both_ends() {
        let (reply, host, wire) = play("ana12345");
        let Reply::Accept(e, key) = reply else { panic!("{reply:?}") };
        assert_eq!(e.code, EAP_SUCCESS);
        assert_eq!(host.pmk, Some(key));
        // Start, hellos, finished, then four tunnel round trips: the password never in the clear.
        assert_eq!(wire.len(), 10, "{}", wire.len());
        assert!(wire.iter().all(|m| !m.windows(8).any(|w| w == b"ana12345")));
        let shown: Vec<String> = wire.iter().map(|m| peap::describe(m)).collect();
        assert_eq!(shown[0], "start");
        assert_eq!(shown[1], "TLS handshake (client hello)");
        assert_eq!(shown[2], "TLS handshake (server hello, finished)");
        assert_eq!(shown[3], "TLS handshake (finished)");
        assert!(shown[4..].iter().all(|s| s.starts_with("TLS application data")), "{shown:?}");
    }

    #[test]
    fn a_wrong_password_ends_in_a_reject_and_no_key() {
        let (reply, host, _) = play("wrong");
        assert!(matches!(reply, Reply::Reject(Eap { code: EAP_FAILURE, .. })), "{reply:?}");
        assert_eq!(host.pmk, None);
    }

    #[test]
    fn a_tampered_handshake_or_record_is_refused() {
        let d = server();
        let mut host = ClientTunnel::new(b"h");
        let hello = host.answer(&[FLAG_START], "ana", "x").unwrap();
        let mut srv = ServerSession { server: Some(d.id), ..ServerSession::default() };
        let Reply::Challenge(q) = srv.step(&d, &Eap { code: EAP_RESPONSE, id: 1, kind: Some(EAP_PEAP), data: hello }, b"s").unwrap() else {
            panic!()
        };
        // Another host (another secret) can't finish this server's handshake.
        let mut other = ClientTunnel::new(b"other");
        other.answer(&[FLAG_START], "ana", "x").unwrap();
        assert!(other.answer(&q.data, "ana", "x").is_err());
        let mut bad = q.data.clone();
        let n = bad.len() - 1;
        bad[n] ^= 1;
        assert!(host.clone().answer(&bad, "ana", "x").is_err());
        // Out of order, a non-PEAP method, truncations: errors, never panics.
        assert!(srv.clone().step(&d, &Eap { code: EAP_RESPONSE, id: 2, kind: Some(EAP_IDENTITY), data: vec![] }, b"s").is_err());
        for k in 0..q.data.len() {
            let _ = host.clone().answer(&q.data[..k], "ana", "x");
            let _ = srv.clone().step(&d, &Eap { code: EAP_RESPONSE, id: 2, kind: Some(EAP_PEAP), data: q.data[..k].to_vec() }, b"s");
        }
    }
}
