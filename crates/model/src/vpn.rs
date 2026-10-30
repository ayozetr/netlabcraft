//! Site-to-site VPN configuration of a router: GRE tunnel interfaces and basic IPsec
//! (ISAKMP policies and keys, transform sets, crypto maps and where they are applied).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::Ipv4Net;
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
    /// `set ikev2-profile NAME`: negotiate with IKEv2 instead of IKEv1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ikev2_profile: Option<String>,
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
    /// IKEv2 proposals, policies, keyrings and profiles.
    #[serde(default, skip_serializing_if = "Ikev2::is_empty")]
    pub ikev2: Ikev2,
}

/// `crypto ikev2 proposal NAME`: the IKE SA algorithms offered (RFC 7296 §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ikev2Proposal {
    pub name: String,
    /// `encryption aes-cbc-128 …`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encryption: Vec<String>,
    /// `integrity sha1 …`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrity: Vec<String>,
    /// `group 14 …`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group: Vec<u8>,
}

impl Ikev2Proposal {
    /// Complete enough to offer: a cipher, an integrity algorithm and a group.
    pub fn complete(&self) -> bool {
        !self.encryption.is_empty() && !self.integrity.is_empty() && !self.group.is_empty()
    }
}

/// `crypto ikev2 policy NAME`: which proposals to use (for which local address).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ikev2Policy {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<String>,
    /// `match address local A.B.C.D`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<Ipv4Addr>,
}

/// `peer NAME` in a keyring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ikev2Peer {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Ipv4Addr>,
    /// `pre-shared-key KEY` (both directions) or `pre-shared-key local KEY`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_key: Option<String>,
    /// `pre-shared-key KEY` (both directions) or `pre-shared-key remote KEY`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_key: Option<String>,
}

/// `crypto ikev2 keyring NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ikev2Keyring {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<Ikev2Peer>,
}

/// `crypto ikev2 profile NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ikev2Profile {
    pub name: String,
    /// `match identity remote address A.B.C.D [MASK]`: (address, mask).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote: Vec<(Ipv4Addr, Ipv4Addr)>,
    /// `identity local address A.B.C.D` (the local interface's address otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<Ipv4Addr>,
    /// `authentication local pre-share` / `authentication remote pre-share`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_local: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_remote: Option<String>,
    /// `keyring local NAME`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring: Option<String>,
}

impl Ikev2Profile {
    /// Whether a peer's identity (its address) matches this profile.
    pub fn matches(&self, peer: Ipv4Addr) -> bool {
        self.remote.iter().any(|(a, m)| u32::from(*a) & u32::from(*m) == u32::from(peer) & u32::from(*m))
    }
}

/// A router's IKEv2 configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ikev2 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<Ikev2Proposal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<Ikev2Policy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyrings: Vec<Ikev2Keyring>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<Ikev2Profile>,
}

impl Ikev2 {
    pub fn is_empty(&self) -> bool {
        *self == Ikev2::default()
    }

    pub fn profile(&self, name: &str) -> Option<&Ikev2Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    pub fn proposal(&self, name: &str) -> Option<&Ikev2Proposal> {
        self.proposals.iter().find(|p| p.name == name)
    }

    /// The proposals offered from `local`: those of the policies matching the address (or
    /// with no address), in order. Without policies or proposals: none.
    pub fn offered(&self, local: Ipv4Addr) -> Vec<&Ikev2Proposal> {
        let mut out: Vec<&Ikev2Proposal> = Vec::new();
        let specific = self.policies.iter().any(|p| p.local == Some(local));
        for pol in self.policies.iter().filter(|p| if specific { p.local == Some(local) } else { p.local.is_none() }) {
            for n in &pol.proposals {
                if let Some(p) = self.proposal(n).filter(|p| p.complete())
                    && !out.iter().any(|o| o.name == p.name)
                {
                    out.push(p);
                }
            }
        }
        out
    }

    /// The pre-shared key a profile's keyring holds for a peer: (to sign with, to check with).
    pub fn keys_for(&self, profile: &Ikev2Profile, peer: Ipv4Addr) -> Option<(String, String)> {
        let ring = self.keyrings.iter().find(|k| Some(&k.name) == profile.keyring.as_ref())?;
        let p =
            ring.peers.iter().find(|p| p.address == Some(peer)).or_else(|| ring.peers.iter().find(|p| p.address == Some(Ipv4Addr::UNSPECIFIED)))?;
        Some((p.local_key.clone()?, p.remote_key.clone()?))
    }

    /// Caps every list (hostile files).
    pub fn sanitize(&mut self) {
        self.proposals.truncate(MAX_CRYPTO);
        self.policies.truncate(MAX_CRYPTO);
        self.keyrings.truncate(MAX_CRYPTO);
        self.profiles.truncate(MAX_CRYPTO);
        for p in &mut self.proposals {
            p.encryption.truncate(8);
            p.integrity.truncate(8);
            p.group.truncate(8);
        }
        for p in &mut self.policies {
            p.proposals.truncate(MAX_CRYPTO);
        }
        for k in &mut self.keyrings {
            k.peers.truncate(MAX_CRYPTO);
        }
        for p in &mut self.profiles {
            p.remote.truncate(MAX_CRYPTO);
        }
    }
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
        self.ikev2.sanitize();
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
