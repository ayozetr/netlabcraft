//! Access control lists: the configuration and the matching rules.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// Most ACLs per device and entries per ACL.
pub const MAX_ACLS: usize = 256;
pub const MAX_ENTRIES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acl {
    /// A number (`10`, `101`) or a name.
    pub name: String,
    pub extended: bool,
    #[serde(default)]
    pub entries: Vec<AclEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AclProto {
    Ip,
    Icmp,
    Tcp,
    Udp,
}

impl AclProto {
    pub fn name(self) -> &'static str {
        match self {
            AclProto::Ip => "ip",
            AclProto::Icmp => "icmp",
            AclProto::Tcp => "tcp",
            AclProto::Udp => "udp",
        }
    }

    pub fn from_name(s: &str) -> Option<AclProto> {
        Some(match s.to_ascii_lowercase().as_str() {
            "ip" => AclProto::Ip,
            "icmp" => AclProto::Icmp,
            "tcp" => AclProto::Tcp,
            "udp" => AclProto::Udp,
            _ => return None,
        })
    }

    /// The IPv4 protocol number this entry matches (`None`: any).
    pub fn number(self) -> Option<u8> {
        match self {
            AclProto::Ip => None,
            AclProto::Icmp => Some(1),
            AclProto::Tcp => Some(6),
            AclProto::Udp => Some(17),
        }
    }
}

/// An address and wildcard (`any` is 0.0.0.0 255.255.255.255, `host a` is a 0.0.0.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclAddr {
    pub addr: Ipv4Addr,
    pub wildcard: Ipv4Addr,
}

impl AclAddr {
    pub const ANY: AclAddr = AclAddr { addr: Ipv4Addr::UNSPECIFIED, wildcard: Ipv4Addr::BROADCAST };

    pub fn host(a: Ipv4Addr) -> AclAddr {
        AclAddr { addr: a, wildcard: Ipv4Addr::UNSPECIFIED }
    }

    pub fn matches(&self, ip: Ipv4Addr) -> bool {
        let keep = !u32::from(self.wildcard);
        u32::from(ip) & keep == u32::from(self.addr) & keep
    }
}

impl std::fmt::Display for AclAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == AclAddr::ANY {
            f.write_str("any")
        } else if self.wildcard.is_unspecified() {
            write!(f, "host {}", self.addr)
        } else {
            write!(f, "{} {}", self.addr, self.wildcard)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PortMatch {
    Eq(u16),
    Neq(u16),
    Lt(u16),
    Gt(u16),
    Range(u16, u16),
}

impl PortMatch {
    pub fn matches(self, p: u16) -> bool {
        match self {
            PortMatch::Eq(a) => p == a,
            PortMatch::Neq(a) => p != a,
            PortMatch::Lt(a) => p < a,
            PortMatch::Gt(a) => p > a,
            PortMatch::Range(a, b) => (a..=b).contains(&p),
        }
    }
}

impl std::fmt::Display for PortMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortMatch::Eq(a) => write!(f, "eq {a}"),
            PortMatch::Neq(a) => write!(f, "neq {a}"),
            PortMatch::Lt(a) => write!(f, "lt {a}"),
            PortMatch::Gt(a) => write!(f, "gt {a}"),
            PortMatch::Range(a, b) => write!(f, "range {a} {b}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclEntry {
    pub permit: bool,
    pub proto: AclProto,
    pub src: AclAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_port: Option<PortMatch>,
    pub dst: AclAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_port: Option<PortMatch>,
}

/// What a packet looks like to an ACL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketKey {
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
}

impl AclEntry {
    pub fn matches(&self, extended: bool, p: &PacketKey) -> bool {
        if !self.src.matches(p.src) {
            return false;
        }
        if !extended {
            return true;
        }
        if self.proto.number().is_some_and(|n| n != p.protocol) || !self.dst.matches(p.dst) {
            return false;
        }
        let port_ok = |m: Option<PortMatch>, v: Option<u16>| m.is_none_or(|m| v.is_some_and(|v| m.matches(v)));
        port_ok(self.src_port, p.src_port) && port_ok(self.dst_port, p.dst_port)
    }

    /// The entry as configuration words after `permit`/`deny`.
    pub fn text(&self, extended: bool) -> String {
        let action = if self.permit { "permit" } else { "deny" };
        if !extended {
            return format!("{action} {}", self.src);
        }
        let mut s = format!("{action} {} {}", self.proto.name(), self.src);
        if let Some(p) = self.src_port {
            s.push_str(&format!(" {p}"));
        }
        s.push_str(&format!(" {}", self.dst));
        if let Some(p) = self.dst_port {
            s.push_str(&format!(" {p}"));
        }
        s
    }
}

impl Acl {
    /// The first matching entry's index and whether it permits; `None`: the implicit deny.
    pub fn evaluate(&self, p: &PacketKey) -> (bool, Option<usize>) {
        match self.entries.iter().position(|e| e.matches(self.extended, p)) {
            Some(i) => (self.entries.get(i).is_some_and(|e| e.permit), Some(i)),
            None => (false, None),
        }
    }
}

/// Whether a numbered ACL is standard (1-99, 1300-1999) or extended (100-199, 2000-2699).
pub fn numbered_kind(n: &str) -> Option<bool> {
    let v: u32 = n.parse().ok()?;
    match v {
        1..=99 | 1300..=1999 => Some(false),
        100..=199 | 2000..=2699 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(proto: u8, src: &str, dst: &str, dport: Option<u16>) -> PacketKey {
        PacketKey { protocol: proto, src: src.parse().unwrap(), dst: dst.parse().unwrap(), src_port: None, dst_port: dport }
    }

    #[test]
    fn standard_and_extended_matching() {
        let std_acl = Acl {
            name: "10".into(),
            extended: false,
            entries: vec![
                AclEntry {
                    permit: false,
                    proto: AclProto::Ip,
                    src: AclAddr::host("192.168.1.5".parse().unwrap()),
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                },
                AclEntry {
                    permit: true,
                    proto: AclProto::Ip,
                    src: AclAddr { addr: "192.168.1.0".parse().unwrap(), wildcard: "0.0.0.255".parse().unwrap() },
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                },
            ],
        };
        assert_eq!(std_acl.evaluate(&key(1, "192.168.1.5", "8.8.8.8", None)), (false, Some(0)));
        assert_eq!(std_acl.evaluate(&key(1, "192.168.1.6", "8.8.8.8", None)), (true, Some(1)));
        assert_eq!(std_acl.evaluate(&key(1, "10.0.0.1", "8.8.8.8", None)), (false, None));
        let ext = Acl {
            name: "WEB".into(),
            extended: true,
            entries: vec![
                AclEntry {
                    permit: true,
                    proto: AclProto::Udp,
                    src: AclAddr::ANY,
                    src_port: None,
                    dst: AclAddr::host("10.0.0.1".parse().unwrap()),
                    dst_port: Some(PortMatch::Eq(67)),
                },
                AclEntry { permit: false, proto: AclProto::Icmp, src: AclAddr::ANY, src_port: None, dst: AclAddr::ANY, dst_port: None },
                AclEntry { permit: true, proto: AclProto::Ip, src: AclAddr::ANY, src_port: None, dst: AclAddr::ANY, dst_port: None },
            ],
        };
        assert!(ext.evaluate(&key(17, "1.1.1.1", "10.0.0.1", Some(67))).0);
        assert_eq!(ext.evaluate(&key(17, "1.1.1.1", "10.0.0.1", Some(68))), (true, Some(2)));
        assert_eq!(ext.evaluate(&key(1, "1.1.1.1", "10.0.0.1", None)), (false, Some(1)));
        assert_eq!(ext.entries[0].text(true), "permit udp any host 10.0.0.1 eq 67");
        assert_eq!(std_acl.entries[1].text(false), "permit 192.168.1.0 0.0.0.255");
        assert_eq!(numbered_kind("10"), Some(false));
        assert_eq!(numbered_kind("101"), Some(true));
        assert_eq!(numbered_kind("500"), None);
    }
}
