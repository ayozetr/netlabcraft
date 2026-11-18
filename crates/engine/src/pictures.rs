//! Picture commands: backgrounds of the logical workspace and of every place, the icon of a place,
//! its size in metres, and the list and removal of the pictures a lab holds (`place.scene.set`,
//! `place.background.set` and `.clear`, `place.icon.set` and `.clear`, `place.size.set` and
//! `.clear`, `workspace.background.set` and `.clear`, `image.list`, `image.remove`). The store and
//! its limits are in `netlabcraft_model::backdrop`, the reading of a picture in [`crate::images`].
//! Each command is one undo step; a programmatic call never opens a dialog, it takes the bytes of
//! a picture (`data`, base64), the id of one the lab already holds (`image`), or, on the desktop
//! and the command line, a file name (`path`).

use netlabcraft_model::Topology;
use netlabcraft_model::backdrop::{self, Background, BgMode, ImageUse, MAX_CUSTOM, Size, UseKind, VIEW_H, VIEW_W};
use serde_json::{Value, json};

use crate::images::{self, MAX_SOURCE_BYTES};
use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// The place a command is about: a number, a name, `"intercity"` (or 0), or, when left out, the
/// place being looked at (the Intercity map when none is).
fn place_param(s: &Session, p: &Value) -> Result<u32> {
    let t = &s.topology;
    let unknown = |what: &str| bad(format!("no place `{what}` (a place number, a name, or \"intercity\")"));
    match p.get("place") {
        None | Some(Value::Null) => Ok(s.place_view.unwrap_or(backdrop::INTERCITY)),
        Some(Value::Number(n)) => {
            let id = n.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| unknown(&n.to_string()))?;
            if id == backdrop::INTERCITY || t.place(id).is_some() { Ok(id) } else { Err(unknown(&n.to_string())) }
        }
        Some(Value::String(name)) => {
            let name = name.trim();
            if name.eq_ignore_ascii_case("intercity") {
                return Ok(backdrop::INTERCITY);
            }
            if let Ok(id) = name.parse::<u32>()
                && t.place(id).is_some()
            {
                return Ok(id);
            }
            t.places().iter().find(|x| x.name.eq_ignore_ascii_case(name)).map(|x| x.id).ok_or_else(|| unknown(name))
        }
        Some(_) => Err(bad("`place` is a place number, a name, or \"intercity\"")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_picture(path: &str) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Failed(format!("{path}: {e}")))?;
    if meta.len() > MAX_SOURCE_BYTES as u64 {
        return Err(EngineError::Failed(format!("{path}: too large to open as a picture")));
    }
    std::fs::read(path).map_err(|e| EngineError::Failed(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn read_picture(_: &str) -> Result<Vec<u8>> {
    Err(EngineError::Failed("the browser build takes the bytes of the picture in `data`".into()))
}

/// A picture the lab holds now.
struct Got {
    id: String,
    scaled_from: Option<(u32, u32)>,
}

/// The picture a spec names: `image` (one the lab holds), `data` (base64) or `path`; a new one is
/// checked, made to fit the limits and stored. `None` when the spec names no picture.
fn picture(t: &mut Topology, spec: &Value) -> Result<Option<Got>> {
    if let Some(v) = spec.get("image").filter(|v| !v.is_null()) {
        let id = v.as_str().ok_or_else(|| bad("`image` is the id of a picture the lab holds"))?;
        t.images.get(id).ok_or_else(|| EngineError::Failed(format!("the lab holds no picture `{id}`")))?;
        return Ok(Some(Got { id: id.to_string(), scaled_from: None }));
    }
    let bytes = if let Some(v) = spec.get("data").filter(|v| !v.is_null()) {
        let text = v.as_str().ok_or_else(|| bad("`data` is the picture's bytes in base64"))?;
        backdrop::base64_decode(text, MAX_SOURCE_BYTES).ok_or_else(|| bad("`data` is not base64, or is too large to be a picture"))?
    } else if let Some(v) = spec.get("path").filter(|v| !v.is_null()) {
        read_picture(v.as_str().ok_or_else(|| bad("`path` is a file name"))?)?
    } else {
        return Ok(None);
    };
    let made = images::prepare(&bytes).map_err(EngineError::Failed)?;
    let id = t.images.add(made.kind, made.w, made.h, made.data)?;
    Ok(Some(Got { id, scaled_from: made.scaled_from }))
}

fn custom_param(spec: &Value) -> Result<Option<u32>> {
    match spec.get("custom") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (1..=MAX_CUSTOM).contains(n))
            .map(Some)
            .ok_or_else(|| bad(format!("`custom` is a width or height of 1 to {MAX_CUSTOM}"))),
    }
}

/// The background a spec describes: a new picture or the one already there, and the layout.
fn background(t: &mut Topology, spec: &Value, current: Option<Background>) -> Result<(Background, Option<(u32, u32)>)> {
    let got = picture(t, spec)?;
    let scaled = got.as_ref().and_then(|g| g.scaled_from);
    let image = match (got, &current) {
        (Some(g), _) => g.id,
        (None, Some(c)) => c.image.clone(),
        (None, None) => return Err(bad("give the picture: `data` (base64), `path` or `image`")),
    };
    let mode = match spec.get("mode") {
        None | Some(Value::Null) => current.as_ref().map_or(BgMode::Original, |c| c.mode),
        Some(v) => {
            let name = v.as_str().unwrap_or_default();
            BgMode::from_id(name).ok_or_else(|| bad(format!("mode `{name}`: original, tiled, fit, width or height")))?
        }
    };
    let custom = custom_param(spec)?.or_else(|| current.as_ref().map(|c| c.custom).filter(|c| *c > 0));
    let custom = if matches!(mode, BgMode::Width | BgMode::Height) {
        custom.ok_or_else(|| bad("a `width` or `height` mode needs `custom`: the size in workspace units"))?
    } else {
        0
    };
    Ok((Background { image, mode, custom }, scaled))
}

fn number(spec: &Value, key: &str) -> Result<Option<f32>> {
    match spec.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(|n| Some(n as f32)).ok_or_else(|| bad(format!("`{key}` is a number"))),
    }
}

/// How big the background of a place is drawn, in workspace units (the map area when it has none).
fn shown(t: &Topology, place: u32) -> (f32, f32) {
    t.scene(place)
        .and_then(|s| s.background.as_ref())
        .and_then(|b| t.images.get(&b.image).map(|i| b.shown_size(i.w, i.h)))
        .unwrap_or((VIEW_W, VIEW_H))
}

/// The size a spec describes. Without `scale` it follows from the width and the size of the
/// background as drawn; without `length` the picture's proportions give it.
fn size(t: &Topology, place: u32, spec: &Value) -> Result<Size> {
    let old = t.scene(place).and_then(|s| s.size);
    let width = number(spec, "width")?;
    let (sw, sh) = shown(t, place);
    let w = width.or(old.map(|z| z.width)).ok_or_else(|| bad("`width` (metres) is required"))?;
    let length = number(spec, "length")?.or(if width.is_some() { None } else { old.map(|z| z.length) }).unwrap_or(w * sh / sw);
    let height = number(spec, "height")?.or(old.map(|z| z.height)).unwrap_or(3.0);
    let scale = number(spec, "scale")?.unwrap_or(match old {
        Some(z) if width.is_none() => z.scale,
        _ => w / sw,
    });
    Ok(Size { width: w, length, height, scale })
}

fn background_json(t: &Topology, b: &Background) -> Value {
    let (iw, ih) = t.images.get(&b.image).map_or((0, 0), |i| (i.w, i.h));
    let (sw, sh) = b.shown_size(iw, ih);
    json!({"image": b.image, "mode": b.mode.id(), "custom": b.custom, "pixels": [iw, ih], "shown": [sw, sh]})
}

fn scene_json(t: &Topology, place: u32) -> Value {
    let scene = t.scene(place);
    let size =
        scene.and_then(|s| s.size).map(|z| json!({"width": z.width, "length": z.length, "height": z.height, "volume": z.volume(), "scale": z.scale}));
    json!({
        "place": place,
        "background": scene.and_then(|s| s.background.as_ref()).map(|b| background_json(t, b)),
        "icon": scene.and_then(|s| s.icon.clone()),
        "size": size,
        "scale": t.map_scale(place),
    })
}

fn commit(s: &mut Session, t: Topology) {
    s.checkpoint();
    s.topology = t;
    s.changed();
}

fn with_scaled(mut v: Value, scaled: Option<(u32, u32)>) -> Value {
    if let (Some((w, h)), Some(o)) = (scaled, v.as_object_mut()) {
        o.insert("scaled_from".into(), json!([w, h]));
    }
    v
}

/// What the combined command does to one part: nothing (absent), clears it (null), or sets it.
fn part<'a>(p: &'a Value, key: &str) -> Option<&'a Value> {
    p.get(key)
}

fn set_icon(t: &mut Topology, place: u32, spec: &Value) -> Result<Option<(u32, u32)>> {
    let got = picture(t, spec)?.ok_or_else(|| bad("give the picture: `data` (base64), `path` or `image`"))?;
    t.set_place_icon(place, Some(got.id))?;
    Ok(got.scaled_from)
}

fn scene_set(s: &mut Session, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    let mut t = s.topology.clone();
    let mut scaled = None;
    for key in ["background", "icon"] {
        match part(p, key) {
            None => {}
            Some(Value::Null) => {
                if key == "background" {
                    t.set_place_background(place, None)?;
                } else {
                    t.set_place_icon(place, None)?;
                }
            }
            Some(spec) => {
                if !spec.is_object() {
                    return Err(bad(format!("`{key}` is an object (`data`, `path` or `image`, and the layout) or null")));
                }
                let sc = if key == "background" {
                    let cur = t.scene(place).and_then(|x| x.background.clone());
                    let (bg, sc) = background(&mut t, spec, cur)?;
                    t.set_place_background(place, Some(bg))?;
                    sc
                } else {
                    set_icon(&mut t, place, spec)?
                };
                scaled = scaled.or(sc);
            }
        }
    }
    match part(p, "size") {
        None => {}
        Some(Value::Null) => t.set_place_size(place, None)?,
        Some(spec) => {
            let z = size(&t, place, spec)?;
            t.set_place_size(place, Some(z))?;
        }
    }
    commit(s, t);
    Ok(with_scaled(scene_json(&s.topology, place), scaled))
}

fn background_set(s: &mut Session, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    let mut t = s.topology.clone();
    let cur = t.scene(place).and_then(|x| x.background.clone());
    let (bg, scaled) = background(&mut t, p, cur)?;
    t.set_place_background(place, Some(bg))?;
    commit(s, t);
    Ok(with_scaled(scene_json(&s.topology, place), scaled))
}

fn background_clear(s: &mut Session, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    let mut t = s.topology.clone();
    t.set_place_background(place, None)?;
    commit(s, t);
    Ok(scene_json(&s.topology, place))
}

fn icon_set(s: &mut Session, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    let mut t = s.topology.clone();
    let scaled = set_icon(&mut t, place, p)?;
    commit(s, t);
    Ok(with_scaled(scene_json(&s.topology, place), scaled))
}

fn icon_clear(s: &mut Session, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    let mut t = s.topology.clone();
    t.set_place_icon(place, None)?;
    commit(s, t);
    Ok(scene_json(&s.topology, place))
}

fn size_set(s: &mut Session, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    let mut t = s.topology.clone();
    let z = size(&t, place, p)?;
    t.set_place_size(place, Some(z))?;
    commit(s, t);
    Ok(scene_json(&s.topology, place))
}

fn size_clear(s: &mut Session, p: &Value) -> Result<Value> {
    let place = place_param(s, p)?;
    let mut t = s.topology.clone();
    t.set_place_size(place, None)?;
    commit(s, t);
    Ok(scene_json(&s.topology, place))
}

fn workspace_set(s: &mut Session, p: &Value) -> Result<Value> {
    let mut t = s.topology.clone();
    let cur = t.background.clone();
    let (bg, scaled) = background(&mut t, p, cur)?;
    t.set_logical_background(Some(bg))?;
    commit(s, t);
    Ok(with_scaled(json!({"background": s.topology.background.as_ref().map(|b| background_json(&s.topology, b))}), scaled))
}

fn workspace_clear(s: &mut Session) -> Result<Value> {
    let mut t = s.topology.clone();
    t.set_logical_background(None)?;
    commit(s, t);
    Ok(json!({"background": null}))
}

fn use_json(t: &Topology, u: &ImageUse) -> Value {
    let place = if u.kind == UseKind::Logical { None } else { Some(u.place) };
    let name = match place {
        Some(0) => "Intercity".to_string(),
        Some(id) => t.place(id).map(|x| x.name).unwrap_or_default(),
        None => String::new(),
    };
    json!({"kind": u.kind.id(), "place": place, "place_name": name, "image": u.image})
}

fn list(s: &Session) -> Value {
    let t = &s.topology;
    let images: Vec<Value> =
        t.images.0.iter().map(|(id, i)| json!({"id": id, "kind": i.kind.id(), "w": i.w, "h": i.h, "bytes": i.data.len()})).collect();
    let uses: Vec<Value> = t.image_uses().iter().map(|u| use_json(t, u)).collect();
    json!({
        "images": images,
        "uses": uses,
        "total_bytes": t.images.total_bytes(),
        "limits": {
            "images": backdrop::MAX_IMAGES,
            "bytes_each": backdrop::MAX_IMAGE_BYTES,
            "bytes_total": backdrop::MAX_LAB_IMAGE_BYTES,
            "side": backdrop::MAX_IMAGE_SIDE,
        },
    })
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let name = p.get("kind").and_then(Value::as_str).ok_or_else(|| bad("`kind` is required: logical, background or icon"))?;
    let kind = UseKind::from_id(name).ok_or_else(|| bad(format!("kind `{name}`: logical, background or icon")))?;
    let place = if kind == UseKind::Logical { backdrop::INTERCITY } else { place_param(s, p)? };
    if !s.topology.image_uses().iter().any(|u| u.kind == kind && u.place == place) {
        return Err(EngineError::Failed("no such picture is in use there".into()));
    }
    let mut t = s.topology.clone();
    t.clear_image_use(kind, place)?;
    commit(s, t);
    Ok(list(s))
}

pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    Some(match id {
        "place.scene.set" => scene_set(s, p),
        "place.background.set" => background_set(s, p),
        "place.background.clear" => background_clear(s, p),
        "place.icon.set" => icon_set(s, p),
        "place.icon.clear" => icon_clear(s, p),
        "place.size.set" => size_set(s, p),
        "place.size.clear" => size_clear(s, p),
        "workspace.background.set" => workspace_set(s, p),
        "workspace.background.clear" => workspace_clear(s),
        "image.list" => Ok(list(s)),
        "image.remove" => remove(s, p),
        _ => return None,
    })
}
