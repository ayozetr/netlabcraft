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

/// The summaries of the events so far, one per line.
fn trace_text(s: &Session) -> String {
    s.sim.trace().iter().map(|e| e.summary.clone()).collect::<Vec<_>>().join("\n")
}

#[test]
fn chap_is_a_real_challenge_and_response_on_the_wire() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "auth": "chap", "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    assert!(dial(&mut s, "ann", "pw-tesT").is_err());
    let text = trace_text(&s);
    assert!(text.contains("CHAP Challenge") && text.contains("CHAP Response (name ann)") && text.contains("CHAP Failure"), "{text}");
    assert!(!text.contains("CHAP Success"), "{text}");
    s.sim.clear_trace();
    dial(&mut s, "ann", "pw-test").unwrap();
    let text = trace_text(&s);
    for want in [
        "PPPoE PADI",
        "PPPoE PADO: access concentrator SRV1",
        "PPPoE PADR",
        "PPPoE PADS: session 0x0001 granted",
        "LCP Configure-Request (MRU 1492, CHAP, magic number)",
        "LCP Configure-Ack",
        "CHAP Success",
        "IPCP Configure-Nak (address 10.9.0.100)",
        "IPCP Configure-Ack (address 10.9.0.100)",
    ] {
        assert!(text.contains(want), "{want}\n{text}");
    }
}

#[test]
fn pap_sends_the_name_and_password_and_the_server_answers() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "auth": "pap", "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    dial(&mut s, "ann", "pw-test").unwrap();
    let text = trace_text(&s);
    assert!(text.contains("PAP Authenticate-Request (user ann)") && text.contains("PAP Authenticate-Ack"), "{text}");
}

#[test]
fn the_session_crosses_switches_and_carries_ip_as_ppp() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    s.execute("device.add", json!({"kind": "server"})).unwrap();
    s.execute("device.add", json!({"kind": "switch"})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "b": "S1"})).unwrap();
    s.execute("link.add", json!({"a": "PC2", "b": "S1"})).unwrap();
    s.execute("link.add", json!({"a": "SRV1", "b": "S1"})).unwrap();
    cli(&mut s, "SRV1", "ipconfig 10.9.0.1 255.255.255.0");
    serve(
        &mut s,
        json!({"on": true, "pool_start": "10.9.0.100", "pool_size": 5, "users": [{"name": "ann", "password": "pw-test"}, {"name": "bob", "password": "pw-two"}]}),
    );
    let a = dial(&mut s, "ann", "pw-test").unwrap();
    assert_eq!((a["state"].clone(), a["address"].clone()), (json!("connected"), json!("10.9.0.100")));
    let b = s.execute("pppoe.connect", json!({"device": "PC2", "username": "bob", "password": "pw-two"})).unwrap();
    assert_eq!((b["state"].clone(), b["address"].clone()), (json!("connected"), json!("10.9.0.101")));
    let id = |s: &Session, name: &str| s.topology.device(s.topology.resolve(name).unwrap()).unwrap().pppoe.session.clone().unwrap().id;
    assert_ne!(id(&s, "PC1"), id(&s, "PC2"), "each caller has a session of its own");
    s.sim.clear_trace();
    assert!(reachable(&mut s));
    let text = trace_text(&s);
    assert!(text.contains("PPPoE session 0x0001 carrying ICMP echo request"), "{text}");
    assert!(text.contains("PPPoE session 0x0001 carrying ICMP echo reply"), "{text}");
    assert!(!text.contains("ARP who has 10.9.0.1"), "a session needs no ARP: {text}");
    // The frames went through the switch and are in the capture.
    assert!(s.sim.trace().iter().any(|e| e.from.device != e.to.device && e.protocol == netlabcraft_wire::Protocol::Pppoe));
    let pcap = s.pcap();
    assert!(pcap.windows(2).any(|w| w == [0x88, 0x64]), "the capture holds session frames");
    // Releasing the address ends the session properly.
    cli(&mut s, "PC1", "ipconfig /release");
    s.sim.run_for(&s.topology, 100_000);
    assert!(trace_text(&s).contains("PPPoE PADT"), "ending a session sends a PADT");
}

#[test]
fn disconnect_sends_a_terminate_request_and_a_padt() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    dial(&mut s, "ann", "pw-test").unwrap();
    s.sim.clear_trace();
    s.execute("pppoe.disconnect", json!({"device": "PC1"})).unwrap();
    let text = trace_text(&s);
    assert!(text.contains("LCP Terminate-Request") && text.contains("PPPoE PADT: session 0x0001 ended"), "{text}");
    // The concentrator forgot it: a new call gets a session again.
    let st = dial(&mut s, "ann", "pw-test").unwrap();
    assert_eq!(st["state"], json!("connected"));
}

#[test]
fn switching_the_service_off_ends_the_sessions_with_a_padt() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    dial(&mut s, "ann", "pw-test").unwrap();
    assert!(reachable(&mut s));
    serve(&mut s, json!({"on": false}));
    assert!(trace_text(&s).contains("PPPoE PADT"));
    let st = s.execute("pppoe.status", json!({"device": "PC1"})).unwrap();
    assert_eq!((st["state"].clone(), st["address"].clone()), (json!("disconnected"), Value::Null));
    assert!(cli(&mut s, "PC1", "ipconfig").contains("not set"));
}

#[test]
fn a_session_the_server_no_longer_knows_is_lost() {
    let mut s = lab();
    serve(&mut s, json!({"on": true, "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}));
    dial(&mut s, "ann", "pw-test").unwrap();
    // Take the server away without telling it to end the session: power it off.
    s.execute("device.power", json!({"device": "SRV1", "on": false})).unwrap();
    let st = s.execute("pppoe.status", json!({"device": "PC1"})).unwrap();
    assert_ne!(st["state"], json!("connected"), "{st}");
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
