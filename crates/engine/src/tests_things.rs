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

#[test]
fn the_city_sensors_read_the_room_and_what_a_place_sets() {
    let mut s = home();
    for (model, name, x) in [
        ("Atmospheric Pressure Monitor", "Baro1", 450),
        ("Wind Detector", "Wind1", 500),
        ("Carbon Dioxide Detector", "CO2a", 550),
        ("Fire Monitor", "Fire1", 600),
    ] {
        add(&mut s, model, name, x);
    }
    // The room is at sea-level pressure, calm, with fresh air and no flames.
    let read = |s: &mut Session, name: &str| page(s, name)["value"].as_i64().unwrap();
    assert_eq!([read(&mut s, "Baro1"), read(&mut s, "Wind1"), read(&mut s, "CO2a"), read(&mut s, "Fire1")], [101, 0, 420, 0]);
    // A reading set by hand sets the room.
    s.execute("iot.configure", json!({"device": "Wind1", "value": 35})).unwrap();
    assert_eq!(read(&mut s, "Wind1"), 35);
    s.execute("iot.environment", json!({"fire": 40, "co2": 900})).unwrap();
    // (Simulated time passes a little with every report, and fresh air thins the carbon dioxide.)
    assert_eq!(read(&mut s, "Fire1"), 40);
    assert!((read(&mut s, "CO2a") - 900).abs() <= 5);
    // A place that sets a variable decides for the sensors in it: the share of carbon dioxide is
    // a percentage and the sensor reads parts per million.
    s.execute("environment.set", json!({"place": "intercity", "variable": "carbon_dioxide", "value": 0.1})).unwrap();
    s.execute("environment.set", json!({"place": "intercity", "variable": "atmospheric_pressure", "value": 95.4})).unwrap();
    s.execute("environment.set", json!({"place": "intercity", "variable": "wind_speed", "value": 60})).unwrap();
    s.execute("environment.set", json!({"place": "intercity", "variable": "fire", "value": 12})).unwrap();
    assert_eq!([read(&mut s, "Baro1"), read(&mut s, "Wind1"), read(&mut s, "CO2a"), read(&mut s, "Fire1")], [95, 60, 1000, 12]);
    // The Environments window shows the room's carbon dioxide as a percentage too.
    s.execute("environment.clear", json!({"place": "intercity"})).unwrap();
    let g = s.execute("environment.get", json!({"place": "intercity"})).unwrap();
    let co2 = g["variables"].as_array().unwrap().iter().find(|v| v["id"] == "carbon_dioxide").unwrap();
    assert_eq!(co2["source"], "room");
    assert!((co2["value"].as_f64().unwrap() - 0.09).abs() < 0.001, "about 900 ppm as a percentage: {co2}");
}

#[test]
fn a_running_car_fills_the_room_with_carbon_monoxide_and_dioxide_and_a_trip_sensor_sees_it() {
    let mut s = home();
    add(&mut s, "Old Car", "Car1", 450);
    add(&mut s, "Carbon Dioxide Detector", "CO2a", 520);
    add(&mut s, "CO Detector", "CO1", 560);
    add(&mut s, "Trip Sensor", "Trip1", 600);
    let value = |s: &mut Session, name: &str| page(s, name)["value"].as_i64().unwrap();
    assert_eq!(value(&mut s, "CO1"), 0);
    // The car is 150 units, 83 m, from the trip sensor: out of its 50 m.
    assert_eq!(value(&mut s, "Trip1"), 0);
    switch(&mut s, "Car1", true);
    s.execute("sim.advance", json!({"seconds": 300})).unwrap();
    assert!(value(&mut s, "CO1") > 100, "carbon monoxide: {}", value(&mut s, "CO1"));
    assert!(value(&mut s, "CO2a") > 1_000, "carbon dioxide: {}", value(&mut s, "CO2a"));
    // The engine off, the car adds nothing more.
    switch(&mut s, "Car1", false);
    let (co, co2) = (value(&mut s, "CO1"), value(&mut s, "CO2a"));
    s.execute("sim.advance", json!({"seconds": 300})).unwrap();
    assert!(value(&mut s, "CO1") <= co && value(&mut s, "CO2a") <= co2);
    // Drive it up to the sensor: it is seen, and not seen again once it has gone.
    s.execute("device.move", json!({"device": "Car1", "x": 640, "y": 250})).unwrap();
    assert_eq!(value(&mut s, "Trip1"), 1);
    s.execute("device.move", json!({"device": "Car1", "x": 100, "y": 250})).unwrap();
    assert_eq!(value(&mut s, "Trip1"), 0);
    // A tablet walking past is seen too; one far away is not.
    s.execute("device.add", json!({"model": "Tablet", "name": "Tab1", "x": 620, "y": 250})).unwrap();
    assert_eq!(value(&mut s, "Trip1"), 1);
    s.execute("device.move", json!({"device": "Tab1", "x": 2_000, "y": 250})).unwrap();
    assert_eq!(value(&mut s, "Trip1"), 0);
}

#[test]
fn a_fire_sprinkler_sprays_on_its_own_while_there_are_flames_and_puts_them_out() {
    let mut s = home();
    add(&mut s, "Fire Sprinkler", "Hose1", 450);
    add(&mut s, "Fire Monitor", "Fire1", 520);
    assert_eq!(page(&mut s, "Hose1")["active"], false, "no flames: it waits");
    s.execute("iot.environment", json!({"fire": 40})).unwrap();
    assert_eq!(page(&mut s, "Hose1")["active"], true, "flames: it sprays although the server never switched it on");
    assert_eq!(page(&mut s, "Hose1")["on"], false);
    assert!((34..=40).contains(&page(&mut s, "Fire1")["value"].as_i64().unwrap()), "the flames have hardly begun to go down");
    s.execute("sim.advance", json!({"seconds": 30})).unwrap();
    assert_eq!(page(&mut s, "Fire1")["value"], 0, "the flames are out");
    assert_eq!(page(&mut s, "Hose1")["active"], false);
    // Without it, the flames burn on and make smoke.
    let mut burning = home();
    burning.execute("iot.environment", json!({"fire": 40})).unwrap();
    burning.execute("sim.advance", json!({"seconds": 30})).unwrap();
    assert_eq!(room(&mut burning, "fire"), 40);
    assert!(room(&mut burning, "smoke") > 0);
    // The server can switch it on by hand too.
    switch(&mut s, "Hose1", true);
    assert_eq!(page(&mut s, "Hose1")["active"], true);
}

#[test]
fn an_rfid_reader_reads_the_nearest_card_in_range_and_a_beacon_announces_its_id() {
    let mut s = home();
    add(&mut s, "RFID Reader", "Reader1", 450);
    add(&mut s, "RFID Card", "CardA", 480);
    add(&mut s, "RFID Card", "CardB", 800);
    add(&mut s, "Beacon", "Beacon1", 520);
    let page_of = |s: &mut Session, n: &str| page(s, n);
    // A card announces a unique id until one is set by hand.
    let (a, b) = (page_of(&mut s, "CardA")["value"].as_i64().unwrap(), page_of(&mut s, "CardB")["value"].as_i64().unwrap());
    assert!(a > 0 && b > 0 && a != b, "{a} {b}");
    // CardA is 30 units (17 m) away: read. CardB is 350 units (194 m): not.
    assert_eq!(page_of(&mut s, "Reader1")["value"].as_i64(), Some(a));
    s.execute("iot.configure", json!({"device": "CardA", "value": 4242})).unwrap();
    assert_eq!(page_of(&mut s, "Reader1")["value"], 4242);
    // Moved out of the 30 m (54 units) range, it is not read; CardB brought in, it is.
    s.execute("device.move", json!({"device": "CardA", "x": 600, "y": 250})).unwrap();
    assert_eq!(page_of(&mut s, "Reader1")["value"], 0);
    s.execute("device.move", json!({"device": "CardB", "x": 470, "y": 250})).unwrap();
    assert_eq!(page_of(&mut s, "Reader1")["value"].as_i64(), Some(b));
    // The beacon reports its own id and how many smart things hear it (within 100 m, 180 units).
    s.execute("iot.configure", json!({"device": "Beacon1", "value": 77})).unwrap();
    let beacon = page_of(&mut s, "Beacon1");
    assert_eq!((beacon["value"].as_i64(), beacon["value2"].as_i64()), (Some(77), Some(3)), "the reader and both cards hear it: {beacon}");
    s.execute("device.move", json!({"device": "Beacon1", "x": 2_500, "y": 250})).unwrap();
    assert_eq!(page_of(&mut s, "Beacon1")["value2"], 0);
    // The server hears each reading through the usual report.
    let list = s.execute("iot.server", json!({"device": "Home"})).unwrap();
    assert!(list["things"].as_array().unwrap().iter().any(|t| t["name"] == "Reader1" && t["value"] == b), "{list}");
}

#[test]
fn a_smart_led_takes_a_brightness_and_a_colour_and_lights_the_room() {
    let mut s = home();
    add(&mut s, "Smart LED", "Led1", 450);
    let p = page(&mut s, "Led1");
    assert_eq!((p["brightness"].as_i64(), p["colour"].as_str()), (Some(100), Some("#ffe4b5")), "{p}");
    let before = room(&mut s, "light");
    switch(&mut s, "Led1", true);
    s.execute("iot.configure", json!({"device": "Led1", "brightness": 40, "colour": "#3070e0"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    let p = page(&mut s, "Led1");
    assert_eq!((p["brightness"].as_i64(), p["colour"].as_str(), p["active"].as_bool()), (Some(40), Some("#3070e0"), Some(true)), "{p}");
    assert!(room(&mut s, "light") > before);
    for bad in [
        json!({"brightness": 101}),
        json!({"brightness": -1}),
        json!({"colour": "red"}),
        json!({"colour": "#12345"}),
        json!({"colour": "#12345é"}),
        json!({"colour": 5}),
    ] {
        let mut p = bad.clone();
        p["device"] = json!("Led1");
        assert!(s.execute("iot.configure", p).is_err(), "{bad}");
    }
    // The settings are saved with the lab, and a thing that never set them writes nothing.
    let back = netlabcraft_model::file::from_json(&netlabcraft_model::file::to_json(&s.topology).unwrap()).unwrap();
    let led = back.find("Led1").unwrap();
    assert_eq!((led.iot.brightness, led.iot.colour), (Some(40), Some([0x30, 0x70, 0xe0])));
    assert!(back.find("Home").unwrap().iot.brightness.is_none());
}
