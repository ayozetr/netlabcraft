//! IKEv1 keys and ciphers (RFC 2409) for the exchange `crate::vpn` runs: the policy a
//! transform stands for, the Diffie-Hellman values, SKEYID and its three children for a
//! pre-shared key (§5, §5.4), HASH_I and HASH_R, the quick-mode hashes and KEYMAT (§5.5), the
//! encryption key and IVs of appendix B, and sealing or opening a message with them.
//!
//! AES policies (`encryption aes [128|192|256]`) encrypt with AES-CBC for real; DES and 3DES,
//! not implemented, use the stand-in keystream of `netlabcraft_wire::ike` keyed with the real
//! SKEYID_e. SHA-384 and SHA-512 policies run HMAC-SHA-256 in their place.

use netlabcraft_wire::aes256::Aes;
use netlabcraft_wire::dh;
use netlabcraft_wire::ike::{self, Isakmp, Payload, Transform};
use netlabcraft_wire::suite::Prf;
use netlabcraft_wire::vpn::EspSuite;

/// The phase-1 cipher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P1Cipher {
    /// AES-CBC with a key of this many bytes.
    Aes(usize),
    /// DES or 3DES: the stand-in keystream.
    Standin,
}

/// What a phase-1 transform stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct P1Suite {
    pub cipher: P1Cipher,
    pub prf: Prf,
    pub group: u16,
    /// Seconds (attribute 12).
    pub lifetime: u32,
}

fn attr(t: &Transform, ty: u16) -> Option<u16> {
    t.attrs.iter().find(|(k, _)| *k == ty).map(|(_, v)| *v)
}

impl P1Suite {
    /// From a transform's attributes (RFC 2409 appendix A).
    pub fn from_transform(t: &Transform) -> P1Suite {
        let cipher = match attr(t, 1) {
            Some(7) => {
                let bits = attr(t, 14).unwrap_or(128);
                if [128, 192, 256].contains(&bits) { P1Cipher::Aes(usize::from(bits) / 8) } else { P1Cipher::Aes(16) }
            }
            _ => P1Cipher::Standin,
        };
        let prf = match attr(t, 2) {
            Some(1) => Prf::HmacMd5,
            Some(2) | None => Prf::HmacSha1,
            _ => Prf::HmacSha256,
        };
        P1Suite { cipher, prf, group: attr(t, 4).unwrap_or(1), lifetime: attr(t, 12).map_or(86_400, u32::from) }
    }

    pub fn describe(&self) -> String {
        let c = match self.cipher {
            P1Cipher::Aes(n) => format!("AES-CBC-{}", n * 8),
            P1Cipher::Standin => "DES/3DES (stand-in cipher)".to_string(),
        };
        let dh = if dh::is_modp(self.group) { format!("DH group {}", self.group) } else { format!("DH group {} (stand-in)", self.group) };
        format!("{c}, {}, {dh}", self.prf.name())
    }
}

/// Whether two transforms propose the same policy (lifetimes may differ).
pub fn same_policy(a: &Transform, b: &Transform) -> bool {
    let key = |t: &Transform| {
        let mut v: Vec<(u16, u16)> = t.attrs.iter().copied().filter(|(k, _)| *k != 11 && *k != 12).collect();
        v.sort_unstable();
        v
    };
    key(a) == key(b)
}

/// Whether a transform authenticates with pre-shared keys (attribute 3 = 1).
pub fn pre_shared(t: &Transform) -> bool {
    attr(t, 3) == Some(1)
}

/// Everything one IKEv1 exchange keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V1State {
    pub suite: Option<P1Suite>,
    dh_priv: Vec<u8>,
    pub ke_i: Vec<u8>,
    pub ke_r: Vec<u8>,
    pub ni: Vec<u8>,
    pub nr: Vec<u8>,
    /// The body of the initiator's SA payload (HASH_I and HASH_R cover it).
    pub sa_b: Vec<u8>,
    skeyid: Vec<u8>,
    skeyid_d: Vec<u8>,
    skeyid_a: Vec<u8>,
    enc_key: Vec<u8>,
    /// The CBC IV of the next phase-1 message.
    iv: Vec<u8>,
    /// The last cipher block of phase 1 (quick mode's IVs start from it).
    p1_last: Vec<u8>,
    /// The CBC IV of the next quick-mode message.
    qm_iv: Vec<u8>,
    pub qm_ni: Vec<u8>,
    pub qm_nr: Vec<u8>,
    /// The inbound SPI this router chose and the peer's.
    pub spi_mine: u32,
    pub spi_peer: u32,
    /// The ESP algorithms run for real (`None`: the stand-in cipher), and the keys.
    pub esp_suite: Option<EspSuite>,
    pub esp_out: Vec<u8>,
    pub esp_in: Vec<u8>,
}

/// Why opening a message failed.
pub const OPEN_FAILED: &str = "does not decrypt and authenticate with this router's keys";

impl V1State {
    /// A new exchange: the phase-1 policy when known, and the initiator's SA payload body.
    pub fn new(suite: Option<P1Suite>, sa_b: Vec<u8>) -> V1State {
        V1State { suite, sa_b, ..V1State::default() }
    }

    /// Picks the private value and makes the public one (g^x mod p, or filler for a group that
    /// isn't computed for real); the public value.
    pub fn make_ke(&mut self, seed: &[u8]) -> Vec<u8> {
        let group = self.suite.map_or(1, |s| s.group);
        self.dh_priv = ike::filler(&[seed, b"dh-x"].concat(), 32);
        match dh::modp(group) {
            Some(g) => g.public(&self.dh_priv),
            None => ike::filler(&[seed, b"ke"].concat(), 128),
        }
    }

    /// The shared secret from the peer's public value; `None` when it isn't valid.
    fn shared(&self, initiator: bool) -> Option<Vec<u8>> {
        let group = self.suite.map_or(1, |s| s.group);
        let peer = if initiator { &self.ke_r } else { &self.ke_i };
        match dh::modp(group) {
            Some(g) => g.shared(&self.dh_priv, peer),
            None => Some(netlabcraft_wire::sha1::sha1(&[&self.ke_i[..], &self.ke_r[..]].concat()).to_vec()),
        }
    }

    fn prf(&self) -> Prf {
        self.suite.map_or(Prf::HmacSha1, |s| s.prf)
    }

    /// SKEYID = prf(pre-shared key, Ni_b | Nr_b), then SKEYID_d, _a and _e (§5), the cipher key
    /// from SKEYID_e (appendix B) and the first IV, hash(g^xi | g^xr). `false` when the peer's
    /// Diffie-Hellman value isn't valid.
    pub fn derive(&mut self, psk: &[u8], icookie: &[u8; 8], rcookie: &[u8; 8], initiator: bool) -> bool {
        let Some(gxy) = self.shared(initiator) else { return false };
        let prf = self.prf();
        self.skeyid = prf.prf(psk, &[&self.ni[..], &self.nr[..]].concat());
        let tail = |n: u8| [&gxy[..], &icookie[..], &rcookie[..], &[n][..]].concat();
        self.skeyid_d = prf.prf(&self.skeyid, &tail(0));
        self.skeyid_a = prf.prf(&self.skeyid, &[&self.skeyid_d[..], &tail(1)].concat());
        let skeyid_e = prf.prf(&self.skeyid, &[&self.skeyid_a[..], &tail(2)].concat());
        let want = match self.suite.map(|s| s.cipher) {
            Some(P1Cipher::Aes(n)) => n,
            _ => skeyid_e.len(),
        };
        // Appendix B: a key longer than SKEYID_e is K1 | K2 | …, K1 = prf(SKEYID_e, 0),
        // Kn = prf(SKEYID_e, Kn−1).
        self.enc_key = if skeyid_e.len() >= want {
            skeyid_e.get(..want).unwrap_or_default().to_vec()
        } else {
            let mut k = prf.prf(&skeyid_e, &[0]);
            let mut out = k.clone();
            while out.len() < want {
                k = prf.prf(&skeyid_e, &k);
                out.extend_from_slice(&k);
            }
            out.truncate(want);
            out
        };
        self.iv = prf.hash(&[&self.ke_i[..], &self.ke_r[..]].concat()).get(..16).unwrap_or_default().to_vec();
        true
    }

    /// HASH_I (`initiator`) or HASH_R over the sender's identification payload body (§5).
    pub fn hash_p1(&self, initiator: bool, icookie: &[u8; 8], rcookie: &[u8; 8], id_body: &[u8]) -> Vec<u8> {
        let data = if initiator {
            [&self.ke_i[..], &self.ke_r[..], &icookie[..], &rcookie[..], &self.sa_b[..], id_body].concat()
        } else {
            [&self.ke_r[..], &self.ke_i[..], &rcookie[..], &icookie[..], &self.sa_b[..], id_body].concat()
        };
        self.prf().prf(&self.skeyid, &data)
    }

    /// HASH(1) or HASH(2) of quick mode (§5.5): over the message ID, for HASH(2) the
    /// initiator's nonce, then every payload after the hash with its header.
    pub fn hash_qm(&self, message_id: u32, ni: Option<&[u8]>, rest: &[Payload]) -> Vec<u8> {
        let data = [&message_id.to_be_bytes()[..], ni.unwrap_or_default(), &ike::chain(rest)].concat();
        self.prf().prf(&self.skeyid_a, &data)
    }

    /// HASH(3): prf(SKEYID_a, 0 | M-ID | Ni_b | Nr_b).
    pub fn hash_qm3(&self, message_id: u32) -> Vec<u8> {
        self.prf().prf(&self.skeyid_a, &[&[0u8][..], &message_id.to_be_bytes()[..], &self.qm_ni[..], &self.qm_nr[..]].concat())
    }

    /// KEYMAT for the SA whose (inbound) SPI is `spi` (§5.5): K1 = prf(SKEYID_d, protocol |
    /// SPI | Ni_b | Nr_b), Kn = prf(SKEYID_d, Kn−1 | protocol | SPI | Ni_b | Nr_b).
    pub fn keymat(&self, spi: u32, len: usize) -> Vec<u8> {
        let seed = [&[ike::PROTO_ESP][..], &spi.to_be_bytes()[..], &self.qm_ni[..], &self.qm_nr[..]].concat();
        let prf = self.prf();
        let mut k = prf.prf(&self.skeyid_d, &seed);
        let mut out = k.clone();
        while out.len() < len.min(1024) {
            k = prf.prf(&self.skeyid_d, &[&k[..], &seed[..]].concat());
            out.extend_from_slice(&k);
        }
        out.truncate(len);
        out
    }

    fn iv16(v: &[u8]) -> [u8; 16] {
        v.get(..16).and_then(|s| s.try_into().ok()).unwrap_or([0; 16])
    }

    /// Seals a message with the phase-1 cipher; quick mode (`qm`) chains its own IV.
    pub fn seal(&mut self, icookie: [u8; 8], rcookie: [u8; 8], exchange: u8, message_id: u32, payloads: &[Payload], qm: bool) -> Isakmp {
        match (self.suite.map(|s| s.cipher), Aes::new(&self.enc_key)) {
            (Some(P1Cipher::Aes(_)), Some(aes)) => {
                let iv = V1State::iv16(if qm { &self.qm_iv } else { &self.iv });
                let m = Isakmp::sealed_cbc(icookie, rcookie, exchange, message_id, payloads, &aes, &iv);
                self.chain_iv(&m, qm);
                m
            }
            _ => Isakmp::sealed(icookie, rcookie, exchange, message_id, payloads, &self.enc_key),
        }
    }

    fn chain_iv(&mut self, m: &Isakmp, qm: bool) {
        if let Some(last) = m.last_block() {
            if qm {
                self.qm_iv = last.to_vec();
            } else {
                self.iv = last.to_vec();
                self.p1_last = last.to_vec();
            }
        }
    }

    /// Opens a message sealed with the phase-1 cipher.
    pub fn open(&mut self, m: &Isakmp, qm: bool) -> Result<Vec<Payload>, String> {
        let opened = match (self.suite.map(|s| s.cipher), Aes::new(&self.enc_key)) {
            (Some(P1Cipher::Aes(_)), Some(aes)) => {
                let iv = V1State::iv16(if qm { &self.qm_iv } else { &self.iv });
                let r = m.open_cbc(&aes, &iv);
                self.chain_iv(m, qm);
                r
            }
            _ => m.open_unchecked(&self.enc_key),
        };
        opened.map_err(|_| OPEN_FAILED.to_string())
    }

    /// Starts quick mode's IV: hash(last phase-1 block | M-ID).
    pub fn start_qm(&mut self, message_id: u32) {
        let last = if self.p1_last.is_empty() { self.iv.clone() } else { self.p1_last.clone() };
        self.qm_iv = self.prf().hash(&[&last[..], &message_id.to_be_bytes()[..]].concat()).get(..16).unwrap_or_default().to_vec();
    }
}

/// The HASH payload of a list, if any.
pub fn hash_of(list: &[Payload]) -> Option<&[u8]> {
    list.iter().find_map(|p| if let Payload::Hash(h) = p { Some(h.as_slice()) } else { None })
}

/// The payloads after the first HASH payload.
pub fn after_hash(list: &[Payload]) -> &[Payload] {
    match list.iter().position(|p| matches!(p, Payload::Hash(_))) {
        Some(i) => list.get(i + 1..).unwrap_or_default(),
        None => list,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(attrs: Vec<(u16, u16)>) -> Transform {
        Transform { number: 1, id: 1, attrs }
    }

    fn pair(cipher: (u16, u16), hash: u16, group: u16) -> (V1State, V1State) {
        let t = tr(vec![(1, cipher.0), (14, cipher.1), (2, hash), (3, 1), (4, group), (11, 1), (12, 3600)]);
        let s = P1Suite::from_transform(&t);
        let mut i = V1State { suite: Some(s), ni: vec![1; 16], sa_b: vec![9; 40], ..V1State::default() };
        let mut r = V1State { suite: Some(s), nr: vec![2; 16], sa_b: vec![9; 40], ..V1State::default() };
        i.ke_i = i.make_ke(b"i");
        r.ke_r = r.make_ke(b"r");
        r.ke_i = i.ke_i.clone();
        i.ke_r = r.ke_r.clone();
        r.ni = i.ni.clone();
        i.nr = r.nr.clone();
        (i, r)
    }

    #[test]
    fn both_ends_derive_the_same_keys_and_hashes() {
        for (cipher, hash, group) in [((7, 128), 2, 14), ((7, 256), 4, 5), ((7, 192), 1, 2), ((1, 0), 2, 1), ((5, 0), 2, 19)] {
            let (mut i, mut r) = pair(cipher, hash, group);
            let (ci, cr) = ([1; 8], [2; 8]);
            assert!(i.derive(b"psk", &ci, &cr, true) && r.derive(b"psk", &ci, &cr, false));
            assert_eq!(i.enc_key, r.enc_key);
            if let Some(P1Cipher::Aes(n)) = i.suite.map(|s| s.cipher) {
                assert_eq!(i.enc_key.len(), n);
            }
            assert_eq!(i.hash_p1(true, &ci, &cr, b"id"), r.hash_p1(true, &ci, &cr, b"id"));
            assert_ne!(i.hash_p1(true, &ci, &cr, b"id"), i.hash_p1(false, &ci, &cr, b"id"));
            // Message 5 one way, 6 the other, then quick mode, with the IVs chained.
            let id = Payload::Id { kind: 1, protocol: 17, port: 500, data: vec![10, 0, 0, 1] };
            let m5 = i.seal(ci, cr, ike::EXCH_MAIN, 0, &[id.clone(), Payload::Hash(vec![3; 20])], false);
            assert_eq!(r.open(&Isakmp::decode(&m5.encode()).unwrap(), false).unwrap()[0], id);
            let m6 = r.seal(ci, cr, ike::EXCH_MAIN, 0, &[id.clone(), Payload::Hash(vec![4; 20])], false);
            assert_eq!(i.open(&Isakmp::decode(&m6.encode()).unwrap(), false).unwrap()[0], id);
            i.start_qm(77);
            r.start_qm(77);
            let q1 = i.seal(ci, cr, ike::EXCH_QUICK, 77, &[Payload::Hash(vec![5; 20]), Payload::Nonce(vec![6; 16])], true);
            assert_eq!(r.open(&Isakmp::decode(&q1.encode()).unwrap(), true).unwrap()[1], Payload::Nonce(vec![6; 16]));
            i.qm_ni = vec![1; 16];
            i.qm_nr = vec![2; 16];
            r.qm_ni = vec![1; 16];
            r.qm_nr = vec![2; 16];
            assert_eq!(i.keymat(0x1234, 64), r.keymat(0x1234, 64));
            assert_ne!(i.keymat(0x1234, 64), i.keymat(0x1235, 64));
            assert_eq!(i.hash_qm3(77), r.hash_qm3(77));
            // A different pre-shared key: other keys, and message 5 doesn't open to the same.
            let (mut i2, mut r2) = pair(cipher, hash, group);
            assert!(i2.derive(b"psk", &ci, &cr, true) && r2.derive(b"other", &ci, &cr, false));
            assert_ne!(i2.enc_key, r2.enc_key);
            let m5 = i2.seal(ci, cr, ike::EXCH_MAIN, 0, &[id.clone(), Payload::Hash(vec![3; 20])], false);
            assert_ne!(r2.open(&m5, false).ok().and_then(|l| l.first().cloned()), Some(id.clone()));
        }
        // An invalid public value is refused.
        let (mut i, _) = pair((7, 128), 2, 14);
        i.ke_r = vec![0; 256];
        assert!(!i.derive(b"psk", &[1; 8], &[2; 8], true));
    }

    #[test]
    fn policies_compare_without_lifetimes() {
        let a = tr(vec![(1, 7), (14, 256), (2, 2), (3, 1), (4, 14), (11, 1), (12, 3600)]);
        let b = tr(vec![(1, 7), (14, 256), (2, 2), (3, 1), (4, 14), (11, 1), (12, 28_800)]);
        let c = tr(vec![(1, 7), (14, 128), (2, 2), (3, 1), (4, 14), (11, 1), (12, 3600)]);
        assert!(same_policy(&a, &b) && !same_policy(&a, &c) && pre_shared(&a));
        let s = P1Suite::from_transform(&a);
        assert_eq!((s.cipher, s.prf, s.group, s.lifetime), (P1Cipher::Aes(32), Prf::HmacSha1, 14, 3600));
        assert_eq!(s.describe(), "AES-CBC-256, HMAC-SHA1, DH group 14");
        assert_eq!(
            P1Suite::from_transform(&tr(vec![(1, 5), (2, 1), (4, 20)])).describe(),
            "DES/3DES (stand-in cipher), HMAC-MD5, DH group 20 (stand-in)"
        );
    }
}
