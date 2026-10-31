//! Programmable boards: the program and its run (`board.program`, `board.status`) and the IoT
//! custom cables from pins to smart things (`board.wire`, and `link.add` with
//! `"cable": "iot-custom"`).

use netlabcraft_core::DeviceId;
use netlabcraft_model::board::{MAX_PROGRAM, analog, find_pin, is_board, pins};
use netlabcraft_model::iot::{is_sensor, thing};
use netlabcraft_sim::script::{RunState, Val};
use serde_json::{Map, Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// What a pin reads now (as the program would), without running anything.
fn level(s: &Session, dev: &netlabcraft_model::Device, pin: &str) -> i64 {
    let run = s.sim.board(dev.id);
    let wired = dev.board.wires.iter().find(|w| w.pin == pin).and_then(|w| s.topology.device(w.thing));
    match wired {
        Some(d) if !is_sensor(d) => i64::from(s.sim.iot_thing(d.id).is_some_and(|x| x.on)),
        Some(d) => {
            let v = i64::from(d.iot.value);
            if analog(pin) { v } else { i64::from(v > 0) }
        }
        None => run.and_then(|r| r.levels.get(pin).copied()).unwrap_or(0),
    }
}

/// A board as its Programming tab shows it.
pub(crate) fn status(s: &Session, d: DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let pins: Vec<Value> = pins(dev.catalogue_model())
        .iter()
        .map(|p| {
            let wired = dev.board.wires.iter().find(|w| w.pin == *p).and_then(|w| s.topology.device(w.thing));
            json!({
                "pin": p,
                "analog": analog(p),
                "wired": wired.map(|x| x.name.clone()),
                "what": wired.and_then(thing).map(|t| t.what),
                "sensor": wired.map(is_sensor),
                "level": level(s, dev, p),
            })
        })
        .collect();
    let run = s.sim.board(d);
    let machine = run.and_then(|r| r.machine.as_ref());
    let state = match machine.map(|m| &m.state) {
        None => "not started",
        Some(RunState::Waiting(_)) => "running",
        Some(RunState::Stopped(_)) => "stopped",
    };
    let message = run.and_then(|r| r.message.clone()).or_else(|| match machine.map(|m| &m.state) {
        Some(RunState::Stopped(Some(why))) => Some(why.clone()),
        _ => None,
    });
    let mut vars = Map::new();
    for (k, v) in machine.map(|m| m.vars.clone()).unwrap_or_default() {
        vars.insert(
            k,
            match v {
                Val::Int(n) => json!(n),
                Val::Text(t) => json!(t),
            },
        );
    }
    json!({
        "device": dev.name,
        "model": dev.catalogue_model().id,
        "server": dev.iot.server.map(|a| a.to_string()),
        "program": dev.board.program,
        "run": dev.board.run,
        "state": state,
        "line": machine.map(|m| m.line),
        "wakes_at_s": machine.and_then(|m| m.due()).map(|us| us / 1_000_000),
        "message": message,
        "log": machine.map(|m| m.log.clone()).unwrap_or_default(),
        "vars": vars,
        "pins": pins,
    })
}

fn board_param(s: &Session, p: &Value) -> Result<DeviceId> {
    let d = crate::cmd::device_param(s, p, "device")?;
    if s.topology.device(d).is_none_or(|x| !is_board(x)) {
        return Err(bad("board commands are for programmable boards (Board SBC, Board MCU)"));
    }
    Ok(d)
}

/// `link.add` with an IoT custom cable: a board's pin (`a_port`/`b_port`, or the first free
/// pin: analog for a sensor, digital for a device) to a smart thing.
pub(crate) fn link_add(s: &mut Session, a: DeviceId, b: DeviceId, p: &Value) -> Result<Value> {
    let t = &s.topology;
    let (board, thing_id, pin) = match (t.device(a), t.device(b)) {
        (Some(x), Some(_)) if is_board(x) => (a, b, p.get("a_port").and_then(Value::as_str)),
        (Some(_), Some(y)) if is_board(y) => (b, a, p.get("b_port").and_then(Value::as_str)),
        (Some(x), Some(y)) => {
            return Err(EngineError::Failed(format!(
                "an IoT custom cable joins a programmable board's pin to a smart thing: neither {} nor {} is a board",
                x.name, y.name
            )));
        }
        _ => return Err(EngineError::Failed("no such device".into())),
    };
    wire(s, board, thing_id, pin)
}

fn wire(s: &mut Session, board: DeviceId, thing_id: DeviceId, pin: Option<&str>) -> Result<Value> {
    let t = &s.topology;
    let dev = t.device(board).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let pin = match pin {
        Some(pn) => {
            find_pin(dev, pn).ok_or_else(|| bad(format!("{} has no pin {pn} (it has {})", dev.name, pins(dev.catalogue_model()).join(", "))))?
        }
        None => {
            let want_analog = t.device(thing_id).is_some_and(is_sensor);
            let free = |x: &String| !dev.board.wires.iter().any(|w| w.pin == *x);
            let all = pins(dev.catalogue_model());
            all.iter()
                .find(|x| free(x) && analog(x) == want_analog)
                .or_else(|| all.iter().find(|x| free(x)))
                .cloned()
                .ok_or_else(|| EngineError::Failed(format!("{} has no free pin", dev.name)))?
        }
    };
    let mut next = s.topology.clone();
    next.wire_pin(board, &pin, Some(thing_id))?;
    s.checkpoint();
    s.topology = next;
    s.changed();
    let name = |id| s.topology.device(id).map_or(String::new(), |d| d.name.clone());
    Ok(json!({"a": format!("{} {pin}", name(board)), "b": name(thing_id), "cable": "iot-custom", "resolved": "iot-custom"}))
}

/// `link.remove` at a board's pin.
pub(crate) fn unwire(s: &mut Session, board: DeviceId, pin: &str) -> Result<Value> {
    let mut next = s.topology.clone();
    next.wire_pin(board, pin, None)?;
    s.checkpoint();
    s.topology = next;
    s.changed();
    Ok(Value::Null)
}

/// The IoT custom cables, as `link.list` lists them.
pub(crate) fn links(s: &Session) -> Vec<Value> {
    let t = &s.topology;
    t.devices
        .iter()
        .flat_map(|d| d.board.wires.iter().map(move |w| (d, w)))
        .map(|(d, w)| {
            let far = t.device(w.thing).map_or(String::new(), |x| x.name.clone());
            json!({"a": format!("{} {}", d.name, w.pin), "b": far, "cable": "iot-custom", "resolved": "iot-custom", "up": !t.is_dark(d.id), "problem": Value::Null})
        })
        .collect()
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    match cmd {
        "board.status" => {
            let d = board_param(s, p)?;
            Ok(status(s, d))
        }
        "board.wire" => {
            let d = board_param(s, p)?;
            let pin = p.get("pin").and_then(Value::as_str).ok_or_else(|| bad("`pin` (D0, A1…) is required"))?;
            match p.get("thing").and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()) {
                Some(name) => {
                    let t = s.topology.resolve(name)?;
                    wire(s, d, t, Some(pin))?;
                }
                None => {
                    unwire(s, d, pin)?;
                }
            }
            Ok(status(s, d))
        }
        "board.program" => {
            let d = board_param(s, p)?;
            let source = match p.get("source") {
                None => None,
                Some(Value::String(x)) if x.chars().count() > MAX_PROGRAM => {
                    return Err(bad(format!("a program is at most {MAX_PROGRAM} characters")));
                }
                Some(Value::String(x)) => {
                    netlabcraft_sim::script::compile(x).map_err(bad)?;
                    Some(x.clone())
                }
                Some(_) => return Err(bad("`source` is the program's text")),
            };
            let run = match p.get("run") {
                None => None,
                Some(Value::Bool(b)) => Some(*b),
                Some(_) => return Err(bad("`run` is true or false")),
            };
            let server = match p.get("server") {
                None => None,
                Some(Value::String(a)) if a.trim().is_empty() => Some(None),
                Some(Value::String(a)) => Some(Some(netlabcraft_core::parse_ip(a.trim()).map_err(|e| bad(e.to_string()))?)),
                Some(_) => return Err(bad("`server` is an address")),
            };
            if source.is_some() || run.is_some() || server.is_some() {
                s.checkpoint();
                if let Some(dev) = s.topology.device_mut(d) {
                    if let Some(src) = &source {
                        dev.board.program = src.clone();
                    }
                    if let Some(r) = run {
                        dev.board.run = r;
                    }
                    if let Some(a) = server {
                        dev.iot.server = a;
                    }
                }
                s.changed();
            }
            // A new program, or Run, starts from the top; Stop stops where it is.
            match run {
                Some(true) => {
                    let _ = s.sim.board_start(&s.topology, d);
                }
                Some(false) => s.sim.board_stop(d),
                None if source.is_some() => s.sim.board_reset(d),
                None => {}
            }
            s.revision += 1;
            Ok(status(s, d))
        }
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}
