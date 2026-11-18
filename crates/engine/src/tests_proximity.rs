//! Proximity, second round: Bluetooth pairing codes and profiles, RFID ids typed in, a trip sensor
//! that tells a crossing from standing near, a beacon's log, and what lamps and speakers give the
//! room: the brightness of a smart LED, the pitch a speaker plays and a detector tuned to one.

use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, device: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": device, "line": line})).unwrap()["output"].as_str().unwrap().to_string()
}

fn home() -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Home-Gateway", "name": "Home", "x": 360, "y": 40})).unwrap();
    s.execute("iot.server", json!({"device": "Home", "service": true})).unwrap();
    s
}

fn add(s: &mut Session, model: &str, name: &str, x: i64) {
    s.execute("device.add", json!({"model": model, "name": name, "x": x, "y": 250})).unwrap();
    s.execute("sim.advance", json!({"seconds": 5})).unwrap();
    let out = cli(s, name, "ipconfig /renew");
    assert!(!out.contains("failed"), "{name}: {out}");
    s.execute("iot.configure", json!({"device": name, "server": "192.168.0.1"})).unwrap();
}

fn switch(s: &mut Session, name: &str, on: bool) {
    s.execute("iot.switch", json!({"device": "Home", "name": name, "on": on})).unwrap();
}

fn page(s: &mut Session, name: &str) -> Value {
    s.execute("iot.configure", json!({"device": name})).unwrap()
}

fn set(s: &mut Session, name: &str, params: Value) -> Result<Value, String> {
    let mut p = params;
    p["device"] = json!(name);
    s.execute("iot.configure", p).map_err(|e| e.to_string())
}

#[test]
fn a_speaker_and_a_player_pair_only_with_the_same_code_and_a_music_profile() {
    let mut s = home();
    add(&mut s, "Bluetooth Speaker", "Speaker1", 450);
    add(&mut s, "Portable Music Player", "Player1", 500);
    assert_eq!(page(&mut s, "Speaker1")["paired"], "Player1");
    assert_eq!((page(&mut s, "Speaker1")["pairing_code"].clone(), page(&mut s, "Speaker1")["profile"].clone()), (json!("0000"), json!("a2dp")));
    set(&mut s, "Speaker1", json!({"pairing_code": "1234"})).unwrap();
    assert!(page(&mut s, "Speaker1")["paired"].is_null() && page(&mut s, "Player1")["paired"].is_null(), "different codes");
    set(&mut s, "Player1", json!({"pairing_code": 1234})).unwrap();
    assert_eq!((page(&mut s, "Speaker1")["paired"].clone(), page(&mut s, "Player1")["paired"].clone()), (json!("Player1"), json!("Speaker1")));
    assert_eq!(page(&mut s, "Player1")["pairing_code"], "1234");
    set(&mut s, "Speaker1", json!({"profile": "headset"})).unwrap();
    assert!(page(&mut s, "Speaker1")["paired"].is_null() && page(&mut s, "Player1")["paired"].is_null(), "a headset takes calls, not music");
    set(&mut s, "Speaker1", json!({"profile": "A2DP"})).unwrap();
    assert_eq!(page(&mut s, "Player1")["paired"], "Speaker1");
    for bad in [
        json!({"pairing_code": "12345"}),
        json!({"pairing_code": "12a4"}),
        json!({"pairing_code": 10000}),
        json!({"pairing_code": -1}),
        json!({"profile": "hifi"}),
        json!({"profile": 3}),
    ] {
        assert!(set(&mut s, "Speaker1", bad.clone()).is_err(), "{bad}");
    }
    assert!(set(&mut s, "Player1", json!({"profile": "a2dp"})).is_err(), "only a speaker has a profile");
    // Saved with the lab.
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
}

#[test]
fn an_rfid_card_can_have_an_id_typed_in() {
    let mut s = home();
    add(&mut s, "RFID Reader", "Reader1", 450);
    add(&mut s, "RFID Card", "Card1", 480);
    let own = page(&mut s, "Reader1")["value"].as_i64().unwrap();
    assert!(own > 0 && own < 100, "the card's own id: {own}");
    set(&mut s, "Card1", json!({"tag": "1A2B3C"})).unwrap();
    assert_eq!(page(&mut s, "Reader1")["value"], 0x1a2b3c);
    assert_eq!(page(&mut s, "Card1")["tag"], "1a2b3c");
    set(&mut s, "Card1", json!({"tag": "0xFF"})).unwrap();
    assert_eq!(page(&mut s, "Reader1")["value"], 255);
    set(&mut s, "Card1", json!({"tag": 4242})).unwrap();
    assert_eq!(page(&mut s, "Reader1")["value"], 4242);
    set(&mut s, "Card1", json!({"tag": 0})).unwrap();
    assert_eq!(page(&mut s, "Reader1")["value"], own, "0 hands the id back to the card");
    for bad in [json!({"tag": "xyz"}), json!({"tag": "123456789"}), json!({"tag": 3_000_000_000_u64}), json!({"tag": ""}), json!({"tag": true})] {
        assert!(set(&mut s, "Card1", bad.clone()).is_err(), "{bad}");
    }
    assert!(set(&mut s, "Reader1", json!({"tag": "1"})).is_err(), "only a card has one");
}

#[test]
fn a_trip_sensor_tells_a_crossing_from_standing_near_the_wire() {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Trip Sensor", "name": "Trip1", "x": 0, "y": 0})).unwrap();
    s.execute("device.add", json!({"model": "Smartphone", "name": "Phone1", "x": 18, "y": 0})).unwrap();
    let reading = |s: &mut Session| page(s, "Trip1")["value"].as_i64().unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert_eq!(reading(&mut s), 1, "as before: something that moves is near");
    set(&mut s, "Trip1", json!({"crossing": true, "angle": 90})).unwrap();
    assert_eq!((page(&mut s, "Trip1")["crossing"].clone(), page(&mut s, "Trip1")["angle"].clone()), (json!(true), json!(90)));
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert_eq!(reading(&mut s), 0, "standing near the wire is not crossing it");
    // Across the wire (the sensor's, vertical, 50 m long): 10 m on one side to 10 m on the other.
    s.execute("device.move", json!({"device": "Phone1", "x": -18, "y": 0})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert_eq!(reading(&mut s), 1, "it crossed");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    assert_eq!(reading(&mut s), 0, "the reading lasts five seconds");
    // Past the end of the wire is not across it.
    s.execute("device.move", json!({"device": "Phone1", "x": 18, "y": 180})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    s.execute("device.move", json!({"device": "Phone1", "x": -18, "y": 180})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert_eq!(reading(&mut s), 0, "100 m up, well past the wire's 25 m");
    // Turned around, the wire runs along the walk.
    set(&mut s, "Trip1", json!({"angle": 0})).unwrap();
    s.execute("device.move", json!({"device": "Phone1", "x": 18, "y": 18})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    s.execute("device.move", json!({"device": "Phone1", "x": 18, "y": -18})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert_eq!(reading(&mut s), 1, "crossing a horizontal wire from north to south");
    for bad in [json!({"angle": 360}), json!({"angle": -1}), json!({"crossing": 1})] {
        assert!(set(&mut s, "Trip1", bad.clone()).is_err(), "{bad}");
    }
    assert!(set(&mut s, "Phone1", json!({"crossing": true})).is_err());
}

#[test]
fn a_beacon_logs_who_came_into_its_range_and_who_left() {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Beacon", "name": "Beacon1", "x": 0, "y": 0})).unwrap();
    s.execute("device.add", json!({"model": "Smart LED", "name": "Led1", "x": 90, "y": 0})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    let log = s.execute("iot.beacon", json!({"device": "Beacon1"})).unwrap();
    assert_eq!(log["audience"], json!(["Led1"]), "{log}");
    assert_eq!(log["log"][0]["event"], "heard");
    s.execute("device.move", json!({"device": "Led1", "x": 900, "y": 0})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    s.execute("device.move", json!({"device": "Led1", "x": 90, "y": 0})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    let log = s.execute("iot.beacon", json!({"device": "Beacon1"})).unwrap();
    let events: Vec<&str> = log["log"].as_array().unwrap().iter().map(|e| e["event"].as_str().unwrap()).collect();
    assert_eq!(events, ["heard", "lost", "heard"], "{log}");
    assert!(log["log"][1]["time_s"].as_u64().unwrap() >= 2);
    assert!(s.execute("iot.beacon", json!({"device": "Led1"})).is_err());
}

#[test]
fn a_smart_leds_brightness_lights_the_room_and_a_speaker_plays_the_pitch_it_was_given() {
    let mut s = home();
    s.execute("iot.environment", json!({"enabled": true, "outside_light": 0})).unwrap();
    add(&mut s, "Smart LED", "Led1", 450);
    let light = |s: &mut Session| s.execute("iot.environment", json!({})).unwrap()["room"]["light"].as_i64().unwrap();
    switch(&mut s, "Led1", true);
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!(light(&mut s), 70, "a lit LED at full brightness adds what it always did");
    set(&mut s, "Led1", json!({"brightness": 30})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!(light(&mut s), 21, "30 % of 70");
    set(&mut s, "Led1", json!({"brightness": 0})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!(light(&mut s), 0, "on, but dark");
    // A speaker and a player play the pitch they were given; a detector tuned to it hears it, one tuned elsewhere does not.
    add(&mut s, "Portable Music Player", "Player1", 500);
    add(&mut s, "Bluetooth Speaker", "Speaker1", 530);
    add(&mut s, "Sound Frequency Detector", "Mic1", 560);
    add(&mut s, "Sound Frequency Detector", "Mic2", 590);
    switch(&mut s, "Player1", true);
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!((page(&mut s, "Mic1")["value"].clone(), page(&mut s, "Mic1")["value2"].clone()), (json!(440), json!(75)), "the usual tone");
    set(&mut s, "Speaker1", json!({"frequency": 880})).unwrap();
    set(&mut s, "Player1", json!({"frequency": 880})).unwrap();
    set(&mut s, "Mic1", json!({"frequency": 900})).unwrap();
    set(&mut s, "Mic2", json!({"frequency": 440})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!(
        (page(&mut s, "Mic1")["value"].clone(), page(&mut s, "Mic1")["value2"].clone()),
        (json!(880), json!(75)),
        "880 is within a tenth of 900"
    );
    assert_eq!(
        (page(&mut s, "Mic2")["value"].clone(), page(&mut s, "Mic2")["value2"].clone()),
        (json!(0), json!(0)),
        "tuned to 440 it hears nothing at 880"
    );
    assert_eq!(page(&mut s, "Mic1")["frequency"], 900);
    assert!(set(&mut s, "Led1", json!({"frequency": 100})).is_err(), "a LED has no pitch");
}

#[test]
fn with_the_places_own_physics_a_speaker_plays_its_pitch_and_a_led_gives_light_by_its_brightness() {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Board-SBC", "name": "Board1"})).unwrap();
    for (model, name, pin) in [("Speaker", "Speaker1", "D0"), ("Smart LED", "Led1", "D1")] {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
        s.execute("board.wire", json!({"device": "Board1", "pin": pin, "thing": name})).unwrap();
    }
    s.execute("device.add", json!({"model": "Sound Frequency Detector", "name": "Mic1"})).unwrap();
    s.execute("environment.physics", json!({"enabled": true})).unwrap();
    s.execute("iot.configure", json!({"device": "Speaker1", "frequency": 1500})).unwrap();
    s.execute("iot.configure", json!({"device": "Led1", "brightness": 40})).unwrap();
    s.execute("environment.set", json!({"variable": "luminosity", "value": 0})).unwrap();
    s.execute("board.program", json!({"device": "Board1", "source": "write(D0, 1)\nwrite(D1, 1)\nsleep 9\n", "run": true})).unwrap();
    s.execute("sim.advance", json!({"seconds": 600})).unwrap();
    assert_eq!(page(&mut s, "Mic1")["value"], 1500, "the speaker's own pitch");
    let g = s.execute("environment.get", json!({})).unwrap();
    let light = g["variables"].as_array().unwrap().iter().find(|v| v["id"] == "luminosity").unwrap()["value"].as_f64().unwrap();
    assert!((light - 28.0).abs() < 0.5, "40 % of the lamp's 70: {light}");
}
