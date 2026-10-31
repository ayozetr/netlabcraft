//! SHA-256 (FIPS 180-4 §6.2) and HMAC-SHA-256 (RFC 2104, checked with the RFC 4231 vectors),
//! for ESP's `esp-sha256-hmac` (HMAC-SHA-256-128, RFC 4868) and IKE's SHA-256 PRF and
//! integrity. Not for anything that needs to be secure.
//!
//! The round constants and the initial hash value are worked out from their definition (the
//! first 32 bits of the fractional parts of the cube roots of the first 64 primes, and of the
//! square roots of the first 8), with exact integer roots, rather than typed in.

const PRIMES: [u128; 64] = first_primes();

const fn first_primes() -> [u128; 64] {
    let mut out = [0u128; 64];
    let mut n = 2u128;
    let mut found = 0;
    while found < 64 {
        let mut d = 2u128;
        let mut prime = true;
        while d * d <= n {
            if n.is_multiple_of(d) {
                prime = false;
                break;
            }
            d += 1;
        }
        if prime {
            out[found] = n;
            found += 1;
        }
        n += 1;
    }
    out
}

/// ⌊root(p) × 2^32⌋ mod 2^32 for the square (`cube` false) or cube root: the largest c with
/// c^k ≤ p × 2^(32k), found by bisection.
const fn frac_root(p: u128, cube: bool) -> u32 {
    let target = if cube { p << 96 } else { p << 64 };
    let (mut lo, mut hi) = (0u128, 1u128 << 40);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        let pow = if cube { mid * mid * mid } else { mid * mid };
        if pow <= target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo as u32
}

const K: [u32; 64] = {
    let mut k = [0u32; 64];
    let mut i = 0;
    while i < 64 {
        k[i] = frac_root(PRIMES[i], true);
        i += 1;
    }
    k
};

const H0: [u32; 8] = {
    let mut h = [0u32; 8];
    let mut i = 0;
    while i < 8 {
        h[i] = frac_root(PRIMES[i], false);
        i += 1;
    }
    h
};

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = H0;
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (x, b) in w.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *x = u32::from_be_bytes(*b);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for (ki, wi) in K.iter().zip(w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(*ki).wrapping_add(wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (o, w) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        o.copy_from_slice(&w.to_be_bytes());
    }
    out
}

/// HMAC-SHA-256 (RFC 2104 with a 64-byte block).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).chain(data.iter().copied()).collect();
    let inner = sha256(&inner);
    let outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).chain(inner).collect();
    sha256(&outer)
}

/// HMAC-SHA-256-128 (RFC 4868 §2.3): the first 128 bits of HMAC-SHA-256, as ESP and IKEv2
/// integrity send them.
pub fn hmac_sha256_128(key: &[u8], data: &[u8]) -> [u8; 16] {
    let full = hmac_sha256(key, data);
    let mut out = [0u8; 16];
    out.copy_from_slice(&full[..16]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn constants_match_fips_180_4() {
        assert_eq!((K[0], K[1], K[63]), (0x428a_2f98, 0x7137_4491, 0xc671_78f2));
        assert_eq!((H0[0], H0[7]), (0x6a09_e667, 0x5be0_cd19));
    }

    #[test]
    fn fips_180_4_examples() {
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(hex(&sha256(&vec![b'a'; 1_000_000])), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn rfc_4231_hmac_vectors() {
        // Test case 1.
        assert_eq!(hex(&hmac_sha256(&[0x0b; 20], b"Hi There")), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
        // Test case 2.
        assert_eq!(hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // Test case 3.
        assert_eq!(hex(&hmac_sha256(&[0xaa; 20], &[0xdd; 50])), "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe");
        // Test case 6: a key longer than the block is hashed first.
        assert_eq!(
            hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        // Test case 5: truncated to 128 bits, as RFC 4868 does.
        assert_eq!(hex(&hmac_sha256_128(&[0x0c; 20], b"Test With Truncation")), "a3b6167473100ee06e0c796c2955552b");
    }
}
