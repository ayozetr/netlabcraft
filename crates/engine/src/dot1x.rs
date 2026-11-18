//! The certificate of a Server's RADIUS (802.1X) service and what a host's WPA2-Enterprise login
//! trusts: `aaa.certificate` and `dot1x.trust`.
//!
//! A Server has no console, so its certificate is set by command: issued by a certificate
//! authority in the lab (a router running `crypto pki server`), or self-signed, both for a key
//! pair the simulator makes for it. A host's login validates the certificate the server shows in
//! the PEAP tunnel against the CA certificates it trusts and, when asked, the name.

use netlabcraft_model::DeviceKind;
use netlabcraft_model::services::AaaCert;
use netlabcraft_model::wlan::MAX_TRUSTED_CAS;
use netlabcraft_sim::pki as simpki;
use netlabcraft_wire::rsa::SigHash;
use netlabcraft_wire::x509::{AltNames, Certificate, Request};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "aaa.certificate" => Some(certificate(s, p)),
        "dot1x.trust" => Some(trust(s, p)),
        "dot1x.status" => Some(status(s, p)),
        "dot1x.supplicant" => Some(supplicant(s, p)),
        _ => None,
    }
}

fn text<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty())
}

fn describe(c: &Certificate, chain_length: usize) -> Value {
    json!({
        "subject": c.subject.text(),
        "issuer": c.issuer.text(),
        "serial": simpki::serial_hex(&c.serial),
        "not_before": c.not_before,
        "not_after": c.not_after,
        "key_bits": c.key.bits(),
        "chain_length": chain_length,
    })
}

fn certificate(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "device")?;
    let dev = s.topology.device(d).ok_or_else(|| bad("no such device"))?;
    if dev.kind != DeviceKind::Server {
        return Err(bad("only a Server runs the RADIUS service that presents a certificate"));
    }
    if p.get("clear").and_then(Value::as_bool) == Some(true) {
        s.checkpoint();
        if let Some(x) = s.topology.device_mut(d) {
            x.services.aaa.cert = None;
        }
        s.sim.forget_associations();
        s.changed();
        return Ok(json!({"certificate": null}));
    }
    let name = text(p, "subject").map_or_else(|| format!("CN={}, O=NetLab", dev.name), str::to_string);
    let subject = simpki::parse_name(&name, &dev.name).map_err(bad)?;
    let days = p.get("days").and_then(Value::as_u64).map_or(365, |n| n.clamp(1, 36_500)) as u32;
    let alt = AltNames { dns: text(p, "fqdn").map(|f| vec![f.to_string()]).unwrap_or_default(), ips: Vec::new() };
    let now = s.sim.clock(d).0;
    let key_pair = simpki::generate_key(dev, "aaa", 1024).map_err(bad)?;
    let key = simpki::key_from_hex(&key_pair.p, &key_pair.q).ok_or_else(|| bad("the key pair could not be made"))?;
    let (leaf, ca_pem) = match text(p, "ca") {
        Some(ca_name) => {
            let ca = s.topology.devices.iter().find(|x| x.name == ca_name).ok_or_else(|| bad(format!("no device named `{ca_name}`")))?;
            let ca_id = ca.id;
            let ca_cert = simpki::ca_certificate(ca).ok_or_else(|| bad(format!("{ca_name} is not running a certificate authority")))?;
            let request = Request::create_alt(&subject, &key, SigHash::Sha256, &alt).map_err(|e| bad(e.to_string()))?;
            let leaf = simpki::sign_request(ca, s.sim.clock(ca_id).0, request.der(), 1).map_err(bad)?;
            (leaf, ca_cert.to_pem())
        }
        None if p.get("self_signed").and_then(Value::as_bool) == Some(true) => {
            let leaf = simpki::issue_alt(&subject, &key, &subject, &key, &[1], now, days, true, &alt).map_err(bad)?;
            (leaf, String::new())
        }
        None => return Err(bad("`ca` (a router running a certificate authority) or `self_signed`: true is required, or `clear`: true")),
    };
    let chain_length = if ca_pem.is_empty() { 1 } else { 2 };
    let cert = AaaCert { certificate: leaf.to_pem(), ca_certificate: ca_pem, p: key_pair.p, q: key_pair.q };
    s.checkpoint();
    if let Some(x) = s.topology.device_mut(d) {
        x.services.aaa.cert = Some(cert);
    }
    s.sim.forget_associations();
    s.changed();
    Ok(describe(&leaf, chain_length))
}

fn trust(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "device")?;
    let dev = s.topology.device(d).ok_or_else(|| bad("no such device"))?;
    if !dev.kind.is_host() {
        return Err(bad("only a host logs in to an enterprise network"));
    }
    let port = match text(p, "port") {
        Some(n) => crate::cmd::port_param(s, d, n)?,
        None => {
            dev.ports.iter().position(|x| netlabcraft_model::ifname::is_wireless(&x.name)).ok_or_else(|| bad(format!("{} has no radio", dev.name)))?
        }
    };
    let has_login = dev.ports.get(port).and_then(|x| x.wifi.as_ref()).is_some_and(|w| w.login.is_some());
    if !has_login {
        return Err(bad("the radio has no login yet: set `user` and `password` with device.wireless first"));
    }
    // The CA certificates to add: a pasted one, or the one a lab device holds (a router's
    // authority, or the CA that issued a Server's certificate).
    let mut add: Vec<String> = Vec::new();
    if let Some(pem) = text(p, "ca_pem") {
        add.push(Certificate::from_pem(pem).map_err(|e| bad(format!("`ca_pem` is not a certificate: {e}")))?.to_pem());
    }
    if let Some(from) = text(p, "ca") {
        let other = s.topology.devices.iter().find(|x| x.name == from).ok_or_else(|| bad(format!("no device named `{from}`")))?;
        let pem = simpki::ca_certificate(other)
            .map(|c| c.to_pem())
            .or_else(|| {
                other.services.aaa.cert.as_ref().map(|c| if c.ca_certificate.is_empty() { c.certificate.clone() } else { c.ca_certificate.clone() })
            })
            .or_else(|| other.pki.trustpoints.iter().map(|t| t.ca_certificate.clone()).find(|c| !c.is_empty()))
            .ok_or_else(|| bad(format!("{from} holds no CA certificate")))?;
        add.push(Certificate::from_pem(&pem).map_err(|e| bad(e.to_string()))?.to_pem());
    }
    let clear = p.get("clear").and_then(Value::as_bool) == Some(true);
    let validate = p.get("validate").and_then(Value::as_bool);
    let name = p.get("server_name").and_then(Value::as_str).map(|n| n.trim().chars().take(128).collect::<String>());
    s.checkpoint();
    let login = s
        .topology
        .device_mut(d)
        .and_then(|x| x.ports.get_mut(port))
        .and_then(|x| x.wifi.as_mut())
        .and_then(|w| w.login.as_mut())
        .ok_or_else(|| bad("the radio has no login"))?;
    if clear {
        login.trusted_cas.clear();
    }
    for pem in add {
        if !login.trusted_cas.contains(&pem) && login.trusted_cas.len() < MAX_TRUSTED_CAS {
            login.trusted_cas.push(pem);
        }
    }
    if let Some(v) = validate {
        login.validate_server = v;
    }
    if let Some(n) = name {
        login.server_name = n;
    }
    let out = json!({
        "validate": login.validate_server,
        "server_name": login.server_name,
        "trusted_cas": login.trusted_cas.iter().filter_map(|c| Certificate::from_pem(c).ok()).map(|c| c.subject.text()).collect::<Vec<_>>(),
    });
    s.sim.forget_associations();
    s.changed();
    Ok(out)
}

/// What a host's login saw of the RADIUS server's certificate, as the client's details show it.
pub(crate) fn report_json(r: &netlabcraft_sim::peap::Report) -> Value {
    json!({
        "presented": r.presented,
        "subject": r.subject,
        "issuer": r.issuer,
        "not_after": r.not_after,
        "chain_length": r.chain_length,
        "verified": r.verified,
        "error": r.error,
        "suite": r.suite,
    })
}

/// The wired port a supplicant is set on: the one named, else the first that has a login,
/// else the first Ethernet port.
fn wired_port(s: &Session, d: netlabcraft_core::DeviceId, p: &Value) -> Result<usize> {
    let dev = s.topology.device(d).ok_or_else(|| bad("no such device"))?;
    match text(p, "port") {
        Some(n) => crate::cmd::port_param(s, d, n),
        None => dev
            .ports
            .iter()
            .position(|x| x.supplicant.is_some())
            .or_else(|| dev.ports.iter().position(|x| !netlabcraft_model::ifname::is_wireless(&x.name)))
            .ok_or_else(|| bad(format!("{} has no wired port", dev.name))),
    }
}

fn supplicant(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "device")?;
    let dev = s.topology.device(d).ok_or_else(|| bad("no such device"))?;
    if !dev.kind.is_host() {
        return Err(bad("only a host runs a supplicant"));
    }
    let port = wired_port(s, d, p)?;
    let clear = p.get("clear").and_then(Value::as_bool) == Some(true);
    let login = if clear {
        None
    } else {
        let user = text(p, "user").ok_or_else(|| bad("`user` (the login name) is required, or `clear`: true"))?;
        let password = p.get("password").and_then(Value::as_str).unwrap_or_default();
        let mut l = netlabcraft_model::wlan::WifiLogin { user: user.chars().take(64).collect(), password: password.chars().take(64).collect(), ..Default::default() };
        // The trust settings stay as they were when only the credentials change.
        if let Some(old) = s.topology.device(d).and_then(|x| x.ports.get(port)).and_then(|x| x.supplicant.clone()) {
            l.validate_server = old.validate_server;
            l.trusted_cas = old.trusted_cas;
            l.server_name = old.server_name;
        }
        if let Some(v) = p.get("validate").and_then(Value::as_bool) {
            l.validate_server = v;
        }
        Some(l)
    };
    s.checkpoint();
    let name = {
        let x = s.topology.device_mut(d).and_then(|x| x.ports.get_mut(port)).ok_or_else(|| bad("no such port"))?;
        x.supplicant = login.clone();
        x.name.clone()
    };
    s.sim.forget_dot1x();
    s.changed();
    Ok(json!({"port": name, "user": login.map(|l| l.user)}))
}

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "device")?;
    let dev = s.topology.device(d).ok_or_else(|| bad("no such device"))?;
    let named_wired = text(p, "port").is_some_and(|n| crate::cmd::port_param(s, d, n).ok().and_then(|i| dev.ports.get(i)).is_some_and(|x| !netlabcraft_model::ifname::is_wireless(&x.name)));
    let has_radio = dev.ports.iter().any(|x| netlabcraft_model::ifname::is_wireless(&x.name));
    if named_wired || (text(p, "port").is_none() && !has_radio) {
        let port = wired_port(s, d, p)?;
        let login = s.topology.device(d).and_then(|x| x.ports.get(port)).and_then(|x| x.supplicant.clone());
        let seen = s.sim.dot1x_supplicant(d, port);
        return Ok(json!({
            "wired": true,
            "login": login.as_ref().map(|l| l.user.clone()),
            "authenticated": seen.is_some_and(|(r, _, _)| r == Some(true)),
            "refused": seen.is_some_and(|(r, _, _)| r == Some(false)),
            "eapol_received": seen.map_or(0, |x| x.1),
            "eapol_sent": seen.map_or(0, |x| x.2),
        }));
    }
    let port = match text(p, "port") {
        Some(n) => crate::cmd::port_param(s, d, n)?,
        None => {
            dev.ports.iter().position(|x| netlabcraft_model::ifname::is_wireless(&x.name)).ok_or_else(|| bad(format!("{} has no radio", dev.name)))?
        }
    };
    let wifi = dev.ports.get(port).and_then(|x| x.wifi.as_ref());
    let login = wifi.and_then(|w| w.login.as_ref());
    let assoc = s.sim.association(netlabcraft_model::End { device: d, port });
    Ok(json!({
        "login": login.map(|l| l.user.clone()),
        "validate": login.is_some_and(|l| l.validate_server),
        "server_name": login.map(|l| l.server_name.clone()).unwrap_or_default(),
        "trusted_cas": login.map(|l| l.trusted_cas.iter().filter_map(|c| Certificate::from_pem(c).ok()).map(|c| c.subject.text()).collect::<Vec<_>>()).unwrap_or_default(),
        "associated": assoc.is_some_and(|a| a.done),
        "server_certificate": assoc.and_then(|a| a.peap_report()).map(report_json),
    }))
}
