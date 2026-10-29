//! DNS messages (RFC 1035): one question, A-record answers. Name compression is read (with a
//! hop limit), never written.

use std::net::Ipv4Addr;

use crate::{Result, WireError};

pub const PORT: u16 = 53;
/// Rcode "name error": no such name.
pub const NXDOMAIN: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dns {
    pub id: u16,
    pub response: bool,
    pub rcode: u8,
    pub name: String,
    pub answers: Vec<Ipv4Addr>,
}

fn write_name(out: &mut Vec<u8>, name: &str) {
    for label in name.trim_end_matches('.').split('.').filter(|l| !l.is_empty()) {
        let l = &label.as_bytes()[..label.len().min(63)];
        out.push(l.len() as u8);
        out.extend_from_slice(l);
    }
    out.push(0);
}

/// Reads a name at `at`; returns it and the offset after it (in the original position).
fn read_name(b: &[u8], mut at: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut end = None;
    for _ in 0..64 {
        let len = *b.get(at)?;
        match len {
            0 => {
                return Some((labels.join("."), end.unwrap_or(at + 1)));
            }
            l if l & 0xc0 == 0xc0 => {
                let ptr = usize::from(u16::from_be_bytes([l & 0x3f, *b.get(at + 1)?]));
                end.get_or_insert(at + 2);
                at = ptr;
            }
            l => {
                let s = b.get(at + 1..at + 1 + usize::from(l))?;
                labels.push(String::from_utf8_lossy(s).to_string());
                at += 1 + usize::from(l);
            }
        }
    }
    None
}

impl Dns {
    pub fn query(id: u16, name: &str) -> Dns {
        Dns { id, response: false, rcode: 0, name: name.to_ascii_lowercase(), answers: Vec::new() }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&self.id.to_be_bytes());
        let flags: u16 = if self.response { 0x8180 | u16::from(self.rcode & 0x0f) } else { 0x0100 };
        b.extend_from_slice(&flags.to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&u16::try_from(self.answers.len()).unwrap_or(0).to_be_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        write_name(&mut b, &self.name);
        b.extend_from_slice(&[0, 1, 0, 1]);
        for a in &self.answers {
            b.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1]);
            b.extend_from_slice(&3600u32.to_be_bytes());
            b.extend_from_slice(&4u16.to_be_bytes());
            b.extend_from_slice(&a.octets());
        }
        b
    }

    pub fn decode(b: &[u8]) -> Result<Dns> {
        let t = || WireError::Truncated("dns");
        if b.len() < 12 {
            return Err(t());
        }
        let id = u16::from_be_bytes([b[0], b[1]]);
        let flags = u16::from_be_bytes([b[2], b[3]]);
        let an = u16::from_be_bytes([b[6], b[7]]).min(64);
        let (name, mut at) = read_name(b, 12).ok_or_else(t)?;
        at += 4;
        let mut answers = Vec::new();
        for _ in 0..an {
            let (_, after) = read_name(b, at).ok_or_else(t)?;
            let rr = b.get(after..after + 10).ok_or_else(t)?;
            let (ty, len) = (u16::from_be_bytes([rr[0], rr[1]]), usize::from(u16::from_be_bytes([rr[8], rr[9]])));
            let data = b.get(after + 10..after + 10 + len).ok_or_else(t)?;
            if ty == 1 && len == 4 {
                answers.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
            }
            at = after + 10 + len;
        }
        Ok(Dns { id, response: flags & 0x8000 != 0, rcode: (flags & 0x0f) as u8, name, answers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_answer_round_trip() {
        let q = Dns::query(7, "WWW.Example.lab");
        assert_eq!(Dns::decode(&q.encode()).unwrap(), q);
        let a = Dns { id: 7, response: true, rcode: 0, name: "www.example.lab".into(), answers: vec![Ipv4Addr::new(10, 0, 0, 5)] };
        let b = a.encode();
        assert_eq!(Dns::decode(&b).unwrap(), a);
        for n in 0..b.len() {
            let _ = Dns::decode(&b[..n]);
        }
        // A pointer loop doesn't hang.
        let mut evil = vec![0u8; 12];
        evil.extend_from_slice(&[0xc0, 12]);
        assert!(Dns::decode(&evil).is_err());
    }
}
