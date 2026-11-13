//! One block cipher in CBC mode for IKE and ESP to use without caring which it is: AES with
//! a 128-, 192- or 256-bit key (16-byte blocks, [`crate::aes256`]), DES or three-key 3DES
//! (8-byte blocks, [`crate::des`]). The IV is as long as the block.

use crate::aes256::{self, Aes};
use crate::des::{self, Des, Tdes};

/// Which cipher a key is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherKind {
    /// AES-CBC; the key length (16, 24 or 32 bytes) picks the variant.
    Aes,
    /// DES-CBC, an 8-byte key.
    Des,
    /// Three-key 3DES-CBC, a 24-byte key.
    Tdes,
}

impl CipherKind {
    /// Bytes of a block, and of the IV.
    pub fn block_len(self) -> usize {
        match self {
            CipherKind::Aes => 16,
            CipherKind::Des | CipherKind::Tdes => 8,
        }
    }
}

/// A keyed block cipher.
#[derive(Debug, Clone)]
pub enum Cipher {
    Aes(Aes),
    Des(Des),
    Tdes(Box<Tdes>),
}

impl Cipher {
    /// The cipher of `kind` for `key`; `None` when the key has a length the cipher cannot take.
    pub fn new(kind: CipherKind, key: &[u8]) -> Option<Cipher> {
        match kind {
            CipherKind::Aes => Aes::new(key).map(Cipher::Aes),
            CipherKind::Des => Des::new(key).map(Cipher::Des),
            CipherKind::Tdes => Tdes::new(key).map(|t| Cipher::Tdes(Box::new(t))),
        }
    }

    pub fn kind(&self) -> CipherKind {
        match self {
            Cipher::Aes(_) => CipherKind::Aes,
            Cipher::Des(_) => CipherKind::Des,
            Cipher::Tdes(_) => CipherKind::Tdes,
        }
    }

    /// Bytes of a block, and of the IV.
    pub fn block_len(&self) -> usize {
        self.kind().block_len()
    }

    /// CBC encryption of whole blocks (a ragged tail is left out). An IV shorter than the block
    /// is padded with zeros and a longer one cut: the callers always give one block.
    pub fn cbc_encrypt(&self, iv: &[u8], data: &[u8]) -> Vec<u8> {
        match self {
            Cipher::Aes(a) => aes256::cbc_encrypt(a, &fit(iv), data),
            Cipher::Des(d) => des::cbc_encrypt(d, &fit(iv), data),
            Cipher::Tdes(t) => des::tdes_cbc_encrypt(t, &fit(iv), data),
        }
    }

    /// CBC decryption of whole blocks.
    pub fn cbc_decrypt(&self, iv: &[u8], data: &[u8]) -> Vec<u8> {
        match self {
            Cipher::Aes(a) => aes256::cbc_decrypt(a, &fit(iv), data),
            Cipher::Des(d) => des::cbc_decrypt(d, &fit(iv), data),
            Cipher::Tdes(t) => des::tdes_cbc_decrypt(t, &fit(iv), data),
        }
    }
}

/// An IV of exactly `N` bytes from whatever was given.
fn fit<const N: usize>(iv: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    for (o, b) in out.iter_mut().zip(iv) {
        *o = *b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_and_agrees_with_its_module() {
        let data = [0x5au8; 48];
        for (kind, key) in
            [(CipherKind::Aes, vec![1u8; 16]), (CipherKind::Aes, vec![2; 32]), (CipherKind::Des, vec![3; 8]), (CipherKind::Tdes, vec![4; 24])]
        {
            let c = Cipher::new(kind, &key).unwrap();
            let iv = vec![9u8; kind.block_len()];
            let ct = c.cbc_encrypt(&iv, &data);
            assert_eq!(ct.len(), 48);
            assert_ne!(ct, data.to_vec());
            assert_eq!(c.cbc_decrypt(&iv, &ct), data.to_vec());
            assert_eq!(c.block_len(), kind.block_len());
        }
        let d = Cipher::new(CipherKind::Des, &[3; 8]).unwrap();
        assert_eq!(d.cbc_encrypt(&[9; 8], &data), des::cbc_encrypt(&Des::new(&[3; 8]).unwrap(), &[9; 8], &data));
        // A key of the wrong length builds nothing.
        assert!(Cipher::new(CipherKind::Des, &[0; 24]).is_none());
        assert!(Cipher::new(CipherKind::Tdes, &[0; 8]).is_none());
        assert!(Cipher::new(CipherKind::Aes, &[0; 8]).is_none());
        // A short IV is tolerated rather than a panic.
        let _ = d.cbc_encrypt(&[], &data);
    }
}
