//! SHA-1 (FIPS 180-4) and HMAC (RFC 2104), for SNMPv3's user-based security (RFC 3414).
//! Not for anything that needs to be secure.

use crate::md5::md5;

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (x, b) in w.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *x = u32::from_be_bytes(*b);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5a82_7999),
                1 => (b ^ c ^ d, 0x6ed9_eba1),
                2 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, v) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(v);
        }
    }
    let mut out = [0u8; 20];
    for (o, w) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        o.copy_from_slice(&w.to_be_bytes());
    }
    out
}

/// A hash SNMPv3 authenticates with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hash {
    Md5,
    Sha1,
}

impl Hash {
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Hash::Md5 => md5(data).to_vec(),
            Hash::Sha1 => sha1(data).to_vec(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Hash::Md5 => "md5",
            Hash::Sha1 => "sha",
        }
    }
}

/// HMAC (RFC 2104) with a 64-byte block, as both hashes have.
pub fn hmac(hash: Hash, key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut k = if key.len() > 64 { hash.digest(key) } else { key.to_vec() };
    k.resize(64, 0);
    let inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).chain(data.iter().copied()).collect();
    let inner = hash.digest(&inner);
    let outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).chain(inner).collect();
    hash.digest(&outer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn fips_and_rfc_2202_vectors() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        assert_eq!(hex(&hmac(Hash::Sha1, b"Jefe", b"what do ya want for nothing?")), "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
        assert_eq!(hex(&hmac(Hash::Md5, b"Jefe", b"what do ya want for nothing?")), "750c783e6ab0b503eaa86e310a5db738");
    }
}
