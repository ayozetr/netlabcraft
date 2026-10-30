//! IoT commands: a smart thing's settings (`iot.configure`), the reports to the server
//! (`iot.sync`), the IoT service and its rules (`iot.server`, on servers and home gateways)
//! and switching a device by hand from the service's page (`iot.switch`).

use netlabcraft_core::parse_ip;
use netlabcraft_model::DeviceKind;
use netlabcraft_model::iot::{Condition, IotRule, MAX_CONDITIONS, MAX_INTERVAL, MAX_RULES, MAX_VALUE, MIN_VALUE, Op, can_serve, is_sensor, thing};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// A smart thing as its own page shows it.
pub(crate) fn thing_page(s: &Session, d: netlabcraft_core::DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let st = s.sim.iot_thing(d).cloned().unwrap_or_default();
    json!({
        "device": dev.name,
        "kind": if is_sensor(dev) { "sensor" } else { "device" },
        "what": thing(dev).map(|t| t.what),
        "server": dev.iot.server.map(|a| a.to_string()),
        "value": is_sensor(dev).then_some(dev.iot.value),
        "interval": dev.iot.interval,
        "registered": st.registered.map(|a| a.to_string()),
        "on": (!is_sensor(dev)).then_some(st.on),
        "image": st.image,
        "message": st.message,
    })
}

/// A rule as the commands give it back.
fn rule_json(r: &IotRule) -> Value {
    let conds: Vec<Value> = r.all_conditions().iter().map(|c| json!({"sensor": c.sensor, "op": c.op.symbol(), "value": c.value})).collect();
    json!({"conditions": conds, "join": if r.any { "or" } else { "and" }, "actuator": r.actuator, "on": r.on, "text": r.text()})
}

/// An IoT service's page.
pub(crate) fn server(s: &Session, d: netlabcraft_core::DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let things: Vec<Value> = s
        .sim
        .iot_registry(d)
        .iter()
        .map(|e| json!({"name": e.name, "kind": e.kind, "what": e.what, "address": e.address.to_string(), "value": e.value, "on": e.on, "image": e.image}))
        .collect();
    let rules: Vec<Value> = dev.iot.rules.iter().map(rule_json).collect();
    json!({"device": dev.name, "service": dev.iot.service, "rules": rules, "things": things})
}

/// A rule from its JSON: `{"sensor", "above", "actuator"}` (one condition), or
/// `{"conditions": [{"sensor", "op": ">"|"<"|"=", "value"}], "join"?: "and"|"or", "actuator", "on"?: bool}`.
fn parse_rule(r: &Value) -> Result<IotRule> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string);
    let whole = |v: &Value, k: &str| v.get(k).and_then(Value::as_i64).and_then(|v| i32::try_from(v).ok());
    let actuator = text(r, "actuator").ok_or_else(|| bad("a rule needs its `actuator` (the smart device it switches)"))?;
    let mut conditions = Vec::new();
    if let Some(sensor) = text(r, "sensor") {
        let above = whole(r, "above").ok_or_else(|| bad("`above` is a whole number"))?;
        conditions.push(Condition { sensor, op: Op::Above, value: above });
    }
    if let Some(list) = r.get("conditions") {
        let list = list.as_array().ok_or_else(|| bad("`conditions` is a list"))?;
        if list.len() > MAX_CONDITIONS {
            return Err(bad(format!("at most {MAX_CONDITIONS} conditions in a rule")));
        }
        for c in list {
            let sensor = text(c, "sensor").ok_or_else(|| bad("a condition is {\"sensor\", \"op\", \"value\"}"))?;
            let op = match c.get("op").and_then(Value::as_str) {
                None => Op::Above,
                Some(o) => Op::parse(o).ok_or_else(|| bad(format!("`op` is >, < or =, not `{o}`")))?,
            };
            let value = whole(c, "value").ok_or_else(|| bad("a condition's `value` is a whole number"))?;
            conditions.push(Condition { sensor, op, value });
        }
    }
    if conditions.is_empty() {
        return Err(bad("a rule is {\"sensor\", \"above\", \"actuator\"} or {\"conditions\": [...], \"join\", \"actuator\", \"on\"}"));
    }
    let any = match r.get("join").and_then(Value::as_str) {
        None => false,
        Some(j) if j.eq_ignore_ascii_case("and") => false,
        Some(j) if j.eq_ignore_ascii_case("or") => true,
        Some(j) => return Err(bad(format!("`join` is \"and\" or \"or\", not `{j}`"))),
    };
    let on = match r.get("on") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("`on` is true or false")),
    };
    Ok(IotRule { sensor: String::new(), above: 0, actuator, conditions, any, on })
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    match cmd {
        "iot.sync" => {
            s.sim.iot_sync(&s.topology);
            s.revision += 1;
            let things: Vec<Value> = s.topology.devices.iter().filter(|d| d.kind == DeviceKind::Iot).map(|d| thing_page(s, d.id)).collect();
            Ok(json!({"things": things}))
        }
        "iot.configure" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| x.kind != DeviceKind::Iot) {
                return Err(bad("iot.configure is for smart things"));
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
            let interval = match p.get("interval") {
                None => None,
                Some(v) => Some(
                    v.as_u64()
                        .filter(|v| *v <= u64::from(MAX_INTERVAL))
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or_else(|| bad(format!("`interval` is 0 (only on change) to {MAX_INTERVAL} seconds")))?,
                ),
            };
            if value.is_some() && s.topology.device(d).is_some_and(|x| !is_sensor(x)) {
                return Err(bad("only sensors have a reading; the server switches smart devices"));
            }
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                if let Some(a) = server {
                    dev.iot.server = a;
                }
                if let Some(v) = value {
                    dev.iot.value = v;
                }
                if let Some(i) = interval {
                    dev.iot.interval = i;
                }
            }
            s.changed();
            // A new reading goes to the server at once, and the devices hear what it decided.
            s.sim.iot_sync(&s.topology);
            Ok(thing_page(s, d))
        }
        "iot.server" | "iot.switch" => {
            let d = crate::cmd::device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| !can_serve(x)) {
                return Err(bad("the IoT service runs on servers and home gateways"));
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
                    Some(a.iter().map(parse_rule).collect::<Result<Vec<_>>>()?)
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
            // The things report (as they do on their timers), then the page shows what the
            // service knows.
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
