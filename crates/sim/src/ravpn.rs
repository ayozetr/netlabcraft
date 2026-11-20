//! Remote-access VPN: a PC's VPN Client against a router set up as a remote-access (Easy VPN
//! style) concentrator.
//!
//! The router side is the configuration of `netlabcraft_model::vpn::RemoteAccess` and the
//! crypto map entries that point at a `crypto dynamic-map`: `crypto isakmp client configuration
//! group`, `ip local pool`, `aaa authentication login LIST local` and the crypto map's `client
//! authentication list`, `isakmp authorization list` and `dynamic` entry, applied to the
//! interface the clients reach.
//!
//! The exchange is **our own, much simpler protocol, not IKE with Xauth and mode-config**: the
//! client sends one UDP datagram to port 10000 of the concentrator's address and gets one back.
//! The request names the group and the user, carries the password hidden under a stream derived
//! from the group key and a nonce, and is signed with the group key (HMAC-SHA-256), so a wrong
//! group name or key is refused and the router never sees the password of a client that does not
//! know the key. The router checks the group, the user against its local `username` list (when
//! the crypto map has a client authentication list), takes the first free address of the
//! group's pool, picks the first transform set of the dynamic map, and answers with the address,
//! its SPI and the transform set, signed with the group key. Both ends then derive the keys of
//! both directions from the group key and the two nonces (an HMAC-SHA-256 expansion) and the
//! data path is real ESP (RFC 4303, the same algorithms the site-to-site IPsec uses, chosen by
//! the transform set): the client seals every packet its VPN address sends or that goes beyond
//! its own subnet and sends it to the concentrator; the router opens it, routes the inner
//! packet, and seals the packets for the client's address back. What differs from IKE is
//! written in `docs/decisions.md`.
//!
//! Sessions are dynamic state (they are not saved with the lab): a PC holds at most one, a
//! router one per client address, to a bound.

use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::acl::AclAddr;
use netlabcraft_model::vpn::TransformSet;
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::sha256::{hmac_sha256, sha256};
use netlabcraft_wire::vpn::{EspEncr, EspSuite, NEXT_IPV4};
use netlabcraft_wire::{Ipv4, Udp};

use crate::{DevState, L3If, Sim, SimError};

/// The UDP port a concentrator listens on (the port Cisco's IPsec over UDP uses).
pub const RA_PORT: u16 = 10_000;
/// Most clients one router serves at a time.
pub const MAX_RA_SESSIONS: usize = 256;
/// Longest text field in a message, in bytes.
pub const MAX_RA_FIELD: usize = 200;
const MAGIC: &[u8; 4] = b"NLRA";
const T_REQUEST: u8 = 1;
const T_REPLY: u8 = 2;
const T_DELETE: u8 = 3;
/// Most split-tunnel networks the concentrator pushes to a client.
pub const MAX_SPLIT: usize = 32;
/// How long the client waits for an answer, simulated µs.
const WAIT_US: u64 = 2_000_000;

/// A remote-access session as one end holds it: on a PC the one with its concentrator, on a
/// router one of its clients.
#[derive(Debug, Clone)]
pub struct RaSa {
    /// The other end's address (the concentrator on a PC, the client on a router).
    pub peer: Ipv4Addr,
    /// This end's own outer address.
    pub local: Ipv4Addr,
    /// The client's address inside the VPN, given by the group's pool.
    pub assigned: Ipv4Addr,
    pub group: String,
    pub user: String,
    /// The router's interface the client reached (empty on a PC).
    pub iface: String,
    /// The ESP algorithms (`None`: the transform set has no cipher, the stand-in cipher runs).
    pub suite: Option<EspSuite>,
    /// The transform set's transforms, as text.
    pub transforms: String,
    pub spi_in: u32,
    pub spi_out: u32,
    /// The split-tunnel networks the concentrator pushed: only these go through the tunnel
    /// (`None`: a full tunnel). Empty on a router.
    pub split: Option<Vec<AclAddr>>,
    /// The name server pushed to the client.
    pub dns: Option<Ipv4Addr>,
    /// The DNS domain pushed to the client (empty: none).
    pub domain: String,
    key_in: Vec<u8>,
    key_out: Vec<u8>,
    /// When the session came up, simulated µs.
    pub since_us: u64,
    pub encaps: u64,
    pub decaps: u64,
    pub drops: u64,
}

/// What `ra_connect` needs: the form of the VPN Client.
pub struct RaParams<'a> {
    pub server: Ipv4Addr,
    pub group: &'a str,
    pub key: &'a str,
    pub user: &'a str,
    pub password: &'a str,
}

// ---- encoding ----

/// Reads fields of a message in order; every read is `None` past the end.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u32(&mut self) -> Option<u32> {
        let a: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(u32::from_be_bytes(a))
    }

    fn array16(&mut self) -> Option<[u8; 16]> {
        self.take(16)?.try_into().ok()
    }

    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = usize::from(self.u8()?);
        Some(self.take(n)?.to_vec())
    }

    fn text(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
}

/// A length-prefixed field (at most 255 bytes; callers keep them below [`MAX_RA_FIELD`]).
fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    let n = b.len().min(255);
    out.push(u8::try_from(n).unwrap_or(u8::MAX));
    out.extend_from_slice(b.get(..n).unwrap_or_default());
}

fn put_text(out: &mut Vec<u8>, t: &str) {
    put_bytes(out, t.as_bytes());
}

/// The first 16 bytes of SHA-256 over the parts joined.
fn digest16(parts: &[&[u8]]) -> [u8; 16] {
    sha256(&parts.concat()).first_chunk::<16>().copied().unwrap_or_default()
}

/// An SPI no lower than 256 (the values below are reserved) derived from a seed.
fn spi_from(tag: &[u8], seed: &[u8]) -> u32 {
    let h = sha256(&[tag, seed].concat());
    u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 0x7fff_0000 + 256
}

/// `len` bytes derived from a key and a seed: HMAC-SHA-256 blocks over the seed and a counter.
fn expand(key: &[u8], seed: &[u8], len: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(len.saturating_add(32));
    let mut counter: u8 = 1;
    while out.len() < len {
        let mut data = seed.to_vec();
        data.push(counter);
        out.extend_from_slice(&hmac_sha256(key, &data));
        counter = counter.wrapping_add(1);
    }
    out.truncate(len);
    out
}

/// The keys of both directions: (client to router, router to client). One direction takes what
/// the suite needs (36 bytes for the stand-in cipher).
fn session_keys(group_key: &[u8], ni: &[u8], nr: &[u8], suite: Option<EspSuite>) -> (Vec<u8>, Vec<u8>) {
    let one = suite.map_or(36, EspSuite::keymat_len);
    let m = expand(group_key, &[b"nlra-keys".as_slice(), ni, nr].concat(), one.saturating_mul(2));
    (m.get(..one).unwrap_or_default().to_vec(), m.get(one..).unwrap_or_default().to_vec())
}

/// The password hidden (or shown again) with a stream from the group key and the nonce.
fn hide(key: &[u8], ni: &[u8], text: &[u8]) -> Vec<u8> {
    let mask = expand(key, &[b"nlra-pw".as_slice(), ni].concat(), text.len());
    text.iter().zip(mask.iter()).map(|(a, b)| a ^ b).collect()
}

fn sign(key: &[u8], tag: &[u8], body: &[u8]) -> [u8; 32] {
    hmac_sha256(key, &[tag, body].concat())
}

/// A request as the router reads it.
struct Request {
    ni: [u8; 16],
    spi_c: u32,
    group: String,
    user: String,
    hidden: Vec<u8>,
    /// The bytes the proof signs.
    body: Vec<u8>,
    proof: Vec<u8>,
}

fn parse_request(b: &[u8]) -> Option<Request> {
    let mut r = Reader { b, at: 0 };
    if r.take(4)? != MAGIC.as_slice() || r.u8()? != T_REQUEST {
        return None;
    }
    let ni = r.array16()?;
    let spi_c = r.u32()?;
    let group = r.text()?;
    let user = r.text()?;
    let hidden = r.bytes()?;
    let body = b.get(..r.at)?.to_vec();
    let proof = r.take(32)?.to_vec();
    if r.at != b.len() {
        return None;
    }
    Some(Request { ni, spi_c, group, user, hidden, body, proof })
}

/// A refusal: magic, reply, the status, the request's nonce and a reason.
fn refusal(status: u8, ni: &[u8; 16], why: &str) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.push(T_REPLY);
    out.push(status);
    out.extend_from_slice(ni);
    put_text(&mut out, why);
    out
}

/// The slot holding a session: a router's by client address, a PC's one.
fn slot(st: &mut DevState, router: bool, peer: Ipv4Addr) -> Option<&mut RaSa> {
    if router { st.ra_sessions.get_mut(&peer) } else { st.ra_client.as_mut() }
}

/// Whether a PC's session sends packets for `dst` through the tunnel: everything beyond its own
/// subnets (a full tunnel), except the concentrator itself.
fn tunnels(d: &Device, sa: &RaSa, dst: Ipv4Addr) -> bool {
    dst != sa.peer
        && dst != sa.assigned
        && !dst.is_loopback()
        && !dst.is_multicast()
        && !dst.is_broadcast()
        && !dst.is_unspecified()
        && !crate::l3_ifaces(d).iter().any(|i| i.net.contains(dst))
        && sa.split.as_ref().is_none_or(|nets| nets.iter().any(|n| n.matches(dst)))
}

/// What a router decided for a request: the session to keep and the answer to send.
struct Decision {
    session: RaSa,
    reply: Vec<u8>,
}

impl Sim {
    /// The session a PC's VPN Client holds, if connected.
    pub fn ra_client(&self, d: DeviceId) -> Option<&RaSa> {
        self.state.get(&d)?.ra_client.as_ref()
    }

    /// The clients a router serves, by outer address.
    pub fn ra_sessions(&self, d: DeviceId) -> Vec<RaSa> {
        self.state.get(&d).map(|s| s.ra_sessions.values().cloned().collect()).unwrap_or_default()
    }

    /// The VPN address a packet to `dst` leaves a PC with, when its VPN Client is connected and
    /// the packet goes through the tunnel (`source_for`).
    pub(crate) fn ra_source(&self, d: &Device, dst: Ipv4Addr) -> Option<Ipv4Addr> {
        let sa = self.state.get(&d.id)?.ra_client.as_ref()?;
        tunnels(d, sa, dst).then_some(sa.assigned)
    }

    /// Whether `a` is the VPN address of a PC's session (it takes packets for it).
    pub(crate) fn ra_owns(&self, d: &Device, a: Ipv4Addr) -> bool {
        self.state.get(&d.id).and_then(|s| s.ra_client.as_ref()).is_some_and(|c| c.assigned == a)
    }

    /// A datagram this device originates: sealed into the tunnel when a PC's VPN address
    /// sends it, or when a router sends it to a client's VPN address. `None`: not for a tunnel.
    pub(crate) fn ra_out(&mut self, t: &Topology, d: &Device, ip: &Ipv4, df: bool) -> Option<Result<(), String>> {
        let st = self.state.get(&d.id)?;
        let sa = if d.kind == DeviceKind::Router {
            st.ra_sessions.values().find(|s| s.assigned == ip.dst)?.clone()
        } else {
            let c = st.ra_client.as_ref()?;
            if ip.src != c.assigned {
                return None;
            }
            c.clone()
        };
        Some(self.ra_seal_out(t, d, &sa, ip, df))
    }

    /// A router forwarding a packet for a client's VPN address: sealed to the client.
    pub(crate) fn ra_forward(&mut self, t: &Topology, d: &Device, ip: &Ipv4) -> Option<Result<(), String>> {
        let sa = self.state.get(&d.id)?.ra_sessions.values().find(|s| s.assigned == ip.dst)?.clone();
        Some(self.ra_seal_out(t, d, &sa, ip, true))
    }

    fn ra_seal_out(&mut self, t: &Topology, d: &Device, sa: &RaSa, ip: &Ipv4, df: bool) -> Result<(), String> {
        let mut raw = ip.encode();
        if !df {
            netlabcraft_wire::ipfrag::set_dont_fragment(&mut raw, false);
        }
        let router = d.kind == DeviceKind::Router;
        let seq = {
            let st = self.st(d.id);
            let Some(s) = slot(st, router, sa.peer) else { return Err("VPN: the session is gone".into()) };
            s.encaps = s.encaps.saturating_add(1);
            u32::try_from(s.encaps).unwrap_or(u32::MAX)
        };
        let id = self.ip_id();
        let esp = crate::ikev2::ipsec_seal(sa.suite, &sa.key_out, sa.spi_out, seq, id, sa.local, sa.peer, &raw);
        let (out, nh) = self.route_flow(t, d, sa.local, sa.peer).ok_or_else(|| format!("VPN: no route to {}", sa.peer))?;
        self.transmit(t, d, &out, nh, esp)
    }

    /// An ESP packet from a client of this router; `None` when the sender is not one.
    pub(crate) fn ra_server_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4) -> Option<Result<(), String>> {
        let sa = self.state.get(&d.id)?.ra_sessions.get(&ip.src)?.clone();
        Some(self.ra_open_in(t, d, ifc, ip, &sa))
    }

    /// An ESP packet for a PC's session; `None` when it has none.
    pub(crate) fn ra_client_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4) -> Option<Result<(), String>> {
        let sa = self.state.get(&d.id)?.ra_client.as_ref()?.clone();
        if ip.src != sa.peer {
            return None;
        }
        Some(self.ra_open_in(t, d, ifc, ip, &sa))
    }

    fn ra_open_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4, sa: &RaSa) -> Result<(), String> {
        let router = d.kind == DeviceKind::Router;
        let spi = crate::ikev2::inbound_spi(ip).map_err(|e| e.to_string())?;
        let opened = if spi != sa.spi_in {
            Err(format!("VPN: ESP SPI 0x{spi:08x} from {} matches no remote-access session", ip.src))
        } else {
            match crate::ikev2::ipsec_open(sa.suite, &sa.key_in, ip) {
                Ok((NEXT_IPV4, inner)) => Ok(inner),
                Ok((next, _)) => Err(format!("VPN: ESP carrying next header {next} is not understood")),
                Err(_) => Err(format!("VPN: ESP from {} failed its integrity check", ip.src)),
            }
        };
        // A router takes only packets that come from the address it gave the client.
        let checked = opened.and_then(|inner| match Ipv4::decode(&inner) {
            Ok(i) if router && i.src != sa.assigned => {
                Err(format!("VPN: {} sent a packet from {}, not from its VPN address {}", ip.src, i.src, sa.assigned))
            }
            Ok(_) => Ok(inner),
            Err(e) => Err(e.to_string()),
        });
        if let Some(s) = slot(self.st(d.id), router, sa.peer) {
            match &checked {
                Ok(_) => s.decaps = s.decaps.saturating_add(1),
                Err(_) => s.drops = s.drops.saturating_add(1),
            }
        }
        match checked {
            Ok(inner) => self.ip_in(t, d, ifc, &inner),
            Err(e) => Err(e),
        }
    }

    // ---- the router's side of the exchange ----

    /// UDP for remote-access VPN: a client's request or delete on a router, an answer on a PC
    /// waiting for one. `None` when the datagram is not one.
    pub(crate) fn ra_udp_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4, u: &Udp) -> Option<Result<(), String>> {
        if !u.payload.starts_with(MAGIC) {
            return None;
        }
        if d.kind == DeviceKind::Router && u.dst_port == RA_PORT {
            return Some(match u.payload.get(4).copied() {
                Some(T_REQUEST) => self.ra_server_request(t, d, ifc, ip, u),
                Some(T_DELETE) => self.ra_server_delete(d, ip, &u.payload),
                _ => Err("remote-access VPN: not a request".into()),
            });
        }
        if d.kind.is_host() && u.src_port == RA_PORT && self.state.get(&d.id).is_some_and(|s| s.ra_port == Some(u.dst_port)) {
            let st = self.st(d.id);
            if st.ra_inbox.len() < 8 {
                st.ra_inbox.push(u.payload.clone());
            }
            return Some(Ok(()));
        }
        None
    }

    fn ra_server_request(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4, u: &Udp) -> Result<(), String> {
        let req = parse_request(&u.payload).ok_or("remote-access VPN: a damaged request")?;
        let now = self.now;
        let reply = match self.ra_decide(d, ifc, ip, &req, now) {
            Err((status, why)) => {
                self.log(d.id, &format!("remote-access VPN: the request from {} (group {}) was refused: {why}", ip.src, req.group));
                refusal(status, &req.ni, &why)
            }
            Ok(Decision { session, reply }) => {
                self.log(
                    d.id,
                    &format!(
                        "remote-access VPN: {} from {} joined group {} and was given {}",
                        if session.user.is_empty() { "a client" } else { session.user.as_str() },
                        ip.src,
                        session.group,
                        session.assigned
                    ),
                );
                self.st(d.id).ra_sessions.insert(ip.src, session);
                reply
            }
        };
        self.udp_out(t, d, ip.dst, ip.src, RA_PORT, u.src_port, reply)
    }

    /// Whether a request is accepted and what to answer: a status and reason when it is not.
    fn ra_decide(&self, d: &Device, ifc: &L3If, ip: &Ipv4, req: &Request, now: u64) -> Result<Decision, (u8, String)> {
        let crypto = &d.crypto;
        let map = crypto
            .applied
            .iter()
            .find(|(n, _)| crate::vpn::same_name(n, &ifc.name))
            .map(|(_, m)| m.clone())
            .ok_or_else(|| (4u8, format!("no crypto map is applied to {}", ifc.name)))?;
        let dynamic = crypto.map(&map).find_map(|e| e.dynamic_ref.clone()).ok_or_else(|| {
            (4u8, format!("crypto map {map} has no entry that uses a dynamic map (`crypto map {map} 10 ipsec-isakmp dynamic NAME`)"))
        })?;
        let ts_name = crypto
            .map(&dynamic)
            .find(|e| e.template)
            .and_then(|e| e.transform_set.clone())
            .ok_or_else(|| (4u8, format!("dynamic map {dynamic} has no transform set")))?;
        let ts = crypto.transform_set(&ts_name).ok_or_else(|| (4u8, format!("transform set {ts_name} does not exist")))?;
        let suite = crate::vpn::esp_suite(ts);
        if suite.is_some_and(|s| s.encr == EspEncr::Ah) || crate::vpn::bundle_ah(ts).is_some() {
            return Err((4u8, "remote-access clients take ESP only; the transform set has AH".into()));
        }
        // The group and its key: a wrong name or key gets the same answer.
        let group = crypto.ra.group(&req.group).filter(|g| !g.key.is_empty());
        let key_ok = group.is_some_and(|g| sign(g.key.as_bytes(), b"nlra-req", &req.body).as_slice() == req.proof.as_slice());
        let (Some(group), true) = (group, key_ok) else {
            return Err((1u8, "the group name or key was not accepted".into()));
        };
        let authz = crypto
            .ra
            .isakmp_authz
            .get(&map)
            .ok_or_else(|| (4u8, format!("crypto map {map} has no `isakmp authorization list`: nothing hands the group's address out")))?;
        if !crypto.ra.authz_lists.contains(authz) {
            return Err((4u8, format!("authorization list {authz} is not defined (`aaa authorization network {authz} local`)")));
        }
        if let Some(list) = crypto.ra.client_auth.get(&map) {
            if !crypto.ra.login_lists.contains(list) {
                return Err((2u8, format!("authentication list {list} is not defined (`aaa authentication login {list} local`)")));
            }
            let password = String::from_utf8_lossy(&hide(group.key.as_bytes(), &req.ni, &req.hidden)).into_owned();
            if !d.access.users.iter().any(|u| u.name == req.user && u.matches(&password)) {
                return Err((2u8, "the user name or password was refused".into()));
            }
        }
        let pool = group
            .pool
            .as_deref()
            .and_then(|p| crypto.ra.pool(p))
            .ok_or_else(|| (3u8, format!("group {} has no address pool (`pool NAME`) or the pool does not exist", group.name)))?;
        let st = self.state.get(&d.id);
        let in_use = |a: &Ipv4Addr| {
            st.is_some_and(|s| s.ra_sessions.iter().any(|(client, x)| *client != ip.src && x.assigned == *a))
                || crate::l3_ifaces(d).iter().any(|i| i.net.addr == *a)
        };
        let assigned = pool.addresses().find(|a| !in_use(a)).ok_or_else(|| (3u8, format!("the address pool {} has no free address", pool.name)))?;
        if st.is_some_and(|s| s.ra_sessions.len() >= MAX_RA_SESSIONS && !s.ra_sessions.contains_key(&ip.src)) {
            return Err((3u8, "this router serves as many VPN clients as it can".into()));
        }
        // The answer: the nonce of the router, the SPI it expects, the address and the transforms.
        let nr = digest16(&[b"nlra-nr".as_slice(), &req.ni, &ip.src.octets(), &ip.dst.octets()]);
        let spi_s = spi_from(b"nlra-spi-s", &req.ni);
        let (c2s, s2c) = session_keys(group.key.as_bytes(), &req.ni, &nr, suite);
        let transforms = ts.transforms.join(",");
        let split = group
            .acl
            .as_deref()
            .and_then(|name| d.acls.iter().find(|a| a.name == name))
            .map(|acl| acl.entries.iter().filter(|e| e.permit).map(|e| e.src).take(MAX_SPLIT).collect::<Vec<AclAddr>>());
        let mut reply = MAGIC.to_vec();
        reply.push(T_REPLY);
        reply.push(0);
        reply.extend_from_slice(&req.ni);
        reply.extend_from_slice(&nr);
        reply.extend_from_slice(&spi_s.to_be_bytes());
        reply.extend_from_slice(&assigned.octets());
        put_text(&mut reply, &transforms);
        reply.extend_from_slice(&group.dns.unwrap_or(Ipv4Addr::UNSPECIFIED).octets());
        put_text(&mut reply, &group.domain);
        match &split {
            Some(nets) => {
                reply.push(1);
                reply.push(u8::try_from(nets.len()).unwrap_or(u8::MAX));
                for n in nets {
                    reply.extend_from_slice(&n.addr.octets());
                    reply.extend_from_slice(&n.wildcard.octets());
                }
            }
            None => reply.push(0),
        }
        let proof = sign(group.key.as_bytes(), b"nlra-rsp", &reply);
        reply.extend_from_slice(&proof);
        let session = RaSa {
            peer: ip.src,
            local: ip.dst,
            assigned,
            group: group.name.clone(),
            user: req.user.clone(),
            iface: ifc.name.clone(),
            suite,
            transforms,
            spi_in: spi_s,
            spi_out: req.spi_c,
            split: None,
            dns: None,
            domain: String::new(),
            key_in: c2s,
            key_out: s2c,
            since_us: now,
            encaps: 0,
            decaps: 0,
            drops: 0,
        };
        Ok(Decision { session, reply })
    }

    fn ra_server_delete(&mut self, d: &Device, ip: &Ipv4, payload: &[u8]) -> Result<(), String> {
        let sa = self.state.get(&d.id).and_then(|s| s.ra_sessions.get(&ip.src)).cloned().ok_or("remote-access VPN: no session to end")?;
        let mut r = Reader { b: payload, at: 5 };
        let spi = r.u32().ok_or("remote-access VPN: a damaged message")?;
        let body = payload.get(..r.at).ok_or("remote-access VPN: a damaged message")?;
        let proof = r.take(32).ok_or("remote-access VPN: a damaged message")?;
        if spi != sa.spi_in || sign(&sa.key_in, b"nlra-del", body).as_slice() != proof {
            return Err(format!("remote-access VPN: the request to end the session of {} was not accepted", ip.src));
        }
        self.st(d.id).ra_sessions.remove(&ip.src);
        self.log(d.id, &format!("remote-access VPN: {} ended its session", ip.src));
        Ok(())
    }

    // ---- the PC's side of the exchange ----

    /// Connects a PC's VPN Client: sends the request, waits for the answer, checks it and keeps
    /// the session. The error says what went wrong in terms a person can act on.
    pub fn ra_connect(&mut self, t: &Topology, dev: DeviceId, p: &RaParams<'_>) -> Result<(), SimError> {
        let fail = |m: String| SimError::NoSource(m);
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?.clone();
        if self.ra_client(dev).is_some() {
            return Err(fail("the VPN Client is already connected: disconnect first".into()));
        }
        for text in [p.group, p.key, p.user, p.password] {
            if text.len() > MAX_RA_FIELD {
                return Err(fail(format!("the group, key, user name and password take at most {MAX_RA_FIELD} bytes each")));
            }
        }
        let local = self.source_for(t, &d, p.server)?;
        self.seq += 1;
        let ni = digest16(&[b"nlra-ni".as_slice(), &dev.0.to_be_bytes(), &self.now.to_be_bytes(), &self.seq.to_be_bytes()]);
        let spi_c = spi_from(b"nlra-spi-c", &ni);
        let key = p.key.as_bytes();
        let mut msg = MAGIC.to_vec();
        msg.push(T_REQUEST);
        msg.extend_from_slice(&ni);
        msg.extend_from_slice(&spi_c.to_be_bytes());
        put_text(&mut msg, p.group);
        put_text(&mut msg, p.user);
        put_bytes(&mut msg, &hide(key, &ni, p.password.as_bytes()));
        let proof = sign(key, b"nlra-req", &msg);
        msg.extend_from_slice(&proof);
        let reply = self.ra_exchange(t, dev, &d, local, p.server, msg, &ni).ok_or_else(|| {
            fail(format!("{} did not answer: there is no VPN concentrator there, or something between blocks UDP port {RA_PORT}", p.server))
        })?;
        let damaged = || fail("the VPN concentrator sent a damaged answer".into());
        let mut r = Reader { b: &reply, at: 5 };
        let status = r.u8().ok_or_else(damaged)?;
        r.take(16).ok_or_else(damaged)?;
        if status != 0 {
            let why = r.text().unwrap_or_default();
            return Err(fail(format!("the VPN concentrator refused the connection: {why}")));
        }
        let nr = r.array16().ok_or_else(damaged)?;
        let spi_s = r.u32().ok_or_else(damaged)?;
        let a: [u8; 4] = r.take(4).ok_or_else(damaged)?.try_into().map_err(|_| damaged())?;
        let assigned = Ipv4Addr::from(a);
        let transforms = r.text().ok_or_else(damaged)?;
        let dns_octets: [u8; 4] = r.take(4).ok_or_else(damaged)?.try_into().map_err(|_| damaged())?;
        let dns = Some(Ipv4Addr::from(dns_octets)).filter(|a| !a.is_unspecified());
        let domain = r.text().ok_or_else(damaged)?;
        let split = match r.u8().ok_or_else(damaged)? {
            0 => None,
            _ => {
                let count = usize::from(r.u8().ok_or_else(damaged)?);
                if count > MAX_SPLIT {
                    return Err(fail("the VPN concentrator sent too many split-tunnel networks".into()));
                }
                let mut nets = Vec::with_capacity(count);
                for _ in 0..count {
                    let a: [u8; 4] = r.take(4).ok_or_else(damaged)?.try_into().map_err(|_| damaged())?;
                    let w: [u8; 4] = r.take(4).ok_or_else(damaged)?.try_into().map_err(|_| damaged())?;
                    nets.push(AclAddr { addr: Ipv4Addr::from(a), wildcard: Ipv4Addr::from(w) });
                }
                Some(nets)
            }
        };
        let signed = reply.get(..r.at).ok_or_else(damaged)?;
        let their_proof = r.take(32).ok_or_else(damaged)?;
        if r.at != reply.len() || sign(key, b"nlra-rsp", signed).as_slice() != their_proof {
            return Err(fail("the answer did not carry a valid proof of the group key (the key differs, or the answer was altered)".into()));
        }
        if assigned.is_unspecified() || assigned.is_multicast() || assigned.is_broadcast() || assigned.is_loopback() {
            return Err(fail(format!("the VPN concentrator offered {assigned}, which cannot be used as an address")));
        }
        let ts = TransformSet {
            name: "remote-access".into(),
            transforms: transforms.split(',').filter(|w| !w.is_empty()).take(4).map(str::to_string).collect(),
            transport: false,
        };
        let suite = crate::vpn::esp_suite(&ts);
        if suite.is_some_and(|s| s.encr == EspEncr::Ah) || crate::vpn::bundle_ah(&ts).is_some() {
            return Err(fail("the VPN concentrator offered AH, which this client does not take".into()));
        }
        let (c2s, s2c) = session_keys(key, &ni, &nr, suite);
        let now = self.now;
        self.st(dev).ra_client = Some(RaSa {
            peer: p.server,
            local,
            assigned,
            group: p.group.to_string(),
            user: p.user.to_string(),
            iface: String::new(),
            suite,
            transforms,
            spi_in: spi_c,
            spi_out: spi_s,
            split,
            dns,
            domain,
            key_in: s2c,
            key_out: c2s,
            since_us: now,
            encaps: 0,
            decaps: 0,
            drops: 0,
        });
        Ok(())
    }

    /// Sends the request and waits for the answer that carries `ni`; `None` when none came.
    #[allow(clippy::too_many_arguments)]
    fn ra_exchange(
        &mut self,
        t: &Topology,
        dev: DeviceId,
        d: &Device,
        local: Ipv4Addr,
        server: Ipv4Addr,
        msg: Vec<u8>,
        ni: &[u8; 16],
    ) -> Option<Vec<u8>> {
        let port = self.next_port();
        let st = self.st(dev);
        st.ra_port = Some(port);
        st.ra_inbox.clear();
        // A reply: the magic, its type, the status, then the nonce of the request.
        let want = |b: &Vec<u8>| b.get(6..22) == Some(&ni[..]);
        let have = |s: &Sim| s.state.get(&dev).is_some_and(|st| st.ra_inbox.iter().any(&want));
        // Sent up to three times: the first may wait for ARP on the way.
        for _ in 0..3 {
            let start = self.now;
            if self.udp_out(t, d, local, server, port, RA_PORT, msg.clone()).is_err() {
                self.advance(t, start.saturating_add(100_000), |_| false);
                continue;
            }
            self.advance(t, start.saturating_add(WAIT_US), have);
            if have(self) {
                break;
            }
        }
        let st = self.st(dev);
        st.ra_port = None;
        st.ra_inbox.iter().find(|b| want(b)).cloned()
    }

    /// Ends a PC's session: the concentrator is told (when it can be reached) and the PC
    /// forgets it either way.
    pub fn ra_disconnect(&mut self, t: &Topology, dev: DeviceId) -> Result<(), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?.clone();
        let sa = self.st(dev).ra_client.take().ok_or_else(|| SimError::NoSource("the VPN Client is not connected".into()))?;
        let mut msg = MAGIC.to_vec();
        msg.push(T_DELETE);
        msg.extend_from_slice(&sa.spi_out.to_be_bytes());
        let proof = sign(&sa.key_out, b"nlra-del", &msg);
        msg.extend_from_slice(&proof);
        let port = self.next_port();
        let start = self.now;
        if self.udp_out(t, &d, sa.local, sa.peer, port, RA_PORT, msg).is_ok() {
            self.advance(t, start.saturating_add(200_000), |_| false);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_expand_the_same_for_both_ends_and_differ_by_direction() {
        let (a1, b1) = session_keys(b"group-key-test", &[1; 16], &[2; 16], None);
        let (a2, b2) = session_keys(b"group-key-test", &[1; 16], &[2; 16], None);
        assert_eq!((a1.clone(), b1.clone()), (a2, b2));
        assert_eq!((a1.len(), b1.len()), (36, 36));
        assert_ne!(a1, b1);
        let (a3, _) = session_keys(b"other-key-test", &[1; 16], &[2; 16], None);
        assert_ne!(a1, a3);
        assert_eq!(expand(b"k", b"s", 100).len(), 100);
    }

    #[test]
    fn a_password_is_hidden_and_comes_back_only_with_the_key() {
        let hidden = hide(b"group-key-test", &[7; 16], b"pw-test");
        assert_ne!(hidden.as_slice(), b"pw-test".as_slice());
        assert_eq!(hide(b"group-key-test", &[7; 16], &hidden), b"pw-test".to_vec());
        assert_ne!(hide(b"wrong-key-test", &[7; 16], &hidden), b"pw-test".to_vec());
    }

    #[test]
    fn a_request_parses_and_every_truncation_is_refused() {
        let ni = [9u8; 16];
        let mut msg = MAGIC.to_vec();
        msg.push(T_REQUEST);
        msg.extend_from_slice(&ni);
        msg.extend_from_slice(&300u32.to_be_bytes());
        put_text(&mut msg, "team");
        put_text(&mut msg, "ann");
        put_bytes(&mut msg, &hide(b"group-key-test", &ni, b"pw-test"));
        let proof = sign(b"group-key-test", b"nlra-req", &msg);
        msg.extend_from_slice(&proof);
        let r = parse_request(&msg).unwrap();
        assert_eq!((r.group.as_str(), r.user.as_str(), r.spi_c), ("team", "ann", 300));
        assert_eq!(r.proof, proof.to_vec());
        for n in 0..msg.len() {
            assert!(parse_request(msg.get(..n).unwrap_or_default()).is_none(), "cut at {n}");
        }
        let mut longer = msg.clone();
        longer.push(0);
        assert!(parse_request(&longer).is_none());
    }

    #[test]
    fn spis_are_never_reserved_and_text_fields_are_bounded() {
        for i in 0..200u8 {
            assert!(spi_from(b"t", &[i]) >= 256);
        }
        let mut out = Vec::new();
        put_text(&mut out, &"x".repeat(1_000));
        assert_eq!(out.len(), 256);
    }
}
