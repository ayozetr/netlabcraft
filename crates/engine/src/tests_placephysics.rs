//! The places' own physics: a place changes by rates, settles back, couples to the place above
//! through an open window (faster in wind), burns and clears, and sound fades with distance.

use serde_json::{Value, json};

use crate::Session;

fn lab(parts: &[(&str, &str, &str)]) -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Board-SBC", "name": "Board1"})).unwrap();
    for (model, name, pin) in parts {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
        if !pin.is_empty() {
            s.execute("board.wire", json!({"device": "Board1", "pin": pin, "thing": name})).unwrap();
        }
    }
    s
}

fn program(s: &mut Session, source: &str) {
    s.execute("board.program", json!({"device": "Board1", "source": source, "run": true})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
}

fn value(s: &mut Session, device: &str, key: &str) -> i64 {
    s.execute("iot.configure", json!({"device": device})).unwrap()[key].as_i64().unwrap()
}

fn live(s: &mut Session, place: Value, id: &str) -> (f64, String) {
    let g = s.execute("environment.get", json!({"place": place})).unwrap();
    let v = g["variables"].as_array().unwrap().iter().find(|v| v["id"] == id).unwrap().clone();
    (v["value"].as_f64().unwrap(), v["source"].as_str().unwrap().to_string())
}

#[test]
fn a_heater_warms_its_place_by_a_rate_and_the_place_settles_back() {
    let mut s = lab(&[("Heating Element", "Heater1", "D0"), ("Temperature Sensor", "Temp1", "A0")]);
    s.execute("environment.set", json!({"place": "intercity", "variable": "ambient_temperature", "value": 20})).unwrap();
    // The older rule: the element's ten degrees at once.
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    assert_eq!(value(&mut s, "Temp1", "value"), 30);
    program(&mut s, "write(D0, 0)\nsleep 9\n");
    assert_eq!(value(&mut s, "Temp1", "value"), 20);
    // With the places' own physics: ten minutes cover 63 % of ten degrees, and it settles at 30.
    assert_eq!(s.execute("environment.physics", json!({"enabled": true})).unwrap()["physics"], true);
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    assert!(value(&mut s, "Temp1", "value") <= 21, "it has barely started");
    s.execute("sim.advance", json!({"seconds": 600})).unwrap();
    let warm = value(&mut s, "Temp1", "value");
    assert!((25..=27).contains(&warm), "one time constant: {warm}");
    let (live_value, source) = live(&mut s, json!(null), "ambient_temperature");
    assert_eq!((source.as_str(), live_value.round() as i64), ("live", warm));
    s.execute("sim.advance", json!({"seconds": 6_000})).unwrap();
    assert_eq!(value(&mut s, "Temp1", "value"), 30);
    program(&mut s, "write(D0, 0)\nsleep 9\n");
    s.execute("sim.advance", json!({"seconds": 600})).unwrap();
    let cooling = value(&mut s, "Temp1", "value");
    assert!((23..=25).contains(&cooling), "it cools back by the same rate: {cooling}");
    s.execute("sim.advance", json!({"seconds": 6_000})).unwrap();
    assert_eq!(value(&mut s, "Temp1", "value"), 20);
    assert_ne!(live(&mut s, json!(null), "ambient_temperature").1, "live", "nothing moves it any more");
    // Off again: back to the older rule.
    s.execute("environment.physics", json!({"enabled": false})).unwrap();
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    assert_eq!(value(&mut s, "Temp1", "value"), 30);
    assert!(s.execute("environment.physics", json!({"enabled": "yes"})).is_err());
}

#[test]
fn the_smart_home_things_change_the_place_they_stand_in() {
    let mut s = lab(&[("IoT-Device", "Fan1", "D0"), ("Humidifier", "Hum1", "D1"), ("Smart LED", "Led1", "D2"), ("Humidity Sensor", "Wet1", "A0")]);
    s.execute("environment.set", json!({"place": "intercity", "variable": "humidity", "value": 40})).unwrap();
    s.execute("environment.physics", json!({"enabled": true})).unwrap();
    program(&mut s, "write(D1, 1)\nsleep 9\n");
    s.execute("sim.advance", json!({"seconds": 3_000})).unwrap();
    assert_eq!(value(&mut s, "Wet1", "value"), 60, "the humidifier's twenty points");
    program(&mut s, "write(D1, 0)\nwrite(D0, 1)\nwrite(D2, 1)\nsleep 9\n");
    s.execute("sim.advance", json!({"seconds": 3_000})).unwrap();
    assert_eq!(value(&mut s, "Wet1", "value"), 34, "a fan dries by six points");
    assert!(live(&mut s, json!(null), "luminosity").0 >= 70.0, "a lamp keeps the light at 70 or more");
}

#[test]
fn an_open_window_pulls_the_place_toward_the_one_above_and_wind_speeds_it_up() {
    let temperature_after = |wind: i64| {
        let mut s = lab(&[("Smart Window", "Window1", "D0"), ("Temperature Sensor", "Temp1", "A0")]);
        let city = s.execute("place.add", json!({"kind": "city", "name": "Harbor"})).unwrap()["id"].as_u64().unwrap();
        let building = s.execute("place.add", json!({"kind": "building", "name": "Annex", "parent": city})).unwrap()["id"].as_u64().unwrap();
        let closet = s.execute("place.add", json!({"kind": "closet", "name": "Closet A", "parent": building})).unwrap()["id"].as_u64().unwrap();
        s.execute("device.place", json!({"devices": ["Window1", "Temp1"], "place": closet})).unwrap();
        s.execute("environment.set", json!({"place": building, "variable": "ambient_temperature", "value": 0})).unwrap();
        s.execute("environment.set", json!({"place": closet, "variable": "ambient_temperature", "value": 20})).unwrap();
        s.execute("environment.set", json!({"place": "intercity", "variable": "wind_speed", "value": wind})).unwrap();
        s.execute("environment.physics", json!({"enabled": true})).unwrap();
        program(&mut s, "write(D0, 1)\nsleep 9\n");
        s.execute("sim.advance", json!({"seconds": 300})).unwrap();
        value(&mut s, "Temp1", "value")
    };
    let calm = temperature_after(0);
    let windy = temperature_after(100);
    assert!((13..=15).contains(&calm), "half a time constant: {calm}");
    assert!((4..=6).contains(&windy), "six times faster in a 100 kph wind: {windy}");
}

#[test]
fn flames_make_smoke_and_a_fire_sprinkler_puts_them_out() {
    let mut s = lab(&[("Smoke Detector", "Smoke1", "")]);
    s.execute("environment.set", json!({"variable": "fire", "value": 50})).unwrap();
    s.execute("environment.physics", json!({"enabled": true})).unwrap();
    program(&mut s, "sleep 9\n");
    s.execute("sim.advance", json!({"seconds": 180})).unwrap();
    let (smoke, source) = live(&mut s, json!(null), "smoke");
    assert_eq!(source, "live");
    assert!((14.0..=18.0).contains(&smoke), "smoke covers 63 % of its way to 25 in three minutes: {smoke}");
    // A fire sprinkler that is not even switched on sees the flames and sprays.
    s.execute("device.add", json!({"model": "Fire Sprinkler", "name": "Hose1"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 900})).unwrap();
    assert!(live(&mut s, json!(null), "fire").0 < 1.0, "the flames are out");
    assert!(live(&mut s, json!(null), "smoke").0 < 1.0, "and the smoke clears");
}

#[test]
fn a_sound_fades_with_the_distance_to_the_sensor() {
    let mut s = lab(&[("Speaker", "Speaker1", "D0")]);
    s.execute("device.add", json!({"model": "Sound Frequency Detector", "name": "Near", "x": 18, "y": 0})).unwrap();
    s.execute("device.add", json!({"model": "Sound Frequency Detector", "name": "Far", "x": 180, "y": 0})).unwrap();
    s.execute("device.move", json!({"device": "Speaker1", "x": 0, "y": 0})).unwrap();
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    // Without the physics both hear the speaker's full 70 dB.
    assert_eq!((value(&mut s, "Near", "value2"), value(&mut s, "Far", "value2")), (70, 70));
    s.execute("environment.physics", json!({"enabled": true})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    // 10 m: 20 dB lost; 100 m: 40 dB lost. The pitch does not change.
    assert_eq!((value(&mut s, "Near", "value2"), value(&mut s, "Far", "value2")), (50, 30));
    assert_eq!((value(&mut s, "Near", "value"), value(&mut s, "Far", "value")), (440, 440));
}
