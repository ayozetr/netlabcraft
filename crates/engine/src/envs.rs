//! The Environments commands: what each place's surroundings are (`environment.get`, `.set`,
//! `.clear`, `.keyframe.add`, `.keyframe.remove`, `.clock`, `.export`, `.import`). The model is
//! in `netlabcraft_model::environments`; the smart sensors read it through the simulator.

use netlabcraft_model::environments::{self as env, DAY_SECONDS, INTERCITY, Source, VARS, Var};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// The place a command is about: a number, a name, `"intercity"` (or 0), or, when left out, the
/// lab's default place (the first wiring closet).
fn place_param(s: &Session, p: &Value) -> Result<u32> {
    let t = &s.topology;
    let unknown = |what: &str| bad(format!("no place `{what}` (a place number, a name, or \"intercity\")"));
    match p.get("place") {
        None | Some(Value::Null) => Ok(t.default_place()),
        Some(Value::Number(n)) => {
            n.as_u64().and_then(|v| u32::try_from(v).ok()).filter(|id| t.env_place_exists(*id)).ok_or_else(|| unknown(&n.to_string()))
        }
        Some(Value::String(name)) => {
            let name = name.trim();
            if name.eq_ignore_ascii_case("intercity") {
                return Ok(INTERCITY);
            }
            if let Ok(id) = name.parse::<u32>()
                && t.env_place_exists(id)
            {
                return Ok(id);
            }
            t.places().iter().find(|x| x.name.eq_ignore_ascii_case(name)).map(|x| x.id).ok_or_else(|| unknown(name))
        }
        Some(_) => Err(bad("`place` is a place number, a name, or \"intercity\"")),
    }
}

fn variable_param(p: &Value) -> Result<&'static Var> {
    let name =
        p.get("variable").and_then(Value::as_str).ok_or_else(|| bad("`variable` (a variable id such as \"ambient_temperature\") is required"))?;
    env::find_var(name).ok_or_else(|| bad(format!("unknown variable `{name}`")))
}

fn number_param(p: &Value, key: &str) -> Result<f64> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(format!("`{key}` is a number")))
}

/// A time of day: `"HH:MM"`, `"HH:MM:SS"` or seconds from midnight.
fn time_value(v: &Value, key: &str) -> Result<u32> {
    let t = match v {
        Value::String(s) => env::parse_time(s),
        Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n < DAY_SECONDS),
        _ => None,
    };
    t.ok_or_else(|| bad(format!("`{key}` is a time of day: \"HH:MM:SS\" or seconds from 0 to {}", DAY_SECONDS - 1)))
}

fn place_json(s: &Session, place: u32) -> Value {
    if place == INTERCITY {
        return json!({"id": 0, "name": "Intercity", "kind": "intercity"});
    }
    match s.topology.place(place) {
        Some(x) => json!({"id": x.id, "name": x.name, "kind": x.kind.id()}),
        None => json!({"id": place, "name": "", "kind": null}),
    }
}

/// The environment of a place as the window shows it, at the clock's time now.
fn view(s: &mut Session, place: u32) -> Value {
    if s.topology.environment.enabled {
        s.sim.env_step(&s.topology);
    }
    let t = &s.topology;
    let time = s.sim.env_time(t);
    let own = t.place_env(place);
    let room = s.sim.environment();
    let mut vars = Vec::new();
    for v in VARS {
        let (mut value, source) = t.env_value(place, v.id, time).unwrap_or((v.default, Source::Default));
        let (mut source_name, mut from) = match source {
            Source::Own => ("own", Some(place)),
            Source::Inherited(p) => ("inherited", Some(p)),
            Source::Default => ("default", None),
        };
        // Nothing sets it: the smart-home room answers for what its sensors read, while it runs.
        if source == Source::Default
            && let Some(c) = room
            && let Some(r) = v.drives.and_then(|w| c.reading(w))
        {
            value = v.from_reading(r);
            source_name = "room";
            from = None;
        }
        let keys: Vec<Value> = own
            .and_then(|e| e.setting(v.id))
            .map(|x| x.keys.iter().map(|k| json!({"time": env::format_time(k.t), "seconds": k.t, "value": k.v})).collect())
            .unwrap_or_default();
        vars.push(json!({
            "id": v.id, "label": v.label, "group": v.group, "unit": v.unit,
            "value": value, "source": source_name, "from": from,
            "default": v.default, "min": v.min, "max": v.max, "drives": v.drives, "keys": keys,
        }));
    }
    let path: Vec<Value> = t.env_chain(place).into_iter().rev().map(|p| place_json(s, p)).collect();
    let c = t.environments.clock;
    json!({
        "place": place_json(s, place),
        "path": path,
        "time": env::format_time(time),
        "seconds": time,
        "clock": {"paused": c.paused(), "real_minutes": c.real_minutes, "sim_minutes": c.sim_minutes},
        "notes": own.map(|e| e.notes.clone()).unwrap_or_default(),
        "show_notes": own.is_some_and(|e| e.show_notes),
        "variables": vars,
    })
}

/// After a change: the lab changed, and the sensors report what they read now.
fn done(s: &mut Session, t: netlabcraft_model::Topology) {
    s.checkpoint();
    s.topology = t;
    s.changed();
    s.sim.iot_sync(&s.topology);
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    match cmd {
        "environment.get" => Ok(view(s, place)),
        "environment.set" => {
            let notes = match p.get("notes") {
                None => None,
                Some(Value::String(x)) => Some(x.as_str()),
                Some(_) => return Err(bad("`notes` is text")),
            };
            let show = match p.get("show_notes") {
                None => None,
                Some(Value::Bool(b)) => Some(*b),
                Some(_) => return Err(bad("`show_notes` is true or false")),
            };
            let variable = p.get("variable").map(|_| variable_param(p)).transpose()?;
            if variable.is_none() && notes.is_none() && show.is_none() {
                return Err(bad("give a `variable` and its `value`, or `notes`, or `show_notes`"));
            }
            let mut t = s.topology.clone();
            if let Some(v) = variable {
                t.env_set(place, v.id, number_param(p, "value")?)?;
            }
            if notes.is_some() || show.is_some() {
                t.env_notes(place, notes, show)?;
            }
            done(s, t);
            Ok(view(s, place))
        }
        "environment.clear" => {
            let variable = p.get("variable").map(|_| variable_param(p)).transpose()?;
            let mut t = s.topology.clone();
            t.env_clear(place, variable.map(|v| v.id))?;
            done(s, t);
            Ok(view(s, place))
        }
        "environment.keyframe.add" | "environment.keyframe.remove" => {
            let v = variable_param(p)?;
            let time = time_value(p.get("time").ok_or_else(|| bad("`time` is required"))?, "time")?;
            let mut t = s.topology.clone();
            if cmd.ends_with(".add") {
                t.env_add_key(place, v.id, time, number_param(p, "value")?)?;
            } else {
                t.env_remove_key(place, v.id, time)?;
            }
            done(s, t);
            Ok(view(s, place))
        }
        "environment.clock" => {
            let now = s.sim.now_us();
            let mut t = s.topology.clone();
            let c = &mut t.environments.clock;
            let mut touched = false;
            let scale = |key: &str, current: u32| -> Result<u32> {
                match p.get(key) {
                    None => Ok(current),
                    Some(v) => v
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .filter(|n| (1..=env::MAX_SCALE).contains(n))
                        .ok_or_else(|| bad(format!("`{key}` is a whole number of minutes from 1 to {}", env::MAX_SCALE))),
                }
            };
            let (real, sim) = (scale("real_minutes", c.real_minutes)?, scale("sim_minutes", c.sim_minutes)?);
            if p.get("real_minutes").is_some() || p.get("sim_minutes").is_some() {
                c.set_scale(now, real, sim);
                touched = true;
            }
            if let Some(v) = p.get("time") {
                c.set_time(now, time_value(v, "time")?);
                touched = true;
            }
            match p.get("paused") {
                None => {}
                Some(Value::Bool(b)) => {
                    c.set_paused(now, *b);
                    touched = true;
                }
                Some(_) => return Err(bad("`paused` is true or false")),
            }
            if touched {
                done(s, t);
            }
            Ok(view(s, place))
        }
        "environment.export" => {
            let text = s.topology.env_export(place)?;
            let name: String = place_json(s, place)["name"]
                .as_str()
                .unwrap_or("environment")
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
                .collect();
            Ok(json!({"place": place_json(s, place), "file": format!("{name}.environment.json"), "text": text}))
        }
        "environment.import" => {
            let text = p.get("text").and_then(Value::as_str).ok_or_else(|| bad("`text` (the contents of an exported environment) is required"))?;
            let merge = match p.get("merge") {
                None => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(bad("`merge` is true or false")),
            };
            let mut t = s.topology.clone();
            let (read, skipped) = t.env_import(place, text, merge)?;
            done(s, t);
            let mut v = view(s, place);
            if let Some(o) = v.as_object_mut() {
                o.insert("imported".into(), json!({"variables": read, "skipped": skipped}));
            }
            Ok(v)
        }
        _ => Err(EngineError::UnknownCommand(cmd.to_string())),
    }
}
