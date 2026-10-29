//! TCP segments (RFC 793): the header fields a lab needs, with the checksum over the IPv4
//! pseudo-header. Options are skipped on decode and never written.

use std::net::Ipv4Addr;

use crate::{PROTO_TCP, Result, WireError, checksum, pseudo_header};

pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const PSH: u8 = 0x08;
pub const ACK: u8 = 0x10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tcp {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub payload: Vec<u8>,
}

impl Tcp {
    pub fn encode(&self, src: Ipv4Addr, dst: Ipv4Addr) -> Vec<u8> {
        let mut b = vec![0u8; 20];
        b[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        b[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        b[4..8].copy_from_slice(&self.seq.to_be_bytes());
        b[8..12].copy_from_slice(&self.ack.to_be_bytes());
        b[12] = 5 << 4;
        b[13] = self.flags;
        b[14..16].copy_from_slice(&self.window.to_be_bytes());
        b.extend_from_slice(&self.payload);
        let len = u16::try_from(b.len()).unwrap_or(u16::MAX);
        let c = checksum(&[&pseudo_header(src, dst, PROTO_TCP, len), &b]);
        if let Some(s) = b.get_mut(16..18) {
            s.copy_from_slice(&c.to_be_bytes());
        }
        b
    }

    pub fn decode(b: &[u8], src: Ipv4Addr, dst: Ipv4Addr) -> Result<Tcp> {
        let t = || WireError::Truncated("tcp");
        if b.len() < 20 {
            return Err(t());
        }
        let off = usize::from(b[12] >> 4) * 4;
        if off < 20 || off > b.len() {
            return Err(t());
        }
        let len = u16::try_from(b.len()).map_err(|_| t())?;
        if checksum(&[&pseudo_header(src, dst, PROTO_TCP, len), b]) != 0 {
            return Err(WireError::Checksum("tcp"));
        }
        let u16_at = |i: usize| u16::from_be_bytes([b[i], b[i + 1]]);
        let u32_at = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Ok(Tcp {
            src_port: u16_at(0),
            dst_port: u16_at(2),
            seq: u32_at(4),
            ack: u32_at(8),
            flags: b[13],
            window: u16_at(14),
            payload: b.get(off..).unwrap_or_default().to_vec(),
        })
    }

    /// `SYN, ACK`-style flag names.
    pub fn flag_names(&self) -> String {
        let names: Vec<&str> = [(SYN, "SYN"), (ACK, "ACK"), (PSH, "PSH"), (FIN, "FIN"), (RST, "RST")]
            .iter()
            .filter(|(f, _)| self.flags & f != 0)
            .map(|(_, n)| *n)
            .collect();
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_checksum_and_truncation() {
        let (s, d) = (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2));
        let seg = Tcp { src_port: 49152, dst_port: 80, seq: 1000, ack: 5, flags: SYN | ACK, window: 65535, payload: b"GET / HTTP/1.1".to_vec() };
        let b = seg.encode(s, d);
        assert_eq!(Tcp::decode(&b, s, d).unwrap(), seg);
        assert_eq!(seg.flag_names(), "SYN, ACK");
        assert_eq!(Tcp::decode(&b, s, Ipv4Addr::new(10, 0, 0, 3)), Err(WireError::Checksum("tcp")));
        for n in 0..b.len() {
            let _ = Tcp::decode(&b[..n], s, d);
        }
    }
}
