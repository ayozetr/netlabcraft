//! A Server's authorization profiles and accounting records, by command.

use netlabcraft_model::services::{AaaClient, AaaUser};
use serde_json::json;

use crate::Session;

fn lab() -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Switch-24", "name": "S1"})).unwrap();
    s.execute("device.add", json!({"kind": "server", "name": "SRV"})).unwrap();
    s.execute("link.add", json!({"a": "SRV", "a_port": "g0", "b": "S1", "b_port": "g0/1"})).unwrap();
    s.execute("cli.exec", json!({"device": "SRV", "line": "ipconfig 10.0.0.10 255.255.255.0"})).unwrap();
    let id = s.topology.resolve("SRV").unwrap();
    if let Some(d) = s.topology.device_mut(id) {
        d.services.aaa.on = true;
        d.services.aaa.clients.push(AaaClient { name: "S1".into(), address: "10.0.0.2".parse().unwrap(), secret: "sw-test-secret".into() });
        d.services.aaa.users.push(AaaUser { name: "ana".into(), password: "ana-test-pw".into() });
    }
    s
}

#[test]
fn a_profile_is_set_listed_and_cleared_and_only_for_a_known_user() {
    let mut s = lab();
    let p = s.execute("aaa.profile", json!({"device": "SRV", "user": "ana", "privilege": 7, "deny": ["reload", "show  running-config"]})).unwrap();
    assert_eq!(p["privilege"], json!(7));
    assert_eq!(p["deny"], json!(["reload", "show running-config"]));
    assert_eq!(s.execute("aaa.profile", json!({"device": "SRV"})).unwrap()["profiles"].as_array().map(Vec::len), Some(1));
    assert!(s.execute("aaa.profile", json!({"device": "SRV", "user": "nobody"})).is_err());
    assert!(s.execute("aaa.profile", json!({"device": "SRV", "user": "ana", "privilege": 16})).is_err());
    assert!(s.execute("aaa.profile", json!({"device": "S1", "user": "ana"})).is_err());
    s.execute("aaa.profile", json!({"device": "SRV", "user": "ana", "clear": true})).unwrap();
    assert_eq!(s.execute("aaa.profile", json!({"device": "SRV"})).unwrap()["profiles"].as_array().map(Vec::len), Some(0));
}

#[test]
fn the_records_are_read_from_the_server_and_can_be_cleared() {
    let mut s = lab();
    let r = s.execute("aaa.records", json!({"device": "SRV"})).unwrap();
    assert_eq!(r["count"], json!(0));
    assert!(s.execute("aaa.records", json!({"device": "S1"})).is_err());
    assert_eq!(s.execute("aaa.records", json!({"device": "SRV", "clear": true})).unwrap()["count"], json!(0));
}
