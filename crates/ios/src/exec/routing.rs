//! Routing commands kept apart from the big dispatcher: loopback interfaces
//! (`interface Loopback N`).

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::MAX_DESCRIPTION;
use netlabcraft_model::routing::loopback_number;

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// The words of a static route's target, and the administrative distance typed after them (a
/// last word of digits: `10.0.0.2 200`, `g0/0 200`, `2001:db8::1 130`).
pub(super) fn split_distance(text: &str) -> Result<(String, Option<u8>), String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.split_last() {
        // `tunnel 0` is an interface, not a distance: the words before must name a target alone.
        Some((last, rest)) if rest.iter().any(|w| w.contains(|c: char| c.is_ascii_digit())) && last.chars().all(|c| c.is_ascii_digit()) => {
            let ad = last.parse::<u8>().ok().filter(|a| *a >= 1).ok_or("% The administrative distance is 1-255")?;
            Ok((rest.join(" "), Some(ad)))
        }
        _ => Ok((words.join(" "), None)),
    }
}

/// The loopback number an interface name gives, if it names a loopback.
pub(super) fn loopback(name: &str) -> Option<u32> {
    loopback_number(name)
}

fn loopback_ok(c: &Ctx) -> Result<(), String> {
    if !netlabcraft_sim::loopback::has_loopbacks(c.d()?) {
        return Err("% Loopback interfaces are for routers and switches".into());
    }
    Ok(())
}

/// `interface Loopback N`: created on first use, up at once.
pub(super) fn enter_loopback(c: &mut Ctx, n: u32) -> R {
    loopback_ok(c)?;
    c.dm()?.loopback_mut(n).ok_or("% Too many loopback interfaces")?;
    c.cli.mode = Mode::Loopback(n);
    Ok(String::new())
}

/// `no interface Loopback N`: gone, with its per-interface routing settings.
pub(super) fn delete_loopback(c: &mut Ctx, n: u32) -> R {
    let d = c.dm()?;
    let before = d.loopbacks.len();
    d.loopbacks.retain(|l| l.number != n);
    if d.loopbacks.len() == before {
        return Err(format!("% There is no interface Loopback{n}"));
    }
    let name = format!("Loopback{n}");
    d.ospf_if.remove(&name);
    d.eigrp_if.remove(&name);
    Ok(String::new())
}

/// The other interface whose subnet overlaps `net`, if any (`except` is the one being set).
pub(super) fn overlap(d: &netlabcraft_model::Device, net: Ipv4Net, except: &str) -> Option<String> {
    let mut all: Vec<(String, Ipv4Net)> = Vec::new();
    for p in &d.ports {
        if let Some(ip) = p.ip {
            all.push((p.name.clone(), ip));
        }
        for s in &p.subifs {
            if let Some(ip) = s.ip {
                all.push((format!("{}.{}", p.name, s.number), ip));
            }
        }
    }
    all.extend(d.svis.iter().filter_map(|s| s.ip.map(|ip| (format!("Vlan{}", s.vlan), ip))));
    all.extend(d.tunnels.iter().filter_map(|t| t.ip.map(|ip| (t.name(), ip))));
    all.extend(d.loopbacks.iter().filter_map(|l| l.ip.map(|ip| (l.name(), ip))));
    all.into_iter().find(|(n, ip)| n != except && ip.overlaps(net)).map(|(n, ip)| format!("% {} overlaps {} on {n}", net.network(), ip.network()))
}

/// `ip address`, `shutdown`, `description` and their `no` forms on a loopback.
pub(super) fn loopback_iface(c: &mut Ctx, id: Id, a0: &str, a1: &str) -> R {
    let Mode::Loopback(n) = c.cli.mode else { return Err("% Not in a loopback interface".into()) };
    let name = format!("Loopback{n}");
    let ip = if id == Id::IpAddress {
        let net = Ipv4Net::with_mask(parse_ip(a0).map_err(|e| format!("% {e}"))?, parse_ip(a1).map_err(|e| format!("% {e}"))?)
            .map_err(|e| format!("% {e}"))?;
        if net.is_network_or_broadcast() {
            return Err(format!("% {} is the network or broadcast address of {}", net.addr, net.network()));
        }
        if let Some(why) = overlap(c.d()?, net, &name) {
            return Err(why);
        }
        Some(net)
    } else {
        None
    };
    let text: String = a0.chars().take(MAX_DESCRIPTION).collect();
    let lo = c.dm()?.loopback_mut(n).ok_or("% Too many loopback interfaces")?;
    match id {
        Id::IpAddress => lo.ip = ip,
        Id::NoIpAddress => lo.ip = None,
        Id::Shutdown => lo.shutdown = true,
        Id::NoShutdown => lo.shutdown = false,
        Id::Description => lo.description = text,
        _ => lo.description.clear(),
    }
    Ok(match id {
        Id::NoShutdown => format!("{name} is now up"),
        Id::Shutdown => format!("{name} is now administratively down"),
        _ => String::new(),
    })
}

// ---- what the show commands print about loopbacks ----

/// The loopbacks' blocks of the running configuration.
pub(crate) fn loopback_config(s: &mut String, d: &netlabcraft_model::Device) {
    use std::fmt::Write as _;
    for l in &d.loopbacks {
        let _ = writeln!(s, "interface {}", l.name());
        if !l.description.is_empty() {
            let _ = writeln!(s, " description {}", l.description);
        }
        match l.ip {
            Some(ip) => {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            None => s.push_str(" no ip address\n"),
        }
        crate::render::ipv6_lines(s, &l.ipv6);
        crate::render::ospf_if_lines(s, d, &l.name());
        if l.shutdown {
            s.push_str(" shutdown\n");
        }
        s.push_str("!\n");
    }
}

/// The loopbacks' rows of `show ip interface brief` (same columns as the other rows).
pub(crate) fn loopback_brief(s: &mut String, d: &netlabcraft_model::Device) {
    use std::fmt::Write as _;
    for l in &d.loopbacks {
        let addr = l.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let state = if l.shutdown { "down" } else { "up" };
        let _ = writeln!(s, "{:<24} {:<18} {:<7} {}", l.name(), addr, state, state);
    }
}

/// One loopback's `show interfaces` block.
pub(crate) fn loopback_block(l: &netlabcraft_model::routing::Loopback) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let state = if l.shutdown { "administratively down, line protocol down" } else { "up, line protocol up" };
    let _ = writeln!(s, "{} is {state}", l.name());
    if !l.description.is_empty() {
        let _ = writeln!(s, "  Description: {}", l.description);
    }
    match l.ip {
        Some(ip) => {
            let _ = writeln!(s, "  Internet address {ip}");
        }
        None => s.push_str("  No address\n"),
    }
    s.push_str("  Virtual interface: no cable, always up unless shut down; nothing is sent out of it\n");
    s
}

/// Every loopback's `show interfaces` block.
pub(crate) fn loopback_blocks(d: &netlabcraft_model::Device) -> String {
    d.loopbacks.iter().map(loopback_block).collect()
}

/// `show interfaces loopback N`.
pub(super) fn loopback_interface(d: &netlabcraft_model::Device, n: u32) -> R {
    let l = d.loopbacks.iter().find(|x| x.number == n).ok_or_else(|| format!("% There is no interface Loopback{n}"))?;
    Ok(loopback_block(l).trim_end().to_string())
}

// ---- OSPF as the courses teach it ----

/// The names of the interfaces an interface mode is configuring (ports, subinterfaces, VLAN
/// interfaces, tunnels and loopbacks).
pub(super) fn if_names(c: &Ctx) -> Result<Vec<String>, String> {
    Ok(match &c.cli.mode {
        Mode::Svi(v) => vec![format!("Vlan{v}")],
        Mode::Tunnel(n) => vec![format!("Tunnel{n}")],
        Mode::Loopback(n) => vec![format!("Loopback{n}")],
        Mode::Iface(ifs) => {
            let d = c.d()?;
            ifs.iter().filter_map(|r| d.ports.get(r.port).map(|p| r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))).collect()
        }
        _ => return Err("% Not in interface mode".into()),
    })
}

/// An OSPF area as typed: a number or dotted-decimal.
fn area_number(text: &str) -> Result<u32, String> {
    if let Ok(n) = text.parse::<u32>() {
        return Ok(n);
    }
    text.parse::<std::net::Ipv4Addr>().map(u32::from).map_err(|_| "% The area is 0-4294967295 or A.B.C.D".to_string())
}

/// Changes the per-interface OSPF settings of every interface the mode is on.
fn each_ospf_if(c: &mut Ctx, f: impl Fn(&mut netlabcraft_model::OspfIf)) -> R {
    let names = if_names(c)?;
    let d = c.dm()?;
    for n in names {
        let e = d.ospf_if.entry(n.clone()).or_default();
        f(e);
        if *e == netlabcraft_model::OspfIf::default() {
            d.ospf_if.remove(&n);
        }
    }
    Ok(String::new())
}

/// Interface-mode OSPF commands: `ip ospf N area A`, the hello and dead intervals and the
/// network type.
pub(super) fn ospf_if_command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim();
    let secs = |text: &str| text.parse::<u16>().ok().filter(|s| *s >= 1).ok_or("% Seconds: 1-65535".to_string());
    match id {
        Id::IpOspfArea => {
            let process: u16 = arg(0).parse().ok().filter(|n| *n >= 1).ok_or("% The process id is 1-65535")?;
            let area = area_number(arg(1))?;
            let d = c.dm()?;
            if !d.routes_ip() {
                return Err("% OSPF runs on routers and on switches with `ip routing`".into());
            }
            match &d.ospf {
                Some(o) if o.process != process => {
                    return Err(format!("% OSPF process {} runs here already: use `ip ospf {} area {area}`", o.process, o.process));
                }
                Some(_) => {}
                None => d.ospf = Some(netlabcraft_model::Ospf { process, ..netlabcraft_model::Ospf::default() }),
            }
            each_ospf_if(c, |e| e.area = Some((process, area)))
        }
        Id::NoIpOspfArea => each_ospf_if(c, |e| e.area = None),
        Id::IpOspfHello => {
            let s = secs(arg(0))?;
            each_ospf_if(c, |e| e.hello = Some(s))
        }
        Id::NoIpOspfHello => each_ospf_if(c, |e| e.hello = None),
        Id::IpOspfDead => {
            let s = secs(arg(0))?;
            each_ospf_if(c, |e| e.dead = Some(s))
        }
        Id::NoIpOspfDead => each_ospf_if(c, |e| e.dead = None),
        Id::IpOspfNetwork => {
            let kind = arg(0).to_ascii_lowercase();
            let p2p = if !kind.is_empty() && "point-to-point".starts_with(&kind) {
                true
            } else if !kind.is_empty() && "broadcast".starts_with(&kind) {
                false
            } else {
                return Err("% Network type: point-to-point or broadcast".into());
            };
            each_ospf_if(c, |e| e.p2p = p2p)
        }
        _ => each_ospf_if(c, |e| e.p2p = false),
    }
}

/// The name an interface is kept under in OSPF lists, from what was typed.
fn ospf_if_name(c: &Ctx, text: &str) -> Result<String, String> {
    if let Some(n) = loopback(text) {
        return Ok(format!("Loopback{n}"));
    }
    if let Some(n) = netlabcraft_model::vpn::tunnel_number(text) {
        return Ok(format!("Tunnel{n}"));
    }
    let r = c.iface_ref(text)?;
    let d = c.d()?;
    let port = d.ports.get(r.port).ok_or("% No such port")?;
    Ok(r.sub.map_or(port.name.clone(), |s| format!("{}.{s}", port.name)))
}

/// `[no] passive-interface default|X` under `router ospf`.
pub(super) fn ospf_passive(c: &mut Ctx, add: bool, text: &str) -> R {
    let default = text.eq_ignore_ascii_case("default");
    let name = if default { String::new() } else { ospf_if_name(c, text)? };
    let o = c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?;
    if default {
        o.passive_default = add;
        o.not_passive.clear();
        o.passive.clear();
        return Ok(String::new());
    }
    // With every interface passive by default, the list holds the exceptions.
    let (on, off) = if o.passive_default { (&mut o.not_passive, add) } else { (&mut o.passive, !add) };
    on.retain(|p| *p != name);
    if !off {
        if on.len() >= netlabcraft_model::MAX_NETWORKS {
            return Err("% Too many passive-interface lines".into());
        }
        on.push(name);
    }
    Ok(String::new())
}

/// `[no] redistribute static [subnets]` under `router ospf`.
pub(super) fn ospf_redistribute(c: &mut Ctx, on: bool, rest: &str) -> R {
    let words: Vec<String> = rest.split_whitespace().map(str::to_ascii_lowercase).collect();
    if words.iter().any(|w| !"subnets".starts_with(w.as_str())) {
        return Err("% Only `redistribute static [subnets]` is simulated".into());
    }
    c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?.redistribute_static = on;
    Ok(if on { "Static routes go to the other OSPF routers as external type-2 routes (metric 20).".into() } else { String::new() })
}

/// `clear ip ospf [N] process`.
pub(super) fn clear_ospf(c: &mut Ctx, process: &str) -> R {
    let o = c.d()?.ospf.as_ref().ok_or("% OSPF is not running here")?;
    if !process.is_empty() && process != o.process.to_string() {
        return Err(format!("% No OSPF process {process} here (it is {})", o.process));
    }
    let p = o.process;
    c.sim.clear_ospf();
    Ok(format!("OSPF process {p} restarted: neighbours form again and each segment elects its DR and BDR anew"))
}

/// `show ip ospf database`: the LSAs this router holds, by area and kind.
pub(super) fn ospf_database(c: &mut Ctx) -> R {
    use netlabcraft_sim::ospf_lsdb::LsaKind;
    use std::fmt::Write as _;
    let d = c.d()?.clone();
    let Some(o) = &d.ospf else { return Ok("OSPF is not running here.".into()) };
    let st = c.sim.ospf_state(c.t);
    let rid = st.router_ids.get(&d.id).map_or("unset".to_string(), ToString::to_string);
    let lsas = netlabcraft_sim::ospf_lsdb::database(c.t, &st, d.id);
    let mut s = format!("OSPF link-state database of router {rid} (process {})\n", o.process);
    if lsas.is_empty() {
        s.push_str("\n(empty: no interface runs OSPF yet)");
        return Ok(s);
    }
    let mut last: Option<(LsaKind, Option<u32>)> = None;
    for l in &lsas {
        if last != Some((l.kind, l.area)) {
            let title = match l.kind {
                LsaKind::Router => "Router LSAs (type 1)",
                LsaKind::Network => "Network LSAs (type 2)",
                LsaKind::Summary => "Summary network LSAs (type 3)",
                LsaKind::External => "AS external LSAs (type 5)",
            };
            let area = l.area.map_or(String::new(), |a| format!(", area {a}"));
            let last_col = if l.kind == LsaKind::Router { "Links" } else { "Prefix" };
            let _ = write!(s, "\n{title}{area}\n{:<16} {:<16} {}\n", "Link ID", "Advertised by", last_col);
            last = Some((l.kind, l.area));
        }
        let count = if l.kind == LsaKind::Router { l.count.to_string() } else { format!("/{}", l.count) };
        let _ = writeln!(s, "{:<16} {:<16} {count}", l.id.to_string(), l.adv.to_string());
    }
    Ok(s.trim_end().to_string())
}

/// `show ip ospf neighbor detail`, and why routers on a shared link are not neighbours.
pub(super) fn ospf_neighbor_detail(c: &mut Ctx) -> R {
    use std::fmt::Write as _;
    let d = c.d()?.clone();
    if d.ospf.is_none() {
        return Ok("OSPF is not running on this router.".into());
    }
    let st = c.sim.ospf_state(c.t);
    let mut s = String::new();
    for n in st.neighbors.get(&d.id).into_iter().flatten() {
        let peer = c.t.device(n.device);
        let name = peer.map_or("?".to_string(), |x| x.name.clone());
        let seg = st.segments.get(&(d.id, n.iface.clone()));
        let role = match seg {
            Some(g) if g.dr.is_none() => "point-to-point (no DR)",
            Some(g) if g.dr == Some(n.address) => "DR",
            Some(g) if g.bdr == Some(n.address) => "BDR",
            _ => "DROTHER",
        };
        let their_if = peer.and_then(|p| netlabcraft_sim::l3_ifaces(p).into_iter().find(|i| i.net.addr == n.address).map(|i| i.name));
        let prio = peer.zip(their_if.as_ref()).and_then(|(p, i)| p.ospf_if.get(i).and_then(|x| x.priority)).unwrap_or(1);
        let (hello, dead) = netlabcraft_sim::ospf::timers(&d, &n.iface);
        let _ = writeln!(s, "Neighbour {} ({name}), interface address {}", n.router_id, n.address);
        let _ = writeln!(s, "    On {}, area {}, state FULL, role {role}", n.iface, seg.map_or(0, |g| g.area));
        let _ = writeln!(s, "    Priority {prio}; segment DR {}, BDR {}", show_addr(seg.and_then(|g| g.dr)), show_addr(seg.and_then(|g| g.bdr)));
        let _ = writeln!(s, "    Hello every {hello} s, declared dead after {dead} s of silence");
    }
    mismatch_lines(&mut s, &st, d.id);
    if s.is_empty() {
        s.push_str("No OSPF neighbours (check the network statements or `ip ospf N area A`, passive interfaces and cables).");
    }
    Ok(s.trim_end().to_string())
}

fn show_addr(a: Option<std::net::Ipv4Addr>) -> String {
    a.map_or("none".to_string(), |a| a.to_string())
}

/// Lines naming the routers that share a link with this one but are not its neighbours.
pub(crate) fn mismatch_lines(s: &mut String, st: &netlabcraft_sim::ospf::OspfState, me: netlabcraft_core::DeviceId) {
    use std::fmt::Write as _;
    for m in st.mismatches.get(&me).into_iter().flatten() {
        let _ = writeln!(s, "Not a neighbour: {} on {}: {}", m.address, m.iface, m.why);
    }
}

// ---- routers as IPv6 clients ----

/// `[no] ipv6 address autoconfig [default]` and `[no] ipv6 address dhcp` on a router
/// interface: the setting is kept, and the address is asked for at once.
pub(super) fn ipv6_client(c: &mut Ctx, id: Id, default: bool) -> R {
    let names = c.v6_names();
    let [name] = names.as_slice() else { return Err("% Set this on one interface at a time".into()) };
    let name = name.clone();
    let slaac = matches!(id, Id::Ipv6AddressAutoconfig | Id::NoIpv6AddressAutoconfig);
    let on = matches!(id, Id::Ipv6AddressAutoconfig | Id::Ipv6AddressDhcp);
    c.each_v6(|v6, _, _| {
        if slaac {
            v6.autoconfig = on;
            v6.autoconfig_default = on && default;
            v6.learned_router = None;
        } else {
            v6.dhcp_client = on;
        }
        v6.learned.clear();
        Ok(())
    })?;
    if !on {
        return Ok(String::new());
    }
    let got = if slaac {
        c.sim.router_autoconfig6(c.t, c.dev, &name).map(|(net, router)| (net, Some(router)))
    } else {
        c.sim.router_dhcp6(c.t, c.dev, &name).map(|net| (net, None))
    };
    match got {
        Ok((net, router)) => {
            c.each_v6(|v6, _, _| {
                v6.learned = vec![net];
                v6.learned_router = router;
                Ok(())
            })?;
            let from = router.map_or("a DHCPv6 server".to_string(), |r| format!("router {r}"));
            Ok(format!("{name} took {net} from {from}"))
        }
        // The setting stays: the address comes when a router or server answers.
        Err(e) => Ok(format!("{name} has no address yet: {e}")),
    }
}

/// After `no shutdown`: an IPv6 client interface without its address asks for it again.
pub(super) fn ipv6_client_retry(c: &mut Ctx) -> String {
    let Mode::Iface(ifs) = &c.cli.mode else { return String::new() };
    let [r] = ifs.as_slice() else { return String::new() };
    let Ok(d) = c.d() else { return String::new() };
    let Some(p) = d.ports.get(r.port).filter(|_| r.sub.is_none()) else { return String::new() };
    let v6 = &p.ipv6;
    if !v6.learned.is_empty() {
        return String::new();
    }
    let id = if v6.autoconfig {
        Id::Ipv6AddressAutoconfig
    } else if v6.dhcp_client {
        Id::Ipv6AddressDhcp
    } else {
        return String::new();
    };
    let default = v6.autoconfig_default;
    ipv6_client(c, id, default).unwrap_or_default()
}

/// The running-configuration lines of an interface that is an IPv6 client.
pub(crate) fn ipv6_client_lines(s: &mut String, c: &netlabcraft_model::Ipv6Conf) {
    if c.autoconfig {
        s.push_str(if c.autoconfig_default { " ipv6 address autoconfig default\n" } else { " ipv6 address autoconfig\n" });
    }
    if c.dhcp_client {
        s.push_str(" ipv6 address dhcp\n");
    }
}
