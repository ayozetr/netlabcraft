//! Routing commands kept apart from the big dispatcher: loopback interfaces
//! (`interface Loopback N`).

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::routing::loopback_number;
use netlabcraft_model::{MAX_DESCRIPTION, VirtualLinkOpts};

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

/// A static route's words with a trailing `tag N` taken off: the words before it and the tag
/// (0 when none was typed).
pub(super) fn split_tag(text: &str) -> Result<(String, u32), String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        [rest @ .., kw, n] if kw.eq_ignore_ascii_case("tag") => {
            let tag = n.parse::<u32>().ok().filter(|t| *t >= 1).ok_or("% The tag is 1-4294967295")?;
            Ok((rest.join(" "), tag))
        }
        _ => Ok((words.join(" "), 0)),
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
        crate::exec::knobs::iface_lines(s, d, &l.name());
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
    let state = if l.shutdown { "administratively down, line protocol is down" } else { "up, line protocol is up" };
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

/// The authentication type a typed word names: none given means a clear-text password.
fn auth_kind_word(text: &str, allow_null: bool) -> Result<netlabcraft_model::OspfAuthKind, String> {
    use netlabcraft_model::OspfAuthKind as K;
    let w = text.trim().to_ascii_lowercase();
    if w.is_empty() {
        Ok(K::Simple)
    } else if "message-digest".starts_with(&w) {
        Ok(K::Md5)
    } else if allow_null && "null".starts_with(&w) {
        Ok(K::Null)
    } else {
        Err(format!("% Authentication here is {}", if allow_null { "message-digest or null" } else { "message-digest" }))
    }
}

/// `area N authentication [message-digest]` and its `no` form, in router OSPF mode.
pub(super) fn ospf_area_auth(c: &mut Ctx, on: bool, args: &[String]) -> R {
    let area: u32 = args.first().and_then(|a| a.trim().parse().ok()).ok_or("% Area number 0-4294967295")?;
    let kind = auth_kind_word(args.get(1).map(String::as_str).unwrap_or(""), false)?;
    let o = c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?;
    let a = o.areas.entry(area).or_default();
    a.auth = on.then_some(kind);
    if a.is_default() {
        o.areas.remove(&area);
    }
    Ok(String::new())
}

/// Whether a word can follow `authentication` on a virtual link: `message-digest` or `null`.
fn is_auth_word(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    !l.is_empty() && ("message-digest".starts_with(&l) || "null".starts_with(&l))
}

/// Takes a number off the words when one comes next.
fn skip_number(it: &mut std::iter::Peekable<std::str::SplitWhitespace<'_>>) {
    let _ = it.next_if(|n| n.parse::<u32>().is_ok());
}

/// The seconds a virtual-link timer takes (1-65535).
fn vlink_seconds(w: Option<&str>, what: &str) -> Result<u16, String> {
    w.and_then(|w| w.parse::<u16>().ok()).filter(|n| *n >= 1).ok_or_else(|| format!("% The {what} is 1-65535 seconds"))
}

/// The options after `area N virtual-link ROUTER-ID`: `hello-interval`, `dead-interval`,
/// `retransmit-interval`, `transmit-delay`, `authentication [message-digest|null]`,
/// `authentication-key` and `message-digest-key ID md5 KEY`. With `on` false they are put back
/// to their defaults (the `no` form).
fn vlink_options(o: &mut VirtualLinkOpts, text: &str, on: bool) -> Result<(), String> {
    let mut it = text.split_whitespace().peekable();
    while let Some(w) = it.next() {
        match w.to_ascii_lowercase().as_str() {
            "hello-interval" => {
                o.hello = if on { Some(vlink_seconds(it.next(), "hello interval")?) } else { None };
                if !on {
                    skip_number(&mut it);
                }
            }
            "dead-interval" => {
                o.dead = if on { Some(vlink_seconds(it.next(), "dead interval")?) } else { None };
                if !on {
                    skip_number(&mut it);
                }
            }
            "retransmit-interval" => {
                o.retransmit = if on { Some(vlink_seconds(it.next(), "retransmit interval")?) } else { None };
                if !on {
                    skip_number(&mut it);
                }
            }
            "transmit-delay" => {
                o.transmit_delay = if on { Some(vlink_seconds(it.next(), "transmit delay")?) } else { None };
                if !on {
                    skip_number(&mut it);
                }
            }
            "authentication" => {
                let word = it.next_if(|n| is_auth_word(n)).unwrap_or("");
                o.auth = if on { Some(auth_kind_word(word, true)?) } else { None };
            }
            "authentication-key" => {
                if on {
                    let key = it.next().ok_or("% The password is 1-8 characters")?;
                    if key.chars().count() > 8 {
                        return Err("% The password is 1-8 characters".into());
                    }
                    o.auth_key = Some(key.to_string());
                } else {
                    o.auth_key = None;
                }
            }
            "message-digest-key" => {
                if on {
                    let n: u8 = it.next().and_then(|n| n.parse().ok()).filter(|n| *n >= 1).ok_or("% The key id is 1-255")?;
                    if !it.next().is_some_and(|m| m.eq_ignore_ascii_case("md5")) {
                        return Err("% Only md5 keys: message-digest-key ID md5 KEY".into());
                    }
                    let key = it.next().filter(|k| (1..=16).contains(&k.chars().count())).ok_or("% The key is 1-16 characters")?;
                    if !o.md5_keys.contains_key(&n) && o.md5_keys.len() >= netlabcraft_model::MAX_OSPF_MD5_KEYS {
                        return Err("% Too many message-digest keys on this virtual link".into());
                    }
                    o.md5_keys.insert(n, key.to_string());
                } else if let Some(n) = it.next_if(|n| n.parse::<u8>().is_ok()) {
                    if let Ok(n) = n.parse::<u8>() {
                        o.md5_keys.remove(&n);
                    }
                } else {
                    o.md5_keys.clear();
                }
            }
            other => {
                return Err(format!(
                    "% `{other}` is not a virtual-link option: hello-interval, dead-interval, retransmit-interval, transmit-delay, authentication, authentication-key or message-digest-key"
                ));
            }
        }
    }
    Ok(())
}

/// `area N nssa [no-summary] [default-information-originate]`, `area N default-cost C` and
/// `area N virtual-link ROUTER-ID`, and their `no` forms, in router OSPF mode.
pub(super) fn ospf_area_type(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let area: u32 = args.first().and_then(|a| a.trim().parse().ok()).ok_or("% Area number 1-4294967295")?;
    let rest = args.get(1).map(|a| a.trim()).unwrap_or("");
    if area == 0 {
        return Err("% The backbone (area 0) can't be a stub area, an NSSA or the transit area of a virtual link".into());
    }
    let o = c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?;
    match id {
        Id::AreaNssa | Id::NoAreaNssa => {
            let (mut no_summary, mut originate, mut no_redist) = (false, false, false);
            for w in rest.split_whitespace() {
                let w = w.to_ascii_lowercase();
                if w.starts_with("no-s") && "no-summary".starts_with(&w) {
                    no_summary = true;
                } else if w.starts_with("no-r") && "no-redistribution".starts_with(&w) {
                    no_redist = true;
                } else if w.starts_with('d') && "default-information-originate".starts_with(&w) {
                    originate = true;
                } else {
                    return Err(format!("% `{w}` is not an NSSA option: no-summary, no-redistribution or default-information-originate"));
                }
            }
            if id == Id::AreaNssa {
                if o.areas.get(&area).is_some_and(|a| a.stub) {
                    return Err(format!("% Area {area} is a stub area: take that off first with `no area {area} stub`"));
                }
                let a = o.areas.entry(area).or_default();
                a.nssa = true;
                a.no_summary = no_summary;
                a.nssa_default = originate;
                a.nssa_no_redist = no_redist;
            } else if let Some(a) = o.areas.get_mut(&area).filter(|a| a.nssa) {
                if rest.is_empty() {
                    a.nssa = false;
                    a.no_summary = false;
                    a.nssa_default = false;
                    a.nssa_no_redist = false;
                    a.default_cost = None;
                } else {
                    a.no_summary &= !no_summary;
                    a.nssa_default &= !originate;
                    a.nssa_no_redist &= !no_redist;
                }
            }
        }
        Id::AreaDefaultCost => {
            let cost: u32 = rest.parse().ok().filter(|v| *v <= 16_777_215).ok_or("% The cost is 0-16777215")?;
            let a = o
                .areas
                .get_mut(&area)
                .filter(|a| a.stub || a.nssa)
                .ok_or_else(|| format!("% Area {area} is not a stub area or an NSSA: set that first"))?;
            a.default_cost = (cost != 1).then_some(cost);
        }
        Id::NoAreaDefaultCost => {
            if let Some(a) = o.areas.get_mut(&area) {
                a.default_cost = None;
            }
        }
        Id::AreaVirtualLink => {
            let peer = parse_ip(rest).map_err(|e| format!("% {e}"))?;
            let extra = args.get(2).map(|a| a.trim()).unwrap_or("");
            let mut opts =
                o.areas.get(&area).and_then(|a| a.vlink_opts.iter().find(|v| v.peer == peer)).cloned().unwrap_or_else(|| VirtualLinkOpts::new(peer));
            vlink_options(&mut opts, extra, true)?;
            let a = o.areas.entry(area).or_default();
            if !a.virtual_links.contains(&peer) {
                if a.virtual_links.len() >= netlabcraft_model::MAX_VIRTUAL_LINKS {
                    return Err("% Too many virtual links in this area".into());
                }
                a.virtual_links.push(peer);
            }
            a.vlink_opts.retain(|v| v.peer != peer);
            if !opts.is_empty() {
                a.vlink_opts.push(opts);
            }
        }
        _ => {
            let peer = parse_ip(rest).map_err(|e| format!("% {e}"))?;
            let extra = args.get(2).map(|a| a.trim()).unwrap_or("");
            if let Some(a) = o.areas.get_mut(&area) {
                if extra.is_empty() {
                    a.virtual_links.retain(|p| *p != peer);
                    a.vlink_opts.retain(|v| v.peer != peer);
                } else if let Some(v) = a.vlink_opts.iter_mut().find(|v| v.peer == peer) {
                    vlink_options(v, extra, false)?;
                    a.vlink_opts.retain(|v| !v.is_empty());
                }
            }
        }
    }
    if o.areas.get(&area).is_some_and(netlabcraft_model::OspfArea::is_default) {
        o.areas.remove(&area);
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
        Id::IpOspfAuth => {
            let kind = auth_kind_word(arg(0), true)?;
            each_ospf_if(c, |e| e.auth = Some(kind))
        }
        Id::NoIpOspfAuth => each_ospf_if(c, |e| e.auth = None),
        Id::IpOspfAuthKey => {
            let key = arg(0).to_string();
            if key.chars().count() > 8 {
                return Err("% The clear-text password is at most 8 characters".into());
            }
            each_ospf_if(c, |e| e.auth_key = Some(key.clone()))
        }
        Id::NoIpOspfAuthKey => each_ospf_if(c, |e| e.auth_key = None),
        Id::IpOspfMd5Key => {
            let n: u8 = arg(0).parse().ok().filter(|n| *n >= 1).ok_or("% The key id is 1-255")?;
            if arg(1).is_empty() || arg(1).chars().count() > 16 {
                return Err("% The key is 1-16 characters".into());
            }
            let key = arg(1).to_string();
            let names = if_names(c)?;
            let d = c.dm()?;
            if names.iter().any(|name| {
                d.ospf_if.get(name).is_some_and(|e| !e.md5_keys.contains_key(&n) && e.md5_keys.len() >= netlabcraft_model::MAX_OSPF_MD5_KEYS)
            }) {
                return Err("% Too many message-digest keys on this interface".into());
            }
            each_ospf_if(c, |e| {
                e.md5_keys.insert(n, key.clone());
            })
        }
        Id::NoIpOspfMd5Key => {
            let n: u8 = arg(0).parse().ok().filter(|n| *n >= 1).ok_or("% The key id is 1-255")?;
            each_ospf_if(c, |e| {
                e.md5_keys.remove(&n);
            })
        }
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
pub(super) fn ospf_if_name(c: &Ctx, text: &str) -> Result<String, String> {
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

/// What `show ip ospf database` keeps of the LSAs it lists.
#[derive(Clone, Copy)]
enum DbFilter {
    All,
    /// The link-state ID.
    Id(std::net::Ipv4Addr),
    /// The advertising router.
    Adv(std::net::Ipv4Addr),
}

/// LSAs in full (`show ip ospf database router`): every LSA of a kind, or of every kind, per area,
/// with its links, attached routers or metric, optionally only the one with a link-state ID, the
/// ones this router originated (`self-originate`) or the ones an `adv-router` advertised.
fn ospf_database_detail(
    d: &netlabcraft_model::Device,
    process: u16,
    rid: &str,
    my_rid: Option<std::net::Ipv4Addr>,
    lsas: &[netlabcraft_sim::ospf_lsdb::Lsa],
    words: &[&str],
) -> R {
    use netlabcraft_sim::ospf_lsdb::{LinkKind, LsaKind};
    use std::fmt::Write as _;
    const USAGE: &str =
        "% show ip ospf database [router|network|summary|asbr-summary|external|nssa-external] [A.B.C.D | self-originate | adv-router A.B.C.D]";
    let kw = |w: &str, k: &str| !w.is_empty() && k.starts_with(w);
    let first = words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
    let named = [
        ("router", LsaKind::Router),
        ("network", LsaKind::Network),
        ("summary", LsaKind::Summary),
        ("asbr-summary", LsaKind::AsbrSummary),
        ("external", LsaKind::External),
        ("nssa-external", LsaKind::Nssa),
    ]
    .into_iter()
    .find(|(n, _)| kw(&first, n));
    if named.is_none() && !kw(&first, "self-originate") && !kw(&first, "adv-router") {
        return Err(USAGE.into());
    }
    let kind = named.map(|(_, k)| k);
    let rest = if named.is_some() { words.get(1..).unwrap_or(&[]) } else { words };
    let filter = match rest {
        [] => DbFilter::All,
        [w] if kw(&w.to_ascii_lowercase(), "self-originate") => DbFilter::Adv(my_rid.ok_or("% This router has no router ID yet")?),
        [w] => DbFilter::Id(w.parse::<std::net::Ipv4Addr>().map_err(|_| format!("% `{w}` is not a link-state ID (A.B.C.D) or self-originate"))?),
        [w, a] if kw(&w.to_ascii_lowercase(), "adv-router") => {
            DbFilter::Adv(a.parse::<std::net::Ipv4Addr>().map_err(|_| format!("% `{a}` is not a router ID (A.B.C.D)"))?)
        }
        _ => return Err(USAGE.into()),
    };
    if kind.is_none() && !matches!(filter, DbFilter::Adv(_)) {
        return Err(USAGE.into());
    }
    let mut s = String::new();
    let mut last: Option<(LsaKind, Option<u32>)> = None;
    let shown = |l: &netlabcraft_sim::ospf_lsdb::Lsa| {
        kind.is_none_or(|k| l.kind == k)
            && match filter {
                DbFilter::All => true,
                DbFilter::Id(i) => l.id == i,
                DbFilter::Adv(a) => l.adv == a,
            }
    };
    for l in lsas.iter().filter(|l| shown(l)) {
        if last != Some((l.kind, l.area)) {
            let title = match l.kind {
                LsaKind::Router => "Router link states",
                LsaKind::Network => "Network link states",
                LsaKind::Summary => "Summary link states",
                LsaKind::AsbrSummary => "ASBR summary link states",
                LsaKind::External => "External link states",
                LsaKind::Nssa => "Type 7 external link states",
            };
            let area = l.area.map_or(String::new(), |a| format!(", area {a}"));
            let _ = writeln!(s, "{}{title}{area} (process {process}, router {rid})", if last.is_some() { "\n" } else { "" });
            last = Some((l.kind, l.area));
        }
        let seq = format!("0x{:08x}", l.seq);
        let age = format!("age {} s", l.age);
        match l.kind {
            LsaKind::Router => {
                let _ = writeln!(s, "Router {}, sequence {seq}, {} link{}, {age}", l.id, l.count, if l.count == 1 { "" } else { "s" });
                for k in &l.links {
                    let line = match k.kind {
                        LinkKind::PointToPoint => format!("point-to-point to router {} through {}", k.id, k.data),
                        LinkKind::Transit => format!("transit network, designated router {} (this router {})", k.id, k.data),
                        LinkKind::Stub => format!("stub network {}/{}", k.id, netlabcraft_core::mask_to_prefix(k.data).unwrap_or(0)),
                    };
                    let _ = writeln!(s, "  {line}, metric {}", k.metric);
                }
            }
            LsaKind::Network => {
                let net = netlabcraft_core::Ipv4Net::new(l.id, u8::try_from(l.count).unwrap_or(32)).network();
                let _ =
                    writeln!(s, "Network {}/{}, designated router {}, sequence {seq}, advertised by {}, {age}", net.addr, net.prefix, l.id, l.adv);
                let list: Vec<String> = l.attached.iter().map(ToString::to_string).collect();
                let _ = writeln!(s, "  attached routers: {}", list.join(", "));
            }
            LsaKind::Summary => {
                let _ = writeln!(s, "Network {}/{}, sequence {seq}, advertised by {}, metric {}, {age}", l.id, l.count, l.adv, l.metric);
            }
            LsaKind::AsbrSummary => {
                let _ = writeln!(s, "AS boundary router {}, sequence {seq}, advertised by {}, metric {}, {age}", l.id, l.adv, l.metric);
            }
            LsaKind::External => {
                let t = if l.type1 { 1 } else { 2 };
                let _ = writeln!(
                    s,
                    "Network {}/{}, sequence {seq}, advertised by {}, type {t} metric {}, tag {}, {age}",
                    l.id, l.count, l.adv, l.metric, l.tag
                );
            }
            LsaKind::Nssa => {
                let t = if l.type1 { 1 } else { 2 };
                let _ = writeln!(
                    s,
                    "Network {}/{}, sequence {seq}, advertised by {}, type {t} metric {}, tag {}, {age}",
                    l.id, l.count, l.adv, l.metric, l.tag
                );
                match l.translator {
                    Some(tr) => {
                        let _ = writeln!(s, "  the border router {tr} translates it into a type 5 LSA for the other areas");
                    }
                    None => {
                        let _ = writeln!(s, "  stays inside the area (not translated)");
                    }
                }
            }
        }
    }
    if s.is_empty() {
        let name = match kind {
            Some(LsaKind::Router) => "router ",
            Some(LsaKind::Network) => "network ",
            Some(LsaKind::Summary) => "summary ",
            Some(LsaKind::AsbrSummary) => "ASBR summary ",
            Some(LsaKind::External) => "external ",
            Some(LsaKind::Nssa) => "NSSA external ",
            None => "",
        };
        let what = match filter {
            DbFilter::All => String::new(),
            DbFilter::Id(i) => format!(" with link-state ID {i}"),
            DbFilter::Adv(a) => format!(" advertised by {a}"),
        };
        return Ok(format!("No {name}LSAs{what} in the database of {}.", d.name));
    }
    Ok(s.trim_end().to_string())
}

pub(super) fn ospf_database(c: &mut Ctx, what: &str) -> R {
    use netlabcraft_sim::ospf_lsdb::LsaKind;
    use std::fmt::Write as _;
    let d = c.d()?.clone();
    let Some(o) = &d.ospf else { return Ok("OSPF is not running here.".into()) };
    let st = c.sim.ospf_state(c.t);
    let rid = st.router_ids.get(&d.id).map_or("unset".to_string(), ToString::to_string);
    let lsas = c.sim.lsdb(c.t, d.id);
    let words: Vec<&str> = what.split_whitespace().collect();
    if !words.is_empty() {
        return ospf_database_detail(&d, o.process, &rid, st.router_ids.get(&d.id).copied(), &lsas, &words);
    }
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
                LsaKind::AsbrSummary => "ASBR summary LSAs (type 4)",
                LsaKind::External => "AS external LSAs (type 5)",
                LsaKind::Nssa => "NSSA external LSAs (type 7)",
            };
            let area = l.area.map_or(String::new(), |a| format!(", area {a}"));
            let last_col = match l.kind {
                LsaKind::Router => "Links",
                LsaKind::AsbrSummary => "Metric",
                _ => "Prefix",
            };
            let _ = write!(s, "\n{title}{area}\n{:<16} {:<16} {:>5} {:<11} {}\n", "Link ID", "Advertised by", "Age", "Seq#", last_col);
            last = Some((l.kind, l.area));
        }
        let count = match l.kind {
            LsaKind::Router => l.count.to_string(),
            LsaKind::AsbrSummary => l.metric.to_string(),
            _ => format!("/{}", l.count),
        };
        let _ = writeln!(s, "{:<16} {:<16} {:>5} {:<11} {count}", l.id.to_string(), l.adv.to_string(), l.age, format!("0x{:08x}", l.seq));
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
