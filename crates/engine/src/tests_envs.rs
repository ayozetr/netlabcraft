//! The Environments commands: inheritance by place, keyframes, the clock, export and import,
//! undo, and the smart sensors that read them.

use serde_json::{Value, json};

use crate::Session;

fn lab(sample: &str) -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": sample})).unwrap();
    s
}

fn variable<'a>(got: &'a Value, id: &str) -> &'a Value {
    got["variables"].as_array().unwrap().iter().find(|v| v["id"] == id).unwrap()
}

fn reading(s: &mut Session, sensor: &str) -> i64 {
    s.execute("iot.configure", json!({"device": sensor})).unwrap()["value"].as_i64().unwrap()
}

/// A city, a building and a closet, with their numbers.
fn places(s: &mut Session) -> (u64, u64, u64) {
    let city = s.execute("place.add", json!({"kind": "city", "name": "Harbor"})).unwrap()["id"].as_u64().unwrap();
    let building = s.execute("place.add", json!({"kind": "building", "name": "Annex", "parent": city})).unwrap()["id"].as_u64().unwrap();
    let closet = s.execute("place.add", json!({"kind": "closet", "name": "Cold room", "parent": building})).unwrap()["id"].as_u64().unwrap();
    (city, building, closet)
}

#[test]
fn a_place_inherits_what_the_places_above_it_set() {
    let mut s = lab("iot");
    let (city, building, closet) = places(&mut s);
    let g = s.execute("environment.get", json!({"place": closet})).unwrap();
    let t = variable(&g, "ambient_temperature");
    assert_eq!((t["value"].as_f64(), t["source"].as_str()), (Some(15.0), Some("default")));
    assert_eq!(g["path"].as_array().unwrap().len(), 4, "Intercity, the city, the building and the closet");
    assert_eq!(g["time"], "08:00:00");
    s.execute("environment.set", json!({"place": city, "variable": "ambient_temperature", "value": 21})).unwrap();
    s.execute("environment.set", json!({"place": "intercity", "variable": "Gravity", "value": 1.62})).unwrap();
    let g = s.execute("environment.get", json!({"place": "Cold room"})).unwrap();
    assert_eq!(
        (variable(&g, "ambient_temperature")["source"].as_str(), variable(&g, "ambient_temperature")["from"].as_u64()),
        (Some("inherited"), Some(city))
    );
    assert_eq!(variable(&g, "gravity")["value"].as_f64(), Some(1.62));
    // The closet overrides; the building above it still reads the city.
    s.execute("environment.set", json!({"place": closet, "variable": "ambient_temperature", "value": 4.5})).unwrap();
    let c = s.execute("environment.get", json!({"place": closet})).unwrap();
    assert_eq!(
        (variable(&c, "ambient_temperature")["value"].as_f64(), variable(&c, "ambient_temperature")["source"].as_str()),
        (Some(4.5), Some("own"))
    );
    let b = s.execute("environment.get", json!({"place": building})).unwrap();
    assert_eq!(variable(&b, "ambient_temperature")["value"].as_f64(), Some(21.0));
    s.execute("environment.clear", json!({"place": closet, "variable": "ambient_temperature"})).unwrap();
    let c = s.execute("environment.get", json!({"place": closet})).unwrap();
    assert_eq!(variable(&c, "ambient_temperature")["source"], "inherited");
    s.execute("environment.clear", json!({"place": "intercity"})).unwrap();
    assert_eq!(variable(&s.execute("environment.get", json!({"place": 0})).unwrap(), "gravity")["source"], "default");
}

#[test]
fn every_change_is_one_undo_step_and_a_bad_call_changes_nothing() {
    let mut s = lab("iot");
    let before = s.topology.clone();
    s.execute("environment.set", json!({"variable": "humidity", "value": 70})).unwrap();
    assert_ne!(s.topology, before);
    assert!(s.execute("edit.undo", json!({})).is_ok());
    assert_eq!(s.topology, before);
    for (cmd, bad) in [
        ("environment.set", json!({})),
        ("environment.set", json!({"variable": "humidity"})),
        ("environment.set", json!({"variable": "humidity", "value": 101})),
        ("environment.set", json!({"variable": "humidity", "value": "wet"})),
        ("environment.set", json!({"variable": "unicorns", "value": 1})),
        ("environment.set", json!({"place": 9999, "variable": "humidity", "value": 1})),
        ("environment.set", json!({"place": "nowhere", "variable": "humidity", "value": 1})),
        ("environment.set", json!({"notes": 5})),
        ("environment.keyframe.add", json!({"variable": "humidity", "time": "25:00", "value": 1})),
        ("environment.keyframe.add", json!({"variable": "humidity", "value": 1})),
        ("environment.keyframe.remove", json!({"variable": "humidity", "time": "01:00"})),
        ("environment.clear", json!({"variable": "unicorns"})),
        ("environment.clock", json!({"real_minutes": 0})),
        ("environment.clock", json!({"sim_minutes": 5000})),
        ("environment.clock", json!({"time": "99:99"})),
        ("environment.clock", json!({"paused": "yes"})),
        ("environment.import", json!({})),
        ("environment.import", json!({"text": "not json"})),
    ] {
        assert!(s.execute(cmd, bad.clone()).is_err(), "{cmd} {bad}");
    }
    assert_eq!(s.topology, before);
    assert!(!s.can_undo(), "nothing failed left an undo step behind");
}

#[test]
fn keyframes_move_in_a_line_with_the_clock_and_pause_holds_it() {
    let mut s = lab("iot");
    s.execute("environment.keyframe.add", json!({"place": "intercity", "variable": "ambient_temperature", "time": "00:00:00", "value": 0})).unwrap();
    let g =
        s.execute("environment.keyframe.add", json!({"place": "intercity", "variable": "ambient_temperature", "time": 43_200, "value": 40})).unwrap();
    assert_eq!(variable(&g, "ambient_temperature")["keys"].as_array().unwrap().len(), 2);
    // One simulated minute is an hour of environment time.
    let g = s.execute("environment.clock", json!({"real_minutes": 1, "sim_minutes": 60, "time": "06:00"})).unwrap();
    assert_eq!((g["time"].as_str(), g["clock"]["sim_minutes"].as_u64()), (Some("06:00:00"), Some(60)));
    assert_eq!(variable(&g, "ambient_temperature")["value"].as_f64(), Some(20.0));
    s.execute("sim.advance", json!({"seconds": 60})).unwrap();
    let g = s.execute("environment.get", json!({})).unwrap();
    assert_eq!(g["time"], "07:00:00");
    let v = variable(&g, "ambient_temperature")["value"].as_f64().unwrap();
    assert!((v - 40.0 * 7.0 / 12.0).abs() < 1e-9, "{v}");
    // Paused, the time stays while the simulation runs, and resuming goes on from there.
    let g = s.execute("environment.clock", json!({"paused": true})).unwrap();
    assert_eq!(g["clock"]["paused"], true);
    s.execute("sim.advance", json!({"seconds": 120})).unwrap();
    assert_eq!(s.execute("environment.get", json!({})).unwrap()["time"], "07:00:00");
    s.execute("environment.clock", json!({"paused": false})).unwrap();
    s.execute("sim.advance", json!({"seconds": 60})).unwrap();
    assert_eq!(s.execute("environment.get", json!({})).unwrap()["time"], "08:00:00");
    // Changing the ratio does not move the time shown.
    let g = s.execute("environment.clock", json!({"real_minutes": 2, "sim_minutes": 1})).unwrap();
    assert_eq!(g["time"], "08:00:00");
    // Removing the keyframes hands the variable back.
    s.execute("environment.keyframe.remove", json!({"place": 0, "variable": "ambient_temperature", "time": "12:00"})).unwrap();
    let g = s.execute("environment.keyframe.remove", json!({"place": 0, "variable": "ambient_temperature", "time": 0})).unwrap();
    assert_eq!(variable(&g, "ambient_temperature")["source"], "default");
    assert!(s.topology.environments.places.is_empty());
}

#[test]
fn notes_are_kept_per_place_and_exported_files_round_trip() {
    let mut s = lab("iot");
    let (city, _, closet) = places(&mut s);
    s.execute("environment.set", json!({"place": closet, "variable": "wind_speed", "value": 12})).unwrap();
    s.execute("environment.keyframe.add", json!({"place": closet, "variable": "rain", "time": "06:00", "value": 2})).unwrap();
    let g = s.execute("environment.set", json!({"place": closet, "notes": "Rack room, door open", "show_notes": true})).unwrap();
    assert_eq!((g["notes"].as_str(), g["show_notes"].as_bool()), (Some("Rack room, door open"), Some(true)));
    let e = s.execute("environment.export", json!({"place": closet})).unwrap();
    assert_eq!(e["file"], "cold-room.environment.json");
    let text = e["text"].as_str().unwrap().to_string();
    assert!(text.contains("netlabcraft-environment") && text.contains("wind_speed"));
    let g = s.execute("environment.import", json!({"place": city, "text": text})).unwrap();
    assert_eq!((g["imported"]["variables"].as_u64(), g["imported"]["skipped"].as_u64()), (Some(2), Some(0)));
    assert_eq!(variable(&g, "wind_speed")["source"], "own");
    assert_eq!(g["notes"], "Rack room, door open");
    assert!(s.execute("edit.undo", json!({})).is_ok());
    assert_eq!(variable(&s.execute("environment.get", json!({"place": city})).unwrap(), "wind_speed")["source"], "default");
    // A lab saved with environments opens with them; one saved before has none.
    let json = netlabcraft_model::file::to_json(&s.topology).unwrap();
    let back = netlabcraft_model::file::from_json(&json).unwrap();
    assert_eq!(back.environments, s.topology.environments);
    let old = json.replacen("\"version\": 11", "\"version\": 10", 1);
    assert!(netlabcraft_model::file::from_json(&old).is_ok());
    assert!(netlabcraft_model::file::from_json(&netlabcraft_model::file::to_json(&lab("iot").topology).unwrap()).unwrap().environments.is_default());
}

#[test]
fn a_place_environment_decides_what_its_sensor_reads_and_nothing_else_changes() {
    let mut s = lab("iot");
    // No environment: the sensor reads its own number, as before.
    s.execute("iot.configure", json!({"device": "Sensor1", "value": 31})).unwrap();
    assert_eq!(reading(&mut s, "Sensor1"), 31);
    s.execute("environment.set", json!({"place": "intercity", "variable": "humidity", "value": 80})).unwrap();
    assert_eq!(reading(&mut s, "Sensor1"), 31, "humidity is not what a temperature sensor reads");
    s.execute("environment.set", json!({"place": "intercity", "variable": "ambient_temperature", "value": 12.4})).unwrap();
    assert_eq!(reading(&mut s, "Sensor1"), 12);
    let report = s.execute("iot.sync", json!({})).unwrap();
    assert!(report["things"].as_array().unwrap().iter().any(|t| t["device"] == "Sensor1" && t["value"] == 12), "{report}");
    s.execute("environment.clear", json!({"place": 0})).unwrap();
    assert_eq!(reading(&mut s, "Sensor1"), 31);
}

#[test]
fn the_smart_room_keeps_working_and_shows_through_where_no_place_sets_a_variable() {
    let mut s = lab("iot-climate");
    s.execute("iot.sync", json!({})).unwrap();
    s.execute("sim.advance", json!({"seconds": 600})).unwrap();
    let room = s.execute("iot.environment", json!({})).unwrap()["room"]["temperature"].as_i64().unwrap();
    let g = s.execute("environment.get", json!({})).unwrap();
    let t = variable(&g, "ambient_temperature");
    assert_eq!((t["source"].as_str(), t["value"].as_f64()), (Some("room"), Some(room as f64)));
    // A place that sets it takes over for its sensors; the room itself goes on running.
    s.execute("environment.set", json!({"place": "intercity", "variable": "ambient_temperature", "value": 25})).unwrap();
    assert_eq!(reading(&mut s, "Thermo1"), 25);
    assert_eq!(s.execute("iot.environment", json!({})).unwrap()["room"]["temperature"].as_i64().unwrap(), room);
    s.execute("environment.clear", json!({})).unwrap();
    s.execute("environment.clear", json!({"place": 0})).unwrap();
    assert_eq!(reading(&mut s, "Thermo1"), room);
}

#[test]
fn removing_a_place_takes_its_environment_with_it() {
    let mut s = lab("iot");
    let (_, building, closet) = places(&mut s);
    s.execute("environment.set", json!({"place": closet, "variable": "ozone", "value": 1})).unwrap();
    s.execute("environment.set", json!({"place": building, "variable": "ozone", "value": 2})).unwrap();
    s.execute("place.remove", json!({"id": closet})).unwrap();
    assert_eq!(s.topology.environments.places.len(), 1);
    assert!(s.execute("environment.get", json!({"place": closet})).is_err());
}
