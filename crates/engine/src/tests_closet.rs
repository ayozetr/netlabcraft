//! Engine tests for wiring-closet furniture, patch panels and power distribution devices.

use serde_json::{Value, json};

use crate::Session;

/// A lab with two switches and a router, in the default closet.
fn lab() -> (Session, String, String, String) {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Switch 24FE"})).unwrap();
    s.execute("device.add", json!({"model": "Switch 24FE"})).unwrap();
    s.execute("device.add", json!({"model": "Router 2GE"})).unwrap();
    let n = |i: usize| s.topology.devices[i].name.clone();
    let names = (n(0), n(1), n(2));
    (s, names.0, names.1, names.2)
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

#[test]
fn furniture_is_added_mounted_moved_and_undone() {
    let (mut s, a, b, _) = lab();
    let r = s.execute("furniture.add", json!({"kind": "rack", "size": 8})).unwrap();
    assert_eq!(r["name"], "Rack0");
    assert_eq!(r["size"], 8);
    let rack = id(&r);
    let shelf = id(&s.execute("furniture.add", json!({"kind": "shelf", "name": "Top"})).unwrap());
    let table = id(&s.execute("furniture.add", json!({"kind": "table"})).unwrap());
    assert!(s.execute("furniture.add", json!({"kind": "sofa"})).is_err());
    assert!(s.execute("furniture.add", json!({"kind": "rack", "size": 2})).is_err());
    assert!(s.execute("furniture.add", json!({"kind": "rack", "place": 1})).is_err(), "a city has no furniture");
    // A rack stacks by units, counted from 1 at the top.
    let m = s.execute("furniture.mount", json!({"device": a, "furniture": "rack0", "slot": 2, "units": 2})).unwrap();
    assert_eq!((m["slot"].as_u64(), m["units"].as_u64()), (Some(2), Some(2)));
    assert!(s.execute("furniture.mount", json!({"device": b, "furniture": rack, "slot": 3})).is_err(), "unit 3 is taken");
    assert!(s.execute("furniture.mount", json!({"device": b, "furniture": rack, "slot": 0})).is_err(), "units count from 1");
    s.execute("furniture.mount", json!({"device": b, "furniture": "Top", "slot": 1})).unwrap();
    let list = s.execute("closet.list", json!({})).unwrap();
    let racks = list["furniture"].as_array().unwrap();
    assert_eq!(racks.len(), 3);
    assert_eq!(racks[0]["items"][0]["device"], json!(a));
    assert_eq!(racks[0]["items"][0]["slot"], 2);
    assert!(list["loose"].as_array().unwrap().iter().all(|n| *n != json!(a) && *n != json!(b)));
    // Moving, renaming and removing.
    s.execute("furniture.move", json!({"furniture": table, "x": 500, "y": 40})).unwrap();
    s.execute("furniture.rename", json!({"furniture": table, "name": "Bench"})).unwrap();
    assert_eq!(s.topology.furniture_piece(table as u32).unwrap().name, "Bench");
    assert!(s.execute("furniture.move", json!({"furniture": table, "x": 5})).is_err());
    s.execute("furniture.unmount", json!({"device": b})).unwrap();
    assert!(s.execute("furniture.unmount", json!({"device": b})).is_err());
    s.execute("furniture.remove", json!({"furniture": shelf})).unwrap();
    assert!(s.execute("furniture.remove", json!({"furniture": shelf})).is_err());
    // Everything is one undo step.
    assert!(s.topology.mount_of(netlabcraft_model::furniture::Item::Device(s.topology.find(&a).unwrap().id)).is_some());
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.furniture_piece(shelf as u32).is_some());
    for _ in 0..20 {
        if !s.can_undo() {
            break;
        }
        s.execute("edit.undo", json!({})).unwrap();
    }
    assert!(s.topology.physical.furniture.is_empty());
}

#[test]
fn a_patch_path_cables_two_devices_through_a_panel() {
    let (mut s, a, b, _) = lab();
    let p = id(&s.execute("passive.add", json!({"kind": "copper-patch-panel"})).unwrap());
    let q = id(&s.execute("passive.add", json!({"kind": "copper-patch-panel", "name": "Floor 2", "ports": 8})).unwrap());
    let end = |dev: &str, port: &str| json!({"device": dev, "port": port});
    let pan = |f: u64, port: u32| json!({"panel": f, "port": port});
    s.execute("patch.wire", json!({"a": end(&a, "Fa0/1"), "b": pan(p, 3)})).unwrap();
    s.execute("patch.wire", json!({"a": end(&b, "Fa0/1"), "b": pan(q, 8)})).unwrap();
    assert!(s.topology.links.is_empty());
    assert!(s.execute("patch.wire", json!({"a": pan(q, 9), "b": pan(p, 1)})).is_err(), "no port 9");
    assert!(s.execute("patch.wire", json!({"a": pan(q, 0), "b": pan(p, 1)})).is_err(), "ports count from 1");
    assert!(s.execute("patch.wire", json!({"a": end(&a, "Fa0/2"), "b": end(&b, "Fa0/2")})).is_err(), "a plain cable");
    assert!(s.execute("patch.wire", json!({"a": end(&a, "Nope"), "b": pan(q, 1)})).is_err());
    let r = s.execute("patch.wire", json!({"a": pan(p, 3), "b": pan(q, 8)})).unwrap();
    assert_eq!(r["cables"].as_array().unwrap().len(), 1);
    assert_eq!(s.topology.links.len(), 1);
    let list = s.execute("closet.list", json!({})).unwrap();
    assert_eq!(list["paths"].as_array().unwrap().len(), 1);
    assert_eq!(list["wires"].as_array().unwrap().len(), 3);
    // The devices behave as if cabled directly.
    assert_eq!(s.topology.cable_length(&s.topology.links[0]).meters, 3);
    // Undo takes the cable and the cord out together; wires at a port come out with it.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.links.is_empty());
    s.execute("edit.redo", json!({})).unwrap();
    s.execute("patch.unwire", json!({"a": pan(q, 8)})).unwrap();
    assert!(s.topology.links.is_empty());
    assert!(s.execute("patch.unwire", json!({"a": pan(q, 8)})).is_err());
    assert!(s.topology.physical.patch_wires.len() == 1);
    // Removing a panel takes its wires.
    s.execute("passive.remove", json!({"passive": p})).unwrap();
    assert!(s.topology.physical.patch_wires.is_empty());
}

#[test]
fn wall_mounts_and_fiber_panels_keep_to_their_medium() {
    let (mut s, a, b, _) = lab();
    let w1 = id(&s.execute("passive.add", json!({"kind": "copper-wall-mount"})).unwrap());
    let w2 = id(&s.execute("passive.add", json!({"kind": "copper-wall-mount", "ports": 1})).unwrap());
    let f = id(&s.execute("passive.add", json!({"kind": "fiber-patch-panel"})).unwrap());
    let fw = id(&s.execute("passive.add", json!({"kind": "fiber-wall-mount"})).unwrap());
    assert_eq!(s.topology.fitting(fw as u32).unwrap().name, "Fiber Wall Mount0");
    let end = |dev: &str, port: &str| json!({"device": dev, "port": port});
    let pan = |f: u64, port: u32| json!({"panel": f, "port": port});
    assert!(s.execute("patch.wire", json!({"a": pan(w1, 1), "b": pan(f, 1)})).is_err(), "copper to fiber");
    assert!(s.execute("patch.wire", json!({"a": end(&a, "Fa0/1"), "b": pan(fw, 1)})).is_err(), "a copper port on a fiber outlet");
    s.execute("patch.wire", json!({"a": end(&a, "Fa0/1"), "b": pan(w1, 1)})).unwrap();
    s.execute("patch.wire", json!({"a": end(&b, "Fa0/1"), "b": pan(w2, 1)})).unwrap();
    s.execute("patch.wire", json!({"a": pan(w1, 1), "b": pan(w2, 1)})).unwrap();
    assert_eq!(s.topology.links.len(), 1);
    assert!(s.execute("passive.add", json!({"kind": "copper-wall-mount", "ports": 9})).is_err());
}

#[test]
fn furniture_is_resized_by_command_and_one_undo_takes_it_back() {
    let (mut s, a, _, _) = lab();
    let rack = id(&s.execute("furniture.add", json!({"kind": "rack", "size": 8})).unwrap());
    s.execute("furniture.mount", json!({"device": a, "furniture": rack, "slot": 6, "units": 2})).unwrap();
    assert!(s.execute("furniture.resize", json!({"furniture": rack})).is_err(), "`size` is required");
    assert!(s.execute("furniture.resize", json!({"furniture": rack, "size": 3})).is_err());
    assert!(s.execute("furniture.resize", json!({"furniture": rack, "size": 6})).is_err(), "the device ends on unit 7");
    s.execute("furniture.resize", json!({"furniture": rack, "size": 20})).unwrap();
    assert_eq!(s.topology.furniture_piece(rack as u32).unwrap().size, 20);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.furniture_piece(rack as u32).unwrap().size, 8);
    let table = id(&s.execute("furniture.add", json!({"kind": "table"})).unwrap());
    assert!(s.execute("furniture.resize", json!({"furniture": table, "size": 2})).is_err());
}

#[test]
fn a_power_strip_switches_what_is_plugged_into_it() {
    let (mut s, a, b, c) = lab();
    let strip = id(&s.execute("passive.add", json!({"kind": "power-strip", "ports": 3})).unwrap());
    assert_eq!(s.topology.fitting(strip as u32).unwrap().name, "Power Distribution Device0");
    assert_eq!(s.execute("power.plug", json!({"device": a, "strip": strip})).unwrap()["outlet"], 1);
    assert_eq!(s.execute("power.plug", json!({"device": b, "strip": "Power Distribution Device0", "outlet": 3})).unwrap()["outlet"], 3);
    assert!(s.execute("power.plug", json!({"device": c, "strip": strip, "outlet": 3})).is_err(), "in use");
    assert!(s.execute("power.plug", json!({"device": c, "strip": strip, "outlet": 4})).is_err());
    let off = s.execute("power.strip", json!({"strip": strip, "on": false})).unwrap();
    assert_eq!(off["devices"].as_array().unwrap().len(), 2);
    assert!(s.topology.find(&a).unwrap().powered_off && s.topology.find(&b).unwrap().powered_off);
    assert!(!s.topology.find(&c).unwrap().powered_off);
    // A device plugged into a strip that is off cannot be switched on, and plugging in cuts it.
    assert!(s.execute("device.power", json!({"device": a, "on": true})).is_err());
    s.execute("power.plug", json!({"device": c, "strip": strip})).unwrap();
    assert!(s.topology.find(&c).unwrap().powered_off);
    s.execute("power.strip", json!({"strip": strip, "on": "on"})).unwrap();
    assert!(s.topology.devices.iter().all(|d| !d.powered_off));
    s.execute("power.unplug", json!({"device": c})).unwrap();
    assert!(s.execute("power.unplug", json!({"device": c})).is_err());
    s.execute("power.strip", json!({"strip": strip, "on": false})).unwrap();
    assert!(!s.topology.find(&c).unwrap().powered_off, "unplugged: it keeps its own switch");
    assert!(s.execute("power.strip", json!({"strip": strip})).is_err());
    // Undo brings the power back with the switch.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.fitting(strip as u32).unwrap().on);
}

#[test]
fn furniture_survives_a_save_and_a_load() {
    let (mut s, a, ..) = lab();
    let rack = id(&s.execute("furniture.add", json!({"kind": "rack"})).unwrap());
    s.execute("furniture.mount", json!({"device": a, "furniture": rack})).unwrap();
    let p = id(&s.execute("passive.add", json!({"kind": "copper-patch-panel", "furniture": rack, "units": 1})).unwrap());
    assert_eq!(s.topology.mounts_on(rack as u32).len(), 2);
    let json = netlabcraft_model::file::to_json(&s.topology).unwrap();
    let back = netlabcraft_model::file::from_json(&json).unwrap();
    assert_eq!(back, s.topology);
    assert!(back.fitting(p as u32).is_some());
}
