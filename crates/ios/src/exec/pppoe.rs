//! PPPoE on routers: the access concentrator (`bba-group pppoe`, `interface Virtual-Template`,
//! `pppoe enable`) and the client (`interface Dialer`, `pppoe-client dial-pool-number`), with
//! `show pppoe session`, `show pppoe summary` and `clear pppoe all`.
//!
//! The exchange itself is the simulator's (`netlabcraft_sim::pppoe`): real discovery and PPP
//! frames between the router and a Server or another router. A router dials when the last piece
//! of a dialer's setup is in place (an `encapsulation ppp`, `ip address negotiated` dialer in a
//! pool whose port is up), and the lab keeps the session on the dialer.

use std::fmt::Write as _;

use netlabcraft_core::{DeviceId, Ipv4Net, parse_ip};
use netlabcraft_model::pppoe::{
    BbaGroup, Dialer, DialerSession, GLOBAL_GROUP, MAX_CREDENTIAL, MAX_GROUPS, MAX_MTU, MAX_NUMBER, MAX_SESSIONS, MIN_MTU, PppoeSession, PppoeUser,
};
use netlabcraft_model::{Device, DeviceKind, End, MAX_DESCRIPTION, PppAuth, Topology};
use netlabcraft_sim::pppoe::DialCreds;
use netlabcraft_sim::{Sim, port_up};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// Simulated time the clock runs after a session starts or ends, so the last frames arrive.
const SETTLE_US: u64 = 50_000;

fn router_only(c: &Ctx) -> Result<(), String> {
    if c.d()?.kind != DeviceKind::Router {
        return Err("% PPPoE on interfaces is a router feature".into());
    }
    Ok(())
}

fn name_ok(text: &str) -> Result<String, String> {
    if text.is_empty() || text.chars().count() > MAX_CREDENTIAL || !text.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% Give a name of 1-{MAX_CREDENTIAL} printable characters"));
    }
    Ok(text.to_string())
}

fn number_ok(text: &str, what: &str) -> Result<u32, String> {
    text.parse::<u32>().ok().filter(|n| *n <= MAX_NUMBER).ok_or_else(|| format!("% {what} is a number from 0 to {MAX_NUMBER}"))
}

fn mtu_ok(text: &str) -> Result<u16, String> {
    text.parse::<u16>().ok().filter(|m| (MIN_MTU..=MAX_MTU).contains(m)).ok_or_else(|| format!("% The MTU is {MIN_MTU}-{MAX_MTU}"))
}

/// `interface Dialer N`: created on first use.
pub(super) fn enter_dialer(c: &mut Ctx, n: u32) -> R {
    router_only(c)?;
    c.dm()?.pppoe_router.dialer_mut(n).ok_or("% Too many dialer interfaces")?;
    c.cli.mode = Mode::Dialer(n);
    Ok(String::new())
}

/// `no interface Dialer N`: its session ends first.
pub(super) fn delete_dialer(c: &mut Ctx, n: u32) -> R {
    router_only(c)?;
    if !c.d()?.pppoe_router.dialers.iter().any(|x| x.number == n) {
        return Err(format!("% There is no interface Dialer{n}"));
    }
    end_session(c, n);
    c.dm()?.pppoe_router.dialers.retain(|x| x.number != n);
    Ok(String::new())
}

/// `interface Virtual-Template N`: created on first use.
pub(super) fn enter_template(c: &mut Ctx, n: u32) -> R {
    router_only(c)?;
    c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?;
    c.cli.mode = Mode::VirtualTemplate(n);
    Ok(String::new())
}

/// `no interface Virtual-Template N`: the groups that used it have no template.
pub(super) fn delete_template(c: &mut Ctx, n: u32) -> R {
    router_only(c)?;
    let r = &mut c.dm()?.pppoe_router;
    let before = r.templates.len();
    r.templates.retain(|t| t.number != n);
    if r.templates.len() == before {
        return Err(format!("% There is no interface Virtual-Template{n}"));
    }
    for g in &mut r.groups {
        if g.template == Some(n) {
            g.template = None;
        }
    }
    Ok(String::new())
}

fn group_name(c: &Ctx) -> Result<String, String> {
    match &c.cli.mode {
        Mode::BbaGroup(n) => Ok(n.clone()),
        _ => Err("% Not in PPPoE group mode".into()),
    }
}

fn template_number(c: &Ctx) -> Result<u32, String> {
    match c.cli.mode {
        Mode::VirtualTemplate(n) => Ok(n),
        _ => Err("% Not in a virtual template".into()),
    }
}

fn dialer_number(c: &Ctx) -> Result<u32, String> {
    match c.cli.mode {
        Mode::Dialer(n) => Ok(n),
        _ => Err("% Not in a dialer interface".into()),
    }
}

/// The interface an `ip unnumbered` names: an Ethernet port or a loopback, by its full name.
fn unnumbered_target(d: &Device, text: &str) -> Result<String, String> {
    if let Some(n) = netlabcraft_model::routing::loopback_number(text) {
        let name = format!("Loopback{n}");
        return if d.loopbacks.iter().any(|l| l.number == n) { Ok(name) } else { Err(format!("% There is no interface {name}")) };
    }
    let parsed = netlabcraft_model::ifname::parse(text).ok_or_else(|| format!("% `{text}` is not an interface name"))?;
    let name = parsed.port_name();
    if parsed.sub.is_some() || d.port_index(&name).is_none() {
        return Err(format!("% {} has no interface {name}", d.name));
    }
    Ok(name)
}

/// The index of a port by its full name.
fn port_of(d: &Device, name: &str) -> Option<usize> {
    d.ports.iter().position(|p| p.name == name)
}

/// Ends the session of dialer `n` with a Terminate-Request and a PADT, then forgets it.
fn end_session(c: &mut Ctx, n: u32) {
    let Some(d) = c.t.device(c.dev) else { return };
    let Some(ds) = d.pppoe_router.dialers.iter().find(|x| x.number == n).and_then(|x| x.session.clone()) else { return };
    if let Some(port) = port_of(d, &ds.port) {
        c.sim.pppoe_hangup_port(c.t, c.dev, port);
        c.sim.run_for(c.t, SETTLE_US);
    }
    if let Some(x) = c.t.device_mut(c.dev).and_then(|d| d.pppoe_router.dialers.iter_mut().find(|x| x.number == n)) {
        x.session = None;
    }
}

/// Ends the session that runs over a port.
fn end_port_session(c: &mut Ctx, port_name: &str) {
    let Some(n) =
        c.t.device(c.dev)
            .and_then(|d| d.pppoe_router.dialers.iter().find(|x| x.session.as_ref().is_some_and(|s| s.port == port_name)).map(|x| x.number))
    else {
        return;
    };
    end_session(c, n);
}

/// Runs the commands of this feature.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    router_only(c)?;
    match id {
        Id::BbaGroup => {
            let name = name_ok(arg(0))?;
            let r = &mut c.dm()?.pppoe_router;
            if !r.groups.iter().any(|g| g.name == name) {
                if r.groups.len() >= MAX_GROUPS {
                    return Err(format!("% At most {MAX_GROUPS} PPPoE groups"));
                }
                r.groups.push(BbaGroup { name: name.clone(), template: None });
            }
            c.cli.mode = Mode::BbaGroup(name);
            Ok(String::new())
        }
        Id::NoBbaGroup => {
            let name = arg(0).to_string();
            let r = &mut c.dm()?.pppoe_router;
            let before = r.groups.len();
            r.groups.retain(|g| g.name != name);
            if r.groups.len() == before {
                return Err(format!("% There is no PPPoE group {name}"));
            }
            Ok(String::new())
        }
        Id::BbaTemplate | Id::NoBbaTemplate => {
            let name = group_name(c)?;
            let template = if id == Id::BbaTemplate { Some(number_ok(arg(0), "A virtual template")?) } else { None };
            let r = &mut c.dm()?.pppoe_router;
            let g = r.groups.iter_mut().find(|g| g.name == name).ok_or("% That PPPoE group no longer exists")?;
            g.template = template;
            match template {
                Some(n) if r.template(n).is_none() => Ok(format!("Note: interface Virtual-Template{n} does not exist yet")),
                _ => Ok(String::new()),
            }
        }
        Id::VtIpAddress => {
            let n = template_number(c)?;
            let net = Ipv4Net::with_mask(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?, parse_ip(arg(1)).map_err(|e| format!("% {e}"))?)
                .map_err(|e| format!("% {e}"))?;
            let t = c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?;
            t.ip = Some(net);
            t.unnumbered = None;
            Ok(String::new())
        }
        Id::VtNoIpAddress => {
            let n = template_number(c)?;
            let t = c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?;
            t.ip = None;
            Ok(String::new())
        }
        Id::VtIpUnnumbered => {
            let n = template_number(c)?;
            let target = unnumbered_target(c.d()?, arg(0))?;
            let t = c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?;
            t.unnumbered = Some(target);
            t.ip = None;
            Ok(String::new())
        }
        Id::VtNoIpUnnumbered => {
            let n = template_number(c)?;
            let t = c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?;
            t.unnumbered = None;
            Ok(String::new())
        }
        Id::VtPeerPool | Id::VtNoPeerPool => {
            let n = template_number(c)?;
            let pool = if id == Id::VtPeerPool { Some(name_ok(arg(0))?) } else { None };
            let known = pool.as_ref().is_none_or(|p| c.d().is_ok_and(|d| d.crypto.ra.pools.iter().any(|x| &x.name == p)));
            let t = c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?;
            let note = match &pool {
                Some(p) if !known => format!("Note: pool {p} does not exist yet (`ip local pool`)"),
                _ => String::new(),
            };
            t.pool = pool;
            Ok(note)
        }
        Id::VtPppAuth | Id::VtNoPppAuth => {
            let n = template_number(c)?;
            let auth = if id == Id::VtPppAuth {
                Some(match arg(0).to_ascii_lowercase().as_str() {
                    "pap" => PppAuth::Pap,
                    "chap" => PppAuth::Chap,
                    _ => return Err("% Authentication is `pap` or `chap`".into()),
                })
            } else {
                None
            };
            c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?.auth = auth;
            Ok(String::new())
        }
        Id::VtMtu | Id::VtNoMtu => {
            let n = template_number(c)?;
            let mtu = if id == Id::VtMtu { Some(mtu_ok(arg(0))?) } else { None };
            c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?.mtu = mtu;
            Ok(String::new())
        }
        Id::VtDescription | Id::VtNoDescription => {
            let n = template_number(c)?;
            let text: String = if id == Id::VtDescription { arg(0).chars().take(MAX_DESCRIPTION).collect() } else { String::new() };
            c.dm()?.pppoe_router.template_mut(n).ok_or("% Too many virtual templates")?.description = text;
            Ok(String::new())
        }
        Id::DlEncapsulation
        | Id::DlNoEncapsulation
        | Id::DlIpNegotiated
        | Id::DlNoIpAddress
        | Id::DlChapHostname
        | Id::DlNoChapHostname
        | Id::DlChapPassword
        | Id::DlNoChapPassword
        | Id::DlPapSent
        | Id::DlNoPapSent
        | Id::DlPool
        | Id::DlNoPool
        | Id::DlMtu
        | Id::DlNoMtu
        | Id::DlDescription
        | Id::DlNoDescription
        | Id::DlShutdown
        | Id::DlNoShutdown => dialer_command(c, id, args),
        Id::PppoeEnable => {
            let group = if args.is_empty() { GLOBAL_GROUP.to_string() } else { name_ok(arg(0))? };
            let names = ethernet_ports(c)?;
            let known = c.d()?.pppoe_router.group(&group).is_some();
            let r = &mut c.dm()?.pppoe_router;
            for n in names {
                if r.enabled.len() >= netlabcraft_model::MAX_SUBIFS && !r.enabled.contains_key(&n) {
                    return Err("% Too many ports".into());
                }
                r.enabled.insert(n, group.clone());
            }
            Ok(if known { String::new() } else { format!("Note: PPPoE group {group} does not exist yet (`bba-group pppoe {group}`)") })
        }
        Id::NoPppoeEnable => {
            let names = ethernet_ports(c)?;
            let r = &mut c.dm()?.pppoe_router;
            for n in names {
                r.enabled.remove(&n);
            }
            Ok(String::new())
        }
        Id::PppoeClientDial => {
            let pool = number_ok(arg(0), "A dial pool")?;
            if pool == 0 {
                return Err("% A dial pool is 1-255".into());
            }
            let names = ethernet_ports(c)?;
            for n in &names {
                end_port_session(c, n);
            }
            let r = &mut c.dm()?.pppoe_router;
            for n in names {
                if r.dial_ports.len() >= netlabcraft_model::MAX_SUBIFS && !r.dial_ports.contains_key(&n) {
                    return Err("% Too many ports".into());
                }
                r.dial_ports.insert(n, pool);
            }
            Ok(dial_ready(c.t, c.sim, c.dev).join("\n"))
        }
        Id::NoPppoeClientDial => {
            let names = ethernet_ports(c)?;
            for n in &names {
                end_port_session(c, n);
            }
            let r = &mut c.dm()?.pppoe_router;
            for n in names {
                r.dial_ports.remove(&n);
            }
            Ok(String::new())
        }
        Id::ShowPppoeSession => show_session(c),
        Id::ShowPppoeSummary => Ok(summary(c.t, c.d()?)),
        Id::ClearPppoeAll => clear_all(c),
        _ => Err("% Not a PPPoE command".into()),
    }
}

/// The Ethernet ports the interface mode is configuring (full names).
fn ethernet_ports(c: &Ctx) -> Result<Vec<String>, String> {
    let names = c.mode_iface_names()?;
    for n in &names {
        let plain = n.contains('.');
        let ethernet = ["GigabitEthernet", "FastEthernet", "Ethernet"].iter().any(|p| n.starts_with(p));
        if plain || !ethernet {
            return Err(format!("% PPPoE runs on Ethernet ports, not on {n}"));
        }
    }
    Ok(names)
}

fn dialer_command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let n = dialer_number(c)?;
    // What takes the session down with it: a setting the session was made with.
    let drops = matches!(id, Id::DlNoEncapsulation | Id::DlNoIpAddress | Id::DlNoPool | Id::DlShutdown | Id::DlPool);
    if drops {
        end_session(c, n);
    }
    let d = c.dm()?.pppoe_router.dialer_mut(n).ok_or("% Too many dialer interfaces")?;
    match id {
        Id::DlEncapsulation => d.ppp = true,
        Id::DlNoEncapsulation => d.ppp = false,
        Id::DlIpNegotiated => d.negotiated = true,
        Id::DlNoIpAddress => d.negotiated = false,
        Id::DlChapHostname => d.chap_hostname = Some(name_ok(arg(0))?),
        Id::DlNoChapHostname => d.chap_hostname = None,
        Id::DlChapPassword => d.chap_password = Some(name_ok(arg(0))?),
        Id::DlNoChapPassword => d.chap_password = None,
        Id::DlPapSent => d.pap = Some(PppoeUser { name: name_ok(arg(0))?, password: name_ok(arg(1))? }),
        Id::DlNoPapSent => d.pap = None,
        Id::DlPool => {
            let pool = number_ok(arg(0), "A dialer pool")?;
            if pool == 0 {
                return Err("% A dialer pool is 1-255".into());
            }
            d.pool = Some(pool);
        }
        Id::DlNoPool => d.pool = None,
        Id::DlMtu => d.mtu = Some(mtu_ok(arg(0))?),
        Id::DlNoMtu => d.mtu = None,
        Id::DlDescription => d.description = arg(0).chars().take(MAX_DESCRIPTION).collect(),
        Id::DlNoDescription => d.description.clear(),
        Id::DlShutdown => d.shutdown = true,
        Id::DlNoShutdown => d.shutdown = false,
        _ => {}
    }
    // The last piece of the setup dials.
    let dials = matches!(
        id,
        Id::DlEncapsulation | Id::DlIpNegotiated | Id::DlChapHostname | Id::DlChapPassword | Id::DlPapSent | Id::DlPool | Id::DlNoShutdown
    );
    Ok(if dials { dial_ready(c.t, c.sim, c.dev).join("\n") } else { String::new() })
}

/// What a dialer proves itself with: the CHAP name (the router's own when none is set) and
/// secret, and the PAP name and secret.
fn creds_of(d: &Device, dl: &Dialer) -> DialCreds {
    let (pap_name, pap_secret) = dl.pap.as_ref().map(|p| (p.name.clone(), p.password.clone())).unwrap_or_default();
    DialCreds {
        chap_name: dl.chap_hostname.clone().unwrap_or_else(|| d.name.clone()),
        chap_secret: dl.chap_password.clone().unwrap_or_default(),
        pap_name,
        pap_secret,
    }
}

/// Dials from every port whose dialer is set up and not connected; returns a line per port.
pub(crate) fn dial_ready(t: &mut Topology, sim: &mut Sim, dev: DeviceId) -> Vec<String> {
    let Some(d) = t.device(dev) else { return Vec::new() };
    if d.kind != DeviceKind::Router {
        return Vec::new();
    }
    // (port index, port name, dialer number, credentials)
    let wanted: Vec<(usize, String, u32, DialCreds)> = d
        .pppoe_router
        .dial_ports
        .keys()
        .filter_map(|name| {
            let port = port_of(d, name)?;
            let dl = d.pppoe_router.dialer_of_port(name)?;
            let busy = dl.session.is_some() || d.pppoe_router.dialers.iter().any(|x| x.session.as_ref().is_some_and(|s| &s.port == name));
            let p = d.ports.get(port)?;
            let ready = dl.ppp && dl.negotiated && !dl.shutdown && !busy && !p.shutdown && port_up(t, End { device: dev, port });
            ready.then(|| (port, name.clone(), dl.number, creds_of(d, dl)))
        })
        .collect();
    let mut out = Vec::new();
    for (port, name, number, creds) in wanted {
        sim.topology_changed(t);
        let r = sim.pppoe_dial_port(t, dev, port, &creds);
        match r {
            Ok(res) => {
                // Let the last frames of the exchange arrive before the lab says the session is up.
                sim.run_for(t, SETTLE_US);
                let Some(server) = t.devices.iter().find(|x| x.ports.iter().any(|q| q.mac == res.server_mac)).map(|x| x.id) else {
                    out.push(format!("%PPPoE: Dialer{number}: the access concentrator is no longer in the lab"));
                    continue;
                };
                let session =
                    PppoeSession { server, address: res.address, gateway: res.gateway, auth: res.auth, id: res.session, server_mac: res.server_mac };
                let line =
                    format!("%PPPoE: Dialer{number} is up on {name}: address {}, session {}, concentrator {}", res.address, res.session, res.ac_name);
                if let Some(dl) = t.device_mut(dev).and_then(|x| x.pppoe_router.dialers.iter_mut().find(|x| x.number == number)) {
                    dl.session = Some(DialerSession { port: name, session });
                }
                out.push(line);
            }
            Err(e) => out.push(format!("%PPPoE: Dialer{number} did not connect on {name}: {e}")),
        }
    }
    sim.topology_changed(t);
    out
}

/// After a change on a router that is a concentrator: the sessions it no longer answers end
/// with a PADT (a port stopped enabling PPPoE, a group or a template went away).
pub(crate) fn settle(t: &mut Topology, sim: &mut Sim, dev: DeviceId) {
    if t.device(dev).is_none_or(|d| d.kind != DeviceKind::Router) {
        return;
    }
    sim.topology_changed(t);
    if sim.pppoe_unserved_down(t, dev) > 0 {
        sim.run_for(t, SETTLE_US);
    }
}

fn clear_all(c: &mut Ctx) -> R {
    let served = netlabcraft_sim::pppoe::served(c.t, c.dev).len();
    let dialed: Vec<u32> = c.d()?.pppoe_router.dialers.iter().filter(|x| x.session.is_some()).map(|x| x.number).collect();
    for n in &dialed {
        end_session(c, *n);
    }
    c.sim.pppoe_server_down(c.t, c.dev);
    c.sim.run_for(c.t, SETTLE_US);
    let mut out = format!("{} session{} ended", served + dialed.len(), if served + dialed.len() == 1 { "" } else { "s" });
    // The dialers dial again at once.
    for line in dial_ready(c.t, c.sim, c.dev) {
        let _ = write!(out, "\n{line}");
    }
    Ok(out)
}

fn mac_text(m: netlabcraft_core::MacAddr) -> String {
    m.dotted()
}

fn show_session(c: &mut Ctx) -> R {
    let d = c.d()?.clone();
    let mut out = String::new();
    let dialed: Vec<(&Dialer, &DialerSession)> = d.pppoe_router.dialers.iter().filter_map(|x| x.session.as_ref().map(|s| (x, s))).collect();
    let _ = writeln!(out, "PPPoE sessions this router dialed: {}", dialed.len());
    for (dl, ds) in &dialed {
        let state = match port_of(&d, &ds.port) {
            Some(port) if c.sim.pppoe_probe_port(c.t, c.dev, port) => "up",
            Some(_) => "no answer to the keep-alive",
            None => "port removed",
        };
        let _ = writeln!(
            out,
            " {} over {}: {state}; session {}, concentrator {}, address {}, gateway {}, {}",
            dl.name(),
            ds.port,
            ds.session.id,
            mac_text(ds.session.server_mac),
            ds.session.address,
            ds.session.gateway,
            ds.session.auth.name()
        );
    }
    let served = netlabcraft_sim::pppoe::served(c.t, c.dev);
    let _ = writeln!(out, "PPPoE sessions this router serves: {}", served.len());
    for s in &served {
        let port = d.ports.get(s.port).map_or("?", |p| p.name.as_str());
        let who = c.t.device(s.client).map_or("?", |x| x.name.as_str());
        let _ = writeln!(out, " session {} on {port}: {} ({who}), address {}, {}", s.session, mac_text(s.client_mac), s.address, s.interface);
    }
    Ok(out.trim_end().to_string())
}

fn summary(t: &Topology, d: &Device) -> String {
    let mut out = format!("PPPoE on {}\n", d.name);
    let served = netlabcraft_sim::pppoe::served(t, d.id);
    let _ = writeln!(out, "Concentrator: {} session{} open (at most {MAX_SESSIONS})", served.len(), if served.len() == 1 { "" } else { "s" });
    for (port, group) in &d.pppoe_router.enabled {
        let template = d.pppoe_router.template_of_port(port).map_or("none".to_string(), |t| t.name());
        let n = d.ports.iter().position(|p| &p.name == port).map_or(0, |i| served.iter().filter(|s| s.port == i).count());
        let _ = writeln!(out, " {port}: group {group}, template {template}, {n} session{}", if n == 1 { "" } else { "s" });
    }
    let dialed = d.pppoe_router.dialers.iter().filter(|x| x.session.is_some()).count();
    let _ = writeln!(out, "Client: {dialed} dialer session{} open", if dialed == 1 { "" } else { "s" });
    for (port, pool) in &d.pppoe_router.dial_ports {
        let dl = d.pppoe_router.dialer_of_port(port);
        let state = match dl {
            None => "no dialer uses this pool".to_string(),
            Some(x) if x.session.is_some() => format!("{} is up", x.name()),
            Some(x) => format!("{} is not connected", x.name()),
        };
        let _ = writeln!(out, " {port}: dial pool {pool}, {state}");
    }
    out.trim_end().to_string()
}

// ---- what the show commands print ----

/// The configuration lines of this feature, for `show running-config`: the groups, the virtual
/// templates and the dialers.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let r = &d.pppoe_router;
    for g in &r.groups {
        let _ = writeln!(s, "bba-group pppoe {}", g.name);
        if let Some(n) = g.template {
            let _ = writeln!(s, " virtual-template {n}");
        }
        let _ = writeln!(s, "!");
    }
    for t in &r.templates {
        let _ = writeln!(s, "interface {}", t.name());
        if !t.description.is_empty() {
            let _ = writeln!(s, " description {}", t.description);
        }
        if let Some(m) = t.mtu {
            let _ = writeln!(s, " mtu {m}");
        }
        match (&t.ip, &t.unnumbered) {
            (Some(ip), _) => {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            (None, Some(u)) => {
                let _ = writeln!(s, " ip unnumbered {u}");
            }
            (None, None) => {}
        }
        if let Some(p) = &t.pool {
            let _ = writeln!(s, " peer default ip address pool {p}");
        }
        if let Some(a) = t.auth {
            let _ = writeln!(s, " ppp authentication {}", a.name());
        }
        let _ = writeln!(s, "!");
    }
    for x in &r.dialers {
        let _ = writeln!(s, "interface {}", x.name());
        if !x.description.is_empty() {
            let _ = writeln!(s, " description {}", x.description);
        }
        if let Some(m) = x.mtu {
            let _ = writeln!(s, " mtu {m}");
        }
        if x.ppp {
            let _ = writeln!(s, " encapsulation ppp");
        }
        if x.negotiated {
            let _ = writeln!(s, " ip address negotiated");
        }
        if let Some(p) = x.pool {
            let _ = writeln!(s, " dialer pool {p}");
        }
        if let Some(h) = &x.chap_hostname {
            let _ = writeln!(s, " ppp chap hostname {h}");
        }
        if let Some(p) = &x.chap_password {
            let _ = writeln!(s, " ppp chap password {p}");
        }
        if let Some(p) = &x.pap {
            let _ = writeln!(s, " ppp pap sent-username {} password {}", p.name, p.password);
        }
        if x.shutdown {
            let _ = writeln!(s, " shutdown");
        }
        let _ = writeln!(s, "!");
    }
}

/// A physical port's PPPoE lines in the running configuration.
pub(crate) fn port_lines(s: &mut String, d: &Device, port: &str) {
    if let Some(g) = d.pppoe_router.enabled.get(port) {
        if g == GLOBAL_GROUP {
            let _ = writeln!(s, " pppoe enable");
        } else {
            let _ = writeln!(s, " pppoe enable group {g}");
        }
    }
    if let Some(p) = d.pppoe_router.dial_ports.get(port) {
        let _ = writeln!(s, " pppoe-client dial-pool-number {p}");
    }
}

/// The dialers' and templates' rows of `show ip interface brief` (same columns as the others).
pub(crate) fn brief(s: &mut String, d: &Device) {
    for t in &d.pppoe_router.templates {
        let addr = netlabcraft_sim::pppoe::template_address(d, t).map_or("unassigned".to_string(), |a| a.to_string());
        let _ = writeln!(s, "{:<24} {:<18} up      up", t.name(), addr);
    }
    for x in &d.pppoe_router.dialers {
        let addr = x.session.as_ref().map_or("unassigned".to_string(), |s| s.session.address.to_string());
        let state = if !x.shutdown && x.session.is_some() { "up" } else { "down" };
        let _ = writeln!(s, "{:<24} {:<18} {:<7} {}", x.name(), addr, if x.shutdown { "admin down" } else { state }, state);
    }
}
