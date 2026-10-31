//! RIPng (RFC 2080): RIP for IPv6. A response carries route table entries of 20 bytes each
//! (prefix, route tag, prefix length, metric) after a 4-byte header (command 2, version 1),
//! over UDP port 521 to `FF02::9` from the sender's link-local address.

use std::net::Ipv6Addr;

use netlabcraft_core::Ipv6Net;

use crate::{Result, WireError};

/// RIPng's UDP port (RFC 2080 §2.5) and its routers group.
pub const RIPNG_PORT: u16 = 521;
pub const RIPNG_ROUTERS: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 9);
/// A response's command.
const COMMAND_RESPONSE: u8 = 2;
/// The only version.
const VERSION: u8 = 1;
/// Most entries one message carries here (hostile input; an Ethernet MTU fits 72).
pub const MAX_ENTRIES: usize = 72;

/// One route table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RipngEntry {
    pub net: Ipv6Net,
    pub tag: u16,
    /// 1-16; 16 is unreachable.
    pub metric: u8,
}

/// A RIPng response.
pub fn encode(entries: &[RipngEntry]) -> Vec<u8> {
    let mut v = vec![COMMAND_RESPONSE, VERSION, 0, 0];
    for e in entries.iter().take(MAX_ENTRIES) {
        v.extend_from_slice(&e.net.network().addr.octets());
        v.extend_from_slice(&e.tag.to_be_bytes());
        v.push(e.net.prefix.min(128));
        v.push(e.metric.clamp(1, 16));
    }
    v
}

/// The entries of a RIPng response.
pub fn decode(b: &[u8]) -> Result<Vec<RipngEntry>> {
    let t = || WireError::Truncated("ripng");
    let head = b.get(..4).ok_or_else(t)?;
    if head.first() != Some(&COMMAND_RESPONSE) || head.get(1) != Some(&VERSION) {
        return Err(WireError::Unsupported("ripng", format!("command {} version {}", head.first().unwrap_or(&0), head.get(1).unwrap_or(&0))));
    }
    let body = b.get(4..).ok_or_else(t)?;
    if body.len() % 20 != 0 {
        return Err(t());
    }
    if body.len() / 20 > MAX_ENTRIES {
        return Err(WireError::Unsupported("ripng", format!("{} entries", body.len() / 20)));
    }
    body.chunks(20)
        .map(|c| {
            let a: [u8; 16] = c.get(..16).ok_or_else(t)?.try_into().map_err(|_| t())?;
            let tag = u16::from_be_bytes([*c.get(16).ok_or_else(t)?, *c.get(17).ok_or_else(t)?]);
            let prefix = *c.get(18).ok_or_else(t)?;
            let metric = *c.get(19).ok_or_else(t)?;
            if prefix > 128 || !(1..=16).contains(&metric) {
                return Err(WireError::Unsupported("ripng", format!("prefix length {prefix}, metric {metric}")));
            }
            Ok(RipngEntry { net: Ipv6Net::new(Ipv6Addr::from(a), prefix), tag, metric })
        })
        .collect()
}

/// One line for the event list.
pub fn describe(entries: &[RipngEntry]) -> String {
    let list: Vec<String> = entries.iter().take(4).map(|e| format!("{} ({})", e.net, e.metric)).collect();
    let more = if entries.len() > 4 { format!(" and {} more", entries.len() - 4) } else { String::new() };
    format!("RIPng response, {} route{}: {}{more}", entries.len(), if entries.len() == 1 { "" } else { "s" }, list.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_response_round_trips_and_every_truncation_fails() {
        let e = vec![
            RipngEntry { net: "2001:db8:1::/64".parse().unwrap(), tag: 0, metric: 1 },
            RipngEntry { net: "2001:db8:2::/48".parse().unwrap(), tag: 7, metric: 16 },
        ];
        let b = encode(&e);
        assert_eq!(b.len(), 4 + 40);
        assert_eq!(&b[..4], &[2, 1, 0, 0]);
        assert_eq!(&b[20..24], &[0, 0, 64, 1]);
        assert_eq!(decode(&b).unwrap(), e);
        for n in 0..b.len() {
            if n != 4 && n != 24 {
                assert!(decode(&b[..n]).is_err(), "{n}");
            }
        }
        // Hostile: a request, a bad metric or prefix length, too many entries.
        assert!(decode(&[1, 1, 0, 0]).is_err());
        let mut bad = b.clone();
        bad[23] = 17;
        assert!(decode(&bad).is_err());
        bad[23] = 1;
        bad[22] = 200;
        assert!(decode(&bad).is_err());
        let mut many = vec![2, 1, 0, 0];
        many.extend(std::iter::repeat_n(encode(&e[..1])[4..].to_vec(), MAX_ENTRIES + 1).flatten());
        assert!(decode(&many).is_err());
        assert!(describe(&e).starts_with("RIPng response, 2 routes: 2001:db8:1::/64 (1)"));
    }
}
