//! Finite-field Diffie-Hellman over the 2048-bit MODP group of RFC 3526 §3 ("group 14"), as
//! the TLS anonymous-DH key exchange (RFC 5246 §7.4.3) and SSH's
//! `diffie-hellman-group14-sha256` (RFC 8268) use it: the group's prime and exponentiation come
//! from [`crate::dh`], which every Diffie-Hellman exchange in the workspace shares, and this
//! module only adds what those two protocols need on top: public values as an `mpint` carries
//! them (no fixed width, an optional leading zero byte).
//!
//! Private exponents come from the caller (the simulator draws them from its own seeded
//! generator, so a lab replays the same keys: fine for teaching, not for secrets).

use crate::dh::{self, ModpGroup};

/// The group's generator.
pub const G: u8 = 2;
/// Bytes in the prime (and in every public value, left-padded).
pub const P_LEN: usize = 256;
/// Bytes in a private exponent (256 bits: twice the group's ~112-bit strength, RFC 3526 §8).
pub const PRIVATE_LEN: usize = 32;

/// The group 14 definition, built the first time it is asked for.
fn group() -> Option<&'static ModpGroup> {
    dh::modp(14)
}

/// The group 14 prime, big-endian.
pub fn prime() -> Vec<u8> {
    group().map(ModpGroup::prime).unwrap_or_default()
}

/// The public value g^x mod p of a private exponent.
pub fn public(private: &[u8]) -> Vec<u8> {
    group().map(|g| g.public(private)).unwrap_or_default()
}

/// A peer's value as exactly [`P_LEN`] bytes: an `mpint` may carry one leading zero byte or omit
/// leading zeros altogether. `None` when it is longer than that.
fn fixed_width(y: &[u8]) -> Option<Vec<u8>> {
    let extra = y.len().saturating_sub(P_LEN);
    if y.len() > P_LEN.saturating_add(1) || y.iter().take(extra).any(|b| *b != 0) {
        return None;
    }
    let mut out = vec![0u8; P_LEN.saturating_sub(y.len())];
    out.extend_from_slice(y.get(extra..)?);
    Some(out)
}

/// Whether a peer's public value is usable: 1 < y < p − 1 (RFC 8268 §4, NIST SP 800-56A).
pub fn valid_public(y: &[u8]) -> bool {
    match (group(), fixed_width(y)) {
        (Some(g), Some(y)) => g.valid_peer(&y),
        _ => false,
    }
}

/// The shared secret y^x mod p (left-padded to [`P_LEN`]); `None` for an unusable `y`.
pub fn shared(peer: &[u8], private: &[u8]) -> Option<Vec<u8>> {
    group()?.shared(private, &fixed_width(peer)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The group 14 prime exactly as RFC 3526 §3 prints it.
    const RFC_3526_GROUP_14: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DD\
EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED\
EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F\
83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B\
E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995497CEA956AE515D2261898FA0510\
15728E5A8AACAA68FFFFFFFFFFFFFFFF";

    fn minus_one(p: &[u8]) -> Vec<u8> {
        let mut v = p.to_vec();
        if let Some(last) = v.last_mut() {
            *last -= 1; // the prime ends in 0xFF
        }
        v
    }

    #[test]
    fn the_prime_is_the_one_in_rfc_3526() {
        let want: Vec<u8> =
            RFC_3526_GROUP_14.as_bytes().chunks(2).map(|h| u8::from_str_radix(std::str::from_utf8(h).unwrap(), 16).unwrap()).collect();
        assert_eq!(prime(), want);
        assert_eq!(prime().len(), P_LEN);
        assert!(group().unwrap().fermat_ok());
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
        for bad in [vec![0u8], vec![1u8], minus_one(&p), p.clone(), vec![0xff; P_LEN + 2], Vec::new()] {
            assert!(shared(&bad, &a).is_none());
            assert!(!valid_public(&bad));
        }
        let mut padded = vec![0u8];
        padded.extend_from_slice(&yb);
        assert_eq!(shared(&padded, &a), shared(&yb, &a), "an mpint's leading zero is fine");
        assert!(valid_public(&yb) && valid_public(&padded));
    }

    #[test]
    fn a_value_with_leading_zeros_stripped_still_works() {
        // An mpint drops leading zero bytes: the value must be read the same either way.
        let a = [0x11u8; PRIVATE_LEN];
        let y = public(&[0x42; PRIVATE_LEN]);
        let mut stripped: Vec<u8> = y.iter().copied().skip_while(|b| *b == 0).collect();
        assert_eq!(shared(&stripped, &a), shared(&y, &a));
        stripped.insert(0, 0);
        assert_eq!(shared(&stripped, &a), shared(&y, &a));
    }
}
