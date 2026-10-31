//! Device-family commands: a powered device's power adapter and where its power comes from
//! (`device.adapter`, `device.power.source`).

use netlabcraft_core::DeviceId;
use netlabcraft_model::poe::{self, Power};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Where a device's power comes from, as its Physical tab says it.
pub(crate) fn power_source(s: &Session, d: DeviceId) -> Value {
    let t = &s.topology;
    let Some(dev) = t.device(d) else { return Value::Null };
    let powered = poe::draw_mw(dev).is_some();
    let (source, detail) = match t.power(d) {
        _ if dev.powered_off => ("off", "switched off".to_string()),
        Power::Mains => ("mains", String::new()),
        Power::Adapter => ("adapter", "power adapter plugged in".to_string()),
        Power::Inline { switch, port, mw } => {
            let name = t.device(switch).map_or_else(String::new, |x| {
                format!("{} {}", x.name, x.ports.get(port).map_or_else(String::new, |p| netlabcraft_model::ifname::short(&p.name)))
            });
            ("poe", format!("{name}, {}.{} W", mw / 1000, (mw % 1000) / 100))
        }
        Power::Dark(why) => ("none", why),
    };
    json!({
        "device": dev.name,
        "powered_device": powered,
        "adapter": dev.adapter.unwrap_or(powered),
        "source": source,
        "detail": detail,
        "draw_w": poe::draw_mw(dev).map(|mw| f64::from(mw) / 1000.0),
    })
}

/// A device's power-supply bays.
fn psu_status(s: &Session, d: DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let bays: Vec<Value> = dev.psus().iter().enumerate().map(|(i, f)| json!({"bay": i + 1, "fitted": f})).collect();
    json!({"device": dev.name, "bays": bays, "power": if dev.powered_off { "off" } else { "on" }, "no_supply": dev.no_psu()})
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    match cmd {
        "device.power.source" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            Ok(power_source(s, d))
        }
        "device.adapter" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad("`on` (true: plugged in, false: unplugged) is required"))?;
            let dev = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
            if poe::draw_mw(dev).is_none() {
                return Err(bad(format!("{} is not powered over Ethernet: it has no separate power adapter here", dev.name)));
            }
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                dev.adapter = Some(on);
            }
            // Losing power is a restart: the phone forgets its registration and calls.
            s.sim.reset_device(d);
            s.changed();
            Ok(power_source(s, d))
        }
        "phone.configure" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| x.kind != netlabcraft_model::DeviceKind::Phone) {
                return Err(bad("phone.configure is for IP phones"));
            }
            let sip = match p.get("protocol").and_then(Value::as_str) {
                None => None,
                Some(v) if v.eq_ignore_ascii_case("sip") => Some(true),
                Some(v) if v.eq_ignore_ascii_case("own") || v.eq_ignore_ascii_case("netlabcraft") => Some(false),
                Some(v) => return Err(bad(format!("`protocol` is \"sip\" or \"own\", not `{v}`"))),
            };
            let server = match p.get("server") {
                None => None,
                Some(Value::String(a)) if a.trim().is_empty() => Some(None),
                Some(Value::String(a)) => Some(Some(netlabcraft_core::parse_ip(a.trim()).map_err(|e| bad(e.to_string()))?)),
                Some(_) => return Err(bad("`server` is an address")),
            };
            let address = match p.get("address") {
                None => None,
                Some(Value::String(a)) if a.trim().eq_ignore_ascii_case("dhcp") => Some("ipconfig /renew".to_string()),
                Some(Value::String(a)) => {
                    let ip = netlabcraft_core::parse_ip(a.trim()).map_err(|e| bad(e.to_string()))?;
                    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty());
                    let mask = text("mask").unwrap_or("255.255.255.0");
                    netlabcraft_core::parse_ip(mask).map_err(|e| bad(format!("`mask`: {e}")))?;
                    let gw = match text("gateway") {
                        Some(g) => {
                            netlabcraft_core::parse_ip(g).map_err(|e| bad(format!("`gateway`: {e}")))?;
                            format!(" {g}")
                        }
                        None => String::new(),
                    };
                    Some(format!("ipconfig {ip} {mask}{gw}"))
                }
                Some(_) => return Err(bad("`address` is \"dhcp\" or an IPv4 address (with `mask` and `gateway`)")),
            };
            if sip.is_none() && server.is_none() && address.is_none() {
                return Err(bad("give `protocol` (\"sip\" or \"own\"), `server` (the call server) and/or `address` (\"dhcp\" or an address)"));
            }
            if let Some(line) = address {
                // The phone's own address setting, typed as on any host.
                let out = crate::cmd::cli_line(s, d, &line);
                let text = out.get("output").and_then(Value::as_str).unwrap_or_default();
                if text.starts_with("DHCP failed") || text.starts_with('%') || text.starts_with("Usage") {
                    return Err(EngineError::Failed(text.trim().to_string()));
                }
                if sip.is_none() && server.is_none() {
                    s.sim.reset_device(d);
                    s.changed();
                    s.sim.phone_sync(&s.topology);
                    return Ok(crate::voice::status(s, d));
                }
            }
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                if let Some(v) = sip {
                    dev.voice.sip = v;
                }
                if let Some(a) = server {
                    dev.voice.sip_server = a;
                }
            }
            // The phone registers again with its new settings.
            s.sim.reset_device(d);
            s.changed();
            s.sim.phone_sync(&s.topology);
            Ok(crate::voice::status(s, d))
        }
        "device.psu" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            let dev = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
            let bays = dev.psus().len();
            if bays == 0 {
                return Err(bad(format!("{} has no power-supply bays", dev.name)));
            }
            let fitted = match p.get("fitted") {
                None => None,
                Some(Value::Bool(b)) => Some(*b),
                Some(_) => return Err(bad("`fitted` is true (fit a supply) or false (take it out)")),
            };
            if let Some(fit) = fitted {
                let bay = p
                    .get("bay")
                    .and_then(Value::as_u64)
                    .and_then(|b| usize::try_from(b).ok())
                    .filter(|b| (1..=bays).contains(b))
                    .ok_or_else(|| bad(format!("`bay` is 1 to {bays}")))?;
                let mut psus = dev.psus();
                if let Some(slot) = psus.get_mut(bay - 1) {
                    *slot = fit;
                }
                let lost = !fit && psus.iter().all(|f| !f) && !dev.powered_off;
                s.checkpoint();
                if let Some(dev) = s.topology.device_mut(d) {
                    dev.psu = Some(psus);
                    // The last supply out: the power is gone, as if switched off.
                    if lost {
                        dev.power_off();
                    }
                }
                if lost {
                    s.sim.reset_device(d);
                    s.consoles.remove(&d);
                }
                s.changed();
            }
            Ok(psu_status(s, d))
        }
        "cloud.map" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| !x.catalogue_model().cloud()) {
                return Err(bad("cloud.map is for clouds"));
            }
            let a = p.get("a").and_then(Value::as_str);
            let b = p.get("b").and_then(Value::as_str);
            match (a, b) {
                (Some(a), Some(b)) => {
                    let remove = p.get("remove").and_then(Value::as_bool).unwrap_or(false);
                    let mut t = s.topology.clone();
                    t.cloud_map(d, a, b, !remove)?;
                    s.checkpoint();
                    s.topology = t;
                    s.changed();
                }
                (None, None) => {}
                _ => return Err(bad("give both ports `a` and `b` (or neither, to list the pairs)")),
            }
            let dev = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
            let pairs: Vec<Value> = dev.bridges.iter().map(|(a, b)| json!([a, b])).collect();
            let ports: Vec<&str> = dev.ports.iter().map(|p| p.name.as_str()).collect();
            Ok(json!({"device": dev.name, "pairs": pairs, "ports": ports}))
        }
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}
