//! The firewall appliance's commands: named interfaces and security levels, access lists bound
//! to interfaces, network objects with their NAT, static routes by interface name, the DHCP
//! server, timeouts, and the tables (`show conn`, `show xlate`, `show nat`, `show access-list`).

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use packetcraft_core::{Ipv4Net, parse_ip};
use packetcraft_model::firewall::{
    self as fw, AccessGroup, Dhcpd, FwAce, FwAcl, MAX_DHCPD, MAX_DHCPD_ADDRESSES, MAX_ENTRIES, MAX_LISTS, MAX_OBJECTS, NatKind, NetObject, ObjAddr,
    ObjNat, duration_text, parse_duration, valid_name,
};
use packetcraft_model::{Device, End, MAX_ROUTES, NextHop, StaticRoute, Topology};

use super::{Ctx, Mode, R};
use crate::grammar::Id;
use crate::render;

/// The full name of an interface reference, as the firewall keys its settings.
fn ref_name(d: &Device, r: super::IfRef) -> Option<String> {
    let p = d.ports.get(r.port)?;
    Some(r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))
}

fn bad_name(what: &str, name: &str) -> String {
    format!("% `{name}` can't name {what}: use letters, digits, - _ or . (starting with a letter, at most {} characters)", fw::MAX_NAME)
}

/// The interface (port name) carrying a nameif, or an error naming the missing nameif.
fn port_of(d: &Device, nameif: &str) -> Result<String, String> {
    d.firewall.port_of(nameif).map(str::to_string).ok_or_else(|| format!("% No interface is named {nameif} (set one with `nameif {nameif}`)"))
}

/// The address of the interface carrying a nameif.
fn net_of(d: &Device, nameif: &str) -> Result<Ipv4Net, String> {
    let port = port_of(d, nameif)?;
    d.ports.iter().find(|p| p.name == port).and_then(|p| p.ip).ok_or_else(|| format!("% {nameif} ({port}) has no address yet"))
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

/// The interfaces of the current interface mode, by name.
fn mode_ifaces(c: &Ctx<'_>) -> Result<Vec<String>, String> {
    let Mode::Iface(refs) = &c.cli.mode else { return Err("% Not in interface mode".into()) };
    let d = c.d()?;
    Ok(refs.iter().filter_map(|r| ref_name(d, *r)).collect())
}

pub(super) fn command(c: &mut Ctx<'_>, id: Id, args: &[String], line: &str) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::FwNameif => {
            let name = arg(0);
            if !valid_name(name) || name.eq_ignore_ascii_case("any") {
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
                for g in &mut f.groups {
                    if g.nameif.eq_ignore_ascii_case(&old) {
                        g.nameif = name.to_string();
                    }
                }
                for r in &mut f.dhcpd {
                    if r.nameif.eq_ignore_ascii_case(&old) {
                        r.nameif = name.to_string();
                    }
                }
                for n in f.objects.iter_mut().filter_map(|o| o.nat.as_mut()) {
                    for side in [&mut n.real_if, &mut n.mapped_if] {
                        if side.eq_ignore_ascii_case(&old) {
                            *side = name.to_string();
                        }
                    }
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
                d.firewall.groups.retain(|g| !g.nameif.eq_ignore_ascii_case(&old));
                d.firewall.dhcpd.retain(|r| !r.nameif.eq_ignore_ascii_case(&old));
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
            if !arg(4).is_empty() && !arg(4).parse::<u8>().is_ok_and(|n| n >= 1) {
                return Err("% A distance is 1-255".into());
            }
            let d = c.dm()?;
            let route = StaticRoute { net, via: NextHop::Address(gw) };
            if id == Id::FwNoRoute {
                let before = d.routes.len();
                d.routes.retain(|r| *r != route);
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
            let f = &mut d.firewall;
            if f.acl(name).is_none() {
                if f.acls.len() >= MAX_LISTS {
                    return Err(format!("% At most {MAX_LISTS} access lists"));
                }
                f.acls.push(FwAcl { name: name.to_string(), entries: Vec::new() });
            }
            let list = f.acls.iter_mut().find(|a| a.name == name).ok_or("% No such list")?;
            if list.entries.contains(&ace) {
                return Ok(String::new());
            }
            if list.entries.len() >= MAX_ENTRIES {
                return Err(format!("% At most {MAX_ENTRIES} rules per list"));
            }
            list.entries.push(ace);
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
            let before = list.entries.len();
            list.entries.retain(|e| *e != ace);
            if list.entries.len() == before {
                return Err(format!("% {name} has no such rule"));
            }
            // A list without rules is gone, and so is its binding.
            if list.entries.is_empty() {
                f.acls.retain(|a| a.name != name);
                f.groups.retain(|g| g.acl != name);
            }
            Ok(String::new())
        }
        Id::FwAccessGroup | Id::FwNoAccessGroup => {
            let (acl, ifname) = (arg(0), arg(1));
            let d = c.dm()?;
            if id == Id::FwNoAccessGroup {
                let before = d.firewall.groups.len();
                d.firewall.groups.retain(|g| !(g.acl == acl && g.nameif.eq_ignore_ascii_case(ifname)));
                return if d.firewall.groups.len() == before { Err(format!("% {acl} is not bound to {ifname}")) } else { Ok(String::new()) };
            }
            if d.firewall.acl(acl).is_none() {
                return Err(format!("% No access list {acl} (add a rule to it first)"));
            }
            port_of(d, ifname)?;
            let f = &mut d.firewall;
            f.groups.retain(|g| !g.nameif.eq_ignore_ascii_case(ifname));
            if f.groups.len() >= MAX_LISTS {
                return Err("% Too many access groups".into());
            }
            f.groups.push(AccessGroup { acl: acl.to_string(), nameif: ifname.to_string() });
            Ok(String::new())
        }
        Id::FwObjectNetwork => {
            let name = arg(0);
            if !valid_name(name) {
                return Err(bad_name("an object", name));
            }
            let f = &mut c.dm()?.firewall;
            if f.object(name).is_none() {
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
            if let Some(user) =
                f.acls.iter().find(|a| a.entries.iter().any(|e| [&e.src, &e.dst].iter().any(|x| **x == fw::FwAddr::Object(name.to_string()))))
            {
                return Err(format!("% Access list {} uses {name}; remove those rules first", user.name));
            }
            f.objects.retain(|o| o.name != name);
            Ok(String::new())
        }
        Id::FwObjHost | Id::FwObjSubnet | Id::FwObjNat | Id::FwObjNoNat => object_line(c, id, args, line),
        Id::FwSameSecurity | Id::FwNoSameSecurity => {
            c.dm()?.firewall.same_security = id == Id::FwSameSecurity;
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
        Id::FwShowRoute => Ok(render::ip_route(c.t, c.d()?)),
        Id::FwShowNameif => Ok(show_nameif(c.d()?)),
        Id::FwShowDhcpdBinding => Ok(show_bindings(c)),
        Id::FwShowObjects => Ok(show_objects(c.d()?)),
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

/// `host`, `subnet` and `nat` inside `object network NAME`.
fn object_line(c: &mut Ctx<'_>, id: Id, args: &[String], line: &str) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let Mode::Object(name) = c.cli.mode.clone() else { return Err("% Not in an object".into()) };
    let nat = if id == Id::FwObjNat {
        let (real_if, mapped_if) = parse_pair(arg(0))?;
        let d = c.d()?;
        for n in [&real_if, &mapped_if] {
            if !n.eq_ignore_ascii_case("any") {
                port_of(d, n)?;
            }
        }
        if mapped_if.eq_ignore_ascii_case("any") || real_if.eq_ignore_ascii_case(&mapped_if) {
            return Err("% The mapped interface must be a named interface other than the real one".into());
        }
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
    let mut s = format!("{:<22} {:<14} {}\n", "Interface", "Name", "Security level");
    for p in &d.ports {
        let (name, level) = d.firewall.zone(&p.name).map_or(("-".to_string(), "-".to_string()), |(n, l)| (n.to_string(), l.to_string()));
        let _ = writeln!(s, "{:<22} {:<14} {level}", p.name, name);
    }
    s.trim_end().to_string()
}

fn show_ip_brief(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<22} {:<12} {:<18} {:<6} {}\n", "Interface", "Name", "IP address", "Admin", "Link");
    for (i, p) in d.ports.iter().enumerate() {
        let name = d.firewall.zone(&p.name).map_or("-", |(n, _)| n);
        let addr = p.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let up = packetcraft_sim::port_up(t, End { device: d.id, port: i });
        let _ =
            writeln!(s, "{:<22} {:<12} {:<18} {:<6} {}", p.name, name, addr, if p.shutdown { "down" } else { "up" }, if up { "up" } else { "down" });
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

fn show_nat(c: &Ctx<'_>) -> String {
    let Ok(d) = c.d() else { return String::new() };
    let hits = c.sim.state(d.id).map(|s| s.fw.nat_hits.clone()).unwrap_or_default();
    let rules: Vec<&NetObject> = d.firewall.objects.iter().filter(|o| o.nat.is_some()).collect();
    if rules.is_empty() {
        return "No NAT rules (give a network object a `nat` line).".into();
    }
    let mut s = String::from("Object NAT, static rules first:\n");
    let mut ordered: Vec<&NetObject> =
        rules.iter().copied().filter(|o| o.nat.as_ref().is_some_and(|n| matches!(n.kind, NatKind::Static(_)))).collect();
    ordered.extend(rules.iter().copied().filter(|o| o.nat.as_ref().is_some_and(|n| !matches!(n.kind, NatKind::Static(_)))));
    for (i, o) in ordered.iter().enumerate() {
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

fn ago(now: u64, then: u64) -> String {
    duration_text(u32::try_from(now.saturating_sub(then) / 1_000_000).unwrap_or(u32::MAX))
}

fn show_xlate(d: &Device, st: &packetcraft_sim::firewall::FwState, now: u64) -> String {
    let statics: Vec<String> = d
        .firewall
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
        let _ = writeln!(
            s,
            "dynamic  {} {} {}:{} → {} {}:{}  idle {}  (object {})",
            packetcraft_sim::firewall::proto_name(x.protocol),
            x.real_if,
            x.real,
            x.real_port,
            x.mapped_if,
            x.mapped,
            x.mapped_port,
            ago(now, x.last_us),
            x.object
        );
    }
    s.trim_end().to_string()
}

fn show_conn(st: &packetcraft_sim::firewall::FwState, now: u64) -> String {
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
        let bound: Vec<&str> = f.groups.iter().filter(|g| g.acl == a.name).map(|g| g.nameif.as_str()).collect();
        let where_ = if bound.is_empty() { "not bound to an interface".to_string() } else { format!("checks traffic in on {}", bound.join(", ")) };
        let _ = writeln!(s, "access-list {}: {} rule{}, {where_}", a.name, a.entries.len(), if a.entries.len() == 1 { "" } else { "s" });
        for (i, e) in a.entries.iter().enumerate() {
            let n = hits.get(&(a.name.clone(), i)).copied().unwrap_or(0);
            let _ = writeln!(s, "  {:>3}. {}  (hits {n})", i + 1, e.text());
        }
        let n = hits.get(&(a.name.clone(), usize::MAX)).copied().unwrap_or(0);
        let _ = writeln!(s, "       anything else is denied  (hits {n})");
    }
    Ok(s.trim_end().to_string())
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
    let _ = writeln!(s, "! PacketCraft configuration of {} ({})", d.name, d.catalogue_model().label);
    let _ = writeln!(s, "hostname {}", d.name);
    if let Some(p) = &d.access.enable_password {
        let _ = writeln!(s, "enable password {p}");
    }
    let _ = writeln!(s, "!");
    for p in &d.ports {
        let _ = writeln!(s, "interface {}", p.name);
        if !p.description.is_empty() {
            let _ = writeln!(s, " description {}", p.description);
        }
        if let Some((name, level)) = f.zone(&p.name) {
            let _ = writeln!(s, " nameif {name}\n security-level {level}");
        }
        if let Some(ip) = p.ip {
            let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
        }
        let _ = writeln!(s, " {}\n!", if p.shutdown { "shutdown" } else { "no shutdown" });
    }
    if f.same_security {
        let _ = writeln!(s, "same-security-traffic permit inter-interface");
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
    for a in &f.acls {
        for e in &a.entries {
            let _ = writeln!(s, "access-list {} extended {}", a.name, e.text());
        }
    }
    for g in &f.groups {
        let _ = writeln!(s, "access-group {} in interface {}", g.acl, g.nameif);
    }
    for r in &d.routes {
        let NextHop::Address(gw) = r.via else { continue };
        let ifname = d.ports.iter().filter(|p| p.ip.is_some_and(|n| n.contains(gw))).find_map(|p| f.zone(&p.name).map(|(n, _)| n));
        match ifname {
            Some(n) => {
                let _ = writeln!(s, "route {n} {} {} {gw} 1", r.net.addr, r.net.mask());
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
