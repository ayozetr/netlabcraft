// Backgrounds, place icons and place sizes: the commands and what they do to the lab.
use netlabcraft_model::backdrop::{INTERCITY, MAX_IMAGES, base64_encode};
use serde_json::{Value, json};

use crate::images::fixtures::{jpeg_with_metadata, png};
use crate::*;

fn b64(w: u32, h: u32) -> String {
    base64_encode(&png(w, h))
}

fn lab() -> (Session, u64) {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    let city = s.execute("place.add", json!({"kind": "city", "name": "Harbor"})).unwrap()["id"].as_u64().unwrap();
    (s, city)
}

#[test]
fn a_background_is_set_listed_undone_and_cleared() {
    let (mut s, city) = lab();
    let r = s.execute("place.background.set", json!({"place": city, "data": b64(60, 40), "mode": "tiled"})).unwrap();
    assert_eq!(r["background"]["mode"], "tiled");
    assert_eq!(r["background"]["pixels"], json!([60, 40]));
    assert_eq!(r["background"]["shown"], json!([60.0, 40.0]));
    let list = s.execute("image.list", json!({})).unwrap();
    assert_eq!(list["images"].as_array().unwrap().len(), 1);
    assert_eq!(list["uses"][0]["kind"], "background");
    assert_eq!(list["uses"][0]["place_name"], "Harbor");
    // Only the layout changes: the picture is kept.
    let r = s.execute("place.background.set", json!({"place": city, "mode": "width", "custom": 120})).unwrap();
    assert_eq!(r["background"]["shown"], json!([120.0, 80.0]));
    assert!(s.execute("place.background.set", json!({"place": city, "mode": "width"})).is_ok(), "the custom size stays");
    assert!(s.execute("place.background.set", json!({"place": city, "mode": "sideways"})).is_err());
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.scene(city as u32).unwrap().background.as_ref().unwrap().mode, netlabcraft_model::backdrop::BgMode::Tiled);
    s.execute("place.background.clear", json!({"place": city})).unwrap();
    assert!(s.topology.images.is_default() && s.topology.physical.scenes.is_empty());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.images.0.len(), 1, "undo brings the picture back");
}

#[test]
fn one_picture_used_in_three_places_is_stored_once() {
    let (mut s, city) = lab();
    let data = b64(32, 32);
    s.execute("place.background.set", json!({"place": city, "data": data})).unwrap();
    s.execute("place.icon.set", json!({"place": city, "data": data})).unwrap();
    s.execute("workspace.background.set", json!({"data": data, "mode": "tiled"})).unwrap();
    s.execute("place.background.set", json!({"place": "intercity", "image": s.topology.images.0.keys().next().unwrap().clone(), "mode": "fit"}))
        .unwrap();
    let list = s.execute("image.list", json!({})).unwrap();
    assert_eq!(list["images"].as_array().unwrap().len(), 1);
    assert_eq!(list["uses"].as_array().unwrap().len(), 4);
    // Removing one use keeps the picture; removing the last takes it away. One undo step each.
    s.execute("image.remove", json!({"kind": "logical"})).unwrap();
    assert_eq!(s.topology.images.0.len(), 1);
    s.execute("image.remove", json!({"kind": "icon", "place": city})).unwrap();
    s.execute("image.remove", json!({"kind": "background", "place": city})).unwrap();
    let list = s.execute("image.remove", json!({"kind": "background", "place": "intercity"})).unwrap();
    assert!(list["images"].as_array().unwrap().is_empty());
    assert!(s.execute("image.remove", json!({"kind": "icon", "place": city})).is_err(), "nothing left to remove");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.images.0.len(), 1);
    // A saved lab holds the picture once and opens again with all its uses.
    let json = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": json})).unwrap();
    assert_eq!(s2.topology, s.topology);
}

#[test]
fn the_size_of_a_place_gives_its_map_a_scale_that_changes_cable_lengths() {
    let (mut s, city) = lab();
    let b1 = s.execute("place.add", json!({"kind": "building", "name": "A", "parent": city, "x": 0, "y": 0})).unwrap()["id"].as_u64().unwrap();
    let b2 = s.execute("place.add", json!({"kind": "building", "name": "B", "parent": city, "x": 30, "y": 40})).unwrap()["id"].as_u64().unwrap();
    let c1 = s.execute("place.add", json!({"kind": "closet", "name": "K1", "parent": b1})).unwrap()["id"].as_u64().unwrap();
    let c2 = s.execute("place.add", json!({"kind": "closet", "name": "K2", "parent": b2})).unwrap()["id"].as_u64().unwrap();
    s.execute("device.add", json!({"model": "Switch 24FE", "name": "S1"})).unwrap();
    s.execute("device.add", json!({"model": "Switch 24FE", "name": "S2"})).unwrap();
    s.execute("device.place", json!({"device": "S1", "place": c1})).unwrap();
    s.execute("device.place", json!({"device": "S2", "place": c2})).unwrap();
    s.execute("link.add", json!({"a": "S1", "a_port": "fa0/1", "b": "S2", "b_port": "fa0/1"})).unwrap();
    let meters = |s: &mut Session| s.execute("place.list", json!({})).unwrap()["cables"][0]["meters"].as_u64().unwrap();
    assert_eq!(meters(&mut s), 50 + 30);
    // A map of the city: a 400-unit-wide picture standing for 4 km, so 10 m a unit.
    let r = s.execute("place.background.set", json!({"place": city, "data": b64(400, 300)})).unwrap();
    assert_eq!(r["size"], Value::Null);
    let r = s.execute("place.size.set", json!({"place": city, "width": 4000, "height": 100})).unwrap();
    assert_eq!(r["size"]["scale"], 10.0);
    assert_eq!(r["size"]["length"], 3000.0, "the picture's proportions");
    assert_eq!(r["size"]["volume"], 4000.0 * 3000.0 * 100.0);
    assert_eq!(r["scale"], 10.0);
    assert_eq!(meters(&mut s), 500 + 30, "50 units at 10 m, plus the walls");
    assert_eq!(s.topology.map_scale(city as u32), 10.0);
    s.execute("place.size.clear", json!({"place": city})).unwrap();
    assert_eq!(s.topology.map_scale(city as u32), 1.0);
    assert_eq!(meters(&mut s), 80);
    // A size must be sensible.
    for bad in [json!({"width": 0}), json!({"width": -3}), json!({"width": 5, "scale": 0}), json!({"width": "wide"}), json!({})] {
        let mut p = bad.clone();
        p["place"] = json!(city);
        assert!(s.execute("place.size.set", p).is_err(), "{bad}");
    }
}

#[test]
fn scene_set_is_one_undo_step_and_all_or_nothing() {
    let (mut s, city) = lab();
    let depth = |s: &Session| s.undo.len();
    let before = depth(&s);
    s.execute(
        "place.scene.set",
        json!({"place": city, "background": {"data": b64(50, 20), "mode": "fit"}, "icon": {"data": b64(16, 16)}, "size": {"width": 100, "height": 4}}),
    )
    .unwrap();
    assert_eq!(depth(&s), before + 1);
    let scene = s.topology.scene(city as u32).unwrap().clone();
    assert!(scene.background.is_some() && scene.icon.is_some());
    assert_eq!(scene.size.unwrap().scale, 0.1, "100 m over the 1000-unit map area");
    // A part that fails leaves the lab as it was.
    let snapshot = s.topology.clone();
    assert!(s.execute("place.scene.set", json!({"place": city, "icon": {"data": "not base64!"}, "background": null})).is_err());
    assert_eq!(s.topology, snapshot);
    assert_eq!(depth(&s), before + 1, "no undo step for a refused command");
    s.execute("place.scene.set", json!({"place": city, "background": null, "icon": null, "size": null})).unwrap();
    assert!(s.topology.physical.scenes.is_empty() && s.topology.images.is_default());
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.scene(city as u32).is_some());
}

#[test]
fn an_oversized_picture_is_scaled_down_and_a_photo_loses_its_metadata() {
    let (mut s, city) = lab();
    let r = s.execute("place.background.set", json!({"place": city, "data": b64(5000, 30)})).unwrap();
    assert_eq!(r["scaled_from"], json!([5000, 30]));
    assert_eq!(r["background"]["pixels"][0], 4096);
    let r = s.execute("place.icon.set", json!({"place": city, "data": base64_encode(&jpeg_with_metadata())})).unwrap();
    assert!(r.get("scaled_from").is_none());
    let icon = s.topology.scene(city as u32).unwrap().icon.clone().unwrap();
    let bytes = s.topology.images.get(&icon).unwrap().data.clone();
    assert!(!bytes.windows(4).any(|w| w == b"Exif"));
}

#[test]
fn hostile_pictures_and_parameters_are_refused_and_change_nothing() {
    let (mut s, city) = lab();
    let snapshot = s.topology.clone();
    let steps = s.undo.len();
    let cases = [
        ("place.background.set", json!({"place": city})),
        ("place.background.set", json!({"place": city, "data": "@@@@"})),
        ("place.background.set", json!({"place": city, "data": base64_encode(b"hello, not a picture")})),
        ("place.background.set", json!({"place": city, "data": base64_encode(&png(8, 8)[..20])})),
        ("place.background.set", json!({"place": city, "data": 5})),
        ("place.background.set", json!({"place": city, "image": "nope"})),
        ("place.background.set", json!({"place": 4242, "data": b64(4, 4)})),
        ("place.background.set", json!({"place": [1], "data": b64(4, 4)})),
        ("place.background.set", json!({"place": city, "data": b64(4, 4), "mode": "height"})),
        ("place.background.set", json!({"place": city, "data": b64(4, 4), "mode": "width", "custom": 99999999})),
        ("place.background.set", json!({"place": city, "path": "/nonexistent/dir/picture.png"})),
        ("place.icon.set", json!({"place": "intercity", "data": b64(4, 4)})),
        ("place.scene.set", json!({"place": city, "background": 5})),
        ("workspace.background.set", json!({"data": []})),
        ("image.remove", json!({})),
        ("image.remove", json!({"kind": "wall"})),
    ];
    for (cmd, p) in cases {
        assert!(s.execute(cmd, p.clone()).is_err(), "{cmd} {p}");
    }
    assert_eq!(s.topology, snapshot);
    assert_eq!(s.undo.len(), steps);
}

#[test]
fn a_lab_holds_at_most_the_limit_of_pictures() {
    let (mut s, city) = lab();
    for i in 0..MAX_IMAGES as u32 {
        let r = s.execute("workspace.background.set", json!({"data": b64(2 + i, 2)}));
        assert!(r.is_ok(), "{i}");
        // Each replaces the last, so only one stays.
    }
    assert_eq!(s.topology.images.0.len(), 1);
    // Keep them all in use: one icon each is not possible on one place, so fill places.
    let mut places = vec![city];
    for i in 0..MAX_IMAGES {
        let id = s.execute("place.add", json!({"kind": "city", "name": format!("C{i}")})).unwrap()["id"].as_u64().unwrap();
        places.push(id);
    }
    let mut stored = 1;
    let mut refused = 0;
    for (i, place) in places.iter().enumerate() {
        match s.execute("place.icon.set", json!({"place": place, "data": b64(100 + i as u32, 3)})) {
            Ok(_) => stored += 1,
            Err(_) => refused += 1,
        }
    }
    assert!(stored <= MAX_IMAGES + 1 && refused >= 1, "{stored} stored, {refused} refused");
    assert!(s.topology.images.0.len() <= MAX_IMAGES);
    let _ = INTERCITY;
}
