//! Cellular commands: `cell.settings` (a phone's or tablet's Cellular Data and APN), `cell.core`
//! (a central office server's pool, APN and data plan, applied as console lines) and
//! `cell.status` (what a device, tower or central office knows of the cellular network).
//! Placing towers and servers is `device.add` with their models.

use netlabcraft_core::DeviceId;
use netlabcraft_model::cellular::MAX_APN;
use netlabcraft_model::{End, ifname};
use serde_json::{Value, json};

use crate::cmd::device_param;
use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs a cellular command, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "cell.settings" => Some(settings(s, p)),
        "cell.core" => Some(core(s, p)),
        "cell.status" => Some(status(s, p)),
        _ => None,
    }
}

fn settings(s: &mut Session, p: &Value) -> Result<Value> {
    let d = device_param(s, p, "device")?;
    if s.topology.device(d).and_then(|x| x.cell_port()).is_none() {
        return Err(bad("only smartphones and tablets have a cellular radio"));
    }
    let data = p.get("data").and_then(Value::as_bool);
    let apn = match p.get("apn") {
        None | Some(Value::Null) => None,
        Some(Value::String(a)) => {
            let a = a.trim();
            if a.chars().count() > MAX_APN || !a.chars().all(|c| c.is_ascii_graphic()) {
                return Err(bad(format!("an APN is up to {MAX_APN} printable characters, without spaces (empty: none)")));
            }
            Some(a.to_string())
        }
        Some(_) => return Err(bad("`apn` is text")),
    };
    if data.is_none() && apn.is_none() {
        return Err(bad("give `data` (true or false) and/or `apn`"));
    }
    s.checkpoint();
    if let Some(x) = s.topology.device_mut(d) {
        if let Some(on) = data {
            x.cell.data_off = !on;
        }
        if let Some(a) = apn {
            x.cell.apn = a;
        }
    }
    s.changed();
    status(s, &json!({"device": d.0}))
}

fn core(s: &mut Session, p: &Value) -> Result<Value> {
    let d = device_param(s, p, "device")?;
    if !s.topology.device(d).is_some_and(|x| x.is_central_office()) {
        return Err(bad("cellular service is set on a Central Office Server"));
    }
    let mut lines: Vec<String> = Vec::new();
    match p.get("pool") {
        None => {}
        Some(Value::Null) => lines.push("no cellular pool".into()),
        Some(Value::String(n)) => {
            let net: netlabcraft_core::Ipv4Net = n.trim().parse().map_err(|_| bad("`pool` is a network such as 10.64.0.0/24 (null: none)"))?;
            lines.push(format!("cellular pool {} {}", net.addr, net.mask()));
        }
        Some(_) => return Err(bad("`pool` is text such as 10.64.0.0/24, or null")),
    }
    match p.get("apn") {
        None => {}
        Some(Value::Null) => lines.push("no cellular apn".into()),
        Some(Value::String(a)) if a.trim().is_empty() => lines.push("no cellular apn".into()),
        Some(Value::String(a)) => lines.push(format!("cellular apn {}", a.trim())),
        Some(_) => return Err(bad("`apn` is text, or null for any")),
    }
    match p.get("plan") {
        None => {}
        Some(Value::Bool(on)) => lines.push(format!("cellular data-plan {}", if *on { "on" } else { "off" })),
        Some(_) => return Err(bad("`plan` is true or false")),
    }
    if lines.is_empty() {
        return Err(bad("give `pool`, `apn` and/or `plan`"));
    }
    let config = lines.join("\n");
    let mut t = s.topology.clone();
    let failed = netlabcraft_ios::apply_config(&mut t, &mut s.sim, d, &config);
    if let Some(f) = failed.first() {
        return Err(bad(f.clone()));
    }
    s.checkpoint();
    s.topology = t;
    s.changed();
    status(s, &json!({"device": d.0}))
}

fn name_of(s: &Session, id: DeviceId) -> String {
    s.topology.device(id).map(|d| d.name.clone()).unwrap_or_default()
}

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let id = device_param(s, p, "device")?;
    let t = &s.topology;
    let d = t.device(id).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    if d.is_central_office() {
        let clients: Vec<Value> = t
            .core_clients(id)
            .into_iter()
            .map(|(c, l, lease)| json!({"device": name_of(s, c), "address": lease.address.addr.to_string(), "tower": name_of(s, l.tower), "bars": l.bars}))
            .collect();
        let refused: Vec<Value> = t
            .cell_links
            .iter()
            .filter(|(_, l)| l.core.is_some_and(|c| c.device == id))
            .filter_map(|(c, l)| l.outcome.as_ref().err().map(|r| json!({"device": name_of(s, *c), "reason": r.text()})))
            .collect();
        return Ok(json!({
            "role": "central-office",
            "pool": d.cell.pool.map(|n| format!("{}/{}", n.addr, n.prefix)),
            "apn": d.cell.apn,
            "plan": !d.cell.plan_off,
            "admitted": clients,
            "refused": refused,
        }));
    }
    if d.is_tower() {
        let clients: Vec<Value> = t
            .tower_clients(id)
            .into_iter()
            .map(|(c, l)| json!({"device": name_of(s, c), "sector": d.ports.get(l.sector).map(|x| x.name.clone()), "bars": l.bars, "admitted": l.outcome.is_ok()}))
            .collect();
        return Ok(json!({"role": "tower", "range": netlabcraft_model::cellular::CELL_RANGE, "clients": clients}));
    }
    let cp = d.cell_port().ok_or_else(|| bad(format!("{} has no cellular radio and is not part of the cellular network", d.name)))?;
    let session = s.sim.cell_session(End { device: id, port: cp }).map(|x| match x {
        netlabcraft_sim::cell::CellSession::Attached { .. } => "accepted".to_string(),
        netlabcraft_sim::cell::CellSession::Rejected(why) => format!("rejected: {why}"),
    });
    let wifi = t.wifi_ap_of(End { device: id, port: d.ports.iter().position(|x| ifname::is_wireless(&x.name)).unwrap_or(0) });
    let active = if wifi.is_some() {
        "wifi"
    } else if t.cell_link(id).is_some_and(|l| l.outcome.is_ok()) && !d.cell.data_off {
        "cellular"
    } else {
        "none"
    };
    Ok(match t.cell_link(id) {
        Some(l) => json!({
            "role": "phone",
            "data": !d.cell.data_off,
            "apn": d.cell.apn,
            "active": active,
            "tower": name_of(s, l.tower),
            "bars": l.bars,
            "attached": l.outcome.is_ok(),
            "address": l.outcome.as_ref().ok().map(|x| x.address.addr.to_string()),
            "gateway": l.outcome.as_ref().ok().map(|x| x.gateway.to_string()),
            "refused": l.outcome.as_ref().err().map(|r| r.text()),
            "attach_answer": session,
        }),
        None => json!({
            "role": "phone",
            "data": !d.cell.data_off,
            "apn": d.cell.apn,
            "active": active,
            "bars": 0,
            "attached": false,
            "refused": t.cell_no_link(id).map(|n| n.text()),
        }),
    })
}
