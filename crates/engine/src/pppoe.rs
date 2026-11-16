//! The PPPoE Dialer of an end device's Desktop and the PPPoE service of a server: `pppoe.connect`,
//! `pppoe.disconnect` and `pppoe.status` (the service is set with `server.services`).
//!
//! The exchange is carried out by the simulator as frames (`netlabcraft_sim::pppoe`): the dialer
//! broadcasts a PADI, a server with the service on answers, and PPP (LCP, PAP or CHAP, IPCP) runs
//! in session frames, across any switches in between. On success the computer gets an address
//! from the server's pool, the mask of the server's port, and the server's own address as
//! default gateway. The lab keeps the session (`Device::pppoe`); a PADT ends it.

use netlabcraft_core::DeviceId;
use netlabcraft_model::pppoe::PppoeSession;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// Simulated time the clock runs after a session starts or ends, so the last frames arrive.
const SETTLE_US: u64 = 50_000;

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "pppoe.connect" => Some(connect(s, p)),
        "pppoe.disconnect" => Some(disconnect(s, p)),
        "pppoe.status" => Some(status(s, p)),
        _ => None,
    }
}

/// A server's service was switched off: it ends every session it holds with a PADT, which the
/// callers hear before the command returns (the lab drops them when the command ends).
pub(crate) fn service_off(s: &mut Session, server: DeviceId) {
    s.sim.pppoe_server_down(&s.topology, server);
    s.sim.run_for(&s.topology, SETTLE_US);
}

fn text_param(p: &Value, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_str()
            .map(|t| Some(t.chars().take(netlabcraft_model::pppoe::MAX_CREDENTIAL).collect::<String>()))
            .ok_or_else(|| bad(format!("`{key}` is text"))),
    }
}

fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let (saved_user, saved_password, connected) = s
        .topology
        .device(d)
        .map(|x| (x.pppoe.username.clone(), x.pppoe.password.clone(), x.pppoe.session.is_some()))
        .ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let username = text_param(p, "username")?.map(|t| t.trim().to_string()).unwrap_or(saved_user);
    let password = text_param(p, "password")?.unwrap_or(saved_password);
    if username.is_empty() {
        return Err(bad("`username` (the account name) is required"));
    }
    if connected {
        return Err(EngineError::Failed("this computer is already connected: disconnect first".into()));
    }
    let r = s.sim.pppoe_dial(&s.topology, d, &username, &password).map_err(|e| EngineError::Failed(e.to_string()))?;
    // Let the last frames of the exchange arrive before the lab says the session is up.
    s.sim.run_for(&s.topology, SETTLE_US);
    let (server, prefix) = s
        .topology
        .devices
        .iter()
        .find_map(|x| x.ports.iter().find(|q| q.mac == r.server_mac).map(|q| (x.id, q.ip.map_or(24, |n| n.prefix))))
        .ok_or_else(|| EngineError::Failed("the access concentrator is no longer in the lab".into()))?;
    s.checkpoint();
    if let Some(h) = s.topology.device_mut(d) {
        if let Some(port) = h.ports.first_mut() {
            port.ip = Some(netlabcraft_core::Ipv4Net::new(r.address, prefix));
        }
        h.gateway = Some(r.gateway);
        h.dhcp_client = false;
        h.dhcp_lease = None;
        h.pppoe.username = username;
        h.pppoe.password = password;
        h.pppoe.session =
            Some(PppoeSession { server, address: r.address, gateway: r.gateway, auth: r.auth, id: r.session, server_mac: r.server_mac });
    }
    s.changed();
    status(s, p)
}

fn disconnect(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    if s.topology.device(d).is_none_or(|x| x.pppoe.session.is_none()) {
        return Err(EngineError::Failed("this computer is not connected".into()));
    }
    // Terminate-Request and PADT go to the server first, then the lab forgets the session.
    s.sim.pppoe_hangup(&s.topology, d);
    s.sim.run_for(&s.topology, SETTLE_US);
    s.checkpoint();
    if let Some(h) = s.topology.device_mut(d) {
        if let Some(port) = h.ports.first_mut() {
            port.ip = None;
        }
        h.gateway = None;
        h.pppoe.session = None;
    }
    s.changed();
    status(s, p)
}

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let has_session = s.topology.device(d).is_some_and(|x| x.pppoe.session.is_some());
    // A session holds while its server still answers a keep-alive over it.
    let live = has_session && s.sim.pppoe_probe(&s.topology, d);
    let found = if live { None } else { s.sim.pppoe_discover(&s.topology, d) };
    let host = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let session = host.pppoe.session.as_ref();
    let name = |id: DeviceId| s.topology.device(id).map(|x| x.name.clone());
    let mask = host.ports.first().and_then(|x| x.ip).map(|n| n.mask().to_string());
    Ok(json!({
        "device": d.0,
        "username": host.pppoe.username,
        "state": match (session, live) {
            (None, _) => "disconnected",
            (Some(_), true) => "connected",
            (Some(_), false) => "lost",
        },
        "connected": live,
        "server_found": live || found.is_some(),
        "server": session.and_then(|x| name(x.server)).or_else(|| found.as_ref().map(|(_, n)| n.clone())),
        "address": session.map(|x| x.address.to_string()),
        "mask": session.and(mask),
        "gateway": session.map(|x| x.gateway.to_string()),
        "auth": session.map(|x| x.auth.name()),
    }))
}
