//! The stateful firewall appliance (model `Firewall-5`): named interfaces with security levels,
//! access rules bound to interfaces, network objects with their own NAT, and a small DHCP
//! server. The simulator keeps the connection and translation tables.
//!
//! Policy, in short: a connection may start from a higher security level towards a lower one;
//! anything else needs an access rule on the interface it arrives on. Replies to a connection
//! the firewall let through always pass.

use std::net::Ipv4Addr;

use packetcraft_core::Ipv4Net;
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
}

impl FwAddr {
    pub fn matches(&self, fw: &Firewall, a: Ipv4Addr) -> bool {
        match self {
            FwAddr::Any => true,
            FwAddr::Host(h) => *h == a,
            FwAddr::Net(n) => n.contains(a),
            FwAddr::Object(name) => fw.object(name).and_then(|o| o.addr).is_some_and(|o| o.contains(a)),
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
}

/// What a packet looks like to a firewall rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowKey {
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
}

impl FwAce {
    pub fn matches(&self, fw: &Firewall, p: &FlowKey) -> bool {
        let port_ok = |m: Option<PortMatch>, v: Option<u16>| m.is_none_or(|m| v.is_some_and(|v| m.matches(v)));
        self.proto.number().is_none_or(|n| n == p.protocol)
            && self.src.matches(fw, p.src)
            && self.dst.matches(fw, p.dst)
            && port_ok(self.src_port, p.src_port)
            && port_ok(self.dst_port, p.dst_port)
    }

    /// The words after `extended`.
    pub fn text(&self) -> String {
        let mut s = format!("{} {} {}", if self.permit { "permit" } else { "deny" }, self.proto.name(), self.src);
        if let Some(p) = self.src_port {
            s.push_str(&format!(" {p}"));
        }
        s.push_str(&format!(" {}", self.dst));
        if let Some(p) = self.dst_port {
            s.push_str(&format!(" {p}"));
        }
        s
    }

    /// Parses `permit|deny PROTO SRC [PORT] DST [PORT]`, where an address is `any`, `any4`,
    /// `host A.B.C.D`, `A.B.C.D MASK` or `object NAME`, and a port `eq|neq|lt|gt P` or
    /// `range P Q` (a number or a usual name: `www`, `https`, `domain`…). Objects must exist.
    pub fn parse(fw: &Firewall, words: &[&str]) -> Result<FwAce, String> {
        let mut it = words.iter().copied().peekable();
        let permit = match it.next().map(str::to_ascii_lowercase).as_deref() {
            Some("permit") => true,
            Some("deny") => false,
            _ => return Err("a rule starts with permit or deny".into()),
        };
        let proto = match it.next() {
            Some(w) => AclProto::from_name(w).ok_or_else(|| format!("`{w}` is not a protocol here (ip, icmp, tcp, udp)"))?,
            None => return Err("give a protocol: ip, icmp, tcp or udp".into()),
        };
        type Words<'a> = std::iter::Peekable<std::iter::Copied<std::slice::Iter<'a, &'a str>>>;
        let addr = |it: &mut Words<'_>| -> Result<FwAddr, String> {
            let w = it.next().ok_or("missing an address (any, host A.B.C.D, A.B.C.D MASK or object NAME)")?;
            let lw = w.to_ascii_lowercase();
            if lw == "any" || lw == "any4" {
                return Ok(FwAddr::Any);
            }
            if lw == "host" {
                let a = it.next().ok_or("`host` needs an address")?;
                return a.parse().map(FwAddr::Host).map_err(|_| format!("`{a}` is not an IPv4 address"));
            }
            if lw == "object" {
                let n = it.next().ok_or("`object` needs a name")?;
                if fw.object(n).is_none() {
                    return Err(format!("there is no network object {n} (create it with `object network {n}`)"));
                }
                return Ok(FwAddr::Object(n.to_string()));
            }
            let a: Ipv4Addr = w.parse().map_err(|_| format!("`{w}` is not an address (any, host A.B.C.D, A.B.C.D MASK or object NAME)"))?;
            let m = it.next().ok_or_else(|| format!("{a} needs a mask (255.255.255.0), or write host {a}"))?;
            let mask: Ipv4Addr = m.parse().map_err(|_| format!("`{m}` is not a mask"))?;
            let net = Ipv4Net::with_mask(a, mask).map_err(|_| format!("`{m}` is not a mask (the firewall takes masks, not wildcards)"))?;
            Ok(if net.prefix == 32 { FwAddr::Host(a) } else { FwAddr::Net(net.network()) })
        };
        let port = |it: &mut Words<'_>| -> Result<Option<PortMatch>, String> {
            let Some(op) = it.peek().map(|w| w.to_ascii_lowercase()) else { return Ok(None) };
            let num = |it: &mut Words<'_>| -> Result<u16, String> {
                let w = it.next().ok_or("missing a port")?;
                port_number(w).ok_or_else(|| format!("`{w}` is not a port"))
            };
            let m = match op.as_str() {
                "eq" | "neq" | "lt" | "gt" | "range" => {
                    it.next();
                    match op.as_str() {
                        "eq" => PortMatch::Eq(num(it)?),
                        "neq" => PortMatch::Neq(num(it)?),
                        "lt" => PortMatch::Lt(num(it)?),
                        "gt" => PortMatch::Gt(num(it)?),
                        _ => {
                            let (a, b) = (num(it)?, num(it)?);
                            PortMatch::Range(a.min(b), a.max(b))
                        }
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
        match it.next() {
            None => {}
            Some(w) if w.eq_ignore_ascii_case("log") => {}
            Some(w) => return Err(format!("`{w}` not understood at the end of the rule")),
        }
        Ok(FwAce { permit, proto, src, src_port, dst, dst_port })
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

/// `access-list NAME extended …`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FwAcl {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<FwAce>,
}

impl FwAcl {
    /// The first matching entry's index and whether it permits; `None`: the implicit deny.
    pub fn evaluate(&self, fw: &Firewall, p: &FlowKey) -> (bool, Option<usize>) {
        match self.entries.iter().position(|e| e.matches(fw, p)) {
            Some(i) => (self.entries.get(i).is_some_and(|e| e.permit), Some(i)),
            None => (false, None),
        }
    }
}

/// `access-group ACL in interface IFNAME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessGroup {
    pub acl: String,
    pub nameif: String,
}

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
        self.groups.iter().find(|g| g.nameif.eq_ignore_ascii_case(nameif)).map(|g| g.acl.as_str())
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
        let k =
            FlowKey { protocol: 6, src: "8.8.8.8".parse().unwrap(), dst: "172.16.1.10".parse().unwrap(), src_port: Some(5000), dst_port: Some(80) };
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
}
