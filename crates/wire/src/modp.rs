//! Finite-field Diffie-Hellman over the 2048-bit MODP group of RFC 3526 §3 ("group 14"), as
//! the TLS anonymous-DH key exchange (RFC 5246 §7.4.3) and SSH's
//! `diffie-hellman-group14-sha256` (RFC 8268) use it: modular exponentiation with Montgomery
//! multiplication (32-bit limbs), and the checks a peer's public value must pass.
//!
//! Private exponents come from the caller (the simulator draws them from its own seeded
//! generator, so a lab replays the same keys: fine for teaching, not for secrets).

use std::cmp::Ordering;

/// The group 14 prime (RFC 3526 §3), big-endian hex.
const P_HEX: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DD\
EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED\
EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F\
83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B\
E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995497CEA956AE515D2261898FA0510\
15728E5A8AACAA68FFFFFFFFFFFFFFFF";

/// The group's generator.
pub const G: u8 = 2;
/// Bytes in the prime (and in every public value, left-padded).
pub const P_LEN: usize = 256;
/// Bytes in a private exponent (256 bits: twice the group's ~112-bit strength, RFC 3526 §8).
pub const PRIVATE_LEN: usize = 32;

/// The group 14 prime, big-endian.
pub fn prime() -> Vec<u8> {
    P_HEX.as_bytes().chunks(2).map(|h| std::str::from_utf8(h).ok().and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0)).collect()
}

/// Little-endian limbs of a big-endian number, `n` limbs (extra high bytes are dropped).
fn limbs(be: &[u8], n: usize) -> Vec<u32> {
    let mut out = vec![0u32; n];
    for (i, b) in be.iter().rev().enumerate() {
        if let Some(l) = out.get_mut(i / 4) {
            *l |= u32::from(*b) << (8 * (i % 4));
        }
    }
    out
}

fn to_be(a: &[u32], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    for (i, o) in out.iter_mut().rev().enumerate() {
        *o = a.get(i / 4).map_or(0, |l| (l >> (8 * (i % 4))) as u8);
    }
    out
}

fn cmp(a: &[u32], b: &[u32]) -> Ordering {
    for i in (0..a.len().max(b.len())).rev() {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    Ordering::Equal
}

/// `a -= b` over `a`'s length (the borrow out is dropped: callers keep `a >= b`).
fn sub(a: &mut [u32], b: &[u32]) {
    let mut borrow = 0i64;
    for (i, x) in a.iter_mut().enumerate() {
        let d = i64::from(*x) - i64::from(b.get(i).copied().unwrap_or(0)) - borrow;
        *x = d as u32;
        borrow = i64::from(d < 0);
    }
}

/// Montgomery arithmetic modulo an odd `m`.
struct Mont {
    m: Vec<u32>,
    /// -m⁻¹ mod 2³².
    inv: u32,
    /// R² mod m, R = 2^(32n).
    r2: Vec<u32>,
}

impl Mont {
    fn new(m: Vec<u32>) -> Option<Mont> {
        let m0 = *m.first()?;
        if m0 & 1 == 0 {
            return None;
        }
        let mut x: u32 = 1;
        for _ in 0..5 {
            x = x.wrapping_mul(2u32.wrapping_sub(m0.wrapping_mul(x)));
        }
        let n = m.len();
        // R² mod m by doubling 1, 2·32n times.
        let mut r = vec![0u32; n];
        r[0] = 1;
        for _ in 0..(64 * n) {
            let mut carry = 0u32;
            for l in r.iter_mut() {
                let v = (*l >> 31) & 1;
                *l = (*l << 1) | carry;
                carry = v;
            }
            if carry == 1 || cmp(&r, &m) != Ordering::Less {
                sub(&mut r, &m);
            }
        }
        Some(Mont { m, inv: x.wrapping_neg(), r2: r })
    }

    /// a·b·R⁻¹ mod m (CIOS).
    #[allow(clippy::needless_range_loop)]
    fn mul(&self, a: &[u32], b: &[u32]) -> Vec<u32> {
        let n = self.m.len();
        let mut t = vec![0u32; n + 2];
        for i in 0..n {
            let bi = u64::from(b.get(i).copied().unwrap_or(0));
            let mut carry = 0u64;
            for j in 0..n {
                let v = u64::from(t[j]) + u64::from(a.get(j).copied().unwrap_or(0)) * bi + carry;
                t[j] = v as u32;
                carry = v >> 32;
            }
            let v = u64::from(t[n]) + carry;
            t[n] = v as u32;
            t[n + 1] = (v >> 32) as u32;
            let q = u64::from(t[0].wrapping_mul(self.inv));
            let mut carry = (u64::from(t[0]) + q * u64::from(self.m[0])) >> 32;
            for j in 1..n {
                let v = u64::from(t[j]) + q * u64::from(self.m[j]) + carry;
                t[j - 1] = v as u32;
                carry = v >> 32;
            }
            let v = u64::from(t[n]) + carry;
            t[n - 1] = v as u32;
            t[n] = t[n + 1].wrapping_add((v >> 32) as u32);
        }
        t.truncate(n + 1);
        if cmp(&t, &self.m) != Ordering::Less {
            sub(&mut t, &self.m);
        }
        t.truncate(n);
        t
    }
}

/// base^exp mod m, all big-endian; the result is as long as `m`. `None` if `m` is even or
/// zero, or `base` is not below `m`.
pub fn modpow(base: &[u8], exp: &[u8], m: &[u8]) -> Option<Vec<u8>> {
    let n = m.len().div_ceil(4).max(1);
    let ml = limbs(m, n);
    let b = limbs(base, n.max(base.len().div_ceil(4)));
    if cmp(&b, &ml) != Ordering::Less || ml.iter().all(|x| *x == 0) {
        return None;
    }
    let mont = Mont::new(ml)?;
    let mut one = vec![0u32; n];
    one[0] = 1;
    let bm = mont.mul(&b, &mont.r2);
    let mut acc = mont.mul(&one, &mont.r2);
    for byte in exp {
        for bit in (0..8).rev() {
            acc = mont.mul(&acc, &acc);
            if (byte >> bit) & 1 == 1 {
                acc = mont.mul(&acc, &bm);
            }
        }
    }
    Some(to_be(&mont.mul(&acc, &one), m.len()))
}

/// The public value g^x mod p of a private exponent.
pub fn public(private: &[u8]) -> Vec<u8> {
    modpow(&[G], private, &prime()).unwrap_or_default()
}

/// Whether a peer's public value is usable: 1 < y < p − 1 (RFC 8268 §4, NIST SP 800-56A).
pub fn valid_public(y: &[u8]) -> bool {
    let p = prime();
    let n = P_LEN / 4;
    if y.len() > P_LEN.saturating_add(1) || y.iter().take(y.len().saturating_sub(P_LEN)).any(|b| *b != 0) {
        return false;
    }
    let yl = limbs(y, n);
    let mut pm1 = limbs(&p, n);
    sub(&mut pm1, &[1]);
    cmp(&yl, &[1]) == Ordering::Greater && cmp(&yl, &pm1) == Ordering::Less
}

/// The shared secret y^x mod p (left-padded to [`P_LEN`]); `None` for an unusable `y`.
pub fn shared(peer: &[u8], private: &[u8]) -> Option<Vec<u8>> {
    if !valid_public(peer) {
        return None;
    }
    let trimmed = peer.get(peer.len().saturating_sub(P_LEN)..).unwrap_or(peer);
    modpow(trimmed, private, &prime())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_numbers_agree_with_plain_arithmetic() {
        assert_eq!(modpow(&[4], &[13], &[1, 241]), Some(vec![1, 189])); // 4^13 mod 497 = 445
        assert_eq!(modpow(&[2], &[0], &[0, 7]), Some(vec![0, 1]));
        assert_eq!(modpow(&[9], &[1], &[7]), None, "base not below the modulus");
        assert_eq!(modpow(&[2], &[1], &[8]), None, "even modulus");
        // A few exponents against u128 arithmetic modulo a 64-bit prime.
        let m: u128 = 0xffff_ffff_ffff_ffc5;
        for (b, e) in [(3u128, 1_000_003u128), (0x1234_5678_9abc, 0xdead_beef), (m - 1, 2)] {
            let mut want = 1u128;
            let (mut x, mut k) = (b % m, e);
            while k > 0 {
                if k & 1 == 1 {
                    want = want * x % m;
                }
                x = x * x % m;
                k >>= 1;
            }
            let got = modpow(&(b as u64).to_be_bytes(), &(e as u64).to_be_bytes(), &(m as u64).to_be_bytes()).unwrap();
            assert_eq!(u64::from_be_bytes(got.try_into().unwrap()) as u128, want, "{b}^{e}");
        }
    }

    #[test]
    fn the_group_14_prime_passes_fermat_and_is_safe() {
        let p = prime();
        assert_eq!(p.len(), P_LEN);
        let mut pm1 = limbs(&p, 64);
        sub(&mut pm1, &[1]);
        let pm1 = to_be(&pm1, P_LEN);
        let mut one = vec![0u8; P_LEN];
        one[P_LEN - 1] = 1;
        assert_eq!(modpow(&[2], &pm1, &p).unwrap(), one, "2^(p-1) = 1 mod p");
        // q = (p − 1) / 2 is prime too.
        let mut q = limbs(&pm1, 64);
        for i in 0..64 {
            let hi = q.get(i + 1).copied().unwrap_or(0);
            q[i] = (q[i] >> 1) | (hi << 31);
        }
        let mut qm1 = q.clone();
        sub(&mut qm1, &[1]);
        assert_eq!(modpow(&[3], &to_be(&qm1, P_LEN), &to_be(&q, P_LEN)).unwrap(), one, "3^(q-1) = 1 mod q");
    }

    #[test]
    fn both_sides_reach_the_same_secret() {
        let (a, b) = ([0x5au8; PRIVATE_LEN], [0xa7u8; PRIVATE_LEN]);
        let (ya, yb) = (public(&a), public(&b));
        assert_eq!(ya.len(), P_LEN);
        assert_ne!(ya, yb);
        assert_eq!(shared(&yb, &a), shared(&ya, &b));
        assert!(shared(&yb, &a).is_some());
        // Degenerate peer values are refused.
        let p = prime();
        let mut pm1 = limbs(&p, 64);
        sub(&mut pm1, &[1]);
        for bad in [vec![0u8], vec![1u8], to_be(&pm1, P_LEN), p.clone(), vec![0xff; P_LEN + 2], Vec::new()] {
            assert!(shared(&bad, &a).is_none());
        }
        let mut padded = vec![0u8];
        padded.extend_from_slice(&yb);
        assert_eq!(shared(&padded, &a), shared(&yb, &a), "an mpint's leading zero is fine");
    }
}
