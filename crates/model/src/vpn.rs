//! Site-to-site VPN configuration of a router: GRE tunnel interfaces and basic IPsec
//! (ISAKMP policies and keys, transform sets, crypto maps and where they are applied).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use packetcraft_core::Ipv4Net;
use serde::{Deserialize, Serialize};

/// Most tunnel interfaces per router.
pub const MAX_TUNNELS: usize = 256;
/// Most entries in each crypto list (policies, keys, transform sets, map entries).
pub const MAX_CRYPTO: usize = 64;
/// Highest tunnel interface number.
pub const MAX_TUNNEL_NUMBER: u32 = 2_147_483_647;
/// Longest pre-shared key or crypto name accepted.
pub const MAX_CRYPTO_TEXT: usize = 128;

/// Where a tunnel's packets come from: an interface's address, or an address given outright.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TunnelSource {
    Interface(String),
    Address(Ipv4Addr),
}

/// `interface Tunnel N`: a point-to-point GRE tunnel (RFC 2784) over IPv4.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tunnel {
    pub number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Ipv4Net>,
    #[serde(default)]
    pub shutdown: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<TunnelSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<Ipv4Addr>,
}

impl Tunnel {
    pub fn name(&self) -> String {
        format!("Tunnel{}", self.number)
    }
}

/// The tunnel number in an interface name (`Tunnel0`, `tunnel 0`, `tu0`), if it names one.
pub fn tunnel_number(name: &str) -> Option<u32> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let split = compact.find(|c: char| c.is_ascii_digit())?;
    let (word, num) = compact.split_at(split);
    let word = word.to_ascii_lowercase();
    if word.len() < 2 || !"tunnel".starts_with(&word) {
        return None;
    }
    num.parse::<u32>().ok().filter(|n| *n <= MAX_TUNNEL_NUMBER)
}

/// `crypto isakmp policy N`: the phase-1 proposal (kept and compared, not run as a protocol).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsakmpPolicy {
    pub priority: u16,
    pub encryption: String,
    pub hash: String,
    pub authentication: String,
    pub group: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<u32>,
}

impl IsakmpPolicy {
    /// A new policy with the defaults most routers use.
    pub fn new(priority: u16) -> IsakmpPolicy {
        IsakmpPolicy { priority, encryption: "des".into(), hash: "sha".into(), authentication: "rsa-sig".into(), group: 1, lifetime: None }
    }

    /// Whether two policies propose the same protection (the lifetime may differ).
    pub fn agrees(&self, o: &IsakmpPolicy) -> bool {
        self.encryption == o.encryption && self.hash == o.hash && self.authentication == o.authentication && self.group == o.group
    }
}

/// `crypto ipsec transform-set NAME T1 [T2 …]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformSet {
    pub name: String,
    pub transforms: Vec<String>,
    /// `mode transport` (tunnel mode otherwise).
    #[serde(default)]
    pub transport: bool,
}

/// One `crypto map NAME SEQ ipsec-isakmp` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoMapEntry {
    pub map: String,
    pub seq: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_set: Option<String>,
    /// `match address ACL`: the traffic to protect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acl: Option<String>,
}

/// A router's IPsec configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Crypto {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<IsakmpPolicy>,
    /// `crypto isakmp key KEY address A.B.C.D`: (peer address, key). 0.0.0.0 matches any peer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<(Ipv4Addr, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transform_sets: Vec<TransformSet>,
    /// Sorted by map name, then sequence number.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<CryptoMapEntry>,
    /// `crypto map NAME` on an interface: interface name → map name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<String, String>,
}

impl Crypto {
    pub fn is_empty(&self) -> bool {
        *self == Crypto::default()
    }

    /// The pre-shared key for a peer: its own line, else a wildcard (0.0.0.0) line.
    pub fn key_for(&self, peer: Ipv4Addr) -> Option<&str> {
        self.keys.iter().find(|(a, _)| *a == peer).or_else(|| self.keys.iter().find(|(a, _)| a.is_unspecified())).map(|(_, k)| k.as_str())
    }

    pub fn transform_set(&self, name: &str) -> Option<&TransformSet> {
        self.transform_sets.iter().find(|t| t.name == name)
    }

    /// The entries of one map, in sequence order.
    pub fn map(&self, name: &str) -> impl Iterator<Item = &CryptoMapEntry> {
        self.maps.iter().filter(move |e| e.map == name)
    }

    /// Caps every list (hostile files).
    pub fn sanitize(&mut self) {
        self.policies.truncate(MAX_CRYPTO);
        self.keys.truncate(MAX_CRYPTO);
        self.transform_sets.truncate(MAX_CRYPTO);
        self.maps.truncate(MAX_CRYPTO);
        while self.applied.len() > MAX_CRYPTO {
            self.applied.pop_last();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_names() {
        assert_eq!(tunnel_number("Tunnel0"), Some(0));
        assert_eq!(tunnel_number("tunnel 12"), Some(12));
        assert_eq!(tunnel_number("tu1"), Some(1));
        assert_eq!(tunnel_number("t1"), None);
        assert_eq!(tunnel_number("g0/0"), None);
        assert_eq!(tunnel_number("tunnel"), None);
        assert_eq!(tunnel_number("tunnel 99999999999"), None);
        assert_eq!(tunnel_number("tunnel 1/2"), None);
    }

    #[test]
    fn keys_fall_back_to_the_wildcard() {
        let mut c = Crypto::default();
        c.keys.push((Ipv4Addr::UNSPECIFIED, "any".into()));
        c.keys.push((Ipv4Addr::new(1, 1, 1, 1), "one".into()));
        assert_eq!(c.key_for(Ipv4Addr::new(1, 1, 1, 1)), Some("one"));
        assert_eq!(c.key_for(Ipv4Addr::new(2, 2, 2, 2)), Some("any"));
    }
}
