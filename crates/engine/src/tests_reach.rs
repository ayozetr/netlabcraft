//! How far each cable reaches: serial, console, USB and octal cables join the media that already
//! had a limit, and `link.info` says what the limit is and which standard it follows.

use serde_json::json;

use crate::Session;

/// A router with a serial card whose two serial ports are cabled to each other.
fn serial_lab() -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Router-M2", "name": "R1"})).unwrap();
    s.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    s.execute("device.module.add", json!({"device": "R1", "slot": "0/0", "module": "Card-2S"})).unwrap();
    s.execute("device.power", json!({"device": "R1", "on": true})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "s0/0/0", "b": "R1", "b_port": "s0/0/1", "cable": "serial-dte"})).unwrap();
    s
}

#[test]
fn a_serial_cable_reaches_twelve_hundred_metres() {
    let mut s = serial_lab();
    let v = s.execute("link.length", json!({"device": "R1", "port": "s0/0/0", "meters": 1_000})).unwrap();
    assert!(v["problem"].is_null(), "{v}");
    let v = s.execute("link.length", json!({"device": "R1", "port": "s0/0/0", "meters": 1_300})).unwrap();
    assert_eq!(v["problem"], "cable too long: 1300 m serial, max 1200 m");
    let info = s.execute("link.info", json!({"device": "R1", "port": "s0/0/0"})).unwrap();
    assert_eq!(info["reach"]["max_m"], 1200);
    assert!(info["reach"]["standard"].as_str().unwrap().contains("V.11"), "{info}");
    assert_eq!(info["problem"], "cable too long: 1300 m serial, max 1200 m");
    // The shortcut that ignores lengths keeps it up.
    s.execute("link.length", json!({"device": "R1", "port": "s0/0/0", "meters": null})).unwrap();
    let info = s.execute("link.info", json!({"device": "R1", "port": "s0/0/0"})).unwrap();
    assert!(info["problem"].is_null(), "{info}");
}

#[test]
fn every_medium_has_a_reach_and_a_standard() {
    use netlabcraft_model::cable::Cable;
    for (cable, max) in [
        (Cable::Straight, 100),
        (Cable::Cross, 100),
        (Cable::FiberMulti, 550),
        (Cable::FiberSingle, 10_000),
        (Cable::Coaxial, 500),
        (Cable::Phone, 5_000),
        (Cable::SerialDce, 1_200),
        (Cable::SerialDte, 1_200),
        (Cable::Console, 15),
        (Cable::Octal, 15),
        (Cable::Usb, 5),
    ] {
        let r = cable.reach().unwrap();
        assert_eq!(r.meters, max, "{}", cable.id());
        assert!(!r.standard.is_empty() && !r.what.is_empty());
    }
    assert!(Cable::Auto.reach().is_none() && Cable::IotCustom.reach().is_none());
}

#[test]
fn a_console_usb_or_octal_cable_longer_than_its_standard_carries_nothing() {
    let mut s = Session::new();
    for (name, model) in [("R1", "Router-M2"), ("R2", "Router-3"), ("R3", "Router-3"), ("PC1", "PC"), ("L1", "Laptop")] {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
    }
    s.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    s.execute("device.module.add", json!({"device": "R1", "slot": "0/1", "module": "Card-8A"})).unwrap();
    s.execute("device.power", json!({"device": "R1", "on": true})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "b": "R2", "cable": "console"})).unwrap();
    s.execute("link.add", json!({"a": "L1", "b": "R1", "cable": "usb"})).unwrap();
    s.execute("link.add", json!({"a": "R1", "b": "R3", "cable": "octal"})).unwrap();
    // Everything stands in one closet: 3 m, every cable reaches.
    assert!(s.execute("link.list", json!({})).unwrap().as_array().unwrap().iter().all(|l| l["problem"].is_null()));
    // A building 230 m away: the console and the USB cable are too long.
    let b = s.execute("place.add", json!({"kind": "building", "name": "Workshop", "x": 500, "y": 220})).unwrap()["id"].as_u64().unwrap();
    s.execute("device.place", json!({"device": "PC1", "place": b})).unwrap();
    s.execute("device.place", json!({"device": "L1", "place": b})).unwrap();
    let far = s.execute("place.add", json!({"kind": "closet", "parent": b})).unwrap()["id"].as_u64().unwrap();
    s.execute("device.place", json!({"device": "R3", "place": far})).unwrap();
    let list = s.execute("link.list", json!({})).unwrap();
    for l in list.as_array().unwrap() {
        let cable = l["cable"].as_str().unwrap();
        let problem = l["problem"].as_str().unwrap_or_default();
        match cable {
            "console" => assert!(problem.contains("m console, max 15 m"), "{l}"),
            "usb" => assert!(problem.contains("m USB, max 5 m"), "{l}"),
            "octal" => assert!(problem.contains("m octal, max 15 m"), "{l}"),
            _ => {}
        }
        assert_eq!(l["up"], false, "{l}");
    }
    let e = s.execute("pc.terminal", json!({"pc": "PC1"})).unwrap_err().to_string();
    assert!(e.contains("cable too long"), "{e}");
    let e = s.execute("octal.terminal", json!({"router": "R1", "line": 0})).unwrap_err().to_string();
    assert!(e.contains("cable too long"), "{e}");
    // Back in the closet it opens again.
    s.execute("device.place", json!({"device": "PC1", "place": 3})).unwrap();
    assert_eq!(s.execute("pc.terminal", json!({"pc": "PC1"})).unwrap()["device"], "R2");
}
