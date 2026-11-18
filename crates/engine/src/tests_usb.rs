//! Engine tests for USB data cables and the USB stick.

use serde_json::{Value, json};

use crate::Session;

fn lab() -> Session {
    let mut s = Session::new();
    for (model, name) in [
        ("PC", "PC1"),
        ("Laptop-WiFi", "LT1"),
        ("Switch 24FE", "SW1"),
        ("Printer", "P1"),
        ("USB Storage", "Stick1"),
        ("Board SBC", "B1"),
        ("Tablet", "T1"),
    ] {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
    }
    s
}

fn listed(s: &mut Session) -> Vec<Value> {
    s.execute("link.list", json!({})).unwrap().as_array().unwrap().iter().filter(|l| l["cable"] == "usb").cloned().collect()
}

#[test]
fn a_usb_cable_joins_a_host_to_a_device_and_is_listed_and_removed() {
    let mut s = lab();
    let r = s.execute("link.add", json!({"a": "P1", "b": "PC1", "cable": "usb"})).unwrap();
    assert_eq!((r["a"].as_str(), r["b"].as_str(), r["cable"].as_str()), (Some("PC1 USB1"), Some("P1 USB"), Some("usb")));
    let r = s.execute("link.add", json!({"a": "PC1", "b": "Stick1", "cable": "usb", "a_port": "USB2"})).unwrap();
    assert_eq!(r["a"], "PC1 USB2");
    s.execute("link.add", json!({"a": "SW1", "b": "B1", "cable": "usb"})).unwrap();
    let list = listed(&mut s);
    assert_eq!(list.len(), 3, "{list:?}");
    assert!(list.iter().all(|l| l["up"] == true && l["problem"].is_null()));
    // The wrong pairs and the full or taken ports are refused.
    for bad in [
        json!({"a": "PC1", "b": "LT1", "cable": "usb"}),
        json!({"a": "P1", "b": "Stick1", "cable": "usb"}),
        json!({"a": "LT1", "b": "P1", "cable": "usb"}),
        json!({"a": "LT1", "b": "T1", "cable": "usb", "a_port": "USB3"}),
        json!({"a": "PC1", "b": "T1", "cable": "usb"}),
        json!({"a": "PC1", "b": "PC1", "cable": "usb"}),
    ] {
        assert!(s.execute("link.add", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(listed(&mut s).len(), 3, "a refused cable changes nothing");
    // A console USB cable still goes from a PC to a router or switch.
    s.execute("device.add", json!({"model": "Router 2GE", "name": "R1"})).unwrap();
    let c = s.execute("link.add", json!({"a": "LT1", "b": "R1", "cable": "usb"})).unwrap();
    assert_eq!((c["a"].as_str(), c["b"].as_str()), (Some("LT1 USB"), Some("R1 USB Console")));
    assert_eq!(s.topology.consoles.len(), 1);
    // Unplug by the device's port, by a host's numbered port.
    s.execute("link.remove", json!({"device": "P1", "port": "USB"})).unwrap();
    s.execute("link.remove", json!({"device": "PC1", "port": "USB2"})).unwrap();
    assert!(s.execute("link.remove", json!({"device": "PC1", "port": "USB2"})).is_err());
    assert_eq!(listed(&mut s).len(), 1 + 1, "the switch's cable and the console cable");
    // One undo step each, and the cable is a named cable like the others.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.usbs.len(), 2);
    let refs = s.topology.cable_refs();
    let dev = s.topology.find("Stick1").unwrap().id;
    assert!(refs.contains(&netlabcraft_model::cable_ref::CableRef::Usb(dev)));
    let info = s.topology.cable_info(netlabcraft_model::cable_ref::CableRef::Usb(dev)).unwrap();
    assert_eq!((info.a_label.as_str(), info.b_label.as_str()), ("PC1 USB2", "Stick1 USB"));
    assert_eq!(s.topology.cable_removal(netlabcraft_model::cable_ref::CableRef::Usb(dev)), Some((dev, "USB".to_string())));
}

#[test]
fn a_usb_cable_longer_than_five_metres_carries_nothing_and_a_switched_off_end_is_down() {
    let mut s = lab();
    s.execute("link.add", json!({"a": "P1", "b": "PC1", "cable": "usb"})).unwrap();
    s.execute("device.power", json!({"device": "P1", "on": false})).unwrap();
    assert_eq!(listed(&mut s)[0]["up"], false);
    s.execute("device.power", json!({"device": "P1", "on": true})).unwrap();
    // Two closets of one building are 30 m apart.
    let annex = s.execute("place.add", json!({"kind": "closet", "name": "Annex", "parent": 2})).unwrap()["id"].as_u64().unwrap();
    s.execute("device.place", json!({"device": "P1", "place": annex, "x": 10, "y": 10})).unwrap();
    let l = &listed(&mut s)[0];
    assert_eq!(l["up"], false, "{l}");
    assert!(l["problem"].as_str().is_some_and(|p| p.contains("too long")), "{l}");
}

#[test]
fn removing_a_device_unplugs_its_usb_cable_and_the_cable_survives_a_save() {
    let mut s = lab();
    s.execute("link.add", json!({"a": "P1", "b": "PC1", "cable": "usb"})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "b": "Stick1", "cable": "usb"})).unwrap();
    let text = netlabcraft_model::file::to_json(&s.topology).unwrap();
    assert_eq!(netlabcraft_model::file::from_json(&text).unwrap().usbs.len(), 2);
    s.execute("device.remove", json!({"device": "P1"})).unwrap();
    assert_eq!(s.topology.usbs.len(), 1);
    s.execute("device.remove", json!({"device": "PC1"})).unwrap();
    assert!(s.topology.usbs.is_empty());
}
