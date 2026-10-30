//! The stateful firewall appliance (model `Firewall-5`): named interfaces with security levels,
//! access rules bound to interfaces, network objects with their own NAT, and a small DHCP
//! server. The simulator keeps the connection and translation tables.
//!
//! Policy, in short: a connection may start from a higher security level towards a lower one;
//! anything else needs an access rule on the interface it arrives on. Replies to a connection
//! the firewall let through always pass.

use std::net::Ipv4Addr;

use netlabcraft_core::Ipv4Net;
use serde::{Deserialize, Serialize};

use crate::Device;
use crate::acl::{AclProto, PortMatch};

/// The catalogue model id.
pub const FIREWALL_MODEL: &str = "Firewall-5";
/// Most network objects, access lists, entries per list and DHCP ranges.
pub const MAX_OBJECTS: usize = 256;
pub const MAX_LISTS: usize = 64;
pub const MAX_ENTRIES: usize = 256;
pub const MAX_DHCPD: usize = 8;
/// Most addresses one `dhcpd address` range may hold.
pub const MAX_DHCPD_ADDRESSES: u32 = 256;
/// Longest interface or object name.
pub const MAX_NAME: usize = 48;

/// Whether a device is the firewall appliance.
pub fn is_firewall(d: &Device) -> bool {
    d.kind == crate::DeviceKind::Router && d.model.eq_ignore_ascii_case(FIREWALL_MODEL)
}

/// A name the firewall accepts for an interface, object or list: letters, digits, `-`, `_`
/// and `.`, starting with a letter.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A port's firewall settings (`nameif`, `security-level`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwIface {
    /// The port's full name (`GigabitEthernet1/1`).
    pub port: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nameif: Option<String>,
    /// 0 (least trusted) to 100 (most trusted).
    #[serde(default)]
    pub level: u8,
}

/// What a network object stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObjAddr {
    Host(Ipv4Addr),
    /// Host bits clear.
    Subnet(Ipv4Net),
}

impl ObjAddr {
    pub fn contains(self, a: Ipv4Addr) -> bool {
        match self {
            ObjAddr::Host(h) => h == a,
            ObjAddr::Subnet(n) => n.contains(a),
        }
    }

    /// The configuration line inside `object network`.
    pub fn text(self) -> String {
        match self {
            ObjAddr::Host(h) => format!("host {h}"),
            ObjAddr::Subnet(n) => format!("subnet {} {}", n.addr, n.mask()),
        }
    }
}

/// Where an object's addresses are translated to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NatKind {
    /// Port translation to the mapped interface's own address.
    DynamicInterface,
    /// Port translation to this one address.
    DynamicAddress(Ipv4Addr),
    /// One to one: a host to this address, a subnet to the subnet starting here.
    Static(Ipv4Addr),
}

/// `nat (REAL,MAPPED) …` inside an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjNat {
    /// The interface the real addresses are behind (`any`: every interface).
    pub real_if: String,
    /// The interface the translated addresses are used on.
    pub mapped_if: String,
    pub kind: NatKind,
}

impl ObjNat {
    pub fn text(&self) -> String {
        let how = match self.kind {
            NatKind::DynamicInterface => "dynamic interface".to_string(),
            NatKind::DynamicAddress(a) => format!("dynamic {a}"),
            NatKind::Static(a) => format!("static {a}"),
        };
        format!("nat ({},{}) {how}", self.real_if, self.mapped_if)
    }
}

/// `object network NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetObject {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<ObjAddr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nat: Option<ObjNat>,
}

impl NetObject {
    /// For a static rule: the mapped address of a real one (same offset into the subnet).
    pub fn static_map(&self, real: Ipv4Addr) -> Option<Ipv4Addr> {
        let (addr, nat) = (self.addr?, self.nat.as_ref()?);
        let NatKind::Static(m) = nat.kind else { return None };
        match addr {
            ObjAddr::Host(h) => (h == real).then_some(m),
            ObjAddr::Subnet(n) if n.contains(real) => {
                let off = u32::from(real) & !u32::from(n.mask());
                Some(Ipv4Addr::from((u32::from(m) & u32::from(n.mask())) | off))
            }
            ObjAddr::Subnet(_) => None,
        }
    }

    /// For a static rule: the real address behind a mapped one.
    pub fn static_unmap(&self, mapped: Ipv4Addr) -> Option<Ipv4Addr> {
        let (addr, nat) = (self.addr?, self.nat.as_ref()?);
        let NatKind::Static(m) = nat.kind else { return None };
        match addr {
            ObjAddr::Host(h) => (m == mapped).then_some(h),
            ObjAddr::Subnet(n) => {
                let mapped_net = Ipv4Net::new(m, n.prefix);
                mapped_net.contains(mapped).then(|| Ipv4Addr::from(u32::from(n.addr) | (u32::from(mapped) & !u32::from(n.mask()))))
            }
        }
    }
}

/// An address in an access rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FwAddr {
    Any,
    Host(Ipv4Addr),
    /// Host bits clear.
    Net(Ipv4Net),
    /// A network object, looked up when a packet is checked.
    Object(String),
    /// A network object group, looked up when a packet is checked.
    Group(String),
}

impl FwAddr {
    pub fn matches(&self, fw: &Firewall, a: Ipv4Addr) -> bool {
        match self {
            FwAddr::Any => true,
            FwAddr::Host(h) => *h == a,
            FwAddr::Net(n) => n.contains(a),
            FwAddr::Object(name) => fw.object(name).and_then(|o| o.addr).is_some_and(|o| o.contains(a)),
            FwAddr::Group(name) => fw.net_group_has(name, a, 0),
        }
    }

    /// The object or group it names, if any.
    pub fn reference(&self) -> Option<&str> {
        match self {
            FwAddr::Object(n) | FwAddr::Group(n) => Some(n),
            _ => None,
        }
    }
}

impl std::fmt::Display for FwAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FwAddr::Any => f.write_str("any"),
            FwAddr::Host(h) => write!(f, "host {h}"),
            FwAddr::Net(n) => write!(f, "{} {}", n.addr, n.mask()),
            FwAddr::Object(o) => write!(f, "object {o}"),
            FwAddr::Group(g) => write!(f, "object-group {g}"),
        }
    }
}

/// One line of a firewall access list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwAce {
    pub permit: bool,
    pub proto: AclProto,
    pub src: FwAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_port: Option<PortMatch>,
    pub dst: FwAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_port: Option<PortMatch>,
    /// ICMP rules: only this message type (`echo-reply`, `time-exceeded`…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icmp_type: Option<u8>,
    /// A service object group in place of the protocol (`permit object-group SVC …`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svc_group: Option<String>,
    /// Service object groups in place of the source or destination port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_port_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_port_group: Option<String>,
}

/// What a packet looks like to a firewall rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowKey {
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
    /// ICMP: the message type.
    pub icmp_type: Option<u8>,
}

/// ICMP message types by their usual names.
pub const ICMP_TYPES: &[(&str, u8)] = &[
    ("echo-reply", 0),
    ("unreachable", 3),
    ("source-quench", 4),
    ("redirect", 5),
    ("echo", 8),
    ("time-exceeded", 11),
    ("parameter-problem", 12),
    ("timestamp-request", 13),
    ("timestamp-reply", 14),
];

/// An ICMP type by name or number.
pub fn icmp_type_number(w: &str) -> Option<u8> {
    w.parse().ok().or_else(|| ICMP_TYPES.iter().find(|(n, _)| n.eq_ignore_ascii_case(w)).map(|(_, t)| *t))
}

/// An ICMP type's name, or its number.
pub fn icmp_type_name(t: u8) -> String {
    ICMP_TYPES.iter().find(|(_, n)| *n == t).map_or_else(|| t.to_string(), |(name, _)| (*name).to_string())
}

impl FwAce {
    pub fn matches(&self, fw: &Firewall, p: &FlowKey) -> bool {
        let port_ok = |m: Option<PortMatch>, v: Option<u16>| m.is_none_or(|m| v.is_some_and(|v| m.matches(v)));
        let group_ok = |g: &Option<String>, v: Option<u16>| g.as_deref().is_none_or(|g| v.is_some_and(|v| fw.svc_group_port(g, p.protocol, v, 0)));
        let proto_ok = match &self.svc_group {
            Some(g) => fw.svc_group_service(g, p, 0),
            None => self.proto.number().is_none_or(|n| n == p.protocol),
        };
        proto_ok
            && self.src.matches(fw, p.src)
            && self.dst.matches(fw, p.dst)
            && port_ok(self.src_port, p.src_port)
            && port_ok(self.dst_port, p.dst_port)
            && group_ok(&self.src_port_group, p.src_port)
            && group_ok(&self.dst_port_group, p.dst_port)
            && self.icmp_type.is_none_or(|t| p.icmp_type == Some(t))
    }

    /// The objects and groups the rule names.
    pub fn references(&self) -> Vec<&str> {
        let mut v: Vec<&str> = [&self.src, &self.dst].iter().filter_map(|a| a.reference()).collect();
        v.extend([&self.svc_group, &self.src_port_group, &self.dst_port_group].iter().filter_map(|g| g.as_deref()));
        v
    }

    /// The words after `extended`.
    pub fn text(&self) -> String {
        let proto = match &self.svc_group {
            Some(g) => format!("object-group {g}"),
            None => self.proto.name().to_string(),
        };
        let mut s = format!("{} {proto} {}", if self.permit { "permit" } else { "deny" }, self.src);
        if let Some(p) = self.src_port {
            s.push_str(&format!(" {p}"));
        }
        if let Some(g) = &self.src_port_group {
            s.push_str(&format!(" object-group {g}"));
        }
        s.push_str(&format!(" {}", self.dst));
        if let Some(p) = self.dst_port {
            s.push_str(&format!(" {p}"));
        }
        if let Some(g) = &self.dst_port_group {
            s.push_str(&format!(" object-group {g}"));
        }
        if let Some(t) = self.icmp_type {
            s.push_str(&format!(" {}", icmp_type_name(t)));
        }
        s
    }

    /// Parses `permit|deny PROTO SRC [PORT] DST [PORT] [ICMP-TYPE]`, where the protocol is
    /// `ip|icmp|tcp|udp` or `object-group SERVICE-GROUP`, an address is `any`, `any4`,
    /// `host A.B.C.D`, `A.B.C.D MASK`, `object NAME` or `object-group NAME`, and a port
    /// `eq|neq|lt|gt P`, `range P Q` (a number or a usual name: `www`, `https`, `domain`…) or
    /// `object-group SERVICE-GROUP`. Objects and groups must exist.
    pub fn parse(fw: &Firewall, words: &[&str]) -> Result<FwAce, String> {
        let mut i = 0usize;
        let w = |i: usize| words.get(i).copied();
        let lw = |i: usize| w(i).map(str::to_ascii_lowercase);
        let permit = match lw(i).as_deref() {
            Some("permit") => true,
            Some("deny") => false,
            _ => return Err("a rule starts with permit or deny".into()),
        };
        i += 1;
        let (proto, svc_group) = match w(i) {
            Some(x) if x.eq_ignore_ascii_case("object-group") => {
                let g = w(i + 1).ok_or("`object-group` needs a service group's name")?;
                if fw.svc_group(g).is_none() {
                    return Err(format!("there is no service object group {g} (create it with `object-group service {g}`)"));
                }
                i += 2;
                (AclProto::Ip, Some(g.to_string()))
            }
            Some(x) => {
                i += 1;
                (AclProto::from_name(x).ok_or_else(|| format!("`{x}` is not a protocol here (ip, icmp, tcp, udp or object-group NAME)"))?, None)
            }
            None => return Err("give a protocol: ip, icmp, tcp, udp or object-group NAME".into()),
        };
        let addr = |i: &mut usize| -> Result<FwAddr, String> {
            let x = w(*i).ok_or("missing an address (any, host A.B.C.D, A.B.C.D MASK, object NAME or object-group NAME)")?;
            *i += 1;
            let lx = x.to_ascii_lowercase();
            if lx == "any" || lx == "any4" {
                return Ok(FwAddr::Any);
            }
            if lx == "host" {
                let a = w(*i).ok_or("`host` needs an address")?;
                *i += 1;
                return a.parse().map(FwAddr::Host).map_err(|_| format!("`{a}` is not an IPv4 address"));
            }
            if lx == "object" {
                let n = w(*i).ok_or("`object` needs a name")?;
                *i += 1;
                if fw.object(n).is_none() {
                    return Err(format!("there is no network object {n} (create it with `object network {n}`)"));
                }
                return Ok(FwAddr::Object(n.to_string()));
            }
            if lx == "object-group" {
                let n = w(*i).ok_or("`object-group` needs a name")?;
                *i += 1;
                if fw.net_group(n).is_none() {
                    return Err(format!("there is no network object group {n} (create it with `object-group network {n}`)"));
                }
                return Ok(FwAddr::Group(n.to_string()));
            }
            let a: Ipv4Addr =
                x.parse().map_err(|_| format!("`{x}` is not an address (any, host A.B.C.D, A.B.C.D MASK, object NAME or object-group NAME)"))?;
            let m = w(*i).ok_or_else(|| format!("{a} needs a mask (255.255.255.0), or write host {a}"))?;
            *i += 1;
            let mask: Ipv4Addr = m.parse().map_err(|_| format!("`{m}` is not a mask"))?;
            let net = Ipv4Net::with_mask(a, mask).map_err(|_| format!("`{m}` is not a mask (the firewall takes masks, not wildcards)"))?;
            Ok(if net.prefix == 32 { FwAddr::Host(a) } else { FwAddr::Net(net.network()) })
        };
        // A port: an operator, or a service group (an `object-group` naming a network group is
        // the next address instead).
        let port = |i: &mut usize| -> Result<(Option<PortMatch>, Option<String>), String> {
            let Some(op) = lw(*i) else { return Ok((None, None)) };
            let num = |i: &mut usize| -> Result<u16, String> {
                let x = w(*i).ok_or("missing a port")?;
                *i += 1;
                port_number(x).ok_or_else(|| format!("`{x}` is not a port"))
            };
            if op == "object-group" {
                return match w(*i + 1) {
                    Some(g) if fw.svc_group(g).is_some() => {
                        *i += 2;
                        Ok((None, Some(g.to_string())))
                    }
                    _ => Ok((None, None)),
                };
            }
            if !matches!(op.as_str(), "eq" | "neq" | "lt" | "gt" | "range") {
                return Ok((None, None));
            }
            *i += 1;
            let m = match op.as_str() {
                "eq" => PortMatch::Eq(num(i)?),
                "neq" => PortMatch::Neq(num(i)?),
                "lt" => PortMatch::Lt(num(i)?),
                "gt" => PortMatch::Gt(num(i)?),
                _ => {
                    let (a, b) = (num(i)?, num(i)?);
                    PortMatch::Range(a.min(b), a.max(b))
                }
            };
            Ok((Some(m), None))
        };
        let ports = matches!(proto, AclProto::Tcp | AclProto::Udp) && svc_group.is_none();
        let src = addr(&mut i)?;
        let (src_port, src_port_group) = if ports { port(&mut i)? } else { (None, None) };
        let dst = addr(&mut i)?;
        let (dst_port, dst_port_group) = if ports { port(&mut i)? } else { (None, None) };
        let mut icmp_type = None;
        if proto == AclProto::Icmp
            && svc_group.is_none()
            && let Some(t) = w(i).filter(|x| !x.eq_ignore_ascii_case("log"))
        {
            icmp_type = Some(
                icmp_type_number(t).ok_or_else(|| format!("`{t}` is not an ICMP type (echo, echo-reply, unreachable, time-exceeded… or 0-255)"))?,
            );
            i += 1;
        }
        match w(i) {
            None => {}
            Some(x) if x.eq_ignore_ascii_case("log") && i + 1 == words.len() => {}
            Some(x) => return Err(format!("`{x}` not understood at the end of the rule")),
        }
        Ok(FwAce { permit, proto, src, src_port, dst, dst_port, icmp_type, svc_group, src_port_group, dst_port_group })
    }
}

/// A port by number or by its usual name.
pub fn port_number(w: &str) -> Option<u16> {
    w.parse().ok().or(match w.to_ascii_lowercase().as_str() {
        "ftp" => Some(21),
        "ssh" => Some(22),
        "telnet" => Some(23),
        "smtp" => Some(25),
        "domain" => Some(53),
        "bootps" => Some(67),
        "bootpc" => Some(68),
        "tftp" => Some(69),
        "www" | "http" => Some(80),
        "pop3" => Some(110),
        "snmp" => Some(161),
        "https" => Some(443),
        _ => None,
    })
}

/// `access-list NAME extended …` (and `access-list NAME remark …`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwAcl {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<FwAce>,
    /// Remarks, each shown before the rule at its position (`entries.len()`: after the last).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remarks: Vec<FwRemark>,
}

/// `access-list NAME remark TEXT`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwRemark {
    pub before: usize,
    pub text: String,
}

/// Longest remark.
pub const MAX_REMARK: usize = 100;

impl FwAcl {
    /// The first matching entry's index and whether it permits; `None`: the implicit deny.
    pub fn evaluate(&self, fw: &Firewall, p: &FlowKey) -> (bool, Option<usize>) {
        match self.entries.iter().position(|e| e.matches(fw, p)) {
            Some(i) => (self.entries.get(i).is_some_and(|e| e.permit), Some(i)),
            None => (false, None),
        }
    }

    /// Removes rule `i`; the remarks after it move up with the rules.
    pub fn remove_entry(&mut self, i: usize) {
        if i >= self.entries.len() {
            return;
        }
        self.entries.remove(i);
        for r in &mut self.remarks {
            if r.before > i {
                r.before -= 1;
            }
        }
    }

    /// The configuration lines, remarks in place.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..=self.entries.len() {
            for r in self.remarks.iter().filter(|r| r.before == i) {
                out.push(format!("access-list {} remark {}", self.name, r.text));
            }
            if let Some(e) = self.entries.get(i) {
                out.push(format!("access-list {} extended {}", self.name, e.text()));
            }
        }
        out
    }
}

/// `access-group ACL in|out interface IFNAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessGroup {
    pub acl: String,
    pub nameif: String,
    /// Checks traffic leaving by the interface instead of arriving on it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub out: bool,
}

/// One line of `object-group network NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetMember {
    Host(Ipv4Addr),
    /// Host bits clear.
    Net(Ipv4Net),
    Object(String),
    Group(String),
}

impl NetMember {
    pub fn text(&self) -> String {
        match self {
            NetMember::Host(h) => format!("network-object host {h}"),
            NetMember::Net(n) => format!("network-object {} {}", n.addr, n.mask()),
            NetMember::Object(o) => format!("network-object object {o}"),
            NetMember::Group(g) => format!("group-object {g}"),
        }
    }
}

/// `object-group network NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetGroup {
    pub name: String,
    #[serde(default)]
    pub members: Vec<NetMember>,
}

/// The protocols a typed service group's ports are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SvcProto {
    Tcp,
    Udp,
    TcpUdp,
}

impl SvcProto {
    pub fn name(self) -> &'static str {
        match self {
            SvcProto::Tcp => "tcp",
            SvcProto::Udp => "udp",
            SvcProto::TcpUdp => "tcp-udp",
        }
    }

    pub fn from_name(s: &str) -> Option<SvcProto> {
        match s.to_ascii_lowercase().as_str() {
            "tcp" => Some(SvcProto::Tcp),
            "udp" => Some(SvcProto::Udp),
            "tcp-udp" => Some(SvcProto::TcpUdp),
            _ => None,
        }
    }

    pub fn covers(self, protocol: u8) -> bool {
        match self {
            SvcProto::Tcp => protocol == 6,
            SvcProto::Udp => protocol == 17,
            SvcProto::TcpUdp => protocol == 6 || protocol == 17,
        }
    }
}

/// One line of `object-group service NAME [tcp|udp|tcp-udp]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SvcMember {
    /// `port-object eq|range …` (typed groups).
    Port(PortMatch),
    /// `service-object PROTO [destination eq|range …]` (untyped groups).
    Service {
        proto: AclProto,
        dst: Option<PortMatch>,
    },
    Group(String),
}

impl SvcMember {
    pub fn text(&self) -> String {
        match self {
            SvcMember::Port(p) => format!("port-object {p}"),
            SvcMember::Service { proto, dst: Some(p) } => format!("service-object {} destination {p}", proto.name()),
            SvcMember::Service { proto, dst: None } => format!("service-object {}", proto.name()),
            SvcMember::Group(g) => format!("group-object {g}"),
        }
    }
}

/// `object-group service NAME [tcp|udp|tcp-udp]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvcGroup {
    pub name: String,
    /// Set: a group of ports for these protocols; unset: a group of services.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proto: Option<SvcProto>,
    #[serde(default)]
    pub members: Vec<SvcMember>,
}

/// Most members in one object group, and how deep groups may nest.
pub const MAX_MEMBERS: usize = 256;
pub const MAX_NESTING: u8 = 8;

/// `icmp permit|deny SOURCE [TYPE] IFNAME`: ICMP to the firewall's own address on an interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IcmpRule {
    pub permit: bool,
    pub src: FwAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icmp_type: Option<u8>,
    pub nameif: String,
}

impl IcmpRule {
    pub fn text(&self) -> String {
        let t = self.icmp_type.map(|t| format!(" {}", icmp_type_name(t))).unwrap_or_default();
        format!("icmp {} {}{t} {}", if self.permit { "permit" } else { "deny" }, self.src, self.nameif)
    }
}

/// `nat (REAL,MAPPED) source static|dynamic REAL-OBJ MAPPED-OBJ [destination static MAPPED-DST REAL-DST]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwiceNat {
    pub real_if: String,
    pub mapped_if: String,
    #[serde(default)]
    pub dynamic: bool,
    /// The real sources: a network object or group.
    pub src_real: String,
    /// What they become: a network object, or `interface` (dynamic only).
    pub src_mapped: String,
    /// `destination static MAPPED REAL`: traffic to the mapped destination goes to the real one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst: Option<(String, String)>,
}

impl TwiceNat {
    pub fn text(&self) -> String {
        let mut s = format!(
            "nat ({},{}) source {} {} {}",
            self.real_if,
            self.mapped_if,
            if self.dynamic { "dynamic" } else { "static" },
            self.src_real,
            self.src_mapped
        );
        if let Some((m, r)) = &self.dst {
            s.push_str(&format!(" destination static {m} {r}"));
        }
        s
    }

    /// Whether the rule names an object or group.
    pub fn uses(&self, name: &str) -> bool {
        self.src_real == name || self.src_mapped == name || self.dst.as_ref().is_some_and(|(m, r)| m == name || r == name)
    }
}

/// A static route's administrative distance, when it isn't 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDistance {
    pub net: Ipv4Net,
    pub gateway: Ipv4Addr,
    pub distance: u8,
}

/// `class-map NAME` (modular policy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwClassMap {
    pub name: String,
    /// `match default-inspection-traffic` or `match any`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches: Option<String>,
}

/// `class NAME` inside a policy map, with what it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwPolicyClass {
    pub class: String,
    /// `inspect icmp`, `inspect icmp error`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inspect: Vec<String>,
    /// `set connection decrement-ttl`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub decrement_ttl: bool,
}

/// `policy-map NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwPolicyMap {
    pub name: String,
    #[serde(default)]
    pub classes: Vec<FwPolicyClass>,
}

/// The inspections the firewall knows.
pub const INSPECTIONS: &[&str] = &["icmp", "icmp error"];
/// Most class maps, policy maps and classes per policy map.
pub const MAX_POLICY: usize = 32;

/// `dhcpd address A-B IFNAME` (and `dhcpd enable IFNAME`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dhcpd {
    pub nameif: String,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
    #[serde(default)]
    pub enabled: bool,
}

impl Dhcpd {
    pub fn addresses(&self) -> impl Iterator<Item = Ipv4Addr> {
        (u32::from(self.start)..=u32::from(self.end)).take(MAX_DHCPD_ADDRESSES as usize).map(Ipv4Addr::from)
    }
}

/// Idle timeouts, in seconds (`timeout conn|udp|icmp H:MM:SS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeouts {
    pub conn: u32,
    pub udp: u32,
    pub icmp: u32,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts { conn: 3_600, udp: 120, icmp: 2 }
    }
}

/// `H:MM:SS` (or plain seconds) → seconds, 1 s to 1193 h.
pub fn parse_duration(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    let n = |w: &str| w.parse::<u32>().ok();
    let secs = match parts.as_slice() {
        [s] => n(s)?,
        [m, s] => n(m)?.checked_mul(60)?.checked_add(n(s).filter(|s| *s < 60)?)?,
        [h, m, s] => n(h)?.checked_mul(3_600)?.checked_add(n(m).filter(|m| *m < 60)?.checked_mul(60)?)?.checked_add(n(s).filter(|s| *s < 60)?)?,
        _ => return None,
    };
    (1..=1_193 * 3_600).contains(&secs).then_some(secs)
}

/// Seconds → `H:MM:SS`.
pub fn duration_text(secs: u32) -> String {
    format!("{}:{:02}:{:02}", secs / 3_600, secs / 60 % 60, secs % 60)
}

/// A firewall's configuration (everything but addresses and routes, which live on the device
/// as on a router).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Firewall {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ifaces: Vec<FwIface>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<NetObject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acls: Vec<FwAcl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<AccessGroup>,
    /// `same-security-traffic permit inter-interface`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub same_security: bool,
    #[serde(default)]
    pub timeouts: Timeouts,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dhcpd: Vec<Dhcpd>,
    /// `dhcpd dns A.B.C.D`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_dns: Option<Ipv4Addr>,
    /// `same-security-traffic permit intra-interface`: traffic may leave by the interface it
    /// came in on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub same_security_intra: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net_groups: Vec<NetGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub svc_groups: Vec<SvcGroup>,
    /// ICMP to the firewall itself, per interface (an interface without rules answers all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icmp_rules: Vec<IcmpRule>,
    /// Twice NAT, checked before object NAT, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub twice: Vec<TwiceNat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route_distances: Vec<RouteDistance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_maps: Vec<FwClassMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_maps: Vec<FwPolicyMap>,
    /// `service-policy NAME global`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_policy: Option<String>,
}

impl Firewall {
    pub fn is_default(&self) -> bool {
        *self == Firewall::default()
    }

    pub fn object(&self, name: &str) -> Option<&NetObject> {
        self.objects.iter().find(|o| o.name == name)
    }

    pub fn acl(&self, name: &str) -> Option<&FwAcl> {
        self.acls.iter().find(|a| a.name == name)
    }

    /// The settings of a port, by its full name.
    pub fn iface(&self, port: &str) -> Option<&FwIface> {
        self.ifaces.iter().find(|i| i.port == port)
    }

    /// The settings of a port, created when missing (`None` past 64 ports).
    pub fn iface_mut(&mut self, port: &str) -> Option<&mut FwIface> {
        if !self.ifaces.iter().any(|i| i.port == port) {
            if self.ifaces.len() >= 64 {
                return None;
            }
            self.ifaces.push(FwIface { port: port.to_string(), nameif: None, level: 0 });
        }
        self.ifaces.iter_mut().find(|i| i.port == port)
    }

    /// The port carrying a nameif.
    pub fn port_of(&self, nameif: &str) -> Option<&str> {
        self.ifaces.iter().find(|i| i.nameif.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(nameif))).map(|i| i.port.as_str())
    }

    /// The nameif and security level of a port, if it has a nameif.
    pub fn zone(&self, port: &str) -> Option<(&str, u8)> {
        let i = self.iface(port)?;
        Some((i.nameif.as_deref()?, i.level))
    }

    /// The access list bound to arriving traffic on an interface.
    pub fn group_for(&self, nameif: &str) -> Option<&str> {
        self.groups.iter().find(|g| !g.out && g.nameif.eq_ignore_ascii_case(nameif)).map(|g| g.acl.as_str())
    }

    /// The access list bound to leaving traffic on an interface.
    pub fn group_out_for(&self, nameif: &str) -> Option<&str> {
        self.groups.iter().find(|g| g.out && g.nameif.eq_ignore_ascii_case(nameif)).map(|g| g.acl.as_str())
    }

    pub fn net_group(&self, name: &str) -> Option<&NetGroup> {
        self.net_groups.iter().find(|g| g.name == name)
    }

    pub fn svc_group(&self, name: &str) -> Option<&SvcGroup> {
        self.svc_groups.iter().find(|g| g.name == name)
    }

    /// Whether a network group (or a group nested in it) holds an address.
    pub fn net_group_has(&self, name: &str, a: Ipv4Addr, depth: u8) -> bool {
        if depth > MAX_NESTING {
            return false;
        }
        self.net_group(name).is_some_and(|g| {
            g.members.iter().any(|m| match m {
                NetMember::Host(h) => *h == a,
                NetMember::Net(n) => n.contains(a),
                NetMember::Object(o) => self.object(o).and_then(|o| o.addr).is_some_and(|o| o.contains(a)),
                NetMember::Group(g) => self.net_group_has(g, a, depth + 1),
            })
        })
    }

    /// Whether a typed service group (or one nested in it) holds a port for a protocol.
    pub fn svc_group_port(&self, name: &str, protocol: u8, port: u16, depth: u8) -> bool {
        if depth > MAX_NESTING {
            return false;
        }
        self.svc_group(name).is_some_and(|g| {
            g.members.iter().any(|m| match m {
                SvcMember::Port(p) => g.proto.is_some_and(|sp| sp.covers(protocol)) && p.matches(port),
                SvcMember::Service { proto, dst } => proto.number().is_none_or(|n| n == protocol) && dst.is_none_or(|d| d.matches(port)),
                SvcMember::Group(n) => self.svc_group_port(n, protocol, port, depth + 1),
            })
        })
    }

    /// Whether a service group, used in place of a protocol, covers a packet (protocol and
    /// destination port).
    pub fn svc_group_service(&self, name: &str, p: &FlowKey, depth: u8) -> bool {
        if depth > MAX_NESTING {
            return false;
        }
        self.svc_group(name).is_some_and(|g| {
            g.members.iter().any(|m| match m {
                SvcMember::Port(pm) => g.proto.is_some_and(|sp| sp.covers(p.protocol)) && p.dst_port.is_some_and(|v| pm.matches(v)),
                SvcMember::Service { proto, dst } => {
                    proto.number().is_none_or(|n| n == p.protocol) && dst.is_none_or(|d| p.dst_port.is_some_and(|v| d.matches(v)))
                }
                SvcMember::Group(n) => self.svc_group_service(n, p, depth + 1),
            })
        })
    }

    /// Whether a name is taken by an object or a group.
    pub fn name_taken(&self, name: &str) -> bool {
        self.object(name).is_some() || self.net_group(name).is_some() || self.svc_group(name).is_some()
    }

    /// What uses an object or group (a rule, a group, a NAT rule), for refusing its deletion.
    pub fn user_of(&self, name: &str) -> Option<String> {
        if let Some(a) = self.acls.iter().find(|a| a.entries.iter().any(|e| e.references().contains(&name))) {
            return Some(format!("access list {}", a.name));
        }
        if let Some(g) =
            self.net_groups.iter().find(|g| g.members.iter().any(|m| matches!(m, NetMember::Object(n) | NetMember::Group(n) if n == name)))
        {
            return Some(format!("object group {}", g.name));
        }
        if let Some(g) = self.svc_groups.iter().find(|g| g.members.iter().any(|m| matches!(m, SvcMember::Group(n) if n == name))) {
            return Some(format!("object group {}", g.name));
        }
        if self.twice.iter().any(|t| t.uses(name)) {
            return Some("a twice NAT rule".into());
        }
        self.icmp_rules.iter().any(|r| r.src.reference() == Some(name)).then(|| "an icmp rule".to_string())
    }

    /// Whether an address is in a network object or network group.
    pub fn addr_in(&self, name: &str, a: Ipv4Addr) -> bool {
        match self.object(name) {
            Some(o) => o.addr.is_some_and(|o| o.contains(a)),
            None => self.net_group_has(name, a, 0),
        }
    }

    /// Twice NAT: `a`, one of `from`'s addresses, as the matching address of object `to` (a host
    /// maps everything to itself; a subnet keeps the host bits).
    pub fn map_between(&self, from: &str, to: &str, a: Ipv4Addr) -> Option<Ipv4Addr> {
        if !self.addr_in(from, a) {
            return None;
        }
        match self.object(to)?.addr? {
            ObjAddr::Host(h) => Some(h),
            ObjAddr::Subnet(n) => Some(Ipv4Addr::from(u32::from(n.addr) | (u32::from(a) & !u32::from(n.mask())))),
        }
    }

    /// The applied global policy's class actions (classes whose class map matches something).
    fn active_classes(&self) -> impl Iterator<Item = &FwPolicyClass> {
        let pm = self.service_policy.as_deref().and_then(|n| self.policy_maps.iter().find(|p| p.name == n));
        pm.into_iter()
            .flat_map(|p| p.classes.iter())
            .filter(|c| c.class.eq_ignore_ascii_case("class-default") || self.class_maps.iter().any(|m| m.name == c.class && m.matches.is_some()))
    }

    /// Whether the applied global policy inspects something (`icmp`, `icmp error`).
    pub fn inspects(&self, what: &str) -> bool {
        self.active_classes().any(|c| c.inspect.iter().any(|i| i == what))
    }

    /// Whether the applied global policy makes the firewall decrement TTL (a hop of its own).
    pub fn decrements_ttl(&self) -> bool {
        self.active_classes().any(|c| c.decrement_ttl)
    }

    /// A static route's administrative distance.
    pub fn route_distance(&self, net: Ipv4Net, gateway: Ipv4Addr) -> u8 {
        self.route_distances.iter().find(|r| r.net == net && r.gateway == gateway).map_or(1, |r| r.distance)
    }

    /// Whether ICMP of this type from `src`, to the firewall's address on `nameif`, is answered:
    /// the first matching `icmp` rule of the interface decides; with none, everything is.
    pub fn icmp_allowed(&self, nameif: &str, src: Ipv4Addr, icmp_type: u8) -> (bool, Option<usize>) {
        let rules: Vec<&IcmpRule> = self.icmp_rules.iter().filter(|r| r.nameif.eq_ignore_ascii_case(nameif)).collect();
        if rules.is_empty() {
            return (true, None);
        }
        match rules.iter().position(|r| r.src.matches(self, src) && r.icmp_type.is_none_or(|t| t == icmp_type)) {
            Some(i) => (rules.get(i).is_some_and(|r| r.permit), Some(i)),
            None => (false, None),
        }
    }

    /// Repairs what a hand-edited file could break.
    pub fn sanitize(&mut self) {
        self.ifaces.truncate(64);
        self.objects.truncate(MAX_OBJECTS);
        self.acls.truncate(MAX_LISTS);
        for a in &mut self.acls {
            a.entries.truncate(MAX_ENTRIES);
        }
        self.groups.truncate(MAX_LISTS);
        self.dhcpd.truncate(MAX_DHCPD);
        for a in &mut self.acls {
            a.remarks.truncate(MAX_ENTRIES);
            let n = a.entries.len();
            for r in &mut a.remarks {
                r.before = r.before.min(n);
            }
        }
        self.net_groups.truncate(MAX_OBJECTS);
        self.svc_groups.truncate(MAX_OBJECTS);
        for g in &mut self.net_groups {
            g.members.truncate(MAX_MEMBERS);
        }
        for g in &mut self.svc_groups {
            g.members.truncate(MAX_MEMBERS);
        }
        self.icmp_rules.truncate(MAX_ENTRIES);
        self.twice.truncate(MAX_ENTRIES);
        self.route_distances.truncate(crate::MAX_ROUTES);
        self.class_maps.truncate(MAX_POLICY);
        self.policy_maps.truncate(MAX_POLICY);
        for p in &mut self.policy_maps {
            p.classes.truncate(MAX_POLICY);
        }
        for i in &mut self.ifaces {
            i.level = i.level.min(100);
        }
        let t = &mut self.timeouts;
        for v in [&mut t.conn, &mut t.udp, &mut t.icmp] {
            *v = (*v).clamp(1, 1_193 * 3_600);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_objects_and_durations() {
        let mut fw = Firewall::default();
        fw.objects.push(NetObject {
            name: "WEB".into(),
            addr: Some(ObjAddr::Host("172.16.1.10".parse().unwrap())),
            nat: Some(ObjNat { real_if: "dmz".into(), mapped_if: "outside".into(), kind: NatKind::Static("203.0.113.10".parse().unwrap()) }),
        });
        let ace = FwAce::parse(&fw, &["permit", "tcp", "any", "object", "WEB", "eq", "www"]).unwrap();
        assert_eq!(ace.text(), "permit tcp any object WEB eq 80");
        let k = FlowKey {
            protocol: 6,
            src: "8.8.8.8".parse().unwrap(),
            dst: "172.16.1.10".parse().unwrap(),
            src_port: Some(5000),
            dst_port: Some(80),
            icmp_type: None,
        };
        assert!(ace.matches(&fw, &k));
        assert!(!ace.matches(&fw, &FlowKey { dst_port: Some(443), ..k }));
        assert!(FwAce::parse(&fw, &["permit", "tcp", "any", "object", "NOPE"]).is_err());
        assert!(FwAce::parse(&fw, &["permit", "ip", "10.0.0.0", "0.0.0.255", "any"]).is_err());
        let net = FwAce::parse(&fw, &["deny", "ip", "10.0.0.0", "255.0.0.0", "any4"]).unwrap();
        assert_eq!(net.text(), "deny ip 10.0.0.0 255.0.0.0 any");
        let o = fw.object("WEB").unwrap();
        assert_eq!(o.static_map("172.16.1.10".parse().unwrap()), Some("203.0.113.10".parse().unwrap()));
        assert_eq!(o.static_unmap("203.0.113.10".parse().unwrap()), Some("172.16.1.10".parse().unwrap()));
        let sub = NetObject {
            name: "LAN".into(),
            addr: Some(ObjAddr::Subnet("10.1.1.0/24".parse().unwrap())),
            nat: Some(ObjNat { real_if: "inside".into(), mapped_if: "outside".into(), kind: NatKind::Static("198.51.100.0".parse().unwrap()) }),
        };
        assert_eq!(sub.static_map("10.1.1.7".parse().unwrap()), Some("198.51.100.7".parse().unwrap()));
        assert_eq!(sub.static_unmap("198.51.100.7".parse().unwrap()), Some("10.1.1.7".parse().unwrap()));
        assert_eq!(parse_duration("0:00:30"), Some(30));
        assert_eq!(parse_duration("1:00:00"), Some(3_600));
        assert_eq!(parse_duration("0:61:00"), None);
        assert_eq!(parse_duration("0"), None);
        assert_eq!(duration_text(125), "0:02:05");
        assert!(valid_name("inside") && !valid_name("1x") && !valid_name("a b"));
    }

    #[test]
    fn groups_twice_mapping_icmp_rules_and_policy() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        let mut fw = Firewall::default();
        fw.objects.push(NetObject { name: "LAN".into(), addr: Some(ObjAddr::Subnet("10.1.1.0/24".parse().unwrap())), nat: None });
        fw.objects.push(NetObject { name: "PUB".into(), addr: Some(ObjAddr::Subnet("198.51.100.0/24".parse().unwrap())), nat: None });
        fw.net_groups.push(NetGroup { name: "A".into(), members: vec![NetMember::Host(ip("8.8.8.8")), NetMember::Object("LAN".into())] });
        fw.net_groups.push(NetGroup { name: "B".into(), members: vec![NetMember::Group("A".into())] });
        // A loop in a hand-edited file stops at the nesting limit.
        fw.net_groups.push(NetGroup { name: "L".into(), members: vec![NetMember::Group("L".into())] });
        assert!(fw.net_group_has("B", ip("10.1.1.9"), 0) && fw.net_group_has("B", ip("8.8.8.8"), 0));
        assert!(!fw.net_group_has("B", ip("8.8.4.4"), 0) && !fw.net_group_has("L", ip("1.1.1.1"), 0));
        fw.svc_groups.push(SvcGroup { name: "W".into(), proto: Some(SvcProto::TcpUdp), members: vec![SvcMember::Port(PortMatch::Eq(53))] });
        fw.svc_groups.push(SvcGroup {
            name: "S".into(),
            proto: None,
            members: vec![SvcMember::Service { proto: AclProto::Tcp, dst: Some(PortMatch::Eq(80)) }, SvcMember::Group("W".into())],
        });
        let ace = FwAce::parse(&fw, &["permit", "object-group", "S", "object-group", "B", "any"]).unwrap();
        assert_eq!(ace.text(), "permit object-group S object-group B any");
        let k = |protocol: u8, port: u16| FlowKey {
            protocol,
            src: ip("10.1.1.5"),
            dst: ip("1.2.3.4"),
            src_port: Some(40000),
            dst_port: Some(port),
            icmp_type: None,
        };
        assert!(ace.matches(&fw, &k(6, 80)) && ace.matches(&fw, &k(17, 53)) && !ace.matches(&fw, &k(17, 80)));
        let ports = FwAce::parse(&fw, &["permit", "udp", "any", "object-group", "B", "object-group", "W"]).unwrap();
        assert_eq!(ports.dst, FwAddr::Group("B".into()));
        assert_eq!(ports.dst_port_group.as_deref(), Some("W"));
        let echo = FwAce::parse(&fw, &["permit", "icmp", "any", "any", "echo-reply"]).unwrap();
        assert_eq!(echo.text(), "permit icmp any any echo-reply");
        assert!(echo.matches(&fw, &FlowKey { protocol: 1, icmp_type: Some(0), ..k(1, 0) }));
        assert!(!echo.matches(&fw, &FlowKey { protocol: 1, icmp_type: Some(8), ..k(1, 0) }));
        assert_eq!(fw.map_between("LAN", "PUB", ip("10.1.1.7")), Some(ip("198.51.100.7")));
        assert_eq!(fw.map_between("PUB", "LAN", ip("198.51.100.7")), Some(ip("10.1.1.7")));
        assert_eq!(fw.map_between("LAN", "PUB", ip("10.1.2.7")), None);
        assert_eq!(fw.user_of("A").as_deref(), Some("object group B"));
        // ICMP to the firewall: everything until an interface has rules, then the first match.
        assert_eq!(fw.icmp_allowed("outside", ip("1.1.1.1"), 8), (true, None));
        fw.icmp_rules.push(IcmpRule { permit: true, src: FwAddr::Any, icmp_type: Some(0), nameif: "outside".into() });
        assert_eq!(fw.icmp_allowed("outside", ip("1.1.1.1"), 0), (true, Some(0)));
        assert_eq!(fw.icmp_allowed("outside", ip("1.1.1.1"), 8), (false, None));
        assert_eq!(fw.icmp_allowed("inside", ip("1.1.1.1"), 8), (true, None));
        // The policy counts only once it is applied, through a class that matches something.
        fw.class_maps.push(FwClassMap { name: "C".into(), matches: None });
        fw.policy_maps.push(FwPolicyMap {
            name: "P".into(),
            classes: vec![FwPolicyClass { class: "C".into(), inspect: vec!["icmp".into()], decrement_ttl: true }],
        });
        assert!(!fw.inspects("icmp"));
        fw.service_policy = Some("P".into());
        assert!(!fw.inspects("icmp") && !fw.decrements_ttl());
        fw.class_maps[0].matches = Some("default-inspection-traffic".into());
        assert!(fw.inspects("icmp") && fw.decrements_ttl() && !fw.inspects("icmp error"));
    }
}
