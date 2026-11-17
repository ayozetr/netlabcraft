//! The second wave of smart things: what each does to the room and to the other things, through
//! the same commands a person or an agent uses.

use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, device: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": device, "line": line})).unwrap()["output"].as_str().unwrap().to_string()
}

/// A home with a gateway whose room physics is on, and a thing added to it and switched on.
fn home() -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Home-Gateway", "name": "Home", "x": 360, "y": 40})).unwrap();
    s.execute("iot.server", json!({"device": "Home", "service": true})).unwrap();
    s.execute("iot.environment", json!({"enabled": true})).unwrap();
    s
}

/// Adds a Wi-Fi thing near the gateway, joins it and points it at the gateway's IoT service.
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

fn room(s: &mut Session, quantity: &str) -> i64 {
    s.execute("iot.environment", json!({})).unwrap()["room"][quantity].as_i64().unwrap()
}

fn page(s: &mut Session, name: &str) -> Value {
    s.execute("iot.configure", json!({"device": name})).unwrap()
}

#[test]
fn a_humidifier_and_a_furnace_change_the_room_and_a_humiture_monitor_reports_both_numbers() {
    let mut humid = home();
    let mut plain = home();
    for s in [&mut humid, &mut plain] {
        s.execute("iot.environment", json!({"temperature": 15, "humidity": 40, "outside_humidity": 40})).unwrap();
        add(s, "Humidifier", "Mist1", 450);
        add(s, "Furnace", "Furnace1", 520);
        add(s, "Humiture Monitor", "Humiture1", 590);
    }
    switch(&mut humid, "Mist1", true);
    switch(&mut humid, "Furnace1", true);
    for s in [&mut humid, &mut plain] {
        s.execute("sim.advance", json!({"seconds": 600})).unwrap();
    }
    assert!(room(&mut humid, "humidity") > room(&mut plain, "humidity") + 5);
    assert!(room(&mut humid, "temperature") >= 25, "ten minutes of furnace from 15 degrees: {}", room(&mut humid, "temperature"));
    assert!(room(&mut plain, "temperature") <= 16);
    // The monitor's reading is the temperature and its second reading the humidity.
    let p = page(&mut humid, "Humiture1");
    assert_eq!(p["value"], room(&mut humid, "temperature"));
    assert_eq!(p["value2"], room(&mut humid, "humidity"));
    let list = humid.execute("iot.server", json!({"device": "Home"})).unwrap();
    let row = list["things"].as_array().unwrap().iter().find(|t| t["name"] == "Humiture1").unwrap();
    assert_eq!((&row["value"], &row["value2"]), (&p["value"], &p["value2"]), "the service lists both: {list}");
    // Off again: the room stops warming and the furnace is only a switch.
    switch(&mut humid, "Furnace1", false);
    assert_eq!(page(&mut humid, "Furnace1")["on"], false);
}

#[test]
fn a_ceiling_fan_cools_toward_the_outside_and_an_appliance_is_only_a_switch() {
    let mut fan = home();
    let mut still = home();
    for s in [&mut fan, &mut still] {
        s.execute("iot.environment", json!({"temperature": 30, "outside_temperature": 15})).unwrap();
        add(s, "Ceiling Fan", "Fan1", 450);
        add(s, "Appliance", "Kettle1", 520);
    }
    switch(&mut fan, "Fan1", true);
    switch(&mut fan, "Kettle1", true);
    for s in [&mut fan, &mut still] {
        s.execute("sim.advance", json!({"seconds": 600})).unwrap();
    }
    assert!(room(&mut fan, "temperature") + 2 <= room(&mut still, "temperature"));
    assert_eq!(page(&mut fan, "Kettle1")["on"], true);
    assert_eq!(page(&mut still, "Kettle1")["on"], false);
}

#[test]
fn a_player_pairs_with_a_speaker_by_distance_and_a_detector_hears_it() {
    let mut s = home();
    add(&mut s, "Portable Music Player", "Player1", 450);
    add(&mut s, "Bluetooth Speaker", "Speaker1", 500);
    add(&mut s, "Sound Frequency Detector", "Mic1", 550);
    assert_eq!(page(&mut s, "Speaker1")["paired"], "Player1");
    assert_eq!(page(&mut s, "Player1")["paired"], "Speaker1");
    // Quiet until the player plays.
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    let silent = page(&mut s, "Mic1");
    assert_eq!((silent["value"].as_i64(), silent["value2"].as_i64()), (Some(0), Some(0)), "{silent}");
    switch(&mut s, "Player1", true);
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    let loud = page(&mut s, "Mic1");
    assert_eq!((loud["value"].as_i64(), loud["value2"].as_i64()), (Some(440), Some(75)), "paired: pitch in Hz, volume in dB: {loud}");
    // The speaker moves out of Bluetooth range (still in Wi-Fi range): the player plays on its own, more quietly.
    s.execute("device.move", json!({"device": "Speaker1", "x": 650, "y": 250})).unwrap();
    assert!(page(&mut s, "Speaker1")["paired"].is_null() && page(&mut s, "Player1")["paired"].is_null());
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!(page(&mut s, "Mic1")["value2"], 55);
    // The speaker on its own plays loud; everything off is silence again.
    switch(&mut s, "Speaker1", true);
    switch(&mut s, "Player1", false);
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!(page(&mut s, "Mic1")["value2"], 75);
    switch(&mut s, "Speaker1", false);
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert_eq!(page(&mut s, "Mic1")["value2"], 0);
}

#[test]
fn a_sensor_whose_name_has_two_words_still_reports_its_reading() {
    // The request is `GET path HTTP/1.1` and the server reads the second word: a space in the
    // thing's type used to cut the reading off.
    let mut s = home();
    add(&mut s, "Power Meter", "Meter1", 450);
    s.execute("iot.configure", json!({"device": "Meter1", "value": 640})).unwrap();
    let list = s.execute("iot.server", json!({"device": "Home"})).unwrap();
    let row = list["things"].as_array().unwrap().iter().find(|t| t["name"] == "Meter1").unwrap();
    assert_eq!((row["value"].as_i64(), row["what"].as_str()), (Some(640), Some("power meter")), "{list}");
}

#[test]
fn the_new_settings_refuse_the_wrong_thing_and_hostile_values() {
    let mut s = home();
    add(&mut s, "Humiture Monitor", "Humiture1", 450);
    for bad in [json!({"brightness": 50}), json!({"colour": "#ff0000"}), json!({"frequency": 100})] {
        let mut p = bad.clone();
        p["device"] = json!("Humiture1");
        assert!(s.execute("iot.configure", p).is_err(), "{bad}");
    }
    for bad in [json!({"fire": 101}), json!({"co2": 20_000}), json!({"fire": "high"})] {
        assert!(s.execute("iot.environment", bad.clone()).is_err(), "{bad}");
    }
    s.execute("iot.environment", json!({"fire": 20, "co2": 800})).unwrap();
    assert_eq!((room(&mut s, "fire"), room(&mut s, "co2")), (20, 800));
    // The lab keeps the new start values.
    let back = netlabcraft_model::file::from_json(&netlabcraft_model::file::to_json(&s.topology).unwrap()).unwrap();
    assert_eq!((back.environment.fire, back.environment.co2), (20, 800));
}
