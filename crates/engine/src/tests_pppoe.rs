//! Engine tests for the PPPoE Dialer: discovery over the cable, PAP and CHAP, the address and the
//! gateway the session gives, and what a disconnect, undo and a saved lab keep.

use netlabcraft_model::PppAuth;
use netlabcraft_model::pppoe::{PppoeServer, PppoeUser};
use serde_json::{Value, json};

use crate::Session;

/// A PC cabled straight to a server that has the address 10.9.0.1/24 (the service is still off).
fn lab() -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    s.execute("device.add", json!({"kind": "server"})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "b": "SRV1"})).unwrap();
    cli(&mut s, "SRV1", "ipconfig 10.9.0.1 255.255.255.0");
    s
}

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn serve(s: &mut Session, pppoe: Value) {
    s.execute("server.services", json!({"device": "SRV1", "services": {"pppoe": pppoe}})).unwrap();
}

fn dial(s: &mut Session, user: &str, password: &str) -> crate::Result<Value> {
    s.execute("pppoe.connect", json!({"device": "PC1", "username": user, "password": password}))
}

fn reachable(s: &mut Session) -> bool {
    let r = s.execute("net.ping", json!({"from": "PC1", "to": "10.9.0.1", "count": 4})).unwrap();
    r["marks"].as_str().unwrap_or_default().contains('!')
}

#[test]
fn a_dialer_needs_a_server_with_the_service_on_at_the_other_end_of_its_cable() {
    let mut s = lab();
    let st = s.execute("pppoe.status", json!({"device": "PC1"})).unwrap();
    assert_eq!((st["state"].clone(), st["server_found"].clone()), (json!("disconnected"), json!(false)));
    let e = dial(&mut s, "ann", "pw-test").unwrap_err().to_string();
    assert!(e.contains("no PPPoE server answered"), "{e}");
    serve(&mut s, json!({"on": true, "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    assert_eq!(s.execute("pppoe.status", json!({"device": "PC1"})).unwrap()["server_found"], json!(true));
    // A computer with no cable at all hears nobody.
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    assert!(s.execute("pppoe.connect", json!({"device": "PC2", "username": "ann", "password": "pw-test"})).is_err());
    // Switches and printers have no dialer.
    s.execute("device.add", json!({"kind": "switch"})).unwrap();
    assert!(s.execute("pppoe.status", json!({"device": "S1"})).is_err());
}

#[test]
fn chap_gives_an_address_a_gateway_and_a_working_link_and_a_wrong_password_gives_nothing() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "auth": "chap", "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    for (user, password) in [("ann", "wrong"), ("bob", "pw-test"), ("", "pw-test")] {
        assert!(dial(&mut s, user, password).is_err(), "{user}/{password}");
    }
    let st = s.execute("pppoe.status", json!({"device": "PC1"})).unwrap();
    assert_eq!(st["state"], json!("disconnected"));
    assert!(cli(&mut s, "PC1", "ipconfig").contains("not set"), "a refused account changes nothing");
    let st = dial(&mut s, "ann", "pw-test").unwrap();
    assert_eq!(
        (st["state"].clone(), st["address"].clone(), st["mask"].clone(), st["gateway"].clone(), st["auth"].clone(), st["server"].clone()),
        (json!("connected"), json!("10.9.0.100"), json!("255.255.255.0"), json!("10.9.0.1"), json!("chap"), json!("SRV1"))
    );
    assert!(!st.to_string().contains("pw-test"), "the password is never echoed");
    let ip = cli(&mut s, "PC1", "ipconfig");
    assert!(ip.contains("10.9.0.100") && ip.contains("10.9.0.1") && ip.contains("by PPPoE"), "{ip}");
    assert!(cli(&mut s, "PC1", "ipconfig /all").contains("PPPoE login ann"));
    assert!(reachable(&mut s), "the link carries traffic with the new address");
    // Connected: a second connect is refused until a disconnect.
    assert!(dial(&mut s, "ann", "pw-test").is_err());
}

#[test]
fn pap_and_the_pool_decide_the_address() {
    let mut s = lab();
    // The pool starts on the server's own address: that one is taken, so the next is given.
    serve(&mut s, json!({"on": true, "auth": "pap", "pool_start": "10.9.0.1", "pool_size": 2, "users": [{"name": "ann", "password": "pw-test"}]}));
    assert!(dial(&mut s, "ann", "PW-TEST").is_err());
    let st = dial(&mut s, "ann", "pw-test").unwrap();
    assert_eq!((st["address"].clone(), st["auth"].clone()), (json!("10.9.0.2"), json!("pap")));
    s.execute("pppoe.disconnect", json!({"device": "PC1"})).unwrap();
    // A pool on another subnet, or an empty one, has nothing to give.
    for pool in [json!({"pool_start": "10.8.0.1", "pool_size": 5}), json!({"pool_start": "10.9.0.50", "pool_size": 0})] {
        let mut cfg = json!({"on": true, "auth": "pap", "users": [{"name": "ann", "password": "pw-test"}]});
        cfg["pool_start"] = pool["pool_start"].clone();
        cfg["pool_size"] = pool["pool_size"].clone();
        serve(&mut s, cfg);
        let e = dial(&mut s, "ann", "pw-test").unwrap_err().to_string();
        assert!(e.contains("no free address"), "{e}");
    }
}

#[test]
fn disconnect_undo_a_lost_cable_and_a_saved_lab() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    assert!(s.execute("pppoe.disconnect", json!({"device": "PC1"})).is_err(), "nothing to disconnect yet");
    dial(&mut s, "ann", "pw-test").unwrap();
    // The account is remembered: a later connect needs no typing.
    let saved = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    let mut again = Session::new();
    again.execute("file.open", json!({"json": saved})).unwrap();
    assert_eq!(again.execute("pppoe.status", json!({"device": "PC1"})).unwrap()["state"], json!("connected"));
    let st = s.execute("pppoe.disconnect", json!({"device": "PC1"})).unwrap();
    assert_eq!((st["state"].clone(), st["address"].clone()), (json!("disconnected"), Value::Null));
    assert!(cli(&mut s, "PC1", "ipconfig").contains("not set"));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("pppoe.status", json!({"device": "PC1"})).unwrap()["state"], json!("connected"));
    s.execute("pppoe.disconnect", json!({"device": "PC1"})).unwrap();
    let st = s.execute("pppoe.connect", json!({"device": "PC1"})).unwrap();
    assert_eq!(st["state"], json!("connected"));
    // Typing an address by hand ends the session.
    cli(&mut s, "PC1", "ipconfig 10.9.0.77 255.255.255.0 10.9.0.1");
    assert_eq!(s.execute("pppoe.status", json!({"device": "PC1"})).unwrap()["state"], json!("disconnected"));
    // Pull the cable out of a live session: the status says it is lost.
    dial(&mut s, "ann", "pw-test").unwrap();
    let port = s.topology.device(s.topology.resolve("PC1").unwrap()).unwrap().ports[0].name.clone();
    s.execute("link.remove", json!({"device": "PC1", "port": port})).unwrap();
    let st = s.execute("pppoe.status", json!({"device": "PC1"})).unwrap();
    assert_eq!((st["state"].clone(), st["connected"].clone()), (json!("lost"), json!(false)));
}

#[test]
fn the_chap_exchange_checks_a_real_md5_response() {
    let cfg =
        |auth| PppoeServer { on: true, auth, users: vec![PppoeUser { name: "ann".into(), password: "pw-test".into() }], ..PppoeServer::default() };
    let chap = cfg(PppAuth::Chap);
    assert_eq!(crate::pppoe::authenticate(&chap, b"seed", "ann", "pw-test"), Ok(PppAuth::Chap));
    for (u, p) in [("ann", "pw-tesT"), ("ann", ""), ("eve", "pw-test")] {
        assert!(crate::pppoe::authenticate(&chap, b"seed", u, p).is_err(), "{u}/{p}");
    }
    assert_eq!(crate::pppoe::authenticate(&cfg(PppAuth::Pap), b"seed", "ann", "pw-test"), Ok(PppAuth::Pap));
    assert!(crate::pppoe::authenticate(&cfg(PppAuth::Pap), b"seed", "ann", "x").is_err());
}

#[test]
fn hostile_service_settings_are_bounded() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "pool_start": "10.9.0.2", "pool_size": 4_000_000_000u64, "users": [{"name": "", "password": "x"}]}));
    let srv = s.topology.device(s.topology.resolve("SRV1").unwrap()).unwrap();
    assert_eq!(srv.services.pppoe.pool_size, netlabcraft_model::pppoe::MAX_POOL);
    assert!(srv.services.pppoe.users.is_empty());
    assert!(s.execute("server.services", json!({"device": "SRV1", "services": {"pppoe": {"auth": "kerberos"}}})).is_err());
}
