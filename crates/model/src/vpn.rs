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
    /// `tunnel protection ipsec profile NAME`: the GRE packets of this tunnel are protected by
    /// IPsec with the settings of that profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<String>,
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
    /// `set security-association lifetime seconds|kilobytes N` (the global values otherwise).
    #[serde(default, skip_serializing_if = "SaLifetime::is_unset")]
    pub lifetime: SaLifetime,
    /// A `crypto dynamic-map` entry: a template for the clients of remote-access VPNs, not a
    /// site-to-site entry (it has no peer and protects nothing by itself).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub template: bool,
    /// `crypto map NAME SEQ ipsec-isakmp dynamic DYNAMIC`: the entry hands clients the template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_ref: Option<String>,
    /// `reverse-route` in a dynamic map entry.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reverse_route: bool,
    /// `set pfs groupN`: the Diffie-Hellman group of the fresh exchange that gives every new
    /// set of IPsec keys (perfect forward secrecy); `None` derives them from the IKE SA alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pfs: Option<u16>,
    /// Only on the entry made for a tunnel protected by an IPsec profile (never stored): the
    /// (local, peer) addresses of its GRE packets, which are what the entry protects instead of
    /// an access list's traffic.
    #[serde(skip)]
    pub gre_ends: Option<(Ipv4Addr, Ipv4Addr)>,
}

/// `crypto ipsec profile NAME`: the IPsec settings of a protected tunnel interface (a crypto map
/// entry without a peer or an access list, which the tunnel gives).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpsecProfile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_set: Option<String>,
    /// `set pfs groupN`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pfs: Option<u16>,
    /// `set security-association lifetime seconds|kilobytes N` (the global values otherwise).
    #[serde(default, skip_serializing_if = "SaLifetime::is_unset")]
    pub lifetime: SaLifetime,
    /// `set ikev2-profile NAME`: negotiate with IKEv2 instead of IKEv1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ikev2_profile: Option<String>,
}

impl IpsecProfile {
    pub fn new(name: &str) -> IpsecProfile {
        IpsecProfile { name: name.to_string(), transform_set: None, pfs: None, lifetime: SaLifetime::default(), ikev2_profile: None }
    }
}

/// The Diffie-Hellman groups `set pfs` accepts, all of them computed for real.
pub const PFS_GROUPS: [u16; 8] = [2, 5, 14, 15, 16, 19, 20, 24];

/// An IPsec SA lifetime: seconds, kilobytes, or neither set (the defaults then apply).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SaLifetime {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kilobytes: Option<u32>,
}

/// The lifetime an IPsec SA gets when nothing is configured.
pub const DEFAULT_SA_SECONDS: u32 = 3_600;
pub const DEFAULT_SA_KILOBYTES: u32 = 4_608_000;
/// The lifetime ranges accepted.
pub const SA_SECONDS: std::ops::RangeInclusive<u32> = 120..=86_400;
pub const SA_KILOBYTES: std::ops::RangeInclusive<u32> = 2_560..=u32::MAX;

impl SaLifetime {
    pub fn is_unset(&self) -> bool {
        *self == SaLifetime::default()
    }

    /// Out-of-range values (hostile files) are forgotten.
    pub fn sanitize(&mut self) {
        self.seconds = self.seconds.filter(|s| SA_SECONDS.contains(s));
        self.kilobytes = self.kilobytes.filter(|k| SA_KILOBYTES.contains(k));
    }
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
    /// `crypto ipsec profile NAME`, in the order they were made.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<IpsecProfile>,
    /// `crypto map NAME` on an interface: interface name → map name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<String, String>,
    /// IKEv2 proposals, policies, keyrings and profiles.
    #[serde(default, skip_serializing_if = "Ikev2::is_empty")]
    pub ikev2: Ikev2,
    /// `crypto ipsec security-association lifetime seconds|kilobytes N`.
    #[serde(default, skip_serializing_if = "SaLifetime::is_unset")]
    pub sa_lifetime: SaLifetime,
    /// The remote-access (Easy VPN style) server: client groups, address pools, AAA lists.
    #[serde(default, skip_serializing_if = "RemoteAccess::is_empty")]
    pub ra: RemoteAccess,
}

/// Most addresses one `ip local pool` may hold.
pub const MAX_POOL_ADDRESSES: u32 = 1_024;

/// `ip local pool NAME FIRST LAST`: the addresses remote-access clients are given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPool {
    pub name: String,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
}

impl LocalPool {
    /// How many addresses the pool holds (at least one for a valid pool).
    pub fn size(&self) -> u32 {
        u32::from(self.end).saturating_sub(u32::from(self.start)).saturating_add(1)
    }

    /// The addresses in order, at most [`MAX_POOL_ADDRESSES`].
    pub fn addresses(&self) -> impl Iterator<Item = Ipv4Addr> {
        let first = u32::from(self.start);
        let count = if u32::from(self.end) >= first { self.size().min(MAX_POOL_ADDRESSES) } else { 0 };
        (0..count).map(move |i| Ipv4Addr::from(first.saturating_add(i)))
    }
}

/// `crypto isakmp client configuration group NAME`: what a remote client that names the group
/// and knows its key is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientGroup {
    pub name: String,
    /// `key KEY`: the group pre-shared key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// `pool NAME`: the local pool the client's address comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}

/// The server side of remote-access VPN clients.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RemoteAccess {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pools: Vec<LocalPool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<ClientGroup>,
    /// `aaa authentication login NAME local`: the names of the lists that check a user locally.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub login_lists: Vec<String>,
    /// `aaa authorization network NAME local`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authz_lists: Vec<String>,
    /// `crypto map NAME client authentication list LIST`: map name → list name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub client_auth: BTreeMap<String, String>,
    /// `crypto map NAME isakmp authorization list LIST`: map name → list name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub isakmp_authz: BTreeMap<String, String>,
    /// `crypto map NAME client configuration address respond|initiate`: map name → word.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub client_address: BTreeMap<String, String>,
}

impl RemoteAccess {
    pub fn is_empty(&self) -> bool {
        *self == RemoteAccess::default()
    }

    pub fn pool(&self, name: &str) -> Option<&LocalPool> {
        self.pools.iter().find(|p| p.name == name)
    }

    pub fn group(&self, name: &str) -> Option<&ClientGroup> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// Caps every list and drops pools that make no sense (hostile files).
    pub fn sanitize(&mut self) {
        self.pools.retain(|p| u32::from(p.end) >= u32::from(p.start) && p.size() <= MAX_POOL_ADDRESSES);
        self.pools.truncate(MAX_CRYPTO);
        self.groups.truncate(MAX_CRYPTO);
        self.login_lists.truncate(MAX_CRYPTO);
        self.authz_lists.truncate(MAX_CRYPTO);
        for m in [&mut self.client_auth, &mut self.isakmp_authz, &mut self.client_address] {
            while m.len() > MAX_CRYPTO {
                m.pop_last();
            }
        }
        for g in &mut self.groups {
            g.key = g.key.chars().take(MAX_CRYPTO_TEXT).collect();
        }
    }
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
    /// `lifetime SECONDS`: how long an IKE SA made under this profile lives (a day otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<u32>,
    /// `pki trustpoint NAME`: the trustpoint whose certificate signs this profile's IKE_AUTH
    /// (the first usable trustpoint when none is named).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trustpoint: Option<String>,
}

/// The IKE SA lifetime when a profile sets none, and the range it accepts.
pub const DEFAULT_IKE_SECONDS: u32 = 86_400;
pub const IKE_SECONDS: std::ops::RangeInclusive<u32> = 120..=86_400;

impl Ikev2Profile {
    /// `authentication local rsa-sig`: this router signs IKE_AUTH with a certificate.
    pub fn rsa_local(&self) -> bool {
        self.auth_local.as_deref() == Some("rsa-sig")
    }

    /// `authentication remote rsa-sig`: the peer must sign IKE_AUTH with a certificate.
    pub fn rsa_remote(&self) -> bool {
        self.auth_remote.as_deref() == Some("rsa-sig")
    }

    /// The lifetime an IKE SA made under this profile gets, in seconds.
    pub fn ike_lifetime(&self) -> u32 {
        self.lifetime.filter(|s| IKE_SECONDS.contains(s)).unwrap_or(DEFAULT_IKE_SECONDS)
    }

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
            p.trustpoint = p.trustpoint.take().map(|t| t.chars().take(64).collect());
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

    pub fn profile(&self, name: &str) -> Option<&IpsecProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// The crypto map entry a protected tunnel stands for: its profile's settings, the tunnel's
    /// destination as the peer and the tunnel's GRE packets (from `local`) as the traffic.
    /// `None` when the tunnel is not protected, the profile does not exist or there is no
    /// destination.
    pub fn profile_entry(&self, tun: &Tunnel, local: Ipv4Addr) -> Option<CryptoMapEntry> {
        let prof = self.profile(tun.protection.as_deref()?)?;
        let peer = tun.destination?;
        Some(CryptoMapEntry {
            map: format!("{}-head-0", tun.name()),
            seq: 1,
            peer: Some(peer),
            transform_set: prof.transform_set.clone(),
            acl: None,
            ikev2_profile: prof.ikev2_profile.clone(),
            lifetime: prof.lifetime,
            template: false,
            dynamic_ref: None,
            reverse_route: false,
            pfs: prof.pfs,
            gre_ends: Some((local, peer)),
        })
    }

    /// The entries of one map, in sequence order.
    pub fn map(&self, name: &str) -> impl Iterator<Item = &CryptoMapEntry> {
        self.maps.iter().filter(move |e| e.map == name)
    }

    /// The lifetime an entry's SAs get (seconds, kilobytes): its own, else the global one, else
    /// the defaults.
    pub fn lifetime(&self, e: &CryptoMapEntry) -> (u32, u32) {
        (
            e.lifetime.seconds.or(self.sa_lifetime.seconds).unwrap_or(DEFAULT_SA_SECONDS),
            e.lifetime.kilobytes.or(self.sa_lifetime.kilobytes).unwrap_or(DEFAULT_SA_KILOBYTES),
        )
    }

    /// Caps every list (hostile files).
    pub fn sanitize(&mut self) {
        self.sa_lifetime.sanitize();
        for e in &mut self.maps {
            e.lifetime.sanitize();
        }
        for p in &mut self.profiles {
            p.lifetime.sanitize();
            p.pfs = p.pfs.filter(|g| PFS_GROUPS.contains(g));
        }
        self.profiles.truncate(MAX_CRYPTO);
        self.policies.truncate(MAX_CRYPTO);
        self.keys.truncate(MAX_CRYPTO);
        self.transform_sets.truncate(MAX_CRYPTO);
        self.maps.truncate(MAX_CRYPTO);
        while self.applied.len() > MAX_CRYPTO {
            self.applied.pop_last();
        }
        self.ikev2.sanitize();
        self.ra.sanitize();
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
    fn a_local_pool_lists_its_addresses_and_is_bounded() {
        let pool = |a: [u8; 4], b: [u8; 4]| LocalPool { name: "P".into(), start: Ipv4Addr::from(a), end: Ipv4Addr::from(b) };
        let p = pool([10, 9, 9, 1], [10, 9, 9, 3]);
        assert_eq!(p.size(), 3);
        assert_eq!(p.addresses().last(), Some(Ipv4Addr::new(10, 9, 9, 3)));
        assert_eq!(pool([10, 0, 0, 0], [10, 255, 255, 255]).addresses().count(), MAX_POOL_ADDRESSES as usize);
        assert_eq!(pool([10, 9, 9, 5], [10, 9, 9, 1]).addresses().count(), 0);
        let mut ra = RemoteAccess {
            pools: vec![pool([10, 0, 0, 0], [10, 255, 255, 255]), p.clone(), pool([10, 9, 9, 5], [10, 9, 9, 1])],
            ..RemoteAccess::default()
        };
        ra.sanitize();
        assert_eq!(ra.pools, vec![p]);
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
