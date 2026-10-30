//! Network-management messages: NTP (RFC 5905), syslog (RFC 5424) and TFTP (RFC 1350).

use crate::{Result, WireError, be16};

pub const NTP_PORT: u16 = 123;
pub const SYSLOG_PORT: u16 = 514;
pub const TFTP_PORT: u16 = 69;
/// Seconds from 1900-01-01 (NTP's era 0) to 1970-01-01.
const NTP_UNIX_OFFSET: i64 = 2_208_988_800;

/// An NTP packet: a client request (mode 3) or a server reply (mode 4) carrying the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ntp {
    pub server: bool,
    pub stratum: u8,
    /// Unix seconds: the transmit timestamp.
    pub time: i64,
}

impl Ntp {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = vec![0u8; 48];
        v[0] = (4 << 3) | if self.server { 4 } else { 3 };
        v[1] = self.stratum;
        v[2] = 6; // poll: 2^6 = 64 s
        v[3] = 0xec; // precision: about a microsecond
        let secs = u32::try_from(self.time.saturating_add(NTP_UNIX_OFFSET).clamp(0, i64::from(u32::MAX))).unwrap_or(0);
        if self.server {
            // Reference and receive timestamps equal the transmit one here.
            for at in [16, 32] {
                v[at..at + 4].copy_from_slice(&secs.to_be_bytes());
            }
        }
        v[40..44].copy_from_slice(&secs.to_be_bytes());
        v
    }

    pub fn decode(b: &[u8]) -> Result<Ntp> {
        if b.len() < 48 {
            return Err(WireError::Truncated("ntp"));
        }
        let mode = b[0] & 7;
        if !(3..=4).contains(&mode) {
            return Err(WireError::Unsupported("ntp", format!("mode {mode}")));
        }
        let secs = u32::from_be_bytes([b[40], b[41], b[42], b[43]]);
        Ok(Ntp { server: mode == 4, stratum: b[1], time: i64::from(secs) - NTP_UNIX_OFFSET })
    }
}

/// Appends an NTP message authentication code (RFC 5905 §7.3): the key identifier, then the MD5
/// digest of the key followed by the 48-byte header.
pub fn ntp_sign(packet: &[u8], key_id: u32, secret: &str) -> Vec<u8> {
    let mut v = packet.get(..48).unwrap_or(packet).to_vec();
    let digest = crate::md5::md5(&[secret.as_bytes(), &v].concat());
    v.extend_from_slice(&key_id.to_be_bytes());
    v.extend_from_slice(&digest);
    v
}

/// The key identifier of a signed NTP message.
pub fn ntp_key_id(b: &[u8]) -> Option<u32> {
    b.get(48..52).filter(|_| b.len() >= 68).map(|k| u32::from_be_bytes([k[0], k[1], k[2], k[3]]))
}

/// Whether a signed NTP message's digest matches this key.
pub fn ntp_verify(b: &[u8], secret: &str) -> bool {
    match (b.get(..48), b.get(52..68)) {
        (Some(head), Some(mac)) => crate::md5::md5(&[secret.as_bytes(), head].concat()) == mac,
        _ => false,
    }
}

/// A syslog message (RFC 5424 header, facility local7, severity notice).
pub fn syslog_encode(host: &str, msg: &str) -> Vec<u8> {
    format!("<189>1 - {host} netlabcraft - - - {msg}").into_bytes()
}

/// The (host, message) of a syslog datagram.
pub fn syslog_decode(b: &[u8]) -> Result<(String, String)> {
    let s = std::str::from_utf8(b).map_err(|_| WireError::Unsupported("syslog", "not text".into()))?;
    let rest = s.strip_prefix('<').and_then(|r| r.split_once('>')).map(|(_, r)| r).ok_or(WireError::Unsupported("syslog", "no priority".into()))?;
    let parts: Vec<&str> = rest.splitn(7, ' ').collect();
    match parts.as_slice() {
        [_, _, host, _, _, _, msg] => Ok((host.to_string(), msg.trim_start_matches("- ").to_string())),
        _ => Ok((String::new(), rest.to_string())),
    }
}

/// A TFTP packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tftp {
    /// Read request (1) and write request (2), always in octet mode here.
    Read(String),
    Write(String),
    /// Block number and up to 512 bytes; a shorter block ends the transfer.
    Data(u16, Vec<u8>),
    Ack(u16),
    Error(u16, String),
}

/// TFTP's block size.
pub const TFTP_BLOCK: usize = 512;

impl Tftp {
    pub fn encode(&self) -> Vec<u8> {
        let req = |op: u8, name: &str| [&[0, op][..], name.as_bytes(), &[0], b"octet", &[0]].concat();
        match self {
            Tftp::Read(n) => req(1, n),
            Tftp::Write(n) => req(2, n),
            Tftp::Data(b, d) => [&[0, 3][..], &b.to_be_bytes(), d].concat(),
            Tftp::Ack(b) => [&[0, 4][..], &b.to_be_bytes()].concat(),
            Tftp::Error(c, m) => [&[0, 5][..], &c.to_be_bytes(), m.as_bytes(), &[0]].concat(),
        }
    }

    pub fn decode(b: &[u8]) -> Result<Tftp> {
        let t = || WireError::Truncated("tftp");
        let op = be16(b, 0).ok_or_else(t)?;
        let rest = b.get(2..).unwrap_or_default();
        let name = || -> Result<String> {
            let end = rest.iter().position(|c| *c == 0).ok_or_else(t)?;
            Ok(String::from_utf8_lossy(rest.get(..end).unwrap_or_default()).into_owned())
        };
        match op {
            1 => Ok(Tftp::Read(name()?)),
            2 => Ok(Tftp::Write(name()?)),
            3 => Ok(Tftp::Data(be16(b, 2).ok_or_else(t)?, b.get(4..).unwrap_or_default().to_vec())),
            4 => Ok(Tftp::Ack(be16(b, 2).ok_or_else(t)?)),
            5 => {
                let msg = b.get(4..).unwrap_or_default();
                let end = msg.iter().position(|c| *c == 0).unwrap_or(msg.len());
                Ok(Tftp::Error(be16(b, 2).ok_or_else(t)?, String::from_utf8_lossy(msg.get(..end).unwrap_or_default()).into_owned()))
            }
            other => Err(WireError::Unsupported("tftp", format!("opcode {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn ntp_signatures() {
        let p = Ntp { server: true, stratum: 3, time: 1_800_000_000 }.encode();
        let signed = ntp_sign(&p, 7, "secret");
        assert_eq!(signed.len(), 68);
        assert_eq!(ntp_key_id(&signed), Some(7));
        assert!(ntp_verify(&signed, "secret"));
        assert!(!ntp_verify(&signed, "other"));
        assert_eq!(ntp_key_id(&p), None);
        assert!(!ntp_verify(&p, "secret"));
        assert_eq!(Ntp::decode(&signed).unwrap(), Ntp::decode(&p).unwrap());
        for n in 0..signed.len() {
            let _ = (ntp_key_id(&signed[..n]), ntp_verify(&signed[..n], "secret"), Ntp::decode(&signed[..n]));
        }
    }

    use super::*;

    #[test]
    fn ntp_syslog_and_tftp_round_trip() {
        let n = Ntp { server: true, stratum: 2, time: 1_800_000_000 };
        assert_eq!(Ntp::decode(&n.encode()).unwrap(), n);
        assert!(Ntp::decode(&[0; 10]).is_err());
        assert_eq!(syslog_decode(&syslog_encode("R1", "link up")).unwrap(), ("R1".into(), "link up".into()));
        for p in [Tftp::Read("a.cfg".into()), Tftp::Write("b".into()), Tftp::Data(3, vec![1, 2]), Tftp::Ack(9), Tftp::Error(1, "no file".into())] {
            let b = p.encode();
            assert_eq!(Tftp::decode(&b).unwrap(), p);
            for k in 0..b.len() {
                let _ = Tftp::decode(&b[..k]);
            }
        }
    }
}
