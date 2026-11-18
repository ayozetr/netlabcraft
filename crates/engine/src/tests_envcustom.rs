//! Custom environments: a name, variables defined by formulas that a sensor reads, the shows saved
//! per place, the choice of interpolation and the bundled environments.

use serde_json::{Value, json};

use crate::Session;

fn lab() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "iot"})).unwrap();
    s
}

fn variable(got: &Value, id: &str) -> Value {
    got["variables"].as_array().unwrap().iter().find(|v| v["id"] == id).cloned().unwrap_or(Value::Null)
}

#[test]
fn a_custom_environment_has_a_name_and_variables_defined_by_formulas() {
    let mut s = lab();
    let g = s.execute("environment.create", json!({"name": "  Cold room  "})).unwrap();
    assert_eq!(g["name"], "Cold room");
    s.execute("environment.set", json!({"variable": "wind_speed", "value": 10})).unwrap();
    s.execute("environment.clock", json!({"time": "15:00"})).unwrap();
    let g = s
        .execute("environment.custom", json!({"id": "wave", "unit": "C", "formula": "20 + 8 * sin((hour - 9) / 24 * 2 * pi)", "min": 0, "max": 40}))
        .unwrap();
    let wave = variable(&g, "wave");
    assert_eq!((wave["custom"].clone(), wave["group"].clone(), wave["source"].clone()), (json!(true), json!("Custom"), json!("own")));
    assert!((wave["value"].as_f64().unwrap() - 28.0).abs() < 1e-6, "{wave}");
    // It reads other variables as the place has them, and other custom variables.
    let g = s.execute("environment.custom", json!({"id": "chill", "formula": "wave - wind_speed / 2"})).unwrap();
    assert!((variable(&g, "chill")["value"].as_f64().unwrap() - 23.0).abs() < 1e-6);
    s.execute("environment.clock", json!({"time": "03:00"})).unwrap();
    let g = s.execute("environment.get", json!({})).unwrap();
    assert!((variable(&g, "wave")["value"].as_f64().unwrap() - 12.0).abs() < 1e-6, "the formula follows the clock");
    // A sensor can read it.
    s.execute("device.add", json!({"model": "Generic Environment Sensor", "name": "Env1"})).unwrap();
    s.execute("iot.configure", json!({"device": "Env1", "variable": "wave"})).unwrap();
    assert_eq!(s.execute("iot.configure", json!({"device": "Env1"})).unwrap()["value"], 12);
    assert_eq!(s.execute("iot.configure", json!({"device": "Env1"})).unwrap()["variable"], "wave");
    // Mistakes change nothing.
    let before = s.topology.clone();
    for bad in [
        json!({"id": "humidity", "formula": "1"}),
        json!({"id": "Bad Id"}),
        json!({"id": "x", "formula": "1 +"}),
        json!({"id": "x", "formula": "nowhere"}),
        json!({"id": "x", "min": 5, "max": 1}),
        json!({"id": "x", "min": "low"}),
        json!({"id": "x", "formula": 5}),
        json!({"formula": "1"}),
    ] {
        assert!(s.execute("environment.custom", bad.clone()).is_err(), "{bad}");
    }
    assert!(s.execute("environment.create", json!({"name": ""})).is_err());
    assert!(s.execute("environment.custom.remove", json!({"id": "nope"})).is_err());
    assert_eq!(s.topology, before);
    // A formula that fails reads its default and says why.
    let g = s.execute("environment.custom", json!({"id": "boom", "formula": "1 / (hour - hour)", "default": 7})).unwrap();
    assert_eq!((variable(&g, "boom")["value"].as_f64(), variable(&g, "boom")["error"].as_str()), (Some(7.0), Some("division by zero")));
    // Saved with the lab, one undo step, removed with the custom environment.
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
    assert!(variable(&s2.execute("environment.get", json!({})).unwrap(), "wave")["custom"] == json!(true));
    s.execute("environment.custom.remove", json!({"id": "boom"})).unwrap();
    assert!(variable(&s.execute("environment.get", json!({})).unwrap(), "boom").is_null());
    s.execute("edit.undo", json!({})).unwrap();
    assert!(!variable(&s.execute("environment.get", json!({})).unwrap(), "boom").is_null());
    let g = s.execute("environment.clear", json!({})).unwrap();
    assert!(variable(&g, "wave").is_null() && g["name"] == "", "Remove Custom Environment removes the name and the variables");
}

#[test]
fn the_shows_are_saved_per_place_and_copied_from_the_parent() {
    let mut s = lab();
    let city = s.execute("place.add", json!({"kind": "city", "name": "Harbor"})).unwrap()["id"].as_u64().unwrap();
    let building = s.execute("place.add", json!({"kind": "building", "name": "Annex", "parent": city})).unwrap()["id"].as_u64().unwrap();
    let closet = s.execute("place.add", json!({"kind": "closet", "name": "Cold room", "parent": building})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(s.execute("environment.get", json!({"place": closet})).unwrap()["hidden"], json!([]));
    let g = s.execute("environment.shows", json!({"place": city, "hidden": ["gravity", "argon"]})).unwrap();
    assert_eq!((g["hidden"].clone(), g["hidden_from"].clone()), (json!(["gravity", "argon"]), json!(city)));
    let g = s.execute("environment.get", json!({"place": closet})).unwrap();
    assert_eq!((g["hidden"].clone(), g["hidden_from"].clone()), (json!(["gravity", "argon"]), json!(city)), "inherited");
    s.execute("environment.shows", json!({"place": closet, "hidden": []})).unwrap();
    assert_eq!(s.execute("environment.get", json!({"place": closet})).unwrap()["hidden"], json!([]), "its own: everything shown");
    let g = s.execute("environment.shows", json!({"place": closet, "copy_from_parent": true})).unwrap();
    assert_eq!((g["hidden"].clone(), g["hidden_from"].clone()), (json!(["gravity", "argon"]), json!(closet)));
    s.execute("environment.set", json!({"place": closet, "variable": "humidity", "value": 55})).unwrap();
    let g = s.execute("environment.shows", json!({"place": closet, "only_active": true})).unwrap();
    let hidden = g["hidden"].as_array().unwrap();
    assert!(!hidden.contains(&json!("humidity")) && hidden.contains(&json!("gravity")), "only what is set is shown");
    let g = s.execute("environment.shows", json!({"place": closet, "reset": true})).unwrap();
    assert_eq!(g["hidden_from"], json!(city));
    assert!(s.execute("environment.shows", json!({"place": closet, "hidden": ["unicorns"]})).is_err());
    assert!(s.execute("environment.shows", json!({"place": closet, "hidden": [1]})).is_err());
    assert!(s.execute("environment.shows", json!({"place": closet, "hidden": "all"})).is_err());
    // Saved with the lab.
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.execute("environment.get", json!({"place": closet})).unwrap()["hidden"], json!(["gravity", "argon"]));
}

#[test]
fn bundled_environments_and_interpolation() {
    let mut s = lab();
    let list = s.execute("environment.defaults", json!({})).unwrap();
    let ids: Vec<&str> = list["defaults"].as_array().unwrap().iter().map(|d| d["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["desert", "tropical", "arctic", "indoor-office", "greenhouse"]);
    let g = s.execute("environment.defaults", json!({"name": "Arctic"})).unwrap();
    assert_eq!(g["name"], "Arctic");
    assert!(variable(&g, "ambient_temperature")["value"].as_f64().unwrap() < -20.0);
    assert_eq!(variable(&g, "snow")["source"], "own");
    assert!(s.execute("environment.defaults", json!({"name": "atlantis"})).is_err());
    assert!(s.execute("environment.defaults", json!({"name": 3})).is_err());
    // Joined by a step, the temperature keeps the morning's value until the next keyframe.
    s.execute("environment.clock", json!({"time": "09:00"})).unwrap();
    let linear = variable(&s.execute("environment.get", json!({})).unwrap(), "ambient_temperature")["value"].as_f64().unwrap();
    let g = s.execute("environment.settings", json!({"interpolation": "step"})).unwrap();
    assert_eq!(g["interpolation"], "step");
    let step = variable(&g, "ambient_temperature")["value"].as_f64().unwrap();
    assert_eq!(step, -32.0, "the 04:00 keyframe until 14:00");
    assert!(linear > step);
    assert!(s.execute("environment.settings", json!({"interpolation": "cubic"})).is_err());
    assert_eq!(s.execute("environment.settings", json!({})).unwrap()["interpolation"], "step");
}
