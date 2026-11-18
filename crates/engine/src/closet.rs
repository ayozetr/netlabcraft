//! Wiring-closet commands: furniture (racks, shelves, tables), the passive devices on them (patch
//! panels, wall mounts and power distribution devices), patch wires and power plugs. The model
//! lives in `netlabcraft_model::furniture`; every change is one undo step.

use netlabcraft_model::furniture::{FittingKind, FurnitureKind, Item, Pole};
use netlabcraft_model::physical::PlaceKind;
use netlabcraft_model::{End, Topology};
use serde_json::{Value, json};

use crate::cmd::device_param;
use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| bad(format!("`{key}` (text) is required")))
}

fn float(p: &Value, key: &str) -> Option<f32> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(-1.0e6, 1.0e6) as f32)
}

fn count(p: &Value, key: &str) -> Result<Option<u32>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| bad(format!("`{key}` is a whole number"))),
    }
}

/// A 1-based number as the API gives it (rack unit, panel port, outlet) to the model's 0-based one.
fn one_based(p: &Value, key: &str) -> Result<Option<u32>> {
    match count(p, key)? {
        Some(0) => Err(bad(format!("`{key}` counts from 1"))),
        Some(n) => Ok(Some(n.saturating_sub(1))),
        None => Ok(None),
    }
}

fn transient(p: &Value) -> bool {
    p.get("transient").and_then(Value::as_bool).unwrap_or(false)
}

fn commit(s: &mut Session, t: Topology, transient: bool) {
    if !transient {
        s.checkpoint();
    }
    s.topology = t;
    s.changed();
}

/// A thing by number or name (`"#3"` too).
fn find_named<'a>(p: &Value, key: &str, what: &str, mut items: impl Iterator<Item = (u32, &'a str)>) -> Result<u32> {
    match p.get(key) {
        Some(Value::Number(n)) => {
            let id = n.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(format!("`{key}` is not a {what} number")))?;
            items.find(|(i, _)| *i == id).map(|x| x.0).ok_or_else(|| EngineError::Failed(format!("no {what} {id}")))
        }
        Some(Value::String(name)) => {
            let name = name.trim();
            let num = name.strip_prefix('#').and_then(|n| n.parse::<u32>().ok());
            items
                .find(|(i, n)| Some(*i) == num || n.eq_ignore_ascii_case(name))
                .map(|x| x.0)
                .ok_or_else(|| EngineError::Failed(format!("no {what} `{name}`")))
        }
        _ => Err(bad(format!("`{key}` ({what} name or number) is required"))),
    }
}

fn furniture_param(s: &Session, p: &Value, key: &str) -> Result<u32> {
    find_named(p, key, "piece of furniture", s.topology.physical.furniture.iter().map(|f| (f.id, f.name.as_str())))
}

fn passive_value(s: &Session, v: &Value, key: &str) -> Result<u32> {
    find_named(v, key, "passive device", s.topology.physical.fittings.iter().map(|f| (f.id, f.name.as_str())))
}

/// The wiring closet a command acts in: `place`, else the one being looked at, else the default.
fn closet_param(s: &Session, p: &Value) -> Result<u32> {
    let t = &s.topology;
    let id = match p.get("place") {
        None | Some(Value::Null) => {
            s.place_view.filter(|v| t.place(*v).is_some_and(|x| x.kind == PlaceKind::Closet)).unwrap_or_else(|| t.default_place())
        }
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad("`place` is a place number"))?,
    };
    match t.place(id) {
        Some(x) if x.kind == PlaceKind::Closet => Ok(id),
        Some(_) => Err(bad("furniture and passive devices stand in a wiring closet")),
        None => Err(EngineError::Failed(format!("no place {id}"))),
    }
}

/// `device` or `passive`: what stands on furniture.
fn item_param(s: &Session, p: &Value) -> Result<Item> {
    if p.get("device").is_some() {
        Ok(Item::Device(device_param(s, p, "device")?))
    } else if p.get("passive").is_some() {
        Ok(Item::Fitting(passive_value(s, p, "passive")?))
    } else {
        Err(bad("`device` or `passive` (a name or number) is required"))
    }
}

/// A wire end: `{"panel": name, "port": 1-based}` or `{"device": name, "port": "Fa0/1"}`.
fn pole_param(s: &Session, p: &Value, key: &str) -> Result<Pole> {
    let v = p.get(key).ok_or_else(|| bad(format!("`{key}` (a wire end) is required")))?;
    if v.get("panel").is_some() {
        let fitting = passive_value(s, v, "panel")?;
        let port = one_based(v, "port")?.ok_or_else(|| bad("`port` (from 1) is required"))?;
        Ok(Pole::Panel { fitting, port })
    } else if v.get("device").is_some() {
        let d = device_param(s, v, "device")?;
        let name = text(v, "port")?;
        let port =
            s.topology.device(d).and_then(|x| x.port_index(name)).ok_or_else(|| EngineError::Failed(format!("no port {name} on that device")))?;
        Ok(Pole::Device(End { device: d, port }))
    } else {
        Err(bad(r#"a wire end is {"panel", "port"} or {"device", "port"}"#))
    }
}

fn item_json(t: &Topology, item: Item) -> Value {
    match item {
        Item::Device(d) => json!({"device": t.device(d).map(|x| x.name.clone())}),
        Item::Fitting(i) => json!({"passive": t.fitting(i).map(|x| x.name.clone())}),
    }
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    Some(match id {
        "closet.list" => list(s, p),
        "furniture.add" => furniture_add(s, p),
        "furniture.rename" | "furniture.move" | "furniture.remove" | "furniture.resize" => furniture_edit(s, id, p),
        "furniture.mount" => mount(s, p),
        "furniture.unmount" => unmount(s, p),
        "passive.add" => passive_add(s, p),
        "passive.rename" | "passive.remove" => passive_edit(s, id, p),
        "patch.wire" => wire(s, p),
        "patch.unwire" => unwire(s, p),
        "power.plug" | "power.unplug" => plug(s, id, p),
        "power.strip" => strip(s, p),
        _ => return None,
    })
}

fn list(s: &Session, p: &Value) -> Result<Value> {
    let place = closet_param(s, p)?;
    let t = &s.topology;
    let furniture: Vec<Value> = t
        .furniture_in(place)
        .iter()
        .map(|f| {
            let items: Vec<Value> = t
                .mounts_on(f.id)
                .iter()
                .map(|m| {
                    let mut v = item_json(t, m.item);
                    v["slot"] = json!(m.slot.saturating_add(1));
                    v["units"] = json!(m.units);
                    v
                })
                .collect();
            json!({"id": f.id, "kind": f.kind.id(), "name": f.name, "size": f.size, "items": items})
        })
        .collect();
    let passive: Vec<Value> = t
        .fittings_in(place)
        .iter()
        .map(|f| {
            let on = t.mount_of(Item::Fitting(f.id));
            let plugged: Vec<Value> = t
                .plugs_in(f.id)
                .iter()
                .map(|g| json!({"device": t.device(g.device).map(|d| d.name.clone()), "outlet": g.outlet.saturating_add(1)}))
                .collect();
            json!({
                "id": f.id, "kind": f.kind.id(), "name": f.name, "ports": f.ports, "on": f.on,
                "furniture": on.map(|m| m.furniture), "slot": on.map(|m| m.slot.saturating_add(1)), "plugged": plugged,
            })
        })
        .collect();
    let loose: Vec<Value> = t.closet_layout(place).items.iter().filter(|i| i.furniture.is_none()).map(|i| t.item_name(i.item).into()).collect();
    let wires: Vec<Value> = t
        .physical
        .patch_wires
        .iter()
        .filter(|w| t.pole_place(w.a) == Some(place))
        .map(|w| json!({"a": t.pole_label(w.a), "b": t.pole_label(w.b)}))
        .collect();
    let paths: Vec<Value> = t
        .patch_paths()
        .iter()
        .filter(|x| t.home_of(x.a.device).map(|h| h.place) == Some(place))
        .map(|x| json!({"a": t.end_label(x.a), "b": t.end_label(x.b)}))
        .collect();
    Ok(json!({"place": place, "furniture": furniture, "passive": passive, "loose": loose, "wires": wires, "paths": paths}))
}

fn furniture_add(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = text(p, "kind")?;
    let kind = FurnitureKind::from_id(kind).ok_or_else(|| bad(format!("unknown furniture `{kind}` (rack, shelf, table)")))?;
    let place = closet_param(s, p)?;
    let (dx, dy) = s.topology.furniture_spot(place);
    let (x, y) = (float(p, "x").unwrap_or(dx), float(p, "y").unwrap_or(dy));
    let mut t = s.topology.clone();
    let id = t.add_furniture(kind, p.get("name").and_then(Value::as_str), place, x, y, count(p, "size")?)?;
    commit(s, t, false);
    let f = s.topology.furniture_piece(id).cloned();
    Ok(json!({"id": id, "name": f.as_ref().map(|f| f.name.clone()), "kind": kind.id(), "size": f.map(|f| f.size), "place": place}))
}

fn furniture_edit(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let id = furniture_param(s, p, "furniture")?;
    let mut t = s.topology.clone();
    let mut quick = false;
    match cmd {
        "furniture.rename" => t.rename_furniture(id, text(p, "name")?)?,
        "furniture.resize" => {
            let size = count(p, "size")?.ok_or_else(|| bad("`size` (rack units or shelf levels) is required"))?;
            t.resize_furniture(id, size)?;
        }
        "furniture.move" => {
            let (x, y) = (float(p, "x").ok_or_else(|| bad("`x` is required"))?, float(p, "y").ok_or_else(|| bad("`y` is required"))?);
            t.move_furniture(id, x, y)?;
            quick = transient(p);
        }
        _ => {
            t.remove_furniture(id)?;
        }
    }
    commit(s, t, quick);
    Ok(Value::Null)
}

fn mount(s: &mut Session, p: &Value) -> Result<Value> {
    let item = item_param(s, p)?;
    let furniture = furniture_param(s, p, "furniture")?;
    let mut t = s.topology.clone();
    let m = t.mount_item(item, furniture, one_based(p, "slot")?, count(p, "units")?)?;
    commit(s, t, false);
    Ok(json!({"furniture": furniture, "slot": m.slot.saturating_add(1), "units": m.units}))
}

fn unmount(s: &mut Session, p: &Value) -> Result<Value> {
    let item = item_param(s, p)?;
    let mut t = s.topology.clone();
    if !t.unmount_item(item) {
        return Err(EngineError::Failed(format!("{} is not on furniture", t.item_name(item))));
    }
    commit(s, t, false);
    Ok(Value::Null)
}

fn passive_add(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = text(p, "kind")?;
    let kind = FittingKind::from_id(kind).ok_or_else(|| {
        let all: Vec<&str> = FittingKind::ALL.iter().map(|k| k.id()).collect();
        bad(format!("unknown passive device `{kind}` ({})", all.join(", ")))
    })?;
    let furniture = match p.get("furniture") {
        None | Some(Value::Null) => None,
        Some(_) => Some(furniture_param(s, p, "furniture")?),
    };
    let place = match furniture.and_then(|f| s.topology.furniture_piece(f)) {
        Some(f) => f.place,
        None => closet_param(s, p)?,
    };
    let mut t = s.topology.clone();
    let id = t.add_fitting(kind, p.get("name").and_then(Value::as_str), place, 0.0, 0.0, count(p, "ports")?)?;
    if let Some(f) = furniture {
        t.mount_item(Item::Fitting(id), f, one_based(p, "slot")?, count(p, "units")?)?;
    }
    commit(s, t, false);
    let f = s.topology.fitting(id).cloned();
    Ok(json!({"id": id, "name": f.as_ref().map(|f| f.name.clone()), "kind": kind.id(), "ports": f.map(|f| f.ports), "place": place}))
}

fn passive_edit(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let id = passive_value(s, p, "passive")?;
    let mut t = s.topology.clone();
    if cmd == "passive.rename" {
        t.rename_fitting(id, text(p, "name")?)?;
    } else {
        t.remove_fitting(id)?;
    }
    commit(s, t, false);
    Ok(Value::Null)
}

fn wire(s: &mut Session, p: &Value) -> Result<Value> {
    let (a, b) = (pole_param(s, p, "a")?, pole_param(s, p, "b")?);
    let mut t = s.topology.clone();
    let made = t.add_patch_wire(a, b)?;
    let cables: Vec<String> = made.iter().map(|e| t.end_label(*e)).collect();
    commit(s, t, false);
    Ok(json!({"cables": cables}))
}

fn unwire(s: &mut Session, p: &Value) -> Result<Value> {
    let a = pole_param(s, p, "a")?;
    let b = match p.get("b") {
        None | Some(Value::Null) => None,
        Some(_) => Some(pole_param(s, p, "b")?),
    };
    let mut t = s.topology.clone();
    if !t.remove_patch_wire(a, b) {
        return Err(EngineError::Failed(format!("no patch wire at {}", t.pole_label(a))));
    }
    commit(s, t, false);
    Ok(Value::Null)
}

fn plug(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let d = device_param(s, p, "device")?;
    let mut t = s.topology.clone();
    let mut out = Value::Null;
    if cmd == "power.unplug" {
        if !t.unplug(d) {
            return Err(EngineError::Failed(format!("{} is not plugged in", t.item_name(Item::Device(d)))));
        }
    } else {
        let strip = passive_value(s, p, "strip")?;
        let g = t.plug_in(d, strip, one_based(p, "outlet")?)?;
        out = json!({"strip": strip, "outlet": g.outlet.saturating_add(1)});
    }
    commit(s, t, false);
    // Plugged into a strip that is off: no power.
    if s.topology.cut_by_strip(d) {
        crate::hardware::set_power(s, d, false);
        s.changed();
    }
    Ok(out)
}

fn strip(s: &mut Session, p: &Value) -> Result<Value> {
    let id = passive_value(s, p, "strip")?;
    let on = match p.get("on") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(v)) if v.eq_ignore_ascii_case("on") => true,
        Some(Value::String(v)) if v.eq_ignore_ascii_case("off") => false,
        _ => return Err(bad("`on` (true/false, or \"on\"/\"off\") is required")),
    };
    let mut t = s.topology.clone();
    let devices = t.set_strip_power(id, on)?;
    commit(s, t, false);
    for d in &devices {
        crate::hardware::set_power(s, *d, on);
    }
    s.changed();
    Ok(json!({"on": on, "devices": devices.iter().filter_map(|d| s.topology.device(*d).map(|x| x.name.clone())).collect::<Vec<_>>()}))
}
