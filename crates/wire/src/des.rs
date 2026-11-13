//! DES (FIPS 46-3) and Triple DES (NIST SP 800-67, the three-key EDE form), with CBC over
//! them (NIST SP 800-38A §6.2 and the original FIPS 81), for IKEv1 `encryption des|3des`,
//! IKEv2 ENCR_3DES and ESP `esp-des` / `esp-3des` (RFC 2405, RFC 2451).
//!
//! The tables are those of FIPS 46-3 (initial permutation, expansion, S-boxes, P, PC-1, PC-2
//! and the rotation schedule); the final permutation is computed as the inverse of the initial
//! one. Bit 1 of every table is the most significant bit. This is a teaching implementation:
//! table lookups, not constant time. DES is broken and 3DES deprecated; nothing here is meant
//! to protect real traffic.

/// Bytes in a DES block.
pub const BLOCK: usize = 8;

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6, 64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25,
    17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

/// The inverse of [`IP`]: `FP[IP[i] - 1] = i + 1`.
const FP: [u8; 64] = {
    let mut fp = [0u8; 64];
    let mut i = 0;
    while i < 64 {
        fp[(IP[i] - 1) as usize] = (i + 1) as u8;
        i += 1;
    }
    fp
};

const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18, 19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26,
    27, 28, 29, 28, 29, 30, 31, 32, 1,
];

const P: [u8; 32] = [16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19, 13, 30, 6, 22, 11, 4, 25];

const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60, 52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62,
    54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 28, 20, 12, 4,
];

const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52, 31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49,
    39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];

const SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

const S: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15,
        12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8,
        12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1,
        2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15,
        1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9,
        12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0,
        4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10,
        15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6,
        10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// Applies a permutation table to the low `from` bits of `x` (bit 1 is the most significant of
/// those); the result has `table.len()` bits.
fn permute(x: u64, from: u32, table: &[u8]) -> u64 {
    let mut out = 0u64;
    for &t in table {
        let shift = from.saturating_sub(u32::from(t));
        out = (out << 1) | ((x >> shift) & 1);
    }
    out
}

fn rotl28(x: u64, n: u8) -> u64 {
    ((x << n) | (x >> (28 - n))) & 0x0fff_ffff
}

/// A DES key schedule: sixteen 48-bit round keys.
#[derive(Clone, PartialEq, Eq)]
pub struct Des {
    rk: [u64; 16],
}

impl std::fmt::Debug for Des {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Des(..)")
    }
}

impl Des {
    /// An 8-byte key (the parity bits are ignored, as FIPS 46-3 says); `None` for any other
    /// length.
    pub fn new(key: &[u8]) -> Option<Des> {
        let k = u64::from_be_bytes(<[u8; 8]>::try_from(key).ok()?);
        let cd = permute(k, 64, &PC1);
        let (mut c, mut d) = ((cd >> 28) & 0x0fff_ffff, cd & 0x0fff_ffff);
        let mut rk = [0u64; 16];
        for (slot, shift) in rk.iter_mut().zip(SHIFTS) {
            c = rotl28(c, shift);
            d = rotl28(d, shift);
            *slot = permute((c << 28) | d, 56, &PC2);
        }
        Some(Des { rk })
    }

    fn f(r: u32, k: u64) -> u32 {
        let x = permute(u64::from(r), 32, &E) ^ k;
        let mut out = 0u32;
        for (i, sbox) in S.iter().enumerate() {
            let six = ((x >> (42 - 6 * i)) & 0x3f) as usize;
            // Row from the outer bits, column from the middle four.
            let idx = ((six & 0x20) >> 4 | (six & 1)) * 16 + ((six >> 1) & 0xf);
            out = (out << 4) | u32::from(sbox.get(idx).copied().unwrap_or(0));
        }
        permute(u64::from(out), 32, &P) as u32
    }

    fn crypt(&self, block: u64, decrypt: bool) -> u64 {
        let ip = permute(block, 64, &IP);
        let (mut l, mut r) = ((ip >> 32) as u32, ip as u32);
        for round in 0..16 {
            let k = if decrypt { 15 - round } else { round };
            let key = self.rk.get(k).copied().unwrap_or(0);
            let next = l ^ Des::f(r, key);
            l = r;
            r = next;
        }
        // The halves are swapped before the final permutation.
        permute((u64::from(r) << 32) | u64::from(l), 64, &FP)
    }

    /// Encrypts one 8-byte block.
    pub fn encrypt(&self, block: &[u8; 8]) -> [u8; 8] {
        self.crypt(u64::from_be_bytes(*block), false).to_be_bytes()
    }

    /// Decrypts one 8-byte block.
    pub fn decrypt(&self, block: &[u8; 8]) -> [u8; 8] {
        self.crypt(u64::from_be_bytes(*block), true).to_be_bytes()
    }
}

/// Triple DES in the EDE form with three keys (SP 800-67 §3): E_K3(D_K2(E_K1(x))).
#[derive(Clone, PartialEq, Eq)]
pub struct Tdes {
    k1: Des,
    k2: Des,
    k3: Des,
}

impl std::fmt::Debug for Tdes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tdes(..)")
    }
}

impl Tdes {
    /// A 24-byte key (three DES keys); `None` for any other length.
    pub fn new(key: &[u8]) -> Option<Tdes> {
        if key.len() != 24 {
            return None;
        }
        Some(Tdes { k1: Des::new(key.get(..8)?)?, k2: Des::new(key.get(8..16)?)?, k3: Des::new(key.get(16..)?)? })
    }

    pub fn encrypt(&self, block: &[u8; 8]) -> [u8; 8] {
        self.k3.encrypt(&self.k2.decrypt(&self.k1.encrypt(block)))
    }

    pub fn decrypt(&self, block: &[u8; 8]) -> [u8; 8] {
        self.k1.decrypt(&self.k2.encrypt(&self.k3.decrypt(block)))
    }
}

fn xor8(a: &mut [u8; 8], b: &[u8; 8]) {
    for (x, y) in a.iter_mut().zip(b) {
        *x ^= y;
    }
}

/// CBC over any 8-byte block function; a ragged tail is left out.
fn cbc_with(iv: &[u8; 8], data: &[u8], enc: &dyn Fn(&[u8; 8]) -> [u8; 8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = *iv;
    for chunk in data.as_chunks::<8>().0 {
        let mut b = *chunk;
        xor8(&mut b, &prev);
        prev = enc(&b);
        out.extend_from_slice(&prev);
    }
    out
}

fn cbc_open_with(iv: &[u8; 8], data: &[u8], dec: &dyn Fn(&[u8; 8]) -> [u8; 8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = *iv;
    for chunk in data.as_chunks::<8>().0 {
        let mut p = dec(chunk);
        xor8(&mut p, &prev);
        out.extend_from_slice(&p);
        prev = *chunk;
    }
    out
}

/// DES-CBC encryption of whole blocks.
pub fn cbc_encrypt(des: &Des, iv: &[u8; 8], data: &[u8]) -> Vec<u8> {
    cbc_with(iv, data, &|b| des.encrypt(b))
}

/// DES-CBC decryption of whole blocks.
pub fn cbc_decrypt(des: &Des, iv: &[u8; 8], data: &[u8]) -> Vec<u8> {
    cbc_open_with(iv, data, &|b| des.decrypt(b))
}

/// 3DES-CBC encryption of whole blocks.
pub fn tdes_cbc_encrypt(k: &Tdes, iv: &[u8; 8], data: &[u8]) -> Vec<u8> {
    cbc_with(iv, data, &|b| k.encrypt(b))
}

/// 3DES-CBC decryption of whole blocks.
pub fn tdes_cbc_decrypt(k: &Tdes, iv: &[u8; 8], data: &[u8]) -> Vec<u8> {
    cbc_open_with(iv, data, &|b| k.decrypt(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn b8(s: &str) -> [u8; 8] {
        hex(s).try_into().unwrap()
    }

    #[test]
    fn final_permutation_inverts_the_initial_one() {
        for i in 0..64usize {
            assert_eq!(IP[usize::from(FP[i]) - 1] as usize, i + 1);
        }
        // Every table is a permutation or a selection of the right bit numbers.
        let mut seen = [false; 65];
        for t in IP {
            seen[usize::from(t)] = true;
        }
        assert!(seen[1..].iter().all(|x| *x));
        // Every S-box row is a permutation of 0..15 (FIPS 46-3).
        for s in S {
            for row in s.chunks(16) {
                let mut r = row.to_vec();
                r.sort_unstable();
                assert_eq!(r, (0..16).collect::<Vec<u8>>());
            }
        }
    }

    #[test]
    fn des_known_answers() {
        // The worked example of the DES walk-throughs (key 133457799BBCDFF1).
        let d = Des::new(&hex("133457799BBCDFF1")).unwrap();
        assert_eq!(d.encrypt(&b8("0123456789ABCDEF")), b8("85E813540F0AB405"));
        assert_eq!(d.decrypt(&b8("85E813540F0AB405")), b8("0123456789ABCDEF"));
        // FIPS 81 / SP 800-17 ECB example: "Now is t".
        let d = Des::new(&hex("0123456789ABCDEF")).unwrap();
        assert_eq!(d.encrypt(&b8("4E6F772069732074")), b8("3FA40E8A984D4815"));
        // NBS variable-plaintext known-answer tests (SP 500-20), key 0101010101010101.
        let d = Des::new(&hex("0101010101010101")).unwrap();
        for (pt, ct) in [
            ("8000000000000000", "95F8A5E5DD31D900"),
            ("4000000000000000", "DD7F121CA5015619"),
            ("2000000000000000", "2E8653104F3834EA"),
            ("1000000000000000", "4BD388FF6CD81D4F"),
        ] {
            assert_eq!(d.encrypt(&b8(pt)), b8(ct), "{pt}");
            assert_eq!(d.decrypt(&b8(ct)), b8(pt), "{pt}");
        }
        // Parity bits are ignored.
        let odd = Des::new(&hex("133457799BBCDFF0")).unwrap();
        assert_eq!(odd.encrypt(&b8("0123456789ABCDEF")), b8("85E813540F0AB405"));
        assert!(Des::new(&[0; 7]).is_none() && Des::new(&[0; 9]).is_none() && Des::new(&[]).is_none());
    }

    #[test]
    fn tdes_known_answers() {
        // SP 800-67 appendix B (ECB, three independent keys).
        let k = Tdes::new(&hex("0123456789ABCDEF 23456789ABCDEF01 456789ABCDEF0123")).unwrap();
        let pt = hex("5468652071756663 6b2062726f776e20 666f78206a756d70"); // "The qufck brown fox jump"
        let ct = hex("A826FD8CE53B855F CCE21C8112256FE6 68D5C05DD9B6B900");
        for (p, c) in pt.chunks(8).zip(ct.chunks(8)) {
            let p: [u8; 8] = p.try_into().unwrap();
            let c: [u8; 8] = c.try_into().unwrap();
            assert_eq!(k.encrypt(&p), c);
            assert_eq!(k.decrypt(&c), p);
        }
        // With three equal keys, EDE collapses to single DES.
        let one = hex("0123456789ABCDEF");
        let t = Tdes::new(&[one.clone(), one.clone(), one.clone()].concat()).unwrap();
        assert_eq!(t.encrypt(&b8("4E6F772069732074")), b8("3FA40E8A984D4815"));
        assert!(Tdes::new(&[0; 16]).is_none() && Tdes::new(&[0; 8]).is_none() && Tdes::new(&[]).is_none());
    }

    #[test]
    fn des_cbc_known_answer() {
        // FIPS 81 appendix: key 0123456789abcdef, IV 1234567890abcdef, "Now is the time for all ".
        let d = Des::new(&hex("0123456789abcdef")).unwrap();
        let iv = b8("1234567890abcdef");
        let pt = hex("4e6f77206973207468652074696d6520666f7220616c6c20");
        let ct = hex("e5c7cdde872bf27c43e934008c389c0f683788499a7c05f6");
        assert_eq!(cbc_encrypt(&d, &iv, &pt), ct);
        assert_eq!(cbc_decrypt(&d, &iv, &ct), pt);
        // A ragged tail is left out; a wrong IV garbles only the first block.
        assert_eq!(cbc_encrypt(&d, &iv, &pt[..20]), ct[..16].to_vec());
        let bad = cbc_decrypt(&d, &[0; 8], &ct);
        assert_ne!(bad[..8], pt[..8]);
        assert_eq!(bad[8..], pt[8..]);
    }

    #[test]
    fn tdes_cbc_chains_its_blocks() {
        // CBC built by hand from the verified ECB answers: C1 = E(P1 ^ IV), C2 = E(P2 ^ C1).
        let k = Tdes::new(&hex("0123456789ABCDEF 23456789ABCDEF01 456789ABCDEF0123")).unwrap();
        let iv = b8("fedcba9876543210");
        let pt = hex("5468652071756663 6b2062726f776e20");
        let mut x = b8("5468652071756663");
        xor8(&mut x, &iv);
        let c1 = k.encrypt(&x);
        let mut y = b8("6b2062726f776e20");
        xor8(&mut y, &c1);
        let c2 = k.encrypt(&y);
        let ct = tdes_cbc_encrypt(&k, &iv, &pt);
        assert_eq!(ct, [c1, c2].concat());
        assert_eq!(tdes_cbc_decrypt(&k, &iv, &ct), pt);
    }
}
