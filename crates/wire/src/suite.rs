//! The keyed algorithms IKE and ESP negotiate, by what they compute: pseudo-random functions
//! (HMAC with MD5, SHA-1 or SHA-256), prf+ (RFC 7296 §2.13), integrity checks truncated as
//! IPsec sends them (HMAC-MD5-96 and HMAC-SHA1-96 of RFC 2403/2404, HMAC-SHA-256-128 of
//! RFC 4868), and the plain hashes IKEv1 makes its first IV with (RFC 2409 appendix B).

use crate::md5::md5;
use crate::sha1::{Hash, hmac, hmac_sha1, sha1};
use crate::sha256::{hmac_sha256, sha256};

/// A pseudo-random function (and the hash behind it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Prf {
    HmacMd5,
    #[default]
    HmacSha1,
    HmacSha256,
}

impl Prf {
    /// Bytes of output (and of the preferred key).
    pub fn len(self) -> usize {
        match self {
            Prf::HmacMd5 => 16,
            Prf::HmacSha1 => 20,
            Prf::HmacSha256 => 32,
        }
    }

    pub fn is_empty(self) -> bool {
        false
    }

    pub fn prf(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        match self {
            Prf::HmacMd5 => hmac(Hash::Md5, key, data),
            Prf::HmacSha1 => hmac_sha1(key, data).to_vec(),
            Prf::HmacSha256 => hmac_sha256(key, data).to_vec(),
        }
    }

    /// The plain hash.
    pub fn hash(self, data: &[u8]) -> Vec<u8> {
        match self {
            Prf::HmacMd5 => md5(data).to_vec(),
            Prf::HmacSha1 => sha1(data).to_vec(),
            Prf::HmacSha256 => sha256(data).to_vec(),
        }
    }

    /// prf+ (RFC 7296 §2.13): T1 = prf(K, S | 0x01), Tn = prf(K, Tn−1 | S | n), `len` bytes.
    pub fn plus(self, key: &[u8], seed: &[u8], len: usize) -> Vec<u8> {
        let len = len.min(255 * self.len());
        let mut out = Vec::with_capacity(len + self.len());
        let mut prev: Vec<u8> = Vec::new();
        let mut n: u8 = 1;
        while out.len() < len {
            let mut input = prev.clone();
            input.extend_from_slice(seed);
            input.push(n);
            prev = self.prf(key, &input);
            out.extend_from_slice(&prev);
            n = n.wrapping_add(1);
        }
        out.truncate(len);
        out
    }

    pub fn name(self) -> &'static str {
        match self {
            Prf::HmacMd5 => "HMAC-MD5",
            Prf::HmacSha1 => "HMAC-SHA1",
            Prf::HmacSha256 => "HMAC-SHA256",
        }
    }
}

/// An integrity check as IPsec sends it (truncated HMAC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Integ {
    /// No integrity check (encryption only).
    None,
    HmacMd5_96,
    #[default]
    HmacSha1_96,
    HmacSha256_128,
}

impl Integ {
    /// Bytes of key.
    pub fn key_len(self) -> usize {
        match self {
            Integ::None => 0,
            Integ::HmacMd5_96 => 16,
            Integ::HmacSha1_96 => 20,
            Integ::HmacSha256_128 => 32,
        }
    }

    /// Bytes of integrity check value.
    pub fn icv_len(self) -> usize {
        match self {
            Integ::None => 0,
            Integ::HmacMd5_96 | Integ::HmacSha1_96 => 12,
            Integ::HmacSha256_128 => 16,
        }
    }

    /// The integrity check value of `data`.
    pub fn mac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut full = match self {
            Integ::None => Vec::new(),
            Integ::HmacMd5_96 => hmac(Hash::Md5, key, data),
            Integ::HmacSha1_96 => hmac_sha1(key, data).to_vec(),
            Integ::HmacSha256_128 => hmac_sha256(key, data).to_vec(),
        };
        full.truncate(self.icv_len());
        full
    }

    /// Whether `icv` is the check value of `data`, every byte compared.
    pub fn verify(self, key: &[u8], data: &[u8], icv: &[u8]) -> bool {
        let want = self.mac(key, data);
        want.len() == icv.len() && want.iter().zip(icv).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }

    pub fn name(self) -> &'static str {
        match self {
            Integ::None => "none",
            Integ::HmacMd5_96 => "HMAC-MD5-96",
            Integ::HmacSha1_96 => "HMAC-SHA1-96",
            Integ::HmacSha256_128 => "HMAC-SHA256-128",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn rfc_4868_truncation_and_lengths() {
        // RFC 4231 test case 2, truncated to 128 bits as RFC 4868 sends it.
        assert_eq!(hex(&Integ::HmacSha256_128.mac(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c7");
        // RFC 2202 test case 2, truncated to 96 bits (RFC 2404 and RFC 2403).
        assert_eq!(hex(&Integ::HmacSha1_96.mac(b"Jefe", b"what do ya want for nothing?")), "effcdf6ae5eb2fa2d27416d5");
        assert_eq!(hex(&Integ::HmacMd5_96.mac(b"Jefe", b"what do ya want for nothing?")), "750c783e6ab0b503eaa86e31");
        assert!(Integ::HmacSha256_128.verify(
            b"Jefe",
            b"what do ya want for nothing?",
            &Integ::HmacSha256_128.mac(b"Jefe", b"what do ya want for nothing?")
        ));
        assert!(!Integ::HmacSha256_128.verify(b"Jefe", b"x", &[0; 16]));
        assert!(!Integ::HmacSha256_128.verify(b"Jefe", b"x", &[]));
        assert!(Integ::None.mac(b"k", b"d").is_empty());
        assert_eq!((Prf::HmacSha256.len(), Integ::HmacSha256_128.key_len(), Integ::HmacSha256_128.icv_len()), (32, 32, 16));
    }

    #[test]
    fn prf_plus_chains_its_blocks() {
        for prf in [Prf::HmacMd5, Prf::HmacSha1, Prf::HmacSha256] {
            let n = prf.len();
            let out = prf.plus(b"key", b"seed", 2 * n + 3);
            let t1 = prf.prf(b"key", b"seed\x01");
            let t2 = prf.prf(b"key", &[&t1[..], b"seed\x02"].concat());
            assert_eq!(&out[..n], &t1[..]);
            assert_eq!(&out[n..2 * n], &t2[..]);
            assert_eq!(out.len(), 2 * n + 3);
        }
        assert_eq!(Prf::HmacSha1.plus(b"k", b"s", 1_000_000).len(), 255 * 20);
    }
}
