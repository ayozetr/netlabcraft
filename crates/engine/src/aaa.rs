//! A Server's AAA service beyond logins: what it authorizes (`aaa.profile`: the privilege level
//! a user gets and the commands refused) and the accounting records it received (`aaa.records`).

use netlabcraft_model::DeviceKind;
use netlabcraft_model::services::{AaaProfile, MAX_PROFILES};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "aaa.profile" => Some(profile(s, p)),
        "aaa.records" => Some(records(s, p)),
        _ => None,
    }
}

fn server(s: &Session, p: &Value) -> Result<netlabcraft_core::DeviceId> {
    let d = crate::cmd::device_param(s, p, "device")?;
    let dev = s.topology.device(d).ok_or_else(|| bad("no such device"))?;
    if dev.kind != DeviceKind::Server {
        return Err(bad("only a Server runs the AAA service"));
    }
    Ok(d)
}

fn describe(p: &AaaProfile) -> Value {
    json!({"user": p.user, "privilege": p.privilege, "deny": p.deny})
}

fn profile(s: &mut Session, p: &Value) -> Result<Value> {
    let d = server(s, p)?;
    let user = p.get("user").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty());
    let Some(user) = user else {
        let all: Vec<Value> = s.topology.device(d).map(|x| x.services.aaa.profiles.iter().map(describe).collect()).unwrap_or_default();
        return Ok(json!({"profiles": all}));
    };
    let known = s.topology.device(d).is_some_and(|x| x.services.aaa.users.iter().any(|u| u.name == user));
    if !known {
        return Err(bad(format!("`{user}` is not a user of this AAA service")));
    }
    let clear = p.get("clear").and_then(Value::as_bool) == Some(true);
    let old = s.topology.device(d).and_then(|x| x.services.aaa.profiles.iter().find(|q| q.user == user).cloned());
    let mut next = old.unwrap_or_else(|| AaaProfile { user: user.to_string(), privilege: 1, deny: Vec::new() });
    if let Some(n) = p.get("privilege").and_then(Value::as_u64) {
        if n > 15 {
            return Err(bad("`privilege` is a level from 0 to 15"));
        }
        next.privilege = n as u8;
    }
    if let Some(list) = p.get("deny").and_then(Value::as_array) {
        next.deny = list
            .iter()
            .filter_map(Value::as_str)
            .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|c| !c.is_empty())
            .take(32)
            .collect();
    }
    s.checkpoint();
    let x = s.topology.device_mut(d).ok_or_else(|| bad("no such device"))?;
    x.services.aaa.profiles.retain(|q| q.user != user);
    if !clear {
        if x.services.aaa.profiles.len() >= MAX_PROFILES {
            return Err(bad("too many profiles"));
        }
        x.services.aaa.profiles.push(next.clone());
    }
    s.changed();
    Ok(if clear { json!({"user": user, "cleared": true}) } else { describe(&next) })
}

fn records(s: &mut Session, p: &Value) -> Result<Value> {
    let d = server(s, p)?;
    if p.get("clear").and_then(Value::as_bool) == Some(true) {
        s.sim.clear_aaa_records(d);
    }
    let rows: Vec<Value> = s
        .sim
        .aaa_records(d)
        .iter()
        .map(|r| json!({"at_us": r.at_us, "from": r.from.to_string(), "protocol": r.protocol, "user": r.user, "kind": r.kind, "detail": r.detail}))
        .collect();
    Ok(json!({"count": rows.len(), "records": rows}))
}
