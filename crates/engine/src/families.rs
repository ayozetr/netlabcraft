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
            if sip.is_none() && server.is_none() {
                return Err(bad("give `protocol` (\"sip\" or \"own\") and/or `server` (the SIP registrar)"));
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
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}
