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

#[test]
fn containers_and_the_new_furniture_are_commands_and_undo_steps() {
    let (mut s, a, _, _) = lab();
    assert!(s.execute("place.add", json!({"kind": "container", "parent": 3})).is_err(), "not in a closet");
    let c = id(&s.execute("place.add", json!({"kind": "container", "name": "Store", "parent": 2})).unwrap());
    assert_eq!(s.topology.place(c as u32).unwrap().name, "Store");
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.place(c as u32).is_none());
    s.execute("edit.redo", json!({})).unwrap();
    let inv = id(&s.execute("furniture.add", json!({"kind": "inventory-shelf", "place": c})).unwrap());
    let peg = id(&s.execute("furniture.add", json!({"kind": "pegboard", "place": 3})).unwrap());
    let icp = id(&s.execute("furniture.add", json!({"kind": "icp", "place": 3})).unwrap());
    assert!(s.execute("furniture.add", json!({"kind": "sofa"})).is_err());
    // Storage switches a device off in one undo step.
    s.execute("device.place", json!({"device": a, "place": c})).unwrap();
    s.execute("furniture.mount", json!({"device": a, "furniture": inv})).unwrap();
    assert!(s.topology.devices[0].powered_off);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(!s.topology.devices[0].powered_off);
    assert!(s.execute("furniture.mount", json!({"device": a, "furniture": peg})).is_err());
    assert!(s.execute("furniture.mount", json!({"device": a, "furniture": icp})).is_err());
    let list = s.execute("closet.list", json!({"place": 3})).unwrap();
    assert_eq!(list["furniture"].as_array().unwrap().len(), 2);
}

#[test]
fn cables_hang_on_a_pegboard_and_the_working_closet_is_set() {
    let (mut s, a, b, _) = lab();
    s.execute("link.add", json!({"a": a, "a_port": "Fa0/1", "b": b, "b_port": "Fa0/1"})).unwrap();
    let peg = id(&s.execute("furniture.add", json!({"kind": "pegboard"})).unwrap());
    let r = s.execute("furniture.hang", json!({"furniture": peg, "device": a, "port": "Fa0/1"})).unwrap();
    assert_eq!(r["peg"], 1);
    assert!(s.execute("furniture.hang", json!({"furniture": peg, "device": a, "port": "Fa0/2"})).is_err(), "no cable there");
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.physical.hangs.is_empty());
    s.execute("edit.redo", json!({})).unwrap();
    s.execute("furniture.unhang", json!({"device": b, "port": "Fa0/1"})).unwrap();
    assert!(s.execute("furniture.unhang", json!({"device": b, "port": "Fa0/1"})).is_err());
    // The working closet.
    assert_eq!(s.execute("place.working.get", json!({})).unwrap()["id"], 3);
    let c2 = id(&s.execute("place.add", json!({"kind": "closet", "parent": 2})).unwrap());
    s.execute("place.working.set", json!({"id": c2})).unwrap();
    assert_eq!(s.execute("place.working.get", json!({})).unwrap()["id"], json!(c2));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("place.working.get", json!({})).unwrap()["id"], 3);
    assert!(s.execute("place.working.set", json!({"id": 2})).is_err(), "a building is not a closet");
}

#[test]
fn a_power_strip_breaker_trips_over_its_rating_and_resets_after_unplugging() {
    let (mut s, a, b, c) = lab();
    let strip = id(&s.execute("passive.add", json!({"kind": "power-strip", "ports": 4})).unwrap());
    // Two 45 W switches and a 60 W router: nominal draws from the model table.
    assert_eq!(s.topology.find(&a).unwrap().nominal_watts(), 45);
    assert_eq!(s.topology.find(&c).unwrap().nominal_watts(), 60);
    assert!(s.execute("power.rating", json!({"strip": strip, "watts": 50})).is_err());
    assert!(s.execute("power.rating", json!({"strip": strip, "watts": 99})).is_err());
    assert!(s.execute("power.rating", json!({"strip": strip, "watts": 3681})).is_err());
    assert!(s.execute("power.rating", json!({"strip": strip})).is_err());
    s.execute("power.rating", json!({"strip": strip, "watts": 100})).unwrap();
    s.execute("power.plug", json!({"device": a, "strip": strip})).unwrap();
    s.execute("power.plug", json!({"device": b, "strip": strip})).unwrap();
    let l = s.execute("power.load", json!({"strip": strip})).unwrap();
    assert_eq!((l["load_w"].as_u64(), l["rating_w"].as_u64(), l["tripped"].as_bool()), (Some(90), Some(100), Some(false)));
    assert_eq!(l["outlets"].as_array().map(Vec::len), Some(2));
    // The third device is one too many: the breaker trips, the strip is off and nothing has power.
    let r = s.execute("power.plug", json!({"device": c, "strip": strip})).unwrap();
    assert_eq!(r["tripped"], true);
    let f = s.topology.fitting(strip as u32).unwrap();
    assert!(f.tripped && !f.on);
    assert!(s.topology.devices.iter().all(|d| d.powered_off));
    let l = s.execute("power.load", json!({"strip": strip})).unwrap();
    assert_eq!((l["load_w"].as_u64(), l["demand_w"].as_u64()), (Some(0), Some(150)));
    // Switching on again is refused while the plugged devices are over the rating.
    let e = s.execute("power.strip", json!({"strip": strip, "on": true})).unwrap_err().to_string();
    assert!(e.contains("unplug something"), "{e}");
    assert!(s.topology.fitting(strip as u32).unwrap().tripped);
    s.execute("power.unplug", json!({"device": c})).unwrap();
    let r = s.execute("power.strip", json!({"strip": strip, "on": true})).unwrap();
    assert_eq!((r["on"].as_bool(), r["tripped"].as_bool()), (Some(true), Some(false)));
    assert!(!s.topology.fitting(strip as u32).unwrap().tripped);
    assert!(!s.topology.find(&a).unwrap().powered_off && !s.topology.find(&b).unwrap().powered_off);
    // Closet lists show it.
    let list = s.execute("closet.list", json!({})).unwrap();
    let p = &list["passive"][0];
    assert_eq!((p["rating_w"].as_u64(), p["load_w"].as_u64(), p["tripped"].as_bool()), (Some(100), Some(90), Some(false)));
    // Undo brings back the strip as it was before the last change.
    s.execute("power.unplug", json!({"device": b})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.plug_of(s.topology.find(&b).unwrap().id).is_some());
}

#[test]
fn a_device_switched_on_can_trip_the_breaker_of_its_strip() {
    let (mut s, a, _, c) = lab();
    let strip = id(&s.execute("passive.add", json!({"kind": "power-strip", "ports": 3})).unwrap());
    s.execute("power.rating", json!({"strip": strip, "watts": 100})).unwrap();
    s.execute("device.power", json!({"device": a, "on": false})).unwrap();
    s.execute("power.plug", json!({"device": a, "strip": strip})).unwrap();
    s.execute("power.plug", json!({"device": c, "strip": strip})).unwrap();
    assert!(!s.topology.fitting(strip as u32).unwrap().tripped, "the switch that is off draws nothing");
    let r = s.execute("device.power", json!({"device": a, "on": true})).unwrap();
    assert_eq!((r["tripped"].as_bool(), r["power"].as_str()), (Some(true), Some("off")));
    assert!(s.topology.fitting(strip as u32).unwrap().tripped);
    assert!(s.topology.find(&c).unwrap().powered_off);
    // A lower rating than the load trips it at once.
    s.execute("power.rating", json!({"strip": strip, "watts": 200})).unwrap();
    s.execute("power.strip", json!({"strip": strip, "on": true})).unwrap();
    assert!(!s.topology.fitting(strip as u32).unwrap().tripped);
    let r = s.execute("power.rating", json!({"strip": strip, "watts": 100})).unwrap();
    assert_eq!(r["tripped"], true);
    s.execute("power.rating", json!({"strip": strip, "watts": 200})).unwrap();
    s.execute("power.strip", json!({"strip": strip, "on": true})).unwrap();
    s.execute("power.rating", json!({"strip": strip, "watts": 100})).unwrap();
    // The rating survives a save.
    let json = netlabcraft_model::file::to_json(&s.topology).unwrap();
    let back = netlabcraft_model::file::from_json(&json).unwrap();
    let f = back.fitting(strip as u32).unwrap();
    assert_eq!((f.rating_w, f.tripped), (100, true));
}

#[test]
fn a_table_is_resized_in_cells_and_one_undo_takes_it_back() {
    let (mut s, a, b, c) = lab();
    let t = s.execute("furniture.add", json!({"kind": "table", "width": 3, "depth": 1})).unwrap();
    let table = id(&t);
    let listed = |s: &mut Session| s.execute("closet.list", json!({})).unwrap()["furniture"][0].clone();
    let v = listed(&mut s);
    assert_eq!((v["width"].as_u64(), v["depth"].as_u64()), (Some(3), Some(1)));
    assert!(v.get("side").is_none(), "only a rack has a side");
    for d in [&a, &b, &c] {
        s.execute("furniture.mount", json!({"device": d, "furniture": table})).unwrap();
    }
    assert!(s.execute("furniture.resize", json!({"furniture": table, "width": 2})).is_err(), "under what stands on it");
    s.execute("furniture.resize", json!({"furniture": table, "depth": 3})).unwrap();
    assert_eq!(listed(&mut s)["depth"], 3);
    s.execute("furniture.resize", json!({"furniture": table, "width": 8, "depth": 6})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(listed(&mut s)["width"], 3);
    assert!(s.execute("furniture.resize", json!({"furniture": table})).is_err(), "a width or a depth is required");
    assert!(s.execute("furniture.resize", json!({"furniture": table, "size": 4})).is_err());
    assert!(s.execute("furniture.resize", json!({"furniture": table, "width": 9})).is_err());
    assert!(s.execute("furniture.resize", json!({"furniture": table, "width": "wide"})).is_err());
    assert!(s.execute("furniture.add", json!({"kind": "table", "width": 1})).is_err());
    assert!(s.execute("furniture.add", json!({"kind": "shelf", "width": 3})).is_err(), "a shelf has no width");
    assert_eq!(s.topology.physical.furniture.len(), 1, "a refused add leaves nothing behind");
}

#[test]
fn a_rack_is_turned_around_in_one_undo_step() {
    let (mut s, a, ..) = lab();
    let rack = id(&s.execute("furniture.add", json!({"kind": "rack"})).unwrap());
    let shelf = id(&s.execute("furniture.add", json!({"kind": "shelf"})).unwrap());
    s.execute("furniture.mount", json!({"device": a, "furniture": rack, "slot": 1})).unwrap();
    let side = |s: &mut Session| s.execute("closet.list", json!({})).unwrap()["furniture"][0]["side"].clone();
    assert_eq!(side(&mut s), "front");
    assert_eq!(s.execute("furniture.turn", json!({"furniture": rack})).unwrap()["side"], "back");
    assert_eq!(side(&mut s), "back");
    assert_eq!(s.execute("furniture.turn", json!({"furniture": rack, "side": "back"})).unwrap()["side"], "back");
    assert_eq!(s.execute("furniture.turn", json!({"furniture": rack, "side": "front"})).unwrap()["side"], "front");
    assert!(s.execute("furniture.turn", json!({"furniture": rack, "side": "sideways"})).is_err());
    assert!(s.execute("furniture.turn", json!({"furniture": shelf})).is_err(), "only a rack turns");
    assert!(s.execute("furniture.turn", json!({})).is_err());
    s.execute("furniture.turn", json!({"furniture": rack})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(side(&mut s), "front", "one undo step takes the turn back");
    // The device stands where it stood.
    assert!(s.topology.mount_of(netlabcraft_model::furniture::Item::Device(s.topology.find(&a).unwrap().id)).is_some());
}

#[test]
fn the_loose_devices_of_a_closet_are_a_grid_or_a_list() {
    let (mut s, ..) = lab();
    let floor = |s: &mut Session| s.execute("closet.list", json!({})).unwrap()["floor"].clone();
    assert_eq!(floor(&mut s), "grid");
    assert_eq!(s.execute("closet.floor", json!({"layout": "list"})).unwrap()["layout"], "list");
    assert_eq!(floor(&mut s), "list");
    assert!(s.topology.floor_is_list(3));
    assert!(s.execute("closet.floor", json!({"layout": "rows"})).is_err());
    assert!(s.execute("closet.floor", json!({})).is_err());
    assert!(s.execute("closet.floor", json!({"layout": "list", "place": 1})).is_err(), "a city has no floor");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(floor(&mut s), "grid", "one undo step");
    s.execute("closet.floor", json!({"place": 3, "layout": "list"})).unwrap();
    let saved = netlabcraft_model::file::from_json(&netlabcraft_model::file::to_json(&s.topology).unwrap()).unwrap();
    assert!(saved.floor_is_list(3), "the choice is part of the lab");
    s.execute("closet.floor", json!({"layout": "grid"})).unwrap();
    assert_eq!(floor(&mut s), "grid");
}
