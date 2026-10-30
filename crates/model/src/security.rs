//! Router security settings: the zone-based policy firewall (security zones, inspect class
//! maps and policy maps, zone pairs), classic inspection rules (`ip inspect`), and per-interface
//! IP MTU and TCP MSS clamping.
//!
//! Interfaces are referred to by their full names (`GigabitEthernet0/1`, `GigabitEthernet0/0.10`,
//! `Tunnel0`), as the simulator's layer-3 interfaces name them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Most zones, inspect class maps, policy maps, zone pairs and inspection rules per router.
pub const MAX_ITEMS: usize = 64;
/// Most match lines in one class map, classes in one policy, protocols in one rule.
pub const MAX_LINES: usize = 64;
/// Most interfaces with a per-interface setting (zone membership, MTU, MSS, inspection).
pub const MAX_IFACES: usize = 1_024;
/// Longest name accepted for zones, maps and rules.
pub const MAX_NAME: usize = 64;
/// The router itself, as a zone.
pub const SELF_ZONE: &str = "self";
/// The class every inspect policy ends with (drop, unless configured otherwise).
pub const CLASS_DEFAULT: &str = "class-default";
/// `ip mtu` range (RFC 791: every host must take 68 bytes).
pub const MIN_MTU: u16 = 68;
pub const MAX_MTU: u16 = 9_216;
/// `ip tcp adjust-mss` range.
pub const MIN_MSS: u16 = 500;
pub const MAX_MSS: u16 = 1_460;

/// `zone security NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// One `match` line of an inspect class map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InspectMatch {
    /// `match protocol NAME` (one of [`PROTOCOLS`]).
    Protocol(String),
    /// `match access-group N` / `match access-group name NAME`.
    AccessGroup(String),
    /// `match class-map NAME`: another inspect class.
    ClassMap(String),
}

/// `class-map type inspect [match-any|match-all] NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectClass {
    pub name: String,
    /// `match-all` (every line must match); `match-any` otherwise. Routers default to
    /// match-all.
    #[serde(default)]
    pub match_all: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<InspectMatch>,
}

/// What an inspect policy does with a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InspectAction {
    /// Let it through and remember the session, so the replies come back.
    Inspect,
    /// Let it through in this direction only.
    Pass,
    /// Throw it away.
    Drop,
}

impl InspectAction {
    pub fn word(self) -> &'static str {
        match self {
            InspectAction::Inspect => "inspect",
            InspectAction::Pass => "pass",
            InspectAction::Drop => "drop",
        }
    }
}

/// `class type inspect NAME` inside an inspect policy map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectPolicyClass {
    pub class: String,
    /// `None` until an action is typed: such a class drops (the safe default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<InspectAction>,
    /// `drop log` / `pass log`: every packet the action takes is logged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub log: bool,
}

impl InspectPolicyClass {
    /// The action taken (a class without one drops).
    pub fn effective(&self) -> InspectAction {
        self.action.unwrap_or(InspectAction::Drop)
    }
}

/// `policy-map type inspect NAME`. `class-default` is kept last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectPolicy {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<InspectPolicyClass>,
}

impl InspectPolicy {
    /// Keeps `class-default` at the end, wherever it was typed.
    pub fn order(&mut self) {
        self.classes.sort_by_key(|c| c.class.eq_ignore_ascii_case(CLASS_DEFAULT));
    }
}

/// `zone-pair security NAME source Z1 destination Z2`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZonePair {
    pub name: String,
    pub source: String,
    pub destination: String,
    /// `service-policy type inspect NAME`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
}

/// `ip inspect name NAME PROTOCOL` lines, gathered by name (classic inspection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectRule {
    pub name: String,
    pub protocols: Vec<String>,
}

/// Where an inspection rule is applied: `ip inspect NAME in|out` on an interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbound: Option<String>,
}

/// A router's security settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RouterSecurity {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
    /// `zone-member security NAME`: interface name → zone name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub members: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<InspectClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<InspectPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<ZonePair>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inspect_rules: Vec<InspectRule>,
    /// `ip inspect NAME in|out`: interface name → rules.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inspect_applied: BTreeMap<String, InspectBinding>,
    /// `ip mtu N`: interface name → largest IPv4 packet sent out of it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ip_mtu: BTreeMap<String, u16>,
    /// `ip tcp adjust-mss N`: interface name → largest MSS let through in SYNs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub adjust_mss: BTreeMap<String, u16>,
}

/// The application protocols `match protocol` and `ip inspect name` know, with their
/// transport protocol (6 TCP, 17 UDP, 1 ICMP) and well-known server port (0: any).
pub const PROTOCOLS: [(&str, u8, u16); 17] = [
    ("tcp", 6, 0),
    ("udp", 17, 0),
    ("icmp", 1, 0),
    ("http", 6, 80),
    ("https", 6, 443),
    ("ftp", 6, 21),
    ("ssh", 6, 22),
    ("telnet", 6, 23),
    ("smtp", 6, 25),
    ("pop3", 6, 110),
    ("imap", 6, 143),
    ("dns", 17, 53),
    ("tftp", 17, 69),
    ("ntp", 17, 123),
    ("snmp", 17, 161),
    ("syslog", 17, 514),
    ("bootps", 17, 67),
];

/// The transport protocol and server port of a protocol name, if known.
pub fn protocol(name: &str) -> Option<(u8, u16)> {
    PROTOCOLS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)).map(|(_, p, port)| (*p, *port))
}

/// Whether a packet (transport protocol, server-side port) is the protocol named. DNS also
/// runs over TCP.
pub fn protocol_matches(name: &str, proto: u8, dst_port: Option<u16>) -> bool {
    let Some((p, port)) = protocol(name) else { return false };
    if name.eq_ignore_ascii_case("dns") && proto == 6 && dst_port == Some(53) {
        return true;
    }
    p == proto && (port == 0 || dst_port == Some(port))
}

impl RouterSecurity {
    pub fn is_default(&self) -> bool {
        *self == RouterSecurity::default()
    }

    pub fn zone(&self, name: &str) -> Option<&Zone> {
        self.zones.iter().find(|z| z.name == name)
    }

    pub fn class(&self, name: &str) -> Option<&InspectClass> {
        self.classes.iter().find(|c| c.name == name)
    }

    pub fn policy(&self, name: &str) -> Option<&InspectPolicy> {
        self.policies.iter().find(|p| p.name == name)
    }

    pub fn pair(&self, name: &str) -> Option<&ZonePair> {
        self.pairs.iter().find(|p| p.name == name)
    }

    pub fn rule(&self, name: &str) -> Option<&InspectRule> {
        self.inspect_rules.iter().find(|r| r.name == name)
    }

    /// The zone an interface belongs to.
    pub fn zone_of(&self, iface: &str) -> Option<&str> {
        self.members.get(iface).map(String::as_str)
    }

    /// The zone pair from one zone to another, if configured.
    pub fn pair_between(&self, src: &str, dst: &str) -> Option<&ZonePair> {
        self.pairs.iter().find(|p| p.source == src && p.destination == dst)
    }

    /// Whether the zone firewall is in use (any interface is a zone member).
    pub fn zones_active(&self) -> bool {
        !self.members.is_empty()
    }

    /// Caps every list (hostile files) and keeps `class-default` last.
    pub fn sanitize(&mut self) {
        self.zones.truncate(MAX_ITEMS);
        self.classes.truncate(MAX_ITEMS);
        self.policies.truncate(MAX_ITEMS);
        self.pairs.truncate(MAX_ITEMS);
        self.inspect_rules.truncate(MAX_ITEMS);
        for c in &mut self.classes {
            c.matches.truncate(MAX_LINES);
        }
        for p in &mut self.policies {
            p.classes.truncate(MAX_LINES);
            p.order();
        }
        for r in &mut self.inspect_rules {
            r.protocols.truncate(MAX_LINES);
        }
        while self.members.len() > MAX_IFACES {
            self.members.pop_last();
        }
        while self.inspect_applied.len() > MAX_IFACES {
            self.inspect_applied.pop_last();
        }
        self.inspect_applied.retain(|_, b| b.inbound.is_some() || b.outbound.is_some());
        self.ip_mtu.retain(|_, m| (MIN_MTU..=MAX_MTU).contains(m));
        self.adjust_mss.retain(|_, m| (MIN_MSS..=MAX_MSS).contains(m));
        while self.ip_mtu.len() > MAX_IFACES {
            self.ip_mtu.pop_last();
        }
        while self.adjust_mss.len() > MAX_IFACES {
            self.adjust_mss.pop_last();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_match_by_port() {
        assert!(protocol_matches("http", 6, Some(80)));
        assert!(!protocol_matches("http", 6, Some(8080)));
        assert!(protocol_matches("tcp", 6, Some(8080)));
        assert!(protocol_matches("dns", 17, Some(53)) && protocol_matches("dns", 6, Some(53)));
        assert!(protocol_matches("icmp", 1, None));
        assert!(!protocol_matches("nonsense", 6, Some(80)));
    }

    #[test]
    fn sanitize_caps_and_orders() {
        let mut s = RouterSecurity::default();
        s.policies.push(InspectPolicy {
            name: "P".into(),
            classes: vec![
                InspectPolicyClass { class: CLASS_DEFAULT.into(), action: Some(InspectAction::Drop), log: false },
                InspectPolicyClass { class: "C".into(), action: None, log: false },
            ],
        });
        s.ip_mtu.insert("g0".into(), 10);
        s.ip_mtu.insert("g1".into(), 1400);
        s.sanitize();
        assert_eq!(s.policies[0].classes[1].class, CLASS_DEFAULT);
        assert_eq!(s.ip_mtu.len(), 1);
        assert_eq!(s.policies[0].classes[0].effective(), InspectAction::Drop);
    }
}
