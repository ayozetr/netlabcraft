//! IKEv2 (RFC 7296) for crypto map entries with `set ikev2-profile`: the IKE_SA_INIT and
//! IKE_AUTH exchanges on the wire (UDP 500), each router acting on its own configuration only.
//!
//! IKE_SA_INIT: the initiator offers the proposals of its IKEv2 policies with a Diffie-Hellman
//! public value for the first group it offers; the responder picks the first proposal it has
//! too (or answers NO_PROPOSAL_CHOSEN), asks for another key exchange with INVALID_KE_PAYLOAD
//! when the value is for a group it didn't pick, and answers with its own value. Both work out
//! the shared secret (a real modular exponentiation for the MODP groups, `netlabcraft_wire::dh`;
//! a stand-in for the elliptic-curve groups) and derive the IKE SA's keys from it with the
//! negotiated PRF (RFC 7296 §2.14). IKE_AUTH, encrypted with the negotiated AES-CBC key size and
//! protected with the negotiated integrity check: the initiator sends
//! its identity, the AUTH value made with its pre-shared key over the first message (§2.15),
//! the ESP proposal from its transform set and the traffic selectors from its crypto map's
//! access list. The responder finds the IKEv2 profile matching that identity, checks the AUTH
//! value with the key its keyring holds for the peer (AUTHENTICATION_FAILED otherwise), the ESP
//! proposal against its own transform set (NO_PROPOSAL_CHOSEN) and the selectors against its
//! own access list (TS_UNACCEPTABLE), then answers with its own identity and AUTH value, which
//! the initiator checks the same way. ESP keys come from SK_d (§2.17); a transform set of
//! `esp-aes` (128, 192 or 256 bits) or `esp-null` with `esp-sha-hmac`, `esp-sha256-hmac` or
//! `esp-md5-hmac` then runs those algorithms for real; DES, 3DES and AH keep the stand-in cipher
//! of `netlabcraft_wire::vpn`.
//!
//! Rekeying: near the end of the child SA's lifetime (90 % of its seconds or kilobytes) the
//! router that started the IKE SA renews it with a CREATE_CHILD_SA exchange (RFC 7296 §1.3.3:
//! REKEY_SA naming the old SPI, a new ESP proposal and nonce, the selectors; new keys from
//! prf+(SK_d, Ni | Nr)), switches to the new SA and deletes the old one with an INFORMATIONAL
//! exchange carrying a Delete payload (§1.4.1), which the peer answers with its own. A child
//! SA past its lifetime is dropped and negotiated again from IKE_SA_INIT.

use std::net::Ipv4Addr;

use netlabcraft_model::Device;
use netlabcraft_model::Topology;
use netlabcraft_model::vpn::{CryptoMapEntry, Ikev2Proposal};
use netlabcraft_wire::dh;
use netlabcraft_wire::ike::filler;
use netlabcraft_wire::ikev2::{self, Keys, Message, Payload, Proposal, TrafficSelector, Transform};
use netlabcraft_wire::suite::{Integ, Prf};
use netlabcraft_wire::vpn::{EspSa, EspSuite};
use netlabcraft_wire::{Ipv4, PROTO_UDP, Udp};

use crate::vpn::{IkePhase, IpsecSa, acl_permits, same_name};
use crate::{L3If, Sim};

/// An exchange that hasn't finished (or failed) this long is started again by the next packet.
const RETRY_US: u64 = 2_000_000;

/// What one IKEv2 exchange keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V2State {
    pub spi_i: [u8; 8],
    pub spi_r: [u8; 8],
    /// The profile that matched the peer.
    pub profile: Option<String>,
    /// The algorithms agreed, for reports.
    pub algorithms: String,
    ni: Vec<u8>,
    nr: Vec<u8>,
    ke_i: Vec<u8>,
    ke_r: Vec<u8>,
    /// This router's Diffie-Hellman private value and the group it is for.
    dh_priv: Vec<u8>,
    pub group: u16,
    msg1: Vec<u8>,
    msg2: Vec<u8>,
    keys: Option<Keys>,
    /// ESP keys: what this router sends with and what it receives with.
    pub esp_out: Vec<u8>,
    pub esp_in: Vec<u8>,
    /// The ESP algorithms run for real, or `None` for the stand-in cipher.
    pub esp_suite: Option<EspSuite>,
    /// The message ID of this router's next request on the IKE SA.
    next_req: u32,
    /// A child SA rekey this router asked for: its nonce, its new inbound SPI and the request's
    /// message ID.
    child: Option<(Vec<u8>, u32, u32)>,
}

/// The ESP algorithms of a crypto map entry's transform set, when they are implemented.
fn esp_suite_for(d: &Device, e: &CryptoMapEntry) -> Option<EspSuite> {
    e.transform_set.as_deref().and_then(|n| d.crypto.transform_set(n)).and_then(crate::vpn::esp_suite)
}

/// The IKE SA's algorithms from the chosen proposal (3DES runs as AES-CBC-128, see
/// `netlabcraft_wire::ikev2`).
fn ike_suite(p: &Proposal) -> ikev2::Suite {
    let enc_len = p
        .of(ikev2::TT_ENCR)
        .next()
        .filter(|t| t.id == ikev2::ENCR_AES_CBC)
        .and_then(|t| t.key_len)
        .map(|b| usize::from(b) / 8)
        .filter(|n| [16, 24, 32].contains(n))
        .unwrap_or(16);
    let prf = match p.of(ikev2::TT_PRF).next().map(|t| t.id) {
        Some(ikev2::PRF_HMAC_SHA2_256) => Prf::HmacSha256,
        Some(ikev2::PRF_HMAC_MD5) => Prf::HmacMd5,
        _ => Prf::HmacSha1,
    };
    let integ = match p.of(ikev2::TT_INTEG).next().map(|t| t.id) {
        Some(ikev2::AUTH_HMAC_SHA2_256_128) => Integ::HmacSha256_128,
        Some(ikev2::AUTH_HMAC_MD5_96) => Integ::HmacMd5_96,
        _ => Integ::HmacSha1_96,
    };
    ikev2::Suite { enc_len, prf, integ }
}

/// A Diffie-Hellman private value (256 bits) from a seed.
fn dh_private(seed: &[u8]) -> Vec<u8> {
    filler(&[seed, b"dh-x"].concat(), 32)
}

/// The public value for a group: g^x mod p for a MODP group, filler of the group's size
/// otherwise.
fn ke_public(group: u16, private: &[u8], seed: &[u8]) -> Vec<u8> {
    match dh::modp(group) {
        Some(g) => g.public(private),
        None => filler(&[seed, b"ke"].concat(), ke_len(u8::try_from(group).unwrap_or(14))),
    }
}

/// The shared secret, or `None` when the peer's public value isn't valid for the group.
fn dh_shared(group: u16, private: &[u8], ke_i: &[u8], ke_r: &[u8], initiator: bool) -> Option<Vec<u8>> {
    match dh::modp(group) {
        Some(g) => g.shared(private, if initiator { ke_r } else { ke_i }),
        None => Some(ikev2::standin_shared(ke_i, ke_r)),
    }
}

fn cipher_transform(name: &str) -> Transform {
    match name {
        "3des" => Transform { kind: ikev2::TT_ENCR, id: ikev2::ENCR_3DES, key_len: None },
        n => {
            let bits = n.rsplit('-').next().and_then(|b| b.parse().ok()).unwrap_or(128);
            Transform { kind: ikev2::TT_ENCR, id: ikev2::ENCR_AES_CBC, key_len: Some(bits) }
        }
    }
}

fn integ_ids(name: &str) -> (u16, u16) {
    match name {
        "sha256" => (ikev2::PRF_HMAC_SHA2_256, ikev2::AUTH_HMAC_SHA2_256_128),
        "md5" => (ikev2::PRF_HMAC_MD5, ikev2::AUTH_HMAC_MD5_96),
        _ => (ikev2::PRF_HMAC_SHA1, ikev2::AUTH_HMAC_SHA1_96),
    }
}

/// A configured proposal as the SA payload offers it.
fn wire_proposal(number: u8, p: &Ikev2Proposal) -> Proposal {
    let mut transforms: Vec<Transform> = p.encryption.iter().map(|e| cipher_transform(e)).collect();
    for i in &p.integrity {
        let (prf, integ) = integ_ids(i);
        transforms.push(Transform { kind: ikev2::TT_PRF, id: prf, key_len: None });
        transforms.push(Transform { kind: ikev2::TT_INTEG, id: integ, key_len: None });
    }
    for g in &p.group {
        transforms.push(Transform { kind: ikev2::TT_DH, id: u16::from(*g), key_len: None });
    }
    Proposal { number, protocol: ikev2::PROTO_IKE, spi: Vec::new(), transforms }
}

/// The responder's choice: one transform of each type that both proposals have, or none; the
/// group of the initiator's key exchange when both have it.
fn choose(offer: &Proposal, mine: &Proposal, ke_group: u16) -> Option<Proposal> {
    let mut chosen = Vec::new();
    for kind in [ikev2::TT_ENCR, ikev2::TT_PRF, ikev2::TT_INTEG, ikev2::TT_DH] {
        let both = |t: &&Transform| mine.of(kind).any(|m| m == *t);
        let preferred = if kind == ikev2::TT_DH { offer.of(kind).filter(both).find(|t| t.id == ke_group) } else { None };
        let t = preferred.or_else(|| offer.of(kind).find(both))?;
        chosen.push(t.clone());
    }
    Some(Proposal { number: offer.number, protocol: ikev2::PROTO_IKE, spi: Vec::new(), transforms: chosen })
}

fn describe(p: &Proposal) -> String {
    let mut parts = Vec::new();
    for t in &p.transforms {
        parts.push(match (t.kind, t.id) {
            (ikev2::TT_ENCR, ikev2::ENCR_AES_CBC) => format!("AES-CBC-{}", t.key_len.unwrap_or(128)),
            (ikev2::TT_ENCR, ikev2::ENCR_3DES) => "3DES".into(),
            (ikev2::TT_PRF, ikev2::PRF_HMAC_SHA1) => "PRF HMAC-SHA1".into(),
            (ikev2::TT_PRF, ikev2::PRF_HMAC_SHA2_256) => "PRF HMAC-SHA256".into(),
            (ikev2::TT_PRF, _) => "PRF HMAC-MD5".into(),
            (ikev2::TT_INTEG, ikev2::AUTH_HMAC_SHA1_96) => "HMAC-SHA1-96".into(),
            (ikev2::TT_INTEG, ikev2::AUTH_HMAC_SHA2_256_128) => "HMAC-SHA256-128".into(),
            (ikev2::TT_INTEG, _) => "HMAC-MD5-96".into(),
            (ikev2::TT_DH, g) => format!("DH group {g}"),
            _ => "?".into(),
        });
    }
    parts.join(", ")
}

/// The ESP proposal from a transform set (RFC 7296 §3.3.2 transform ids).
fn esp_proposal(d: &Device, e: &CryptoMapEntry, spi: u32) -> Proposal {
    let ts = e.transform_set.as_deref().and_then(|n| d.crypto.transform_set(n));
    let has = |w: &str| ts.is_some_and(|t| t.transforms.iter().any(|x| x.split_whitespace().next() == Some(w)));
    let bits = ts.and_then(|t| t.transforms.iter().find_map(|x| x.strip_prefix("esp-aes ").and_then(|b| b.parse().ok()))).unwrap_or(128);
    let mut transforms = Vec::new();
    if has("esp-aes") {
        transforms.push(Transform { kind: ikev2::TT_ENCR, id: ikev2::ENCR_AES_CBC, key_len: Some(bits) });
    } else if has("esp-3des") {
        transforms.push(Transform { kind: ikev2::TT_ENCR, id: ikev2::ENCR_3DES, key_len: None });
    } else if has("esp-null") {
        transforms.push(Transform { kind: ikev2::TT_ENCR, id: ikev2::ENCR_NULL, key_len: None });
    } else {
        transforms.push(Transform { kind: ikev2::TT_ENCR, id: 2, key_len: None });
    }
    if has("esp-sha-hmac") {
        transforms.push(Transform { kind: ikev2::TT_INTEG, id: ikev2::AUTH_HMAC_SHA1_96, key_len: None });
    } else if has("esp-sha256-hmac") {
        transforms.push(Transform { kind: ikev2::TT_INTEG, id: ikev2::AUTH_HMAC_SHA2_256_128, key_len: None });
    } else if has("esp-md5-hmac") {
        transforms.push(Transform { kind: ikev2::TT_INTEG, id: ikev2::AUTH_HMAC_MD5_96, key_len: None });
    }
    transforms.push(Transform { kind: ikev2::TT_ESN, id: 0, key_len: None });
    Proposal { number: 1, protocol: ikev2::PROTO_ESP, spi: spi.to_be_bytes().to_vec(), transforms }
}

/// The traffic selectors of a crypto map entry: the first permit line of its access list
/// (source range, destination range).
fn selectors(d: &Device, e: &CryptoMapEntry) -> Option<(TrafficSelector, TrafficSelector)> {
    let acl = d.acls.iter().find(|a| Some(&a.name) == e.acl.as_ref())?;
    let line = acl.entries.iter().find(|x| x.permit)?;
    let range = |a: &netlabcraft_model::acl::AclAddr| {
        let lo = u32::from(a.addr) & !u32::from(a.wildcard);
        TrafficSelector { protocol: 0, start_port: 0, end_port: 65_535, start: Ipv4Addr::from(lo), end: Ipv4Addr::from(lo | u32::from(a.wildcard)) }
    };
    Some((range(&line.src), range(&line.dst)))
}

/// The crypto map entry for a peer on the interface holding `local`.
pub(crate) fn entry_for(d: &Device, local: Ipv4Addr, peer: Ipv4Addr) -> Option<CryptoMapEntry> {
    let ifc = crate::l3_ifaces(d).into_iter().find(|i| i.net.addr == local)?;
    let map = d.crypto.applied.iter().find(|(n, _)| same_name(n, &ifc.name)).map(|(_, m)| m.clone())?;
    d.crypto.map(&map).find(|e| e.peer == Some(peer)).cloned()
}

fn id_body(a: Ipv4Addr) -> Vec<u8> {
    let mut v = vec![ikev2::ID_IPV4_ADDR, 0, 0, 0];
    v.extend_from_slice(&a.octets());
    v
}

fn spi_u32(spi: &[u8]) -> u32 {
    spi.get(..4).and_then(|s| s.try_into().ok()).map_or(0, u32::from_be_bytes)
}

fn iv_for(seed: &[u8]) -> [u8; 16] {
    filler(seed, 16).try_into().unwrap_or([7; 16])
}

/// ESP keys (RFC 7296 §2.17): KEYMAT = prf+(SK_d, Ni | Nr), as much as the ESP algorithms take
/// for each direction (36 bytes for the stand-in cipher); the initiator's outbound key first.
fn esp_keys(keys: &Keys, suite: Option<EspSuite>, ni: &[u8], nr: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let one = suite.map_or(36, EspSuite::keymat_len);
    let m = keys.suite.prf.plus(&keys.sk_d, &[ni, nr].concat(), 2 * one);
    (m.get(..one).unwrap_or_default().to_vec(), m.get(one..).unwrap_or_default().to_vec())
}

/// Seals a datagram as ESP with an association's outbound keys.
pub(crate) fn esp_seal(suite: Option<EspSuite>, key: &[u8], spi: u32, seq: u32, datagram: &[u8]) -> netlabcraft_wire::vpn::Esp {
    match suite.and_then(|s| EspSa::from_keymat(s, key)) {
        Some(sa) => {
            let iv = iv_for(&[key, &seq.to_be_bytes()[..], &spi.to_be_bytes()[..]].concat());
            netlabcraft_wire::vpn::Esp::seal_with(&sa, spi, seq, netlabcraft_wire::vpn::NEXT_IPV4, datagram, iv)
        }
        None => netlabcraft_wire::vpn::Esp::seal(key, spi, seq, netlabcraft_wire::vpn::NEXT_IPV4, datagram),
    }
}

/// Opens an ESP packet with an association's inbound keys.
pub(crate) fn esp_open(suite: Option<EspSuite>, key: &[u8], esp: &netlabcraft_wire::vpn::Esp) -> netlabcraft_wire::Result<(u8, Vec<u8>)> {
    match suite.and_then(|s| EspSa::from_keymat(s, key)) {
        Some(sa) => esp.open_with(&sa),
        None => esp.open(key),
    }
}

/// What this router needs before it starts IKEv2 for an entry: a peer, a transform set, a
/// profile matching the peer with keys for it, and proposals to offer.
pub(crate) fn v2_local_check(d: &Device, e: &CryptoMapEntry, local: Ipv4Addr) -> Result<(), String> {
    let peer = e.peer.ok_or_else(|| format!("crypto map {} {} has no peer (`set peer`)", e.map, e.seq))?;
    let ts = e.transform_set.as_deref().ok_or_else(|| format!("crypto map {} {} has no transform set", e.map, e.seq))?;
    d.crypto.transform_set(ts).ok_or_else(|| format!("transform set {ts} does not exist"))?;
    let name = e.ikev2_profile.as_deref().unwrap_or_default();
    let prof = d.crypto.ikev2.profile(name).ok_or_else(|| format!("IKEv2 profile {name} does not exist"))?;
    if !prof.matches(peer) {
        return Err(format!("IKEv2 profile {name} does not match identity {peer} (`match identity remote address`)"));
    }
    d.crypto.ikev2.keys_for(prof, peer).ok_or_else(|| format!("IKEv2 profile {name}: no keyring key for {peer}"))?;
    if d.crypto.ikev2.offered(local).is_empty() {
        return Err("no IKEv2 policy offers a complete proposal (encryption, integrity and group)".into());
    }
    Ok(())
}

impl Sim {
    fn ike2_send(&mut self, t: &Topology, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, raw: Vec<u8>) {
        let udp = Udp { src_port: netlabcraft_wire::ike::ISAKMP_PORT, dst_port: netlabcraft_wire::ike::ISAKMP_PORT, payload: raw };
        let ip = Ipv4 { id: self.ip_id(), ttl: 255, protocol: PROTO_UDP, src: local, dst: peer, payload: udp.encode(local, peer) };
        let held = self.hold_for_arp;
        self.hold_for_arp = true;
        let _ = self.send_ip(t, d, ip);
        self.hold_for_arp = held;
    }

    fn ike2_fail(&mut self, d: &Device, peer: Ipv4Addr, why: String) {
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.phase = IkePhase::Failed;
        sa.failed = Some(why.clone());
        self.log(d.id, &format!("IKEv2 with {peer} failed: {why}"));
    }

    /// A packet a crypto map entry with an IKEv2 profile protects: sealed as ESP once the SA is
    /// up, otherwise IKEv2 is started (and the packet dropped).
    pub(crate) fn crypto_out_v2(
        &mut self,
        t: &Topology,
        d: &Device,
        out: &L3If,
        datagram: &[u8],
        entry: &CryptoMapEntry,
    ) -> Result<Option<(Vec<u8>, Ipv4Addr)>, String> {
        let peer = entry.peer.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let local = out.net.addr;
        let now = self.now;
        let st = self.st(d.id);
        if !st.ipsec.contains_key(&peer) && st.ipsec.len() >= 256 {
            st.ipsec.clear();
        }
        let sa = st.ipsec.entry(peer).or_default();
        sa.local = local;
        sa.peer = peer;
        sa.iface = out.name.clone();
        sa.map = entry.map.clone();
        sa.seq = entry.seq;
        if let Err(why) = v2_local_check(d, entry, local) {
            sa.failed = Some(why.clone());
            sa.phase = IkePhase::Failed;
            sa.drops = sa.drops.saturating_add(1);
            return Err(format!("IPsec: no security association with {peer}: {why}"));
        }
        if sa.phase == IkePhase::Established && sa.v2.is_some() {
            match sa.life_state(now) {
                crate::vpn::Life::Expired => {
                    let why = format!("IPsec SA with {peer} reached the end of its lifetime; negotiating new keys");
                    let drops = sa.drops.saturating_add(1);
                    *sa = IpsecSa { local, peer, iface: out.name.clone(), map: entry.map.clone(), seq: entry.seq, drops, ..IpsecSa::default() };
                    self.log(d.id, &why);
                    self.ike2_start(t, d, local, peer, entry, None);
                    return Err(format!("IPsec: {why}"));
                }
                crate::vpn::Life::Soft if sa.initiator && (sa.rekey_us == 0 || now.saturating_sub(sa.rekey_us) >= RETRY_US) => {
                    self.ike2_rekey(t, d, local, peer, entry);
                }
                _ => {}
            }
            let sa = self.st(d.id).ipsec.entry(peer).or_default();
            sa.encaps = sa.encaps.saturating_add(1);
            sa.bytes = sa.bytes.saturating_add(u64::try_from(datagram.len()).unwrap_or(u64::MAX));
            let seq = u32::try_from(sa.encaps).unwrap_or(u32::MAX);
            let key = sa.v2.as_ref().map(|v| v.esp_out.clone()).unwrap_or_default();
            let suite = sa.v2.as_ref().and_then(|v| v.esp_suite);
            let spi = sa.spi_out;
            let esp = esp_seal(suite, &key, spi, seq, datagram);
            let id = self.ip_id();
            let mut outer = Ipv4 { id, ttl: 255, protocol: netlabcraft_wire::vpn::PROTO_ESP, src: local, dst: peer, payload: esp.encode() }.encode();
            crate::qos::copy_ds(datagram, &mut outer);
            return Ok(Some((outer, peer)));
        }
        sa.drops = sa.drops.saturating_add(1);
        let busy = matches!(sa.phase, IkePhase::V2Init | IkePhase::V2Auth | IkePhase::Failed) && now.saturating_sub(sa.started_us) < RETRY_US;
        if busy {
            let why = match (&sa.failed, sa.phase) {
                (Some(w), IkePhase::Failed) => w.clone(),
                _ => format!("IKEv2 negotiation in progress ({})", sa.phase.describe()),
            };
            return Err(format!("IPsec: no security association with {peer}: {why}"));
        }
        self.ike2_start(t, d, local, peer, entry, None);
        Err(format!("IPsec: no security association with {peer} yet: IKEv2 IKE_SA_INIT sent"))
    }

    /// IKE_SA_INIT request, with a key exchange for `group` (the first group offered when
    /// `None`).
    fn ike2_start(&mut self, t: &Topology, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, entry: &CryptoMapEntry, group: Option<u16>) {
        let now = self.now;
        let seed = [&local.octets()[..], &peer.octets()[..], &now.to_be_bytes()[..], &group.unwrap_or(0).to_be_bytes()[..]].concat();
        let spi_i: [u8; 8] = filler(&seed, 8).try_into().unwrap_or([1; 8]);
        let offered = d.crypto.ikev2.offered(local);
        let proposals: Vec<Proposal> = offered.iter().take(16).zip(1u8..).map(|(p, n)| wire_proposal(n, p)).collect();
        let group = group.unwrap_or_else(|| offered.first().and_then(|p| p.group.first()).copied().map_or(14, u16::from));
        let dh_priv = dh_private(&seed);
        let ke = ke_public(group, &dh_priv, &seed);
        let ni = filler(&[&seed[..], b"ni"].concat(), 32);
        let m = Message {
            spi_i,
            spi_r: [0; 8],
            exchange: ikev2::IKE_SA_INIT,
            flags: ikev2::FLAG_INITIATOR,
            message_id: 0,
            payloads: vec![Payload::Sa(proposals), Payload::Ke { group, data: ke.clone() }, Payload::Nonce(ni.clone())],
        };
        let raw = m.encode();
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        *sa = IpsecSa {
            local,
            peer,
            iface: sa.iface.clone(),
            map: entry.map.clone(),
            seq: entry.seq,
            phase: IkePhase::V2Init,
            initiator: true,
            started_us: now,
            drops: sa.drops,
            v2: Some(Box::new(V2State {
                spi_i,
                profile: entry.ikev2_profile.clone(),
                ni,
                ke_i: ke,
                dh_priv,
                group,
                msg1: raw.clone(),
                ..V2State::default()
            })),
            ..IpsecSa::default()
        };
        self.ike2_send(t, d, local, peer, raw);
    }

    /// An IKEv2 message to this router.
    pub(crate) fn ike2_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, raw: &[u8]) -> Result<(), String> {
        let m = Message::decode(raw).map_err(|e| e.to_string())?;
        let (peer, local) = (ip.src, ip.dst);
        if d.crypto.ikev2.is_empty() {
            return Err(format!("IKEv2 from {peer}: IKEv2 is not configured here"));
        }
        let response = m.flags & ikev2::FLAG_RESPONSE != 0;
        let cur = self.state.get(&d.id).and_then(|s| s.ipsec.get(&peer)).cloned().unwrap_or_default();
        match (m.exchange, response) {
            (ikev2::IKE_SA_INIT, false) => self.ike2_init_request(t, d, local, peer, &m, raw),
            (ikev2::IKE_SA_INIT, true) => {
                let Some(v2) = cur.v2.filter(|v| cur.initiator && cur.phase == IkePhase::V2Init && v.spi_i == m.spi_i) else {
                    return Err(format!("IKEv2 IKE_SA_INIT response from {peer} for no exchange in progress"));
                };
                self.ike2_init_response(t, d, local, peer, &m, raw, *v2)
            }
            (ikev2::IKE_AUTH, false) => {
                let Some(v2) = cur.v2.filter(|v| !cur.initiator && cur.phase == IkePhase::V2Init && v.spi_i == m.spi_i && v.spi_r == m.spi_r) else {
                    return Err(format!("IKEv2 IKE_AUTH from {peer} for no exchange in progress (SPIs)"));
                };
                self.ike2_auth_request(t, d, local, peer, &m, raw, *v2)
            }
            (ikev2::IKE_AUTH, true) => {
                let Some(v2) = cur.v2.filter(|v| cur.initiator && cur.phase == IkePhase::V2Auth && v.spi_i == m.spi_i && v.spi_r == m.spi_r) else {
                    return Err(format!("IKEv2 IKE_AUTH response from {peer} for no exchange in progress"));
                };
                self.ike2_auth_response(d, local, peer, raw, *v2)
            }
            (ikev2::CREATE_CHILD_SA | ikev2::INFORMATIONAL, _) => {
                let Some(v2) = cur.v2.filter(|v| cur.phase == IkePhase::Established && v.spi_i == m.spi_i && v.spi_r == m.spi_r) else {
                    return Err(format!("IKEv2 {} from {peer} for no IKE SA here", ikev2::exchange_name(m.exchange)));
                };
                self.ike2_on_sa(t, d, local, peer, &m, raw, *v2, response)
            }
            (e, _) => Err(format!("IKEv2 {} from {peer} is not handled here", ikev2::exchange_name(e))),
        }
    }

    fn ike2_init_request(&mut self, t: &Topology, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, m: &Message, raw: &[u8]) -> Result<(), String> {
        let now = self.now;
        let seed = [&m.spi_i[..], &local.octets()[..], &now.to_be_bytes()[..]].concat();
        let spi_r: [u8; 8] = filler(&seed, 8).try_into().unwrap_or([2; 8]);
        let offers: Vec<&Proposal> = m.payloads.iter().filter_map(|p| if let Payload::Sa(v) = p { Some(v) } else { None }).flatten().collect();
        let mine: Vec<Proposal> = d.crypto.ikev2.offered(local).iter().zip(1u8..).map(|(p, n)| wire_proposal(n, p)).collect();
        let (ke_group, ke_i) =
            m.payloads.iter().find_map(|p| if let Payload::Ke { group, data } = p { Some((*group, data.clone())) } else { None }).unwrap_or_default();
        let chosen = offers.iter().find_map(|o| mine.iter().find_map(|x| choose(o, x, ke_group)));
        let ni = m.payloads.iter().find_map(|p| if let Payload::Nonce(n) = p { Some(n.clone()) } else { None }).unwrap_or_default();
        let st = self.st(d.id);
        if !st.ipsec.contains_key(&peer) && st.ipsec.len() >= 256 {
            st.ipsec.clear();
        }
        let entry = entry_for(d, local, peer);
        let iface = crate::l3_ifaces(d).into_iter().find(|i| i.net.addr == local).map(|i| i.name).unwrap_or_default();
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        *sa = IpsecSa {
            local,
            peer,
            iface,
            map: entry.as_ref().map(|e| e.map.clone()).unwrap_or_default(),
            seq: entry.as_ref().map_or(0, |e| e.seq),
            phase: IkePhase::V2Init,
            started_us: now,
            ..IpsecSa::default()
        };
        let Some(chosen) = chosen else {
            let reply = Message {
                spi_i: m.spi_i,
                spi_r: [0; 8],
                exchange: ikev2::IKE_SA_INIT,
                flags: ikev2::FLAG_RESPONSE,
                message_id: m.message_id,
                payloads: vec![Payload::Notify { protocol: 0, spi: Vec::new(), kind: ikev2::NO_PROPOSAL_CHOSEN, data: Vec::new() }],
            };
            self.ike2_fail(d, peer, "none of the peer's IKEv2 proposals matches one here; answered NO_PROPOSAL_CHOSEN".into());
            self.ike2_send(t, d, local, peer, reply.encode());
            return Ok(());
        };
        let group = chosen.of(ikev2::TT_DH).next().map_or(14, |g| g.id);
        if group != ke_group {
            // RFC 7296 §1.2: the key exchange is for another group; say which one to use.
            let reply = Message {
                spi_i: m.spi_i,
                spi_r: [0; 8],
                exchange: ikev2::IKE_SA_INIT,
                flags: ikev2::FLAG_RESPONSE,
                message_id: m.message_id,
                payloads: vec![Payload::Notify { protocol: 0, spi: Vec::new(), kind: ikev2::INVALID_KE_PAYLOAD, data: group.to_be_bytes().to_vec() }],
            };
            self.st(d.id).ipsec.remove(&peer);
            self.ike2_send(t, d, local, peer, reply.encode());
            return Ok(());
        }
        let dh_priv = dh_private(&seed);
        let ke_r = ke_public(group, &dh_priv, &seed);
        let nr = filler(&[&seed[..], b"nr"].concat(), 32);
        let Some(shared) = dh_shared(group, &dh_priv, &ke_i, &ke_r, false) else {
            self.ike2_fail(d, peer, format!("the peer's Diffie-Hellman value is not valid for group {group}"));
            return Ok(());
        };
        let reply = Message {
            spi_i: m.spi_i,
            spi_r,
            exchange: ikev2::IKE_SA_INIT,
            flags: ikev2::FLAG_RESPONSE,
            message_id: m.message_id,
            payloads: vec![Payload::Sa(vec![chosen.clone()]), Payload::Ke { group, data: ke_r.clone() }, Payload::Nonce(nr.clone())],
        };
        let msg2 = reply.encode();
        let keys = Keys::derive_with(ike_suite(&chosen), &shared, &ni, &nr, &m.spi_i, &spi_r);
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.v2 = Some(Box::new(V2State {
            spi_i: m.spi_i,
            spi_r,
            algorithms: describe(&chosen),
            ni,
            nr,
            ke_i,
            ke_r,
            dh_priv,
            group,
            msg1: raw.to_vec(),
            msg2: msg2.clone(),
            keys: Some(keys),
            ..V2State::default()
        }));
        self.ike2_send(t, d, local, peer, msg2);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn ike2_init_response(
        &mut self,
        t: &Topology,
        d: &Device,
        local: Ipv4Addr,
        peer: Ipv4Addr,
        m: &Message,
        raw: &[u8],
        mut v2: V2State,
    ) -> Result<(), String> {
        if let Some(kind) = Message::notify(&m.payloads) {
            let wanted = m.payloads.iter().find_map(|p| match p {
                Payload::Notify { kind: ikev2::INVALID_KE_PAYLOAD, data, .. } => Some(u16::from_be_bytes([*data.first()?, *data.get(1)?])),
                _ => None,
            });
            let offered = d.crypto.ikev2.offered(local).iter().any(|p| p.group.iter().any(|g| Some(u16::from(*g)) == wanted));
            if let (Some(g), true, Some(entry)) = (wanted, offered, entry_for(d, local, peer))
                && g != v2.group
            {
                // Once more with a key exchange for the group the responder picked.
                self.log(d.id, &format!("IKEv2 with {peer}: the peer wants a key exchange for DH group {g}; IKE_SA_INIT sent again"));
                self.ike2_start(t, d, local, peer, &entry, Some(g));
                return Ok(());
            }
            self.ike2_fail(d, peer, format!("the peer answered IKE_SA_INIT with {} (no proposal both sides have)", ikev2::notify_name(kind)));
            return Ok(());
        }
        let chosen = m.payloads.iter().find_map(|p| if let Payload::Sa(v) = p { v.first().cloned() } else { None });
        let ke_r = m.payloads.iter().find_map(|p| if let Payload::Ke { data, .. } = p { Some(data.clone()) } else { None });
        let nr = m.payloads.iter().find_map(|p| if let Payload::Nonce(n) = p { Some(n.clone()) } else { None });
        let (Some(chosen), Some(ke_r), Some(nr)) = (chosen, ke_r, nr) else {
            self.ike2_fail(d, peer, "the IKE_SA_INIT response lacks SA, KE or nonce".into());
            return Ok(());
        };
        let Some(entry) = entry_for(d, local, peer) else {
            self.ike2_fail(d, peer, format!("no crypto map entry for {peer} any more"));
            return Ok(());
        };
        let Some((my_key, _)) = entry.ikev2_profile.as_deref().and_then(|n| d.crypto.ikev2.profile(n)).and_then(|p| d.crypto.ikev2.keys_for(p, peer))
        else {
            self.ike2_fail(d, peer, "the IKEv2 profile has no key for the peer any more".into());
            return Ok(());
        };
        let group = chosen.of(ikev2::TT_DH).next().map_or(v2.group, |g| g.id);
        if group != v2.group {
            self.ike2_fail(d, peer, format!("the peer chose DH group {group} but answered no request for it"));
            return Ok(());
        }
        let Some(shared) = dh_shared(group, &v2.dh_priv, &v2.ke_i, &ke_r, true) else {
            self.ike2_fail(d, peer, format!("the peer's Diffie-Hellman value is not valid for group {group}"));
            return Ok(());
        };
        let keys = Keys::derive_with(ike_suite(&chosen), &shared, &v2.ni, &nr, &v2.spi_i, &m.spi_r);
        let my_id = entry.ikev2_profile.as_deref().and_then(|n| d.crypto.ikev2.profile(n)).and_then(|p| p.local_id).unwrap_or(local);
        let idi = id_body(my_id);
        let auth = ikev2::psk_auth_with(keys.suite.prf, my_key.as_bytes(), &v2.msg1, &nr, &keys.sk_pi, &idi);
        let spi_in = spi_of(&v2.ni, local, peer);
        let (tsi, tsr) = selectors(d, &entry).unwrap_or((
            TrafficSelector { protocol: 0, start_port: 0, end_port: 65_535, start: Ipv4Addr::UNSPECIFIED, end: Ipv4Addr::BROADCAST },
            TrafficSelector { protocol: 0, start_port: 0, end_port: 65_535, start: Ipv4Addr::UNSPECIFIED, end: Ipv4Addr::BROADCAST },
        ));
        let req = Message {
            spi_i: v2.spi_i,
            spi_r: m.spi_r,
            exchange: ikev2::IKE_AUTH,
            flags: ikev2::FLAG_INITIATOR,
            message_id: 1,
            payloads: vec![
                Payload::IdI { kind: ikev2::ID_IPV4_ADDR, data: my_id.octets().to_vec() },
                Payload::Auth { method: ikev2::AUTH_SHARED_KEY, data: auth },
                Payload::Sa(vec![esp_proposal(d, &entry, spi_in)]),
                Payload::TsI(vec![tsi]),
                Payload::TsR(vec![tsr]),
            ],
        };
        let sealed = req.encode_sealed(keys.suite.integ, &keys.sk_ei, &keys.sk_ai, iv_for(&[&v2.ni[..], b"auth-i"].concat()));
        v2.spi_r = m.spi_r;
        v2.algorithms = describe(&chosen);
        v2.nr = nr;
        v2.ke_r = ke_r;
        v2.msg2 = raw.to_vec();
        v2.keys = Some(keys);
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.phase = IkePhase::V2Auth;
        sa.spi_in = spi_in;
        sa.v2 = Some(Box::new(v2));
        self.ike2_send(t, d, local, peer, sealed);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn ike2_auth_request(
        &mut self,
        t: &Topology,
        d: &Device,
        local: Ipv4Addr,
        peer: Ipv4Addr,
        m: &Message,
        raw: &[u8],
        mut v2: V2State,
    ) -> Result<(), String> {
        let Some(keys) = v2.keys.clone() else { return Err("IKEv2 keys missing".into()) };
        let inner = match Message::open(raw, keys.suite.integ, &keys.sk_ei, &keys.sk_ai) {
            Ok(p) => p,
            Err(e) => {
                self.ike2_fail(d, peer, format!("IKE_AUTH from {peer} does not open ({e})"));
                return Ok(());
            }
        };
        let refuse = |kind: u16| Message {
            spi_i: m.spi_i,
            spi_r: m.spi_r,
            exchange: ikev2::IKE_AUTH,
            flags: ikev2::FLAG_RESPONSE,
            message_id: m.message_id,
            payloads: vec![Payload::Notify { protocol: 0, spi: Vec::new(), kind, data: Vec::new() }],
        };
        let iv = iv_for(&[&v2.nr[..], b"auth-r"].concat());
        let idi = inner.iter().find_map(|p| if let Payload::IdI { kind, data } = p { Some((*kind, data.clone())) } else { None });
        let auth = inner.iter().find_map(|p| if let Payload::Auth { data, .. } = p { Some(data.clone()) } else { None });
        let (Some((id_kind, id_data)), Some(auth)) = (idi, auth) else {
            self.ike2_fail(d, peer, "IKE_AUTH without identity or AUTH".into());
            self.ike2_send(t, d, local, peer, refuse(ikev2::AUTHENTICATION_FAILED).encode_sealed(keys.suite.integ, &keys.sk_er, &keys.sk_ar, iv));
            return Ok(());
        };
        let their_id = <[u8; 4]>::try_from(id_data.as_slice()).map(Ipv4Addr::from).unwrap_or(Ipv4Addr::UNSPECIFIED);
        let profile = d.crypto.ikev2.profiles.iter().find(|p| p.matches(their_id) && d.crypto.ikev2.keys_for(p, peer).is_some());
        let Some(profile) = profile else {
            self.ike2_fail(
                d,
                peer,
                format!("no IKEv2 profile here matches identity {their_id} with a key for {peer}; answered AUTHENTICATION_FAILED"),
            );
            self.ike2_send(t, d, local, peer, refuse(ikev2::AUTHENTICATION_FAILED).encode_sealed(keys.suite.integ, &keys.sk_er, &keys.sk_ar, iv));
            return Ok(());
        };
        let (my_key, their_key) = d.crypto.ikev2.keys_for(profile, peer).unwrap_or_default();
        let mut idi_body = vec![id_kind, 0, 0, 0];
        idi_body.extend_from_slice(&id_data);
        if ikev2::psk_auth_with(keys.suite.prf, their_key.as_bytes(), &v2.msg1, &v2.nr, &keys.sk_pi, &idi_body) != auth {
            self.ike2_fail(
                d,
                peer,
                "the peer's AUTH value does not match this router's pre-shared key (the keys differ); answered AUTHENTICATION_FAILED".into(),
            );
            self.ike2_send(t, d, local, peer, refuse(ikev2::AUTHENTICATION_FAILED).encode_sealed(keys.suite.integ, &keys.sk_er, &keys.sk_ar, iv));
            return Ok(());
        }
        // The child SA: a crypto map entry back to the peer with the same ESP transforms, and
        // selectors its access list (mirrored) permits.
        let entry = entry_for(d, local, peer);
        let offer = inner.iter().find_map(|p| if let Payload::Sa(v) = p { v.first().cloned() } else { None });
        let spi_in = spi_of(&v2.nr, local, peer);
        let verdict = match (&entry, &offer) {
            (None, _) => Err((ikev2::NO_PROPOSAL_CHOSEN, format!("no crypto map entry here for {peer}"))),
            (_, None) => Err((ikev2::NO_PROPOSAL_CHOSEN, "IKE_AUTH carries no ESP proposal".to_string())),
            (Some(e), Some(o)) => {
                let mine = esp_proposal(d, e, spi_in);
                if mine.transforms != o.transforms {
                    Err((ikev2::NO_PROPOSAL_CHOSEN, "the ESP transforms differ from this router's transform set".to_string()))
                } else {
                    let tsi = inner.iter().find_map(|p| if let Payload::TsI(v) = p { v.first().copied() } else { None });
                    let tsr = inner.iter().find_map(|p| if let Payload::TsR(v) = p { v.first().copied() } else { None });
                    let ok = match (tsi, tsr, e.acl.as_deref()) {
                        (Some(i), Some(r), Some(acl)) => {
                            let probe = Ipv4 { id: 0, ttl: 1, protocol: 0, src: r.start, dst: i.start, payload: Vec::new() };
                            acl_permits(d, acl, &probe)
                        }
                        _ => false,
                    };
                    if ok {
                        Ok(())
                    } else {
                        Err((ikev2::TS_UNACCEPTABLE, "the peer's traffic selectors are not what this router's access list protects".to_string()))
                    }
                }
            }
        };
        if let Err((kind, why)) = verdict {
            self.ike2_fail(d, peer, format!("{why}; answered {}", ikev2::notify_name(kind)));
            self.ike2_send(t, d, local, peer, refuse(kind).encode_sealed(keys.suite.integ, &keys.sk_er, &keys.sk_ar, iv));
            return Ok(());
        }
        let my_id = profile.local_id.unwrap_or(local);
        let idr = id_body(my_id);
        let my_auth = ikev2::psk_auth_with(keys.suite.prf, my_key.as_bytes(), &v2.msg2, &v2.ni, &keys.sk_pr, &idr);
        let tsi = inner.iter().find_map(|p| if let Payload::TsI(v) = p { Some(v.clone()) } else { None }).unwrap_or_default();
        let tsr = inner.iter().find_map(|p| if let Payload::TsR(v) = p { Some(v.clone()) } else { None }).unwrap_or_default();
        let mut esp = entry.as_ref().map(|e| esp_proposal(d, e, spi_in)).unwrap_or(Proposal {
            number: 1,
            protocol: ikev2::PROTO_ESP,
            spi: Vec::new(),
            transforms: Vec::new(),
        });
        esp.number = offer.as_ref().map_or(1, |o| o.number);
        let reply = Message {
            spi_i: m.spi_i,
            spi_r: m.spi_r,
            exchange: ikev2::IKE_AUTH,
            flags: ikev2::FLAG_RESPONSE,
            message_id: m.message_id,
            payloads: vec![
                Payload::IdR { kind: ikev2::ID_IPV4_ADDR, data: my_id.octets().to_vec() },
                Payload::Auth { method: ikev2::AUTH_SHARED_KEY, data: my_auth },
                Payload::Sa(vec![esp]),
                Payload::TsI(tsi),
                Payload::TsR(tsr),
            ],
        };
        let suite = entry.as_ref().and_then(|e| esp_suite_for(d, e));
        let (k_ir, k_ri) = esp_keys(&keys, suite, &v2.ni, &v2.nr);
        v2.esp_out = k_ri;
        v2.esp_in = k_ir;
        v2.profile = Some(profile.name.clone());
        v2.esp_suite = suite;
        v2.next_req = 0;
        let spi_out = offer.as_ref().map_or(0, |o| spi_u32(&o.spi));
        let (now, life) = (self.now, entry.as_ref().map_or((0, 0), |e| d.crypto.lifetime(e)));
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.rekeyed(now, life, true);
        sa.phase = IkePhase::Established;
        sa.failed = None;
        sa.spi_in = spi_in;
        sa.spi_out = spi_out;
        sa.v2 = Some(Box::new(v2));
        self.log(d.id, &format!("IKEv2 with {peer}: IKE_SA_INIT and IKE_AUTH complete, IKE and IPsec security associations up"));
        self.ike2_send(t, d, local, peer, reply.encode_sealed(keys.suite.integ, &keys.sk_er, &keys.sk_ar, iv));
        Ok(())
    }

    fn ike2_auth_response(&mut self, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, raw: &[u8], mut v2: V2State) -> Result<(), String> {
        let Some(keys) = v2.keys.clone() else { return Err("IKEv2 keys missing".into()) };
        let inner = match Message::open(raw, keys.suite.integ, &keys.sk_er, &keys.sk_ar) {
            Ok(p) => p,
            Err(e) => {
                self.ike2_fail(d, peer, format!("the IKE_AUTH response does not open ({e})"));
                return Ok(());
            }
        };
        if let Some(kind) = Message::notify(&inner) {
            let why = match kind {
                ikev2::AUTHENTICATION_FAILED => {
                    "the peer refused this router's AUTH value (the pre-shared keys differ, or no profile there matches this router)"
                }
                ikev2::TS_UNACCEPTABLE => "the peer does not protect the same traffic (its access list doesn't mirror this one)",
                _ => "the peer has no matching ESP proposal or crypto map entry",
            };
            self.ike2_fail(d, peer, format!("{why}; the peer answered IKE_AUTH with {}", ikev2::notify_name(kind)));
            return Ok(());
        }
        let idr = inner.iter().find_map(|p| if let Payload::IdR { kind, data } = p { Some((*kind, data.clone())) } else { None });
        let auth = inner.iter().find_map(|p| if let Payload::Auth { data, .. } = p { Some(data.clone()) } else { None });
        let esp = inner.iter().find_map(|p| if let Payload::Sa(v) = p { v.first().cloned() } else { None });
        let their_key = v2.profile.as_deref().and_then(|n| d.crypto.ikev2.profile(n)).and_then(|p| d.crypto.ikev2.keys_for(p, peer)).map(|k| k.1);
        let ok = match (idr, auth, their_key) {
            (Some((k, data)), Some(auth), Some(key)) => {
                let mut body = vec![k, 0, 0, 0];
                body.extend_from_slice(&data);
                ikev2::psk_auth_with(keys.suite.prf, key.as_bytes(), &v2.msg2, &v2.ni, &keys.sk_pr, &body) == auth
            }
            _ => false,
        };
        if !ok {
            self.ike2_fail(d, peer, "the peer's AUTH value does not match this router's pre-shared key (the keys differ)".into());
            return Ok(());
        }
        let entry = entry_for(d, local, peer);
        let suite = entry.as_ref().and_then(|e| esp_suite_for(d, e));
        let (k_ir, k_ri) = esp_keys(&keys, suite, &v2.ni, &v2.nr);
        v2.esp_out = k_ir;
        v2.esp_in = k_ri;
        v2.esp_suite = suite;
        v2.next_req = 2;
        let (now, life) = (self.now, entry.as_ref().map_or((0, 0), |e| d.crypto.lifetime(e)));
        let sa = self.st(d.id).ipsec.entry(peer).or_default();
        sa.rekeyed(now, life, true);
        sa.phase = IkePhase::Established;
        sa.failed = None;
        sa.spi_out = esp.as_ref().map_or(0, |e| spi_u32(&e.spi));
        sa.local = local;
        sa.v2 = Some(Box::new(v2));
        self.log(d.id, &format!("IKEv2 with {peer}: IKE_SA_INIT and IKE_AUTH complete, IKE and IPsec security associations up"));
        Ok(())
    }

    /// Sends a request or response on an IKE SA that is up, sealed with this router's keys.
    #[allow(clippy::too_many_arguments)]
    fn ike2_sa_send(
        &mut self,
        t: &Topology,
        d: &Device,
        local: Ipv4Addr,
        peer: Ipv4Addr,
        v2: &V2State,
        initiator: bool,
        exchange: u8,
        id: u32,
        response: bool,
        payloads: Vec<Payload>,
    ) {
        let Some(keys) = &v2.keys else { return };
        let mut flags = if initiator { ikev2::FLAG_INITIATOR } else { 0 };
        if response {
            flags |= ikev2::FLAG_RESPONSE;
        }
        let m = Message { spi_i: v2.spi_i, spi_r: v2.spi_r, exchange, flags, message_id: id, payloads };
        let (ek, ak) = if initiator { (&keys.sk_ei, &keys.sk_ai) } else { (&keys.sk_er, &keys.sk_ar) };
        let iv = iv_for(&[&ek[..], &id.to_be_bytes()[..], &[exchange, u8::from(response)][..], &self.now.to_be_bytes()[..]].concat());
        let raw = m.encode_sealed(keys.suite.integ, ek, ak, iv);
        self.ike2_send(t, d, local, peer, raw);
    }

    /// CREATE_CHILD_SA request renewing the child SA's keys (RFC 7296 §1.3.3).
    fn ike2_rekey(&mut self, t: &Topology, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, entry: &CryptoMapEntry) {
        let now = self.now;
        let Some(sa) = self.state.get(&d.id).and_then(|s| s.ipsec.get(&peer)).cloned() else { return };
        let Some(mut v2) = sa.v2.as_deref().cloned() else { return };
        let seed = [&v2.spi_i[..], &v2.spi_r[..], &now.to_be_bytes()[..], b"child"].concat();
        let ni = filler(&[&seed[..], b"ni"].concat(), 32);
        let new_in = spi_of(&ni, local, peer);
        let id = v2.next_req;
        let Some((tsi, tsr)) = selectors(d, entry) else { return };
        let payloads = vec![
            Payload::Notify { protocol: ikev2::PROTO_ESP, spi: sa.spi_in.to_be_bytes().to_vec(), kind: ikev2::REKEY_SA, data: Vec::new() },
            Payload::Sa(vec![esp_proposal(d, entry, new_in)]),
            Payload::Nonce(ni.clone()),
            Payload::TsI(vec![tsi]),
            Payload::TsR(vec![tsr]),
        ];
        v2.child = Some((ni, new_in, id));
        v2.next_req = id.wrapping_add(1);
        self.log(d.id, &format!("IKEv2 with {peer}: the child SA nears the end of its lifetime; CREATE_CHILD_SA renews its keys"));
        self.ike2_sa_send(t, d, local, peer, &v2, sa.initiator, ikev2::CREATE_CHILD_SA, id, false, payloads);
        if let Some(s) = self.st(d.id).ipsec.get_mut(&peer) {
            s.rekey_us = now.max(1);
            s.v2 = Some(Box::new(v2));
        }
    }

    /// Installs a child SA's new keys (the old inbound SA kept until it is deleted).
    fn ike2_install(&mut self, d: &Device, local: Ipv4Addr, peer: Ipv4Addr, mut v2: V2State, spis: (u32, u32), keys: (Vec<u8>, Vec<u8>)) {
        let now = self.now;
        let life = entry_for(d, local, peer).map_or((0, 0), |e| d.crypto.lifetime(&e));
        let Some(sa) = self.st(d.id).ipsec.get_mut(&peer) else { return };
        sa.prev_in = Some((sa.spi_in, v2.esp_suite, v2.esp_in.clone()));
        (sa.spi_in, sa.spi_out) = spis;
        (v2.esp_out, v2.esp_in) = keys;
        v2.child = None;
        sa.rekeyed(now, life, false);
        sa.v2 = Some(Box::new(v2));
    }

    /// CREATE_CHILD_SA and INFORMATIONAL on an IKE SA that is up.
    #[allow(clippy::too_many_arguments)]
    fn ike2_on_sa(
        &mut self,
        t: &Topology,
        d: &Device,
        local: Ipv4Addr,
        peer: Ipv4Addr,
        m: &Message,
        raw: &[u8],
        v2: V2State,
        response: bool,
    ) -> Result<(), String> {
        let Some(keys) = v2.keys.clone() else { return Err("IKEv2 keys missing".into()) };
        let from_initiator = m.flags & ikev2::FLAG_INITIATOR != 0;
        let (ek, ak) = if from_initiator { (&keys.sk_ei, &keys.sk_ai) } else { (&keys.sk_er, &keys.sk_ar) };
        let inner = Message::open(raw, keys.suite.integ, ek, ak)
            .map_err(|e| format!("IKEv2 {} from {peer} does not open ({e})", ikev2::exchange_name(m.exchange)))?;
        let sa = self.state.get(&d.id).and_then(|s| s.ipsec.get(&peer)).cloned().unwrap_or_default();
        let me_initiator = sa.initiator;
        let nonce = inner.iter().find_map(|p| if let Payload::Nonce(n) = p { Some(n.clone()) } else { None });
        let esp = inner.iter().find_map(|p| if let Payload::Sa(v) = p { v.first().cloned() } else { None });
        match (m.exchange, response) {
            (ikev2::CREATE_CHILD_SA, false) => {
                let refuse = |kind: u16| vec![Payload::Notify { protocol: 0, spi: Vec::new(), kind, data: Vec::new() }];
                let rekeyed = inner.iter().find_map(|p| match p {
                    Payload::Notify { kind: ikev2::REKEY_SA, spi, .. } => Some(spi_u32(spi)),
                    _ => None,
                });
                let entry = entry_for(d, local, peer);
                let verdict = match (rekeyed, &entry, &esp, &nonce) {
                    (Some(s), _, _, _) if s != sa.spi_out => Err(ikev2::CHILD_SA_NOT_FOUND),
                    (None, _, _, _) => Err(ikev2::NO_PROPOSAL_CHOSEN),
                    (_, Some(e), Some(o), Some(_)) if esp_proposal(d, e, 0).transforms == o.transforms => Ok(()),
                    _ => Err(ikev2::NO_PROPOSAL_CHOSEN),
                };
                if let Err(kind) = verdict {
                    self.log(d.id, &format!("IKEv2 with {peer}: refused to renew the child SA ({})", ikev2::notify_name(kind)));
                    self.ike2_sa_send(t, d, local, peer, &v2, me_initiator, ikev2::CREATE_CHILD_SA, m.message_id, true, refuse(kind));
                    return Ok(());
                }
                let (Some(e), Some(o), Some(ni)) = (entry, esp, nonce) else { return Ok(()) };
                let nr = filler(&[&ni[..], &local.octets()[..], b"nr"].concat(), 32);
                let new_in = spi_of(&nr, local, peer);
                let ts_i = inner.iter().find_map(|p| if let Payload::TsI(v) = p { Some(v.clone()) } else { None }).unwrap_or_default();
                let ts_r = inner.iter().find_map(|p| if let Payload::TsR(v) = p { Some(v.clone()) } else { None }).unwrap_or_default();
                let mut mine = esp_proposal(d, &e, new_in);
                mine.number = o.number;
                // The requester's direction first (§2.17).
                let (k_req, k_resp) = esp_keys(&keys, v2.esp_suite, &ni, &nr);
                let reply = vec![Payload::Sa(vec![mine]), Payload::Nonce(nr), Payload::TsI(ts_i), Payload::TsR(ts_r)];
                self.ike2_sa_send(t, d, local, peer, &v2, me_initiator, ikev2::CREATE_CHILD_SA, m.message_id, true, reply);
                self.ike2_install(d, local, peer, v2, (new_in, spi_u32(&o.spi)), (k_resp, k_req));
                self.log(d.id, &format!("IKEv2 with {peer}: CREATE_CHILD_SA complete, the child SA has new keys"));
            }
            (ikev2::CREATE_CHILD_SA, true) => {
                let Some((ni, new_in, id)) = v2.child.clone().filter(|c| c.2 == m.message_id) else {
                    return Err(format!("CREATE_CHILD_SA response from {peer} to no request"));
                };
                if let Some(kind) = Message::notify(&inner) {
                    self.log(d.id, &format!("IKEv2 with {peer}: renewing the child SA failed: the peer answered {}", ikev2::notify_name(kind)));
                    if let Some(s) = self.st(d.id).ipsec.get_mut(&peer) {
                        s.rekey_us = 0;
                    }
                    return Ok(());
                }
                let (Some(nr), Some(o)) = (nonce, esp) else { return Err("CREATE_CHILD_SA response without SA or nonce".into()) };
                let (k_req, k_resp) = esp_keys(&keys, v2.esp_suite, &ni, &nr);
                let old_in = sa.spi_in;
                let mut v2 = v2;
                let del_id = v2.next_req;
                v2.next_req = del_id.wrapping_add(1);
                self.ike2_install(d, local, peer, v2.clone(), (new_in, spi_u32(&o.spi)), (k_req, k_resp));
                let _ = id;
                self.log(d.id, &format!("IKEv2 with {peer}: CREATE_CHILD_SA complete, the child SA has new keys; deleting the old one"));
                let del = vec![Payload::Delete { protocol: ikev2::PROTO_ESP, spis: vec![old_in.to_be_bytes().to_vec()] }];
                let v2 = self.state.get(&d.id).and_then(|s| s.ipsec.get(&peer)).and_then(|s| s.v2.as_deref().cloned()).unwrap_or(v2);
                self.ike2_sa_send(t, d, local, peer, &v2, me_initiator, ikev2::INFORMATIONAL, del_id, false, del);
            }
            (ikev2::INFORMATIONAL, false) => {
                // A Delete for the peer's old inbound SA (this router's old outbound one): the
                // answer deletes this router's old inbound SA in turn.
                let deletes: Vec<u32> = inner
                    .iter()
                    .filter_map(|p| {
                        if let Payload::Delete { protocol: ikev2::PROTO_ESP, spis } = p { Some(spis.iter().map(|s| spi_u32(s))) } else { None }
                    })
                    .flatten()
                    .collect();
                let mut reply = Vec::new();
                if !deletes.is_empty()
                    && let Some(s) = self.st(d.id).ipsec.get_mut(&peer)
                    && let Some(prev) = s.prev_in.take()
                {
                    reply.push(Payload::Delete { protocol: ikev2::PROTO_ESP, spis: vec![prev.0.to_be_bytes().to_vec()] });
                    self.log(d.id, &format!("IKEv2 with {peer}: the old child SA (inbound SPI 0x{:08x}) is deleted", prev.0));
                }
                self.ike2_sa_send(t, d, local, peer, &v2, me_initiator, ikev2::INFORMATIONAL, m.message_id, true, reply);
            }
            _ => {
                // The answer to this router's Delete: the old inbound SA goes too.
                if let Some(s) = self.st(d.id).ipsec.get_mut(&peer)
                    && let Some(prev) = s.prev_in.take()
                {
                    self.log(d.id, &format!("IKEv2 with {peer}: the old child SA (inbound SPI 0x{:08x}) is deleted", prev.0));
                }
            }
        }
        Ok(())
    }

    /// ESP for a peer whose association IKEv2 set up: `None` when it isn't one.
    pub(crate) fn esp_in_v2(
        &mut self,
        t: &Topology,
        d: &Device,
        ifc: &L3If,
        ip: &Ipv4,
        esp: &netlabcraft_wire::vpn::Esp,
    ) -> Option<Result<(), String>> {
        let sa = self.state.get(&d.id).and_then(|s| s.ipsec.get(&ip.src))?;
        let v2 = sa.v2.as_ref()?;
        if sa.phase != IkePhase::Established {
            return None;
        }
        let prev = sa.prev_in.clone().filter(|p| p.0 == esp.spi);
        let opened = if esp.spi != sa.spi_in && prev.is_none() {
            Err(format!("ESP SPI 0x{:08x} from {} matches no security association", esp.spi, ip.src))
        } else {
            let (suite, key) = prev.map_or((v2.esp_suite, v2.esp_in.clone()), |p| (p.1, p.2));
            match esp_open(suite, &key, esp) {
                Ok((netlabcraft_wire::vpn::NEXT_IPV4, inner)) => Ok(inner),
                Ok((next, _)) => Err(format!("ESP carrying next header {next} is not understood")),
                Err(_) => Err(format!("ESP from {} failed its integrity check", ip.src)),
            }
        };
        let sa = self.st(d.id).ipsec.entry(ip.src).or_default();
        Some(match opened {
            Ok(inner) => {
                sa.decaps = sa.decaps.saturating_add(1);
                sa.bytes = sa.bytes.saturating_add(u64::try_from(inner.len()).unwrap_or(u64::MAX));
                self.ip_in(t, d, ifc, &inner)
            }
            Err(e) => {
                sa.drops = sa.drops.saturating_add(1);
                Err(e)
            }
        })
    }
}

/// The KE data length for a group (bytes of its public value).
fn ke_len(group: u8) -> usize {
    match group {
        2 => 128,
        5 => 192,
        15 => 384,
        16 => 512,
        19 => 64,
        20 => 96,
        _ => 256,
    }
}

/// An inbound SPI this router picks: from its nonce and both addresses, never below 256.
fn spi_of(nonce: &[u8], local: Ipv4Addr, peer: Ipv4Addr) -> u32 {
    let h = netlabcraft_wire::md5::md5(&[nonce, &local.octets()[..], &peer.octets()[..]].concat());
    u32::from_be_bytes([h[0], h[1], h[2], h[3]]) | 0x100
}
