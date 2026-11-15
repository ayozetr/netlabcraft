//! The VPN Client of a computer's Desktop: `vpnclient.connect`, `vpnclient.disconnect` and
//! `vpnclient.status`.
//!
//! Connecting sends a real request over the network to the VPN concentrator (a router set up as
//! a remote-access server, see `netlabcraft_sim::ravpn`), which checks the group and its key and
//! the user, and answers with an address from the group's pool and the transform set. From then
//! on the computer's traffic beyond its own subnet goes through an ESP tunnel to the router. The
//! connection is dynamic state of the simulator: it is not saved with the lab, the settings of
//! the form are.

use std::net::Ipv4Addr;

use netlabcraft_model::vpnclient::{MAX_VPN_TEXT, VpnClientSettings};
use netlabcraft_sim::ravpn::RaParams;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "vpnclient.connect" => Some(connect(s, p)),
        "vpnclient.disconnect" => Some(disconnect(s, p)),
        "vpnclient.status" => Some(status(s, p)),
        _ => None,
    }
}

fn text_param(p: &Value, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().map(|t| Some(t.chars().take(MAX_VPN_TEXT).collect::<String>())).ok_or_else(|| bad(format!("`{key}` is text"))),
    }
}

fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let saved = s.topology.device(d).map(|x| x.vpn_client.clone()).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let host = text_param(p, "host")?.map(|t| t.trim().to_string()).unwrap_or(saved.host);
    let group = text_param(p, "group")?.map(|t| t.trim().to_string()).unwrap_or(saved.group);
    let key = text_param(p, "key")?.unwrap_or(saved.key);
    let username = text_param(p, "username")?.map(|t| t.trim().to_string()).unwrap_or(saved.username);
    let password = text_param(p, "password")?.unwrap_or(saved.password);
    if host.is_empty() {
        return Err(bad("`host` (the VPN concentrator's address) is required"));
    }
    let server: Ipv4Addr = host.parse().map_err(|_| bad(format!("`host` `{host}` is not an IPv4 address")))?;
    if group.is_empty() {
        return Err(bad("`group` (the group name) is required"));
    }
    if key.is_empty() {
        return Err(bad("`key` (the group key) is required"));
    }
    let params = RaParams { server, group: &group, key: &key, user: &username, password: &password };
    s.sim.ra_connect(&s.topology, d, &params).map_err(|e| EngineError::Failed(e.to_string()))?;
    s.checkpoint();
    if let Some(h) = s.topology.device_mut(d) {
        h.vpn_client = VpnClientSettings { host, group, key, username, password };
    }
    s.changed();
    status(s, p)
}

fn disconnect(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    s.sim.ra_disconnect(&s.topology, d).map_err(|e| EngineError::Failed(e.to_string()))?;
    status(s, p)
}

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let saved = s.topology.device(d).map(|x| x.vpn_client.clone()).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let sa = s.sim.ra_client(d);
    Ok(json!({
        "device": d.0,
        "state": if sa.is_some() { "connected" } else { "disconnected" },
        "connected": sa.is_some(),
        "host": saved.host,
        "group": saved.group,
        "username": saved.username,
        "server": sa.map(|x| x.peer.to_string()),
        "address": sa.map(|x| x.assigned.to_string()),
        "transforms": sa.map(|x| x.transforms.clone()),
        "esp": sa.map(|x| x.suite.map_or_else(|| "stand-in cipher".to_string(), |su| su.describe())),
        "encapsulated": sa.map(|x| x.encaps),
        "decapsulated": sa.map(|x| x.decaps),
    }))
}
