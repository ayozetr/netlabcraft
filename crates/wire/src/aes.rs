//! AES-128 (FIPS 197, encryption direction only) and the CCM mode built on it (RFC 3610, as
//! IEEE 802.11 CCMP and other protocols use it: counter-mode encryption with a CBC-MAC).
//!
//! Only the forward cipher is needed: CCM decrypts by running the same counter keystream.
//! Written from the published specifications; checked against their test vectors.

/// The AES S-box, generated from its definition (the multiplicative inverse in GF(2^8)
/// followed by the affine transform), rather than typed in.
const SBOX: [u8; 256] = make_sbox();

const fn make_sbox() -> [u8; 256] {
    let mut s = [0u8; 256];
    // p walks the multiplicative group by powers of 3; q by powers of 3's inverse, so q = 1/p.
    let mut p: u8 = 1;
    let mut q: u8 = 1;
    loop {
        p = p ^ (p << 1) ^ if p & 0x80 != 0 { 0x1b } else { 0 };
        q ^= q << 1;
        q ^= q << 2;
        q ^= q << 4;
        if q & 0x80 != 0 {
            q ^= 0x09;
        }
        let x = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4);
        s[p as usize] = x ^ 0x63;
        if p == 1 {
            break;
        }
    }
    s[0] = 0x63;
    s
}

fn xtime(b: u8) -> u8 {
    (b << 1) ^ if b & 0x80 != 0 { 0x1b } else { 0 }
}

/// An AES-128 key, expanded into its eleven round keys.
#[derive(Clone)]
pub struct Aes128 {
    rk: [[u8; 16]; 11],
}

impl std::fmt::Debug for Aes128 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Aes128(..)")
    }
}

impl Aes128 {
    pub fn new(key: &[u8; 16]) -> Aes128 {
        let mut w = [[0u8; 4]; 44];
        for (i, word) in w.iter_mut().take(4).enumerate() {
            for (j, b) in word.iter_mut().enumerate() {
                *b = key.get(4 * i + j).copied().unwrap_or(0);
            }
        }
        let mut rcon: u8 = 1;
        for i in 4..44 {
            let mut t = w[i - 1];
            if i % 4 == 0 {
                t = [SBOX[t[1] as usize] ^ rcon, SBOX[t[2] as usize], SBOX[t[3] as usize], SBOX[t[0] as usize]];
                rcon = xtime(rcon);
            }
            let prev = w[i - 4];
            w[i] = [prev[0] ^ t[0], prev[1] ^ t[1], prev[2] ^ t[2], prev[3] ^ t[3]];
        }
        let mut rk = [[0u8; 16]; 11];
        for (r, k) in rk.iter_mut().enumerate() {
            for c in 0..4 {
                k[4 * c..4 * c + 4].copy_from_slice(&w[4 * r + c]);
            }
        }
        Aes128 { rk }
    }

    /// Encrypts one 16-byte block.
    pub fn encrypt(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut s = *block;
        add(&mut s, &self.rk[0]);
        for round in 1..11 {
            for b in s.iter_mut() {
                *b = SBOX[*b as usize];
            }
            // ShiftRows: row r (bytes r, r+4, r+8, r+12) turns left by r.
            let o = s;
            for c in 0..4 {
                for r in 0..4 {
                    s[r + 4 * c] = o[r + 4 * ((c + r) % 4)];
                }
            }
            if round != 10 {
                for c in 0..4 {
                    let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    let all = a[0] ^ a[1] ^ a[2] ^ a[3];
                    for r in 0..4 {
                        s[4 * c + r] = a[r] ^ all ^ xtime(a[r] ^ a[(r + 1) % 4]);
                    }
                }
            }
            add(&mut s, &self.rk[round]);
        }
        s
    }
}

fn add(s: &mut [u8; 16], k: &[u8; 16]) {
    for (a, b) in s.iter_mut().zip(k) {
        *a ^= b;
    }
}

/// CCM with a 13-byte nonce (so a 2-byte length field, L = 2) and an `M`-byte tag.
#[derive(Debug, Clone)]
pub struct Ccm {
    aes: Aes128,
    tag_len: usize,
}

/// Why CCM refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcmError {
    /// More than 65 535 bytes of message (L = 2), or too much associated data.
    TooLong,
    /// Shorter than the tag.
    Truncated,
    /// The tag doesn't match: wrong key, or the bytes were changed.
    BadTag,
}

impl Ccm {
    /// `tag_len` is 4, 6, 8, 10, 12, 14 or 16; others are rounded into that range.
    pub fn new(key: &[u8; 16], tag_len: usize) -> Ccm {
        let tag_len = (tag_len.clamp(4, 16)) & !1;
        Ccm { aes: Aes128::new(key), tag_len }
    }

    fn ctr_block(nonce: &[u8; 13], i: u16) -> [u8; 16] {
        let mut a = [0u8; 16];
        a[0] = 1; // L - 1
        a[1..14].copy_from_slice(nonce);
        a[14..].copy_from_slice(&i.to_be_bytes());
        a
    }

    fn mac(&self, nonce: &[u8; 13], aad: &[u8], msg: &[u8]) -> Result<[u8; 16], CcmError> {
        let len = u16::try_from(msg.len()).map_err(|_| CcmError::TooLong)?;
        if aad.len() >= 0xff00 {
            return Err(CcmError::TooLong);
        }
        let mut b0 = [0u8; 16];
        b0[0] = (if aad.is_empty() { 0 } else { 0x40 }) | ((((self.tag_len - 2) / 2) as u8) << 3) | 1;
        b0[1..14].copy_from_slice(nonce);
        b0[14..].copy_from_slice(&len.to_be_bytes());
        let mut x = self.aes.encrypt(&b0);
        let absorb = |data: &[u8], x: &mut [u8; 16]| {
            for chunk in data.chunks(16) {
                for (a, b) in x.iter_mut().zip(chunk) {
                    *a ^= b;
                }
                *x = self.aes.encrypt(x);
            }
        };
        if !aad.is_empty() {
            let mut a = (aad.len() as u16).to_be_bytes().to_vec();
            a.extend_from_slice(aad);
            absorb(&a, &mut x);
        }
        absorb(msg, &mut x);
        Ok(x)
    }

    fn ctr(&self, nonce: &[u8; 13], data: &mut [u8]) {
        for (i, chunk) in data.chunks_mut(16).enumerate() {
            let s = self.aes.encrypt(&Ccm::ctr_block(nonce, (i as u16).wrapping_add(1)));
            for (a, b) in chunk.iter_mut().zip(s) {
                *a ^= b;
            }
        }
    }

    /// Encrypts and authenticates: the ciphertext, then the tag.
    pub fn seal(&self, nonce: &[u8; 13], aad: &[u8], msg: &[u8]) -> Result<Vec<u8>, CcmError> {
        let t = self.mac(nonce, aad, msg)?;
        let s0 = self.aes.encrypt(&Ccm::ctr_block(nonce, 0));
        let mut out = msg.to_vec();
        self.ctr(nonce, &mut out);
        out.extend(t.iter().zip(s0).take(self.tag_len).map(|(a, b)| a ^ b));
        Ok(out)
    }

    /// Checks and decrypts what [`Ccm::seal`] made.
    pub fn open(&self, nonce: &[u8; 13], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CcmError> {
        let n = sealed.len().checked_sub(self.tag_len).ok_or(CcmError::Truncated)?;
        let (ct, tag) = sealed.split_at(n);
        let mut msg = ct.to_vec();
        self.ctr(nonce, &mut msg);
        let t = self.mac(nonce, aad, &msg)?;
        let s0 = self.aes.encrypt(&Ccm::ctr_block(nonce, 0));
        let want: Vec<u8> = t.iter().zip(s0).take(self.tag_len).map(|(a, b)| a ^ b).collect();
        // Compare every byte, whatever the first difference.
        let diff = want.iter().zip(tag).fold(0u8, |acc, (a, b)| acc | (a ^ b));
        if diff != 0 || want.len() != tag.len() {
            return Err(CcmError::BadTag);
        }
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn sbox_matches_its_definition() {
        assert_eq!((SBOX[0x00], SBOX[0x01], SBOX[0x53], SBOX[0xff]), (0x63, 0x7c, 0xed, 0x16));
        let mut seen = [false; 256];
        for b in SBOX {
            seen[b as usize] = true;
        }
        assert!(seen.iter().all(|s| *s), "the S-box is a permutation");
    }

    #[test]
    fn fips_197_appendix_c1() {
        let key: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let pt: [u8; 16] = hex("00112233445566778899aabbccddeeff").try_into().unwrap();
        assert_eq!(Aes128::new(&key).encrypt(&pt).to_vec(), hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
    }

    #[test]
    fn fips_197_appendix_b() {
        let key: [u8; 16] = hex("2b7e151628aed2a6abf7158809cf4f3c").try_into().unwrap();
        let pt: [u8; 16] = hex("3243f6a8885a308d313198a2e0370734").try_into().unwrap();
        assert_eq!(Aes128::new(&key).encrypt(&pt).to_vec(), hex("3925841d02dc09fbdc118597196a0b32"));
    }

    #[test]
    fn rfc_3610_packet_vector_1() {
        let key: [u8; 16] = hex("c0c1c2c3c4c5c6c7c8c9cacbcccdcecf").try_into().unwrap();
        let nonce: [u8; 13] = hex("00000003020100a0a1a2a3a4a5").try_into().unwrap();
        let aad = hex("0001020304050607");
        let msg = hex("08090a0b0c0d0e0f101112131415161718191a1b1c1d1e");
        let ccm = Ccm::new(&key, 8);
        let sealed = ccm.seal(&nonce, &aad, &msg).unwrap();
        assert_eq!(sealed, hex("588c979a61c663d2f066d0c2c0f989806d5f6b61dac38417e8d12cfdf926e0"));
        assert_eq!(ccm.open(&nonce, &aad, &sealed).unwrap(), msg);
        let mut bad = sealed.clone();
        bad[3] ^= 1;
        assert_eq!(ccm.open(&nonce, &aad, &bad), Err(CcmError::BadTag));
        assert_eq!(ccm.open(&nonce, b"other", &sealed), Err(CcmError::BadTag));
        for n in 0..sealed.len() {
            assert!(ccm.open(&nonce, &aad, &sealed[..n]).is_err());
        }
    }
}
