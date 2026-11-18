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

#[test]
fn a_stick_is_mounted_ejected_and_read_and_written_through_its_host() {
    let mut s = lab();
    assert!(s.execute("usb.put", json!({"device": "Stick1", "name": "a.txt", "text": "x"})).is_err(), "not plugged in");
    s.execute("link.add", json!({"a": "PC1", "b": "Stick1", "cable": "usb"})).unwrap();
    let r = s.execute("usb.put", json!({"device": "Stick1", "host": "PC1", "name": "notes.txt", "text": "hello"})).unwrap();
    assert_eq!((r["size"].as_u64(), r["files"].as_u64()), (Some(5), Some(2)));
    assert_eq!(s.execute("usb.get", json!({"device": "Stick1", "name": "NOTES.TXT"})).unwrap()["text"], "hello");
    let l = s.execute("usb.list", json!({"device": "PC1"})).unwrap();
    assert_eq!((l[0]["kind"].as_str(), l[0]["mounted"].as_bool(), l[0]["files"].as_array().map(Vec::len)), (Some("storage"), Some(true), Some(2)));
    assert_eq!(s.execute("usb.list", json!({})).unwrap().as_array().map(Vec::len), Some(1));
    // The wrong host, a bad name, a missing file.
    assert!(s.execute("usb.get", json!({"device": "Stick1", "host": "LT1", "name": "notes.txt"})).is_err());
    assert!(s.execute("usb.put", json!({"device": "Stick1", "name": "../x", "text": ""})).is_err());
    assert!(s.execute("usb.put", json!({"device": "Stick1", "name": "big", "text": "x".repeat(20_000)})).is_err());
    assert!(s.execute("usb.get", json!({"device": "Stick1", "name": "nothing"})).is_err());
    assert!(s.execute("usb.delete", json!({"device": "Stick1", "name": "nothing"})).is_err());
    assert!(s.execute("usb.put", json!({"device": "Stick1", "name": "n"})).is_err(), "`text` is required");
    // Ejected: nothing is read or written; mounting brings it back.
    assert_eq!(s.execute("usb.eject", json!({"device": "Stick1"})).unwrap()["mounted"], false);
    assert!(s.execute("usb.eject", json!({"device": "Stick1"})).is_err());
    for (id, p) in
        [("usb.get", json!({"name": "notes.txt"})), ("usb.put", json!({"name": "b", "text": "y"})), ("usb.delete", json!({"name": "notes.txt"}))]
    {
        let mut p = p;
        p["device"] = json!("Stick1");
        assert!(s.execute(id, p).unwrap_err().to_string().contains("ejected"), "{id}");
    }
    assert_eq!(s.execute("usb.list", json!({"device": "Stick1"})).unwrap()[0]["mounted"], false);
    s.execute("usb.mount", json!({"device": "Stick1"})).unwrap();
    assert!(s.execute("usb.mount", json!({"device": "Stick1"})).is_err());
    s.execute("usb.delete", json!({"device": "Stick1", "name": "notes.txt"})).unwrap();
    assert_eq!(s.execute("usb.list", json!({})).unwrap()[0]["files"].as_array().map(Vec::len), Some(1));
    // One undo step for each change.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("usb.get", json!({"device": "Stick1", "name": "notes.txt"})).unwrap()["text"], "hello");
    // Only storage is mounted; a switched off stick or a cable that is too long cannot be used.
    s.execute("link.add", json!({"a": "SW1", "b": "B1", "cable": "usb"})).unwrap();
    assert!(s.execute("usb.eject", json!({"device": "B1"})).is_err());
    let stick = s.topology.find("Stick1").unwrap().id;
    s.topology.device_mut(stick).unwrap().powered_off = true;
    assert!(s.execute("usb.get", json!({"device": "Stick1", "name": "notes.txt"})).unwrap_err().to_string().contains("powered off"));
}

#[test]
fn a_page_is_printed_over_usb_or_over_the_network() {
    let mut s = lab();
    let print = |s: &mut Session, extra: Value| {
        let mut p = json!({"from": "PC1", "printer": "P1", "text": "page"});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            p[k] = v;
        }
        s.execute("printer.print", p)
    };
    // No cable, no address: nothing reaches it.
    assert!(print(&mut s, json!({})).unwrap_err().to_string().contains("no address"));
    s.execute("link.add", json!({"a": "PC1", "b": "P1", "cable": "usb"})).unwrap();
    assert_eq!(print(&mut s, json!({})).unwrap()["via"], "usb", "the cable is used when there is one");
    assert!(print(&mut s, json!({"via": "network"})).is_err(), "the printer has no address yet");
    assert!(print(&mut s, json!({"via": "fax"})).is_err());
    assert!(print(&mut s, json!({"from": "P1", "printer": "PC1"})).is_err());
    assert!(print(&mut s, json!({"from": "LT1", "via": "usb"})).unwrap_err().to_string().contains("no USB cable"), "the printer's cable goes to PC1");
    let printer = s.topology.find("P1").unwrap().id;
    s.topology.device_mut(printer).unwrap().powered_off = true;
    assert!(print(&mut s, json!({})).is_err());
    s.topology.device_mut(printer).unwrap().powered_off = false;
    // The network: both on one subnet.
    s.execute("link.add", json!({"a": "PC1", "b": "SW1"})).unwrap();
    s.execute("link.add", json!({"a": "P1", "b": "SW1"})).unwrap();
    for (name, ip) in [("PC1", "10.9.0.2/24"), ("P1", "10.9.0.3/24")] {
        let id = s.topology.find(name).unwrap().id;
        s.topology.device_mut(id).unwrap().ports[0].ip = ip.parse::<netlabcraft_core::Ipv4Net>().ok();
    }
    s.changed();
    let r = print(&mut s, json!({"via": "network", "text": "over the wire"})).unwrap();
    assert_eq!((r["via"].as_str(), r["job"].as_u64()), (Some("network"), Some(2)));
    let jobs = s.execute("printer.jobs", json!({"printer": "P1"})).unwrap();
    assert_eq!(jobs["total"], 2);
    assert_eq!(
        (jobs["jobs"][0]["via"].as_str(), jobs["jobs"][1]["via"].as_str(), jobs["jobs"][1]["text"].as_str()),
        (Some("usb"), Some("network"), Some("over the wire"))
    );
    assert!(s.execute("printer.jobs", json!({"printer": "PC1"})).is_err());
    // Take the cable away and the page goes the network way by default.
    s.execute("link.remove", json!({"device": "P1", "port": "USB"})).unwrap();
    assert_eq!(print(&mut s, json!({})).unwrap()["via"], "network");
    // The printer's jobs are saved with the lab.
    let back = netlabcraft_model::file::from_json(&netlabcraft_model::file::to_json(&s.topology).unwrap()).unwrap();
    assert_eq!(back.find("P1").unwrap().printer.total, 3);
}

#[test]
fn a_board_is_programmed_from_a_host_over_its_usb_cable() {
    let mut s = lab();
    let p = |from: &str| json!({"device": "B1", "from": from, "source": "write(D0, 1)", "run": false});
    assert!(s.execute("board.program", p("PC1")).unwrap_err().to_string().contains("no USB cable"));
    s.execute("link.add", json!({"a": "B1", "b": "PC1", "cable": "usb"})).unwrap();
    s.execute("board.program", p("PC1")).unwrap();
    assert_eq!(s.topology.find("B1").unwrap().board.program, "write(D0, 1)");
    assert!(s.execute("board.program", p("LT1")).is_err(), "another host's cable is not this one");
    let pc = s.topology.find("PC1").unwrap().id;
    s.topology.device_mut(pc).unwrap().powered_off = true;
    assert!(s.execute("board.program", p("PC1")).unwrap_err().to_string().contains("powered off"));
    // Without a host the board is programmed as before.
    s.execute("board.program", json!({"device": "B1", "source": "write(D0, 0)"})).unwrap();
}
