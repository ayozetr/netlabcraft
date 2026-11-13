//! Diffie-Hellman over the MODP groups of RFC 2409 §6 (groups 1 and 2) and RFC 3526 (groups 5,
//! 14, 15 and 16), as IKEv1 and IKEv2 use them, with a small unsigned big-integer type of its
//! own: Montgomery multiplication (the CIOS form) and fixed-window exponentiation.
//!
//! The primes are not typed in: each RFC defines its prime as
//! `2^n − 2^(n−64) − 1 + 2^64 × (⌊2^(n−130) × π⌋ + k)` with a published `k`, so the digits of π
//! are worked out here (Machin's formula in fixed point) and the primes built from that
//! definition, then checked by the tests (Fermat's test and the well-known first and last
//! digits). The generator is 2 for every group.
//!
//! The arithmetic is not constant-time and the private values come from the simulator's
//! deterministic random source: right for a teaching simulator, never for protecting real data.

use std::cmp::Ordering;
use std::sync::OnceLock;

/// An unsigned integer of any size: 64-bit limbs, least significant first, with no zero limbs
/// at the top (zero is the empty vector).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Big(Vec<u64>);

/// Longest number accepted from outside (bytes): far beyond the 4096-bit groups.
pub const MAX_BYTES: usize = 1024;

impl Big {
    pub fn zero() -> Big {
        Big(Vec::new())
    }

    pub fn from_u64(v: u64) -> Big {
        let mut b = Big(vec![v]);
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    /// From big-endian bytes (at most [`MAX_BYTES`]; longer input keeps its last bytes).
    pub fn from_be_bytes(b: &[u8]) -> Big {
        let b = b.get(b.len().saturating_sub(MAX_BYTES)..).unwrap_or_default();
        let mut limbs = Vec::with_capacity(b.len() / 8 + 1);
        for chunk in b.rchunks(8) {
            let mut w = [0u8; 8];
            w[8 - chunk.len()..].copy_from_slice(chunk);
            limbs.push(u64::from_be_bytes(w));
        }
        let mut out = Big(limbs);
        out.trim();
        out
    }

    /// Big-endian bytes, left-padded with zeros to `len` (the low `len` bytes if longer).
    pub fn to_be_bytes(&self, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        for (i, limb) in self.0.iter().enumerate() {
            for (j, byte) in limb.to_le_bytes().iter().enumerate() {
                let at = i * 8 + j;
                if at < len {
                    out[len - 1 - at] = *byte;
                }
            }
        }
        out
    }

    /// The number of significant bits.
    pub fn bits(&self) -> usize {
        match self.0.last() {
            None => 0,
            Some(top) => self.0.len() * 64 - top.leading_zeros() as usize,
        }
    }

    fn bit(&self, i: usize) -> bool {
        self.0.get(i / 64).is_some_and(|w| (w >> (i % 64)) & 1 == 1)
    }

    pub fn add(&self, o: &Big) -> Big {
        let n = self.0.len().max(o.0.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let (a, b) = (self.0.get(i).copied().unwrap_or(0), o.0.get(i).copied().unwrap_or(0));
            let (s1, c1) = a.overflowing_add(b);
            let (s2, c2) = s1.overflowing_add(carry);
            out.push(s2);
            carry = u64::from(c1) + u64::from(c2);
        }
        out.push(carry);
        let mut b = Big(out);
        b.trim();
        b
    }

    /// `self − o`, or `None` when `o` is larger.
    pub fn sub(&self, o: &Big) -> Option<Big> {
        if self.cmp(o) == Ordering::Less {
            return None;
        }
        let mut out = Vec::with_capacity(self.0.len());
        let mut borrow = 0u64;
        for (i, a) in self.0.iter().enumerate() {
            let b = o.0.get(i).copied().unwrap_or(0);
            let (d1, b1) = a.overflowing_sub(b);
            let (d2, b2) = d1.overflowing_sub(borrow);
            out.push(d2);
            borrow = u64::from(b1) + u64::from(b2);
        }
        let mut b = Big(out);
        b.trim();
        Some(b)
    }

    pub fn shl(&self, bits: usize) -> Big {
        if self.is_zero() {
            return Big::zero();
        }
        let (words, rest) = (bits / 64, bits % 64);
        let mut out = vec![0u64; words];
        let mut carry = 0u64;
        for w in &self.0 {
            if rest == 0 {
                out.push(*w);
            } else {
                out.push((w << rest) | carry);
                carry = w >> (64 - rest);
            }
        }
        out.push(carry);
        let mut b = Big(out);
        b.trim();
        b
    }

    pub fn shr(&self, bits: usize) -> Big {
        let (words, rest) = (bits / 64, bits % 64);
        let src = self.0.get(words..).unwrap_or_default();
        let mut out = Vec::with_capacity(src.len());
        for (i, w) in src.iter().enumerate() {
            let hi = src.get(i + 1).copied().unwrap_or(0);
            out.push(if rest == 0 { *w } else { (w >> rest) | (hi << (64 - rest)) });
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    pub fn mul(&self, o: &Big) -> Big {
        if self.is_zero() || o.is_zero() {
            return Big::zero();
        }
        let mut out = vec![0u64; self.0.len() + o.0.len()];
        for (i, a) in self.0.iter().enumerate() {
            let mut carry = 0u128;
            for (j, b) in o.0.iter().enumerate() {
                let t = u128::from(out[i + j]) + u128::from(*a) * u128::from(*b) + carry;
                out[i + j] = t as u64;
                carry = t >> 64;
            }
            out[i + o.0.len()] = carry as u64;
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    /// Division by a small divisor (not zero): the quotient.
    pub fn div_small(&self, d: u64) -> Big {
        if d == 0 {
            return Big::zero();
        }
        let mut out = vec![0u64; self.0.len()];
        let mut rem = 0u128;
        for i in (0..self.0.len()).rev() {
            let cur = (rem << 64) | u128::from(self.0[i]);
            out[i] = (cur / u128::from(d)) as u64;
            rem = cur % u128::from(d);
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    /// `self mod m` by shift-and-subtract (slow; for numbers that aren't already reduced).
    pub fn rem(&self, m: &Big) -> Big {
        if m.is_zero() {
            return Big::zero();
        }
        if self.cmp(m) == Ordering::Less {
            return self.clone();
        }
        let mut r = Big::zero();
        for i in (0..self.bits()).rev() {
            r = r.shl(1);
            if self.bit(i) {
                r = r.add(&Big::from_u64(1));
            }
            if let Some(d) = r.sub(m) {
                r = d;
            }
        }
        r
    }
}

impl PartialOrd for Big {
    fn partial_cmp(&self, o: &Big) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Big {
    fn cmp(&self, o: &Big) -> Ordering {
        self.0.len().cmp(&o.0.len()).then_with(|| self.0.iter().rev().cmp(o.0.iter().rev()))
    }
}

/// Montgomery arithmetic modulo an odd number `n` of `s` limbs (R = 2^(64·s)).
struct Mont {
    n: Vec<u64>,
    /// −n⁻¹ mod 2^64.
    n0: u64,
    /// R² mod n, to enter the Montgomery domain.
    r2: Vec<u64>,
}

impl Mont {
    fn new(n: &Big) -> Option<Mont> {
        let low = *n.0.first()?;
        if low & 1 == 0 {
            return None;
        }
        // Newton's iteration doubles the correct low bits each step: 1 → 64 in six.
        let mut inv: u64 = 1;
        for _ in 0..6 {
            inv = inv.wrapping_mul(2u64.wrapping_sub(low.wrapping_mul(inv)));
        }
        let s = n.0.len();
        // R² mod n by doubling 1 modulo n, 2·64·s times.
        let mut r = Big::from_u64(1).rem(n);
        for _ in 0..2 * 64 * s {
            r = r.shl(1);
            if let Some(d) = r.sub(n) {
                r = d;
            }
        }
        Some(Mont { n: n.0.clone(), n0: inv.wrapping_neg(), r2: Mont::pad(&r, s) })
    }

    fn pad(b: &Big, s: usize) -> Vec<u64> {
        let mut v = b.0.clone();
        v.resize(s, 0);
        v
    }

    /// a·b·R⁻¹ mod n, for a and b below n (CIOS).
    fn mul(&self, a: &[u64], b: &[u64]) -> Vec<u64> {
        let s = self.n.len();
        let mut t = vec![0u64; s + 2];
        for bi in b.iter().take(s) {
            let mut c = 0u128;
            for j in 0..s {
                let x = u128::from(t[j]) + u128::from(a[j]) * u128::from(*bi) + c;
                t[j] = x as u64;
                c = x >> 64;
            }
            let x = u128::from(t[s]) + c;
            t[s] = x as u64;
            t[s + 1] = (x >> 64) as u64;
            let m = t[0].wrapping_mul(self.n0);
            let x = u128::from(t[0]) + u128::from(m) * u128::from(self.n[0]);
            let mut c = x >> 64;
            for j in 1..s {
                let x = u128::from(t[j]) + u128::from(m) * u128::from(self.n[j]) + c;
                t[j - 1] = x as u64;
                c = x >> 64;
            }
            let x = u128::from(t[s]) + c;
            t[s - 1] = x as u64;
            t[s] = t[s + 1] + (x >> 64) as u64;
        }
        // The result is below 2n: one conditional subtraction.
        let mut out = t[..s].to_vec();
        let ge = t[s] != 0 || out.iter().rev().cmp(self.n.iter().rev()) != Ordering::Less;
        if ge {
            let mut borrow = 0u64;
            for (o, n) in out.iter_mut().zip(&self.n) {
                let (d1, b1) = o.overflowing_sub(*n);
                let (d2, b2) = d1.overflowing_sub(borrow);
                *o = d2;
                borrow = u64::from(b1) + u64::from(b2);
            }
        }
        out
    }

    /// base^exp mod n with a fixed 4-bit window.
    fn pow(&self, base: &Big, exp: &Big) -> Big {
        let s = self.n.len();
        let modulus = Big(self.n.clone());
        let base = Mont::pad(&base.rem(&modulus), s);
        let mut one = vec![0u64; s];
        one[0] = 1;
        let base_m = self.mul(&base, &self.r2);
        let one_m = self.mul(&one, &self.r2);
        let mut table = vec![one_m.clone(), base_m.clone()];
        for i in 2..16 {
            let next = self.mul(&table[i - 1], &base_m);
            table.push(next);
        }
        let mut acc = one_m;
        let nibbles = exp.bits().div_ceil(4);
        for k in (0..nibbles).rev() {
            for _ in 0..4 {
                acc = self.mul(&acc, &acc);
            }
            let w = (0..4).fold(0usize, |v, b| v | (usize::from(exp.bit(4 * k + b)) << b));
            if w != 0 {
                acc = self.mul(&acc, &table[w]);
            }
        }
        let mut out = Big(self.mul(&acc, &one));
        out.trim();
        out
    }
}

/// base^exp mod m for an odd modulus (`None` for an even or zero one).
pub fn mod_pow(base: &Big, exp: &Big, m: &Big) -> Option<Big> {
    let mont = Mont::new(m)?;
    if m == &Big::from_u64(1) {
        return Some(Big::zero());
    }
    Some(mont.pow(base, exp))
}

/// ⌊2^bits × atan(1/x)⌋, give or take a few units in the last place.
fn arctan_inv(x: u64, bits: usize) -> Big {
    let mut power = Big::from_u64(1).shl(bits).div_small(x);
    let mut sum = power.clone();
    let x2 = x * x;
    for k in 1u64.. {
        power = power.div_small(x2);
        if power.is_zero() {
            break;
        }
        let term = power.div_small(2 * k + 1);
        sum = if k % 2 == 1 { sum.sub(&term).unwrap_or_default() } else { sum.add(&term) };
    }
    sum
}

/// ⌊2^bits × π⌋ by Machin's formula, π = 16·atan(1/5) − 4·atan(1/239), with 64 guard bits.
pub fn pi_bits(bits: usize) -> Big {
    let wide = bits + 64;
    let a = arctan_inv(5, wide).shl(4);
    let b = arctan_inv(239, wide).shl(2);
    a.sub(&b).unwrap_or_default().shr(64)
}

/// A MODP group: its number, its prime's size and the prime.
#[derive(Debug)]
pub struct ModpGroup {
    pub id: u16,
    pub bits: usize,
    p: Big,
    mont: Mont,
}

impl std::fmt::Debug for Mont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Mont(..)")
    }
}

/// (group, bits, k) from RFC 2409 §6.1–6.2 and RFC 3526 §2–5.
const GROUPS: [(u16, usize, u64); 6] =
    [(1, 768, 149_686), (2, 1024, 129_093), (5, 1536, 741_804), (14, 2048, 124_476), (15, 3072, 1_690_314), (16, 4096, 240_904)];

static BUILT: [OnceLock<Option<ModpGroup>>; 6] =
    [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];

/// The prime of a group from its definition.
fn build(id: u16, n: usize, k: u64) -> Option<ModpGroup> {
    let mid = pi_bits(n - 130).add(&Big::from_u64(k)).shl(64);
    let p = Big::from_u64(1).shl(n).add(&mid).sub(&Big::from_u64(1).shl(n - 64))?.sub(&Big::from_u64(1))?;
    let mont = Mont::new(&p)?;
    Some(ModpGroup { id, bits: n, p, mont })
}

/// The MODP group with this number (1, 2, 5, 14, 15 or 16), built the first time it is asked
/// for; `None` for any other number (the elliptic-curve groups among them).
pub fn modp(id: u16) -> Option<&'static ModpGroup> {
    let at = GROUPS.iter().position(|g| g.0 == id)?;
    let (_, n, k) = *GROUPS.get(at)?;
    BUILT.get(at)?.get_or_init(|| build(id, n, k)).as_ref()
}

/// Whether a group number is one of the MODP groups computed for real.
pub fn is_modp(id: u16) -> bool {
    GROUPS.iter().any(|g| g.0 == id)
}

impl ModpGroup {
    /// Bytes of a public value or a shared secret.
    pub fn len(&self) -> usize {
        self.bits / 8
    }

    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// The prime, big-endian.
    pub fn prime(&self) -> Vec<u8> {
        self.p.to_be_bytes(self.len())
    }

    /// The public value g^x mod p for a private value `x` (big-endian bytes).
    pub fn public(&self, private: &[u8]) -> Vec<u8> {
        self.mont.pow(&Big::from_u64(2), &Big::from_be_bytes(private)).to_be_bytes(self.len())
    }

    /// The shared secret (g^y)^x mod p, as many bytes as the prime (RFC 7296 §2.14 keeps the
    /// leading zeros). `None` when the peer's value is not of the group's size or is not
    /// between 2 and p − 2 (RFC 2631 §2.1.5).
    pub fn shared(&self, private: &[u8], peer: &[u8]) -> Option<Vec<u8>> {
        if !self.valid_peer(peer) {
            return None;
        }
        Some(self.mont.pow(&Big::from_be_bytes(peer), &Big::from_be_bytes(private)).to_be_bytes(self.len()))
    }

    /// Whether a peer's public value is usable: as many bytes as the prime and between 2 and
    /// p − 2 (RFC 2631 §2.1.5, NIST SP 800-56A).
    pub fn valid_peer(&self, peer: &[u8]) -> bool {
        if peer.len() != self.len() {
            return false;
        }
        let y = Big::from_be_bytes(peer);
        self.p.sub(&Big::from_u64(1)).is_some_and(|top| y > Big::from_u64(1) && y < top)
    }

    /// Whether the prime passes Fermat's test to base 3 (3^(p−1) ≡ 1).
    pub fn fermat_ok(&self) -> bool {
        self.p.sub(&Big::from_u64(1)).is_some_and(|e| self.mont.pow(&Big::from_u64(3), &e) == Big::from_u64(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02X}")).collect()
    }

    /// Independent square-and-multiply in u128 for small numbers.
    fn naive(mut b: u128, mut e: u128, m: u128) -> u128 {
        let mut r = 1 % m;
        b %= m;
        while e > 0 {
            if e & 1 == 1 {
                r = r * b % m;
            }
            b = b * b % m;
            e >>= 1;
        }
        r
    }

    #[test]
    fn small_modular_exponentiation_matches_a_naive_one() {
        // The textbook example: 4^13 mod 497 = 445.
        assert_eq!(mod_pow(&Big::from_u64(4), &Big::from_u64(13), &Big::from_u64(497)), Some(Big::from_u64(445)));
        // Fermat: a^(p−1) ≡ 1 for a prime.
        assert_eq!(mod_pow(&Big::from_u64(3), &Big::from_u64(1_000_000_006), &Big::from_u64(1_000_000_007)), Some(Big::from_u64(1)));
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..300 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let m = (x >> 33) | 1;
            let (b, e) = (x & 0xffff_ffff, x >> 40);
            let got = mod_pow(&Big::from_u64(b), &Big::from_u64(e), &Big::from_u64(m)).unwrap();
            assert_eq!(got, Big::from_u64(naive(b.into(), e.into(), m.into()) as u64), "{b}^{e} mod {m}");
        }
        // A two-limb modulus against u128 arithmetic.
        let m: u128 = (1u128 << 100) + 277;
        let mb = Big::from_be_bytes(&m.to_be_bytes());
        let got = mod_pow(&Big::from_u64(7), &Big::from_u64(1 << 20), &mb).unwrap();
        let mut want: u128 = 7;
        for _ in 0..20 {
            want = Big::from_be_bytes(&want.to_be_bytes())
                .mul(&Big::from_be_bytes(&want.to_be_bytes()))
                .rem(&mb)
                .to_be_bytes(16)
                .iter()
                .fold(0u128, |a, b| (a << 8) | u128::from(*b));
        }
        assert_eq!(got.to_be_bytes(16), want.to_be_bytes().to_vec());
        // Even or zero moduli are refused, never a panic.
        assert!(mod_pow(&Big::from_u64(2), &Big::from_u64(3), &Big::from_u64(10)).is_none());
        assert!(mod_pow(&Big::from_u64(2), &Big::from_u64(3), &Big::zero()).is_none());
        assert_eq!(mod_pow(&Big::from_u64(2), &Big::from_u64(3), &Big::from_u64(1)), Some(Big::zero()));
    }

    #[test]
    fn big_integer_basics() {
        let a = Big::from_be_bytes(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(a.to_be_bytes(10), vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(a.to_be_bytes(12), vec![0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(a.shl(77).shr(77), a);
        assert_eq!(a.add(&a).sub(&a), Some(a.clone()));
        assert!(Big::from_u64(1).sub(&a).is_none());
        assert_eq!(a.mul(&Big::from_u64(1000)).div_small(1000), a);
        assert_eq!(Big::from_u64(1).shl(64).bits(), 65);
        assert_eq!(Big::from_be_bytes(&[0, 0, 0]), Big::zero());
        assert_eq!(Big::from_u64(1000).rem(&Big::from_u64(7)), Big::from_u64(6));
    }

    #[test]
    fn pi_comes_out_right() {
        // π = 3.243F6A8885A308D313198A2E0370734… in hexadecimal.
        assert_eq!(hex(&pi_bits(126).to_be_bytes(16)), "C90FDAA22168C234C4C6628B80DC1CD1");
    }

    #[test]
    fn modp_primes_are_built_from_their_definition() {
        for (id, bits, _) in GROUPS {
            let g = modp(id).unwrap();
            let p = hex(&g.prime());
            assert_eq!(p.len(), bits / 4);
            assert!(p.starts_with("FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1"), "group {id}");
            assert!(p.ends_with("FFFFFFFFFFFFFFFF"), "group {id}");
        }
        // The last digits before the trailing ones, as the RFCs print them.
        assert!(hex(&modp(1).unwrap().prime()).ends_with("A63A3620FFFFFFFFFFFFFFFF"));
        assert!(hex(&modp(2).unwrap().prime()).ends_with("49286651ECE65381FFFFFFFFFFFFFFFF"));
        assert!(hex(&modp(5).unwrap().prime()).ends_with("CA237327FFFFFFFFFFFFFFFF"));
        assert!(hex(&modp(14).unwrap().prime()).ends_with("15728E5A8AACAA68FFFFFFFFFFFFFFFF"));
        // A wrong k would almost surely not give a prime.
        for id in [1, 2, 5, 14] {
            assert!(modp(id).unwrap().fermat_ok(), "group {id}");
        }
        assert!(modp(19).is_none() && modp(0).is_none());
    }

    #[test]
    fn both_sides_agree_in_every_group() {
        for id in [1u16, 2, 5, 14, 15, 16] {
            let g = modp(id).unwrap();
            let a = [0x5au8; 32];
            let mut b = [0u8; 32];
            for (i, x) in b.iter_mut().enumerate() {
                *x = (i as u8).wrapping_mul(37).wrapping_add(id as u8);
            }
            let (pa, pb) = (g.public(&a), g.public(&b));
            assert_eq!(pa.len(), g.len());
            assert_ne!(pa, pb);
            let (sa, sb) = (g.shared(&a, &pb).unwrap(), g.shared(&b, &pa).unwrap());
            assert_eq!(sa, sb, "group {id}");
            assert_eq!(sa.len(), g.len());
            // A third party with another private value gets something else.
            assert_ne!(g.shared(&[1; 32], &pb).unwrap(), sa);
            // Values outside 2..p−2, or of the wrong size, are refused.
            assert!(g.shared(&a, &vec![0; g.len()]).is_none());
            let mut one = vec![0; g.len()];
            *one.last_mut().unwrap() = 1;
            assert!(g.shared(&a, &one).is_none());
            assert!(g.shared(&a, &g.prime()).is_none());
            assert!(g.shared(&a, &pb[1..]).is_none());
        }
        // g^1 = 2, g^10 = 1024.
        let g = modp(2).unwrap();
        assert_eq!(Big::from_be_bytes(&g.public(&[10])), Big::from_u64(1024));
        // g^(a+b) = g^a · g^b mod p.
        let p = Big::from_be_bytes(&g.prime());
        let (ga, gb, gab) = (g.public(&[3, 7]), g.public(&[1, 9]), g.public(&[4, 16]));
        assert_eq!(Big::from_be_bytes(&ga).mul(&Big::from_be_bytes(&gb)).rem(&p), Big::from_be_bytes(&gab));
    }
}
