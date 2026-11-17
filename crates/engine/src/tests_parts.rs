//! The components of the Actuators and Sensors groups: what a board's program reads and writes
//! through them, what they do to the variables of their place, and the settings of each.

use serde_json::{Value, json};

use crate::Session;

/// A board with every part of the list wired to a pin, by `board.wire`.
fn lab(parts: &[(&str, &str, &str)]) -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Board-SBC", "name": "Board1"})).unwrap();
    for (model, name, pin) in parts {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
        s.execute("board.wire", json!({"device": "Board1", "pin": pin, "thing": name})).unwrap();
    }
    s
}

fn program(s: &mut Session, source: &str) -> Value {
    s.execute("board.program", json!({"device": "Board1", "source": source, "run": true})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    s.execute("board.status", json!({"device": "Board1"})).unwrap()
}

fn var(s: &mut Session, device: &str, key: &str) -> Value {
    s.execute("iot.configure", json!({"device": device})).unwrap()[key].clone()
}

#[test]
fn a_program_writes_levels_and_reads_what_the_sensors_hold() {
    let mut s = lab(&[
        ("Dimmable LED", "Dim1", "D1"),
        ("RGB LED", "Rgb1", "D2"),
        ("Servo", "Servo1", "A3"),
        ("Humiture Sensor", "Hum1", "A2"),
        ("Flex Sensor", "Flex1", "A0"),
        ("Trip Wire", "Trip1", "D3"),
    ]);
    s.execute("iot.configure", json!({"device": "Hum1", "value": 22, "humidity": 63})).unwrap();
    s.execute("iot.configure", json!({"device": "Flex1", "value": 80})).unwrap();
    s.execute("iot.configure", json!({"device": "Trip1", "value": 1})).unwrap();
    let st = program(
        &mut s,
        "write(D1, 40)\nwrite(D2, rgb(255, 128, 0))\nwrite(A3, 270)\nt = read(A2)\nh = channel(A2, 1)\nf = read(A0)\ntrip = read(D3)\nservo = read(A3)\nsleep 5\n",
    );
    assert_eq!(st["state"], "running", "{st}");
    let v = &st["vars"];
    assert_eq!(
        (v["t"].clone(), v["h"].clone(), v["f"].clone(), v["trip"].clone(), v["servo"].clone()),
        (json!(22), json!(63), json!(80), json!(1), json!(180)),
        "{st}"
    );
    assert_eq!(var(&mut s, "Dim1", "level"), 40);
    assert_eq!(var(&mut s, "Rgb1", "level"), 0xff_8000);
    assert_eq!(var(&mut s, "Servo1", "level"), 180, "270 is held at the end of the servo's range");
    assert_eq!(var(&mut s, "Hum1", "second"), 63);
    // Writing 0 puts a dimmable LED out; a servo keeps its angle.
    program(&mut s, "write(D1, 0)\nwrite(A3, 0)\nsleep 5\n");
    assert_eq!(var(&mut s, "Dim1", "level"), 0);
    assert_eq!(var(&mut s, "Dim1", "on"), false);
    assert_eq!(var(&mut s, "Servo1", "level"), 0);
    // Hand settings: brightness, angle and the red, green and blue channels.
    s.execute("iot.configure", json!({"device": "Dim1", "level": 65})).unwrap();
    assert_eq!((var(&mut s, "Dim1", "level"), var(&mut s, "Dim1", "on")), (json!(65), json!(true)));
    s.execute("iot.configure", json!({"device": "Rgb1", "red": 1, "green": 2, "blue": 3})).unwrap();
    assert_eq!(var(&mut s, "Rgb1", "level"), 0x01_0203);
    // The program reads a dimmable LED's level on an analog pin and its on/off state on a digital one.
    let st = program(&mut s, "write(D1, 30)\nx = read(D1)\nsleep 5\n");
    assert_eq!(st["vars"]["x"], 1, "{st}");
    // A pin that is not wired has no second channel, and a sensor with none refuses it.
    let st = program(&mut s, "x = channel(A1, 1)\n");
    assert!(st["message"].as_str().unwrap_or_default().contains("no channel 1"), "{st}");
    let st = program(&mut s, "x = channel(A0, 1)\n");
    assert!(st["message"].as_str().unwrap_or_default().contains("has no channel 1"), "{st}");
    let st = program(&mut s, "write(A0, 1)\n");
    assert!(st["message"].as_str().unwrap_or_default().contains("a sensor"), "{st}");
}

#[test]
fn the_switches_the_flags_and_the_settings_refuse_what_does_not_fit() {
    let mut s = lab(&[]);
    for (model, name) in [
        ("Push Button Toggle Switch", "Latch1"),
        ("Rocker Switch", "Rocker1"),
        ("Toggle Push Button", "Push1"),
        ("Metal Sensor", "Metal1"),
        ("Membrane Potentiometer", "Strip1"),
        ("Generic Sensor", "Raw1"),
        ("Generic Environment Sensor", "Env1"),
        ("Dimmable LED", "Dim1"),
        ("Alarm", "Alarm1"),
    ] {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
    }
    // A latching switch and a toggle push button flip at each press; a rocker is set to a position.
    for name in ["Latch1", "Push1"] {
        assert_eq!(var(&mut s, name, "value"), 0);
        s.execute("iot.configure", json!({"device": name, "press": true})).unwrap();
        assert_eq!(var(&mut s, name, "value"), 1, "{name}");
        s.execute("iot.configure", json!({"device": name, "press": true})).unwrap();
        assert_eq!(var(&mut s, name, "value"), 0, "{name}");
    }
    s.execute("iot.configure", json!({"device": "Rocker1", "value": 1})).unwrap();
    assert_eq!(var(&mut s, "Rocker1", "value"), 1);
    for bad in [
        json!({"device": "Rocker1", "value": 2}),
        json!({"device": "Rocker1", "press": true}),
        json!({"device": "Metal1", "value": -1}),
        json!({"device": "Strip1", "value": 101}),
        json!({"device": "Raw1", "value": 1001}),
        json!({"device": "Raw1", "humidity": 5}),
        json!({"device": "Raw1", "variable": "gravity"}),
        json!({"device": "Raw1", "level": 5}),
        json!({"device": "Env1", "variable": "no such variable"}),
        json!({"device": "Env1", "variable": 7}),
        json!({"device": "Dim1", "level": 101}),
        json!({"device": "Dim1", "level": -1}),
        json!({"device": "Dim1", "red": 5}),
        json!({"device": "Dim1", "value": 5}),
        json!({"device": "Alarm1", "level": 5}),
    ] {
        assert!(s.execute("iot.configure", bad.clone()).is_err(), "{bad}");
    }
    s.execute("iot.configure", json!({"device": "Metal1", "value": 1})).unwrap();
    s.execute("iot.configure", json!({"device": "Strip1", "value": 35})).unwrap();
    s.execute("iot.configure", json!({"device": "Raw1", "value": -400})).unwrap();
    assert_eq!((var(&mut s, "Metal1", "value"), var(&mut s, "Strip1", "value"), var(&mut s, "Raw1", "value")), (json!(1), json!(35), json!(-400)));
    // The variable a generic environment sensor reads is picked by id or by name, and saved with the lab.
    assert_eq!(var(&mut s, "Env1", "variable"), "ambient_temperature");
    s.execute("iot.configure", json!({"device": "Env1", "variable": "Wind speed"})).unwrap();
    assert_eq!(var(&mut s, "Env1", "variable"), "wind_speed");
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut back = Session::new();
    back.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(back.topology, s.topology);
    assert_eq!(var(&mut back, "Env1", "variable"), "wind_speed");
    assert_eq!(var(&mut back, "Strip1", "value"), 35);
}

#[test]
fn sensors_read_the_variables_of_their_place_and_running_parts_change_them() {
    let mut s = lab(&[
        ("Air Cooler", "Cooler1", "D0"),
        ("Heating Element", "Heater1", "D1"),
        ("Ceiling Sprinkler", "Spray1", "D2"),
        ("Floor Sprinkler", "Spray2", "D3"),
        ("Speaker", "Speaker1", "D4"),
        ("Alarm", "Alarm1", "D5"),
        ("Temperature Sensor", "Temp1", "A0"),
        ("Humidity Sensor", "Humid1", "A1"),
        ("Generic Environment Sensor", "Env1", "A2"),
        ("Water Detector", "Wet1", "D6"),
    ]);
    s.execute("environment.set", json!({"place": "intercity", "variable": "ambient_temperature", "value": 30})).unwrap();
    s.execute("environment.set", json!({"place": "intercity", "variable": "humidity", "value": 40})).unwrap();
    s.execute("environment.set", json!({"place": "intercity", "variable": "fire", "value": 80})).unwrap();
    let read = |s: &mut Session, name: &str, variable: &str| {
        s.execute("iot.configure", json!({"device": name, "variable": variable})).unwrap();
        var(s, name, "value")
    };
    assert_eq!((var(&mut s, "Temp1", "value"), var(&mut s, "Humid1", "value"), read(&mut s, "Env1", "fire")), (json!(30), json!(40), json!(80)));
    assert_eq!(var(&mut s, "Wet1", "value"), 0, "no water anywhere: the detector reads its own number");
    // The air cooler lowers the temperature and the humidity while its pin is high, and only then.
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    assert_eq!((var(&mut s, "Temp1", "value"), var(&mut s, "Humid1", "value")), (json!(25), json!(30)));
    program(&mut s, "write(D0, 0)\nsleep 9\n");
    assert_eq!((var(&mut s, "Temp1", "value"), var(&mut s, "Humid1", "value")), (json!(30), json!(40)));
    // The heating element raises the temperature.
    program(&mut s, "write(D1, 1)\nsleep 9\n");
    assert_eq!(var(&mut s, "Temp1", "value"), 40);
    // A sprinkler wets the air, adds water and puts the flames out; the detector sees the water.
    program(&mut s, "write(D1, 0)\nwrite(D2, 1)\nsleep 9\n");
    assert_eq!(var(&mut s, "Humid1", "value"), 70);
    assert_eq!(read(&mut s, "Env1", "fire"), 0);
    assert_eq!(read(&mut s, "Env1", "rain"), 2);
    assert_eq!(read(&mut s, "Env1", "water_level"), 1);
    assert_eq!(var(&mut s, "Wet1", "value"), 1);
    program(&mut s, "write(D2, 0)\nwrite(D3, 1)\nsleep 9\n");
    assert_eq!(read(&mut s, "Env1", "water_level"), 3);
    assert_eq!(read(&mut s, "Env1", "soil_moisture"), 70);
    assert_eq!(var(&mut s, "Humid1", "value"), 60);
    // A speaker and an alarm make sound.
    assert_eq!(read(&mut s, "Env1", "sound_volume"), 0);
    program(&mut s, "write(D3, 0)\nwrite(D4, 1)\nsleep 9\n");
    assert_eq!((read(&mut s, "Env1", "sound_volume"), read(&mut s, "Env1", "sound_pitch")), (json!(70), json!(440)));
    program(&mut s, "write(D5, 1)\nsleep 9\n");
    assert_eq!((read(&mut s, "Env1", "sound_volume"), read(&mut s, "Env1", "sound_pitch")), (json!(100), json!(1000)));
    // With everything off the place reads what it sets again.
    program(&mut s, "write(D4, 0)\nwrite(D5, 0)\nsleep 9\n");
    assert_eq!((read(&mut s, "Env1", "sound_volume"), var(&mut s, "Wet1", "value")), (json!(0), json!(0)));
    // A variable no place sets reads its default.
    assert_eq!(read(&mut s, "Env1", "oxygen"), 21);
    assert_eq!(read(&mut s, "Env1", "gravity"), 10);
}

#[test]
fn a_part_only_acts_on_the_sensors_of_its_own_place() {
    let mut s = lab(&[("Heating Element", "Heater1", "D0"), ("Temperature Sensor", "Temp1", "A0")]);
    let city = s.execute("place.add", json!({"kind": "city", "name": "Harbor"})).unwrap()["id"].as_u64().unwrap();
    let building = s.execute("place.add", json!({"kind": "building", "name": "Annex", "parent": city})).unwrap()["id"].as_u64().unwrap();
    let a = s.execute("place.add", json!({"kind": "closet", "name": "Closet A", "parent": building})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("place.add", json!({"kind": "closet", "name": "Closet B", "parent": building})).unwrap()["id"].as_u64().unwrap();
    s.execute("device.place", json!({"devices": ["Heater1"], "place": a})).unwrap();
    s.execute("device.place", json!({"devices": ["Temp1"], "place": b})).unwrap();
    s.execute("environment.set", json!({"place": "intercity", "variable": "ambient_temperature", "value": 20})).unwrap();
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    assert_eq!(var(&mut s, "Temp1", "value"), 20, "the heating element is in another closet");
    s.execute("device.place", json!({"devices": ["Temp1"], "place": a})).unwrap();
    assert_eq!(var(&mut s, "Temp1", "value"), 30, "now they share a closet");
    s.execute("device.remove", json!({"device": "Heater1"})).unwrap();
    assert_eq!(var(&mut s, "Temp1", "value"), 20);
}

#[test]
fn without_an_environment_a_part_still_moves_the_reading_and_the_room_keeps_its_own_physics() {
    let mut s = lab(&[("Heating Element", "Heater1", "D0"), ("Temperature Sensor", "Temp1", "A0"), ("Motion Sensor", "Motion1", "D1")]);
    s.execute("iot.configure", json!({"device": "Temp1", "value": 18})).unwrap();
    assert_eq!(var(&mut s, "Temp1", "value"), 18, "its own number");
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    assert_eq!(var(&mut s, "Temp1", "value"), 25, "the 15 degree default plus the element's 10");
    program(&mut s, "write(D0, 0)\nsleep 9\n");
    assert_eq!(var(&mut s, "Temp1", "value"), 18);
    // With the smart room's physics running the room answers, and the element warms it.
    s.execute("iot.environment", json!({"enabled": true, "temperature": 20, "outside_temperature": 20})).unwrap();
    s.execute("iot.configure", json!({"device": "Motion1", "value": 1})).unwrap();
    assert_eq!(var(&mut s, "Motion1", "value"), 1, "the motion sensor reads the room's motion");
    program(&mut s, "write(D0, 1)\nsleep 9\n");
    s.execute("sim.advance", json!({"seconds": 600})).unwrap();
    let room = s.execute("iot.environment", json!({})).unwrap()["room"]["temperature"].as_i64().unwrap();
    assert!(room > 20, "ten minutes of heating warm the room: {room}");
    assert_eq!(var(&mut s, "Temp1", "value"), room);
}

#[test]
fn every_part_can_be_wired_read_or_written_and_survives_hostile_settings() {
    let parts: Vec<_> = netlabcraft_model::models::MODELS
        .iter()
        .filter(|m| matches!(m.group(), netlabcraft_model::models::Group::Actuators | netlabcraft_model::models::Group::Sensors))
        .collect();
    assert!(parts.len() >= 36, "{} parts", parts.len());
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Board-SBC", "name": "Board1"})).unwrap();
    for (i, m) in parts.iter().enumerate() {
        let name = format!("P{i}");
        s.execute("device.add", json!({"model": m.id, "name": name})).unwrap();
        // One thing to a pin; the SBC has twelve, the rest are checked with an IoT custom cable to a second board.
        let board = if i < 12 { "Board1".to_string() } else { format!("Board{}", i / 12 + 1) };
        if s.topology.find(&board).is_none() {
            s.execute("device.add", json!({"model": "Board-SBC", "name": board})).unwrap();
        }
        let pin = if i % 12 < 8 { format!("D{}", i % 12) } else { format!("A{}", i % 12 - 8) };
        s.execute("board.wire", json!({"device": board, "pin": pin, "thing": name})).unwrap();
        for hostile in
            [json!({"value": 1_000_000}), json!({"level": 99_999_999}), json!({"humidity": -1}), json!({"variable": ""}), json!({"red": 300})]
        {
            let mut p = hostile;
            p["device"] = json!(name);
            let _ = s.execute("iot.configure", p);
        }
    }
    // Reading the status of every board reads every part.
    for n in 1..=parts.len().div_ceil(12) {
        let st = s.execute("board.status", json!({"device": format!("Board{n}")})).unwrap();
        assert!(st["pins"].as_array().is_some_and(|p| p.iter().any(|x| x["wired"].is_string())), "{st}");
    }
    s.execute("iot.sync", json!({})).unwrap();
    s.execute("sim.advance", json!({"seconds": 20})).unwrap();
}
