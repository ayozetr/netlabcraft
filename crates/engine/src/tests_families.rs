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
