//! USB commands: what is plugged into a host, mounting and ejecting a stick, putting files on it
//! and taking them off through a host, and printing over USB or the network. The cables are
//! `link.add` with the `usb` cable (see `cmd.rs`); the model is `netlabcraft_model::usb`.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_model::usb::{STICK_CAPACITY, UsbCable};
use netlabcraft_sim::{PingOptions, Probe};
use serde_json::{Value, json};

use crate::cmd::device_param;
use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

fn failed(msg: impl Into<String>) -> EngineError {
    EngineError::Failed(msg.into())
}

fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| bad(format!("`{key}` (text) is required")))
}

pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    Some(match id {
        "usb.list" => list(s, p),
        "usb.eject" | "usb.mount" => eject(s, id == "usb.eject", p),
        "usb.put" => put(s, p),
        "usb.get" => get(s, p),
        "usb.delete" => delete(s, p),
        "printer.print" => print(s, p),
        "printer.jobs" => jobs(s, p),
        _ => return None,
    })
}

fn name(s: &Session, d: DeviceId) -> String {
    s.topology.device(d).map(|x| x.name.clone()).unwrap_or_default()
}

/// One cable as the commands tell it.
fn cable_json(s: &Session, c: &UsbCable) -> Value {
    let t = &s.topology;
    let Some(dev) = t.device(c.device) else { return Value::Null };
    let problem = t.usb_problem(c);
    let mut v = json!({
        "host": name(s, c.host),
        "port": u16::from(c.host_port) + 1,
        "device": dev.name,
        "model": dev.catalogue_model().label,
        "kind": if dev.is_usb_stick() { "storage" } else if dev.kind == DeviceKind::Printer { "printer" } else if netlabcraft_model::board::is_board(dev) { "board" } else { "phone" },
        "up": problem.is_none(),
        "problem": problem,
    });
    if dev.is_usb_stick() {
        v["mounted"] = json!(!c.ejected);
        v["used"] = json!(dev.usb.used());
        v["capacity"] = json!(STICK_CAPACITY);
        v["files"] = json!(dev.usb.files.iter().map(|f| json!({"name": f.name, "size": f.size()})).collect::<Vec<_>>());
    }
    v
}

/// `{"device"?: a host or a USB device}` → what is plugged in: every cable of the lab, those of a
/// host, or the one a device is plugged into.
fn list(s: &Session, p: &Value) -> Result<Value> {
    let t = &s.topology;
    let wanted = match p.get("device") {
        None | Some(Value::Null) => None,
        Some(_) => Some(device_param(s, p, "device")?),
    };
    let out: Vec<Value> = t.usbs.iter().filter(|c| wanted.is_none_or(|d| c.host == d || c.device == d)).map(|c| cable_json(s, c)).collect();
    Ok(Value::Array(out))
}

/// The cable of a USB device, and the host asked for (when named) must be the one it is plugged into.
fn plugged(s: &Session, p: &Value) -> Result<UsbCable> {
    let device = device_param(s, p, "device")?;
    let c = *s.topology.usb_of_device(device).ok_or_else(|| failed(format!("{} is not plugged into a USB port", name(s, device))))?;
    if p.get("host").is_some_and(|h| !h.is_null()) {
        let host = device_param(s, p, "host")?;
        if host != c.host {
            return Err(failed(format!("{} is plugged into {}, not {}", name(s, device), name(s, c.host), name(s, host))));
        }
    }
    Ok(c)
}

fn eject(s: &mut Session, eject: bool, p: &Value) -> Result<Value> {
    let c = plugged(s, p)?;
    if !s.topology.device(c.device).is_some_and(|d| d.is_usb_stick()) {
        return Err(failed(format!("{} is not a USB stick: only storage is mounted", name(s, c.device))));
    }
    if c.ejected == eject {
        return Err(failed(format!("{} is already {}", name(s, c.device), if eject { "ejected" } else { "mounted" })));
    }
    s.checkpoint();
    if let Some(x) = s.topology.usbs.iter_mut().find(|x| x.device == c.device) {
        x.ejected = eject;
    }
    s.changed();
    Ok(json!({"device": name(s, c.device), "host": name(s, c.host), "mounted": !eject}))
}

/// The stick a command works on, mounted and reachable through its host.
fn mounted_stick(s: &Session, p: &Value) -> Result<(UsbCable, DeviceId)> {
    let c = plugged(s, p)?;
    let t = &s.topology;
    if !t.device(c.device).is_some_and(|d| d.is_usb_stick()) {
        return Err(failed(format!("{} is not a USB stick", name(s, c.device))));
    }
    if c.ejected {
        return Err(failed(format!("{} is ejected: mount it first (usb.mount)", name(s, c.device))));
    }
    if let Some(why) = t.usb_problem(&c) {
        return Err(failed(why));
    }
    Ok((c, c.device))
}

fn put(s: &mut Session, p: &Value) -> Result<Value> {
    let (_, stick) = mounted_stick(s, p)?;
    let (file, body) = (text(p, "name")?, text(p, "text")?);
    let mut t = s.topology.clone();
    let d = t.device_mut(stick).ok_or_else(|| failed("no such device"))?;
    d.usb.put(file, body)?;
    let (used, count) = (d.usb.used(), d.usb.files.len());
    s.checkpoint();
    s.topology = t;
    s.changed();
    Ok(json!({"name": file, "size": body.len(), "used": used, "files": count}))
}

fn get(s: &Session, p: &Value) -> Result<Value> {
    let (_, stick) = mounted_stick(s, p)?;
    let file = text(p, "name")?;
    let f = s.topology.device(stick).and_then(|d| d.usb.find(file)).ok_or_else(|| failed(format!("no file `{file}` on the stick")))?;
    Ok(json!({"name": f.name, "size": f.size(), "text": f.text}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let (_, stick) = mounted_stick(s, p)?;
    let file = text(p, "name")?;
    let mut t = s.topology.clone();
    if !t.device_mut(stick).is_some_and(|d| d.usb.delete(file)) {
        return Err(failed(format!("no file `{file}` on the stick")));
    }
    s.checkpoint();
    s.topology = t;
    s.changed();
    Ok(Value::Null)
}

/// `{"from": host, "printer", "text", "via"?: "usb" | "network"}`: sends a page. By default the
/// USB cable between the two is used when there is one, else the network (the printer needs an
/// address the host can reach).
fn print(s: &mut Session, p: &Value) -> Result<Value> {
    let host = device_param(s, p, "from")?;
    let printer = device_param(s, p, "printer")?;
    let body = text(p, "text")?;
    if s.topology.device(printer).is_none_or(|d| d.kind != DeviceKind::Printer) {
        return Err(failed(format!("{} is not a printer", name(s, printer))));
    }
    if s.topology.device(host).is_none_or(|d| d.kind == DeviceKind::Printer || d.kind == DeviceKind::Iot) {
        return Err(failed(format!("{} cannot send a page", name(s, host))));
    }
    let cable = s.topology.usb_of_device(printer).copied().filter(|c| c.host == host);
    let via = match p.get("via") {
        None | Some(Value::Null) => {
            if cable.is_some() {
                "usb"
            } else {
                "network"
            }
        }
        Some(Value::String(v)) if v.eq_ignore_ascii_case("usb") => "usb",
        Some(Value::String(v)) if v.eq_ignore_ascii_case("network") => "network",
        _ => return Err(bad("`via` is \"usb\" or \"network\"")),
    };
    if s.topology.device(printer).is_some_and(|d| d.powered_off) {
        return Err(failed(format!("{} is switched off", name(s, printer))));
    }
    if via == "usb" {
        let c = cable.ok_or_else(|| failed(format!("{} has no USB cable to {}", name(s, host), name(s, printer))))?;
        if let Some(why) = s.topology.usb_problem(&c) {
            return Err(failed(why));
        }
    } else {
        let ip = s
            .topology
            .device(printer)
            .and_then(|d| d.ports.first())
            .and_then(|port| port.ip)
            .ok_or_else(|| failed(format!("{} has no address: give it one, or use its USB cable", name(s, printer))))?;
        let probes = s.sim.ping(&s.topology, host, ip.addr, PingOptions { count: 2, ..PingOptions::default() }).map_err(|e| failed(e.to_string()))?;
        s.revision += 1;
        if !probes.iter().any(|x| matches!(x, Probe::Reply { .. })) {
            return Err(failed(format!("{} cannot reach {} ({})", name(s, host), name(s, printer), ip.addr)));
        }
    }
    let from = name(s, host);
    s.checkpoint();
    if let Some(d) = s.topology.device_mut(printer) {
        d.printer.print(&from, via, body);
    }
    s.changed();
    let total = s.topology.device(printer).map_or(0, |d| d.printer.total);
    Ok(json!({"printer": name(s, printer), "via": via, "job": total}))
}

fn jobs(s: &Session, p: &Value) -> Result<Value> {
    let printer = device_param(s, p, "printer")?;
    let d = s
        .topology
        .device(printer)
        .filter(|d| d.kind == DeviceKind::Printer)
        .ok_or_else(|| failed(format!("{} is not a printer", name(s, printer))))?;
    let list: Vec<Value> = d.printer.jobs.iter().map(|j| json!({"from": j.from, "via": j.via, "text": j.text})).collect();
    Ok(json!({"printer": d.name, "total": d.printer.total, "jobs": list}))
}
