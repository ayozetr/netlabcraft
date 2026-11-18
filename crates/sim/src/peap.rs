//! PEAP logins of WPA2-Enterprise, end to end: the host's side of the tunnel, and the
//! authentication server's, with the controller passing EAP between them inside RADIUS
//! (EAP-Message and Message-Authenticator, RFC 3579) as a pass-through authenticator.
//!
//! The tunnel is a real TLS 1.2 session ([`netlabcraft_wire::tls`], carried as
//! [`netlabcraft_wire::peap`] describes, with EAP fragmentation for the server's long flights).
//! A server with a certificate (`aaa.certificate`) presents its chain and signs its
//! Diffie-Hellman parameters with it; a host with `validate_server` checks the chain against its
//! trusted CAs (and the name), at its own clock, and refuses the login when it does not hold. A
//! server with no certificate, and a host that does not validate, use the anonymous suite as
//! before. Inside the tunnel the server asks for the identity, then the password with EAP-GTC,
//! checks them against its AAA users and says how it went with a Result TLV; the host's password
//! is only ever on the wire encrypted. The server then accepts with the EAP master session key
//! (exported from the TLS session) in an MPPE key attribute, which the controller hands to the
//! access point for the 4-way handshake, as the host derives the same key from its end.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::peap::{self, EAP_GTC, EAP_TLV, FLAG_START, KEY_LABEL};
use netlabcraft_wire::radius::{ACCESS_ACCEPT, ACCESS_CHALLENGE, ACCESS_REJECT, RADIUS_PORT, Radius};
use netlabcraft_wire::tls::{Identity, Tls, Trust};
use netlabcraft_wire::wlan::{EAP_FAILURE, EAP_IDENTITY, EAP_PEAP, EAP_REQUEST, EAP_RESPONSE, EAP_SUCCESS, Eap};
use netlabcraft_wire::{Ipv4, Udp};

use crate::Sim;

/// Most PEAP logins a server keeps under way at once.
const MAX_SESSIONS: usize = 64;

/// A TLS session in a state that is compared by whether it exists (the session itself holds
/// keys and has no equality of its own).
#[derive(Debug, Clone, Default)]
struct Session(Option<Tls>);

impl PartialEq for Session {
    fn eq(&self, other: &Session) -> bool {
        self.0.is_some() == other.0.is_some()
    }
}

impl Eq for Session {}

/// What a host's login learned of the server's certificate, for the client's details and for
/// `debug dot1x`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The server showed a certificate.
    pub presented: bool,
    pub subject: String,
    pub issuer: String,
    /// The end of its validity, Unix seconds.
    pub not_after: u64,
    /// Certificates in the chain it sent.
    pub chain_length: usize,
    /// The chain was checked against the host's trusted CAs and passed.
    pub verified: bool,
    /// Why the tunnel was refused (empty when it was not).
    pub error: String,
    /// The TLS suite the server chose.
    pub suite: String,
}

/// The pairwise master key of an established tunnel: the first 32 bytes of the exported key.
fn pmk_of(tls: &Tls) -> Option<[u8; 32]> {
    let msk = tls.export(KEY_LABEL, 64)?;
    <[u8; 32]>::try_from(msk.get(..32)?).ok()
}

/// The host's end of a PEAP tunnel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientTunnel {
    seed: Vec<u8>,
    trust: Option<TrustSpec>,
    tls: Session,
    /// Fragments of the server's flight that have arrived.
    rx: Vec<u8>,
    /// The pairwise master key the tunnel exported, once the server said the login was good.
    pub pmk: Option<[u8; 32]>,
    /// What the host saw of the server's certificate.
    pub report: Report,
}

/// What a host checks a server's certificate against (PEM and a clock, kept as text so the
/// tunnel stays comparable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustSpec {
    pub cas: Vec<String>,
    pub name: String,
    pub now: u64,
}

impl TrustSpec {
    fn trust(&self) -> Trust {
        Trust {
            anchors: self.cas.iter().filter_map(|p| netlabcraft_wire::x509::Certificate::from_pem(p).ok()).collect(),
            now: self.now,
            required: true,
            host: Some(self.name.clone()).filter(|n| !n.is_empty()),
            crls: Vec::new(),
        }
    }
}

impl ClientTunnel {
    pub fn new(seed: &[u8], trust: Option<TrustSpec>) -> ClientTunnel {
        ClientTunnel { seed: seed.to_vec(), trust, ..ClientTunnel::default() }
    }

    fn note_chain(&mut self) {
        if let Some(tls) = self.tls.0.as_ref() {
            self.report.verified = tls.verified();
            self.report.suite = tls.suite_name().to_string();
            if let Some(leaf) = tls.peer_chain().first() {
                self.report.presented = true;
                self.report.subject = leaf.subject.text();
                self.report.issuer = leaf.issuer.text();
                self.report.not_after = leaf.not_after;
                self.report.chain_length = tls.peer_chain().len();
            }
        }
    }

    /// The host's answer to the data of a PEAP request from the server.
    pub fn answer(&mut self, data: &[u8], user: &str, password: &str) -> Result<Vec<u8>, String> {
        let part = peap::parse(data).map_err(|e| e.to_string())?;
        if part.flags & FLAG_START != 0 {
            let (tls, hello) = Tls::client_with(&self.seed, self.trust.as_ref().map(TrustSpec::trust));
            self.tls = Session(Some(tls));
            self.pmk = None;
            self.rx.clear();
            self.report = Report::default();
            return Ok(peap::message(0, None, &hello));
        }
        let Some(whole) = peap::reassemble(&mut self.rx, &part).map_err(|e| e.to_string())? else { return Ok(peap::ack()) };
        let tls = self.tls.0.as_mut().ok_or("PEAP data before the start")?;
        let mut buf = whole;
        let fed = tls.feed(&mut buf);
        let out = match fed {
            Ok(o) => o,
            Err(e) => {
                self.note_chain();
                self.report.error = e.why.clone();
                return Err(format!("the PEAP tunnel was refused: {}", e.why));
            }
        };
        let tls = self.tls.0.as_mut().ok_or("no tunnel")?;
        let mut send = out.send;
        if out.peer_alert.is_some() || out.closed {
            return Err("a TLS alert from the server".into());
        }
        if !out.app.is_empty() {
            let q = Eap::decode(&out.app).map_err(|e| e.to_string())?;
            if q.code != EAP_REQUEST {
                return Err("inside the tunnel: not an EAP request".into());
            }
            let mut finished_ok = false;
            let reply = match q.kind {
                Some(EAP_IDENTITY) => Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_IDENTITY), data: user.as_bytes().to_vec() },
                Some(EAP_GTC) => Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_GTC), data: password.as_bytes().to_vec() },
                Some(EAP_TLV) => {
                    finished_ok = peap::tlv_result(&q.data).ok_or("a TLV request without a result")?;
                    Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_TLV), data: peap::result_tlv(finished_ok) }
                }
                other => return Err(format!("inside the tunnel: EAP method {} not supported", other.unwrap_or(0))),
            };
            if finished_ok {
                self.pmk = pmk_of(tls);
            }
            send.extend(tls.send(&reply.encode()));
        }
        self.note_chain();
        Ok(peap::message(0, None, &send))
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
#[derive(Debug, Clone, Default)]
pub(crate) struct ServerSession {
    server: Option<netlabcraft_core::DeviceId>,
    tls: Session,
    /// The host's flight as far as it has arrived.
    rx: Vec<u8>,
    /// Messages of the server's own long flight still to send, one per acknowledgement.
    queue: Vec<Vec<u8>>,
    stage: Stage,
    user: String,
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

    /// A TLS flight as the first message, the rest waiting for the host's acknowledgements.
    fn flight(&mut self, id: u8, tls: &[u8]) -> Reply {
        let mut parts = peap::fragments(tls);
        let first = parts.remove(0);
        self.queue = parts;
        ServerSession::request(id, first)
    }

    /// The next step for a PEAP response; `identity`: the server's certificate, if it has one.
    fn step(&mut self, server: &Device, r: &Eap, seed: &[u8], identity: Option<Identity>) -> Result<Reply, String> {
        if r.kind != Some(EAP_PEAP) {
            return Err(format!("EAP method {} instead of PEAP", r.kind.unwrap_or(0)));
        }
        let part = peap::parse(&r.data).map_err(|e| e.to_string())?;
        // An acknowledgement of a fragment: the next one goes.
        if part.tls.is_empty() && !self.queue.is_empty() {
            let next = self.queue.remove(0);
            return Ok(ServerSession::request(r.id, next));
        }
        let Some(whole) = peap::reassemble(&mut self.rx, &part).map_err(|e| e.to_string())? else {
            return Ok(ServerSession::request(r.id, peap::ack()));
        };
        let next_id = r.id.wrapping_add(1);
        if self.stage == Stage::Started {
            self.tls = Session(Some(Tls::server_with(seed, identity)));
        }
        let tls = self.tls.0.as_mut().ok_or("no tunnel")?;
        let mut buf = whole;
        let out = tls.feed(&mut buf).map_err(|e| e.why)?;
        if out.peer_alert.is_some() || out.closed {
            return Err("a TLS alert from the host".into());
        }
        let mut send = out.send;
        let seal = |tls: &mut Tls, send: &mut Vec<u8>, inner: Eap| send.extend(tls.send(&inner.encode()));
        match self.stage {
            Stage::Started => {
                self.stage = Stage::HelloSent;
                Ok(self.flight(r.id, &send))
            }
            Stage::HelloSent => {
                if !out.established {
                    return Err("the host's handshake did not finish".into());
                }
                self.stage = Stage::Tunnel;
                seal(tls, &mut send, Eap { code: EAP_REQUEST, id: next_id, kind: Some(EAP_IDENTITY), data: Vec::new() });
                Ok(self.flight(r.id, &send))
            }
            stage => {
                let inner = Eap::decode(&out.app).map_err(|e| e.to_string())?;
                match (stage, inner.kind) {
                    (Stage::Tunnel, Some(EAP_IDENTITY)) => {
                        self.user = String::from_utf8_lossy(&inner.data).chars().take(64).collect();
                        self.stage = Stage::AskedPassword;
                        seal(tls, &mut send, Eap { code: EAP_REQUEST, id: next_id, kind: Some(EAP_GTC), data: b"Password".to_vec() });
                        Ok(self.flight(r.id, &send))
                    }
                    (Stage::AskedPassword, Some(EAP_GTC)) => {
                        let password = String::from_utf8_lossy(&inner.data).into_owned();
                        let ok = server.services.aaa.users.iter().any(|u| u.name == self.user && u.password == password);
                        self.stage = Stage::SentResult(ok);
                        seal(tls, &mut send, Eap { code: EAP_REQUEST, id: next_id, kind: Some(EAP_TLV), data: peap::result_tlv(ok) });
                        Ok(self.flight(r.id, &send))
                    }
                    (Stage::SentResult(ok), Some(EAP_TLV)) => {
                        let done = Eap { code: if ok { EAP_SUCCESS } else { EAP_FAILURE }, id: next_id, kind: None, data: Vec::new() };
                        if !ok || peap::tlv_result(&inner.data) != Some(true) {
                            return Ok(Reply::Reject(Eap { code: EAP_FAILURE, ..done }));
                        }
                        let pmk = pmk_of(tls).ok_or("no key to export")?;
                        Ok(Reply::Accept(done, pmk))
                    }
                    _ => Err("an unexpected message inside the tunnel".into()),
                }
            }
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
            (ServerSession::request(eap.id, peap::start()), Some(state))
        } else {
            let state = req.state().map(<[u8]>::to_vec).unwrap_or_default();
            match self.peap_srv.get_mut(&state).filter(|s| s.server == Some(d.id)) {
                Some(s) => match s.step(d, &eap, &seed, crate::pki::aaa_identity(d)) {
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
    use netlabcraft_wire::rsa::RsaKey;
    use netlabcraft_wire::tls::Drbg;
    use netlabcraft_wire::x509::{Certificate, Name};

    const NOW: i64 = 1_800_000_000;

    fn server() -> Device {
        let mut t = Topology::new();
        let id = t.add_model(netlabcraft_model::models::find("Server").unwrap(), Some("RADIUS1"), 0.0, 0.0).unwrap();
        let mut d = t.device(id).unwrap().clone();
        d.services.aaa.on = true;
        d.services.aaa.users.push(AaaUser { name: "ana".into(), password: "ana12345".into() });
        d
    }

    /// A throwaway server certificate for `cn`, issued by itself as a CA, with its key.
    fn identity(cn: &str) -> (Identity, Certificate) {
        let key = RsaKey::generate(1024, &mut Drbg::new(format!("peap test {cn}").as_bytes())).unwrap();
        let name = Name::new(cn, "NetLab");
        let cert = crate::pki::issue(&name, &key, &name, &key, &[1], NOW, 30, true).unwrap();
        (Identity { chain: vec![cert.der().to_vec()], key }, cert)
    }

    fn trusting(cert: &Certificate, name: &str) -> Option<TrustSpec> {
        Some(TrustSpec { cas: vec![cert.to_pem()], name: name.into(), now: u64::try_from(NOW).unwrap() })
    }

    /// Plays a whole login between a host's tunnel and a server session, as the controller
    /// would pass it; returns how it ended, the host's tunnel and every PEAP message seen.
    fn play(password: &str, id: Option<Identity>, trust: Option<TrustSpec>) -> (Result<Reply, String>, ClientTunnel, Vec<Vec<u8>>) {
        let d = server();
        let mut host = ClientTunnel::new(b"host", trust);
        let mut srv = ServerSession { server: Some(d.id), ..ServerSession::default() };
        let mut wire = Vec::new();
        let mut req = ServerSession::request(1, peap::start());
        for n in 0..60u8 {
            let Reply::Challenge(q) = &req else { return (Ok(req), host, wire) };
            wire.push(q.data.clone());
            let data = match host.answer(&q.data, "ana", password) {
                Ok(d) => d,
                Err(e) => return (Err(e), host, wire),
            };
            wire.push(data.clone());
            let r = Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_PEAP), data };
            req = match srv.step(&d, &r, &[n], id.clone()) {
                Ok(r) => r,
                Err(e) => return (Err(e), host, wire),
            };
        }
        panic!("the login never ended")
    }

    #[test]
    fn a_good_login_without_certificates_ends_in_an_accept_with_the_same_key_on_both_ends() {
        let (reply, host, wire) = play("ana12345", None, None);
        let Ok(Reply::Accept(e, key)) = reply else { panic!("{reply:?}") };
        assert_eq!(e.code, EAP_SUCCESS);
        assert_eq!(host.pmk, Some(key));
        assert!(wire.iter().all(|m| !m.windows(8).any(|w| w == b"ana12345")));
        assert_eq!(peap::describe(&wire[0]), "start");
        assert!(peap::describe(&wire[1]).contains("ClientHello"), "{}", peap::describe(&wire[1]));
        assert!(!host.report.presented && !host.report.verified);
        assert!(host.report.suite.contains("anon"), "{}", host.report.suite);
    }

    #[test]
    fn a_server_certificate_is_presented_in_fragments_and_a_host_that_trusts_it_logs_in() {
        let (id, cert) = identity("radius.lab.test");
        let (reply, host, wire) = play("ana12345", Some(id), trusting(&cert, "radius.lab.test"));
        let Ok(Reply::Accept(_, key)) = reply else { panic!("{reply:?}") };
        assert_eq!(host.pmk, Some(key));
        assert!(host.report.presented && host.report.verified, "{:?}", host.report);
        assert_eq!(host.report.subject, "CN=radius.lab.test, O=NetLab");
        assert!(host.report.suite.contains("RSA"));
        // The server's first flight is longer than one message: it is cut, and each piece but
        // the last is acknowledged with an empty response.
        let more = wire.iter().filter(|m| m.first().is_some_and(|f| f & peap::FLAG_MORE != 0)).count();
        assert!(more >= 1, "{} messages", wire.len());
        assert!(wire.iter().any(|m| m.as_slice() == peap::ack().as_slice()));
        let all = wire.concat();
        assert!(all.windows(15).any(|w| w == b"radius.lab.test"), "the certificate is on the wire, in the clear, as in any TLS handshake");
    }

    #[test]
    fn a_host_that_validates_refuses_a_server_it_does_not_trust_or_that_has_another_name() {
        let (id, _cert) = identity("radius.lab.test");
        let (_, other) = identity("somebody.else");
        let (reply, host, _) = play("ana12345", Some(id.clone()), trusting(&other, ""));
        assert!(reply.is_err(), "{reply:?}");
        assert_eq!(host.pmk, None);
        assert!(!host.report.verified && !host.report.error.is_empty(), "{:?}", host.report);
        // Trusted, but not for the name the host expects.
        let (id, cert) = identity("radius.lab.test");
        let (reply, host, _) = play("ana12345", Some(id), trusting(&cert, "wrong.name.test"));
        assert!(reply.is_err() && host.pmk.is_none() && !host.report.error.is_empty(), "{reply:?} {:?}", host.report);
    }

    #[test]
    fn a_host_that_validates_gets_nowhere_with_a_server_that_has_no_certificate() {
        let (_, cert) = identity("radius.lab.test");
        let (reply, host, _) = play("ana12345", None, trusting(&cert, ""));
        assert!(reply.is_err(), "{reply:?}");
        assert_eq!(host.pmk, None);
    }

    #[test]
    fn a_wrong_password_ends_in_a_reject_and_no_key() {
        let (id, cert) = identity("radius.lab.test");
        let (reply, host, _) = play("wrong", Some(id), trusting(&cert, ""));
        assert!(matches!(reply, Ok(Reply::Reject(Eap { code: EAP_FAILURE, .. }))), "{reply:?}");
        assert_eq!(host.pmk, None);
    }

    #[test]
    fn hostile_and_out_of_order_messages_are_errors_never_panics() {
        let d = server();
        let mut host = ClientTunnel::new(b"h", None);
        let hello = host.answer(&peap::start(), "ana", "x").unwrap();
        let mut srv = ServerSession { server: Some(d.id), ..ServerSession::default() };
        let Reply::Challenge(q) = srv.step(&d, &Eap { code: EAP_RESPONSE, id: 1, kind: Some(EAP_PEAP), data: hello.clone() }, b"s", None).unwrap()
        else {
            panic!()
        };
        // Not PEAP, and nothing before the start.
        assert!(srv.clone().step(&d, &Eap { code: EAP_RESPONSE, id: 2, kind: Some(EAP_IDENTITY), data: vec![] }, b"s", None).is_err());
        assert!(ClientTunnel::new(b"x", None).answer(&q.data, "ana", "x").is_err());
        for k in 0..q.data.len().min(60) {
            let _ = host.clone().answer(&q.data[..k], "ana", "x");
            let _ = srv.clone().step(
                &d,
                &Eap { code: EAP_RESPONSE, id: 2, kind: Some(EAP_PEAP), data: hello[..k.min(hello.len())].to_vec() },
                b"s",
                None,
            );
        }
        let mut bad = q.data.clone();
        let n = bad.len() - 1;
        bad[n] ^= 1;
        let _ = host.clone().answer(&bad, "ana", "x");
    }
}
