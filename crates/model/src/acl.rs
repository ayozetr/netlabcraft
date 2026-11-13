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
    /// `remark` lines: notes kept with the list (shown in the configuration).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remarks: Vec<String>,
}

/// Most remarks kept per list.
pub const MAX_REMARKS: usize = 64;
/// The highest sequence number a line can have.
pub const MAX_SEQ: u32 = 2_147_483_647;

/// The ICMP message types an extended entry can name.
pub const ICMP_TYPES: &[(&str, u8)] = &[("echo-reply", 0), ("unreachable", 3), ("echo", 8), ("time-exceeded", 11)];

/// The ICMP type a name stands for (a number is taken as is).
pub fn icmp_type_from_name(w: &str) -> Option<u8> {
    let lw = w.to_ascii_lowercase();
    ICMP_TYPES.iter().find(|(n, _)| *n == lw).map(|(_, t)| *t).or_else(|| w.parse().ok())
}

/// The name an ICMP type is written with (its number when it has none).
pub fn icmp_type_name(t: u8) -> String {
    ICMP_TYPES.iter().find(|(_, v)| *v == t).map_or_else(|| t.to_string(), |(n, _)| (*n).to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AclProto {
    Ip,
    Icmp,
    Tcp,
    Udp,
    /// GRE tunnels (protocol 47): what a crypto map protects for GRE over IPsec.
    Gre,
    /// IPsec ESP (50).
    Esp,
    /// IPsec AH (51), `ahp` in access lists.
    Ahp,
    /// OSPF (89).
    Ospf,
}

impl AclProto {
    pub fn name(self) -> &'static str {
        match self {
            AclProto::Ip => "ip",
            AclProto::Icmp => "icmp",
            AclProto::Tcp => "tcp",
            AclProto::Udp => "udp",
            AclProto::Gre => "gre",
            AclProto::Esp => "esp",
            AclProto::Ahp => "ahp",
            AclProto::Ospf => "ospf",
        }
    }

    pub fn from_name(s: &str) -> Option<AclProto> {
        Some(match s.to_ascii_lowercase().as_str() {
            "ip" => AclProto::Ip,
            "icmp" => AclProto::Icmp,
            "tcp" => AclProto::Tcp,
            "udp" => AclProto::Udp,
            "gre" => AclProto::Gre,
            "esp" => AclProto::Esp,
            "ahp" => AclProto::Ahp,
            "ospf" => AclProto::Ospf,
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
            AclProto::Gre => Some(47),
            AclProto::Esp => Some(50),
            AclProto::Ahp => Some(51),
            AclProto::Ospf => Some(89),
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
    /// The line's sequence number (0 in files from before sequence numbers: its position
    /// decides, see [`Acl::seq_of`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub seq: u32,
    /// `established`: only TCP segments with ACK or RST set (replies to inside connections).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub established: bool,
    /// An ICMP message type (`echo`, `echo-reply`, `unreachable`, `time-exceeded` or a number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icmp_type: Option<u8>,
    /// `log`: a match leaves a message in the device's log.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub log: bool,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl AclEntry {
    /// A line matching `proto` from `src` to `dst`, with no ports or options.
    pub fn new(permit: bool, proto: AclProto, src: AclAddr, dst: AclAddr) -> AclEntry {
        AclEntry { permit, proto, src, src_port: None, dst, dst_port: None, seq: 0, established: false, icmp_type: None, log: false }
    }

    /// Whether two lines match the same packets the same way (sequence numbers aside).
    pub fn same_rule(&self, o: &AclEntry) -> bool {
        AclEntry { seq: 0, ..self.clone() } == AclEntry { seq: 0, ..o.clone() }
    }
}

/// What a packet looks like to an ACL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketKey {
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
    /// The ICMP message type, for ICMP packets.
    pub icmp_type: Option<u8>,
    /// The TCP flags byte, for TCP segments.
    pub tcp_flags: Option<u8>,
}

impl PacketKey {
    /// A key with only protocol and addresses (no ports, type or flags).
    pub fn bare(protocol: u8, src: Ipv4Addr, dst: Ipv4Addr) -> PacketKey {
        PacketKey { protocol, src, dst, src_port: None, dst_port: None, icmp_type: None, tcp_flags: None }
    }
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
        if !(port_ok(self.src_port, p.src_port) && port_ok(self.dst_port, p.dst_port)) {
            return false;
        }
        // ACK (0x10) or RST (0x04): part of a connection already under way.
        if self.established && self.proto == AclProto::Tcp && p.tcp_flags.is_none_or(|f| f & 0x14 == 0) {
            return false;
        }
        if self.proto == AclProto::Icmp && self.icmp_type.is_some_and(|t| p.icmp_type != Some(t)) {
            return false;
        }
        true
    }

    /// The entry as configuration words after `permit`/`deny`.
    pub fn text(&self, extended: bool) -> String {
        let action = if self.permit { "permit" } else { "deny" };
        let log = if self.log { " log" } else { "" };
        if !extended {
            return format!("{action} {}{log}", self.src);
        }
        let mut s = format!("{action} {} {}", self.proto.name(), self.src);
        if let Some(p) = self.src_port {
            s.push_str(&format!(" {p}"));
        }
        s.push_str(&format!(" {}", self.dst));
        if let Some(p) = self.dst_port {
            s.push_str(&format!(" {p}"));
        }
        if let Some(t) = self.icmp_type.filter(|_| self.proto == AclProto::Icmp) {
            s.push_str(&format!(" {}", icmp_type_name(t)));
        }
        if self.established && self.proto == AclProto::Tcp {
            s.push_str(" established");
        }
        s.push_str(log);
        s
    }
}

impl Acl {
    /// An empty list.
    pub fn new(name: impl Into<String>, extended: bool) -> Acl {
        Acl { name: name.into(), extended, entries: Vec::new(), remarks: Vec::new() }
    }

    /// The sequence number of the line at `i` (its stored number, or ten per position for
    /// lines from older files).
    pub fn seq_of(&self, i: usize) -> u32 {
        match self.entries.get(i).map(|e| e.seq) {
            Some(n) if n > 0 => n,
            _ => u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX).saturating_mul(10).min(MAX_SEQ),
        }
    }

    /// Gives every line its effective sequence number, so explicit numbers can be added.
    pub fn number_lines(&mut self) {
        let seqs: Vec<u32> = (0..self.entries.len()).map(|i| self.seq_of(i)).collect();
        for (e, n) in self.entries.iter_mut().zip(seqs) {
            e.seq = n;
        }
    }

    /// The number the next unnumbered line gets: ten past the last.
    pub fn next_seq(&self) -> u32 {
        match self.entries.len() {
            0 => 10,
            n => self.seq_of(n - 1).saturating_add(10).min(MAX_SEQ),
        }
    }

    /// Inserts a line in sequence order (an identical line is kept once). Errors: the number
    /// is taken.
    pub fn insert(&mut self, mut e: AclEntry) -> Result<(), String> {
        if self.entries.iter().any(|x| x.same_rule(&e)) {
            return Ok(());
        }
        self.number_lines();
        if e.seq == 0 {
            e.seq = self.next_seq();
        }
        if self.entries.iter().any(|x| x.seq == e.seq) {
            return Err(format!("% {} already has a line {}", self.name, e.seq));
        }
        let at = self.entries.iter().position(|x| x.seq > e.seq).unwrap_or(self.entries.len());
        self.entries.insert(at, e);
        Ok(())
    }

    /// Removes the line numbered `seq`; whether there was one.
    pub fn remove_seq(&mut self, seq: u32) -> bool {
        self.number_lines();
        let before = self.entries.len();
        self.entries.retain(|e| e.seq != seq);
        self.entries.len() != before
    }

    /// Renumbers the lines `start`, `start + step`, …
    pub fn resequence(&mut self, start: u32, step: u32) {
        let mut n = start.clamp(1, MAX_SEQ);
        for e in &mut self.entries {
            e.seq = n;
            n = n.saturating_add(step.max(1)).min(MAX_SEQ);
        }
    }

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
        PacketKey { dst_port: dport, ..PacketKey::bare(proto, src.parse().unwrap(), dst.parse().unwrap()) }
    }

    #[test]
    fn standard_and_extended_matching() {
        let std_acl = Acl {
            name: "10".into(),
            extended: false,
            remarks: Vec::new(),
            entries: vec![
                AclEntry {
                    permit: false,
                    proto: AclProto::Ip,
                    src: AclAddr::host("192.168.1.5".parse().unwrap()),
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                    seq: 0,
                    established: false,
                    icmp_type: None,
                    log: false,
                },
                AclEntry {
                    permit: true,
                    proto: AclProto::Ip,
                    src: AclAddr { addr: "192.168.1.0".parse().unwrap(), wildcard: "0.0.0.255".parse().unwrap() },
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                    seq: 0,
                    established: false,
                    icmp_type: None,
                    log: false,
                },
            ],
        };
        assert_eq!(std_acl.evaluate(&key(1, "192.168.1.5", "8.8.8.8", None)), (false, Some(0)));
        assert_eq!(std_acl.evaluate(&key(1, "192.168.1.6", "8.8.8.8", None)), (true, Some(1)));
        assert_eq!(std_acl.evaluate(&key(1, "10.0.0.1", "8.8.8.8", None)), (false, None));
        let ext = Acl {
            name: "WEB".into(),
            extended: true,
            remarks: Vec::new(),
            entries: vec![
                AclEntry {
                    permit: true,
                    proto: AclProto::Udp,
                    src: AclAddr::ANY,
                    src_port: None,
                    dst: AclAddr::host("10.0.0.1".parse().unwrap()),
                    dst_port: Some(PortMatch::Eq(67)),
                    seq: 0,
                    established: false,
                    icmp_type: None,
                    log: false,
                },
                AclEntry {
                    permit: false,
                    proto: AclProto::Icmp,
                    src: AclAddr::ANY,
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                    seq: 0,
                    established: false,
                    icmp_type: None,
                    log: false,
                },
                AclEntry {
                    permit: true,
                    proto: AclProto::Ip,
                    src: AclAddr::ANY,
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                    seq: 0,
                    established: false,
                    icmp_type: None,
                    log: false,
                },
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

    #[test]
    fn established_icmp_types_and_sequence_numbers() {
        let est = AclEntry { established: true, ..AclEntry::new(true, AclProto::Tcp, AclAddr::ANY, AclAddr::ANY) };
        let mut k = key(6, "1.1.1.1", "2.2.2.2", Some(80));
        k.tcp_flags = Some(0x02);
        assert!(!est.matches(true, &k), "a SYN opens a new connection");
        k.tcp_flags = Some(0x12);
        assert!(est.matches(true, &k));
        k.tcp_flags = Some(0x04);
        assert!(est.matches(true, &k));
        assert_eq!(est.text(true), "permit tcp any any established");
        let echo = AclEntry { icmp_type: icmp_type_from_name("echo"), ..AclEntry::new(false, AclProto::Icmp, AclAddr::ANY, AclAddr::ANY) };
        let mut k = key(1, "1.1.1.1", "2.2.2.2", None);
        k.icmp_type = Some(8);
        assert!(echo.matches(true, &k));
        k.icmp_type = Some(0);
        assert!(!echo.matches(true, &k));
        assert_eq!(echo.text(true), "deny icmp any any echo");
        assert_eq!(icmp_type_name(42), "42");
        let mut a = Acl::new("X", true);
        a.entries.push(AclEntry::new(true, AclProto::Udp, AclAddr::ANY, AclAddr::ANY));
        a.entries.push(AclEntry::new(true, AclProto::Tcp, AclAddr::ANY, AclAddr::ANY));
        assert_eq!((a.seq_of(0), a.seq_of(1), a.next_seq()), (10, 20, 30));
        a.insert(AclEntry { seq: 15, ..AclEntry::new(false, AclProto::Ip, AclAddr::ANY, AclAddr::ANY) }).unwrap();
        assert_eq!(a.entries[1].proto, AclProto::Ip);
        assert!(a.insert(AclEntry { seq: 15, ..AclEntry::new(false, AclProto::Icmp, AclAddr::ANY, AclAddr::ANY) }).is_err());
        assert!(a.remove_seq(20) && !a.remove_seq(20));
        a.resequence(u32::MAX, u32::MAX);
        assert_eq!((a.entries[0].seq, a.entries[1].seq), (MAX_SEQ, MAX_SEQ));
    }
}

/// An IPv6 address match: any, one host, or a prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Addr6 {
    Any,
    Host(std::net::Ipv6Addr),
    Prefix(netlabcraft_core::Ipv6Net),
}

impl Addr6 {
    pub fn matches(&self, a: std::net::Ipv6Addr) -> bool {
        match self {
            Addr6::Any => true,
            Addr6::Host(h) => *h == a,
            Addr6::Prefix(n) => n.contains(a),
        }
    }
}

impl std::fmt::Display for Addr6 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Addr6::Any => f.write_str("any"),
            Addr6::Host(h) => write!(f, "host {h}"),
            Addr6::Prefix(n) => write!(f, "{}", n.network()),
        }
    }
}

/// One line of an IPv6 access list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acl6Entry {
    pub permit: bool,
    /// `Ip` stands for `ipv6` (any next header).
    pub proto: AclProto,
    pub src: Addr6,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_port: Option<PortMatch>,
    pub dst: Addr6,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_port: Option<PortMatch>,
}

/// A named IPv6 access list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acl6 {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<Acl6Entry>,
}

/// What an IPv6 packet looks like to an access list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet6Key {
    pub next_header: u8,
    pub src: std::net::Ipv6Addr,
    pub dst: std::net::Ipv6Addr,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
    /// Neighbour discovery, which every IPv6 list lets through before its final deny.
    pub nd: bool,
}

impl Acl6Entry {
    pub fn matches(&self, p: &Packet6Key) -> bool {
        let proto = match self.proto {
            AclProto::Icmp => Some(58),
            other => other.number(),
        };
        let port_ok = |m: Option<PortMatch>, v: Option<u16>| m.is_none_or(|m| v.is_some_and(|v| m.matches(v)));
        proto.is_none_or(|n| n == p.next_header)
            && self.src.matches(p.src)
            && self.dst.matches(p.dst)
            && port_ok(self.src_port, p.src_port)
            && port_ok(self.dst_port, p.dst_port)
    }

    /// The entry as typed after `permit` / `deny`.
    pub fn text(&self) -> String {
        let action = if self.permit { "permit" } else { "deny" };
        let proto = if self.proto == AclProto::Ip { "ipv6" } else { self.proto.name() };
        let mut s = format!("{action} {proto} {}", self.src);
        if let Some(p) = self.src_port {
            s.push_str(&format!(" {p}"));
        }
        s.push_str(&format!(" {}", self.dst));
        if let Some(p) = self.dst_port {
            s.push_str(&format!(" {p}"));
        }
        s
    }

    /// Parses `permit|deny <ipv6|icmp|tcp|udp> <src> [port] <dst> [port]`, where an address is
    /// `any`, `host X` or `X::/n`, and a port `eq|neq|lt|gt N` or `range A B`.
    pub fn parse(words: &[&str]) -> Result<Acl6Entry, String> {
        let mut it = words.iter().copied().peekable();
        let permit = match it.next().map(str::to_ascii_lowercase).as_deref() {
            Some("permit") => true,
            Some("deny") => false,
            _ => return Err("permit or deny".into()),
        };
        let proto = match it.next().map(str::to_ascii_lowercase).as_deref() {
            Some("ipv6") => AclProto::Ip,
            Some(p) => AclProto::from_name(p).filter(|x| *x != AclProto::Ip).ok_or_else(|| format!("`{p}`: ipv6, icmp, tcp or udp"))?,
            None => return Err("give a protocol: ipv6, icmp, tcp or udp".into()),
        };
        let addr = |it: &mut std::iter::Peekable<std::iter::Copied<std::slice::Iter<'_, &str>>>| -> Result<Addr6, String> {
            match it.next() {
                Some(w) if w.eq_ignore_ascii_case("any") => Ok(Addr6::Any),
                Some(w) if w.eq_ignore_ascii_case("host") => {
                    let a = it.next().ok_or("host needs an address")?;
                    a.parse().map(Addr6::Host).map_err(|_| format!("`{a}` is not an IPv6 address"))
                }
                Some(w) => w.parse::<netlabcraft_core::Ipv6Net>().map(Addr6::Prefix).map_err(|e| e.to_string()),
                None => Err("missing an address (any, host X or X::/n)".into()),
            }
        };
        let port = |it: &mut std::iter::Peekable<std::iter::Copied<std::slice::Iter<'_, &str>>>| -> Result<Option<PortMatch>, String> {
            let Some(op) = it.peek().map(|w| w.to_ascii_lowercase()) else { return Ok(None) };
            let num = |it: &mut std::iter::Peekable<std::iter::Copied<std::slice::Iter<'_, &str>>>| -> Result<u16, String> {
                let w = it.next().ok_or("missing a port")?;
                w.parse::<u16>().map_err(|_| format!("`{w}` is not a port"))
            };
            let m = match op.as_str() {
                "eq" | "neq" | "lt" | "gt" | "range" => {
                    it.next();
                    match op.as_str() {
                        "eq" => PortMatch::Eq(num(it)?),
                        "neq" => PortMatch::Neq(num(it)?),
                        "lt" => PortMatch::Lt(num(it)?),
                        "gt" => PortMatch::Gt(num(it)?),
                        _ => PortMatch::Range(num(it)?, num(it)?),
                    }
                }
                _ => return Ok(None),
            };
            Ok(Some(m))
        };
        let ports = matches!(proto, AclProto::Tcp | AclProto::Udp);
        let src = addr(&mut it)?;
        let src_port = if ports { port(&mut it)? } else { None };
        let dst = addr(&mut it)?;
        let dst_port = if ports { port(&mut it)? } else { None };
        if let Some(extra) = it.next() {
            return Err(format!("`{extra}` not understood"));
        }
        Ok(Acl6Entry { permit, proto, src, src_port, dst, dst_port })
    }
}

impl Acl6 {
    /// The first matching entry's index and whether it permits; neighbour discovery always
    /// passes (`None` index with `true`); otherwise the implicit deny (`None`, `false`).
    pub fn evaluate(&self, p: &Packet6Key) -> (bool, Option<usize>) {
        match self.entries.iter().position(|e| e.matches(p)) {
            Some(i) => (self.entries.get(i).is_some_and(|e| e.permit), Some(i)),
            None => (p.nd, None),
        }
    }
}
