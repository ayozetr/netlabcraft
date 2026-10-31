//! AES with any of its three key sizes (FIPS 197: 128, 192 or 256 bits, so 10, 12 or 14
//! rounds), and CBC over it (NIST SP 800-38A §6.2), for ESP's `esp-aes 192|256` and IKE's
//! `aes-cbc-192|256`. The round function is the one in [`crate::aes`]; only the key expansion
//! (FIPS 197 §5.2, with the extra SubWord of 256-bit keys) and the round count differ.

use crate::aes::{INV_SBOX, SBOX, gmul, xtime};

/// An AES key, expanded into its round keys.
#[derive(Clone)]
pub struct Aes {
    rk: Vec<[u8; 16]>,
}

impl std::fmt::Debug for Aes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Aes{}(..)", (self.rk.len() - 7) * 32)
    }
}

impl Aes {
    /// A 16-, 24- or 32-byte key; `None` for any other length.
    pub fn new(key: &[u8]) -> Option<Aes> {
        let nk = match key.len() {
            16 => 4,
            24 => 6,
            32 => 8,
            _ => return None,
        };
        let rounds = nk + 6;
        let total = 4 * (rounds + 1);
        let mut w: Vec<[u8; 4]> = key.as_chunks::<4>().0.to_vec();
        let mut rcon: u8 = 1;
        for i in nk..total {
            let mut t = *w.get(i - 1)?;
            if i % nk == 0 {
                t = [SBOX[t[1] as usize] ^ rcon, SBOX[t[2] as usize], SBOX[t[3] as usize], SBOX[t[0] as usize]];
                rcon = xtime(rcon);
            } else if nk > 6 && i % nk == 4 {
                t = t.map(|b| SBOX[b as usize]);
            }
            let prev = *w.get(i - nk)?;
            w.push([prev[0] ^ t[0], prev[1] ^ t[1], prev[2] ^ t[2], prev[3] ^ t[3]]);
        }
        let rk = w
            .as_chunks::<4>()
            .0
            .iter()
            .map(|four| {
                let mut k = [0u8; 16];
                for (c, word) in four.iter().enumerate() {
                    k[4 * c..4 * c + 4].copy_from_slice(word);
                }
                k
            })
            .collect();
        Some(Aes { rk })
    }

    fn rounds(&self) -> usize {
        self.rk.len() - 1
    }

    /// Encrypts one 16-byte block.
    pub fn encrypt(&self, block: &[u8; 16]) -> [u8; 16] {
        let n = self.rounds();
        let mut s = *block;
        add(&mut s, &self.rk[0]);
        for round in 1..=n {
            for b in s.iter_mut() {
                *b = SBOX[*b as usize];
            }
            let o = s;
            for c in 0..4 {
                for r in 0..4 {
                    s[r + 4 * c] = o[r + 4 * ((c + r) % 4)];
                }
            }
            if round != n {
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

    /// Decrypts one 16-byte block.
    pub fn decrypt(&self, block: &[u8; 16]) -> [u8; 16] {
        let n = self.rounds();
        let mut s = *block;
        add(&mut s, &self.rk[n]);
        for round in (0..n).rev() {
            let o = s;
            for c in 0..4 {
                for r in 0..4 {
                    s[r + 4 * ((c + r) % 4)] = o[r + 4 * c];
                }
            }
            for b in s.iter_mut() {
                *b = INV_SBOX[*b as usize];
            }
            add(&mut s, &self.rk[round]);
            if round != 0 {
                for c in 0..4 {
                    let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    for r in 0..4 {
                        s[4 * c + r] = gmul(a[r], 14) ^ gmul(a[(r + 1) % 4], 11) ^ gmul(a[(r + 2) % 4], 13) ^ gmul(a[(r + 3) % 4], 9);
                    }
                }
            }
        }
        s
    }
}

fn add(s: &mut [u8; 16], k: &[u8; 16]) {
    for (a, b) in s.iter_mut().zip(k) {
        *a ^= b;
    }
}

/// CBC encryption of whole blocks; a ragged tail is left out.
pub fn cbc_encrypt(aes: &Aes, iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = *iv;
    for chunk in data.as_chunks::<16>().0 {
        let mut b = *chunk;
        add(&mut b, &prev);
        prev = aes.encrypt(&b);
        out.extend_from_slice(&prev);
    }
    out
}

/// CBC decryption of whole blocks; a ragged tail is left out.
pub fn cbc_decrypt(aes: &Aes, iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = *iv;
    for chunk in data.as_chunks::<16>().0 {
        let mut p = aes.decrypt(chunk);
        add(&mut p, &prev);
        out.extend_from_slice(&p);
        prev = *chunk;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn block(s: &str) -> [u8; 16] {
        hex(s).try_into().unwrap()
    }

    #[test]
    fn fips_197_appendix_c_all_key_sizes() {
        let pt = block("00112233445566778899aabbccddeeff");
        for (key, ct) in [
            ("000102030405060708090a0b0c0d0e0f", "69c4e0d86a7b0430d8cdb78070b4c55a"),
            ("000102030405060708090a0b0c0d0e0f1011121314151617", "dda97ca4864cdfe06eaf70a0ec0d7191"),
            ("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f", "8ea2b7ca516745bfeafc49904b496089"),
        ] {
            let aes = Aes::new(&hex(key)).unwrap();
            assert_eq!(aes.encrypt(&pt), block(ct), "{key}");
            assert_eq!(aes.decrypt(&block(ct)), pt, "{key}");
        }
        assert!(Aes::new(&[0; 15]).is_none() && Aes::new(&[0; 33]).is_none() && Aes::new(&[]).is_none());
    }

    #[test]
    fn fips_197_appendix_a3_key_expansion() {
        // The last round key of the 256-bit example key.
        let aes = Aes::new(&hex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4")).unwrap();
        assert_eq!(aes.rk.len(), 15);
        assert_eq!(aes.rk[14].to_vec(), hex("fe4890d1e6188d0b046df344706c631e"));
    }

    #[test]
    fn sp_800_38a_cbc_aes256() {
        let aes = Aes::new(&hex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4")).unwrap();
        let iv = block("000102030405060708090a0b0c0d0e0f");
        let pt = hex("6bc1bee22e409f96e93d7e117393172a ae2d8a571e03ac9c9eb76fac45af8e51");
        let ct = hex("f58c4c04d6e5f1ba779eabfb5f7bfbd6 9cfc4e967edb808d679f777bc6702c7d");
        assert_eq!(cbc_encrypt(&aes, &iv, &pt), ct);
        assert_eq!(cbc_decrypt(&aes, &iv, &ct), pt);
        // AES-128 here agrees with the original module.
        let k = [7u8; 16];
        let a = Aes::new(&k).unwrap();
        assert_eq!(cbc_encrypt(&a, &iv, &pt), crate::aes::cbc_encrypt(&crate::aes::Aes128::new(&k), &iv, &pt));
    }
}
