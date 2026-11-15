//! The firewall of an end device (a PC, laptop or server): an ordered list of allow and deny rules
//! for what arrives at it, kept apart for IPv4 and IPv6, with one switch per list.
//!
//! Policy, in short: with the switch off everything is let in. With it on, the first rule that
//! matches an inbound packet decides, and a packet no rule matches is dropped. Only traffic that
//! starts something is matched (ICMP echo requests, the first segment of a TCP connection and UDP
//! datagrams); answers to what the device itself sent (echo replies, the rest of a TCP connection,
//! DHCP and DNS answers) always pass, so the device can still use the network.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::Device;

/// Most rules in one list.
pub const MAX_RULES: usize = 32;

/// The protocol a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FwProto {
    Any,
    Tcp,
    Udp,
    Icmp,
}

impl FwProto {
    pub fn name(self) -> &'static str {
        match self {
            FwProto::Any => "any",
            FwProto::Tcp => "tcp",
            FwProto::Udp => "udp",
            FwProto::Icmp => "icmp",
        }
    }

    pub fn from_name(s: &str) -> Option<FwProto> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "any" | "ip" => FwProto::Any,
            "tcp" => FwProto::Tcp,
            "udp" => FwProto::Udp,
            "icmp" | "icmpv6" => FwProto::Icmp,
            _ => return None,
        })
    }
}

/// The kind of inbound packet a rule is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inbound {
    Tcp,
    Udp,
    Icmp,
}

/// Whether `addr` is inside the network `net`/`len` (different families never match).
pub fn in_prefix(addr: IpAddr, net: IpAddr, len: u8) -> bool {
    match (addr, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => {
            let len = u32::from(len.min(32));
            let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
            u32::from(a) & mask == u32::from(n) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(n)) => {
            let len = u32::from(len.min(128));
            let mask = if len == 0 { 0 } else { u128::MAX << (128 - len) };
            u128::from(a) & mask == u128::from(n) & mask
        }
        _ => false,
    }
}

/// One rule: allow or deny, for a protocol, a remote address (or any) and a port range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFwRule {
    pub allow: bool,
    pub proto: FwProto,
    /// The remote network and its prefix length (a host is /32 or /128); `None` is any address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<(IpAddr, u8)>,
    /// The first and last port of this device the rule is about (TCP and UDP); `None` is any port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<(u16, u16)>,
}

impl HostFwRule {
    /// A rule for the IPv4 list (`v6` false) or the IPv6 list, checked for sense.
    pub fn new(v6: bool, allow: bool, proto: FwProto, remote: Option<(IpAddr, u8)>, ports: Option<(u16, u16)>) -> Result<HostFwRule, String> {
        let rule = HostFwRule { allow, proto, remote, ports };
        rule.check(v6)?;
        Ok(rule)
    }

    /// Why the rule makes no sense in the IPv4 or IPv6 list, if it does not.
    pub fn check(&self, v6: bool) -> Result<(), String> {
        if let Some((addr, len)) = self.remote {
            match (addr, v6) {
                (IpAddr::V4(_), false) if len <= 32 => {}
                (IpAddr::V6(_), true) if len <= 128 => {}
                _ => return Err(format!("`{addr}/{len}` is not a valid {} network", if v6 { "IPv6" } else { "IPv4" })),
            }
        }
        if let Some((lo, hi)) = self.ports {
            if self.proto == FwProto::Icmp {
                return Err("ICMP has no ports".into());
            }
            if lo > hi {
                return Err("the first port is above the last one".into());
            }
        }
        Ok(())
    }

    /// Whether this rule matches an inbound packet of `kind` from `remote` to local port `port`.
    pub fn matches(&self, kind: Inbound, remote: IpAddr, port: Option<u16>) -> bool {
        let proto_ok = matches!(
            (self.proto, kind),
            (FwProto::Any, _) | (FwProto::Tcp, Inbound::Tcp) | (FwProto::Udp, Inbound::Udp) | (FwProto::Icmp, Inbound::Icmp)
        );
        let remote_ok = self.remote.is_none_or(|(net, len)| in_prefix(remote, net, len));
        let ports_ok = self.ports.is_none_or(|(lo, hi)| port.is_some_and(|p| (lo..=hi).contains(&p)));
        proto_ok && remote_ok && ports_ok
    }
}

/// One list of rules (IPv4 or IPv6) and its switch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwList {
    #[serde(default)]
    pub on: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<HostFwRule>,
}

impl FwList {
    pub fn is_default(&self) -> bool {
        !self.on && self.rules.is_empty()
    }

    /// Whether an inbound packet may come in: always when the switch is off, else the first
    /// matching rule decides and no match means no.
    pub fn permits(&self, kind: Inbound, remote: IpAddr, port: Option<u16>) -> bool {
        if !self.on {
            return true;
        }
        self.rules.iter().find(|r| r.matches(kind, remote, port)).is_some_and(|r| r.allow)
    }
}

/// An end device's firewall: one list for IPv4 and one for IPv6.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFw {
    #[serde(default, skip_serializing_if = "FwList::is_default")]
    pub v4: FwList,
    #[serde(default, skip_serializing_if = "FwList::is_default")]
    pub v6: FwList,
}

impl HostFw {
    pub fn is_default(&self) -> bool {
        self.v4.is_default() && self.v6.is_default()
    }

    /// The IPv4 or the IPv6 list.
    pub fn list(&self, v6: bool) -> &FwList {
        if v6 { &self.v6 } else { &self.v4 }
    }

    pub fn list_mut(&mut self, v6: bool) -> &mut FwList {
        if v6 { &mut self.v6 } else { &mut self.v4 }
    }

    /// Drops what a hostile file may hold: too many rules, rules that make no sense.
    pub fn sanitize(&mut self) {
        for v6 in [false, true] {
            let list = self.list_mut(v6);
            list.rules.retain(|r| r.check(v6).is_ok());
            list.rules.truncate(MAX_RULES);
        }
    }
}

impl Device {
    /// Whether the Desktop tab has the full set of computer apps (Telnet/SSH Client, Text Editor,
    /// Firewall): PCs, laptops and servers, but not tablets and smartphones.
    pub fn has_desktop_apps(&self) -> bool {
        matches!(self.kind, crate::DeviceKind::Pc | crate::DeviceKind::Laptop | crate::DeviceKind::Server) && !self.catalogue_model().touch()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn rule(allow: bool, proto: FwProto, remote: Option<(&str, u8)>, ports: Option<(u16, u16)>) -> HostFwRule {
        HostFwRule { allow, proto, remote: remote.map(|(a, l)| (ip(a), l)), ports }
    }

    #[test]
    fn prefixes_match_by_family_and_length() {
        assert!(in_prefix(ip("10.1.2.3"), ip("10.1.0.0"), 16));
        assert!(!in_prefix(ip("10.2.2.3"), ip("10.1.0.0"), 16));
        assert!(in_prefix(ip("1.2.3.4"), ip("0.0.0.0"), 0));
        assert!(in_prefix(ip("1.2.3.4"), ip("1.2.3.4"), 32));
        assert!(in_prefix(ip("2001:db8::5"), ip("2001:db8::"), 32));
        assert!(!in_prefix(ip("2001:db9::5"), ip("2001:db8::"), 32));
        assert!(in_prefix(ip("::1"), ip("::"), 0));
        assert!(!in_prefix(ip("10.0.0.1"), ip("::"), 0));
    }

    #[test]
    fn an_off_list_lets_everything_in_and_an_on_list_is_first_match_then_deny() {
        let mut l = FwList::default();
        assert!(l.permits(Inbound::Tcp, ip("10.0.0.1"), Some(80)));
        l.on = true;
        assert!(!l.permits(Inbound::Tcp, ip("10.0.0.1"), Some(80)), "no rule: dropped");
        l.rules.push(rule(false, FwProto::Tcp, Some(("10.0.0.0", 24)), Some((80, 80))));
        l.rules.push(rule(true, FwProto::Tcp, None, Some((80, 90))));
        l.rules.push(rule(true, FwProto::Icmp, None, None));
        assert!(!l.permits(Inbound::Tcp, ip("10.0.0.7"), Some(80)), "the first rule wins");
        assert!(l.permits(Inbound::Tcp, ip("10.0.1.7"), Some(85)));
        assert!(!l.permits(Inbound::Tcp, ip("10.0.1.7"), Some(91)));
        assert!(!l.permits(Inbound::Udp, ip("10.0.1.7"), Some(85)));
        assert!(l.permits(Inbound::Icmp, ip("10.0.1.7"), None));
    }

    #[test]
    fn rules_are_checked_for_sense() {
        assert!(HostFwRule::new(false, true, FwProto::Tcp, Some((ip("10.0.0.0"), 8)), Some((1, 2))).is_ok());
        assert!(HostFwRule::new(false, true, FwProto::Tcp, Some((ip("10.0.0.0"), 33)), None).is_err());
        assert!(HostFwRule::new(false, true, FwProto::Tcp, Some((ip("2001:db8::"), 32)), None).is_err());
        assert!(HostFwRule::new(true, true, FwProto::Any, Some((ip("2001:db8::"), 32)), None).is_ok());
        assert!(HostFwRule::new(true, true, FwProto::Any, Some((ip("10.0.0.0"), 8)), None).is_err());
        assert!(HostFwRule::new(false, true, FwProto::Icmp, None, Some((1, 2))).is_err());
        assert!(HostFwRule::new(false, true, FwProto::Udp, None, Some((9, 3))).is_err());
    }

    #[test]
    fn sanitize_drops_bad_rules_and_caps_the_list() {
        let mut fw = HostFw::default();
        fw.v4.rules = vec![rule(true, FwProto::Any, None, None); MAX_RULES + 5];
        fw.v4.rules.push(rule(true, FwProto::Any, Some(("2001:db8::", 32)), None));
        fw.v6.rules.push(rule(true, FwProto::Any, Some(("10.0.0.0", 8)), None));
        fw.sanitize();
        assert_eq!(fw.v4.rules.len(), MAX_RULES);
        assert!(fw.v6.rules.is_empty());
    }

    #[test]
    fn an_empty_firewall_is_not_saved_and_a_set_one_round_trips() {
        let mut fw = HostFw::default();
        assert_eq!(serde_json::to_string(&fw).unwrap(), "{}");
        fw.v6.on = true;
        fw.v6.rules.push(rule(true, FwProto::Tcp, Some(("2001:db8::", 64)), Some((22, 22))));
        let back: HostFw = serde_json::from_str(&serde_json::to_string(&fw).unwrap()).unwrap();
        assert_eq!(back, fw);
    }
}
