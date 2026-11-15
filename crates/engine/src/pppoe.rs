//! The PPPoE Dialer of an end device's Desktop and the PPPoE service of a server: `pppoe.connect`,
//! `pppoe.disconnect` and `pppoe.status` (the service is set with `server.services`).
//!
//! The exchange is carried out as one step over the cable: the dialer's port must be cabled
//! straight to a server with the service on (discovery), the server asks for the user name and
//! password with PAP (RFC 1334) or CHAP (RFC 1994, a real MD5 challenge and response), and on
//! success gives the dialer the first free address of its pool and its own address as the default
//! gateway. No PPPoE or PPP frames are put on the wire (see `docs/gaps.md`).

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::pppoe::{PppoeServer, PppoeSession};
use netlabcraft_model::{DeviceKind, End, PppAuth};
use netlabcraft_wire::md5::md5;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

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

/// The CHAP response (RFC 1994 section 4.1): MD5 of the identifier, the secret and the challenge.
fn chap_response(id: u8, secret: &str, challenge: &[u8]) -> [u8; 16] {
    let mut m = vec![id];
    m.extend_from_slice(secret.as_bytes());
    m.extend_from_slice(challenge);
    md5(&m)
}

/// What the server does with a caller's name and password: the protocol that was used, or why it
/// refused. `seed` makes the CHAP challenge (the same inputs give the same challenge).
pub(crate) fn authenticate(cfg: &PppoeServer, seed: &[u8], username: &str, password: &str) -> std::result::Result<PppAuth, String> {
    match cfg.auth {
        PppAuth::Pap => match cfg.user(username) {
            Some(u) if u.password == password => Ok(PppAuth::Pap),
            _ => Err("authentication failed: the server refused the name or password (PAP)".into()),
        },
        PppAuth::Chap => {
            let challenge = md5(seed);
            let id = challenge.first().copied().unwrap_or(1);
            // The dialer answers with its password; the server computes the same from its own copy.
            let answer = chap_response(id, password, &challenge);
            match cfg.user(username) {
                Some(u) if chap_response(id, &u.password, &challenge) == answer => Ok(PppAuth::Chap),
                _ => Err("authentication failed: the server refused the response (CHAP)".into()),
            }
        }
    }
}

/// The server on the other end of a dialer's cable and its port: discovery.
fn find_server(s: &Session, host: DeviceId) -> Result<(DeviceId, usize)> {
    let peer = s
        .topology
        .peer(End { device: host, port: 0 })
        .ok_or_else(|| EngineError::Failed("no PPPoE server answered: this computer's network port has no cable".into()))?;
    let srv = s.topology.device(peer.device).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    if srv.kind != DeviceKind::Server || !srv.services.pppoe.on || srv.powered_off {
        return Err(EngineError::Failed(format!("no PPPoE server answered: {} is not running the PPPoE service", srv.name)));
    }
    Ok((peer.device, peer.port))
}

/// Every IPv4 address some device of the lab holds.
fn addresses_in_use(s: &Session) -> BTreeSet<Ipv4Addr> {
    let mut used = BTreeSet::new();
    for d in &s.topology.devices {
        used.extend(netlabcraft_sim::l3_ifaces(d).iter().map(|i| i.net.addr));
        used.extend(d.ports.iter().filter_map(|p| p.ip).map(|n| n.addr));
    }
    used
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
    let (saved_user, saved_password, connected, host_name) = s
        .topology
        .device(d)
        .map(|x| (x.pppoe.username.clone(), x.pppoe.password.clone(), x.pppoe.session.is_some(), x.name.clone()))
        .ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let username = text_param(p, "username")?.map(|t| t.trim().to_string()).unwrap_or(saved_user);
    let password = text_param(p, "password")?.unwrap_or(saved_password);
    if username.is_empty() {
        return Err(bad("`username` (the account name) is required"));
    }
    if connected {
        return Err(EngineError::Failed("this computer is already connected: disconnect first".into()));
    }
    let (sid, sport) = find_server(s, d)?;
    let srv = s.topology.device(sid).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let cfg = srv.services.pppoe.clone();
    let net = srv
        .ports
        .get(sport)
        .and_then(|x| x.ip)
        .ok_or_else(|| EngineError::Failed(format!("{} has no IP address on that port, so it cannot give addresses out", srv.name)))?;
    let seed = format!("{}|{host_name}|{}", srv.name, s.sim.now_us());
    let auth = authenticate(&cfg, seed.as_bytes(), &username, &password).map_err(EngineError::Failed)?;
    let used = addresses_in_use(s);
    let address = cfg
        .pool()
        .find(|a| net.contains(*a) && !Ipv4Net::new(*a, net.prefix).is_network_or_broadcast() && !used.contains(a))
        .ok_or_else(|| EngineError::Failed("the server has no free address in its pool on this subnet".into()))?;
    s.checkpoint();
    if let Some(h) = s.topology.device_mut(d) {
        if let Some(port) = h.ports.first_mut() {
            port.ip = Some(Ipv4Net::new(address, net.prefix));
        }
        h.gateway = Some(net.addr);
        h.dhcp_client = false;
        h.dhcp_lease = None;
        h.pppoe.username = username;
        h.pppoe.password = password;
        h.pppoe.session = Some(PppoeSession { server: sid, address, gateway: net.addr, auth });
    }
    s.changed();
    status(s, p)
}

fn disconnect(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    if s.topology.device(d).is_none_or(|x| x.pppoe.session.is_none()) {
        return Err(EngineError::Failed("this computer is not connected".into()));
    }
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
    let host = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let found = find_server(s, d).ok();
    let session = host.pppoe.session.as_ref();
    // A session holds only while the server it came from is still on the cable.
    let live = session.is_some_and(|x| found.is_some_and(|(sid, _)| sid == x.server));
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
        "server_found": found.is_some(),
        "server": session.and_then(|x| name(x.server)).or_else(|| found.and_then(|(sid, _)| name(sid))),
        "address": session.map(|x| x.address.to_string()),
        "mask": session.and(mask),
        "gateway": session.map(|x| x.gateway.to_string()),
        "auth": session.map(|x| x.auth.name()),
    }))
}
