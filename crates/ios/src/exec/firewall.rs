//! The firewall appliance's commands: named interfaces (and subinterfaces) with security
//! levels, access lists bound to interfaces, network objects and object groups, object and
//! twice NAT, ICMP rules for the firewall itself, static routes by interface name, the global
//! inspection policy, the DHCP server, timeouts, and the tables (`show conn`, `show xlate`,
//! `show nat`, `show access-list`).

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::acl::{AclProto, PortMatch};
use netlabcraft_model::firewall::{
    self as fw, AccessGroup, Dhcpd, FwAce, FwAcl, FwClassMap, FwPolicyClass, FwPolicyMap, FwRemark, IcmpRule, MAX_DHCPD, MAX_DHCPD_ADDRESSES,
    MAX_ENTRIES, MAX_LISTS, MAX_MEMBERS, MAX_OBJECTS, MAX_POLICY, MAX_REMARK, NatKind, NetGroup, NetMember, NetObject, ObjAddr, ObjNat,
    RouteDistance, SvcGroup, SvcMember, SvcProto, TwiceNat, duration_text, parse_duration, port_number, valid_name,
};
use netlabcraft_model::{Device, End, MAX_ROUTES, NextHop, StaticRoute, Topology};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// The full name of an interface reference, as the firewall keys its settings.
fn ref_name(d: &Device, r: super::IfRef) -> Option<String> {
    let p = d.ports.get(r.port)?;
    Some(r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))
}

fn bad_name(what: &str, name: &str) -> String {
    format!("% `{name}` can't name {what}: use letters, digits, - _ or . (starting with a letter, at most {} characters)", fw::MAX_NAME)
}

/// Every interface the firewall has, subinterfaces after their port: (name, address,
/// shut down, port index, VLAN of a subinterface).
fn all_ifaces(d: &Device) -> Vec<(String, Option<Ipv4Net>, bool, usize, Option<u16>)> {
    let mut out = Vec::new();
    for (i, p) in d.ports.iter().enumerate() {
        out.push((p.name.clone(), p.ip, p.shutdown, i, None));
        for s in &p.subifs {
            out.push((format!("{}.{}", p.name, s.number), s.ip, s.shutdown, i, Some(s.vlan.map_or(0, |v| v.get()))));
        }
    }
    out
}

/// The interface (port or subinterface name) carrying a nameif, or an error naming the
/// missing nameif.
fn port_of(d: &Device, nameif: &str) -> Result<String, String> {
    d.firewall.port_of(nameif).map(str::to_string).ok_or_else(|| format!("% No interface is named {nameif} (set one with `nameif {nameif}`)"))
}

/// The address of the interface carrying a nameif.
fn net_of(d: &Device, nameif: &str) -> Result<Ipv4Net, String> {
    let port = port_of(d, nameif)?;
    all_ifaces(d).into_iter().find(|i| i.0 == port).and_then(|i| i.1).ok_or_else(|| format!("% {nameif} ({port}) has no address yet"))
}

/// A subinterface was deleted: its firewall settings go too.
pub(crate) fn forget_subif(d: &mut Device, port: usize, n: u32) {
    let Some(p) = d.ports.get(port) else { return };
    let name = format!("{}.{n}", p.name);
    let old = d.firewall.iface(&name).and_then(|i| i.nameif.clone());
    d.firewall.ifaces.retain(|i| i.port != name);
    if let Some(old) = old {
        forget_nameif(&mut d.firewall, &old);
    }
}

/// What named an interface that lost its name goes too.
fn forget_nameif(f: &mut fw::Firewall, old: &str) {
    f.groups.retain(|g| !g.nameif.eq_ignore_ascii_case(old));
    f.dhcpd.retain(|r| !r.nameif.eq_ignore_ascii_case(old));
    f.icmp_rules.retain(|r| !r.nameif.eq_ignore_ascii_case(old));
}

/// `A.B.C.D-A.B.C.D`.
fn parse_range(s: &str) -> Result<(Ipv4Addr, Ipv4Addr), String> {
    let (a, b) = s.split_once('-').ok_or("% Give the range as A.B.C.D-A.B.C.D")?;
    let a = parse_ip(a.trim()).map_err(|e| format!("% {e}"))?;
    let b = parse_ip(b.trim()).map_err(|e| format!("% {e}"))?;
    if u32::from(b) < u32::from(a) {
        return Err(format!("% The range ends ({b}) before it starts ({a})"));
    }
    Ok((a, b))
}

/// `(REAL,MAPPED)`.
fn parse_pair(s: &str) -> Result<(String, String), String> {
    let inner = s
        .trim()
        .strip_prefix('(')
        .and_then(|x| x.strip_suffix(')'))
        .ok_or("% Write the interfaces as (real,mapped), for example (inside,outside)")?;
    let (a, b) = inner.split_once(',').ok_or("% Write the interfaces as (real,mapped), for example (inside,outside)")?;
    let (a, b) = (a.trim(), b.trim());
    for n in [a, b] {
        if !valid_name(n) {
            return Err(bad_name("an interface", n));
        }
    }
    Ok((a.to_string(), b.to_string()))
}

/// `(REAL,MAPPED)`, checked against the interfaces (`any` allowed for the real side).
fn nat_pair(d: &Device, s: &str) -> Result<(String, String), String> {
    let (real_if, mapped_if) = parse_pair(s)?;
    for n in [&real_if, &mapped_if] {
        if !n.eq_ignore_ascii_case("any") {
            port_of(d, n)?;
        }
    }
    if mapped_if.eq_ignore_ascii_case("any") || real_if.eq_ignore_ascii_case(&mapped_if) {
        return Err("% The mapped interface must be a named interface other than the real one".into());
    }
    Ok((real_if, mapped_if))
}

/// The interfaces of the current interface mode, by name.
fn mode_ifaces(c: &Ctx<'_>) -> Result<Vec<String>, String> {
    let Mode::Iface(refs) = &c.cli.mode else { return Err("% Not in interface mode".into()) };
    let d = c.d()?;
    Ok(refs.iter().filter_map(|r| ref_name(d, *r)).collect())
}

/// `eq P` or `range P Q`.
fn parse_port_match(words: &[&str]) -> Result<PortMatch, String> {
    let num = |w: Option<&&str>| -> Result<u16, String> {
        let w = w.ok_or("% Give a port")?;
        port_number(w).ok_or_else(|| format!("% `{w}` is not a port"))
    };
    match words.first().map(|w| w.to_ascii_lowercase()).as_deref() {
        Some("eq") if words.len() == 2 => Ok(PortMatch::Eq(num(words.get(1))?)),
        Some("range") if words.len() == 3 => {
            let (a, b) = (num(words.get(1))?, num(words.get(2))?);
            Ok(PortMatch::Range(a.min(b), a.max(b)))
        }
        _ => Err("% Give the ports as eq PORT or range PORT PORT".into()),
    }
}

/// One member line of an object group (`network-object …`, `port-object …`, …).
enum Member {
    Net(NetMember),
    Svc(SvcMember),
}

fn parse_member(f: &fw::Firewall, service: bool, group: &str, words: &[&str]) -> Result<Member, String> {
    let kw = words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
    let rest = words.get(1..).unwrap_or_default();
    match (kw.as_str(), service) {
        ("network-object", false) => {
            let m = match rest {
                [h, a] if h.eq_ignore_ascii_case("host") => NetMember::Host(parse_ip(a).map_err(|e| format!("% {e}"))?),
                [o, n] if o.eq_ignore_ascii_case("object") => {
                    if f.object(n).is_none() {
                        return Err(format!("% There is no network object {n}"));
                    }
                    NetMember::Object((*n).to_string())
                }
                [a, m] => {
                    let net = Ipv4Net::with_mask(parse_ip(a).map_err(|e| format!("% {e}"))?, parse_ip(m).map_err(|e| format!("% {e}"))?)
                        .map_err(|e| format!("% {e}"))?;
                    if net.network() != net {
                        return Err(format!("% {a} has host bits set; the network is {}", net.network().addr));
                    }
                    if net.prefix == 32 { NetMember::Host(net.addr) } else { NetMember::Net(net) }
                }
                _ => return Err("% Give host A.B.C.D, A.B.C.D MASK or object NAME".into()),
            };
            Ok(Member::Net(m))
        }
        ("group-object", _) => {
            let [n] = rest else { return Err("% Give one group's name".into()) };
            if *n == group {
                return Err("% A group can't hold itself".into());
            }
            let exists = if service { f.svc_group(n).is_some() } else { f.net_group(n).is_some() };
            if !exists {
                let kind = if service { "service" } else { "network" };
                return Err(format!("% There is no {kind} object group {n}"));
            }
            Ok(if service { Member::Svc(SvcMember::Group((*n).to_string())) } else { Member::Net(NetMember::Group((*n).to_string())) })
        }
        ("port-object", true) => {
            if f.svc_group(group).is_some_and(|g| g.proto.is_none()) {
                return Err("% port-object is for groups of ports: create the group as `object-group service NAME tcp|udp|tcp-udp`".into());
            }
            Ok(Member::Svc(SvcMember::Port(parse_port_match(rest)?)))
        }
        ("service-object", true) => {
            if f.svc_group(group).is_some_and(|g| g.proto.is_some()) {
                return Err("% This group holds ports (port-object); service-object is for groups made without a protocol".into());
            }
            let p = rest.first().ok_or("% Give a protocol: ip, icmp, tcp or udp")?;
            let proto = AclProto::from_name(p).ok_or_else(|| format!("% `{p}` is not a protocol here (ip, icmp, tcp, udp)"))?;
            let dst = match rest.get(1..).unwrap_or_default() {
                [] => None,
                [d, more @ ..] if d.eq_ignore_ascii_case("destination") && matches!(proto, AclProto::Tcp | AclProto::Udp) => {
                    Some(parse_port_match(more)?)
                }
                _ => return Err("% After the protocol: destination eq PORT or destination range PORT PORT (tcp and udp only)".into()),
            };
            Ok(Member::Svc(SvcMember::Service { proto, dst }))
        }
        (_, true) => Err("% A service group holds port-object, service-object and group-object lines".into()),
        (_, false) => Err("% A network group holds network-object and group-object lines".into()),
    }
}

/// `icmp permit|deny SOURCE [TYPE] IFNAME`, from the words after `permit|deny`.
fn parse_icmp_rule(d: &Device, permit: &str, words: &[&str]) -> Result<IcmpRule, String> {
    let permit = match permit.to_ascii_lowercase().as_str() {
        "permit" => true,
        "deny" => false,
        _ => return Err("% An icmp rule starts with permit or deny".into()),
    };
    let (ifname, mid) = words.split_last().ok_or("% Give the source and the interface: icmp permit any inside")?;
    port_of(d, ifname)?;
    let f = &d.firewall;
    let try_parse = |addr: &[&str], ty: Option<&str>| {
        let mut w: Vec<&str> = vec!["permit", "icmp"];
        w.extend_from_slice(addr);
        w.push("any");
        w.extend(ty);
        FwAce::parse(f, &w)
    };
    let typed = match mid.split_last() {
        Some((ty, addr)) if !addr.is_empty() && fw::icmp_type_number(ty).is_some() => try_parse(addr, Some(ty)).ok(),
        _ => None,
    };
    let ace = match typed {
        Some(a) => a,
        None => try_parse(mid, None).map_err(|e| format!("% {e}"))?,
    };
    Ok(IcmpRule { permit, src: ace.src, icmp_type: ace.icmp_type, nameif: (*ifname).to_string() })
}

/// `source static|dynamic REAL MAPPED [destination static MAPPED REAL]`, from the words after
/// `source`.
fn parse_twice(d: &Device, pair: &str, words: &[&str]) -> Result<TwiceNat, String> {
    let (real_if, mapped_if) = nat_pair(d, pair)?;
    let f = &d.firewall;
    let (kind, src_real, src_mapped, rest) = match words {
        [k, r, m, rest @ ..] => (k.to_ascii_lowercase(), *r, *m, rest),
        _ => return Err("% Give static|dynamic, the real object and the mapped one (or interface)".into()),
    };
    let dynamic = match kind.as_str() {
        "static" => false,
        "dynamic" => true,
        _ => return Err(format!("% `{kind}`: static or dynamic")),
    };
    let has_addr = |n: &str| f.object(n).is_some_and(|o| o.addr.is_some()) || f.net_group(n).is_some();
    if !has_addr(src_real) {
        return Err(format!("% {src_real} is not a network object or group with addresses"));
    }
    if !dynamic && f.object(src_real).is_none() {
        return Err("% A static rule's real side is a network object (groups only for dynamic rules)".into());
    }
    let interface = src_mapped.eq_ignore_ascii_case("interface");
    if interface && !dynamic {
        return Err("% `interface` is for dynamic rules: the interface's address is shared by port translation".into());
    }
    if !interface && !f.object(src_mapped).is_some_and(|o| o.addr.is_some()) {
        return Err(format!("% {src_mapped} is not a network object with an address"));
    }
    let dst = match rest {
        [] => None,
        [dw, sw, m, r] if dw.eq_ignore_ascii_case("destination") && sw.eq_ignore_ascii_case("static") => {
            for n in [m, r] {
                if !f.object(n).is_some_and(|o| o.addr.is_some()) {
                    return Err(format!("% {n} is not a network object with an address"));
                }
            }
            Some(((*m).to_string(), (*r).to_string()))
        }
        _ => return Err("% After the source: destination static MAPPED-OBJ REAL-OBJ".into()),
    };
    Ok(TwiceNat {
        real_if,
        mapped_if,
        dynamic,
        src_real: src_real.to_string(),
        src_mapped: if interface { "interface".into() } else { src_mapped.to_string() },
        dst,
    })
}

pub(super) fn command(c: &mut Ctx<'_>, id: Id, args: &[String], line: &str) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let lwords: Vec<String> = line.split_whitespace().map(str::to_ascii_lowercase).collect();
    let has_word = |w: &str| lwords.iter().any(|x| x == w);
    match id {
        Id::FwNameif => {
            let name = arg(0);
            if !valid_name(name) || name.eq_ignore_ascii_case("any") || name.eq_ignore_ascii_case("interface") {
                return Err(bad_name("an interface", name));
            }
            let ifs = mode_ifaces(c)?;
            let [port] = ifs.as_slice() else { return Err("% Name one interface at a time".into()) };
            let d = c.dm()?;
            if let Some(other) = d.firewall.port_of(name).filter(|p| p != port) {
                return Err(format!("% {name} already names {other}"));
            }
            let old = d.firewall.iface(port).and_then(|i| i.nameif.clone());
            let i = d.firewall.iface_mut(port).ok_or("% Too many interfaces")?;
            i.nameif = Some(name.to_string());
            let mut msg = String::new();
            if old.is_none() {
                i.level = if name.eq_ignore_ascii_case("inside") { 100 } else { 0 };
                msg = format!("{name} starts at security level {} (change it with security-level)", i.level);
            }
            // Rules, ranges and NAT that named the old name follow the new one.
            if let Some(old) = old.filter(|o| o != name) {
                let f = &mut d.firewall;
                let follow = |s: &mut String| {
                    if s.eq_ignore_ascii_case(&old) {
                        *s = name.to_string();
                    }
                };
                f.groups.iter_mut().for_each(|g| follow(&mut g.nameif));
                f.dhcpd.iter_mut().for_each(|r| follow(&mut r.nameif));
                f.icmp_rules.iter_mut().for_each(|r| follow(&mut r.nameif));
                for n in f.objects.iter_mut().filter_map(|o| o.nat.as_mut()) {
                    follow(&mut n.real_if);
                    follow(&mut n.mapped_if);
                }
                for r in &mut f.twice {
                    follow(&mut r.real_if);
                    follow(&mut r.mapped_if);
                }
            }
            Ok(msg)
        }
        Id::FwNoNameif => {
            for port in mode_ifaces(c)? {
                let d = c.dm()?;
                let Some(old) = d.firewall.iface(&port).and_then(|i| i.nameif.clone()) else { continue };
                if let Some(i) = d.firewall.iface_mut(&port) {
                    i.nameif = None;
                    i.level = 0;
                }
                forget_nameif(&mut d.firewall, &old);
            }
            Ok(String::new())
        }
        Id::FwSecurityLevel | Id::FwNoSecurityLevel => {
            let level: u8 =
                if id == Id::FwSecurityLevel { arg(0).parse().ok().filter(|l| *l <= 100).ok_or("% A security level is 0-100")? } else { 0 };
            for port in mode_ifaces(c)? {
                c.dm()?.firewall.iface_mut(&port).ok_or("% Too many interfaces")?.level = level;
            }
            Ok(String::new())
        }
        Id::FwRoute | Id::FwNoRoute => {
            let net = Ipv4Net::with_mask(parse_ip(arg(1)).map_err(|e| format!("% {e}"))?, parse_ip(arg(2)).map_err(|e| format!("% {e}"))?)
                .map_err(|e| format!("% {e}"))?;
            if net.network() != net {
                return Err(format!("% {net} has host bits set; the network is {}", net.network()));
            }
            let gw = parse_ip(arg(3)).map_err(|e| format!("% {e}"))?;
            let distance: u8 = match arg(4) {
                "" => 1,
                w => w.parse().ok().filter(|n| *n >= 1).ok_or("% A distance is 1-255")?,
            };
            let d = c.dm()?;
            let route = StaticRoute { net, via: NextHop::Address(gw), distance: None };
            if id == Id::FwNoRoute {
                let before = d.routes.len();
                d.routes.retain(|r| *r != route);
                d.firewall.route_distances.retain(|r| !(r.net == net && r.gateway == gw));
                return if d.routes.len() == before { Err("% No such route".into()) } else { Ok(String::new()) };
            }
            let ifnet = net_of(d, arg(0))?;
            if !ifnet.contains(gw) || ifnet.addr == gw {
                return Err(format!("% The gateway {gw} is not a neighbour on {} ({})", arg(0), ifnet.network()));
            }
            if !d.routes.contains(&route) {
                if d.routes.len() >= MAX_ROUTES {
                    return Err(format!("% At most {MAX_ROUTES} static routes"));
                }
                d.routes.push(route);
                d.routes.sort();
            }
            let ds = &mut d.firewall.route_distances;
            ds.retain(|r| !(r.net == net && r.gateway == gw));
            if distance != 1 {
                ds.push(RouteDistance { net, gateway: gw, distance });
            }
            Ok(String::new())
        }
        Id::FwAccessList => {
            let name = arg(0);
            if !valid_name(name) {
                return Err(bad_name("an access list", name));
            }
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            let d = c.dm()?;
            let ace = FwAce::parse(&d.firewall, &words).map_err(|e| format!("% {e}"))?;
            let list = acl_mut(&mut d.firewall, name)?;
            if list.entries.contains(&ace) {
                return Ok(String::new());
            }
            if list.entries.len() >= MAX_ENTRIES {
                return Err(format!("% At most {MAX_ENTRIES} rules per list"));
            }
            list.entries.push(ace);
            Ok(String::new())
        }
        Id::FwAclRemark | Id::FwNoAclRemark => {
            let (name, text) = (arg(0), arg(1).trim());
            if !valid_name(name) {
                return Err(bad_name("an access list", name));
            }
            if text.is_empty() || text.chars().count() > MAX_REMARK {
                return Err(format!("% A remark is 1 to {MAX_REMARK} characters"));
            }
            let f = &mut c.dm()?.firewall;
            if id == Id::FwNoAclRemark {
                let list = f.acls.iter_mut().find(|a| a.name == name).ok_or_else(|| format!("% No access list {name}"))?;
                let before = list.remarks.len();
                list.remarks.retain(|r| r.text != text);
                if list.remarks.len() == before {
                    return Err(format!("% {name} has no such remark"));
                }
                if list.entries.is_empty() && list.remarks.is_empty() {
                    f.acls.retain(|a| a.name != name);
                    f.groups.retain(|g| g.acl != name);
                }
                return Ok(String::new());
            }
            let list = acl_mut(f, name)?;
            if list.remarks.len() >= MAX_ENTRIES {
                return Err(format!("% At most {MAX_ENTRIES} remarks per list"));
            }
            list.remarks.push(FwRemark { before: list.entries.len(), text: text.to_string() });
            Ok(String::new())
        }
        Id::FwNoAccessList => {
            let name = arg(0);
            let f = &mut c.dm()?.firewall;
            if f.acl(name).is_none() {
                return Err(format!("% No access list {name}"));
            }
            f.acls.retain(|a| a.name != name);
            let bound = f.groups.iter().any(|g| g.acl == name);
            f.groups.retain(|g| g.acl != name);
            Ok(if bound { format!("{name} deleted, and unbound from its interfaces") } else { String::new() })
        }
        Id::FwNoAccessListEntry => {
            let name = arg(0);
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            let f = &mut c.dm()?.firewall;
            let ace = FwAce::parse(f, &words).map_err(|e| format!("% {e}"))?;
            let list = f.acls.iter_mut().find(|a| a.name == name).ok_or_else(|| format!("% No access list {name}"))?;
            let at = list.entries.iter().position(|e| *e == ace).ok_or_else(|| format!("% {name} has no such rule"))?;
            list.remove_entry(at);
            // A list without rules or remarks is gone, and so is its binding.
            if list.entries.is_empty() && list.remarks.is_empty() {
                f.acls.retain(|a| a.name != name);
                f.groups.retain(|g| g.acl != name);
            }
            Ok(String::new())
        }
        Id::FwAccessGroup | Id::FwNoAccessGroup => {
            let (acl, ifname) = (arg(0), arg(1));
            let out = has_word("out");
            let d = c.dm()?;
            if id == Id::FwNoAccessGroup {
                let before = d.firewall.groups.len();
                d.firewall.groups.retain(|g| !(g.acl == acl && g.out == out && g.nameif.eq_ignore_ascii_case(ifname)));
                return if d.firewall.groups.len() == before { Err(format!("% {acl} is not bound to {ifname} that way")) } else { Ok(String::new()) };
            }
            if d.firewall.acl(acl).is_none() {
                return Err(format!("% No access list {acl} (add a rule to it first)"));
            }
            port_of(d, ifname)?;
            let f = &mut d.firewall;
            f.groups.retain(|g| !(g.out == out && g.nameif.eq_ignore_ascii_case(ifname)));
            if f.groups.len() >= MAX_LISTS {
                return Err("% Too many access groups".into());
            }
            f.groups.push(AccessGroup { acl: acl.to_string(), nameif: ifname.to_string(), out });
            Ok(String::new())
        }
        Id::FwObjectNetwork => {
            let name = arg(0);
            if !valid_name(name) {
                return Err(bad_name("an object", name));
            }
            let f = &mut c.dm()?.firewall;
            if f.object(name).is_none() {
                if f.name_taken(name) {
                    return Err(format!("% {name} already names an object group"));
                }
                if f.objects.len() >= MAX_OBJECTS {
                    return Err(format!("% At most {MAX_OBJECTS} network objects"));
                }
                f.objects.push(NetObject { name: name.to_string(), addr: None, nat: None });
            }
            c.cli.mode = Mode::Object(name.to_string());
            Ok(String::new())
        }
        Id::FwNoObjectNetwork => {
            let name = arg(0);
            let f = &mut c.dm()?.firewall;
            if f.object(name).is_none() {
                return Err(format!("% No network object {name}"));
            }
            if let Some(user) = f.user_of(name) {
                return Err(format!("% {name} is used by {user}; remove that first"));
            }
            f.objects.retain(|o| o.name != name);
            Ok(String::new())
        }
        Id::FwObjHost | Id::FwObjSubnet | Id::FwObjNat | Id::FwObjNoNat => object_line(c, id, args, line),
        Id::FwObjectGroup => {
            let name = arg(0);
            if !valid_name(name) {
                return Err(bad_name("an object group", name));
            }
            let service = has_word("service");
            let proto = match arg(1) {
                "" => None,
                p => Some(SvcProto::from_name(p).ok_or_else(|| format!("% `{p}`: tcp, udp or tcp-udp (or nothing, for a group of services)"))?),
            };
            let f = &mut c.dm()?.firewall;
            let (is_net, is_svc) = (f.net_group(name).is_some(), f.svc_group(name).is_some());
            if (service && is_net) || (!service && is_svc) || f.object(name).is_some() {
                return Err(format!("% {name} already names another kind of object"));
            }
            if f.net_groups.len() + f.svc_groups.len() >= MAX_OBJECTS && !is_net && !is_svc {
                return Err(format!("% At most {MAX_OBJECTS} object groups"));
            }
            if service {
                match f.svc_groups.iter().find(|g| g.name == name) {
                    Some(g) if g.proto != proto => {
                        let was = g.proto.map_or("services", |p| p.name());
                        return Err(format!("% {name} is a group of {was}; delete it to make it another kind"));
                    }
                    Some(_) => {}
                    None => f.svc_groups.push(SvcGroup { name: name.to_string(), proto, members: Vec::new() }),
                }
            } else if !is_net {
                f.net_groups.push(NetGroup { name: name.to_string(), members: Vec::new() });
            }
            c.cli.mode = Mode::ObjectGroup(name.to_string(), service);
            Ok(String::new())
        }
        Id::FwNoObjectGroup => {
            let name = arg(0);
            let service = has_word("service");
            let f = &mut c.dm()?.firewall;
            let exists = if service { f.svc_group(name).is_some() } else { f.net_group(name).is_some() };
            if !exists {
                return Err(format!("% No {} object group {name}", if service { "service" } else { "network" }));
            }
            if let Some(user) = f.user_of(name) {
                return Err(format!("% {name} is used by {user}; remove that first"));
            }
            f.net_groups.retain(|g| g.name != name);
            f.svc_groups.retain(|g| g.name != name);
            Ok(String::new())
        }
        Id::FwGrpMember | Id::FwGrpNoMember => {
            let Mode::ObjectGroup(name, service) = c.cli.mode.clone() else {
                return Err("% Not in an object group (`object-group network|service NAME`)".into());
            };
            let words: Vec<&str> = line.split_whitespace().collect();
            let words = if id == Id::FwGrpNoMember { words.get(1..).unwrap_or_default() } else { words.as_slice() };
            let f = &mut c.dm()?.firewall;
            let member = parse_member(f, service, &name, words)?;
            // Nesting that comes back around would loop.
            if let Member::Net(NetMember::Group(g)) | Member::Svc(SvcMember::Group(g)) = &member
                && contains_group(f, g, &name, service, 0)
            {
                return Err(format!("% {g} already holds {name}: groups can't hold each other"));
            }
            match member {
                Member::Net(m) => {
                    let g = f.net_groups.iter_mut().find(|g| g.name == name).ok_or("% The group was deleted")?;
                    edit_members(&mut g.members, m, id == Id::FwGrpNoMember)
                }
                Member::Svc(m) => {
                    let g = f.svc_groups.iter_mut().find(|g| g.name == name).ok_or("% The group was deleted")?;
                    edit_members(&mut g.members, m, id == Id::FwGrpNoMember)
                }
            }
        }
        Id::FwSameSecurity | Id::FwNoSameSecurity => {
            let f = &mut c.dm()?.firewall;
            if has_word("intra-interface") {
                f.same_security_intra = id == Id::FwSameSecurity;
            } else {
                f.same_security = id == Id::FwSameSecurity;
            }
            Ok(String::new())
        }
        Id::FwIcmp | Id::FwNoIcmp => {
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            let d = c.dm()?;
            let rule = parse_icmp_rule(d, arg(0), &words)?;
            let rules = &mut d.firewall.icmp_rules;
            if id == Id::FwNoIcmp {
                let before = rules.len();
                rules.retain(|r| *r != rule);
                return if rules.len() == before { Err("% No such icmp rule".into()) } else { Ok(String::new()) };
            }
            if !rules.contains(&rule) {
                if rules.len() >= MAX_ENTRIES {
                    return Err(format!("% At most {MAX_ENTRIES} icmp rules"));
                }
                rules.push(rule);
            }
            Ok(String::new())
        }
        Id::FwTwiceNat | Id::FwNoTwiceNat => {
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            let d = c.dm()?;
            let rule = parse_twice(d, arg(0), &words)?;
            let rules = &mut d.firewall.twice;
            if id == Id::FwNoTwiceNat {
                let before = rules.len();
                rules.retain(|r| *r != rule);
                return if rules.len() == before { Err("% No such NAT rule".into()) } else { Ok(String::new()) };
            }
            if !rules.contains(&rule) {
                if rules.len() >= MAX_ENTRIES {
                    return Err(format!("% At most {MAX_ENTRIES} twice NAT rules"));
                }
                rules.push(rule);
            }
            Ok(String::new())
        }
        Id::FwClassMap => {
            let name = arg(0);
            if !valid_name(name) || name.eq_ignore_ascii_case("class-default") {
                return Err(bad_name("a class map", name));
            }
            let f = &mut c.dm()?.firewall;
            if !f.class_maps.iter().any(|m| m.name == name) {
                if f.class_maps.len() >= MAX_POLICY {
                    return Err(format!("% At most {MAX_POLICY} class maps"));
                }
                f.class_maps.push(FwClassMap { name: name.to_string(), matches: None });
            }
            c.cli.mode = Mode::ClassMap(name.to_string());
            Ok(String::new())
        }
        Id::FwNoClassMap => {
            let name = arg(0);
            let f = &mut c.dm()?.firewall;
            if !f.class_maps.iter().any(|m| m.name == name) {
                return Err(format!("% No class map {name}"));
            }
            if let Some(p) = f.policy_maps.iter().find(|p| p.classes.iter().any(|k| k.class == name)) {
                return Err(format!("% Policy map {} uses {name}; remove the class there first", p.name));
            }
            f.class_maps.retain(|m| m.name != name);
            Ok(String::new())
        }
        Id::FwClassMatch | Id::FwNoClassMatch => {
            let Mode::ClassMap(name) = c.cli.mode.clone() else { return Err("% Not in a class map".into()) };
            let what = if has_word("default-inspection-traffic") { "default-inspection-traffic" } else { "any" };
            let m = c.dm()?.firewall.class_maps.iter_mut().find(|m| m.name == name).ok_or("% The class map was deleted")?;
            m.matches = (id == Id::FwClassMatch).then(|| what.to_string());
            Ok(String::new())
        }
        Id::FwPolicyMap => {
            let name = arg(0);
            if !valid_name(name) {
                return Err(bad_name("a policy map", name));
            }
            let f = &mut c.dm()?.firewall;
            if !f.policy_maps.iter().any(|p| p.name == name) {
                if f.policy_maps.len() >= MAX_POLICY {
                    return Err(format!("% At most {MAX_POLICY} policy maps"));
                }
                f.policy_maps.push(FwPolicyMap { name: name.to_string(), classes: Vec::new() });
            }
            c.cli.mode = Mode::PolicyMap(name.to_string());
            Ok(String::new())
        }
        Id::FwNoPolicyMap => {
            let name = arg(0);
            let f = &mut c.dm()?.firewall;
            if !f.policy_maps.iter().any(|p| p.name == name) {
                return Err(format!("% No policy map {name}"));
            }
            if f.service_policy.as_deref() == Some(name) {
                return Err(format!("% {name} is the global policy; `no service-policy {name} global` first"));
            }
            f.policy_maps.retain(|p| p.name != name);
            Ok(String::new())
        }
        Id::FwPolicyClass | Id::FwNoPolicyClass => {
            let policy = match &c.cli.mode {
                Mode::PolicyMap(p) | Mode::PolicyClass(p, _) => p.clone(),
                _ => return Err("% Not in a policy map".into()),
            };
            let class = arg(0);
            let f = &mut c.dm()?.firewall;
            let known = class.eq_ignore_ascii_case("class-default") || f.class_maps.iter().any(|m| m.name == class);
            let pm = f.policy_maps.iter_mut().find(|p| p.name == policy).ok_or("% The policy map was deleted")?;
            if id == Id::FwNoPolicyClass {
                let before = pm.classes.len();
                pm.classes.retain(|k| k.class != class);
                let gone = pm.classes.len() != before;
                c.cli.mode = Mode::PolicyMap(policy);
                return if gone { Ok(String::new()) } else { Err(format!("% The policy has no class {class}")) };
            }
            if !known {
                return Err(format!("% No class map {class} (create it with `class-map {class}`, or use class-default)"));
            }
            let class = if class.eq_ignore_ascii_case("class-default") { "class-default" } else { class };
            if !pm.classes.iter().any(|k| k.class == class) {
                if pm.classes.len() >= MAX_POLICY {
                    return Err(format!("% At most {MAX_POLICY} classes in a policy"));
                }
                pm.classes.push(FwPolicyClass { class: class.to_string(), inspect: Vec::new(), decrement_ttl: false });
            }
            c.cli.mode = Mode::PolicyClass(policy, class.to_string());
            Ok(String::new())
        }
        Id::FwPolicyAction | Id::FwNoPolicyAction => {
            let Mode::PolicyClass(policy, class) = c.cli.mode.clone() else { return Err("% Not in a policy map's class".into()) };
            let on = id == Id::FwPolicyAction;
            let inspect = if has_word("inspect") {
                let what = arg(0).split_whitespace().map(str::to_ascii_lowercase).collect::<Vec<_>>().join(" ");
                if !fw::INSPECTIONS.contains(&what.as_str()) {
                    return Err(format!("% The firewall inspects: {}", fw::INSPECTIONS.join(", ")));
                }
                Some(what)
            } else {
                None
            };
            let f = &mut c.dm()?.firewall;
            let k = f
                .policy_maps
                .iter_mut()
                .find(|p| p.name == policy)
                .and_then(|p| p.classes.iter_mut().find(|k| k.class == class))
                .ok_or("% The class was removed from the policy")?;
            match inspect {
                Some(what) => {
                    k.inspect.retain(|i| *i != what);
                    if on {
                        k.inspect.push(what);
                    }
                }
                None => k.decrement_ttl = on,
            }
            Ok(String::new())
        }
        Id::FwServicePolicy | Id::FwNoServicePolicy => {
            let name = arg(0);
            let f = &mut c.dm()?.firewall;
            if id == Id::FwNoServicePolicy {
                if f.service_policy.as_deref() != Some(name) {
                    return Err(format!("% {name} is not the global policy"));
                }
                f.service_policy = None;
                return Ok(String::new());
            }
            if !f.policy_maps.iter().any(|p| p.name == name) {
                return Err(format!("% No policy map {name}"));
            }
            f.service_policy = Some(name.to_string());
            Ok(String::new())
        }
        Id::FwDhcpdAddress => {
            let (start, end) = parse_range(arg(0))?;
            let ifname = arg(1);
            let d = c.dm()?;
            let net = net_of(d, ifname)?;
            if !net.contains(start) || !net.contains(end) {
                return Err(format!("% The range must lie in {ifname}'s subnet {}", net.network()));
            }
            if (u32::from(start)..=u32::from(end)).contains(&u32::from(net.addr)) {
                return Err(format!("% The range holds {ifname}'s own address {}", net.addr));
            }
            if u32::from(end) - u32::from(start) >= MAX_DHCPD_ADDRESSES {
                return Err(format!("% A range holds at most {MAX_DHCPD_ADDRESSES} addresses"));
            }
            let f = &mut d.firewall;
            let enabled = f.dhcpd.iter().find(|r| r.nameif.eq_ignore_ascii_case(ifname)).is_some_and(|r| r.enabled);
            f.dhcpd.retain(|r| !r.nameif.eq_ignore_ascii_case(ifname));
            if f.dhcpd.len() >= MAX_DHCPD {
                return Err(format!("% At most {MAX_DHCPD} DHCP ranges"));
            }
            f.dhcpd.push(Dhcpd { nameif: ifname.to_string(), start, end, enabled });
            Ok(String::new())
        }
        Id::FwNoDhcpdAddress => {
            let (start, end) = parse_range(arg(0))?;
            let f = &mut c.dm()?.firewall;
            let before = f.dhcpd.len();
            f.dhcpd.retain(|r| !(r.nameif.eq_ignore_ascii_case(arg(1)) && r.start == start && r.end == end));
            if f.dhcpd.len() == before { Err("% No such DHCP range".into()) } else { Ok(String::new()) }
        }
        Id::FwDhcpdEnable | Id::FwNoDhcpdEnable => {
            let ifname = arg(0);
            let d = c.dm()?;
            port_of(d, ifname)?;
            let r = d
                .firewall
                .dhcpd
                .iter_mut()
                .find(|r| r.nameif.eq_ignore_ascii_case(ifname))
                .ok_or_else(|| format!("% Give {ifname} a range first: dhcpd address A.B.C.D-A.B.C.D {ifname}"))?;
            r.enabled = id == Id::FwDhcpdEnable;
            Ok(String::new())
        }
        Id::FwDhcpdDns => {
            c.dm()?.firewall.dhcpd_dns = Some(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?);
            Ok(String::new())
        }
        Id::FwNoDhcpdDns => {
            c.dm()?.firewall.dhcpd_dns = None;
            Ok(String::new())
        }
        Id::FwTimeout => {
            let secs = parse_duration(arg(1)).ok_or("% Give the time as H:MM:SS (from 0:00:01)")?;
            let t = &mut c.dm()?.firewall.timeouts;
            match arg(0).to_ascii_lowercase().as_str() {
                "conn" => t.conn = secs,
                "udp" => t.udp = secs,
                "icmp" => t.icmp = secs,
                other => return Err(format!("% `{other}`: conn, udp or icmp")),
            }
            Ok(String::new())
        }
        Id::FwShowNat => Ok(show_nat(c)),
        Id::FwShowNatDetail => {
            let d = c.d()?.clone();
            Ok(show_nat_detail(&d, &c.sim.fw_state(&d)))
        }
        Id::FwShowXlate => {
            let d = c.d()?.clone();
            Ok(show_xlate(&d, &c.sim.fw_state(&d), c.sim.now_us()))
        }
        Id::FwShowConn => {
            let d = c.d()?.clone();
            Ok(show_conn(&c.sim.fw_state(&d), c.sim.now_us()))
        }
        Id::FwShowAccessList => show_access_list(c, args.first().map(String::as_str)),
        Id::FwShowIntIpBrief => Ok(show_ip_brief(c.t, c.d()?)),
        Id::FwShowRoute => Ok(show_route(c.t, c.d()?)),
        Id::FwShowNameif => Ok(show_nameif(c.d()?)),
        Id::FwShowDhcpdBinding => Ok(show_bindings(c)),
        Id::FwShowObjects => Ok(show_objects(c.d()?)),
        Id::FwShowObjectGroup => Ok(show_object_groups(c.d()?)),
        Id::FwShowServicePolicy => {
            let d = c.d()?.clone();
            Ok(show_service_policy(&d, &c.sim.fw_state(&d)))
        }
        Id::FwClearConn => {
            c.sim.fw_clear_conns(c.dev);
            Ok(String::new())
        }
        Id::FwClearXlate => {
            c.sim.fw_clear_xlates(c.dev);
            Ok(String::new())
        }
        Id::FwClearAclCounters => {
            c.sim.clear_acl_hits(c.dev);
            Ok(String::new())
        }
        _ => Err("% Not a firewall command".into()),
    }
}

/// The access list called `name`, created when missing.
fn acl_mut<'a>(f: &'a mut fw::Firewall, name: &str) -> Result<&'a mut FwAcl, String> {
    if f.acl(name).is_none() {
        if f.acls.len() >= MAX_LISTS {
            return Err(format!("% At most {MAX_LISTS} access lists"));
        }
        f.acls.push(FwAcl { name: name.to_string(), entries: Vec::new(), remarks: Vec::new() });
    }
    f.acls.iter_mut().find(|a| a.name == name).ok_or_else(|| "% No such list".to_string())
}

/// Whether group `g` holds group `target`, directly or deeper.
fn contains_group(f: &fw::Firewall, g: &str, target: &str, service: bool, depth: u8) -> bool {
    if g == target {
        return true;
    }
    if depth > fw::MAX_NESTING {
        return true;
    }
    let nested: Vec<&str> = if service {
        f.svc_group(g).map(|x| x.members.iter().filter_map(|m| if let SvcMember::Group(n) = m { Some(n.as_str()) } else { None }).collect())
    } else {
        f.net_group(g).map(|x| x.members.iter().filter_map(|m| if let NetMember::Group(n) = m { Some(n.as_str()) } else { None }).collect())
    }
    .unwrap_or_default();
    nested.iter().any(|n| contains_group(f, n, target, service, depth + 1))
}

fn edit_members<T: PartialEq>(members: &mut Vec<T>, m: T, remove: bool) -> R {
    if remove {
        let before = members.len();
        members.retain(|x| *x != m);
        return if members.len() == before { Err("% The group has no such member".into()) } else { Ok(String::new()) };
    }
    if !members.contains(&m) {
        if members.len() >= MAX_MEMBERS {
            return Err(format!("% At most {MAX_MEMBERS} members in a group"));
        }
        members.push(m);
    }
    Ok(String::new())
}

/// `host`, `subnet` and `nat` inside `object network NAME`.
fn object_line(c: &mut Ctx<'_>, id: Id, args: &[String], line: &str) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let Mode::Object(name) = c.cli.mode.clone() else { return Err("% Not in a network object (`object network NAME`)".into()) };
    let nat = if id == Id::FwObjNat {
        let (real_if, mapped_if) = nat_pair(c.d()?, arg(0))?;
        let dynamic = line.split_whitespace().any(|w| w.eq_ignore_ascii_case("dynamic"));
        let target = arg(1);
        let kind = match (dynamic, target.eq_ignore_ascii_case("interface")) {
            (true, true) => NatKind::DynamicInterface,
            (true, false) => NatKind::DynamicAddress(parse_ip(target).map_err(|e| format!("% {e} (or `interface`)"))?),
            (false, _) => NatKind::Static(parse_ip(target).map_err(|e| format!("% {e}"))?),
        };
        Some(ObjNat { real_if, mapped_if, kind })
    } else {
        None
    };
    let o = c.dm()?.firewall.objects.iter_mut().find(|o| o.name == name).ok_or("% The object was deleted")?;
    match id {
        Id::FwObjHost => o.addr = Some(ObjAddr::Host(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?)),
        Id::FwObjSubnet => {
            let n = Ipv4Net::with_mask(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?, parse_ip(arg(1)).map_err(|e| format!("% {e}"))?)
                .map_err(|e| format!("% {e}"))?;
            if n.network() != n {
                return Err(format!("% {} has host bits set; the subnet is {}", arg(0), n.network().addr));
            }
            o.addr = Some(ObjAddr::Subnet(n));
        }
        Id::FwObjNat => {
            if o.addr.is_none() {
                return Err("% Give the object its addresses first (host or subnet)".into());
            }
            o.nat = nat;
        }
        _ => o.nat = None,
    }
    Ok(String::new())
}

fn show_nameif(d: &Device) -> String {
    let mut s = format!("{:<22} {:<14} {:<15} {}\n", "Interface", "Name", "Security level", "VLAN");
    for (name, _, _, _, vlan) in all_ifaces(d) {
        let (n, level) = d.firewall.zone(&name).map_or(("-".to_string(), "-".to_string()), |(n, l)| (n.to_string(), l.to_string()));
        let vlan = match vlan {
            Some(0) => "none yet".to_string(),
            Some(v) => v.to_string(),
            None => "-".to_string(),
        };
        let _ = writeln!(s, "{:<22} {:<14} {level:<15} {vlan}", name, n);
    }
    s.trim_end().to_string()
}

fn show_ip_brief(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<22} {:<12} {:<18} {:<6} {}\n", "Interface", "Name", "IP address", "Admin", "Link");
    for (name, ip, shut, port, _) in all_ifaces(d) {
        let n = d.firewall.zone(&name).map_or("-", |(n, _)| n);
        let addr = ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let parent_shut = d.ports.get(port).is_some_and(|p| p.shutdown);
        let up = !shut && !parent_shut && netlabcraft_sim::port_up(t, End { device: d.id, port });
        let _ = writeln!(s, "{:<22} {:<12} {:<18} {:<6} {}", name, n, addr, if shut { "down" } else { "up" }, if up { "up" } else { "down" });
    }
    s.trim_end().to_string()
}

/// The routing table, interfaces by name and static routes with their distance.
fn show_route(t: &Topology, d: &Device) -> String {
    let table = netlabcraft_sim::routing_table(t, d);
    let ifname = |i: &Option<String>| i.as_deref().map(|p| d.firewall.zone(p).map_or(p.to_string(), |(n, _)| n.to_string()));
    let via_if = |gw: Ipv4Addr| {
        all_ifaces(d).into_iter().find(|i| i.1.is_some_and(|n| n.contains(gw))).and_then(|i| d.firewall.zone(&i.0).map(|(n, _)| n.to_string()))
    };
    let mut s = String::from("Codes: C connected, L local, S static (distance/metric in brackets)\n\n");
    match table.iter().find(|r| r.net.prefix == 0 && r.via.is_some()) {
        Some(r) => {
            let gw = r.via.map_or(String::new(), |v| v.to_string());
            let on = r.via.and_then(via_if).map(|n| format!(" on {n}")).unwrap_or_default();
            let _ = writeln!(s, "Gateway of last resort: {gw}{on}\n");
        }
        None => s.push_str("No gateway of last resort\n\n"),
    }
    if table.is_empty() {
        s.push_str("(no routes: no named interface is up with an address)");
        return s;
    }
    for r in &table {
        let line = match (r.code, r.via) {
            ('C', _) => format!("C    {:<20} connected, {}", r.net.to_string(), ifname(&r.iface).unwrap_or_default()),
            ('L', _) => format!("L    {:<20} local address, {}", r.net.to_string(), ifname(&r.iface).unwrap_or_default()),
            (code, Some(gw)) => {
                let dist = if code == 'S' { d.firewall.route_distance(r.net, gw) } else { 1 };
                let on = via_if(gw).map(|n| format!(", {n}")).unwrap_or_default();
                format!("{code}    {:<20} [{dist}/0] via {gw}{on}", r.net.to_string())
            }
            (code, None) => format!("{code}    {:<20} {}", r.net.to_string(), ifname(&r.iface).unwrap_or_default()),
        };
        let _ = writeln!(s, "{line}");
    }
    s.trim_end().to_string()
}

fn show_objects(d: &Device) -> String {
    if d.firewall.objects.is_empty() {
        return "No network objects.".into();
    }
    let mut s = String::new();
    for o in &d.firewall.objects {
        let what = o.addr.map_or("(no address yet)".to_string(), |a| a.text());
        let nat = o.nat.as_ref().map(|n| format!(", {}", n.text())).unwrap_or_default();
        let _ = writeln!(s, "object network {}: {what}{nat}", o.name);
    }
    s.trim_end().to_string()
}

fn show_object_groups(d: &Device) -> String {
    let f = &d.firewall;
    if f.net_groups.is_empty() && f.svc_groups.is_empty() {
        return "No object groups.".into();
    }
    let mut s = String::new();
    for g in &f.net_groups {
        let _ = writeln!(s, "object-group network {} ({} member{})", g.name, g.members.len(), if g.members.len() == 1 { "" } else { "s" });
        for m in &g.members {
            let _ = writeln!(s, "  {}", m.text());
        }
    }
    for g in &f.svc_groups {
        let kind = g.proto.map_or("services".to_string(), |p| format!("{} ports", p.name()));
        let _ = writeln!(s, "object-group service {} ({kind}, {} member{})", g.name, g.members.len(), if g.members.len() == 1 { "" } else { "s" });
        for m in &g.members {
            let _ = writeln!(s, "  {}", m.text());
        }
    }
    s.trim_end().to_string()
}

/// An object or group's addresses, for NAT tables.
fn addresses_of(f: &fw::Firewall, name: &str) -> String {
    if name.eq_ignore_ascii_case("interface") {
        return "the interface's address".into();
    }
    match f.object(name).and_then(|o| o.addr) {
        Some(ObjAddr::Host(h)) => h.to_string(),
        Some(ObjAddr::Subnet(n)) => n.to_string(),
        None => format!("group {name}"),
    }
}

fn show_nat(c: &Ctx<'_>) -> String {
    let Ok(d) = c.d() else { return String::new() };
    let hits = c.sim.state(d.id).map(|s| s.fw.nat_hits.clone()).unwrap_or_default();
    let f = &d.firewall;
    let rules: Vec<&NetObject> = f.objects.iter().filter(|o| o.nat.is_some()).collect();
    if rules.is_empty() && f.twice.is_empty() {
        return "No NAT rules (give a network object a `nat` line, or add a twice NAT rule).".into();
    }
    let mut s = String::new();
    if !f.twice.is_empty() {
        s.push_str("Twice NAT, in order:\n");
        for (i, r) in f.twice.iter().enumerate() {
            let used = hits.get(&netlabcraft_sim::firewall::twice_label(i)).copied().unwrap_or(0);
            let _ = writeln!(s, "{:>3}. {}  [used {used} times]", i + 1, r.text());
        }
    }
    if rules.is_empty() {
        return s.trim_end().to_string();
    }
    s.push_str("Object NAT, static rules first:\n");
    for (i, o) in ordered_objects(&rules).iter().enumerate() {
        let Some(n) = &o.nat else { continue };
        let real = o.addr.map_or("?".to_string(), |a| match a {
            ObjAddr::Host(h) => h.to_string(),
            ObjAddr::Subnet(n) => n.to_string(),
        });
        let mapped = match n.kind {
            NatKind::DynamicInterface => format!("{}'s address, ports shared", n.mapped_if),
            NatKind::DynamicAddress(a) => format!("{a}, ports shared"),
            NatKind::Static(a) => a.to_string(),
        };
        let used = hits.get(&o.name).copied().unwrap_or(0);
        let _ = writeln!(s, "{:>3}. ({},{}) object {}: {real} → {mapped}  [used {used} times]", i + 1, n.real_if, n.mapped_if, o.name);
    }
    s.trim_end().to_string()
}

fn ordered_objects<'a>(rules: &[&'a NetObject]) -> Vec<&'a NetObject> {
    let is_static = |o: &&NetObject| o.nat.as_ref().is_some_and(|n| matches!(n.kind, NatKind::Static(_)));
    let mut ordered: Vec<&NetObject> = rules.iter().copied().filter(is_static).collect();
    ordered.extend(rules.iter().copied().filter(|o| !is_static(o)));
    ordered
}

/// `show nat detail`: both sections in the order they are checked, what each rule turns
/// into what, and how often it translated and was undone.
fn show_nat_detail(d: &Device, st: &netlabcraft_sim::firewall::FwState) -> String {
    let f = &d.firewall;
    let count = |key: &str| (st.nat_hits.get(key).copied().unwrap_or(0), st.nat_unhits.get(key).copied().unwrap_or(0));
    let mut s = String::from("Section 1, twice NAT (checked first, in order):\n");
    if f.twice.is_empty() {
        s.push_str("  (none)\n");
    }
    for (i, r) in f.twice.iter().enumerate() {
        let (out, back) = count(&netlabcraft_sim::firewall::twice_label(i));
        let _ = writeln!(s, "{:>3}. {}", i + 1, r.text());
        let how = if r.dynamic { "port translation" } else { "one to one, both directions" };
        let _ = writeln!(s, "       source {} → {} ({how})", addresses_of(f, &r.src_real), addresses_of(f, &r.src_mapped));
        if let Some((m, real)) = &r.dst {
            let _ = writeln!(s, "       destination {} → {}", addresses_of(f, m), addresses_of(f, real));
        }
        let _ = writeln!(s, "       translated {out} times, undone {back} times");
    }
    s.push_str("Section 2, object NAT (static rules first, then dynamic):\n");
    let rules: Vec<&NetObject> = f.objects.iter().filter(|o| o.nat.is_some()).collect();
    if rules.is_empty() {
        s.push_str("  (none)\n");
    }
    for (i, o) in ordered_objects(&rules).iter().enumerate() {
        let Some(n) = &o.nat else { continue };
        let (out, back) = count(&o.name);
        let _ = writeln!(s, "{:>3}. ({},{}) object {}: {}", i + 1, n.real_if, n.mapped_if, o.name, n.text());
        let real = o.addr.map_or("?".to_string(), |a| match a {
            ObjAddr::Host(h) => h.to_string(),
            ObjAddr::Subnet(n) => n.to_string(),
        });
        let mapped = match n.kind {
            NatKind::DynamicInterface => format!("{}'s address (port translation)", n.mapped_if),
            NatKind::DynamicAddress(a) => format!("{a} (port translation)"),
            NatKind::Static(a) => format!("{a} (one to one, both directions)"),
        };
        let _ = writeln!(s, "       source {real} → {mapped}");
        let _ = writeln!(s, "       translated {out} times, undone {back} times");
    }
    s.trim_end().to_string()
}

fn ago(now: u64, then: u64) -> String {
    duration_text(u32::try_from(now.saturating_sub(then) / 1_000_000).unwrap_or(u32::MAX))
}

fn show_xlate(d: &Device, st: &netlabcraft_sim::firewall::FwState, now: u64) -> String {
    let f = &d.firewall;
    let mut statics: Vec<String> = f
        .objects
        .iter()
        .filter_map(|o| {
            let n = o.nat.as_ref()?;
            let NatKind::Static(m) = n.kind else { return None };
            let real = match o.addr? {
                ObjAddr::Host(h) => h.to_string(),
                ObjAddr::Subnet(net) => net.to_string(),
            };
            let mapped = match o.addr? {
                ObjAddr::Host(_) => m.to_string(),
                ObjAddr::Subnet(net) => Ipv4Net::new(m, net.prefix).network().to_string(),
            };
            Some(format!("static   {} {real} ↔ {} {mapped}  (object {})", n.real_if, n.mapped_if, o.name))
        })
        .collect();
    statics.extend(f.twice.iter().enumerate().filter(|(_, r)| !r.dynamic).map(|(i, r)| {
        format!("static   {} {} ↔ {} {}  (twice {})", r.real_if, addresses_of(f, &r.src_real), r.mapped_if, addresses_of(f, &r.src_mapped), i + 1)
    }));
    let mut s = format!(
        "{} translation{} in use: {} static, {} dynamic\n",
        statics.len() + st.xlates.len(),
        if statics.len() + st.xlates.len() == 1 { "" } else { "s" },
        statics.len(),
        st.xlates.len()
    );
    for l in &statics {
        let _ = writeln!(s, "{l}");
    }
    for x in &st.xlates {
        let rule = if x.object.starts_with("twice ") { x.object.clone() } else { format!("object {}", x.object) };
        let _ = writeln!(
            s,
            "dynamic  {} {} {}:{} → {} {}:{}  idle {}  ({rule})",
            netlabcraft_sim::firewall::proto_name(x.protocol),
            x.real_if,
            x.real,
            x.real_port,
            x.mapped_if,
            x.mapped,
            x.mapped_port,
            ago(now, x.last_us),
        );
    }
    s.trim_end().to_string()
}

fn show_conn(st: &netlabcraft_sim::firewall::FwState, now: u64) -> String {
    let n = st.conns.len();
    let mut s = format!("{n} connection{} in use\n", if n == 1 { "" } else { "s" });
    for c in &st.conns {
        let _ = writeln!(
            s,
            "{:<4} {} {}:{} → {} {}:{}  idle {}, up {}, {} packets, {} bytes{}",
            c.protocol_name(),
            c.from_if,
            c.src,
            c.src_port,
            c.to_if,
            c.dst,
            c.dst_port,
            ago(now, c.last_us),
            ago(now, c.started_us),
            c.packets,
            c.bytes,
            if c.closing { ", closing" } else { "" }
        );
    }
    s.trim_end().to_string()
}

fn show_access_list(c: &Ctx<'_>, only: Option<&str>) -> R {
    let d = c.d()?;
    let f = &d.firewall;
    let lists: Vec<&FwAcl> = f.acls.iter().filter(|a| only.is_none_or(|o| a.name == o)).collect();
    if lists.is_empty() {
        return match only {
            Some(o) => Err(format!("% No access list {o}")),
            None => Ok("No access lists.".into()),
        };
    }
    let hits = c.sim.state(d.id).map(|s| s.acl_hits.clone()).unwrap_or_default();
    let mut s = String::new();
    for a in lists {
        let bound: Vec<String> =
            f.groups.iter().filter(|g| g.acl == a.name).map(|g| format!("{} on {}", if g.out { "out" } else { "in" }, g.nameif)).collect();
        let where_ = if bound.is_empty() { "not bound to an interface".to_string() } else { format!("checks traffic {}", bound.join(", ")) };
        let _ = writeln!(s, "access-list {}: {} rule{}, {where_}", a.name, a.entries.len(), if a.entries.len() == 1 { "" } else { "s" });
        for i in 0..=a.entries.len() {
            for r in a.remarks.iter().filter(|r| r.before == i) {
                let _ = writeln!(s, "       -- {}", r.text);
            }
            if let Some(e) = a.entries.get(i) {
                let n = hits.get(&(a.name.clone(), i)).copied().unwrap_or(0);
                let _ = writeln!(s, "  {:>3}. {}  (hits {n})", i + 1, e.text());
            }
        }
        let n = hits.get(&(a.name.clone(), usize::MAX)).copied().unwrap_or(0);
        let _ = writeln!(s, "       anything else is denied  (hits {n})");
    }
    Ok(s.trim_end().to_string())
}

fn show_service_policy(d: &Device, st: &netlabcraft_sim::firewall::FwState) -> String {
    let f = &d.firewall;
    let Some(name) = f.service_policy.as_deref() else {
        return "No global policy (`service-policy NAME global`): ICMP is not inspected, and the firewall leaves TTL alone.".into();
    };
    let Some(pm) = f.policy_maps.iter().find(|p| p.name == name) else { return format!("The global policy {name} no longer exists.") };
    let mut s = format!("Global policy: {name}\n");
    for k in &pm.classes {
        let matched = if k.class == "class-default" {
            "every packet".to_string()
        } else {
            match f.class_maps.iter().find(|m| m.name == k.class).and_then(|m| m.matches.clone()) {
                Some(m) => format!("match {m}"),
                None => "no match line yet: the class catches nothing".into(),
            }
        };
        let _ = writeln!(s, "  class {} ({matched})", k.class);
        for i in &k.inspect {
            let seen = if i == "icmp" { st.inspected_icmp } else { st.icmp_errors };
            let what = if i == "icmp" { "ICMP packets tracked" } else { "ICMP errors let through for their connection" };
            let _ = writeln!(s, "    inspect {i}: {seen} {what}");
        }
        if k.decrement_ttl {
            s.push_str("    set connection decrement-ttl: the firewall counts as a hop\n");
        }
        if k.inspect.is_empty() && !k.decrement_ttl {
            s.push_str("    (no actions)\n");
        }
    }
    s.trim_end().to_string()
}

fn show_bindings(c: &Ctx<'_>) -> String {
    let Ok(d) = c.d() else { return String::new() };
    let leases: Vec<(Ipv4Addr, String, String)> = c
        .sim
        .state(d.id)
        .map(|s| s.leases.iter().map(|(a, l)| (*a, l.mac.to_string(), l.pool.trim_start_matches("dhcpd ").to_string())).collect())
        .unwrap_or_default();
    if leases.is_empty() {
        return "No addresses handed out.".into();
    }
    let mut s = format!("{:<17} {:<19} {}\n", "Address", "Client MAC", "Interface");
    for (a, mac, ifname) in leases {
        let _ = writeln!(s, "{:<17} {:<19} {ifname}", a.to_string(), mac);
    }
    s.trim_end().to_string()
}

/// The running configuration of the firewall, in the order it replays.
pub(crate) fn running_config(d: &Device) -> String {
    let f = &d.firewall;
    let mut s = String::new();
    let _ = writeln!(s, "! NetLabCraft configuration of {} ({})", d.name, d.catalogue_model().label);
    let _ = writeln!(s, "hostname {}", d.name);
    if let Some(p) = &d.access.enable_password {
        let _ = writeln!(s, "enable password {p}");
    }
    let _ = writeln!(s, "!");
    for p in &d.ports {
        let mut blocks = vec![(p.name.clone(), None, p.ip, p.shutdown, p.description.clone())];
        blocks
            .extend(p.subifs.iter().map(|sub| (format!("{}.{}", p.name, sub.number), Some(sub.vlan), sub.ip, sub.shutdown, sub.description.clone())));
        for (name, vlan, ip, shut, description) in blocks {
            let _ = writeln!(s, "interface {name}");
            if !description.is_empty() {
                let _ = writeln!(s, " description {description}");
            }
            if let Some(Some(v)) = vlan {
                let _ = writeln!(s, " vlan {v}");
            }
            if let Some((n, level)) = f.zone(&name) {
                let _ = writeln!(s, " nameif {n}\n security-level {level}");
            }
            if let Some(ip) = ip {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            let _ = writeln!(s, " {}\n!", if shut { "shutdown" } else { "no shutdown" });
        }
    }
    if f.same_security {
        let _ = writeln!(s, "same-security-traffic permit inter-interface");
    }
    if f.same_security_intra {
        let _ = writeln!(s, "same-security-traffic permit intra-interface");
    }
    for o in &f.objects {
        let _ = writeln!(s, "object network {}", o.name);
        if let Some(a) = o.addr {
            let _ = writeln!(s, " {}", a.text());
        }
        if let Some(n) = &o.nat {
            let _ = writeln!(s, " {}", n.text());
        }
    }
    for g in &f.net_groups {
        let _ = writeln!(s, "object-group network {}", g.name);
        for m in &g.members {
            let _ = writeln!(s, " {}", m.text());
        }
    }
    for g in &f.svc_groups {
        let proto = g.proto.map(|p| format!(" {}", p.name())).unwrap_or_default();
        let _ = writeln!(s, "object-group service {}{proto}", g.name);
        for m in &g.members {
            let _ = writeln!(s, " {}", m.text());
        }
    }
    for r in &f.twice {
        let _ = writeln!(s, "{}", r.text());
    }
    for a in &f.acls {
        for l in a.lines() {
            let _ = writeln!(s, "{l}");
        }
    }
    for g in &f.groups {
        let _ = writeln!(s, "access-group {} {} interface {}", g.acl, if g.out { "out" } else { "in" }, g.nameif);
    }
    for r in &f.icmp_rules {
        let _ = writeln!(s, "{}", r.text());
    }
    for r in &d.routes {
        let NextHop::Address(gw) = r.via else { continue };
        let ifname = all_ifaces(d).into_iter().filter(|i| i.1.is_some_and(|n| n.contains(gw))).find_map(|i| f.zone(&i.0).map(|(n, _)| n.to_string()));
        match ifname {
            Some(n) => {
                let _ = writeln!(s, "route {n} {} {} {gw} {}", r.net.addr, r.net.mask(), f.route_distance(r.net, gw));
            }
            None => {
                let _ = writeln!(s, "! route to {} via {gw}: no named interface reaches {gw}", r.net);
            }
        }
    }
    let def = fw::Timeouts::default();
    for (what, now, was) in [("conn", f.timeouts.conn, def.conn), ("udp", f.timeouts.udp, def.udp), ("icmp", f.timeouts.icmp, def.icmp)] {
        if now != was {
            let _ = writeln!(s, "timeout {what} {}", duration_text(now));
        }
    }
    for m in &f.class_maps {
        let _ = writeln!(s, "class-map {}", m.name);
        if let Some(x) = &m.matches {
            let _ = writeln!(s, " match {x}");
        }
    }
    for p in &f.policy_maps {
        let _ = writeln!(s, "policy-map {}", p.name);
        for k in &p.classes {
            let _ = writeln!(s, " class {}", k.class);
            for i in &k.inspect {
                let _ = writeln!(s, "  inspect {i}");
            }
            if k.decrement_ttl {
                let _ = writeln!(s, "  set connection decrement-ttl");
            }
        }
    }
    if let Some(p) = &f.service_policy {
        let _ = writeln!(s, "service-policy {p} global");
    }
    for r in &f.dhcpd {
        let _ = writeln!(s, "dhcpd address {}-{} {}", r.start, r.end, r.nameif);
    }
    if let Some(dns) = f.dhcpd_dns {
        let _ = writeln!(s, "dhcpd dns {dns}");
    }
    for r in f.dhcpd.iter().filter(|r| r.enabled) {
        let _ = writeln!(s, "dhcpd enable {}", r.nameif);
    }
    s.push_str("end");
    s
}
