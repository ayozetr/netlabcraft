//! Network automation, engine side: what a router's RESTCONF server (RFC 8040) answers, and the
//! PC commands that call it (`pc.http_request`, `pc.http_saved`).
//!
//! # The data this server knows
//!
//! A small, documented subset, named after the public IETF modules where one exists:
//!
//! - `ietf-system:system/hostname` (RFC 7317).
//! - `ietf-interfaces:interfaces/interface=NAME` (RFC 8343): `name`, `description`, `type`
//!   (`iana-if-type:…`), `enabled`, read-only `oper-status` and `phys-address`, and the
//!   `ietf-ip:ipv4` container (RFC 8344) with one `address` entry (`ip`, and `netmask` or
//!   `prefix-length`), and the `ietf-ip:ipv6` container with its global `address` entries
//!   (`ip`, `prefix-length`). Ports, subinterfaces and loopbacks are listed; loopbacks can be
//!   created and deleted.
//! - `netlabcraft-routing:static-routes/route=PREFIX`: NetLabCraft's own small module, since
//!   the IETF routing model (RFC 8349) is far deeper than a lab needs. A route is
//!   `{"prefix": "10.1.0.0/16", "next-hop": [{"address": "10.0.0.2"} | {"interface": "Serial0/1/0"}, "distance"?: 1-255]}`.
//!
//! Every edit is turned into configuration lines, checked by running them through the console
//! grammar on a copy of the lab, and applied the same way afterwards, so `show running-config`
//! shows exactly what a person would have typed.
//!
//! Requests log in with HTTP Basic authentication against the router's local usernames; only
//! privilege 15 users may use the API (RFC 8040 §2.5 leaves the method to the server).
//!
//! Bodies are JSON (`application/yang-data+json`, RFC 7951) or XML (`application/yang-data+xml`,
//! RFC 7950 §7): `Content-Type` says which a request carries, and `Accept` picks the answer's
//! encoding (JSON when it says nothing; 406 when it accepts neither). Errors come back in the
//! encoding asked for.

use netlabcraft_core::{DeviceId, Ipv4Net, Ipv6Net, parse_ip};
use netlabcraft_model::{Device, End, NextHop, Topology, ifname};
use netlabcraft_sim::automation::Served;
use netlabcraft_wire::http::{self, Request, Response};
use serde_json::{Map, Value, json};

use crate::{EngineError, Result, Session};

pub const IF: &str = "ietf-interfaces";
pub const IP: &str = "ietf-ip";
pub const SYS: &str = "ietf-system";
pub const RT: &str = "netlabcraft-routing";
pub const RESTCONF: &str = "ietf-restconf";

/// XML namespaces of the modules (for NETCONF).
pub fn namespace(module: &str) -> &'static str {
    match module {
        IF => "urn:ietf:params:xml:ns:yang:ietf-interfaces",
        IP => "urn:ietf:params:xml:ns:yang:ietf-ip",
        SYS => "urn:ietf:params:xml:ns:yang:ietf-system",
        RT => "urn:netlabcraft:yang:netlabcraft-routing",
        RESTCONF => "urn:ietf:params:xml:ns:yang:ietf-restconf",
        _ => "",
    }
}

/// The module whose namespace this is (the reverse of [`namespace`]).
pub fn module_of_ns(ns: &str) -> Option<&'static str> {
    [IF, IP, SYS, RT, RESTCONF].into_iter().find(|m| namespace(m) == ns)
}

/// The modules this server implements, with their revisions (for the capabilities NETCONF
/// announces and `ietf-yang-library`-style listings).
pub const MODULES: [(&str, &str); 4] = [(IF, "2018-02-20"), (IP, "2018-02-22"), (SYS, "2014-08-06"), (RT, "2026-10-30")];

/// The JSON media type (RFC 8040 §11.3.2).
pub const YANG_JSON: &str = "application/yang-data+json";
/// The XML media type (RFC 8040 §11.3.1).
pub const YANG_XML: &str = "application/yang-data+xml";
/// Most interfaces or routes one request may touch.
const MAX_ENTRIES: usize = 64;
/// Most characters in a text value.
const MAX_TEXT: usize = 240;

/// An error as RESTCONF reports it (RFC 8040 §7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub tag: &'static str,
    pub message: String,
}

fn err(status: u16, tag: &'static str, message: impl Into<String>) -> ApiError {
    ApiError { status, tag, message: message.into() }
}

fn invalid(message: impl Into<String>) -> ApiError {
    err(400, "invalid-value", message)
}

fn missing(message: impl Into<String>) -> ApiError {
    err(404, "invalid-value", message)
}

type ApiResult<T> = std::result::Result<T, ApiError>;

// ---- the data ----

/// One interface as the data shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iface {
    pub name: String,
    pub kind: &'static str,
    pub description: String,
    pub enabled: bool,
    pub ip: Option<Ipv4Net>,
    /// Global IPv6 addresses (`ietf-ip:ipv6`).
    pub ipv6: Vec<Ipv6Net>,
    pub up: bool,
    pub mac: Option<String>,
    pub loopback: bool,
}

/// The interfaces of a router: ports, their subinterfaces, loopbacks.
pub fn interfaces(t: &Topology, d: &Device) -> Vec<Iface> {
    let mut out = Vec::new();
    for (i, p) in d.ports.iter().enumerate() {
        if ifname::parse(&p.name).is_none() {
            continue;
        }
        let kind = if ifname::is_serial(&p.name) {
            "iana-if-type:propPointToPointSerial"
        } else if ifname::is_wireless(&p.name) {
            "iana-if-type:ieee80211"
        } else {
            "iana-if-type:ethernetCsmacd"
        };
        let link = netlabcraft_sim::port_up(t, End { device: d.id, port: i });
        out.push(Iface {
            name: p.name.clone(),
            kind,
            description: p.description.clone(),
            enabled: !p.shutdown,
            ip: p.ip,
            ipv6: p.ipv6.addresses.clone(),
            up: link && !p.shutdown,
            mac: Some(p.mac.to_string()),
            loopback: false,
        });
        for s in &p.subifs {
            out.push(Iface {
                name: format!("{}.{}", p.name, s.number),
                kind: "iana-if-type:l2vlan",
                description: s.description.clone(),
                enabled: !s.shutdown,
                ip: s.ip,
                ipv6: s.ipv6.addresses.clone(),
                up: link && !p.shutdown && !s.shutdown,
                mac: Some(p.mac.to_string()),
                loopback: false,
            });
        }
    }
    for l in &d.loopbacks {
        out.push(Iface {
            name: l.name(),
            kind: "iana-if-type:softwareLoopback",
            description: l.description.clone(),
            enabled: !l.shutdown,
            ip: l.ip,
            ipv6: l.ipv6.addresses.clone(),
            up: !l.shutdown,
            mac: None,
            loopback: true,
        });
    }
    out
}

fn iface_json(i: &Iface, config_only: bool) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), json!(i.name));
    if !i.description.is_empty() {
        m.insert("description".into(), json!(i.description));
    }
    m.insert("type".into(), json!(i.kind));
    m.insert("enabled".into(), json!(i.enabled));
    if let Some(ip) = i.ip {
        m.insert(format!("{IP}:ipv4"), json!({"address": [{"ip": ip.addr.to_string(), "netmask": ip.mask().to_string()}]}));
    }
    if !i.ipv6.is_empty() {
        let list: Vec<Value> = i.ipv6.iter().map(|n| json!({"ip": n.addr.to_string(), "prefix-length": n.prefix})).collect();
        m.insert(format!("{IP}:ipv6"), json!({ "address": list }));
    }
    if !config_only {
        m.insert("oper-status".into(), json!(if i.up { "up" } else { "down" }));
        if let Some(mac) = &i.mac {
            m.insert("phys-address".into(), json!(mac.to_lowercase()));
        }
    }
    Value::Object(m)
}

/// The static routes, grouped by prefix.
fn routes_json(d: &Device) -> Vec<Value> {
    let mut out: Vec<(Ipv4Net, Vec<Value>)> = Vec::new();
    for r in &d.routes {
        let mut hop = Map::new();
        match &r.via {
            NextHop::Address(a) => hop.insert("address".into(), json!(a.to_string())),
            NextHop::Interface(n) => hop.insert("interface".into(), json!(n)),
        };
        if let Some(dist) = r.distance {
            hop.insert("distance".into(), json!(dist));
        }
        match out.iter_mut().find(|(n, _)| *n == r.net) {
            Some((_, hops)) => hops.push(Value::Object(hop)),
            None => out.push((r.net, vec![Value::Object(hop)])),
        }
    }
    out.into_iter().map(|(n, hops)| json!({"prefix": n.to_string(), "next-hop": hops})).collect()
}

/// A module's top container, as `{"module:name": {...}}` members.
pub fn module_data(t: &Topology, d: &Device, module: &str, config_only: bool) -> Option<(String, Value)> {
    match module {
        SYS => Some((format!("{SYS}:system"), json!({"hostname": d.name}))),
        IF => {
            Some((format!("{IF}:interfaces"), json!({"interface": interfaces(t, d).iter().map(|i| iface_json(i, config_only)).collect::<Vec<_>>()})))
        }
        RT => Some((format!("{RT}:static-routes"), json!({"route": routes_json(d)}))),
        _ => None,
    }
}

// ---- paths ----

/// What a request path names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `/restconf/data`.
    Data,
    System,
    Hostname,
    Interfaces,
    Interface(String),
    /// A leaf or the `ietf-ip:ipv4` container of an interface.
    IfChild(String, &'static str),
    Routes,
    Route(String),
}

const IF_CHILDREN: [&str; 8] = ["name", "description", "type", "enabled", "oper-status", "phys-address", "ietf-ip:ipv4", "ietf-ip:ipv6"];

/// Parses the part of a path after `/restconf/data` (`/ietf-interfaces:interfaces/…`).
pub fn parse_target(rest: &str) -> ApiResult<Target> {
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() {
        return Ok(Target::Data);
    }
    let mut segs = Vec::new();
    for raw in rest.split('/') {
        if segs.len() >= 8 {
            return Err(err(501, "operation-not-supported", "this server's data is at most four levels deep"));
        }
        let (name, key) = match raw.split_once('=') {
            Some((n, k)) => (n, Some(http::percent_decode(k).ok_or_else(|| invalid(format!("`{raw}`: a broken %-escape")))?)),
            None => (raw, None),
        };
        let name = http::percent_decode(name).ok_or_else(|| invalid(format!("`{raw}`: a broken %-escape")))?;
        segs.push((name, key));
    }
    let Some((first, first_key)) = segs.first() else { return Ok(Target::Data) };
    let Some((module, top)) = first.split_once(':') else {
        return Err(err(
            400,
            "unknown-element",
            format!(
                "`{first}`: the first name carries its module, such as ietf-interfaces:interfaces (modules here: ietf-system, ietf-interfaces, netlabcraft-routing)"
            ),
        ));
    };
    if first_key.is_some() {
        return Err(invalid(format!("`{first}` is a container, not a list: it takes no `=key`")));
    }
    // Later names may repeat their parent's module prefix.
    let local = |n: &str, m: &str| -> String { n.strip_prefix(&format!("{m}:")).unwrap_or(n).to_string() };
    let deeper = |what: &str| err(501, "operation-not-supported", format!("this server's data stops at {what}"));
    match (module, top) {
        (SYS, "system") => match segs.get(1) {
            None => Ok(Target::System),
            Some((n, None)) if local(n, SYS) == "hostname" && segs.len() == 2 => Ok(Target::Hostname),
            Some((n, _)) if local(n, SYS) == "hostname" => Err(deeper("hostname")),
            Some((n, _)) => Err(missing(format!("ietf-system:system has no `{n}` here (only hostname)"))),
        },
        (IF, "interfaces") => match segs.get(1) {
            None => Ok(Target::Interfaces),
            Some((n, key)) if local(n, IF) == "interface" => {
                let key = key.clone().ok_or_else(|| invalid("`interface` is a list: name the entry, interface=GigabitEthernet0%2F0"))?;
                let Some((child, ckey)) = segs.get(2) else { return Ok(Target::Interface(key)) };
                if segs.len() > 3 {
                    return Err(deeper("ietf-ip:ipv4, ietf-ip:ipv6 and the interface's leaves"));
                }
                if ckey.is_some() {
                    return Err(invalid(format!("`{child}` takes no `=key`")));
                }
                let c = local(child, IF);
                if c == "ipv4" || c == "ipv6" {
                    return Err(err(400, "unknown-element", format!("{c} comes from another module: write ietf-ip:{c}")));
                }
                match IF_CHILDREN.iter().find(|x| **x == c) {
                    Some(x) => Ok(Target::IfChild(key, x)),
                    None => Err(missing(format!("an interface has no `{child}` here ({})", IF_CHILDREN.join(", ")))),
                }
            }
            Some((n, _)) => Err(missing(format!("ietf-interfaces:interfaces has no `{n}` (only interface=NAME)"))),
        },
        (RT, "static-routes") => match segs.get(1) {
            None => Ok(Target::Routes),
            Some((n, key)) if local(n, RT) == "route" => {
                let key = key.clone().ok_or_else(|| invalid("`route` is a list: name the entry, route=10.1.0.0%2F16"))?;
                if segs.len() > 2 {
                    return Err(deeper("a whole route"));
                }
                Ok(Target::Route(key))
            }
            Some((n, _)) => Err(missing(format!("netlabcraft-routing:static-routes has no `{n}` (only route=PREFIX)"))),
        },
        _ => {
            Err(missing(format!("no data named `{first}` (here: ietf-system:system, ietf-interfaces:interfaces, netlabcraft-routing:static-routes)")))
        }
    }
}

/// An interface name as the router spells it, if it has (or may create) that interface.
fn canonical(t: &Topology, d: &Device, name: &str) -> Option<(String, bool)> {
    let list = interfaces(t, d);
    if let Some(i) = list.iter().find(|i| i.name.eq_ignore_ascii_case(name)) {
        return Some((i.name.clone(), true));
    }
    if let Some(n) = netlabcraft_model::routing::loopback_number(name) {
        let full = format!("Loopback{n}");
        let exists = list.iter().any(|i| i.name == full);
        return Some((full, exists));
    }
    let parsed = ifname::parse(name)?;
    let full = parsed.to_string();
    list.iter().find(|i| i.name == full).map(|i| (i.name.clone(), true))
}

fn find_iface(t: &Topology, d: &Device, name: &str) -> ApiResult<Iface> {
    let (full, exists) = canonical(t, d, name).ok_or_else(|| missing(format!("{} has no interface {name}", d.name)))?;
    if !exists {
        return Err(missing(format!("{} has no interface {full}", d.name)));
    }
    interfaces(t, d).into_iter().find(|i| i.name == full).ok_or_else(|| missing(format!("{} has no interface {full}", d.name)))
}

/// The GET answer for a target.
pub fn get(t: &Topology, d: &Device, target: &Target, config_only: bool) -> ApiResult<Value> {
    Ok(match target {
        Target::Data => {
            let mut m = Map::new();
            for (module, _) in MODULES {
                if let Some((k, v)) = module_data(t, d, module, config_only) {
                    m.insert(k, v);
                }
            }
            json!({"ietf-restconf:data": m})
        }
        Target::System => json!({"ietf-system:system": {"hostname": d.name}}),
        Target::Hostname => json!({"ietf-system:hostname": d.name}),
        Target::Interfaces => {
            let (k, v) = module_data(t, d, IF, config_only).unwrap_or_default();
            json!({ k: v })
        }
        Target::Interface(n) => json!({"ietf-interfaces:interface": [iface_json(&find_iface(t, d, n)?, config_only)]}),
        Target::IfChild(n, c) => {
            let i = find_iface(t, d, n)?;
            let full = iface_json(&i, config_only);
            let v = full.get(*c).cloned().ok_or_else(|| missing(format!("{} has no {c} value", i.name)))?;
            if c.contains(':') { json!({ *c: v }) } else { json!({ format!("{IF}:{c}"): v }) }
        }
        Target::Routes => json!({"netlabcraft-routing:static-routes": {"route": routes_json(d)}}),
        Target::Route(p) => {
            let net = prefix(p)?;
            let r: Vec<Value> =
                routes_json(d).into_iter().filter(|r| r.get("prefix").and_then(Value::as_str) == Some(net.to_string().as_str())).collect();
            if r.is_empty() {
                return Err(missing(format!("no static route for {net}")));
            }
            json!({"netlabcraft-routing:route": r})
        }
    })
}

// ---- edits ----

/// How an edit combines with what is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// PUT: the target becomes exactly the body.
    Replace,
    /// PATCH: the body's values are merged in.
    Merge,
    /// POST: a new entry; it must not exist yet.
    Create,
    Delete,
}

fn text(v: &Value, what: &str) -> ApiResult<String> {
    let s = v.as_str().ok_or_else(|| invalid(format!("`{what}` is text")))?;
    if s.chars().any(char::is_control) {
        return Err(invalid(format!("`{what}` may not hold line breaks or control characters")));
    }
    if s.chars().count() > MAX_TEXT {
        return Err(invalid(format!("`{what}` is longer than {MAX_TEXT} characters")));
    }
    Ok(s.to_string())
}

fn prefix(p: &str) -> ApiResult<Ipv4Net> {
    let net: Ipv4Net = p.trim().parse().map_err(|_| invalid(format!("`{p}` is not a prefix such as 10.1.0.0/16")))?;
    if net.network() != net {
        return Err(invalid(format!("{net} has host bits set (the network is {})", net.network())));
    }
    Ok(net)
}

/// One list entry or a one-entry array (RFC 7951 encodes list entries as arrays).
fn entries<'a>(v: &'a Value, what: &str) -> ApiResult<Vec<&'a Map<String, Value>>> {
    let list: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![v],
        _ => return Err(invalid(format!("`{what}` is an object or a list of objects"))),
    };
    if list.len() > MAX_ENTRIES {
        return Err(err(413, "too-big", format!("at most {MAX_ENTRIES} entries per request")));
    }
    list.into_iter().map(|e| e.as_object().ok_or_else(|| invalid(format!("each `{what}` entry is an object")))).collect()
}

/// The single top member of a body, which must be `want` (`module:name`).
fn top<'a>(body: &'a Value, want: &str) -> ApiResult<&'a Value> {
    let m = body.as_object().ok_or_else(|| invalid("the body is a JSON object"))?;
    if m.len() != 1 {
        return Err(err(400, "malformed-message", format!("the body holds one member, \"{want}\"")));
    }
    let (k, v) = m.iter().next().ok_or_else(|| invalid("the body is empty"))?;
    if k != want {
        let hint = if !k.contains(':') && want.ends_with(&format!(":{k}")) { " (with its module prefix)" } else { "" };
        return Err(err(400, "unknown-element", format!("the body's member is \"{want}\"{hint}, not \"{k}\"")));
    }
    Ok(v)
}

/// The `ip address` line (or `no ip address`) of an `ietf-ip:ipv4` value.
fn ipv4_line(v: &Value) -> ApiResult<String> {
    let m = v.as_object().ok_or_else(|| invalid("`ietf-ip:ipv4` is an object"))?;
    for k in m.keys() {
        if k != "address" {
            return Err(err(400, "unknown-element", format!("`ietf-ip:ipv4` here holds only `address`, not `{k}`")));
        }
    }
    let Some(a) = m.get("address") else { return Ok(" no ip address".into()) };
    let list = entries(a, "address")?;
    match list.as_slice() {
        [] => Ok(" no ip address".into()),
        [one] => {
            for k in one.keys() {
                if !["ip", "netmask", "prefix-length"].contains(&k.as_str()) {
                    return Err(err(400, "unknown-element", format!("an address holds ip and netmask (or prefix-length), not `{k}`")));
                }
            }
            let ip = parse_ip(&text(one.get("ip").ok_or_else(|| err(400, "missing-element", "an address needs `ip`"))?, "ip")?)
                .map_err(|e| invalid(e.to_string()))?;
            let mask = match (one.get("netmask"), one.get("prefix-length")) {
                (Some(m), None) => parse_ip(&text(m, "netmask")?).map_err(|e| invalid(e.to_string()))?,
                (None, Some(p)) => {
                    let n = p.as_u64().filter(|n| *n <= 32).ok_or_else(|| invalid("`prefix-length` is a number from 0 to 32"))?;
                    Ipv4Net::new(ip, n as u8).mask()
                }
                (Some(_), Some(_)) => return Err(invalid("give netmask or prefix-length, not both")),
                (None, None) => return Err(err(400, "missing-element", "an address needs `netmask` or `prefix-length`")),
            };
            Ok(format!(" ip address {ip} {mask}"))
        }
        _ => Err(invalid("one address per interface here")),
    }
}

/// Most IPv6 addresses one interface takes here.
const MAX_V6: usize = 8;

/// The addresses of an `ietf-ip:ipv6` value (RFC 8344 §3: `address` entries of `ip` and
/// `prefix-length`).
fn ipv6_addresses(v: &Value) -> ApiResult<Vec<Ipv6Net>> {
    let m = v.as_object().ok_or_else(|| invalid("`ietf-ip:ipv6` is an object"))?;
    for k in m.keys() {
        if k != "address" {
            return Err(err(400, "unknown-element", format!("`ietf-ip:ipv6` here holds only `address`, not `{k}`")));
        }
    }
    let Some(a) = m.get("address") else { return Ok(Vec::new()) };
    let list = entries(a, "address")?;
    if list.len() > MAX_V6 {
        return Err(invalid(format!("at most {MAX_V6} IPv6 addresses per interface")));
    }
    let mut out: Vec<Ipv6Net> = Vec::new();
    for one in list {
        for k in one.keys() {
            if !["ip", "prefix-length"].contains(&k.as_str()) {
                return Err(err(400, "unknown-element", format!("an IPv6 address holds ip and prefix-length, not `{k}`")));
            }
        }
        let ip: std::net::Ipv6Addr = text(one.get("ip").ok_or_else(|| err(400, "missing-element", "an address needs `ip`"))?, "ip")?
            .parse()
            .map_err(|_| invalid("`ip` is an IPv6 address such as 2001:db8::1"))?;
        let len = one
            .get("prefix-length")
            .ok_or_else(|| err(400, "missing-element", "an IPv6 address needs `prefix-length`"))?
            .as_u64()
            .filter(|n| (1..=128).contains(n))
            .ok_or_else(|| invalid("`prefix-length` is a number from 1 to 128"))?;
        if ip.is_multicast() || ip.is_unspecified() || ip.is_loopback() || Ipv6Net::is_link_local(ip) {
            return Err(invalid(format!("{ip} can't be an interface's global address")));
        }
        let n = Ipv6Net::new(ip, len as u8);
        if !out.contains(&n) {
            out.push(n);
        }
    }
    Ok(out)
}

/// The lines that give an interface these IPv6 addresses: all of them (`replace`, dropping the
/// others) or in addition to what it has.
fn ipv6_lines(have: &[Ipv6Net], want: &[Ipv6Net], replace: bool, lines: &mut Vec<String>) {
    if replace {
        for n in have.iter().filter(|n| !want.contains(n)) {
            lines.push(format!(" no ipv6 address {n}"));
        }
    }
    for n in want.iter().filter(|n| !have.contains(n)) {
        lines.push(format!(" ipv6 address {n}"));
    }
}

/// The lines that make an interface match an entry (`Replace`) or take its values (`Merge`).
fn iface_lines(t: &Topology, d: &Device, key: Option<&str>, entry: &Map<String, Value>, op: Op, lines: &mut Vec<String>) -> ApiResult<bool> {
    let body_name = entry.get("name").map(|v| text(v, "name")).transpose()?;
    let name = match (key, body_name.as_deref()) {
        (Some(k), Some(b)) => {
            let same = canonical(t, d, k).zip(canonical(t, d, b)).is_some_and(|(x, y)| x.0 == y.0);
            if !same {
                return Err(invalid(format!("the body names {b} but the path names {k}")));
            }
            k.to_string()
        }
        (Some(k), None) => k.to_string(),
        (None, Some(b)) => b.to_string(),
        (None, None) => return Err(err(400, "missing-element", "an interface entry needs its `name`")),
    };
    let (full, exists) = canonical(t, d, &name).ok_or_else(|| missing(format!("{} has no interface {name}", d.name)))?;
    let is_loop = full.starts_with("Loopback");
    if !exists && !is_loop {
        return Err(missing(format!("{} has no interface {full} (only loopbacks can be created)", d.name)));
    }
    if exists && op == Op::Create {
        return Err(err(409, "data-exists", format!("{full} already exists")));
    }
    for k in entry.keys() {
        match k.as_str() {
            "name" | "description" | "type" | "enabled" => {}
            "ietf-ip:ipv4" | "ietf-ip:ipv6" => {}
            "oper-status" | "phys-address" => return Err(invalid(format!("`{k}` is state data: it can be read, not written"))),
            "ipv4" | "ipv6" => return Err(err(400, "unknown-element", format!("{k} comes from another module: write ietf-ip:{k}"))),
            other => return Err(err(400, "unknown-element", format!("an interface here has no `{other}`"))),
        }
    }
    if let Some(ty) = entry.get("type") {
        let ty = text(ty, "type")?;
        let want = if is_loop { "iana-if-type:softwareLoopback".to_string() } else { find_iface(t, d, &full)?.kind.to_string() };
        if ty != want {
            return Err(invalid(format!("{full} is of type {want}; the type can't change")));
        }
    }
    let replace = op != Op::Merge;
    lines.push(format!("interface {full}"));
    match entry.get("description") {
        Some(v) => {
            let s = text(v, "description")?;
            lines.push(if s.is_empty() { " no description".into() } else { format!(" description {s}") });
        }
        None if replace && exists => lines.push(" no description".into()),
        None => {}
    }
    match entry.get(&format!("{IP}:ipv4")) {
        Some(v) => lines.push(ipv4_line(v)?),
        None if replace && exists => lines.push(" no ip address".into()),
        None => {}
    }
    let have = if exists { find_iface(t, d, &full)?.ipv6 } else { Vec::new() };
    match entry.get(&format!("{IP}:ipv6")) {
        Some(v) => ipv6_lines(&have, &ipv6_addresses(v)?, replace, lines),
        None if replace => ipv6_lines(&have, &[], true, lines),
        None => {}
    }
    match entry.get("enabled") {
        Some(Value::Bool(b)) => lines.push(if *b { " no shutdown".into() } else { " shutdown".into() }),
        Some(_) => return Err(invalid("`enabled` is true or false")),
        None if replace => lines.push(" no shutdown".into()),
        None => {}
    }
    lines.push("exit".into());
    Ok(!exists)
}

/// The lines of one route entry.
fn route_lines(entry: &Map<String, Value>, key: Option<&str>, lines: &mut Vec<String>) -> ApiResult<Ipv4Net> {
    for k in entry.keys() {
        if !["prefix", "next-hop"].contains(&k.as_str()) {
            return Err(err(400, "unknown-element", format!("a route holds prefix and next-hop, not `{k}`")));
        }
    }
    let p = match (entry.get("prefix"), key) {
        (Some(v), Some(k)) => {
            let (a, b) = (prefix(&text(v, "prefix")?)?, prefix(k)?);
            if a != b {
                return Err(invalid(format!("the body names {a} but the path names {b}")));
            }
            a
        }
        (Some(v), None) => prefix(&text(v, "prefix")?)?,
        (None, Some(k)) => prefix(k)?,
        (None, None) => return Err(err(400, "missing-element", "a route needs its `prefix`")),
    };
    let hops = entries(entry.get("next-hop").ok_or_else(|| err(400, "missing-element", "a route needs a `next-hop`"))?, "next-hop")?;
    if hops.is_empty() {
        return Err(err(400, "missing-element", "a route needs a `next-hop`"));
    }
    for h in hops {
        let mut via = None;
        let mut dist = String::new();
        for (k, v) in h {
            match k.as_str() {
                "address" => via = Some(parse_ip(&text(v, "address")?).map_err(|e| invalid(e.to_string()))?.to_string()),
                "interface" => {
                    let n = text(v, "interface")?;
                    if n.contains(char::is_whitespace) || n.is_empty() {
                        return Err(invalid("an interface name has no spaces"));
                    }
                    via = Some(n);
                }
                "distance" => dist = format!(" {}", v.as_u64().filter(|n| (1..=255).contains(n)).ok_or_else(|| invalid("`distance` is 1-255"))?),
                other => return Err(err(400, "unknown-element", format!("a next hop holds address or interface (and distance), not `{other}`"))),
            }
        }
        let via = via.ok_or_else(|| err(400, "missing-element", "a next hop needs `address` or `interface`"))?;
        lines.push(format!("ip route {} {} {via}{dist}", p.addr, p.mask()));
    }
    Ok(p)
}

fn remove_routes(d: &Device, net: Ipv4Net, lines: &mut Vec<String>) -> bool {
    let any = d.routes.iter().any(|r| r.net == net);
    if any {
        lines.push(format!("no ip route {} {}", net.addr, net.mask()));
    }
    any
}

/// What an edit does: the configuration lines, and whether it created the target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    pub lines: Vec<String>,
    pub created: bool,
}

/// Turns an edit of a target into configuration lines (not yet checked or applied).
pub fn edit(t: &Topology, d: &Device, target: &Target, op: Op, body: Option<&Value>) -> ApiResult<Edit> {
    let mut e = Edit::default();
    let need = || body.ok_or_else(|| err(400, "malformed-message", "this request needs a JSON body"));
    let not_here = |what: &str| err(405, "operation-not-supported", what.to_string());
    match (target, op) {
        (Target::Data, _) => return Err(not_here("edit one module's data at a time (such as /restconf/data/ietf-interfaces:interfaces)")),
        (Target::System | Target::Hostname, Op::Delete | Op::Create) => return Err(not_here("the hostname can be replaced, not created or deleted")),
        (Target::System, _) => {
            let v = top(need()?, "ietf-system:system")?;
            let m = v.as_object().ok_or_else(|| invalid("ietf-system:system is an object"))?;
            for k in m.keys() {
                if k != "hostname" {
                    return Err(err(400, "unknown-element", format!("ietf-system:system here holds only hostname, not `{k}`")));
                }
            }
            match m.get("hostname") {
                Some(h) => e.lines.push(format!("hostname {}", text(h, "hostname")?)),
                None if op == Op::Replace => return Err(err(400, "missing-element", "the system needs a hostname")),
                None => {}
            }
        }
        (Target::Hostname, _) => e.lines.push(format!("hostname {}", text(top(need()?, "ietf-system:hostname")?, "hostname")?)),
        (Target::Interfaces, Op::Replace) => return Err(not_here("replace one interface at a time: PUT …/interface=NAME")),
        (Target::Interfaces, Op::Delete) => return Err(not_here("delete one interface at a time")),
        (Target::Interfaces, Op::Create) => {
            for entry in entries(top(need()?, "ietf-interfaces:interface")?, "interface")? {
                e.created |= iface_lines(t, d, None, entry, Op::Create, &mut e.lines)?;
            }
        }
        (Target::Interfaces, Op::Merge) => {
            let v = top(need()?, "ietf-interfaces:interfaces")?;
            let list = v.get("interface").ok_or_else(|| err(400, "missing-element", "ietf-interfaces:interfaces holds an `interface` list"))?;
            for entry in entries(list, "interface")? {
                iface_lines(t, d, None, entry, Op::Merge, &mut e.lines)?;
            }
        }
        (Target::Interface(n), Op::Delete) => {
            let i = find_iface(t, d, n)?;
            if !i.loopback {
                return Err(not_here("only loopbacks can be deleted; change a port with PUT or PATCH"));
            }
            e.lines.push(format!("no interface {}", i.name));
        }
        (Target::Interface(_), Op::Create) => return Err(not_here("create interfaces by POST to …/ietf-interfaces:interfaces")),
        (Target::Interface(n), op) => {
            let list = entries(top(need()?, "ietf-interfaces:interface")?, "interface")?;
            let [entry] = list.as_slice() else { return Err(invalid("one interface entry, the one the path names")) };
            e.created = iface_lines(t, d, Some(n), entry, op, &mut e.lines)?;
        }
        (Target::IfChild(n, c), op) => {
            let i = find_iface(t, d, n)?;
            if matches!(*c, "name" | "type" | "oper-status" | "phys-address") {
                return Err(not_here(&format!("`{c}` can't be changed")));
            }
            if op == Op::Create {
                return Err(not_here("change a leaf with PUT, PATCH or DELETE"));
            }
            e.lines.push(format!("interface {}", i.name));
            match (*c, op) {
                ("description", Op::Delete) => {
                    if i.description.is_empty() {
                        return Err(missing(format!("{} has no description", i.name)));
                    }
                    e.lines.push(" no description".into());
                }
                ("description", _) => {
                    let s = text(top(need()?, "ietf-interfaces:description")?, "description")?;
                    e.lines.push(if s.is_empty() { " no description".into() } else { format!(" description {s}") });
                }
                ("enabled", Op::Delete) => e.lines.push(" no shutdown".into()),
                ("enabled", _) => match top(need()?, "ietf-interfaces:enabled")? {
                    Value::Bool(b) => e.lines.push(if *b { " no shutdown".into() } else { " shutdown".into() }),
                    _ => return Err(invalid("`enabled` is true or false")),
                },
                ("ietf-ip:ipv6", Op::Delete) => {
                    if i.ipv6.is_empty() {
                        return Err(missing(format!("{} has no IPv6 address", i.name)));
                    }
                    ipv6_lines(&i.ipv6, &[], true, &mut e.lines);
                }
                ("ietf-ip:ipv6", op) => ipv6_lines(&i.ipv6, &ipv6_addresses(top(need()?, "ietf-ip:ipv6")?)?, op == Op::Replace, &mut e.lines),
                (_, Op::Delete) => {
                    if i.ip.is_none() {
                        return Err(missing(format!("{} has no IPv4 address", i.name)));
                    }
                    e.lines.push(" no ip address".into());
                }
                _ => e.lines.push(ipv4_line(top(need()?, "ietf-ip:ipv4")?)?),
            }
            e.lines.push("exit".into());
        }
        (Target::Routes, Op::Delete) => {
            let mut nets: Vec<Ipv4Net> = d.routes.iter().map(|r| r.net).collect();
            nets.dedup();
            for n in nets {
                remove_routes(d, n, &mut e.lines);
            }
        }
        (Target::Routes, op) => {
            let list = if op == Op::Create {
                top(need()?, "netlabcraft-routing:route")?
            } else {
                let v = top(need()?, "netlabcraft-routing:static-routes")?;
                v.get("route").ok_or_else(|| err(400, "missing-element", "static-routes holds a `route` list"))?
            };
            let entries = entries(list, "route")?;
            if op == Op::Replace {
                let mut nets: Vec<Ipv4Net> = d.routes.iter().map(|r| r.net).collect();
                nets.dedup();
                for n in nets {
                    remove_routes(d, n, &mut e.lines);
                }
            }
            for entry in entries {
                let mut mine = Vec::new();
                let net = route_lines(entry, None, &mut mine)?;
                if op == Op::Create && d.routes.iter().any(|r| r.net == net) {
                    return Err(err(409, "data-exists", format!("a static route for {net} already exists")));
                }
                e.created |= !d.routes.iter().any(|r| r.net == net);
                e.lines.extend(mine);
            }
        }
        (Target::Route(p), Op::Delete) => {
            let net = prefix(p)?;
            if !remove_routes(d, net, &mut e.lines) {
                return Err(missing(format!("no static route for {net}")));
            }
        }
        (Target::Route(_), Op::Create) => return Err(not_here("create routes by POST to …/netlabcraft-routing:static-routes")),
        (Target::Route(p), op) => {
            let list = entries(top(need()?, "netlabcraft-routing:route")?, "route")?;
            let [entry] = list.as_slice() else { return Err(invalid("one route entry, the one the path names")) };
            let net = prefix(p)?;
            e.created = !d.routes.iter().any(|r| r.net == net);
            if op == Op::Replace {
                remove_routes(d, net, &mut e.lines);
            }
            route_lines(entry, Some(p), &mut e.lines)?;
        }
    }
    Ok(e)
}

/// Runs lines through the console grammar on a copy of the lab: the first failure, if any.
pub fn check(t: &Topology, dev: DeviceId, lines: &[String]) -> ApiResult<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let mut scratch = t.clone();
    let mut sim = netlabcraft_sim::Sim::default();
    let failed = netlabcraft_ios::apply_config(&mut scratch, &mut sim, dev, &lines.join("\n"));
    match failed.first() {
        Some(f) => Err(invalid(format!("the router refused `{}`", f.trim_start_matches('%').trim()))),
        None => Ok(()),
    }
}

// ---- the RESTCONF server ----

fn response(status: u16, content_type: Option<&str>, body: Vec<u8>) -> Response {
    let mut r = Response::new(status);
    r.headers.push(("Server".into(), "NetLabCraft".into()));
    if let Some(ct) = content_type {
        r.headers.push(("Content-Type".into(), ct.into()));
    }
    r.headers.push(("Cache-Control".into(), "no-cache".into()));
    r.headers.push(("Connection".into(), "close".into()));
    r.body = body;
    r
}

/// A data body in the encoding the client asked for.
fn data_response(status: u16, v: &Value, enc: Encoding) -> Response {
    match enc {
        Encoding::Json => {
            let mut text = serde_json::to_string_pretty(v).unwrap_or_default();
            text.push('\n');
            response(status, Some(YANG_JSON), text.into_bytes())
        }
        Encoding::Xml => {
            let mut text = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            if let Some(m) = v.as_object() {
                for (k, x) in m {
                    crate::netconf::to_xml(k, x, "", &mut text);
                }
            }
            text.push('\n');
            response(status, Some(YANG_XML), text.into_bytes())
        }
    }
}

/// An `ietf-restconf:errors` body (RFC 8040 §7.1), in JSON.
pub fn error_response(e: &ApiError) -> Response {
    error_in(e, Encoding::Json)
}

/// An `ietf-restconf:errors` body in the encoding the client asked for.
pub fn error_in(e: &ApiError, enc: Encoding) -> Response {
    let kind = if matches!(e.tag, "malformed-message" | "access-denied" | "operation-not-supported") { "protocol" } else { "application" };
    let mut r = data_response(
        e.status,
        &json!({"ietf-restconf:errors": {"error": [{"error-type": kind, "error-tag": e.tag, "error-message": e.message}]}}),
        enc,
    );
    if e.status == 401 {
        r.headers.insert(1, ("WWW-Authenticate".into(), "Basic realm=\"restconf\"".into()));
    }
    r
}

/// Who is asking: a local user with privilege 15.
fn authorize(d: &Device, req: &Request) -> ApiResult<String> {
    let Some(h) = req.header("authorization") else {
        return Err(err(401, "access-denied", "log in: send an Authorization: Basic header with a local username and password"));
    };
    let (user, pw) = http::basic_credentials(h).ok_or_else(|| err(401, "access-denied", "the Authorization header is not Basic user:password"))?;
    let u =
        d.access.users.iter().find(|u| u.name == user && u.matches(&pw)).ok_or_else(|| err(401, "access-denied", "wrong username or password"))?;
    if u.privilege < 15 {
        return Err(err(403, "access-denied", format!("{user} has privilege {}: the API needs privilege 15", u.privilege)));
    }
    Ok(user)
}

const ALLOW: &str = "GET, HEAD, OPTIONS, PUT, PATCH, POST, DELETE";

/// The router's web server: RESTCONF under `/restconf`, the root discovery document, and
/// nothing else (no web pages).
pub fn restconf(t: &Topology, dev: DeviceId, req: &Request) -> Served {
    let Some(d) = t.device(dev) else {
        return Served { response: error_response(&err(500, "operation-failed", "the router is gone")), lines: Vec::new() };
    };
    let enc = match negotiate(req.header("accept")) {
        Ok(e) => e,
        Err(e) => return Served { response: error_response(&e), lines: Vec::new() },
    };
    match serve(t, d, req, enc) {
        Ok((r, lines)) => Served { response: r, lines },
        Err(e) => Served { response: error_in(&e, enc), lines: Vec::new() },
    }
}

/// How a body is encoded (RFC 8040 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Json,
    Xml,
}

/// The encoding an `Accept` header asks for (RFC 9110 §12.5.1): the supported media range with
/// the highest quality, the first listed on a tie; JSON when the header is absent or says `*/*`.
pub fn negotiate(accept: Option<&str>) -> ApiResult<Encoding> {
    let Some(accept) = accept.map(str::trim).filter(|a| !a.is_empty()) else { return Ok(Encoding::Json) };
    let mut best: Option<(u16, Encoding)> = None;
    for range in accept.split(',').take(32) {
        let mut parts = range.split(';');
        let media = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let mut q = 1000u16;
        for p in parts {
            if let Some(v) = p.trim().strip_prefix("q=") {
                q = v.trim().parse::<f32>().ok().filter(|x| (0.0..=1.0).contains(x)).map_or(0, |x| (x * 1000.0) as u16);
            }
        }
        let enc = match media.as_str() {
            YANG_JSON | "application/json" | "*/*" | "application/*" => Encoding::Json,
            YANG_XML | "application/xml" | "text/xml" => Encoding::Xml,
            _ => continue,
        };
        if q > 0 && best.is_none_or(|(b, _)| q > b) {
            best = Some((q, enc));
        }
    }
    let shown: String = accept.chars().take(80).collect();
    best.map(|(_, e)| e).ok_or_else(|| err(406, "invalid-value", format!("this server answers {YANG_JSON} or {YANG_XML}, not {shown}")))
}

/// List names: their entries become JSON arrays (RFC 7951 §5.4).
const LISTS: [(&str, &str); 5] =
    [("interfaces", "interface"), ("ipv4", "address"), ("ipv6", "address"), ("static-routes", "route"), ("route", "next-hop")];
/// Containers: an empty element is an empty object, not an empty text.
const CONTAINERS: [&str; 7] = ["system", "interfaces", "static-routes", "ipv4", "ipv6", "data", "restconf"];

type XmlEl = netlabcraft_wire::xml::Element;

/// A member name for an element: with its module prefix where the namespace changes.
fn xml_key(e: &XmlEl, parent: Option<&str>) -> ApiResult<String> {
    match e.ns.as_deref() {
        Some(ns) if Some(ns) == parent => Ok(e.name.clone()),
        Some(ns) => match module_of_ns(ns) {
            Some(m) => Ok(format!("{m}:{}", e.name)),
            None => Err(err(
                400,
                "unknown-element",
                format!("<{}>: no module here has the namespace {}", e.name, ns.chars().take(80).collect::<String>()),
            )),
        },
        None => Ok(e.name.clone()),
    }
}

fn xml_value(e: &XmlEl, depth: usize) -> ApiResult<Value> {
    if depth > 16 {
        return Err(err(400, "malformed-message", "the body is nested too deeply"));
    }
    if e.children.is_empty() {
        if e.text.is_empty() && CONTAINERS.contains(&e.name.as_str()) {
            return Ok(json!({}));
        }
        return Ok(match (e.name.as_str(), e.text.as_str()) {
            ("enabled", "true") => json!(true),
            ("enabled", "false") => json!(false),
            ("prefix-length" | "distance", t) => t.parse::<u64>().map_or_else(|_| json!(t), |n| json!(n)),
            ("type", t) if !t.is_empty() => json!(format!("iana-if-type:{}", t.rsplit(':').next().unwrap_or(t))),
            (_, t) => json!(t),
        });
    }
    if !e.text.is_empty() {
        return Err(err(400, "malformed-message", format!("<{}> holds both text and elements", e.name)));
    }
    let mut m = Map::new();
    for c in &e.children {
        let k = xml_key(c, e.ns.as_deref())?;
        let v = xml_value(c, depth + 1)?;
        if LISTS.contains(&(e.name.as_str(), c.name.as_str())) {
            match m.entry(k).or_insert_with(|| Value::Array(Vec::new())) {
                Value::Array(a) => a.push(v),
                _ => return Err(err(400, "malformed-message", format!("<{}> is a list", c.name))),
            }
        } else if m.contains_key(&k) {
            return Err(err(400, "malformed-message", format!("<{}> appears twice", c.name)));
        } else {
            m.insert(k, v);
        }
    }
    Ok(Value::Object(m))
}

/// An XML body (RFC 8040 §5.2, RFC 7950 §7) as the JSON value the edits read: members take
/// their module prefix where the namespace changes, lists become arrays, booleans and numbers
/// their JSON types, and `iana-if-type` identities their module prefix.
pub fn xml_to_json(el: &XmlEl) -> ApiResult<Value> {
    let k = xml_key(el, None)?;
    let v = xml_value(el, 0)?;
    // A list entry as the resource itself is a one-entry array (RFC 7951 §5.4).
    if LISTS.iter().any(|(_, l)| *l == el.name && *l != "address" && *l != "next-hop") { Ok(json!({ k: [v] })) } else { Ok(json!({ k: v })) }
}

fn serve(t: &Topology, d: &Device, req: &Request, enc: Encoding) -> ApiResult<(Response, Vec<String>)> {
    let path = req.path();
    // RFC 8040 §3.1: root discovery through RFC 6415 host-meta.
    if path == "/.well-known/host-meta" {
        let xrd = "<XRD xmlns='http://docs.oasis-open.org/ns/xri/xrd-1.0'>\n  <Link rel='restconf' href='/restconf'/>\n</XRD>\n";
        return Ok((response(200, Some("application/xrd+xml"), xrd.as_bytes().to_vec()), Vec::new()));
    }
    let Some(rest) = path.strip_prefix("/restconf").filter(|r| r.is_empty() || r.starts_with('/')) else {
        return Err(missing(format!("{path}: this router has no web pages; RESTCONF lives under /restconf")));
    };
    if !d.automation.restconf {
        return Err(missing("RESTCONF is off on this router (configure `restconf`)"));
    }
    if !matches!(req.method.as_str(), "GET" | "HEAD" | "OPTIONS" | "PUT" | "PATCH" | "POST" | "DELETE") {
        let mut r = error_in(&err(405, "operation-not-supported", format!("{} is not a RESTCONF method", req.method)), enc);
        r.headers.push(("Allow".into(), ALLOW.into()));
        return Ok((r, Vec::new()));
    }
    authorize(d, req)?;
    let mut config_only = false;
    for (k, v) in req.query() {
        match (k.as_str(), v.as_str()) {
            ("content", "config") => config_only = true,
            ("content", "all" | "nonconfig") => {}
            _ => return Err(invalid(format!("the query parameter `{k}={v}` is not supported here (content=config|all is)"))),
        }
    }
    if req.method == "OPTIONS" {
        let mut r = response(200, None, Vec::new());
        r.headers.push(("Allow".into(), ALLOW.into()));
        return Ok((r, Vec::new()));
    }
    let rest = rest.trim_end_matches('/');
    let data = match rest {
        "" => Some(json!({"ietf-restconf:restconf": {"data": {}, "operations": {}, "yang-library-version": "2019-01-04"}})),
        "/operations" => Some(json!({"ietf-restconf:operations": {}})),
        "/yang-library-version" => Some(json!({"ietf-restconf:yang-library-version": "2019-01-04"})),
        _ => None,
    };
    if let Some(v) = data {
        if !matches!(req.method.as_str(), "GET" | "HEAD") {
            return Err(err(405, "operation-not-supported", "this resource is read-only"));
        }
        let mut r = data_response(200, &v, enc);
        if req.method == "HEAD" {
            r.body.clear();
        }
        return Ok((r, Vec::new()));
    }
    let Some(rest) = rest.strip_prefix("/data") else {
        return Err(missing(format!("{path}: RESTCONF resources are /restconf/data, /restconf/operations and /restconf/yang-library-version")));
    };
    if !rest.is_empty() && !rest.starts_with('/') {
        return Err(missing(format!("{path}: no such resource")));
    }
    let target = parse_target(rest)?;
    let op = match req.method.as_str() {
        "GET" | "HEAD" => {
            let mut r = data_response(200, &get(t, d, &target, config_only)?, enc);
            if req.method == "HEAD" {
                r.body.clear();
            }
            return Ok((r, Vec::new()));
        }
        "PUT" => Op::Replace,
        "PATCH" => Op::Merge,
        "POST" => Op::Create,
        _ => Op::Delete,
    };
    let xml_body = match req.header("content-type").map(str::to_ascii_lowercase) {
        _ if req.body.is_empty() => false,
        None => false,
        Some(ct) if ct.contains("json") => false,
        Some(ct) if ct.contains("xml") => true,
        Some(ct) => {
            return Err(err(
                415,
                "invalid-value",
                format!("this server reads {YANG_JSON} or {YANG_XML}, not {}", ct.chars().take(80).collect::<String>()),
            ));
        }
    };
    let body: Option<Value> = if req.body.is_empty() {
        None
    } else if xml_body {
        let text = std::str::from_utf8(&req.body).map_err(|_| err(400, "malformed-message", "the XML body is not UTF-8"))?;
        let el = netlabcraft_wire::xml::parse(text).map_err(|e| err(400, "malformed-message", format!("the body is not valid XML: {e}")))?;
        Some(xml_to_json(&el)?)
    } else {
        Some(serde_json::from_slice(&req.body).map_err(|e| err(400, "malformed-message", format!("the body is not valid JSON: {e}")))?)
    };
    let e = edit(t, d, &target, op, body.as_ref())?;
    check(t, d.id, &e.lines)?;
    let mut r = response(if e.created { 201 } else { 204 }, None, Vec::new());
    if e.created {
        r.headers.push(("Location".into(), path.to_string()));
    }
    Ok((r, e.lines))
}

// ---- engine side: applying what servers accepted, and the PC's requests ----

/// Applies the edits servers accepted to the lab, through the console grammar. Returns what
/// was applied: (router, lines, failures).
pub(crate) fn apply_changes(s: &mut Session) -> Vec<Value> {
    let changes = s.sim.take_automation_changes();
    let mut out = Vec::new();
    for (dev, lines) in changes {
        let mut t = s.topology.clone();
        let failed = netlabcraft_ios::apply_config(&mut t, &mut s.sim, dev, &lines.join("\n"));
        s.checkpoint();
        s.topology = t;
        s.changed();
        let name = s.topology.device(dev).map_or(String::new(), |d| d.name.clone());
        out.push(json!({"device": name, "lines": lines, "failed": failed}));
    }
    out
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// A URL split into scheme, host, port and path.
pub fn split_url(url: &str) -> Result<(bool, String, u16, String)> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let (tls, rest) = if lower.starts_with("https://") {
        (true, url.get(8..).unwrap_or(""))
    } else if lower.starts_with("http://") {
        (false, url.get(7..).unwrap_or(""))
    } else {
        return Err(bad("the URL starts with http:// or https://"));
    };
    let (hostport, path) = match rest.find(['/', '?']) {
        Some(i) => (rest.get(..i).unwrap_or(""), rest.get(i..).unwrap_or("/").to_string()),
        None => (rest, "/".to_string()),
    };
    let path = if path.starts_with('?') { format!("/{path}") } else { path };
    if path.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("the URL's path has spaces: write them as %20"));
    }
    // An IPv6 address is written in brackets (RFC 3986 §3.2.2): `https://[2001:db8::1]:443/`.
    let (host, after) = match hostport.strip_prefix('[') {
        Some(r) => {
            let (inside, after) = r.split_once(']').ok_or_else(|| bad("an IPv6 address in a URL ends with ]"))?;
            inside.parse::<std::net::Ipv6Addr>().map_err(|_| bad(format!("`{inside}` is not an IPv6 address")))?;
            (hostport.get(..inside.len() + 2).unwrap_or(""), after)
        }
        None => match hostport.rsplit_once(':') {
            Some((h, _)) => (h, hostport.get(h.len()..).unwrap_or("")),
            None => (hostport, ""),
        },
    };
    let port = match after.strip_prefix(':') {
        Some(p) => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| bad(format!("`{p}` is not a port number")))?,
        None if after.is_empty() => {
            if tls {
                http::TLS_PORT
            } else {
                http::PORT
            }
        }
        None => return Err(bad("after the host comes :port or the path")),
    };
    if host.is_empty() || host.len() > 253 || host.contains(['@', ' ']) {
        return Err(bad("the URL needs a host: an address or a name"));
    }
    if !host.starts_with('[') && host.contains(':') {
        return Err(bad("write an IPv6 address in brackets, such as https://[2001:db8::1]/restconf"));
    }
    Ok((tls, host.to_string(), port, path))
}

/// The address of a host given as an IPv4 or IPv6 address (brackets allowed) or a DNS name.
pub(crate) fn resolve(s: &mut Session, d: DeviceId, host: &str) -> Result<std::net::IpAddr> {
    let h = host.trim();
    let bare = h.strip_prefix('[').and_then(|x| x.strip_suffix(']')).unwrap_or(h);
    if let Ok(a) = bare.parse::<std::net::IpAddr>() {
        return Ok(a);
    }
    s.sim.dns_resolve(&s.topology, d, bare).map(std::net::IpAddr::V4).map_err(|e| EngineError::Failed(e.to_string()))
}

/// Header pairs from `{"Name": "value"}` or `[["Name", "value"]]`.
fn header_param(p: &Value) -> Result<Vec<(String, String)>> {
    let pairs: Vec<(String, String)> = match p.get("headers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x.as_array().map(Vec::as_slice) {
                Some([Value::String(k), Value::String(v)]) => Ok((k.clone(), v.clone())),
                _ => Err(bad("each header is [\"Name\", \"value\"]")),
            })
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad("`headers` is {\"Name\": \"value\"} or [[\"Name\", \"value\"]]")),
    };
    if pairs.len() > 32 {
        return Err(bad("at most 32 headers"));
    }
    for (k, v) in &pairs {
        if k.is_empty() || k.len() > 64 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(bad(format!("`{k}` is not a header name (letters, digits, -)")));
        }
        if v.len() > 1_024 || v.chars().any(char::is_control) {
            return Err(bad(format!("the {k} header's value is too long or has line breaks")));
        }
    }
    Ok(pairs)
}

/// `pc.http_request`.
pub(crate) fn http_request(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "pc")?;
    let dev = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    if !dev.kind.is_host() {
        return Err(bad("requests are sent from PCs, laptops and servers"));
    }
    let method = p.get("method").and_then(Value::as_str).unwrap_or("GET").trim().to_ascii_uppercase();
    if method.is_empty() || method.len() > 16 || !method.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(bad("`method` is a word such as GET, PUT, PATCH, POST or DELETE"));
    }
    let url = p.get("url").and_then(Value::as_str).ok_or_else(|| bad("`url` (text) is required"))?;
    let (tls, host, port, path) = split_url(url)?;
    let mut headers = header_param(p)?;
    let body: Vec<u8> = match p.get("body") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(t)) => t.as_bytes().to_vec(),
        Some(v) => serde_json::to_vec_pretty(v).unwrap_or_default(),
    };
    if body.len() > http::MAX_BODY {
        return Err(bad(format!("the body is larger than {} bytes", http::MAX_BODY)));
    }
    let has = |h: &[(String, String)], n: &str| h.iter().any(|(k, _)| k.eq_ignore_ascii_case(n));
    if let (Some(u), Some(pw)) = (p.get("user").and_then(Value::as_str), p.get("password").and_then(Value::as_str))
        && !has(&headers, "authorization")
    {
        headers.push(("Authorization".into(), format!("Basic {}", http::base64_encode(format!("{u}:{pw}").as_bytes()))));
    }
    if !body.is_empty() && !has(&headers, "content-type") {
        let xml = body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<');
        headers.push(("Content-Type".into(), if xml { YANG_XML } else { YANG_JSON }.into()));
    }
    let mut all = vec![("Host".to_string(), host.clone()), ("User-Agent".to_string(), "NetLabCraft-Programming".to_string())];
    all.extend(headers);
    if !has(&all, "connection") {
        all.push(("Connection".into(), "close".into()));
    }
    let req = Request { method: method.clone(), target: path.clone(), headers: all, body };
    let server = resolve(s, d, &host)?;
    let got =
        if tls { s.sim.https(&s.topology, d, server, port, &req.encode()) } else { s.sim.tcp_session(&s.topology, d, server, port, &req.encode()) };
    let applied = apply_changes(s);
    s.revision += 1;
    let got = got.map_err(|e| EngineError::Failed(e.to_string()))?;
    let resp = match http::Response::parse(&got) {
        Ok(Some(r)) => r,
        // A TLS alert record: this port wants TLS.
        _ if !tls && got.first() == Some(&netlabcraft_wire::tls::ALERT) && got.get(1) == Some(&3) => {
            return Err(EngineError::Failed(format!("{host} port {port} speaks TLS and refused plain HTTP: use https://")));
        }
        Ok(None) => return Err(EngineError::Failed("the server closed the connection before a full answer".into())),
        Err(e) => return Err(EngineError::Failed(format!("the answer is not HTTP: {e}"))),
    };
    let text = String::from_utf8_lossy(&resp.body).to_string();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(json!({
        "url": format!("{}://{host}{}{path}", if tls { "https" } else { "http" }, if port == if tls { 443 } else { 80 } { String::new() } else { format!(":{port}") }),
        "method": method,
        "status": resp.status,
        "reason": resp.reason,
        "headers": resp.headers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "body": text,
        "json": parsed,
        "applied": applied,
        "tls": if tls { json!({"version": "TLS 1.2", "cipher": netlabcraft_wire::tls::SUITE_NAME, "certificate": null}) } else { Value::Null },
    }))
}

/// `pc.http_saved`: the Programming app's saved requests.
pub(crate) fn http_saved(s: &mut Session, p: &Value) -> Result<Value> {
    use netlabcraft_model::automation::{MAX_SAVED, MAX_SAVED_BODY, SavedRequest};
    let d = crate::cmd::device_param(s, p, "pc")?;
    let dev = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    if !dev.kind.is_host() {
        return Err(bad("saved requests live on PCs, laptops and servers"));
    }
    let mut saved = dev.automation.saved.clone();
    let mut changed = false;
    if let Some(name) = p.get("remove").and_then(Value::as_str) {
        let before = saved.len();
        saved.retain(|r| r.name != name);
        if saved.len() == before {
            return Err(EngineError::Failed(format!("no saved request named {name}")));
        }
        changed = true;
    }
    if let Some(x) = p.get("save") {
        let name: String = x.get("name").and_then(Value::as_str).unwrap_or("").trim().chars().take(64).collect();
        if name.is_empty() {
            return Err(bad("a saved request needs a `name`"));
        }
        let url = x.get("url").and_then(Value::as_str).ok_or_else(|| bad("a saved request needs a `url`"))?;
        split_url(url)?;
        let method: String = x.get("method").and_then(Value::as_str).unwrap_or("GET").trim().to_ascii_uppercase().chars().take(16).collect();
        let headers = header_param(x)?;
        let body: String = match x.get("body") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(t)) => t.clone(),
            Some(v) => serde_json::to_string_pretty(v).unwrap_or_default(),
        };
        if body.len() > MAX_SAVED_BODY {
            return Err(bad(format!("a saved body is at most {MAX_SAVED_BODY} bytes")));
        }
        let r = SavedRequest { name: name.clone(), method, url: url.trim().to_string(), headers, body };
        match saved.iter_mut().find(|s| s.name == name) {
            Some(old) => *old = r,
            None => {
                if saved.len() >= MAX_SAVED {
                    return Err(EngineError::Failed(format!("at most {MAX_SAVED} saved requests")));
                }
                saved.push(r);
            }
        }
        changed = true;
    }
    if changed {
        s.checkpoint();
        if let Some(x) = s.topology.device_mut(d) {
            x.automation.saved = saved.clone();
        }
        s.changed();
    }
    Ok(
        json!({"saved": saved.iter().map(|r| json!({"name": r.name, "method": r.method, "url": r.url, "headers": r.headers, "body": r.body})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn lab() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "automation"})).unwrap();
        s
    }

    fn req(s: &mut Session, method: &str, path: &str, body: Option<Value>) -> Value {
        let mut p = json!({"pc": "PC1", "method": method, "url": format!("https://192.168.1.1{path}"), "user": "admin", "password": "NetLab2026"});
        if let Some(b) = body {
            p["body"] = b;
        }
        s.execute("pc.http_request", p).unwrap()
    }

    fn run_of(s: &Session, name: &str) -> String {
        netlabcraft_ios::running_config(s.topology.device(s.topology.resolve(name).unwrap()).unwrap())
    }

    fn run(s: &mut Session) -> String {
        run_of(s, "R1")
    }

    #[test]
    fn get_interfaces_and_put_a_description() {
        let mut s = lab();
        let r = req(&mut s, "GET", "/restconf/data/ietf-interfaces:interfaces", None);
        assert_eq!(r["status"], 200, "{r}");
        let list = r["json"]["ietf-interfaces:interfaces"]["interface"].as_array().unwrap();
        let g0 = list.iter().find(|i| i["name"] == "GigabitEthernet0/0").unwrap();
        assert_eq!(g0["description"], "LAN");
        assert_eq!(g0["ietf-ip:ipv4"]["address"][0]["ip"], "192.168.1.1");
        assert_eq!(g0["oper-status"], "up");
        assert!(list.iter().any(|i| i["name"] == "Loopback0" && i["type"] == "iana-if-type:softwareLoopback"));
        assert!(r["headers"].as_array().unwrap().iter().any(|h| h[0] == "Content-Type" && h[1] == YANG_JSON));
        // The request crossed the lab as TLS on port 443: a handshake, then encrypted data.
        let trace = s.sim.trace();
        assert!(trace.iter().any(|e| e.protocol == netlabcraft_wire::Protocol::Tls && e.summary.contains("ClientHello")));
        assert!(trace.iter().any(|e| e.summary.contains("Application Data (encrypted")));
        assert!(trace.iter().all(|e| !e.summary.contains("GET /restconf")), "nothing readable");

        let r = req(
            &mut s,
            "PUT",
            "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1/description",
            Some(json!({"ietf-interfaces:description": "Uplink to ISP"})),
        );
        assert_eq!(r["status"], 204, "{r}");
        assert!(run(&mut s).contains("interface GigabitEthernet0/1\n description Uplink to ISP"), "{}", run(&mut s));
        let r = req(&mut s, "GET", "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1/description", None);
        assert_eq!(r["json"], json!({"ietf-interfaces:description": "Uplink to ISP"}));
        assert!(s.can_undo());
    }

    #[test]
    fn put_replaces_and_patch_merges_an_interface() {
        let mut s = lab();
        let path = "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1";
        let r = req(
            &mut s,
            "PATCH",
            path,
            Some(
                json!({"ietf-interfaces:interface": {"name": "GigabitEthernet0/1", "enabled": true, "ietf-ip:ipv4": {"address": [{"ip": "10.9.9.1", "prefix-length": 30}]}}}),
            ),
        );
        assert_eq!(r["status"], 204, "{r}");
        let cfg = run(&mut s);
        assert!(cfg.contains(" ip address 10.9.9.1 255.255.255.252"), "{cfg}");
        // PUT: what the body leaves out goes away.
        let r = req(
            &mut s,
            "PUT",
            path,
            Some(json!({"ietf-interfaces:interface": [{"name": "GigabitEthernet0/1", "description": "spare", "enabled": false}]})),
        );
        assert_eq!(r["status"], 204, "{r}");
        let cfg = run(&mut s);
        assert!(!cfg.contains("10.9.9.1") && cfg.contains(" description spare"), "{cfg}");
        let r = req(&mut s, "GET", &format!("{path}?content=config"), None);
        let i = &r["json"]["ietf-interfaces:interface"][0];
        assert_eq!((i["enabled"].clone(), i.get("oper-status").is_none()), (json!(false), true), "{r}");
    }

    #[test]
    fn loopbacks_hostname_and_routes() {
        let mut s = lab();
        let r = req(
            &mut s,
            "POST",
            "/restconf/data/ietf-interfaces:interfaces",
            Some(
                json!({"ietf-interfaces:interface": {"name": "Loopback7", "ietf-ip:ipv4": {"address": [{"ip": "7.7.7.7", "netmask": "255.255.255.255"}]}}}),
            ),
        );
        assert_eq!(r["status"], 201, "{r}");
        assert!(r["headers"].as_array().unwrap().iter().any(|h| h[0] == "Location"));
        assert!(run(&mut s).contains("interface Loopback7\n ip address 7.7.7.7 255.255.255.255"));
        // Twice is a conflict.
        let again =
            req(&mut s, "POST", "/restconf/data/ietf-interfaces:interfaces", Some(json!({"ietf-interfaces:interface": {"name": "Loopback7"}})));
        assert_eq!(again["status"], 409);
        assert_eq!(again["json"]["ietf-restconf:errors"]["error"][0]["error-tag"], "data-exists");
        assert_eq!(req(&mut s, "DELETE", "/restconf/data/ietf-interfaces:interfaces/interface=Loopback7", None)["status"], 204);
        assert!(!run(&mut s).contains("Loopback7"));
        // Ports can't be deleted.
        assert_eq!(req(&mut s, "DELETE", "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0", None)["status"], 405);

        let r = req(&mut s, "PATCH", "/restconf/data/ietf-system:system", Some(json!({"ietf-system:system": {"hostname": "Edge1"}})));
        assert_eq!(r["status"], 204, "{r}");
        assert!(s.topology.resolve("Edge1").is_ok());

        let r = req(
            &mut s,
            "POST",
            "/restconf/data/netlabcraft-routing:static-routes",
            Some(json!({"netlabcraft-routing:route": {"prefix": "172.16.0.0/16", "next-hop": [{"address": "192.168.1.254", "distance": 5}]}})),
        );
        assert_eq!(r["status"], 201, "{r}");
        let cfg = run_of(&s, "Edge1");
        assert!(cfg.contains("ip route 172.16.0.0 255.255.0.0 192.168.1.254 5"), "{cfg}");
        let r = req(&mut s, "GET", "/restconf/data/netlabcraft-routing:static-routes/route=172.16.0.0%2F16", None);
        assert_eq!(r["json"]["netlabcraft-routing:route"][0]["next-hop"][0]["distance"], 5, "{r}");
        assert_eq!(req(&mut s, "DELETE", "/restconf/data/netlabcraft-routing:static-routes/route=172.16.0.0%2F16", None)["status"], 204);
        assert!(s.topology.devices.iter().all(|d| d.routes.is_empty()));
    }

    #[test]
    fn logins_are_checked() {
        let mut s = lab();
        let url = "https://192.168.1.1/restconf/data/ietf-system:system";
        let r = s.execute("pc.http_request", json!({"pc": "PC1", "url": url})).unwrap();
        assert_eq!(r["status"], 401);
        assert!(r["headers"].as_array().unwrap().iter().any(|h| h[0] == "WWW-Authenticate"));
        let r = s.execute("pc.http_request", json!({"pc": "PC1", "url": url, "user": "admin", "password": "wrong"})).unwrap();
        assert_eq!(r["status"], 401);
        s.execute("device.configure", json!({"device": "R1", "lines": "username guest secret Guest2026"})).unwrap();
        let r = s.execute("pc.http_request", json!({"pc": "PC1", "url": url, "user": "guest", "password": "Guest2026"})).unwrap();
        assert_eq!(r["status"], 403, "{r}");
        // A header given by hand works the same as user and password.
        let auth = format!("Basic {}", http::base64_encode(b"admin:NetLab2026"));
        let r = s.execute("pc.http_request", json!({"pc": "PC1", "url": url, "headers": {"Authorization": auth}})).unwrap();
        assert_eq!(r["status"], 200);
    }

    #[test]
    fn bad_requests_get_honest_errors() {
        let mut s = lab();
        let cases: Vec<(&str, &str, Option<Value>, u64)> = vec![
            ("GET", "/restconf/data/ietf-interfaces:interfaces/interface=Serial9%2F9", None, 404),
            ("GET", "/restconf/data/interfaces", None, 400),
            ("GET", "/restconf/data/acme-widgets:widgets", None, 404),
            ("GET", "/restconf/data/ietf-interfaces:interfaces/interface", None, 400),
            ("GET", "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0/ipv4", None, 400),
            ("GET", "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0/ietf-ip:ipv4/address=1", None, 501),
            ("GET", "/restconf/data?depth=2", None, 400),
            ("GET", "/restconf/data/ietf-interfaces:interfaces/interface=%zz", None, 400),
            ("GET", "/index.html", None, 404),
            ("PUT", "/restconf/data/ietf-system:system/hostname", Some(json!("{not json")), 400),
            ("PUT", "/restconf/data/ietf-system:system/hostname", Some(json!({"hostname": "X"})), 400),
            ("PUT", "/restconf/data/ietf-system:system/hostname", Some(json!({"ietf-system:hostname": "two words"})), 400),
            ("PUT", "/restconf/data/ietf-system:system/hostname", Some(json!({"ietf-system:hostname": "R9\ninterface g0/0"})), 400),
            ("PUT", "/restconf/data/ietf-interfaces:interfaces", Some(json!({})), 405),
            (
                "PATCH",
                "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1",
                Some(json!({"ietf-interfaces:interface": {"speed": 10}})),
                400,
            ),
            (
                "PATCH",
                "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1",
                Some(json!({"ietf-interfaces:interface": {"name": "GigabitEthernet0/0"}})),
                400,
            ),
            (
                "PATCH",
                "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1",
                Some(json!({"ietf-interfaces:interface": {"ietf-ip:ipv4": {"address": [{"ip": "300.1.1.1", "netmask": "255.0.0.0"}]}}})),
                400,
            ),
            (
                "POST",
                "/restconf/data/netlabcraft-routing:static-routes",
                Some(json!({"netlabcraft-routing:route": {"prefix": "10.0.0.1/8", "next-hop": [{"address": "1.1.1.1"}]}})),
                400,
            ),
            ("DELETE", "/restconf/data/netlabcraft-routing:static-routes/route=10.0.0.0%2F8", None, 404),
            ("BREW", "/restconf/data", None, 405),
        ];
        let before = s.topology.clone();
        for (m, path, body, want) in cases {
            let r = req(&mut s, m, path, body);
            assert_eq!(r["status"], want, "{m} {path}: {r}");
            assert!(r["json"]["ietf-restconf:errors"]["error"][0]["error-message"].is_string(), "{m} {path}: {r}");
        }
        assert_eq!(s.topology, before, "failed requests change nothing");
        // Media types other than JSON and XML are refused with 415; XML without its namespace
        // names no module.
        let put = |ct: &str, body: &str| {
            json!({"pc": "PC1", "method": "PUT", "url": "https://192.168.1.1/restconf/data/ietf-system:system/hostname", "user": "admin", "password": "NetLab2026",
                   "headers": {"Content-Type": ct}, "body": body})
        };
        assert_eq!(s.execute("pc.http_request", put("text/plain", "X")).unwrap()["status"], 415);
        assert_eq!(s.execute("pc.http_request", put(YANG_XML, "<hostname>X</hostname>")).unwrap()["status"], 400);
        assert_eq!(s.execute("pc.http_request", put(YANG_XML, "<hostname xmlns='urn:acme'>X</hostname>")).unwrap()["status"], 400);
        assert_eq!(s.execute("pc.http_request", put(YANG_XML, "<hostname")).unwrap()["status"], 400);
        assert_eq!(s.topology, before);
    }

    fn xml_req(s: &mut Session, method: &str, path: &str, body: Option<&str>) -> Value {
        let mut p = json!({"pc": "PC1", "method": method, "url": format!("https://192.168.1.1{path}"), "user": "admin", "password": "NetLab2026",
                           "headers": {"Accept": YANG_XML}});
        if let Some(b) = body {
            p["body"] = json!(b);
        }
        s.execute("pc.http_request", p).unwrap()
    }

    #[test]
    fn xml_bodies_round_trip_through_get_and_edits() {
        let mut s = lab();
        // GET in XML: namespaces on the module tops, ietf-ip's ipv4 in its own.
        let r = xml_req(&mut s, "GET", "/restconf/data/ietf-interfaces:interfaces", None);
        assert_eq!(r["status"], 200, "{r}");
        assert!(r["headers"].as_array().unwrap().iter().any(|h| h[0] == "Content-Type" && h[1] == YANG_XML), "{r}");
        let doc = netlabcraft_wire::xml::parse(r["body"].as_str().unwrap()).unwrap();
        assert_eq!((doc.name.as_str(), doc.ns.as_deref()), ("interfaces", Some(namespace(IF))));
        let g0 = doc.children_named("interface").find(|i| i.text_of("name") == Some("GigabitEthernet0/0")).unwrap();
        assert_eq!(g0.child("ipv4").unwrap().ns.as_deref(), Some(namespace(IP)));
        // Every resource reads the same in XML as in JSON.
        for path in [
            "/restconf/data",
            "/restconf/data/ietf-interfaces:interfaces",
            "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0",
            "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0/description",
            "/restconf/data/ietf-system:system",
            "/restconf/data/netlabcraft-routing:static-routes",
        ] {
            let j = req(&mut s, "GET", path, None)["json"].clone();
            let x = xml_req(&mut s, "GET", path, None);
            let back = xml_to_json(&netlabcraft_wire::xml::parse(x["body"].as_str().unwrap()).unwrap()).unwrap();
            // An empty list leaves nothing in XML.
            fn strip(v: &Value) -> Value {
                match v {
                    Value::Object(m) => Value::Object(
                        m.iter().filter(|(_, x)| x.as_array().is_none_or(|a| !a.is_empty())).map(|(k, x)| (k.clone(), strip(x))).collect(),
                    ),
                    Value::Array(a) => Value::Array(a.iter().map(strip).collect()),
                    x => x.clone(),
                }
            }
            assert_eq!(back, strip(&j), "{path}");
        }
        // Edits in XML.
        let ifns = namespace(IF);
        let r = xml_req(
            &mut s,
            "PUT",
            "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1/description",
            Some(&format!("<description xmlns=\"{ifns}\">Set &amp; read in XML</description>")),
        );
        assert_eq!(r["status"], 204, "{r}");
        assert!(run(&mut s).contains(" description Set & read in XML"));
        let body = format!(
            "<interface xmlns=\"{ifns}\"><name>GigabitEthernet0/1</name><enabled>false</enabled>\
             <ipv4 xmlns=\"{}\"><address><ip>10.9.9.1</ip><prefix-length>30</prefix-length></address></ipv4></interface>",
            namespace(IP)
        );
        let r = xml_req(&mut s, "PATCH", "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F1", Some(&body));
        assert_eq!(r["status"], 204, "{r}");
        let cfg = run(&mut s);
        assert!(cfg.contains(" ip address 10.9.9.1 255.255.255.252") && cfg.contains(" shutdown"), "{cfg}");
        let route = format!(
            "<route xmlns=\"{}\"><prefix>172.20.0.0/16</prefix><next-hop><address>10.9.9.2</address><distance>7</distance></next-hop></route>",
            namespace(RT)
        );
        let r = xml_req(&mut s, "POST", "/restconf/data/netlabcraft-routing:static-routes", Some(&route));
        assert_eq!(r["status"], 201, "{r}");
        assert!(run(&mut s).contains("ip route 172.20.0.0 255.255.0.0 10.9.9.2 7"));
        let r = xml_req(&mut s, "GET", "/restconf/data/netlabcraft-routing:static-routes/route=172.20.0.0%2F16", None);
        let back = xml_to_json(&netlabcraft_wire::xml::parse(r["body"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(back["netlabcraft-routing:route"][0]["next-hop"][0]["distance"], 7, "{back}");
        // Errors come back in XML too.
        let r = xml_req(&mut s, "GET", "/restconf/data/ietf-interfaces:interfaces/interface=Serial9%2F9", None);
        assert_eq!(r["status"], 404);
        let e = netlabcraft_wire::xml::parse(r["body"].as_str().unwrap()).unwrap();
        assert_eq!((e.name.as_str(), e.ns.as_deref()), ("errors", Some(namespace(RESTCONF))));
        assert_eq!(e.child("error").and_then(|x| x.text_of("error-tag")), Some("invalid-value"));
    }

    #[test]
    fn ipv6_addresses_are_read_and_edited() {
        let mut s = lab();
        s.execute("device.configure", json!({"device": "R1", "lines": "interface g0/0\n ipv6 address 2001:db8:1::1/64"})).unwrap();
        let path = "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0";
        let r = req(&mut s, "GET", path, None);
        assert_eq!(
            r["json"]["ietf-interfaces:interface"][0]["ietf-ip:ipv6"],
            json!({"address": [{"ip": "2001:db8:1::1", "prefix-length": 64}]}),
            "{r}"
        );
        // PATCH adds, PUT on the container replaces, DELETE removes them all.
        let v6 = |list: Value| json!({"ietf-interfaces:interface": {"name": "GigabitEthernet0/0", "ietf-ip:ipv6": {"address": list}}});
        let r = req(&mut s, "PATCH", path, Some(v6(json!([{"ip": "2001:DB8:2::1", "prefix-length": 64}]))));
        assert_eq!(r["status"], 204, "{r}");
        let cfg = run(&mut s);
        assert!(cfg.contains(" ipv6 address 2001:db8:1::1/64") && cfg.contains(" ipv6 address 2001:db8:2::1/64"), "{cfg}");
        let r = req(
            &mut s,
            "PUT",
            &format!("{path}/ietf-ip:ipv6"),
            Some(json!({"ietf-ip:ipv6": {"address": [{"ip": "2001:db8:3::1", "prefix-length": 48}]}})),
        );
        assert_eq!(r["status"], 204, "{r}");
        let cfg = run(&mut s);
        assert!(!cfg.contains("2001:db8:1::1") && !cfg.contains("2001:db8:2::1") && cfg.contains(" ipv6 address 2001:db8:3::1/48"), "{cfg}");
        // The IPv4 address is untouched by IPv6 edits.
        assert!(cfg.contains(" ip address 192.168.1.1 255.255.255.0"), "{cfg}");
        assert_eq!(req(&mut s, "DELETE", &format!("{path}/ietf-ip:ipv6"), None)["status"], 204);
        assert!(!run(&mut s).contains("2001:db8:3::1"));
        assert_eq!(req(&mut s, "DELETE", &format!("{path}/ietf-ip:ipv6"), None)["status"], 404);
        // A loopback created with an IPv6 address only.
        let r = req(
            &mut s,
            "POST",
            "/restconf/data/ietf-interfaces:interfaces",
            Some(
                json!({"ietf-interfaces:interface": {"name": "Loopback6", "ietf-ip:ipv6": {"address": [{"ip": "2001:db8:66::1", "prefix-length": 128}]}}}),
            ),
        );
        assert_eq!(r["status"], 201, "{r}");
        assert!(run(&mut s).contains(" ipv6 address 2001:db8:66::1/128"), "{}", run(&mut s));
        let before = s.topology.clone();
        for bad in [
            json!([{"ip": "fe80::1", "prefix-length": 64}]),
            json!([{"ip": "2001:db8::1"}]),
            json!([{"ip": "2001:db8::1", "prefix-length": 0}]),
            json!([{"ip": "2001:db8::1", "prefix-length": 129}]),
            json!([{"ip": "10.0.0.1", "prefix-length": 64}]),
            json!([{"ip": "2001:db8::1", "prefix-length": 64, "origin": "static"}]),
            json!((0..9).map(|i| json!({"ip": format!("2001:db8:{i}::1"), "prefix-length": 64})).collect::<Vec<_>>()),
        ] {
            let r = req(&mut s, "PATCH", path, Some(v6(bad.clone())));
            assert_eq!(r["status"], 400, "{bad}: {r}");
        }
        assert_eq!(s.topology, before);
    }

    #[test]
    fn requests_reach_a_routers_ipv6_address() {
        let mut s = lab();
        s.execute("device.configure", json!({"device": "R1", "lines": "interface g0/0\n ipv6 address 2001:db8:1::1/64"})).unwrap();
        s.execute("cli.exec", json!({"device": "PC1", "line": "ipv6config 2001:db8:1::10/64 2001:db8:1::1"})).unwrap();
        let r = s
            .execute(
                "pc.http_request",
                json!({"pc": "PC1", "url": "https://[2001:db8:1::1]/restconf/data/ietf-system:system", "user": "admin", "password": "NetLab2026"}),
            )
            .unwrap();
        assert_eq!(r["status"], 200, "{r}");
        assert_eq!(r["json"]["ietf-system:system"]["hostname"], "R1");
        assert_eq!(r["url"], "https://[2001:db8:1::1]/restconf/data/ietf-system:system");
        // The segments crossed the lab as TCP over IPv6.
        assert!(s.sim.trace().iter().any(|e| e.summary.contains("2001:db8:1::10 → 2001:db8:1::1")), "IPv6 TCP in the trace");
        // An edit over IPv6 lands too.
        let r = s
            .execute(
                "pc.http_request",
                json!({"pc": "PC1", "method": "PATCH", "url": "https://[2001:db8:1::1]:443/restconf/data/ietf-system:system", "user": "admin", "password": "NetLab2026",
                       "body": {"ietf-system:system": {"hostname": "R1v6"}}}),
            )
            .unwrap();
        assert_eq!(r["status"], 204, "{r}");
        assert!(s.topology.resolve("R1v6").is_ok());
        // NETCONF over IPv6.
        let r = s
            .execute("netconf.session", json!({"pc": "PC1", "target": "2001:db8:1::1", "user": "admin", "password": "NetLab2026", "rpcs": ["<get-config><source><running/></source></get-config>"]}))
            .unwrap();
        assert_eq!(r["connected"], true, "{r}");
        assert!(r["replies"][0].as_str().unwrap().contains("2001:db8:1::1"), "{r}");
        // An address nobody has times out; a host without IPv6 can't try.
        assert!(s.execute("pc.http_request", json!({"pc": "PC1", "url": "https://[2001:db8:1::99]/restconf"})).is_err());
        assert!(s.execute("pc.http_request", json!({"pc": "PC2", "url": "https://[2001:db8:1::1]/restconf"})).is_err());
        assert_eq!(split_url("https://[2001:db8::1]:8443/x").unwrap(), (true, "[2001:db8::1]".into(), 8443, "/x".into()));
        assert_eq!(split_url("http://[fe80::1]").unwrap(), (false, "[fe80::1]".into(), 80, "/".into()));
        for bad in ["https://2001:db8::1/", "https://[2001:db8::1/", "https://[nope]/", "https://[::1]x/", "https://[::1]:/"] {
            assert!(split_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn https_is_real_tls_and_plain_text_is_refused_on_443() {
        let mut s = lab();
        let r = req(&mut s, "GET", "/restconf/data/ietf-system:system", None);
        assert_eq!(r["status"], 200, "{r}");
        assert_eq!(r["tls"]["cipher"], netlabcraft_wire::tls::SUITE_NAME);
        // Every frame on port 443 is TLS, and neither the request nor the password shows.
        let auth = http::base64_encode(b"admin:NetLab2026");
        let mut kinds = Vec::new();
        for e in s.sim.trace() {
            assert!(!e.frame.windows(8).any(|w| w == b"restconf"), "{}", e.summary);
            assert!(!e.frame.windows(auth.len()).any(|w| w == auth.as_bytes()));
            if e.protocol == netlabcraft_wire::Protocol::Tls {
                kinds.push(e.summary.clone());
            }
        }
        for want in ["ClientHello", "ServerHello", "ServerKeyExchange", "ClientKeyExchange", "ChangeCipherSpec", "Application Data", "Alert"] {
            assert!(kinds.iter().any(|k| k.contains(want)), "{want}: {kinds:?}");
        }
        // Plain HTTP to the TLS port gets a TLS alert, and the client says why.
        let e = s
            .execute(
                "pc.http_request",
                json!({"pc": "PC1", "url": "http://192.168.1.1:443/restconf/data", "user": "admin", "password": "NetLab2026"}),
            )
            .unwrap_err();
        assert!(e.to_string().contains("speaks TLS"), "{e}");
        assert!(s.sim.trace().iter().any(|e| e.summary.contains("Alert (unexpected_message)")));
        // TLS to the plain web server is refused too.
        s.execute("device.configure", json!({"device": "R1", "lines": "ip http server"})).unwrap();
        let e = s.execute("pc.http_request", json!({"pc": "PC1", "url": "https://192.168.1.1:80/restconf"})).unwrap_err();
        assert!(e.to_string().contains("plain text"), "{e}");
        let r = s
            .execute("pc.http_request", json!({"pc": "PC1", "url": "http://192.168.1.1/restconf", "user": "admin", "password": "NetLab2026"}))
            .unwrap();
        assert_eq!((r["status"].clone(), r["tls"].clone()), (json!(200), Value::Null));
    }

    #[test]
    fn accept_headers_pick_the_encoding() {
        assert_eq!(negotiate(None).unwrap(), Encoding::Json);
        assert_eq!(negotiate(Some("*/*")).unwrap(), Encoding::Json);
        assert_eq!(negotiate(Some("application/yang-data+xml")).unwrap(), Encoding::Xml);
        assert_eq!(negotiate(Some("application/yang-data+xml;q=0.5, application/yang-data+json")).unwrap(), Encoding::Json);
        assert_eq!(negotiate(Some("application/yang-data+json;q=0.1, application/yang-data+xml")).unwrap(), Encoding::Xml);
        assert_eq!(negotiate(Some("text/html, application/xml;q=0.9")).unwrap(), Encoding::Xml);
        for bad in ["text/html", "application/yang-data+json;q=0", "image/png;q=1.0"] {
            assert_eq!(negotiate(Some(bad)).unwrap_err().status, 406, "{bad}");
        }
        let mut s = lab();
        let r = s
            .execute(
                "pc.http_request",
                json!({"pc": "PC1", "url": "https://192.168.1.1/restconf/data", "user": "admin", "password": "NetLab2026", "headers": {"Accept": "text/html"}}),
            )
            .unwrap();
        assert_eq!(r["status"], 406, "{r}");
        // Hostile XML is answered, never trusted.
        let deep = format!("{}{}", "<a>".repeat(40), "</a>".repeat(40));
        for body in [
            deep.as_str(),
            "<!DOCTYPE x [<!ENTITY a 'b'>]><x>&a;</x>",
            "<system xmlns='urn:ietf:params:xml:ns:yang:ietf-system'>t<hostname>X</hostname></system>",
        ] {
            let r = xml_req(&mut s, "PATCH", "/restconf/data/ietf-system:system", Some(body));
            assert_eq!(r["status"], 400, "{body}: {r}");
        }
    }

    #[test]
    fn the_server_must_be_on() {
        let mut s = lab();
        // Port 80 is closed: only the secure server runs.
        let e = s.execute("pc.http_request", json!({"pc": "PC1", "url": "http://192.168.1.1/restconf/data"})).unwrap_err();
        assert!(e.to_string().contains("refused"), "{e}");
        s.execute("device.configure", json!({"device": "R1", "lines": "ip http server\nno restconf"})).unwrap();
        let r = req(&mut s, "GET", "/restconf/data", None);
        assert_eq!(r["status"], 404);
        let r = req(&mut s, "GET", "/.well-known/host-meta", None);
        assert!(r["body"].as_str().unwrap().contains("rel='restconf'"));
        s.execute("device.configure", json!({"device": "R1", "lines": "restconf"})).unwrap();
        let r = s
            .execute("pc.http_request", json!({"pc": "PC1", "url": "http://192.168.1.1/restconf", "user": "admin", "password": "NetLab2026"}))
            .unwrap();
        assert_eq!(r["json"]["ietf-restconf:restconf"]["yang-library-version"], "2019-01-04");
        // From a router: refused.
        assert!(s.execute("pc.http_request", json!({"pc": "R1", "url": "http://192.168.1.1/"})).is_err());
    }

    #[test]
    fn huge_and_hostile_requests_are_capped() {
        let mut s = lab();
        let big = "x".repeat(http::MAX_BODY + 1);
        assert!(
            s.execute("pc.http_request", json!({"pc": "PC1", "method": "PUT", "url": "https://192.168.1.1/restconf/data", "body": big})).is_err()
        );
        for bad in [
            json!({"pc": "PC1", "url": "ftp://x"}),
            json!({"pc": "PC1", "url": "https://"}),
            json!({"pc": "PC1", "url": "https://192.168.1.1:99999/"}),
            json!({"pc": "PC1", "url": "https://192.168.1.1/a b"}),
            json!({"pc": "PC1", "url": "https://192.168.1.1/", "method": "G E T"}),
            json!({"pc": "PC1", "url": "https://192.168.1.1/", "headers": {"Bad Name": "x"}}),
            json!({"pc": "PC1", "url": "https://192.168.1.1/", "headers": {"X": "a\r\nInjected: 1"}}),
            json!({"pc": "PC1", "url": "https://192.168.1.1/", "headers": 5}),
        ] {
            assert!(s.execute("pc.http_request", bad.clone()).is_err(), "{bad}");
        }
        // Deeply nested or enormous JSON is answered, never trusted.
        let deep = format!("{}{}", "[".repeat(5_000), "]".repeat(5_000));
        let r = req(&mut s, "PATCH", "/restconf/data/ietf-system:system", Some(json!(deep)));
        assert_eq!(r["status"], 400);
        let many: Vec<Value> = (0..200).map(|i| json!({"name": format!("Loopback{i}")})).collect();
        let r = req(&mut s, "POST", "/restconf/data/ietf-interfaces:interfaces", Some(json!({"ietf-interfaces:interface": many})));
        assert_eq!(r["status"], 413, "{r}");
    }

    #[test]
    fn saved_requests_live_on_the_pc() {
        let mut s = lab();
        let r = s.execute("pc.http_saved", json!({"pc": "PC1"})).unwrap();
        let n = r["saved"].as_array().unwrap().len();
        assert!(n >= 5);
        let r = s
            .execute("pc.http_saved", json!({"pc": "PC1", "save": {"name": "Mine", "method": "get", "url": "https://192.168.1.1/restconf"}}))
            .unwrap();
        assert_eq!(r["saved"].as_array().unwrap().len(), n + 1);
        let saved = s.topology.device(s.topology.resolve("PC1").unwrap()).unwrap().automation.saved.clone();
        assert_eq!(saved.last().unwrap().method, "GET");
        assert!(s.execute("pc.http_saved", json!({"pc": "PC1", "save": {"name": "Bad", "url": "nope"}})).is_err());
        assert!(s.execute("pc.http_saved", json!({"pc": "PC1", "remove": "Nope"})).is_err());
        let r = s.execute("pc.http_saved", json!({"pc": "PC1", "remove": "Mine"})).unwrap();
        assert_eq!(r["saved"].as_array().unwrap().len(), n);
        // Every saved request of the sample works, in order.
        for x in s.topology.device(s.topology.resolve("PC1").unwrap()).unwrap().automation.saved.clone() {
            let r = s
                .execute("pc.http_request", json!({"pc": "PC1", "method": x.method, "url": x.url, "headers": x.headers, "body": x.body, "user": "admin", "password": "NetLab2026"}))
                .unwrap();
            assert!(r["status"].as_u64().unwrap() < 300, "{}: {r}", x.name);
        }
    }

    #[test]
    fn paths_parse_into_targets() {
        assert_eq!(parse_target("").unwrap(), Target::Data);
        assert_eq!(parse_target("/ietf-system:system/hostname").unwrap(), Target::Hostname);
        assert_eq!(parse_target("/ietf-system:system/ietf-system:hostname").unwrap(), Target::Hostname);
        assert_eq!(parse_target("/ietf-interfaces:interfaces/interface=Gi0%2F0").unwrap(), Target::Interface("Gi0/0".into()));
        assert_eq!(
            parse_target("/ietf-interfaces:interfaces/interface=Gi0%2F0/ietf-ip:ipv4").unwrap(),
            Target::IfChild("Gi0/0".into(), "ietf-ip:ipv4")
        );
        assert_eq!(parse_target("/netlabcraft-routing:static-routes/route=10.0.0.0%2F8").unwrap(), Target::Route("10.0.0.0/8".into()));
        for bad in ["/a/b/c/d/e/f/g/h/i", "/ietf-system:system=1", "/x", "/ietf-system:system/contact"] {
            assert!(parse_target(bad).is_err(), "{bad}");
        }
        assert_eq!(split_url("https://r1.lab:8443/x?y=1").unwrap(), (true, "r1.lab".into(), 8443, "/x?y=1".into()));
        assert_eq!(split_url("HTTP://10.0.0.1").unwrap(), (false, "10.0.0.1".into(), 80, "/".into()));
    }
}
