//! Dial-up: a Dial-up PC's modem calls a telephone number through the telephone cloud, and an
//! access server's modem line answers with PPP (`dialup.connect`, `dialup.disconnect`,
//! `dialup.status`; the numbers are `cloud.number`).
//!
//! A call is a pair of ports in the telephone cloud (`bridges`), made when the call is placed and
//! parted when it ends, so the circuit shows in the cloud's Physical tab and a line in use is
//! busy to the next caller. Over that circuit the modem and the line speak the same PPP as a
//! PPPoE session (discovery, LCP, PAP or CHAP, IPCP in session frames, `pppoe.connect`): the
//! simulator does not model asynchronous framing or modem tones ([decisions.md](../../../docs/decisions.md)).

use netlabcraft_model::End;
use netlabcraft_model::cloud::normalize_number;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

fn failed(msg: impl Into<String>) -> EngineError {
    EngineError::Failed(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "dialup.connect" => Some(connect(s, p)),
        "dialup.disconnect" => Some(disconnect(s, p)),
        "dialup.status" => Some(status(s, p)),
        "cloud.number" => Some(number(s, p)),
        _ => None,
    }
}

/// The telephone cloud a modem is cabled to, and the cloud's port the cable ends at.
struct Line {
    cloud: netlabcraft_core::DeviceId,
    port: usize,
}

fn line_of(s: &Session, d: netlabcraft_core::DeviceId) -> Result<Line> {
    let dev = s.topology.device(d).ok_or_else(|| failed("no such device"))?;
    if dev.ports.first().is_none_or(|p| !p.name.starts_with("Phone")) {
        return Err(failed(format!("{} has no modem: use a Dial-up PC", dev.name)));
    }
    let far = s.topology.peer(End { device: d, port: 0 }).ok_or_else(|| failed("the modem has no phone cable: plug it into a telephone cloud"))?;
    if !s.topology.device(far.device).is_some_and(|c| c.catalogue_model().pstn()) {
        return Err(failed("the modem's phone cable must go to a telephone cloud"));
    }
    Ok(Line { cloud: far.device, port: far.port })
}

fn port_name(s: &Session, id: netlabcraft_core::DeviceId, port: usize) -> String {
    s.topology.device(id).and_then(|d| d.ports.get(port)).map(|p| p.name.clone()).unwrap_or_default()
}

/// Parts the pair the caller's line is in, if it is in one.
fn release(s: &mut Session, line: &Line) {
    let partner = s.topology.device(line.cloud).and_then(|c| c.cloud_partner(line.port));
    if let Some(other) = partner {
        let (a, b) = (port_name(s, line.cloud, line.port), port_name(s, line.cloud, other));
        let _ = s.topology.cloud_map(line.cloud, &a, &b, false);
        s.changed();
    }
}

fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let line = line_of(s, d)?;
    let (saved, connected) = s.topology.device(d).map(|x| (x.pppoe.number.clone(), x.pppoe.session.is_some())).unwrap_or_default();
    let typed = match p.get("number") {
        None | Some(Value::Null) => saved,
        Some(v) => v.as_str().ok_or_else(|| bad("`number` is text"))?.to_string(),
    };
    let digits = normalize_number(&typed).ok_or_else(|| bad("`number` (the telephone number to dial) is required: 1 to 15 digits"))?;
    if connected {
        return Err(failed("this computer is already connected: hang up first"));
    }
    // A call the far end hung up leaves the circuit in place: the next call starts from a free line.
    release(s, &line);
    let cloud = s.topology.device(line.cloud).ok_or_else(|| failed("no such device"))?;
    let target = cloud.port_of_number(&digits).ok_or_else(|| failed(format!("{digits} is not a number on the telephone network")))?;
    if target == line.port {
        return Err(failed("busy: that is this modem's own number"));
    }
    if cloud.cloud_partner(target).is_some() {
        return Err(failed(format!("{digits} is busy")));
    }
    let far = s
        .topology
        .peer(End { device: line.cloud, port: target })
        .ok_or_else(|| failed(format!("no answer from {digits}: nothing is plugged into that line")))?;
    if !s.topology.device(far.device).is_some_and(|x| netlabcraft_sim::pppoe::answers(x, far.port)) {
        return Err(failed(format!("no answer from {digits}: no modem line picked up")));
    }
    let (own_name, target_name) = (port_name(s, line.cloud, line.port), port_name(s, line.cloud, target));
    s.checkpoint();
    s.topology.cloud_map(line.cloud, &own_name, &target_name, true)?;
    s.changed();
    if let Err(e) = crate::pppoe::connect(s, p) {
        release(s, &line);
        return Err(failed(format!("the call connected but PPP failed: {e}")));
    }
    if let Some(h) = s.topology.device_mut(d) {
        h.pppoe.number = digits;
    }
    s.changed();
    status(s, p)
}

fn disconnect(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let line = line_of(s, d)?;
    let has_session = s.topology.device(d).is_some_and(|x| x.pppoe.session.is_some());
    let has_circuit = s.topology.device(line.cloud).is_some_and(|c| c.cloud_partner(line.port).is_some());
    if !has_session && !has_circuit {
        return Err(failed("this computer is not connected"));
    }
    if has_session {
        crate::pppoe::disconnect(s, p)?;
    }
    release(s, &line);
    status(s, p)
}

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let line = line_of(s, d)?;
    let mut v = crate::pppoe::status(s, p)?;
    let dev = s.topology.device(d).ok_or_else(|| failed("no such device"))?;
    let cloud = s.topology.device(line.cloud).ok_or_else(|| failed("no such device"))?;
    let called = cloud.cloud_partner(line.port).and_then(|o| cloud.ports.get(o)).and_then(|port| cloud.phone_numbers.get(&port.name).cloned());
    v["number"] = json!(dev.pppoe.number);
    v["carrier"] = json!(cloud.cloud_partner(line.port).is_some());
    v["called"] = json!(called);
    v["cloud"] = json!(cloud.name);
    Ok(v)
}

/// `cloud.number`: gives a phone port of a telephone cloud its number, or lists them.
fn number(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "device")?;
    if s.topology.device(d).is_none_or(|x| !x.catalogue_model().pstn()) {
        return Err(bad("cloud.number is for telephone clouds"));
    }
    if let Some(port) = p.get("port") {
        let port = port.as_str().ok_or_else(|| bad("`port` is text"))?;
        let number = match p.get("number") {
            Some(Value::Null) => None,
            Some(v) => Some(v.as_str().ok_or_else(|| bad("`number` is text, or null to remove the number"))?),
            None => return Err(bad("give `number` (digits, or null to remove it) with `port`")),
        };
        let mut t = s.topology.clone();
        t.cloud_number(d, port, number)?;
        s.checkpoint();
        s.topology = t;
        s.changed();
    }
    let dev = s.topology.device(d).ok_or_else(|| failed("no such device"))?;
    let numbers: Vec<Value> = dev.phone_numbers.iter().map(|(port, n)| json!({"port": port, "number": n})).collect();
    let calls: Vec<Value> = dev.bridges.iter().map(|(a, b)| json!([a, b])).collect();
    let ports: Vec<&str> = dev.ports.iter().map(|x| x.name.as_str()).collect();
    Ok(json!({"device": dev.name, "numbers": numbers, "calls": calls, "ports": ports}))
}
