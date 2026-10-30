//! Site-to-site VPN encapsulations: GRE (RFC 2784) and IPsec ESP (RFC 4303).
//!
//! GRE is the real wire format. ESP has the real header and trailer layout (SPI, sequence
//! number, padding, pad length, next header, integrity check value), but the "cipher" is
//! **not real encryption**: the payload is XORed with a keystream made from MD5 over the
//! pre-shared key, the SPI, the sequence number and a block counter, and the integrity value
//! is a truncated MD5 over the key and the packet. That is enough for a teaching simulator to
//! show that the payload is unreadable on the wire and that peers with different keys cannot
//! talk; it offers no protection whatsoever and must never be used to protect real data.

use crate::md5::md5;
use crate::{Result, WireError, checksum};

/// IP protocol number of GRE.
pub const PROTO_GRE: u8 = 47;
/// IP protocol number of ESP.
pub const PROTO_ESP: u8 = 50;
/// ESP's "next header" for a whole IPv4 packet inside (tunnel mode).
pub const NEXT_IPV4: u8 = 4;
/// Bytes of integrity check value the simulated `esp-sha-hmac` appends (96 bits, as
/// HMAC-SHA1-96 does).
pub const ICV_LEN: usize = 12;

/// A GRE packet (RFC 2784): an optional checksum, the payload's EtherType, the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gre {
    /// The payload's protocol type, an EtherType (0x0800 for IPv4).
    pub protocol: u16,
    /// Whether the checksum field is present (the C bit).
    pub checksum: bool,
    pub payload: Vec<u8>,
}

impl Gre {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.payload.len());
        out.extend_from_slice(&[if self.checksum { 0x80 } else { 0 }, 0]);
        out.extend_from_slice(&self.protocol.to_be_bytes());
        if self.checksum {
            out.extend_from_slice(&[0, 0, 0, 0]);
        }
        out.extend_from_slice(&self.payload);
        if self.checksum {
            let c = checksum(&[&out]);
            if let Some(slot) = out.get_mut(4..6) {
                slot.copy_from_slice(&c.to_be_bytes());
            }
        }
        out
    }

    pub fn decode(b: &[u8]) -> Result<Gre> {
        let t = || WireError::Truncated("gre");
        let flags = u16::from_be_bytes([*b.first().ok_or_else(t)?, *b.get(1).ok_or_else(t)?]);
        let protocol = u16::from_be_bytes([*b.get(2).ok_or_else(t)?, *b.get(3).ok_or_else(t)?]);
        let version = flags & 0x7;
        if version != 0 {
            return Err(WireError::Unsupported("gre", format!("version {version}")));
        }
        // Bits 1-12 are reserved; RFC 2784 has receivers discard packets with bits 1-5 set
        // (they are the RFC 1701 routing, key and sequence options, not implemented here).
        if flags & 0x7c00 != 0 {
            return Err(WireError::Unsupported("gre", "key, sequence or routing options".into()));
        }
        let with_sum = flags & 0x8000 != 0;
        let start = if with_sum { 8 } else { 4 };
        if b.len() < start {
            return Err(t());
        }
        if with_sum && checksum(&[b]) != 0 {
            return Err(WireError::Checksum("gre"));
        }
        Ok(Gre { protocol, checksum: with_sum, payload: b.get(start..).ok_or_else(t)?.to_vec() })
    }
}

/// An ESP packet (RFC 4303) as it travels: SPI, sequence number, and the protected rest
/// (payload, padding, pad length and next header, then the integrity check value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Esp {
    pub spi: u32,
    pub seq: u32,
    pub data: Vec<u8>,
}

impl Esp {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.data.len());
        out.extend_from_slice(&self.spi.to_be_bytes());
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Esp> {
        let t = || WireError::Truncated("esp");
        let word = |at: usize| -> Result<u32> { Ok(u32::from_be_bytes(b.get(at..at + 4).ok_or_else(t)?.try_into().map_err(|_| t())?)) };
        let spi = word(0)?;
        let seq = word(4)?;
        if spi < 256 {
            // SPI values 1-255 are reserved by IANA and 0 is never sent (RFC 4303 §2.1).
            return Err(WireError::Unsupported("esp", format!("reserved SPI {spi}")));
        }
        Ok(Esp { spi, seq, data: b.get(8..).ok_or_else(t)?.to_vec() })
    }

    /// Wraps `inner` (a packet of protocol `next`) with the simulated cipher and integrity
    /// check. NOT real encryption: see the module documentation.
    pub fn seal(key: &[u8], spi: u32, seq: u32, next: u8, inner: &[u8]) -> Esp {
        let mut body = inner.to_vec();
        // Pad so payload + pad length + next header fill whole 16-byte blocks, as a block
        // cipher such as AES would need; padding bytes count 1, 2, 3… (RFC 4303 §2.4).
        let pad = (16 - (body.len() + 2) % 16) % 16;
        body.extend((1..=pad).map(|i| u8::try_from(i).unwrap_or(0)));
        body.push(u8::try_from(pad).unwrap_or(0));
        body.push(next);
        xor_stream(key, spi, seq, &mut body);
        let icv = icv(key, spi, seq, &body);
        body.extend_from_slice(&icv);
        Esp { spi, seq, data: body }
    }

    /// Checks the integrity value with `key` and undoes the simulated cipher: the next header
    /// and the inner packet. A wrong key fails the integrity check.
    pub fn open(&self, key: &[u8]) -> Result<(u8, Vec<u8>)> {
        let t = || WireError::Truncated("esp");
        let split = self.data.len().checked_sub(ICV_LEN).ok_or_else(t)?;
        let (body, got) = self.data.split_at(split);
        if icv(key, self.spi, self.seq, body).as_slice() != got {
            return Err(WireError::Checksum("esp integrity"));
        }
        let mut plain = body.to_vec();
        xor_stream(key, self.spi, self.seq, &mut plain);
        let next = plain.pop().ok_or_else(t)?;
        let pad = usize::from(plain.pop().ok_or_else(t)?);
        let keep = plain.len().checked_sub(pad).ok_or_else(t)?;
        plain.truncate(keep);
        Ok((next, plain))
    }
}

/// XORs `data` with a keystream: MD5(key ‖ SPI ‖ sequence ‖ block counter), 16 bytes a block.
/// NOT real encryption.
fn xor_stream(key: &[u8], spi: u32, seq: u32, data: &mut [u8]) {
    for (n, chunk) in data.chunks_mut(16).enumerate() {
        let mut seed = key.to_vec();
        seed.extend_from_slice(&spi.to_be_bytes());
        seed.extend_from_slice(&seq.to_be_bytes());
        seed.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes());
        let block = md5(&seed);
        for (b, k) in chunk.iter_mut().zip(block.iter()) {
            *b ^= k;
        }
    }
}

/// The simulated integrity check value: the first [`ICV_LEN`] bytes of MD5(key ‖ header ‖ body).
fn icv(key: &[u8], spi: u32, seq: u32, body: &[u8]) -> [u8; ICV_LEN] {
    let mut seed = key.to_vec();
    seed.extend_from_slice(&spi.to_be_bytes());
    seed.extend_from_slice(&seq.to_be_bytes());
    seed.extend_from_slice(body);
    let h = md5(&seed);
    let mut out = [0u8; ICV_LEN];
    for (o, b) in out.iter_mut().zip(h.iter()) {
        *o = *b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gre_round_trips_with_and_without_checksum() {
        for sum in [false, true] {
            let g = Gre { protocol: 0x0800, checksum: sum, payload: vec![0x45, 1, 2, 3, 4, 5] };
            let b = g.encode();
            assert_eq!(b.len(), if sum { 8 } else { 4 } + 6);
            assert_eq!(Gre::decode(&b).unwrap(), g);
            for n in 0..b.len().min(if sum { 8 } else { 4 }) {
                assert!(Gre::decode(&b[..n]).is_err(), "cut at {n}");
            }
            for n in 0..b.len() {
                let _ = Gre::decode(&b[..n]);
            }
        }
        // RFC 2784 header: no flags, version 0, protocol type 0x0800.
        assert_eq!(&Gre { protocol: 0x0800, checksum: false, payload: vec![] }.encode(), &[0, 0, 8, 0]);
    }

    #[test]
    fn gre_rejects_options_versions_and_bad_checksums() {
        assert!(Gre::decode(&[0x20, 0, 8, 0, 0, 0, 0, 1]).is_err(), "key bit");
        assert!(Gre::decode(&[0, 1, 8, 0]).is_err(), "version 1");
        let mut b = Gre { protocol: 0x0800, checksum: true, payload: vec![9, 9] }.encode();
        if let Some(x) = b.last_mut() {
            *x ^= 0xff;
        }
        assert_eq!(Gre::decode(&b), Err(WireError::Checksum("gre")));
    }

    #[test]
    fn esp_round_trips_and_survives_truncation() {
        let inner: Vec<u8> = (0..50u8).collect();
        let e = Esp::seal(b"secret", 0x1234_5678, 7, NEXT_IPV4, &inner);
        assert_eq!((e.data.len() - ICV_LEN) % 16, 0);
        assert_ne!(&e.data[..inner.len()], inner.as_slice(), "the payload is not in the clear");
        let b = e.encode();
        let back = Esp::decode(&b).unwrap();
        assert_eq!(back, e);
        assert_eq!(back.open(b"secret").unwrap(), (NEXT_IPV4, inner));
        assert!(back.open(b"other").is_err(), "a different key fails the integrity check");
        for n in 0..b.len() {
            if let Ok(x) = Esp::decode(&b[..n]) {
                assert!(x.open(b"secret").is_err());
            }
        }
        assert!(Esp::decode(&[0, 0, 0, 5, 0, 0, 0, 1]).is_err(), "reserved SPI");
        // A forged trailer claiming more padding than there is never panics.
        let mut f = Esp::seal(b"k", 300, 1, NEXT_IPV4, &[]);
        f.data = vec![0xff; ICV_LEN + 1];
        let _ = f.open(b"k");
    }
}
