//! SHA-1 (FIPS 180-4), HMAC-SHA1 (RFC 2104), PBKDF2 (RFC 8018) and the IEEE 802.11i PRF: what
//! WPA2-Personal needs to turn a passphrase into the keys that sign its 4-way handshake.
//! Not for anything that needs to be secure.

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
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
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

pub fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let inner = sha1(&[&ipad[..], data].concat());
    sha1(&[&opad[..], &inner[..]].concat())
}

/// PBKDF2 with HMAC-SHA1: `len` bytes from `password` and `salt` after `rounds` iterations.
pub fn pbkdf2_sha1(password: &[u8], salt: &[u8], rounds: u32, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 20);
    let mut block = 1u32;
    while out.len() < len {
        let mut u = hmac_sha1(password, &[salt, &block.to_be_bytes()[..]].concat());
        let mut acc = u;
        for _ in 1..rounds {
            u = hmac_sha1(password, &u);
            for (a, b) in acc.iter_mut().zip(u) {
                *a ^= b;
            }
        }
        out.extend_from_slice(&acc);
        block = block.wrapping_add(1);
    }
    out.truncate(len);
    out
}

/// The IEEE 802.11i pseudo-random function: `len` bytes of HMAC-SHA1(key, label, 0, data, i).
pub fn prf(key: &[u8], label: &str, data: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 20);
    let mut i = 0u8;
    while out.len() < len {
        out.extend_from_slice(&hmac_sha1(key, &[label.as_bytes(), &[0], data, &[i]].concat()));
        i = i.wrapping_add(1);
    }
    out.truncate(len);
    out
}

/// The WPA2-Personal pairwise master key: PBKDF2(passphrase, SSID, 4096 rounds, 32 bytes).
pub fn wpa_pmk(passphrase: &str, ssid: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&pbkdf2_sha1(passphrase.as_bytes(), ssid.as_bytes(), 4096, 32));
    out
}

/// The pairwise transient key (48 bytes for CCMP) from the PMK, both addresses and both nonces;
/// its first 16 bytes (the KCK) sign the handshake messages.
pub fn wpa_ptk(pmk: &[u8; 32], aa: &[u8; 6], spa: &[u8; 6], anonce: &[u8; 32], snonce: &[u8; 32]) -> Vec<u8> {
    let (lo, hi) = if aa <= spa { (aa, spa) } else { (spa, aa) };
    let (n1, n2) = if anonce <= snonce { (anonce, snonce) } else { (snonce, anonce) };
    let data = [&lo[..], &hi[..], &n1[..], &n2[..]].concat();
    prf(pmk, "Pairwise key expansion", &data, 48)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn known_answers() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        // RFC 2202 test case 1.
        assert_eq!(hex(&hmac_sha1(&[0x0b; 20], b"Hi There")), "b617318655057264e28bc0b6fb378c8ef146be00");
        // IEEE 802.11i Annex: passphrase "password", SSID "IEEE".
        assert_eq!(hex(&wpa_pmk("password", "IEEE")), "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e");
        assert_eq!(prf(b"k", "x", b"y", 50).len(), 50);
    }
}
