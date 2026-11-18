//! VRF-lite: `ip vrf NAME` (route distinguisher, route targets), `ip vrf forwarding NAME` on
//! interfaces, `ip route vrf NAME ...`, `router ospf N vrf NAME`, `address-family ipv4 vrf NAME`
//! under BGP, and the `show` and `ping` forms that take a VRF.
//!
//! The VRF's own OSPF process and BGP address family are edited with the ordinary OSPF and BGP
//! commands: while the mode belongs to a VRF, the VRF's copy of the process stands in for the
//! device's own (see [`dispatch_in_vrf`]), so every command of those modes works there with no
//! second implementation. The routing tables are the simulator's (`netlabcraft_sim::vrf`).

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, VlanId, parse_ip};
use netlabcraft_model::bgp::Bgp;
use netlabcraft_model::routing::loopback_number;
use netlabcraft_model::vrf::{MAX_NAME, MAX_TARGETS, MAX_VRFS, Vrf, valid_rd};
use netlabcraft_model::{Device, DeviceKind, MAX_ROUTES, NextHop, Ospf, StaticRoute, Topology, ifname};
use netlabcraft_sim::{PingOptions, Sim};

use super::{Cli, Ctx, Mode, R, routing};
use crate::grammar::{self, Id, Modes};
use crate::render;
use netlabcraft_core::DeviceId;

/// Runs a line with the VRF's copy of OSPF or BGP in the device's place when the mode is one of
/// a VRF's, and plainly otherwise.
pub(super) fn dispatch_in_vrf(t: &mut Topology, sim: &mut Sim, dev: DeviceId, cli: &mut Cli, line: &str) -> R {
    let mode = cli.mode.clone();
    let ctx = cli.vrf_ctx.clone();
    let vrf = match (ctx, &mode) {
        (Some(v), Mode::Ospf | Mode::Bgp) => v,
        _ => return super::dispatch_line(t, sim, dev, cli, line),
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    // `do` runs a privileged command on the device as it is, whatever mode this is.
    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("do")) {
        return super::dispatch_line(t, sim, dev, cli, line);
    }
    // `exit` and `exit-address-family` leave a VRF's address family for the BGP process itself.
    if mode == Mode::Bgp
        && words.len() == 1
        && let Some(w) = words.first()
        && (w.eq_ignore_ascii_case("exit") || (w.len() >= 6 && "exit-address-family".starts_with(&w.to_ascii_lowercase())))
    {
        cli.vrf_ctx = None;
        return Ok(String::new());
    }
    let in_mode = t.device(dev).is_some_and(|d| {
        let bits = mode.bits();
        grammar::resolve(&words, bits, super::device_bits(d, &mode)).is_ok_and(|(s, _)| s.modes.has(bits) && !s.modes.has(Modes::CONFIG))
    });
    if !in_mode {
        cli.vrf_ctx = None;
        return super::dispatch_line(t, sim, dev, cli, line);
    }
    swap_process(t, dev, &vrf, &mode);
    let r = super::dispatch_line(t, sim, dev, cli, line);
    swap_process(t, dev, &vrf, &mode);
    if !matches!(cli.mode, Mode::Ospf | Mode::Bgp) {
        cli.vrf_ctx = None;
    }
    r
}

/// Exchanges the device's OSPF process (or BGP process) with the VRF's.
fn swap_process(t: &mut Topology, dev: DeviceId, vrf: &str, mode: &Mode) {
    let Some(d) = t.device_mut(dev) else { return };
    let Some(v) = d.vrfs.iter_mut().find(|v| v.name == vrf) else { return };
    match mode {
        Mode::Ospf => std::mem::swap(&mut d.ospf, &mut v.ospf),
        Mode::Bgp => std::mem::swap(&mut d.bgp, &mut v.bgp),
        _ => {}
    }
}

fn vrf_name(text: &str) -> Result<String, String> {
    let name = text.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || !name.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% A VRF name is 1-{MAX_NAME} printable characters with no spaces"));
    }
    Ok(name.to_string())
}

fn existing<'a>(d: &'a Device, name: &str) -> Result<&'a Vrf, String> {
    d.vrf(name).ok_or_else(|| format!("% VRF {name} does not exist"))
}

fn capable(c: &Ctx) -> Result<(), String> {
    let d = c.d()?;
    if d.kind == DeviceKind::Switch && !d.ip_routing {
        return Err("% VRFs need a router, or a multilayer switch with `ip routing`".into());
    }
    Ok(())
}

/// The interfaces the current mode configures.
fn mode_names(c: &Ctx) -> Result<Vec<String>, String> {
    match &c.cli.mode {
        Mode::Svi(v) => Ok(vec![format!("Vlan{v}")]),
        Mode::Loopback(n) => Ok(vec![format!("Loopback{n}")]),
        Mode::Iface(ifs) => {
            let d = c.d()?;
            Ok(ifs.iter().filter_map(|r| d.ports.get(r.port).map(|p| r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))).collect())
        }
        _ => Err("% Not in interface mode".into()),
    }
}

/// Whether an interface of this device has an IPv4 address.
fn has_address(d: &Device, name: &str) -> bool {
    if let Some(v) = name.strip_prefix("Vlan") {
        return v.parse::<u16>().ok().and_then(VlanId::new).is_some_and(|v| d.svis.iter().any(|s| s.vlan == v && s.ip.is_some()));
    }
    if let Some(n) = loopback_number(name) {
        return d.loopbacks.iter().any(|l| l.number == n && l.ip.is_some());
    }
    d.ports
        .iter()
        .any(|p| (p.name == name && p.ip.is_some()) || p.subifs.iter().any(|s| s.ip.is_some() && format!("{}.{}", p.name, s.number) == name))
}

/// Takes the IPv4 address off an interface (it does not carry over into another VRF).
fn clear_address(d: &mut Device, name: &str) {
    if let Some(v) = name.strip_prefix("Vlan") {
        if let Some(v) = v.parse::<u16>().ok().and_then(VlanId::new)
            && let Some(s) = d.svis.iter_mut().find(|s| s.vlan == v)
        {
            s.ip = None;
        }
        return;
    }
    if let Some(n) = loopback_number(name) {
        if let Some(l) = d.loopbacks.iter_mut().find(|l| l.number == n) {
            l.ip = None;
        }
        return;
    }
    for p in &mut d.ports {
        if p.name == name {
            p.ip = None;
            p.dhcp = false;
        }
        let base = p.name.clone();
        for s in &mut p.subifs {
            if format!("{base}.{}", s.number) == name {
                s.ip = None;
            }
        }
    }
}

fn set_forwarding(c: &mut Ctx, vrf: Option<&str>) -> R {
    capable(c)?;
    let names = mode_names(c)?;
    if let Some(v) = vrf {
        existing(c.d()?, v)?;
    }
    let mut said = String::new();
    for name in names {
        let d = c.dm()?;
        if let Some(port) = d.ports.iter().position(|p| p.name == name)
            && d.is_switchport(port)
        {
            return Err(format!("% {name} is a switch port: `no switchport` first (multilayer switches), or put a VLAN interface in the VRF"));
        }
        if d.vrf_of(&name) == vrf {
            continue;
        }
        let had = has_address(d, &name);
        d.set_vrf_of(&name, vrf);
        clear_address(d, &name);
        if had {
            let _ = writeln!(said, "{name}: IP address removed (the interface changed VRF); set it again with `ip address`");
        }
    }
    Ok(said.trim_end().to_string())
}

fn target_ok(text: &str) -> Result<String, String> {
    if valid_rd(text.trim()) { Ok(text.trim().to_string()) } else { Err("% Write it as ASN:NN (65000:100) or A.B.C.D:NN (10.0.0.1:100)".into()) }
}

/// The VRF the mode edits.
fn current<'a>(c: &'a mut Ctx) -> Result<&'a mut Vrf, String> {
    let Mode::Vrf(name) = c.cli.mode.clone() else { return Err("% Not in VRF mode".into()) };
    c.dm()?.vrfs.iter_mut().find(|v| v.name == name).ok_or_else(|| "% The VRF was deleted".to_string())
}

/// `[next hop or interface] [distance] [tag N]` of a VRF's static route.
fn route_target(c: &Ctx, text: &str) -> Result<(NextHop, Option<u8>, u32), String> {
    let (rest, tag) = routing::split_tag(text)?;
    let (rest, distance) = routing::split_distance(&rest)?;
    let rest = rest.trim();
    if rest.is_empty() {
        return Err("% Give a next-hop address or an exit interface".into());
    }
    let via = match rest.parse::<Ipv4Addr>() {
        Ok(a) => NextHop::Address(a),
        Err(_) => {
            let r = c.iface_ref(rest)?;
            let d = c.d()?;
            let p = d.ports.get(r.port).ok_or("% No such port")?;
            NextHop::Interface(r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))
        }
    };
    Ok((via, distance, tag))
}

fn net_of(net: &str, mask: &str) -> Result<Ipv4Net, String> {
    let n = Ipv4Net::with_mask(parse_ip(net).map_err(|e| format!("% {e}"))?, parse_ip(mask).map_err(|e| format!("% {e}"))?)
        .map_err(|e| format!("% {e}"))?;
    if n.network() != n {
        return Err(format!("% {n} has host bits set; the network is {}", n.network()));
    }
    Ok(n)
}

/// `ping vrf NAME A.B.C.D [repeat N] [size S] [source A.B.C.D] [timeout T]`.
fn ping_options(rest: &str) -> Result<PingOptions, String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let mut opts = PingOptions::default();
    let usage = "% ping vrf NAME A.B.C.D [repeat N] [size S] [source A.B.C.D] [timeout SECONDS] [df-bit]";
    let mut i = 0;
    while let Some(w) = words.get(i) {
        let lw = w.to_ascii_lowercase();
        if lw.len() >= 2 && "df-bit".starts_with(&lw) {
            opts.df = true;
            i += 1;
            continue;
        }
        let key = ["repeat", "size", "source", "timeout"].into_iter().find(|k| !lw.is_empty() && k.starts_with(&lw)).ok_or(usage)?;
        let v = *words.get(i + 1).ok_or(usage)?;
        match key {
            "repeat" => opts.count = v.parse().ok().filter(|n| (1..=1000).contains(n)).ok_or("% Repeat count must be 1-1000")?,
            "size" => opts.size = v.parse::<usize>().ok().filter(|n| (36..=18_024).contains(n)).ok_or("% Datagram size 36-18024")? - 28,
            "timeout" => opts.timeout_us = v.parse::<u64>().ok().filter(|n| *n <= 3_600).ok_or("% Timeout 0-3600 seconds")?.max(1) * 1_000_000,
            _ => opts.source = Some(parse_ip(v).map_err(|e| format!("% {e}"))?),
        }
        i += 2;
    }
    Ok(opts)
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::IpVrf => {
            capable(c)?;
            let name = vrf_name(arg(0))?;
            let d = c.dm()?;
            if d.vrf(&name).is_none() {
                if d.vrfs.len() >= MAX_VRFS {
                    return Err(format!("% At most {MAX_VRFS} VRFs"));
                }
                d.vrfs.push(Vrf::new(&name));
            }
            c.cli.mode = Mode::Vrf(name);
            Ok(String::new())
        }
        Id::NoIpVrf => {
            let name = arg(0).trim().to_string();
            let d = c.dm()?;
            let Some(at) = d.vrfs.iter().position(|v| v.name == name) else { return Err(format!("% VRF {name} does not exist")) };
            let gone = d.vrfs.remove(at);
            for m in &gone.members {
                clear_address(d, m);
            }
            Ok(String::new())
        }
        Id::VrfRd => {
            let rd = target_ok(arg(0))?;
            let me = match &c.cli.mode {
                Mode::Vrf(n) => n.clone(),
                _ => return Err("% Not in VRF mode".into()),
            };
            if c.d()?.vrfs.iter().any(|v| v.name != me && v.rd.as_deref() == Some(rd.as_str())) {
                return Err(format!("% {rd} is the route distinguisher of another VRF; each VRF needs its own"));
            }
            current(c)?.rd = Some(rd);
            Ok(String::new())
        }
        Id::NoVrfRd => {
            current(c)?.rd = None;
            Ok(String::new())
        }
        Id::VrfRouteTarget | Id::NoVrfRouteTarget => {
            let which = arg(0).to_ascii_lowercase();
            if !["import", "export", "both"].contains(&which.as_str()) {
                return Err("% Say import, export or both".into());
            }
            let target = target_ok(arg(1))?;
            let add = id == Id::VrfRouteTarget;
            let v = current(c)?;
            for (word, list) in [("import", &mut v.import), ("export", &mut v.export)] {
                if which != "both" && which != word {
                    continue;
                }
                if add {
                    if !list.contains(&target) {
                        if list.len() >= MAX_TARGETS {
                            return Err(format!("% At most {MAX_TARGETS} route targets to {word}"));
                        }
                        list.push(target.clone());
                    }
                } else {
                    list.retain(|x| *x != target);
                }
            }
            Ok(String::new())
        }
        Id::VrfDescription => {
            current(c)?.description = arg(0).chars().take(netlabcraft_model::MAX_DESCRIPTION).collect();
            Ok(String::new())
        }
        Id::IpVrfForwarding => {
            let name = vrf_name(arg(0))?;
            set_forwarding(c, Some(&name))
        }
        Id::NoIpVrfForwarding => set_forwarding(c, None),
        Id::IpRouteVrf => {
            capable(c)?;
            let name = arg(0).to_string();
            existing(c.d()?, &name)?;
            let net = net_of(arg(1), arg(2))?;
            let (via, distance, tag) = route_target(c, arg(3))?;
            let d = c.dm()?;
            let Some(v) = d.vrfs.iter_mut().find(|v| v.name == name) else { return Err(format!("% VRF {name} does not exist")) };
            let route = StaticRoute { net, via, distance, tag };
            v.routes.retain(|r| !(r.net == route.net && r.via == route.via));
            if v.routes.len() >= MAX_ROUTES {
                return Err(format!("% At most {MAX_ROUTES} static routes"));
            }
            v.routes.push(route);
            v.routes.sort();
            Ok(String::new())
        }
        Id::NoIpRouteVrf => {
            let name = arg(0).to_string();
            existing(c.d()?, &name)?;
            let net = net_of(arg(1), arg(2))?;
            let via = match args.get(3).map(|t| t.trim()).filter(|t| !t.is_empty()) {
                Some(t) => Some(route_target(c, t)?.0),
                None => None,
            };
            let d = c.dm()?;
            let Some(v) = d.vrfs.iter_mut().find(|v| v.name == name) else { return Err(format!("% VRF {name} does not exist")) };
            let before = v.routes.len();
            v.routes.retain(|r| !(r.net == net && via.as_ref().is_none_or(|x| *x == r.via)));
            if v.routes.len() == before {
                return Err("% No such static route".into());
            }
            Ok(String::new())
        }
        Id::ShowIpRouteVrf => {
            let name = arg(0).to_string();
            let d = c.d()?.clone();
            existing(&d, &name)?;
            let table = c.sim.vrf_routing_table(c.t, &d, &name).ok_or_else(|| format!("% VRF {name} does not exist"))?;
            Ok(format!("Routing Table: {name}\n{}", render::ip_route_table(&d, &table)))
        }
        Id::ShowIpVrf => Ok(brief(c.d()?)),
        Id::ShowIpVrfDetail => Ok(detail(c.d()?)),
        Id::ShowIpVrfInterfaces => Ok(interfaces(c.d()?)),
        Id::ShowIpArpVrf => {
            let name = arg(0).to_string();
            existing(c.d()?, &name)?;
            Ok(arp(c.t, c.sim, c.d()?, &name))
        }
        Id::PingVrf | Id::PingVrfRepeat => {
            let name = arg(0).to_string();
            existing(c.d()?, &name)?;
            let dst = parse_ip(arg(1)).map_err(|e| format!("% {e}"))?;
            let opts = if id == Id::PingVrfRepeat { ping_options(arg(2))? } else { PingOptions::default() };
            let probes = c.sim.ping_vrf(c.t, c.dev, &name, dst, opts).map_err(|e| format!("% {e}"))?;
            Ok(render::ping(dst.into(), &probes, opts))
        }
        Id::RouterOspfVrf | Id::NoRouterOspfVrf => {
            capable(c)?;
            let n: u16 = arg(0).parse().ok().filter(|n| *n >= 1).ok_or("% The process id is 1-65535")?;
            let name = arg(1).to_string();
            existing(c.d()?, &name)?;
            let d = c.dm()?;
            let Some(v) = d.vrfs.iter_mut().find(|v| v.name == name) else { return Err(format!("% VRF {name} does not exist")) };
            if id == Id::NoRouterOspfVrf {
                return if v.ospf.as_ref().is_some_and(|o| o.process == n) {
                    v.ospf = None;
                    Ok(String::new())
                } else {
                    Err(format!("% No OSPF process {n} in VRF {name}"))
                };
            }
            match &v.ospf {
                Some(o) if o.process != n => {
                    return Err(format!("% VRF {name} already runs OSPF process {}; each VRF runs one process", o.process));
                }
                Some(_) => {}
                None => v.ospf = Some(Ospf { process: n, ..Ospf::default() }),
            }
            c.cli.mode = Mode::Ospf;
            c.cli.vrf_ctx = Some(name);
            Ok(String::new())
        }
        Id::BgpAddressFamilyVrf => {
            let name = arg(0).to_string();
            existing(c.d()?, &name)?;
            let d = c.dm()?;
            let Some(asn) = d.bgp.as_ref().map(|b| b.asn) else { return Err("% Start BGP first: router bgp <AS>".into()) };
            let Some(v) = d.vrfs.iter_mut().find(|v| v.name == name) else { return Err(format!("% VRF {name} does not exist")) };
            if v.bgp.is_none() {
                v.bgp = Some(Bgp::new(asn));
            }
            c.cli.vrf_ctx = Some(name);
            Ok(String::new())
        }
        Id::ShowIpBgpVrf | Id::ShowIpBgpVrfSummary => {
            let name = arg(0).to_string();
            existing(c.d()?, &name)?;
            super::bgp::vrf_show(c, &name, id == Id::ShowIpBgpVrfSummary)
        }
        Id::ShowOspfNeighborVrf => {
            let name = arg(0).to_string();
            existing(c.d()?, &name)?;
            let dev = c.dev;
            let (sub, topo) = c.sim.vrf_inner(c.t, &name).ok_or_else(|| format!("% VRF {name} does not exist"))?;
            let st = sub.ospf_state(topo);
            let d = topo.device(dev).ok_or("% That device no longer exists")?;
            Ok(render::ospf_neighbors(topo, d, &st, &*sub))
        }
        _ => Err("% Not a VRF command".into()),
    }
}

// ---- what the show commands print ----

fn names_of(v: &Vrf) -> String {
    if v.members.is_empty() { "-".to_string() } else { v.members.iter().map(|m| ifname::short(m)).collect::<Vec<_>>().join(", ") }
}

fn brief(d: &Device) -> String {
    if d.vrfs.is_empty() {
        return "No VRFs are defined.".into();
    }
    let mut s = format!("{:<20} {:<18} {}\n", "Name", "Default RD", "Interfaces");
    for v in &d.vrfs {
        let _ = writeln!(s, "{:<20} {:<18} {}", v.name, v.rd.as_deref().unwrap_or("<not set>"), names_of(v));
    }
    s.trim_end().to_string()
}

fn detail(d: &Device) -> String {
    if d.vrfs.is_empty() {
        return "No VRFs are defined.".into();
    }
    let list = |l: &[String]| if l.is_empty() { "none".to_string() } else { l.join(", ") };
    let mut s = String::new();
    for v in &d.vrfs {
        let _ = writeln!(s, "VRF {} (default RD {})", v.name, v.rd.as_deref().unwrap_or("not set"));
        if !v.description.is_empty() {
            let _ = writeln!(s, "  Description: {}", v.description);
        }
        let _ = writeln!(s, "  Interfaces: {}", names_of(v));
        let _ = writeln!(s, "  Export route targets: {}", list(&v.export));
        let _ = writeln!(s, "  Import route targets: {}", list(&v.import));
        let _ = writeln!(s, "  Static routes: {}", v.routes.len());
        let ospf = v.ospf.as_ref().map_or("no".to_string(), |o| format!("process {}", o.process));
        let bgp = v.bgp.as_ref().map_or("no".to_string(), |b| format!("AS {}", b.asn));
        let _ = writeln!(s, "  OSPF: {ospf}; BGP: {bgp}");
    }
    s.trim_end().to_string()
}

fn interfaces(d: &Device) -> String {
    let mut s = format!("{:<22} {:<18} {:<18} {}\n", "Interface", "IP address", "VRF", "Protocol");
    let mut any = false;
    for i in netlabcraft_sim::l3_ifaces_all(d).into_iter().filter(|i| i.vrf.is_some()) {
        any = true;
        let _ = writeln!(s, "{:<22} {:<18} {:<18} configured", ifname::short(&i.name), i.net.addr, i.vrf.as_deref().unwrap_or(""));
    }
    if !any {
        return "No interface is in a VRF with an address.".into();
    }
    s.trim_end().to_string()
}

fn arp(t: &Topology, sim: &Sim, d: &Device, vrf: &str) -> String {
    let mut s = format!("{:<17} {:<10} {:<19} {}\n", "Address", "Age (min)", "MAC address", "Interface");
    let mut rows: Vec<(Ipv4Addr, String, String, String)> = netlabcraft_sim::l3_ifaces_all(d)
        .into_iter()
        .filter(|i| i.vrf.as_deref() == Some(vrf))
        .filter(|i| i.port >= usize::MAX / 2 || netlabcraft_sim::port_up(t, netlabcraft_model::End { device: d.id, port: i.port }))
        .map(|i| (i.net.addr, "-".to_string(), i.mac.to_string(), i.name))
        .collect();
    if let Some(table) = sim.state(d.id).and_then(|st| st.vrf_arp.get(vrf)) {
        for (ip, e) in table {
            let age = sim.now_us().saturating_sub(e.learned_us) / 60_000_000;
            rows.push((*ip, age.to_string(), e.mac.to_string(), e.iface.clone()));
        }
    }
    rows.sort();
    for (ip, age, mac, iface) in rows {
        let _ = writeln!(s, "{:<17} {:<10} {:<19} {iface}", ip.to_string(), age, mac);
    }
    s.trim_end().to_string()
}

// ---- running configuration ----

/// The `ip vrf` blocks.
pub(crate) fn definitions(s: &mut String, d: &Device) {
    for v in &d.vrfs {
        let _ = writeln!(s, "ip vrf {}", v.name);
        if !v.description.is_empty() {
            let _ = writeln!(s, " description {}", v.description);
        }
        if let Some(rd) = &v.rd {
            let _ = writeln!(s, " rd {rd}");
        }
        for t in &v.export {
            let _ = writeln!(s, " route-target export {t}");
        }
        for t in &v.import {
            let _ = writeln!(s, " route-target import {t}");
        }
        s.push_str("!\n");
    }
}

/// ` ip vrf forwarding NAME` under an interface that is in a VRF (before its address).
pub(crate) fn iface_line(s: &mut String, d: &Device, name: &str) {
    if let Some(v) = d.vrf_of(name) {
        let _ = writeln!(s, " ip vrf forwarding {v}");
    }
}

/// The `ip route vrf` lines.
pub(crate) fn static_routes(s: &mut String, d: &Device) {
    for v in &d.vrfs {
        for r in &v.routes {
            let via = match &r.via {
                NextHop::Address(a) => a.to_string(),
                NextHop::Interface(n) => n.clone(),
            };
            let distance = r.distance.map_or(String::new(), |a| format!(" {a}"));
            let tag = if r.tag == 0 { String::new() } else { format!(" tag {}", r.tag) };
            let _ = writeln!(s, "ip route vrf {} {} {} {via}{distance}{tag}", v.name, r.net.addr, r.net.mask());
        }
        if !v.routes.is_empty() {
            s.push_str("!\n");
        }
    }
}

/// The section of a configuration a device writes for one routing process, taken from the text
/// the device would print with `process` as its own: from the line that starts it to the `!`.
fn section_of(text: &str, header: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if !inside {
            inside = line.starts_with(header);
            continue;
        }
        if line == "!" {
            break;
        }
        out.push(line.to_string());
    }
    out
}

/// `router ospf N vrf NAME` blocks.
pub(crate) fn ospf_sections(s: &mut String, d: &Device) {
    for v in d.vrfs.iter().filter(|v| v.ospf.is_some()) {
        let Some(o) = &v.ospf else { continue };
        let mut alone = d.clone();
        alone.vrfs.clear();
        alone.ospf = v.ospf.clone();
        let text = render::running_config(&alone);
        let _ = writeln!(s, "router ospf {} vrf {}", o.process, v.name);
        for line in section_of(&text, &format!("router ospf {}", o.process)) {
            let _ = writeln!(s, "{line}");
        }
        s.push_str("!\n");
    }
}

/// The text of `router bgp N` with each VRF's address family added at its end.
pub(crate) fn with_bgp_families(d: &Device, text: String) -> String {
    if !d.vrfs.iter().any(|v| v.bgp.is_some()) || !text.ends_with("!\n") {
        return text;
    }
    let mut out = text.trim_end_matches("!\n").to_string();
    for v in d.vrfs.iter().filter(|v| v.bgp.is_some()) {
        let mut alone = d.clone();
        alone.vrfs.clear();
        alone.bgp = v.bgp.clone();
        let mut body = String::new();
        super::bgp::running_config(&mut body, &alone);
        let _ = writeln!(out, " address-family ipv4 vrf {}", v.name);
        for line in section_of(&body, "router bgp ") {
            let _ = writeln!(out, " {line}");
        }
        out.push_str(" exit-address-family\n");
    }
    out.push_str("!\n");
    out
}
