//! IoT commands: a smart thing's settings (`iot.configure`), the reports to the server
//! (`iot.sync`), the IoT service and its rules (`iot.server`, on servers and home gateways)
//! and switching a device by hand from the service's page (`iot.switch`).

use netlabcraft_core::parse_ip;
use netlabcraft_model::DeviceKind;
use netlabcraft_model::environment::{MAX_CO, MAX_TEMP, MIN_TEMP, QUANTITIES};
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
        "value": is_sensor(dev).then(|| s.sim.iot_reading(&s.topology, dev)),
        "interval": dev.iot.interval,
        "registered": st.registered.map(|a| a.to_string()),
        "on": (!is_sensor(dev)).then_some(st.on),
        "image": st.image,
        "message": st.message,
    })
}

/// The room: its settings, and its values now while the physics runs.
pub(crate) fn environment(s: &Session) -> Value {
    let e = &s.topology.environment;
    let room = s.sim.environment().map(|c| {
        let mut m = serde_json::Map::new();
        for q in QUANTITIES {
            m.insert(q.into(), json!(c.reading(q)));
        }
        m.insert("light".into(), json!(c.light));
        Value::Object(m)
    });
    json!({
        "enabled": e.enabled,
        "outside": {"temperature": e.outside_temperature, "humidity": e.outside_humidity, "light": e.outside_light},
        "start": {"temperature": e.temperature, "humidity": e.humidity, "soil": e.soil, "smoke": e.smoke, "co": e.co, "motion": e.motion},
        "room": room,
    })
}

/// `iot.environment`: reads or changes the shared room.
fn set_environment(s: &mut Session, p: &Value) -> Result<Value> {
    let whole = |k: &str, lo: i32, hi: i32| -> Result<Option<i32>> {
        match p.get(k) {
            None => Ok(None),
            Some(v) => v
                .as_i64()
                .filter(|v| (i64::from(lo)..=i64::from(hi)).contains(v))
                .and_then(|v| i32::try_from(v).ok())
                .map(Some)
                .ok_or_else(|| bad(format!("`{k}` is a whole number from {lo} to {hi}"))),
        }
    };
    let flag = |k: &str| -> Result<Option<bool>> {
        match p.get(k) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(bad(format!("`{k}` is true or false"))),
        }
    };
    let enabled = flag("enabled")?;
    let reset = flag("reset")?.unwrap_or(false);
    let temperature = whole("temperature", MIN_TEMP, MAX_TEMP)?;
    let humidity = whole("humidity", 0, 100)?;
    let soil = whole("soil", 0, 100)?;
    let smoke = whole("smoke", 0, 100)?;
    let co = whole("co", 0, MAX_CO)?;
    let motion = whole("motion", 0, 1)?;
    let out_t = whole("outside_temperature", MIN_TEMP, MAX_TEMP)?;
    let out_h = whole("outside_humidity", 0, 100)?;
    let out_l = whole("outside_light", 0, 100)?;
    let touched = [temperature, humidity, soil, smoke, co, motion, out_t, out_h, out_l].iter().any(Option::is_some);
    if enabled.is_some() || touched || reset {
        s.checkpoint();
        let e = &mut s.topology.environment;
        let was = e.enabled;
        if let Some(v) = enabled {
            e.enabled = v;
        }
        let set = |slot: &mut i32, v: Option<i32>| {
            if let Some(v) = v {
                *slot = v;
            }
        };
        set(&mut e.temperature, temperature);
        set(&mut e.humidity, humidity);
        set(&mut e.soil, soil);
        set(&mut e.smoke, smoke);
        set(&mut e.co, co);
        set(&mut e.motion, motion);
        set(&mut e.outside_temperature, out_t);
        set(&mut e.outside_humidity, out_h);
        set(&mut e.outside_light, out_l);
        if reset || was != e.enabled {
            s.sim.env_reset();
        }
        s.changed();
        // The values given now are also the room's values now.
        if s.topology.environment.enabled {
            s.sim.env_step(&s.topology);
            for (q, v) in [("temperature", temperature), ("humidity", humidity), ("soil", soil), ("smoke", smoke), ("co", co), ("motion", motion)] {
                if let Some(v) = v {
                    s.sim.env_set(&s.topology, q, v);
                }
            }
        }
        s.sim.iot_sync(&s.topology);
    }
    s.revision += 1;
    Ok(environment(s))
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
            let things: Vec<Value> = s.topology.devices.iter().filter(|d| thing(d).is_some()).map(|d| thing_page(s, d.id)).collect();
            Ok(json!({"things": things}))
        }
        "iot.environment" => set_environment(s, p),
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
            // With the physics on, a sensor's new reading is the room's new quantity.
            if let (Some(v), true, Some(w)) = (value, s.topology.environment.enabled, s.topology.device(d).and_then(thing).map(|t| t.what)) {
                let q = if w == "thermostat" { "temperature" } else { w };
                s.sim.env_step(&s.topology);
                s.sim.env_set(&s.topology, q, v);
            }
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

    fn temp(s: &mut Session) -> i64 {
        s.execute("iot.environment", json!({})).unwrap()["room"]["temperature"].as_i64().unwrap()
    }

    #[test]
    fn a_thermostat_rule_closes_the_loop_in_simulated_time() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "iot-climate"})).unwrap();
        s.execute("iot.sync", json!({})).unwrap();
        assert_eq!(temp(&mut s), 15);
        // The first window lets the rule settle on the heater; the room then warms.
        s.execute("sim.advance", json!({"seconds": 600})).unwrap();
        let warm = temp(&mut s);
        assert!(warm >= 18, "ten minutes of heating from 15: {warm}");
        s.execute("sim.advance", json!({"seconds": 1200})).unwrap();
        let t = temp(&mut s);
        assert!((18..=21).contains(&t), "it holds near the 19 degree threshold: {t}");
        // The same run again gives the same number: the model is deterministic.
        let mut again = Session::new();
        again.execute("file.new", json!({"sample": "iot-climate"})).unwrap();
        again.execute("iot.sync", json!({})).unwrap();
        again.execute("sim.advance", json!({"seconds": 600})).unwrap();
        again.execute("sim.advance", json!({"seconds": 1200})).unwrap();
        assert_eq!(temp(&mut again), t);
        // A hot afternoon: the air conditioner cools it back down.
        s.execute("iot.environment", json!({"temperature": 30})).unwrap();
        s.execute("sim.advance", json!({"seconds": 600})).unwrap();
        assert!(temp(&mut s) < 30);
        let page = s.execute("iot.configure", json!({"device": "Thermo1"})).unwrap();
        assert_eq!(page["value"].as_i64().unwrap(), temp(&mut s));
    }

    #[test]
    fn environment_command_validates_and_old_labs_keep_their_own_numbers() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "iot"})).unwrap();
        let e = s.execute("iot.environment", json!({})).unwrap();
        assert_eq!(e["enabled"], false);
        assert!(e["room"].is_null());
        for bad in [json!({"temperature": 500}), json!({"humidity": -1}), json!({"enabled": "yes"}), json!({"co": 2000}), json!({"motion": 2})] {
            assert!(s.execute("iot.environment", bad).is_err());
        }
        // Off: the sensor still reads its own number.
        let r = s.execute("iot.configure", json!({"device": "Sensor1", "value": 31})).unwrap();
        assert_eq!(r["value"], 31);
        // On: a sensor's new reading sets the room, and the room is what it reads.
        s.execute("iot.environment", json!({"enabled": true, "temperature": 22})).unwrap();
        assert_eq!(s.execute("iot.configure", json!({"device": "Sensor1"})).unwrap()["value"], 22);
        s.execute("iot.configure", json!({"device": "Sensor1", "value": 35})).unwrap();
        assert_eq!(temp(&mut s), 35);
        // The settings survive a save and load.
        let json = netlabcraft_model::file::to_json(&s.topology).unwrap();
        let back = netlabcraft_model::file::from_json(&json).unwrap();
        assert!(back.environment.enabled && back.environment.temperature == 22);
    }
}
