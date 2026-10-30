//! Quality of service, configured the modular way: class maps say which packets belong to a
//! class, policy maps say what happens to each class (mark, police, queue), and a service
//! policy applies a policy map to an interface's input or output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Most class maps, and most policy maps, per device.
pub const MAX_MAPS: usize = 64;
/// Most match lines per class map, and most classes per policy map.
pub const MAX_LINES: usize = 32;
/// Most values on one `match dscp` / `match ip precedence` line.
pub const MAX_VALUES: usize = 8;
/// Longest class or policy name.
pub const MAX_NAME: usize = 40;
/// Most interfaces with service policies.
pub const MAX_APPLIED: usize = 256;
/// The class every packet no other class took falls into.
pub const CLASS_DEFAULT: &str = "class-default";

/// The protocols `match protocol` recognises: by IP protocol number, and for applications by
/// their well-known TCP/UDP port (either direction). No deeper inspection is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppProto {
    Icmp,
    Tcp,
    Udp,
    Http,
    Https,
    Dns,
    Telnet,
    Ssh,
    Ftp,
    Smtp,
    Pop3,
    Snmp,
    Tftp,
    Ntp,
}

impl AppProto {
    pub const ALL: [AppProto; 14] = [
        AppProto::Icmp,
        AppProto::Tcp,
        AppProto::Udp,
        AppProto::Http,
        AppProto::Https,
        AppProto::Dns,
        AppProto::Telnet,
        AppProto::Ssh,
        AppProto::Ftp,
        AppProto::Smtp,
        AppProto::Pop3,
        AppProto::Snmp,
        AppProto::Tftp,
        AppProto::Ntp,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AppProto::Icmp => "icmp",
            AppProto::Tcp => "tcp",
            AppProto::Udp => "udp",
            AppProto::Http => "http",
            AppProto::Https => "https",
            AppProto::Dns => "dns",
            AppProto::Telnet => "telnet",
            AppProto::Ssh => "ssh",
            AppProto::Ftp => "ftp",
            AppProto::Smtp => "smtp",
            AppProto::Pop3 => "pop3",
            AppProto::Snmp => "snmp",
            AppProto::Tftp => "tftp",
            AppProto::Ntp => "ntp",
        }
    }

    pub fn parse(text: &str) -> Option<AppProto> {
        let t = text.trim().to_ascii_lowercase();
        AppProto::ALL.into_iter().find(|p| p.name() == t)
    }

    /// The IP protocol (1 ICMP, 6 TCP, 17 UDP) and well-known ports this protocol is
    /// recognised by; no ports means any packet of that IP protocol.
    pub fn signature(self) -> (&'static [u8], &'static [u16]) {
        const TCP: &[u8] = &[6];
        const UDP: &[u8] = &[17];
        const BOTH: &[u8] = &[6, 17];
        match self {
            AppProto::Icmp => (&[1], &[]),
            AppProto::Tcp => (TCP, &[]),
            AppProto::Udp => (UDP, &[]),
            AppProto::Http => (TCP, &[80]),
            AppProto::Https => (TCP, &[443]),
            AppProto::Dns => (BOTH, &[53]),
            AppProto::Telnet => (TCP, &[23]),
            AppProto::Ssh => (TCP, &[22]),
            AppProto::Ftp => (TCP, &[20, 21]),
            AppProto::Smtp => (TCP, &[25]),
            AppProto::Pop3 => (TCP, &[110]),
            AppProto::Snmp => (UDP, &[161, 162]),
            AppProto::Tftp => (UDP, &[69]),
            AppProto::Ntp => (UDP, &[123]),
        }
    }
}

/// One `match` line of a class map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Match {
    /// `match dscp V…`: any of the values.
    Dscp(Vec<u8>),
    /// `match ip precedence P…`: any of the values (0-7).
    Precedence(Vec<u8>),
    /// `match access-group name X` / `match access-group N`: what the list permits.
    AccessGroup(String),
    /// `match protocol P`.
    Protocol(AppProto),
    /// `match any`.
    Any,
}

/// `class-map [match-all|match-any] NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassMap {
    pub name: String,
    /// `match-any`: one line matching is enough (otherwise every line must match).
    #[serde(default)]
    pub any: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<Match>,
}

/// What `police` does with conforming or exceeding packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PoliceAction {
    Transmit,
    Drop,
    /// Re-mark the DSCP, then send.
    SetDscpTransmit(u8),
}

/// `police RATE [BURST] conform-action A exceed-action B`: a single-rate token bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Police {
    /// Committed rate, bits per second.
    pub rate_bps: u64,
    /// Bucket depth in bytes (`None`: the default, from the rate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst: Option<u32>,
    pub conform: PoliceAction,
    pub exceed: PoliceAction,
}

/// Smallest default bucket: one full-size Ethernet payload.
pub const MIN_BURST: u64 = 1_500;

impl Police {
    /// The bucket depth in bytes: as given, else a quarter second of traffic at the rate
    /// (at least [`MIN_BURST`]).
    pub fn depth(&self) -> u64 {
        self.burst.map_or_else(|| (self.rate_bps / 32).max(MIN_BURST), u64::from)
    }
}

/// `bandwidth KBPS` or `bandwidth percent N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rate {
    Kbps(u32),
    Percent(u8),
}

/// One `class` of a policy map and what it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyClass {
    pub class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_dscp: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_precedence: Option<u8>,
    /// `priority`: strict-priority (low-latency) queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Rate>,
    /// `bandwidth`: a guaranteed share under congestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<Rate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub police: Option<Police>,
    /// `shape average BPS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<u64>,
}

impl PolicyClass {
    pub fn new(class: &str) -> PolicyClass {
        PolicyClass { class: class.to_string(), set_dscp: None, set_precedence: None, priority: None, bandwidth: None, police: None, shape: None }
    }
}

/// `policy-map NAME`: its classes in the order they are checked (class-default last).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyMap {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<PolicyClass>,
}

impl PolicyMap {
    /// Keeps class-default after every other class.
    pub fn order(&mut self) {
        self.classes.sort_by_key(|c| c.class == CLASS_DEFAULT);
    }
}

/// The policy maps one interface applies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// A device's QoS configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Qos {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_maps: Vec<ClassMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_maps: Vec<PolicyMap>,
    /// Interface name → the policies on it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<String, ServicePolicy>,
}

impl Qos {
    pub fn is_empty(&self) -> bool {
        *self == Qos::default()
    }

    pub fn class_map(&self, name: &str) -> Option<&ClassMap> {
        self.class_maps.iter().find(|c| c.name == name)
    }

    pub fn policy_map(&self, name: &str) -> Option<&PolicyMap> {
        self.policy_maps.iter().find(|p| p.name == name)
    }

    /// The policy applied to an interface in one direction.
    pub fn policy_on(&self, iface: &str, input: bool) -> Option<&PolicyMap> {
        let sp = self.applied.get(iface)?;
        let name = if input { sp.input.as_ref() } else { sp.output.as_ref() }?;
        self.policy_map(name)
    }

    /// Caps every list (hostile files).
    pub fn sanitize(&mut self) {
        self.class_maps.truncate(MAX_MAPS);
        self.policy_maps.truncate(MAX_MAPS);
        for c in &mut self.class_maps {
            c.matches.truncate(MAX_LINES);
            for m in &mut c.matches {
                if let Match::Dscp(v) | Match::Precedence(v) = m {
                    v.truncate(MAX_VALUES);
                }
            }
        }
        for p in &mut self.policy_maps {
            p.classes.truncate(MAX_LINES);
            p.order();
        }
        self.applied.retain(|_, sp| sp.input.is_some() || sp.output.is_some());
        while self.applied.len() > MAX_APPLIED {
            self.applied.pop_last();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_default_stays_last() {
        let mut p = PolicyMap { name: "P".into(), classes: vec![PolicyClass::new(CLASS_DEFAULT), PolicyClass::new("A"), PolicyClass::new("B")] };
        p.order();
        let names: Vec<&str> = p.classes.iter().map(|c| c.class.as_str()).collect();
        assert_eq!(names, ["A", "B", CLASS_DEFAULT]);
    }

    #[test]
    fn protocols_and_buckets() {
        for p in AppProto::ALL {
            assert_eq!(AppProto::parse(p.name()), Some(p));
        }
        assert_eq!(AppProto::parse("gopher"), None);
        let pol = Police { rate_bps: 8_000, burst: None, conform: PoliceAction::Transmit, exceed: PoliceAction::Drop };
        assert_eq!(pol.depth(), MIN_BURST);
        assert_eq!(Police { rate_bps: 1_000_000, ..pol }.depth(), 31_250);
        assert_eq!(Police { burst: Some(2_000), ..pol }.depth(), 2_000);
    }
}
