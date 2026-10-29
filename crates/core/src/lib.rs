//! PacketCraft L0: the value types every other crate shares.
//!
//! - [`MacAddr`]: an Ethernet address (IEEE 802).
//! - [`Ipv4Net`]: an IPv4 address with its prefix length (an interface address or a route).
//! - [`VlanId`] and [`VlanSet`]: IEEE 802.1Q VLAN ids (1–4094) and sets of them.
//! - [`DeviceId`]: a device's stable id in a topology.
//!
//! Everything here parses hostile input (files, commands, MCP parameters) without panicking.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Why a value didn't parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    #[error("`{0}` is not a MAC address")]
    Mac(String),
    #[error("`{0}` is not an IPv4 address")]
    Ip(String),
    #[error("`{0}` is not a contiguous subnet mask")]
    Mask(String),
    #[error("`{0}` is not a prefix length (0-32)")]
    Prefix(String),
    #[error("`{0}` is not a VLAN id (1-4094)")]
    Vlan(String),
}

/// A device's id, stable for the life of a topology (never reused after removal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub u32);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// An Ethernet (IEEE 802) address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr([0xff; 6]);
    pub const ZERO: MacAddr = MacAddr([0; 6]);

    /// A locally administered unicast address (`02:…`) derived from a device id and port index,
    /// so a saved lab keeps its addresses.
    pub fn local(device: DeviceId, port: u16) -> MacAddr {
        let d = device.0.to_be_bytes();
        let p = port.to_be_bytes();
        MacAddr([0x02, d[1], d[2], d[3], p[0], p[1]])
    }

    pub fn is_broadcast(self) -> bool {
        self == Self::BROADCAST
    }

    /// Group (multicast or broadcast) address: the I/G bit of the first octet.
    pub fn is_group(self) -> bool {
        self.0[0] & 1 == 1
    }

    /// The dotted three-group form (`0200.0000.0101`) that switch tables print.
    pub fn dotted(self) -> String {
        let b = self.0;
        format!("{:02x}{:02x}.{:02x}{:02x}.{:02x}{:02x}", b[0], b[1], b[2], b[3], b[4], b[5])
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2], b[3], b[4], b[5])
    }
}

impl FromStr for MacAddr {
    type Err = CoreError;
    /// Accepts `aa:bb:cc:dd:ee:ff`, `aa-bb-cc-dd-ee-ff` and `aabb.ccdd.eeff`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || CoreError::Mac(s.to_string());
        let hex: String = s.chars().filter(|c| !matches!(c, ':' | '-' | '.')).collect();
        if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(err());
        }
        let mut out = [0u8; 6];
        for (i, byte) in out.iter_mut().enumerate() {
            let pair = hex.get(i * 2..i * 2 + 2).ok_or_else(err)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| err())?;
        }
        Ok(MacAddr(out))
    }
}

impl Serialize for MacAddr {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for MacAddr {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Parses a dotted-quad IPv4 address.
pub fn parse_ip(s: &str) -> Result<Ipv4Addr, CoreError> {
    s.parse().map_err(|_| CoreError::Ip(s.to_string()))
}

/// The prefix length of a contiguous subnet mask (`255.255.255.0` → 24).
pub fn mask_to_prefix(mask: Ipv4Addr) -> Option<u8> {
    let m = u32::from(mask);
    let ones = m.leading_ones();
    if m.checked_shl(ones).unwrap_or(0) == 0 { u8::try_from(ones).ok() } else { None }
}

/// The subnet mask of a prefix length (24 → `255.255.255.0`); lengths above 32 saturate.
pub fn prefix_to_mask(prefix: u8) -> Ipv4Addr {
    let p = u32::from(prefix.min(32));
    Ipv4Addr::from(if p == 0 { 0 } else { u32::MAX << (32 - p) })
}

/// An IPv4 address with a prefix length: an interface address (`10.0.0.1/24`) or, with the host
/// bits clear, a network (`10.0.0.0/24`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv4Net {
    pub addr: Ipv4Addr,
    pub prefix: u8,
}

impl Ipv4Net {
    /// `prefix` is clamped to 32.
    pub fn new(addr: Ipv4Addr, prefix: u8) -> Ipv4Net {
        Ipv4Net { addr, prefix: prefix.min(32) }
    }

    /// From an address and a dotted mask, as interface commands take them.
    pub fn with_mask(addr: Ipv4Addr, mask: Ipv4Addr) -> Result<Ipv4Net, CoreError> {
        let prefix = mask_to_prefix(mask).ok_or_else(|| CoreError::Mask(mask.to_string()))?;
        Ok(Ipv4Net { addr, prefix })
    }

    pub fn mask(self) -> Ipv4Addr {
        prefix_to_mask(self.prefix)
    }

    /// The network: host bits cleared.
    pub fn network(self) -> Ipv4Net {
        Ipv4Net { addr: Ipv4Addr::from(u32::from(self.addr) & u32::from(self.mask())), prefix: self.prefix }
    }

    /// The directed broadcast address (all host bits set).
    pub fn broadcast(self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.addr) | !u32::from(self.mask()))
    }

    pub fn contains(self, ip: Ipv4Addr) -> bool {
        let m = u32::from(self.mask());
        u32::from(ip) & m == u32::from(self.addr) & m
    }

    /// Whether this address is the network or the broadcast address of its own subnet (not
    /// usable on an interface, except on /31 and /32).
    pub fn is_network_or_broadcast(self) -> bool {
        self.prefix < 31 && (self.addr == self.network().addr || self.addr == self.broadcast())
    }

    /// Whether two subnets share any address.
    pub fn overlaps(self, other: Ipv4Net) -> bool {
        let p = self.prefix.min(other.prefix);
        Ipv4Net::new(self.addr, p).contains(other.addr)
    }
}

impl fmt::Display for Ipv4Net {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

impl FromStr for Ipv4Net {
    type Err = CoreError;
    /// `10.0.0.1/24`; a bare address is a /32.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('/') {
            Some((a, p)) => {
                let prefix: u8 = p.parse().map_err(|_| CoreError::Prefix(p.to_string()))?;
                if prefix > 32 {
                    return Err(CoreError::Prefix(p.to_string()));
                }
                Ok(Ipv4Net { addr: parse_ip(a)?, prefix })
            }
            None => Ok(Ipv4Net { addr: parse_ip(s)?, prefix: 32 }),
        }
    }
}

impl Serialize for Ipv4Net {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Ipv4Net {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// An IEEE 802.1Q VLAN id, 1–4094.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct VlanId(u16);

impl VlanId {
    pub const DEFAULT: VlanId = VlanId(1);
    pub const MAX: u16 = 4094;

    pub fn new(v: u16) -> Option<VlanId> {
        (1..=Self::MAX).contains(&v).then_some(VlanId(v))
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for VlanId {
    type Error = CoreError;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        VlanId::new(v).ok_or_else(|| CoreError::Vlan(v.to_string()))
    }
}

impl From<VlanId> for u16 {
    fn from(v: VlanId) -> u16 {
        v.0
    }
}

impl fmt::Display for VlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for VlanId {
    type Err = CoreError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim().parse::<u16>().ok().and_then(VlanId::new).ok_or_else(|| CoreError::Vlan(s.to_string()))
    }
}

/// A set of VLAN ids, as a trunk's allowed list: a 4096-bit bitmap.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct VlanSet([u64; 64]);

impl Default for VlanSet {
    fn default() -> Self {
        VlanSet::all()
    }
}

impl fmt::Debug for VlanSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VlanSet({self})")
    }
}

impl VlanSet {
    pub fn none() -> VlanSet {
        VlanSet([0; 64])
    }

    /// 1–4094.
    pub fn all() -> VlanSet {
        let mut s = VlanSet::none();
        for v in 1..=VlanId::MAX {
            s.set(v, true);
        }
        s
    }

    fn set(&mut self, v: u16, on: bool) {
        let (word, bit) = (usize::from(v / 64), v % 64);
        if let Some(w) = self.0.get_mut(word) {
            if on {
                *w |= 1 << bit;
            } else {
                *w &= !(1 << bit);
            }
        }
    }

    pub fn contains(&self, v: VlanId) -> bool {
        let (word, bit) = (usize::from(v.0 / 64), v.0 % 64);
        self.0.get(word).is_some_and(|w| w & (1 << bit) != 0)
    }

    pub fn insert(&mut self, v: VlanId) {
        self.set(v.0, true);
    }

    pub fn remove(&mut self, v: VlanId) {
        self.set(v.0, false);
    }

    pub fn union(&mut self, other: &VlanSet) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a |= b;
        }
    }

    pub fn subtract(&mut self, other: &VlanSet) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a &= !b;
        }
    }

    pub fn is_all(&self) -> bool {
        *self == VlanSet::all()
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|w| *w == 0)
    }

    pub fn iter(&self) -> impl Iterator<Item = VlanId> + '_ {
        (1..=VlanId::MAX).filter_map(VlanId::new).filter(|v| self.contains(*v))
    }

    /// Parses a list such as `1-10,20,30-32`, or `all` / `none`.
    pub fn parse_list(s: &str) -> Result<VlanSet, CoreError> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("all") {
            return Ok(VlanSet::all());
        }
        if t.eq_ignore_ascii_case("none") {
            return Ok(VlanSet::none());
        }
        let mut set = VlanSet::none();
        for part in t.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('-') {
                Some((a, b)) => {
                    let (a, b): (VlanId, VlanId) = (a.parse()?, b.parse()?);
                    if a > b {
                        return Err(CoreError::Vlan(part.to_string()));
                    }
                    for v in a.0..=b.0 {
                        set.set(v, true);
                    }
                }
                None => set.insert(part.parse()?),
            }
        }
        if set.is_empty() {
            return Err(CoreError::Vlan(s.to_string()));
        }
        Ok(set)
    }
}

impl fmt::Display for VlanSet {
    /// Compact ranges (`1-10,20`); `all` / `none` for the full and empty sets.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_all() {
            return f.write_str("all");
        }
        if self.is_empty() {
            return f.write_str("none");
        }
        let mut parts = Vec::new();
        let mut run: Option<(u16, u16)> = None;
        for v in self.iter().map(VlanId::get) {
            run = match run {
                Some((a, b)) if b + 1 == v => Some((a, v)),
                Some((a, b)) => {
                    parts.push(if a == b { a.to_string() } else { format!("{a}-{b}") });
                    Some((v, v))
                }
                None => Some((v, v)),
            };
        }
        if let Some((a, b)) = run {
            parts.push(if a == b { a.to_string() } else { format!("{a}-{b}") });
        }
        f.write_str(&parts.join(","))
    }
}

impl Serialize for VlanSet {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for VlanSet {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        VlanSet::parse_list(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_forms_round_trip() {
        let m: MacAddr = "02:00:00:00:01:0a".parse().unwrap();
        assert_eq!(m.to_string(), "02:00:00:00:01:0a");
        assert_eq!(m.dotted(), "0200.0000.010a");
        assert_eq!("0200.0000.010a".parse::<MacAddr>().unwrap(), m);
        assert_eq!("02-00-00-00-01-0A".parse::<MacAddr>().unwrap(), m);
        assert!("02:00:00".parse::<MacAddr>().is_err());
        assert!("zz:00:00:00:01:0a".parse::<MacAddr>().is_err());
        assert!("ä2:00:00:00:01:0a".parse::<MacAddr>().is_err());
        assert!(MacAddr::BROADCAST.is_group() && !m.is_group());
        assert_eq!(MacAddr::local(DeviceId(0x0102), 3).to_string(), "02:00:01:02:00:03");
    }

    #[test]
    fn masks_and_prefixes() {
        assert_eq!(mask_to_prefix(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
        assert_eq!(mask_to_prefix(Ipv4Addr::new(0, 0, 0, 0)), Some(0));
        assert_eq!(mask_to_prefix(Ipv4Addr::new(255, 255, 255, 255)), Some(32));
        assert_eq!(mask_to_prefix(Ipv4Addr::new(255, 0, 255, 0)), None);
        assert_eq!(prefix_to_mask(26), Ipv4Addr::new(255, 255, 255, 192));
        assert_eq!(prefix_to_mask(0), Ipv4Addr::UNSPECIFIED);
        assert_eq!(prefix_to_mask(99), Ipv4Addr::BROADCAST);
    }

    #[test]
    fn subnet_math() {
        let n: Ipv4Net = "192.168.1.77/26".parse().unwrap();
        assert_eq!(n.network().to_string(), "192.168.1.64/26");
        assert_eq!(n.broadcast(), Ipv4Addr::new(192, 168, 1, 127));
        assert!(n.contains(Ipv4Addr::new(192, 168, 1, 100)));
        assert!(!n.contains(Ipv4Addr::new(192, 168, 1, 128)));
        assert!(!n.is_network_or_broadcast());
        assert!("10.0.0.0/24".parse::<Ipv4Net>().unwrap().is_network_or_broadcast());
        assert!("10.0.0.0/31".parse::<Ipv4Net>().is_ok_and(|n| !n.is_network_or_broadcast()));
        assert!("10.0.0.0/8".parse::<Ipv4Net>().unwrap().overlaps("10.9.0.0/16".parse().unwrap()));
        assert!(!"10.0.0.0/24".parse::<Ipv4Net>().unwrap().overlaps("10.0.1.0/24".parse().unwrap()));
        assert!("10.0.0.1/33".parse::<Ipv4Net>().is_err());
        assert!(Ipv4Net::with_mask(Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(255, 0, 255, 0)).is_err());
        let all: Ipv4Net = "0.0.0.0/0".parse().unwrap();
        assert!(all.contains(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn vlan_ids_and_sets() {
        assert!(VlanId::new(0).is_none() && VlanId::new(4095).is_none() && VlanId::new(4094).is_some());
        let s = VlanSet::parse_list("1-3, 10,20-21").unwrap();
        assert_eq!(s.to_string(), "1-3,10,20-21");
        assert!(s.contains(VlanId::new(2).unwrap()) && !s.contains(VlanId::new(4).unwrap()));
        assert_eq!(VlanSet::all().to_string(), "all");
        assert_eq!(VlanSet::parse_list("none").unwrap().to_string(), "none");
        assert!(VlanSet::parse_list("5-2").is_err());
        assert!(VlanSet::parse_list("0").is_err());
        assert!(VlanSet::parse_list("").is_err());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<VlanSet>(&json).unwrap(), s);
        assert!(serde_json::from_str::<VlanId>("5000").is_err());
    }
}
