//! Hardware commands: the power switch and the cards of modular routers.

use packetcraft_model::models;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| bad(format!("`{key}` (text) is required")))
}

/// What a device's hardware looks like: power, bays and the cards in them.
fn describe(s: &Session, d: packetcraft_core::DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let model = dev.catalogue_model();
    let slots: Vec<Value> = model
        .slots()
        .iter()
        .map(|slot| {
            let card = dev.module_in(slot);
            json!({
                "slot": slot,
                "module": card.map(|c| c.id),
                "label": card.map(|c| c.label),
                "ports": card.map(|c| c.port_names(slot)).unwrap_or_default(),
            })
        })
        .collect();
    json!({"device": dev.name, "model": model.id, "power": if dev.powered_off { "off" } else { "on" }, "slots": slots})
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    match cmd {
        "device.module.list" => {
            let cards: Vec<Value> = models::MODULES
                .iter()
                .map(|m| json!({"id": m.id, "label": m.label, "description": m.description, "ports": m.port_count, "type": m.port_type}))
                .collect();
            let device = match p.get("device") {
                Some(Value::Null) | None => Value::Null,
                Some(_) => describe(s, crate::cmd::device_param(s, p, "device")?),
            };
            Ok(json!({"modules": cards, "device": device}))
        }
        "device.module.add" | "device.module.remove" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            let slot = text(p, "slot")?;
            let module = if cmd == "device.module.add" { Some(text(p, "module")?) } else { None };
            let mut t = s.topology.clone();
            let ports = t.set_module(d, slot, module)?;
            s.checkpoint();
            s.topology = t;
            s.sim.reset_device(d);
            s.changed();
            let mut out = describe(s, d);
            if let Some(o) = out.as_object_mut() {
                o.insert(if module.is_some() { "added".into() } else { "removed".into() }, json!(ports));
            }
            Ok(out)
        }
        "device.power" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            let on = match p.get("on") {
                Some(Value::Bool(b)) => *b,
                Some(Value::String(v)) if v.eq_ignore_ascii_case("on") => true,
                Some(Value::String(v)) if v.eq_ignore_ascii_case("off") => false,
                _ => return Err(bad("`on` (true/false, or \"on\"/\"off\") is required")),
            };
            let dev = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
            if dev.powered_off != on {
                return Ok(json!({"power": if on { "on" } else { "off" }, "changed": false, "errors": []}));
            }
            s.checkpoint();
            let mut errors = Vec::new();
            if on {
                errors = power_on(s, d);
            } else if let Some(dev) = s.topology.device_mut(d) {
                dev.power_off();
            }
            s.sim.reset_device(d);
            // A fresh console: the device starts again at its first prompt.
            s.consoles.remove(&d);
            s.changed();
            Ok(json!({"power": if on { "on" } else { "off" }, "changed": true, "errors": errors}))
        }
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}

/// Boots a switched-off device: factory settings, then the saved configuration (routers and
/// switches), as after a reload.
fn power_on(s: &mut Session, d: packetcraft_core::DeviceId) -> Vec<String> {
    let Some(dev) = s.topology.device_mut(d) else { return Vec::new() };
    dev.powered_off = false;
    if !dev.has_running_config() {
        return Vec::new();
    }
    if dev.vlan_dat_deleted {
        dev.vlans.clear();
        dev.vlan_dat_deleted = false;
        dev.ensure_default_vlan();
    }
    dev.reset_config();
    let startup = dev.startup.clone();
    match startup {
        Some(cfg) => packetcraft_ios::apply_config(&mut s.topology, &mut s.sim, d, &cfg),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn cli(s: &mut Session, dev: &str, line: &str) -> String {
        s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn cards_go_in_only_while_switched_off_and_power_loses_what_was_not_saved() {
        let mut s = Session::new();
        s.execute("device.add", json!({"model": "Modular Router 2GE", "name": "R1"})).unwrap();
        let list = s.execute("device.module.list", json!({"device": "R1"})).unwrap();
        assert_eq!(list["device"]["slots"].as_array().unwrap().len(), 2);
        assert!(list["modules"].as_array().unwrap().iter().any(|m| m["id"] == "Card-2S"));
        let err = s.execute("device.module.add", json!({"device": "R1", "slot": "0/1", "module": "Card-2S"})).unwrap_err();
        assert!(err.to_string().contains("switch it off"), "{err}");
        // Typed configuration, not saved.
        for l in ["enable", "configure terminal", "interface g0/0", "ip address 10.0.0.1 255.255.255.0", "end"] {
            cli(&mut s, "R1", l);
        }
        assert!(s.topology.devices[0].ports[0].ip.is_some());
        let r = s.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
        assert_eq!(r["power"], "off");
        assert!(s.topology.devices[0].ports[0].ip.is_none());
        assert!(cli(&mut s, "R1", "enable").contains("switched off"));
        let r = s.execute("device.module.add", json!({"device": "R1", "slot": "0/1", "module": "Card-2S"})).unwrap();
        assert_eq!(r["added"], json!(["Serial0/1/0", "Serial0/1/1"]));
        s.execute("device.power", json!({"device": "R1", "on": "on"})).unwrap();
        let brief = cli(&mut s, "R1", "show ip interface brief");
        assert!(brief.contains("Serial0/1/0"), "{brief}");
        // Undo takes the card back out.
        s.execute("edit.undo", json!({})).unwrap();
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.topology.devices[0].ports.len(), 2);
        assert!(s.execute("device.module.remove", json!({"device": "R1", "slot": "0/0"})).is_err());
        assert!(s.execute("device.power", json!({"device": "R1"})).is_err());
    }

    #[test]
    fn the_modular_sample_survives_a_power_cycle() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "modular"})).unwrap();
        let ping = |s: &mut Session| {
            s.execute("net.ping", json!({"from": "PC1", "to": "10.2.2.10", "count": 4})).unwrap();
            s.execute("net.ping", json!({"from": "PC1", "to": "10.2.2.10", "count": 3})).unwrap()["marks"].as_str().unwrap().to_string()
        };
        assert_eq!(ping(&mut s), "!!!");
        s.execute("device.power", json!({"device": "Branch", "on": false})).unwrap();
        assert!(s.topology.find("Branch").unwrap().ports[0].ip.is_none(), "running configuration gone");
        assert_ne!(ping(&mut s), "!!!");
        let r = s.execute("device.power", json!({"device": "Branch", "on": true})).unwrap();
        assert_eq!(r["errors"], json!([]));
        let brief = cli(&mut s, "Branch", "show ip interface brief");
        // The switch port facing Branch listens and learns for 30 s first.
        s.execute("sim.advance", json!({"seconds": 35})).unwrap();
        assert_eq!(ping(&mut s), "!!!", "booted from the saved configuration: {brief}");
        // Taking the serial card out unplugs the WAN cable.
        s.execute("device.power", json!({"device": "Branch", "on": false})).unwrap();
        let links = s.topology.links.len();
        s.execute("device.module.remove", json!({"device": "Branch", "slot": "0/1"})).unwrap();
        assert_eq!(s.topology.links.len(), links - 1);
    }
}
