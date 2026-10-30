//! Engine tests for the device families: the switch card in routers, power over Ethernet,
//! SIP phones, IoT things on a timer, the cloud and the modems.

use serde_json::json;

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn marks(s: &mut Session, from: &str, to: &str) -> String {
    s.execute("net.ping", json!({"from": from, "to": to, "count": 4})).unwrap();
    s.execute("net.ping", json!({"from": from, "to": to, "count": 3})).unwrap()["marks"].as_str().unwrap_or_default().to_string()
}

/// Saving and opening the lab gives the same lab back.
fn round_trip(s: &mut Session) -> Session {
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
    s2
}

#[test]
fn the_switch_card_sample_switches_routes_and_survives_save_and_power() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "switch-card"})).unwrap();
    assert_eq!(marks(&mut s, "PC1", "192.168.10.11"), "!!!", "switched over the trunk");
    assert_eq!(marks(&mut s, "PC1", "192.168.20.11"), "!!!", "routed by the VLAN interfaces");
    let mut s2 = round_trip(&mut s);
    assert_eq!(marks(&mut s2, "PC2", "192.168.10.10"), "!!!");
    // A power cycle boots from the saved configuration; the VLAN database stays.
    s2.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    s2.execute("device.power", json!({"device": "R1", "on": true})).unwrap();
    s2.execute("sim.advance", json!({"seconds": 35})).unwrap();
    assert!(cli(&mut s2, "R1", "show vlan brief").contains("STAFF"));
    assert_eq!(marks(&mut s2, "PC1", "192.168.20.10"), "!!!");
    // The card comes out (switched off): its cables are unplugged.
    s2.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    let links = s2.topology.links.len();
    s2.execute("device.module.remove", json!({"device": "R1", "slot": "0/1"})).unwrap();
    assert_eq!(s2.topology.links.len(), links - 3);
    assert!(!s2.topology.find("R1").unwrap().has_switch_card());
}

#[test]
fn phones_boot_on_poe_or_their_adapter_and_stay_dark_on_a_plain_switch() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
    let st = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
    assert_eq!((st["registered"].clone(), st["power"]["source"].clone()), (json!(true), json!("poe")), "{st}");
    assert!(cli(&mut s, "S1", "show power inline").contains("Phone1"));
    // `power inline never` darkens the phone: it forgets its registration.
    for l in ["enable", "configure terminal", "interface fa0/1", "power inline never", "end"] {
        cli(&mut s, "S1", l);
    }
    let src = s.execute("device.power.source", json!({"device": "Phone1"})).unwrap();
    assert_eq!(src["source"], "none", "{src}");
    assert!(src["detail"].as_str().unwrap().contains("power inline never"));
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert!(s.execute("phone.dial", json!({"device": "Phone1", "number": "1002"})).is_err(), "a dark phone can't dial");
    // Its adapter plugged in, it runs again (and the file keeps the adapter).
    s.execute("device.adapter", json!({"device": "Phone1", "on": true})).unwrap();
    let st = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
    assert_eq!((st["registered"].clone(), st["power"]["source"].clone()), (json!(true), json!("adapter")), "{st}");
    let s2 = round_trip(&mut s);
    assert_eq!(s2.topology.find("Phone1").unwrap().adapter, Some(true));
    assert!(s2.topology.find("S1").unwrap().ports[0].poe_never);
    // A phone on a plain switch stays dark and says why.
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Switch 24FE", "name": "SW"})).unwrap();
    s.execute("device.add", json!({"model": "IP Phone", "name": "P"})).unwrap();
    s.execute("link.add", json!({"a": "P", "a_port": "fa0", "b": "SW", "b_port": "fa0/1"})).unwrap();
    let src = s.execute("device.power.source", json!({"device": "P"})).unwrap();
    assert!(src["detail"].as_str().unwrap().contains("gives no power over Ethernet"), "{src}");
    assert!(s.execute("device.adapter", json!({"device": "SW", "on": true})).is_err(), "a switch has no adapter to plug");
    assert!(s.execute("device.adapter", json!({"device": "P"})).is_err());
}
