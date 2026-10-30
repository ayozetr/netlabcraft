//! IoT commands: a smart thing's settings (`iot.configure`), the reports to the server
//! (`iot.sync`), the server's IoT service and rules (`iot.server`) and switching a device by
//! hand from the server's page (`iot.switch`).

use netlabcraft_core::parse_ip;
use netlabcraft_model::DeviceKind;
use netlabcraft_model::iot::{IotRule, MAX_RULES, MAX_VALUE, MIN_VALUE, is_sensor};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// A smart thing as its own page shows it.
pub(crate) fn thing(s: &Session, d: netlabcraft_core::DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let st = s.sim.iot_thing(d).cloned().unwrap_or_default();
    json!({
        "device": dev.name,
        "kind": if is_sensor(dev) { "sensor" } else { "device" },
        "server": dev.iot.server.map(|a| a.to_string()),
        "value": is_sensor(dev).then_some(dev.iot.value),
        "registered": st.registered.map(|a| a.to_string()),
        "on": (!is_sensor(dev)).then_some(st.on),
        "message": st.message,
    })
}

/// A server's IoT page.
pub(crate) fn server(s: &Session, d: netlabcraft_core::DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let things: Vec<Value> = s
        .sim
        .iot_registry(d)
        .iter()
        .map(|e| json!({"name": e.name, "kind": e.kind, "address": e.address.to_string(), "value": e.value, "on": e.on}))
        .collect();
    let rules: Vec<Value> = dev.iot.rules.iter().map(|r| json!({"sensor": r.sensor, "above": r.above, "actuator": r.actuator})).collect();
    json!({"device": dev.name, "service": dev.iot.service, "rules": rules, "things": things})
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    match cmd {
        "iot.sync" => {
            s.sim.iot_sync(&s.topology);
            s.revision += 1;
            let things: Vec<Value> = s.topology.devices.iter().filter(|d| d.kind == DeviceKind::Iot).map(|d| thing(s, d.id)).collect();
            Ok(json!({"things": things}))
        }
        "iot.configure" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| x.kind != DeviceKind::Iot) {
                return Err(bad("iot.configure is for smart sensors and smart devices"));
            }
            let server = match p.get("server") {
                None => None,
                Some(Value::String(a)) if a.trim().is_empty() => Some(None),
                Some(Value::String(a)) => Some(Some(parse_ip(a.trim()).map_err(|e| bad(e.to_string()))?)),
                Some(_) => return Err(bad("`server` is an address")),
            };
            let value = match p.get("value") {
                None => None,
                Some(v) => Some(
                    v.as_i64()
                        .filter(|v| (i64::from(MIN_VALUE)..=i64::from(MAX_VALUE)).contains(v))
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or_else(|| bad(format!("`value` is a whole number from {MIN_VALUE} to {MAX_VALUE}")))?,
                ),
            };
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                if let Some(a) = server {
                    dev.iot.server = a;
                }
                if let Some(v) = value {
                    dev.iot.value = v;
                }
            }
            s.changed();
            // A new reading goes to the server at once, and the devices hear what it decided.
            s.sim.iot_sync(&s.topology);
            Ok(thing(s, d))
        }
        "iot.server" | "iot.switch" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| x.kind != DeviceKind::Server) {
                return Err(bad("the IoT service runs on servers"));
            }
            if cmd == "iot.switch" {
                let name = p.get("name").and_then(Value::as_str).ok_or_else(|| bad("`name` (a registered smart device) is required"))?;
                let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad("`on` (true/false) is required"))?;
                s.sim.iot_switch(d, name, on).map_err(|e| EngineError::Failed(e.to_string()))?;
                s.sim.iot_sync(&s.topology);
                s.revision += 1;
                return Ok(server(s, d));
            }
            let service = p.get("service").and_then(Value::as_bool);
            let rules = match p.get("rules") {
                None => None,
                Some(Value::Array(a)) => {
                    if a.len() > MAX_RULES {
                        return Err(bad(format!("at most {MAX_RULES} rules")));
                    }
                    let mut out = Vec::new();
                    for r in a {
                        let text = |k: &str| r.get(k).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string);
                        let (Some(sensor), Some(actuator)) = (text("sensor"), text("actuator")) else {
                            return Err(bad("a rule is {\"sensor\", \"above\", \"actuator\"}"));
                        };
                        let above = r
                            .get("above")
                            .and_then(Value::as_i64)
                            .and_then(|v| i32::try_from(v).ok())
                            .ok_or_else(|| bad("`above` is a whole number"))?;
                        out.push(IotRule { sensor, above, actuator });
                    }
                    Some(out)
                }
                Some(_) => return Err(bad("`rules` is a list")),
            };
            if service.is_some() || rules.is_some() {
                s.checkpoint();
                if let Some(dev) = s.topology.device_mut(d) {
                    if let Some(on) = service {
                        dev.iot.service = on;
                    }
                    if let Some(r) = rules {
                        dev.iot.rules = r;
                        dev.iot.sanitize();
                    }
                }
                s.changed();
            }
            // The things report (as they do every few seconds on real networks), then the
            // page shows what the server knows.
            s.sim.iot_sync(&s.topology);
            s.revision += 1;
            Ok(server(s, d))
        }
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn a_hot_sensor_switches_the_fan_on_through_the_server() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "iot"})).unwrap();
        let page = s.execute("iot.server", json!({"device": "IoT-Server"})).unwrap();
        let things = page["things"].as_array().unwrap();
        assert_eq!(things.len(), 2, "{page}");
        let fan = |s: &mut Session| {
            s.execute("iot.sync", json!({})).unwrap()["things"].as_array().unwrap().iter().find(|t| t["device"] == "Fan1").unwrap()["on"].clone()
        };
        assert_eq!(fan(&mut s), json!(false), "25 degrees: the fan stays off");
        let r = s.execute("iot.configure", json!({"device": "Sensor1", "value": 31})).unwrap();
        assert_eq!(r["registered"], "192.168.1.2", "{r}");
        assert_eq!(fan(&mut s), json!(true), "above 30 the rule switches it on");
        let page = s.execute("iot.server", json!({"device": "IoT-Server"})).unwrap();
        assert!(page["things"].as_array().unwrap().iter().any(|t| t["name"] == "Sensor1" && t["value"] == 31), "{page}");
        // By hand from the server's page.
        s.execute("iot.switch", json!({"device": "IoT-Server", "name": "Fan1", "on": false})).unwrap();
        assert_eq!(s.execute("iot.configure", json!({"device": "Fan1"})).unwrap()["on"], false);
        assert!(s.execute("iot.switch", json!({"device": "IoT-Server", "name": "Sensor1", "on": true})).is_err());
        // The service off: things can't register.
        s.execute("iot.server", json!({"device": "IoT-Server", "service": false})).unwrap();
        let r = s.execute("iot.sync", json!({})).unwrap();
        assert!(r["things"][0]["message"].as_str().is_some(), "{r}");
        // Hostile settings.
        assert!(s.execute("iot.configure", json!({"device": "Sensor1", "value": 1_000_000})).is_err());
        assert!(s.execute("iot.configure", json!({"device": "PC1", "value": 3})).is_err());
        assert!(s.execute("iot.server", json!({"device": "IoT-Server", "rules": [{"sensor": "x"}]})).is_err());
        assert!(s.execute("iot.server", json!({"device": "Sensor1", "service": true})).is_err());
    }
}
