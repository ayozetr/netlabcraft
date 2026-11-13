//! The central office server's commands (`cellular pool`, `cellular apn`, `cellular data-plan`,
//! `show cellular`), its running-config lines, and the status a phone or tablet shows with the
//! `cellular` command of its shell.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net, parse_ip};
use netlabcraft_model::cellular::{MAX_APN, NoLink};
use netlabcraft_model::{Device, Topology};
use netlabcraft_sim::Sim;
use netlabcraft_sim::cell::CellSession;

use super::{Ctx, R};
use crate::grammar::Id;

/// The smallest and largest pool (prefix length) a central office takes.
const MIN_PREFIX: u8 = 8;
const MAX_PREFIX: u8 = 30;

fn apn_text(s: &str) -> Result<String, String> {
    if s.is_empty() || s.chars().count() > MAX_APN || !s.chars().all(|c| c.is_ascii_graphic()) {
        return Err(format!("% An APN is 1-{MAX_APN} printable characters, without spaces"));
    }
    Ok(s.to_string())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim();
    if id == Id::ShowCellular {
        return show_cellular(c.t, c.sim, c.dev);
    }
    if !c.d()?.is_central_office() {
        return Err("% Cellular settings are set on a central office server (model Central Office Server)".into());
    }
    match id {
        Id::CellularPool => {
            let addr = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let mask = parse_ip(arg(1)).map_err(|e| format!("% {e}"))?;
            let net = Ipv4Net::with_mask(addr, mask).map_err(|e| format!("% {e}"))?;
            if !(MIN_PREFIX..=MAX_PREFIX).contains(&net.prefix) {
                return Err(format!("% A pool is a /{MIN_PREFIX} to /{MAX_PREFIX} network"));
            }
            if net.network().addr != addr {
                return Err("% Give the network address of the pool (the host bits must be zero)".into());
            }
            c.dm()?.cell.pool = Some(net.network());
        }
        Id::NoCellularPool => c.dm()?.cell.pool = None,
        Id::CellularApn => c.dm()?.cell.apn = apn_text(arg(0))?,
        Id::NoCellularApn => c.dm()?.cell.apn.clear(),
        Id::CellularPlan => {
            let on = match arg(0).to_ascii_lowercase().as_str() {
                "on" => true,
                "off" => false,
                _ => return Err("% The data plan is on or off".into()),
            };
            c.dm()?.cell.plan_off = !on;
        }
        _ => return Err("% Not a cellular command".into()),
    }
    c.t.cell_refresh();
    Ok(String::new())
}

fn show_cellular(t: &Topology, sim: &Sim, dev: DeviceId) -> R {
    let d = t.device(dev).ok_or("% That device no longer exists")?;
    if d.is_central_office() {
        return Ok(core_status(t, d));
    }
    if d.is_tower() {
        return Ok(tower_status(t, d));
    }
    if d.cell_port().is_some() {
        return Ok(phone_status(t, sim, d));
    }
    Err("% This device has no cellular settings".into())
}

fn core_status(t: &Topology, d: &Device) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Cellular data service on {}", d.name);
    let _ = writeln!(s, "  Address pool : {}", d.cell.pool.map_or("not set".to_string(), |p| format!("{}/{}", p.addr, p.prefix)));
    let gw: Vec<String> = d
        .ports
        .iter()
        .filter_map(|p| {
            p.ip.filter(|ip| d.cell.pool.is_some_and(|pool| pool.contains(ip.addr) && ip.prefix == pool.prefix))
                .map(|ip| format!("{} on {}", ip.addr, p.name))
        })
        .collect();
    let _ = writeln!(s, "  Gateway      : {}", if gw.is_empty() { "none (no interface is in the pool)".to_string() } else { gw.join(", ") });
    let _ = writeln!(s, "  APN accepted : {}", if d.cell.apn.is_empty() { "any".to_string() } else { d.cell.apn.clone() });
    let _ = writeln!(s, "  Data plan    : {}", if d.cell.plan_off { "off (every attach is refused)" } else { "on" });
    let clients = t.core_clients(d.id);
    let _ = writeln!(s, "  Devices admitted: {}", clients.len());
    for (id, link, lease) in clients {
        let name = t.device(id).map_or("?", |x| x.name.as_str());
        let tower = t.device(link.tower).map_or("?", |x| x.name.as_str());
        let _ = writeln!(s, "    {name:<16} {:<16} via {tower}", lease.address.addr);
    }
    // Devices whose attach reached this server and was refused.
    for (id, link) in &t.cell_links {
        if let (Some(core), Err(r)) = (link.core, &link.outcome)
            && core.device == d.id
        {
            let name = t.device(*id).map_or("?", |x| x.name.as_str());
            let _ = writeln!(s, "    {name:<16} refused: {}", r.text());
        }
    }
    s.trim_end().to_string()
}

fn tower_status(t: &Topology, d: &Device) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Cell tower {}", d.name);
    let clients = t.tower_clients(d.id);
    let _ =
        writeln!(s, "  Sectors in use: {} of {}", clients.len(), d.ports.iter().filter(|p| netlabcraft_model::ifname::is_cellular(&p.name)).count());
    for (id, link) in clients {
        let name = t.device(id).map_or("?", |x| x.name.as_str());
        let port = d.ports.get(link.sector).map_or("?", |p| p.name.as_str());
        let state = match &link.outcome {
            Ok(l) => format!("admitted, {}", l.address.addr),
            Err(r) => format!("refused: {}", r.text()),
        };
        let _ = writeln!(s, "    {port:<10} {name:<16} {} bars, {state}", link.bars);
    }
    s.trim_end().to_string()
}

fn bars_text(bars: u8) -> String {
    format!("{}{} ({bars}/4)", "#".repeat(usize::from(bars.min(4))), ".".repeat(usize::from(4u8.saturating_sub(bars))))
}

fn phone_status(t: &Topology, sim: &Sim, d: &Device) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Cellular data: {}", if d.cell.data_off { "off" } else { "on" });
    let _ = writeln!(s, "APN          : {}", if d.cell.apn.is_empty() { "not set".to_string() } else { d.cell.apn.clone() });
    match t.cell_link(d.id) {
        Some(l) => {
            let tower = t.device(l.tower).map_or("?", |x| x.name.as_str());
            let _ = writeln!(s, "Signal       : {} from {tower}", bars_text(l.bars));
            match &l.outcome {
                Ok(lease) => {
                    let _ = writeln!(s, "Address      : {}/{}", lease.address.addr, lease.address.prefix);
                    let _ = writeln!(s, "Gateway      : {}", lease.gateway);
                    let _ = writeln!(s, "State        : attached");
                }
                Err(r) => {
                    let _ = writeln!(s, "State        : not attached ({})", r.text());
                }
            }
        }
        None => {
            let why = t.cell_no_link(d.id).unwrap_or(NoLink::Unavailable);
            let _ = writeln!(s, "Signal       : none");
            let _ = writeln!(s, "State        : {}", why.text());
        }
    }
    // What the attach exchange has shown so far (nothing until its answer crossed the network).
    let answer = d.cell_port().and_then(|cp| sim.cell_session(netlabcraft_model::End { device: d.id, port: cp }));
    match answer {
        Some(CellSession::Attached { .. }) => s.push_str("Attach answer: accepted\n"),
        Some(CellSession::Rejected(why)) => {
            let _ = writeln!(s, "Attach answer: rejected ({why})");
        }
        None => {}
    }
    s.trim_end().to_string()
}

/// A phone's or tablet's `ipconfig` lines for its cellular port.
pub(crate) fn ipconfig_lines(d: &Device) -> String {
    let Some(p) = d.cell_port().and_then(|cp| d.ports.get(cp)) else { return String::new() };
    format!(
        "\nInterface   {}\nMAC address {}\nIP address  {}\nSubnet mask {}\nGateway     {}\nAssigned    by the central office",
        p.name,
        p.mac,
        p.ip.map_or("not set".to_string(), |ip| ip.addr.to_string()),
        p.ip.map_or("not set".to_string(), |ip| ip.mask().to_string()),
        p.dhcp_router.map_or("not set".to_string(), |g: Ipv4Addr| g.to_string()),
    )
}

/// The shell of a phone or tablet: `cellular`, `cellular on|off`, `cellular apn <name>|none`.
pub(crate) fn host_command(t: &mut Topology, sim: &Sim, dev: DeviceId, args: &[&str]) -> String {
    let Some(d) = t.device(dev).filter(|d| d.cell_port().is_some()) else {
        return "This device has no cellular radio.".to_string();
    };
    let result: Result<(), String> = match args {
        [] => return phone_status(t, sim, d),
        [w] if w.eq_ignore_ascii_case("on") || w.eq_ignore_ascii_case("off") => {
            let off = w.eq_ignore_ascii_case("off");
            if let Some(d) = t.device_mut(dev) {
                d.cell.data_off = off;
            }
            Ok(())
        }
        [w, name] if w.eq_ignore_ascii_case("apn") => {
            let apn = if name.eq_ignore_ascii_case("none") { Ok(String::new()) } else { apn_text(name) };
            apn.map(|a| {
                if let Some(d) = t.device_mut(dev) {
                    d.cell.apn = a;
                }
            })
        }
        _ => Err("Usage: cellular [on | off | apn <name|none>]".to_string()),
    };
    match result {
        Ok(()) => {
            t.cell_refresh();
            match t.device(dev) {
                Some(d) => phone_status(t, sim, d),
                None => String::new(),
            }
        }
        Err(e) => e.trim_start_matches("% ").to_string(),
    }
}

/// The lines of a central office server's running-config.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    if !d.is_central_office() || d.cell.is_default() {
        return;
    }
    if let Some(p) = d.cell.pool {
        let _ = writeln!(s, "cellular pool {} {}", p.addr, p.mask());
    }
    if !d.cell.apn.is_empty() {
        let _ = writeln!(s, "cellular apn {}", d.cell.apn);
    }
    if d.cell.plan_off {
        let _ = writeln!(s, "cellular data-plan off");
    }
    let _ = writeln!(s, "!");
}
