//! RSA (RFC 8017): key generation from a seeded generator, and the RSASSA-PKCS1-v1_5
//! signature scheme (§8.2) with the EMSA-PKCS1-v1_5 encoding (§9.2) over SHA-256 or SHA-1.
//!
//! Primes are found by a random start and a bounded upward search: trial division by the small
//! primes, then a bounded number of Miller-Rabin rounds over [`Big`] with the Montgomery code of
//! [`crate::dh`]. Everything is capped (candidates per window, windows, rounds), so key
//! generation always ends, with an error if the budget runs out. The public exponent is 65537.
//!
//! Random bits come from [`Drbg`] (SHA-256 in counter mode) seeded by the caller: the simulator
//! seeds it from the device and the key label, so a lab replays the same keys. That is right for
//! teaching and never for protecting real data. The arithmetic is not constant-time and the
//! private operation does not use the Chinese remainder theorem; 512 to 2048-bit moduli are
//! accepted (multiples of 64 bits); measured here, a 2048-bit key is generated, used to sign and
//! verified in about 0.1 s with the test profile, so every size is allowed.

use crate::dh::{Big, ModMul, mod_pow};
use crate::sha1::sha1;
use crate::sha256::sha256;
use crate::tls::Drbg;
use crate::{Result, WireError};

/// The public exponent, F4.
pub const E: u64 = 65_537;
/// Smallest and largest modulus size (bits) this module generates or accepts.
pub const MIN_BITS: usize = 512;
pub const MAX_BITS: usize = 2048;
/// Candidates examined in one upward search before starting again somewhere else.
const WINDOW: usize = 4096;
/// Searches started for one prime before giving up.
const MAX_WINDOWS: usize = 48;
/// Miller-Rabin rounds (bases 2, 3, 5, 7, 11, 13).
const ROUNDS: usize = 6;
const BASES: [u64; ROUNDS] = [2, 3, 5, 7, 11, 13];

/// The hash a signature covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigHash {
    Sha1,
    Sha256,
}

impl SigHash {
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            SigHash::Sha1 => sha1(data).to_vec(),
            SigHash::Sha256 => sha256(data).to_vec(),
        }
    }

    /// The DER `DigestInfo` header before the hash (RFC 8017 §9.2, note 1).
    fn prefix(self) -> &'static [u8] {
        match self {
            SigHash::Sha1 => &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14],
            SigHash::Sha256 => &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20],
        }
    }
}

fn err<T>(why: &str) -> Result<T> {
    Err(WireError::Unsupported("rsa", why.to_string()))
}

/// Whether a modulus size is one this module handles.
pub fn bits_ok(bits: usize) -> bool {
    (MIN_BITS..=MAX_BITS).contains(&bits) && bits.is_multiple_of(64)
}

/// The odd primes below 2000, for trial division.
fn small_primes() -> Vec<u64> {
    (3u64..2000).filter(|n| (2..).take_while(|d| d * d <= *n).all(|d| n % d != 0)).collect()
}

/// Miller-Rabin over the fixed bases: `true` when `n` (odd, above 3) passes every round.
fn probably_prime(n: &Big) -> bool {
    let Some(nm1) = n.sub(&Big::from_u64(1)) else { return false };
    let Some(mm) = ModMul::new(n) else { return false };
    // n − 1 = d · 2^s with d odd.
    let mut s = 0usize;
    while !nm1.bit(s) && s < nm1.bits() {
        s += 1;
    }
    let d = nm1.shr(s);
    let one = Big::from_u64(1);
    for base in BASES {
        let a = Big::from_u64(base);
        if &a >= n {
            continue;
        }
        let Some(mut x) = mod_pow(&a, &d, n) else { return false };
        if x == one || x == nm1 {
            continue;
        }
        let mut witness = true;
        for _ in 1..s {
            x = mm.mul(&x, &x);
            if x == nm1 {
                witness = false;
                break;
            }
            if x == one {
                break;
            }
        }
        if witness {
            return false;
        }
    }
    true
}

/// A prime of exactly `bits` bits with its top two bits set (so a product of two has the full
/// length) and `(p − 1)` not divisible by [`E`].
fn gen_prime(bits: usize, rng: &mut Drbg, small: &[u64]) -> Result<Big> {
    let nbytes = bits / 8;
    for _ in 0..MAX_WINDOWS {
        let mut raw = rng.bytes(nbytes);
        if let Some(first) = raw.first_mut() {
            *first |= 0xC0;
        }
        if let Some(last) = raw.last_mut() {
            *last |= 1;
        }
        let mut cand = Big::from_be_bytes(&raw);
        let two = Big::from_u64(2);
        for _ in 0..WINDOW {
            if cand.bits() != bits {
                break;
            }
            let sieve_ok = small.iter().all(|p| cand.rem_small(*p) != 0);
            if sieve_ok && cand.sub(&Big::from_u64(1)).is_some_and(|m| m.rem_small(E) != 0) && probably_prime(&cand) {
                return Ok(cand);
            }
            cand = cand.add(&two);
        }
    }
    err("prime search ran out of its budget")
}

/// Inverse of `a` modulo the prime `m` by the extended Euclidean algorithm in `i128`.
fn inv_mod_small(a: u64, m: u64) -> Option<u64> {
    let (mut r0, mut r1) = (i128::from(m), i128::from(a % m));
    let (mut t0, mut t1) = (0i128, 1i128);
    while r1 != 0 {
        let q = r0 / r1;
        (r0, r1) = (r1, r0 - q * r1);
        (t0, t1) = (t1, t0 - q * t1);
    }
    if r0 != 1 {
        return None;
    }
    u64::try_from(t0.rem_euclid(i128::from(m))).ok()
}

/// An RSA public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsaPublicKey {
    n: Big,
    e: u64,
}

impl RsaPublicKey {
    /// From a big-endian modulus and an exponent; refuses sizes outside the supported range, an
    /// even modulus and an exponent below 3 or even.
    pub fn new(n: &[u8], e: u64) -> Result<RsaPublicKey> {
        let n = Big::from_be_bytes(n);
        if !bits_ok(n.bits().div_ceil(64) * 64) || n.bits() < MIN_BITS || n.is_even() || e < 3 || e.is_multiple_of(2) {
            return err("modulus or exponent not acceptable");
        }
        Ok(RsaPublicKey { n, e })
    }

    pub fn bits(&self) -> usize {
        self.n.bits()
    }

    /// Bytes in the modulus (and in a signature).
    pub fn len(&self) -> usize {
        self.n.bits().div_ceil(8)
    }

    pub fn is_empty(&self) -> bool {
        self.n.is_zero()
    }

    /// The modulus, big-endian, as many bytes as it has.
    pub fn modulus(&self) -> Vec<u8> {
        self.n.to_be_bytes(self.len())
    }

    pub fn exponent(&self) -> u64 {
        self.e
    }

    /// RSASSA-PKCS1-v1_5-VERIFY (RFC 8017 §8.2.2): the signature must be exactly as long as the
    /// modulus and below it, and its public-key image must equal the expected encoding of the
    /// message's hash byte for byte.
    pub fn verify(&self, hash: SigHash, msg: &[u8], sig: &[u8]) -> bool {
        let k = self.len();
        if sig.len() != k {
            return false;
        }
        let s = Big::from_be_bytes(sig);
        if s >= self.n {
            return false;
        }
        let Some(m) = mod_pow(&s, &Big::from_u64(self.e), &self.n) else { return false };
        match emsa_encode(hash, msg, k) {
            Some(em) => m.to_be_bytes(k) == em,
            None => false,
        }
    }
}

/// EMSA-PKCS1-v1_5 (RFC 8017 §9.2): `00 01 FF.. 00 DigestInfo`, `k` bytes. `None` when the
/// modulus is too short for the hash.
fn emsa_encode(hash: SigHash, msg: &[u8], k: usize) -> Option<Vec<u8>> {
    let t: Vec<u8> = hash.prefix().iter().copied().chain(hash.digest(msg)).collect();
    let pad = k.checked_sub(t.len())?.checked_sub(3)?;
    if pad < 8 {
        return None;
    }
    let mut em = vec![0x00, 0x01];
    em.extend(std::iter::repeat_n(0xFF, pad));
    em.push(0x00);
    em.extend_from_slice(&t);
    Some(em)
}

/// An RSA key pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsaKey {
    public: RsaPublicKey,
    d: Big,
    p: Big,
    q: Big,
}

impl RsaKey {
    /// A key of `bits` (a multiple of 64 between [`MIN_BITS`] and [`MAX_BITS`]) from the seeded
    /// generator; deterministic for a seed.
    pub fn generate(bits: usize, rng: &mut Drbg) -> Result<RsaKey> {
        if !bits_ok(bits) {
            return err("modulus size not supported");
        }
        let small = small_primes();
        let p = gen_prime(bits / 2, rng, &small)?;
        // A second, different prime (the odds of a repeat are nil, but the loop is bounded).
        let mut q = gen_prime(bits / 2, rng, &small)?;
        for _ in 0..4 {
            if q != p {
                break;
            }
            q = gen_prime(bits / 2, rng, &small)?;
        }
        RsaKey::from_primes(&p.to_be_bytes(bits / 16), &q.to_be_bytes(bits / 16))
    }

    /// The key whose modulus is the product of two primes (big-endian), for a stored key: the
    /// primes are checked again (sieve and Miller-Rabin), must differ and give a supported size.
    pub fn from_primes(p: &[u8], q: &[u8]) -> Result<RsaKey> {
        let (p, q) = (Big::from_be_bytes(p), Big::from_be_bytes(q));
        let small = small_primes();
        for x in [&p, &q] {
            let bad =
                x.bits() < MIN_BITS / 2 || x.bits() > MAX_BITS / 2 || x.is_even() || small.iter().any(|s| x.rem_small(*s) == 0) || !probably_prime(x);
            if bad {
                return err("not a prime");
            }
        }
        if p == q {
            return err("equal primes");
        }
        let n = p.mul(&q);
        if !bits_ok(n.bits()) {
            return err("modulus size not supported");
        }
        let (Some(pm), Some(qm)) = (p.sub(&Big::from_u64(1)), q.sub(&Big::from_u64(1))) else {
            return err("not a prime");
        };
        let phi = pm.mul(&qm);
        // d = (1 + k·φ) / e with k = −φ⁻¹ mod e, so no general division is needed (e is prime).
        let r = phi.rem_small(E);
        let Some(inv) = inv_mod_small(r, E) else { return err("exponent not coprime") };
        let k = (E - inv) % E;
        let d = phi.mul(&Big::from_u64(k)).add(&Big::from_u64(1)).div_small(E);
        Ok(RsaKey { public: RsaPublicKey { n, e: E }, d, p, q })
    }

    pub fn public(&self) -> &RsaPublicKey {
        &self.public
    }

    pub fn bits(&self) -> usize {
        self.public.bits()
    }

    /// The two primes, big-endian, each half the modulus long, for storing the key.
    pub fn primes(&self) -> (Vec<u8>, Vec<u8>) {
        let half = self.public.len().div_ceil(2);
        (self.p.to_be_bytes(half), self.q.to_be_bytes(half))
    }

    /// RSASSA-PKCS1-v1_5-SIGN (RFC 8017 §8.2.1): `None` when the modulus is too short for the
    /// hash.
    pub fn sign(&self, hash: SigHash, msg: &[u8]) -> Option<Vec<u8>> {
        let k = self.public.len();
        let em = emsa_encode(hash, msg, k)?;
        let s = mod_pow(&Big::from_be_bytes(&em), &self.d, &self.public.n)?;
        Some(s.to_be_bytes(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn digest_info_encodings_match_the_standard() {
        // SHA-256("abc") and SHA-1("abc") are the FIPS 180 examples; the prefixes are RFC 8017 §9.2.
        let em = emsa_encode(SigHash::Sha256, b"abc", 64).unwrap();
        assert_eq!(&em[..2], &[0, 1]);
        assert!(em[2..em.len() - 52].iter().all(|b| *b == 0xff) && em[em.len() - 52] == 0);
        assert_eq!(
            hex(&em[em.len() - 51..]),
            "3031300d060960864801650304020105000420ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let em = emsa_encode(SigHash::Sha1, b"abc", 64).unwrap();
        assert_eq!(hex(&em[em.len() - 35..]), "3021300906052b0e03021a05000414a9993e364706816aba3e25717850c26c9cd0d89d");
        // Too short a modulus (needs 8 bytes of padding and 3 of framing) is refused.
        assert!(emsa_encode(SigHash::Sha256, b"abc", 61).is_none());
        assert!(emsa_encode(SigHash::Sha256, b"abc", 62).is_some());
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn a_fixed_key_gives_the_expected_signatures() {
        // Two 256-bit primes, and the signatures of "abc" and "" computed independently with
        // Python big integers following RFC 8017 §8.2.1 and §9.2 (PKCS#1 v1.5 signatures are
        // deterministic, so the bytes are a known answer).
        let p = unhex("c00000000000000000000000000000000123456789abcdef0123456789abce45");
        let q = unhex("d000000000000000000000000000000000fedcba987654321fedcba987654381");
        let key = RsaKey::from_primes(&p, &q).unwrap();
        assert_eq!(
            hex(&key.public().modulus()),
            "9c00000000000000000000000000000001abcdf012345677c8df012345678a38d00121fa00ad77d7423578729b6a56d9172ff18471407a47393fdf501ac7ffc5"
        );
        let want256 =
            "537feafc80c075c139923d3e9b52ab03e4c07ff37b516eb2c8281debd471ed83e78fce709c5d809cc8c236c35c7d03d5428d1daafa514f2ffaca39adcdbff54c";
        let want1 =
            "0f0b4f58a7f2d0e30275f82de5e10a15f730867686ac7d22c65f2c60f9f4de2f3addb5cf934dbfccbb8384454d829b96f19a32a815acd731223cc36e7dfaa2a4";
        let wantempty =
            "597140b8b53963aa2a368f0f901bf2caa7bcc203a2fec8c542c20234687db6caf3525b8be0ed53ff97e33ea7718b008bd299e44839a791697c02a173b1e6d7b6";
        assert_eq!(hex(&key.sign(SigHash::Sha256, b"abc").unwrap()), want256);
        assert_eq!(hex(&key.sign(SigHash::Sha1, b"abc").unwrap()), want1);
        assert_eq!(hex(&key.sign(SigHash::Sha256, b"").unwrap()), wantempty);
        assert!(key.public().verify(SigHash::Sha256, b"abc", &unhex(want256)));
        assert!(key.public().verify(SigHash::Sha1, b"abc", &unhex(want1)));
    }

    #[test]
    fn generated_keys_have_the_requested_size_and_sign_and_verify() {
        for bits in [512usize, 1024] {
            let mut rng = Drbg::new(b"rsa test seed");
            let key = RsaKey::generate(bits, &mut rng).unwrap();
            assert_eq!(key.bits(), bits);
            assert_eq!(key.public().exponent(), 65_537);
            for hash in [SigHash::Sha256, SigHash::Sha1] {
                let sig = key.sign(hash, b"hello").unwrap();
                assert_eq!(sig.len(), bits / 8);
                assert!(key.public().verify(hash, b"hello", &sig));
                assert!(!key.public().verify(hash, b"hellp", &sig));
                // A flipped bit, a short one, a long one, a value not below n.
                let mut bad = sig.clone();
                bad[5] ^= 1;
                assert!(!key.public().verify(hash, b"hello", &bad));
                assert!(!key.public().verify(hash, b"hello", &sig[1..]));
                assert!(!key.public().verify(hash, b"hello", &[sig.clone(), vec![0]].concat()));
                assert!(!key.public().verify(hash, b"hello", &vec![0xff; bits / 8]));
                assert!(!key.public().verify(hash, b"hello", &vec![0; bits / 8]));
            }
            // A signature with one hash does not verify as the other.
            let sig = key.sign(SigHash::Sha256, b"x").unwrap();
            assert!(!key.public().verify(SigHash::Sha1, b"x", &sig));
        }
    }

    #[test]
    fn a_2048_bit_key_is_generated_and_signs() {
        let key = RsaKey::generate(2048, &mut Drbg::new(b"big seed")).unwrap();
        let sig = key.sign(SigHash::Sha256, b"m").unwrap();
        assert!(key.public().verify(SigHash::Sha256, b"m", &sig));
        assert_eq!((key.bits(), sig.len()), (2048, 256));
    }

    #[test]
    fn generation_is_deterministic_and_seed_dependent() {
        let a = RsaKey::generate(512, &mut Drbg::new(b"seed one")).unwrap();
        let b = RsaKey::generate(512, &mut Drbg::new(b"seed one")).unwrap();
        let c = RsaKey::generate(512, &mut Drbg::new(b"seed two")).unwrap();
        assert_eq!(a, b);
        assert_ne!(a.public(), c.public());
        // A stored key (its primes) comes back identical.
        let (p, q) = a.primes();
        assert_eq!(RsaKey::from_primes(&p, &q).unwrap(), a);
    }

    #[test]
    fn hostile_inputs_are_refused_not_crashed_on() {
        let mut rng = Drbg::new(b"hostile");
        for bits in [0usize, 1, 63, 64, 511, 520, 2049, 4096, 1 << 40] {
            assert!(RsaKey::generate(bits, &mut rng).is_err(), "{bits}");
        }
        assert!(RsaKey::from_primes(&[], &[]).is_err());
        assert!(RsaKey::from_primes(&[7], &[11]).is_err());
        assert!(RsaKey::from_primes(&[0xff; 64], &[0xff; 64]).is_err());
        assert!(RsaPublicKey::new(&[], 65_537).is_err());
        assert!(RsaPublicKey::new(&[0xff; 64], 4).is_err());
        assert!(RsaPublicKey::new(&[0xff; 64], 65_537).is_ok());
        assert!(RsaPublicKey::new(&[0xff; 5000], 65_537).is_err());
        // Verifying against an even or tiny modulus never panics.
        let k = RsaPublicKey::new(&[0xff; 64], 3).unwrap();
        assert!(!k.verify(SigHash::Sha256, b"m", &[1; 64]));
        assert!(!k.verify(SigHash::Sha256, b"m", &[]));
    }

    #[test]
    fn miller_rabin_agrees_with_known_primes_and_composites() {
        assert!(probably_prime(&Big::from_u64(1_000_000_007)));
        assert!(probably_prime(&Big::from_u64(0xffff_ffff_ffff_ffc5)));
        // Carmichael numbers and a strong pseudoprime to base 2.
        for c in [561u64, 41_041, 825_265, 3_215_031_751] {
            assert!(!probably_prime(&Big::from_u64(c)), "{c}");
        }
        assert!(!probably_prime(&Big::from_u64(1_000_000_007 * 998_244_353)));
    }
}
