//! 802.1X on a switch port: the host logs in through EAPOL, the switch relays it to the Server's
//! RADIUS service, and the port opens (or does not).

use netlabcraft_model::services::{AaaClient, AaaUser};
use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, device: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": device, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

/// A PC on Fa0/1 of a switch that has 802.1X on, and a Server with the RADIUS service on Gi0/1.
fn lab(extra: &[&str]) -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Switch-24", "name": "S1"})).unwrap();
    s.execute("device.add", json!({"model": "PC", "name": "PC1"})).unwrap();
    s.execute("device.add", json!({"kind": "server", "name": "SRV"})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "a_port": "fa0", "b": "S1", "b_port": "fa0/1"})).unwrap();
    s.execute("link.add", json!({"a": "SRV", "a_port": "g0", "b": "S1", "b_port": "g0/1"})).unwrap();
    cli(&mut s, "PC1", "ipconfig 10.0.0.1 255.255.255.0");
    cli(&mut s, "SRV", "ipconfig 10.0.0.10 255.255.255.0");
    cli(&mut s, "S1", "enable");
    let mut lines = vec![
        "vlan 99",
        "exit",
        "aaa new-model",
        "radius-server host 10.0.0.10 key sw-test-secret",
        "aaa authentication dot1x default group radius",
        "dot1x system-auth-control",
        "interface vlan 1",
        "ip address 10.0.0.2 255.255.255.0",
        "no shutdown",
        "exit",
        "interface fa0/1",
        "switchport mode access",
        "authentication port-control auto",
    ];
    lines.extend_from_slice(extra);
    s.execute("device.configure", json!({"device": "S1", "lines": lines})).unwrap();
    let sid = s.topology.resolve("SRV").unwrap();
    if let Some(d) = s.topology.device_mut(sid) {
        d.services.aaa.on = true;
        d.services.aaa.clients.push(AaaClient { name: "S1".into(), address: "10.0.0.2".parse().unwrap(), secret: "sw-test-secret".into() });
        d.services.aaa.users.push(AaaUser { name: "ana".into(), password: "ana-test-pw".into() });
    }
    s
}

fn supplicant(s: &mut Session, password: &str) {
    s.execute("dot1x.supplicant", json!({"device": "PC1", "user": "ana", "password": password})).unwrap();
}

fn marks(s: &mut Session) -> String {
    let _ = s.execute("net.ping", json!({"from": "PC1", "to": "10.0.0.10", "count": 2}));
    s.execute("net.ping", json!({"from": "PC1", "to": "10.0.0.10", "count": 2}))
        .map_or_else(|_| "can't send".into(), |v: Value| v["marks"].as_str().unwrap_or_default().to_string())
}

#[test]
fn a_host_with_the_right_login_opens_its_port_and_the_frames_are_eapol() {
    let mut s = lab(&[]);
    supplicant(&mut s, "ana-test-pw");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    let show = cli(&mut s, "S1", "show dot1x interface fa0/1");
    assert!(show.contains("AUTHORIZED (the server accepted"), "{show}");
    assert!(show.contains("Identity:       ana"), "{show}");
    let st = s.execute("dot1x.status", json!({"device": "PC1"})).unwrap();
    assert_eq!(st["authenticated"], json!(true), "{st}");
    assert!(marks(&mut s).contains('!'));
    let eapol = s.sim.trace().iter().filter(|e| e.protocol == netlabcraft_wire::Protocol::Eapol).count();
    assert!(eapol >= 6, "{eapol} EAPOL frames");
    // The password is not on the wire in the clear.
    assert!(s.sim.trace().iter().all(|e| !e.frame.windows(11).any(|w| w == b"ana-test-pw")));
}

#[test]
fn a_port_stays_shut_until_the_host_has_logged_in() {
    let mut s = lab(&[]);
    // No supplicant: only EAPOL passes, so nothing is answered.
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    assert!(!marks(&mut s).contains('!'));
    let show = cli(&mut s, "S1", "show dot1x all");
    assert!(show.contains("UNAUTHORIZED"), "{show}");
    // A wrong password is refused by the server.
    supplicant(&mut s, "wrong");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    let show = cli(&mut s, "S1", "show dot1x interface fa0/1");
    assert!(show.contains("UNAUTHORIZED (the server refused the login)"), "{show}");
    assert!(!marks(&mut s).contains('!'));
    // The right one, and `dot1x re-authenticate`.
    supplicant(&mut s, "ana-test-pw");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    assert!(marks(&mut s).contains('!'));
    cli(&mut s, "S1", "dot1x re-authenticate interface fa0/1");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    assert!(cli(&mut s, "S1", "show dot1x interface fa0/1").contains("AUTHORIZED (the server accepted"));
}

#[test]
fn the_guest_vlan_takes_a_host_that_never_answers_and_the_auth_fail_vlan_one_that_is_refused() {
    let mut s = lab(&["authentication event no-response action authorize vlan 99", "authentication event fail action authorize vlan 99"]);
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    assert!(cli(&mut s, "S1", "show dot1x interface fa0/1").contains("UNAUTHORIZED"));
    s.execute("sim.advance", json!({"seconds": 100})).unwrap();
    let show = cli(&mut s, "S1", "show dot1x interface fa0/1");
    assert!(show.contains("guest VLAN 99"), "{show}");
    assert!(show.contains("Access VLAN:    99"), "{show}");
    // The guest VLAN is not the server's: nothing is answered.
    assert!(!marks(&mut s).contains('!'));
    supplicant(&mut s, "wrong");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    let show = cli(&mut s, "S1", "show dot1x interface fa0/1");
    assert!(show.contains("auth-fail VLAN 99"), "{show}");
}

#[test]
fn debug_dot1x_and_the_running_config_tell_what_happened() {
    let mut s = lab(&[]);
    cli(&mut s, "S1", "debug dot1x events");
    supplicant(&mut s, "ana-test-pw");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    let log = cli(&mut s, "S1", "show logging");
    assert!(log.contains("goes to the authentication server"), "{log}");
    assert!(log.contains("is accepted"), "{log}");
    let cfg = cli(&mut s, "S1", "show running-config");
    assert!(cfg.contains("dot1x system-auth-control") && cfg.contains("authentication port-control auto"), "{cfg}");
    assert!(cli(&mut s, "S1", "show dot1x").contains("Controlled ports: 1"));
}

#[test]
fn force_unauthorized_never_opens_and_force_authorized_needs_no_login() {
    let mut s = lab(&[]);
    cli(&mut s, "S1", "configure terminal");
    cli(&mut s, "S1", "interface fa0/1");
    cli(&mut s, "S1", "authentication port-control force-authorized");
    cli(&mut s, "S1", "end");
    s.execute("sim.advance", json!({"seconds": 5})).unwrap();
    assert!(marks(&mut s).contains('!'));
    cli(&mut s, "S1", "configure terminal");
    cli(&mut s, "S1", "interface fa0/1");
    cli(&mut s, "S1", "authentication port-control force-unauthorized");
    cli(&mut s, "S1", "end");
    supplicant(&mut s, "ana-test-pw");
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    assert!(!marks(&mut s).contains('!'));
    assert!(cli(&mut s, "S1", "show dot1x interface fa0/1").contains("force-unauthorized"));
}

#[test]
fn the_commands_refuse_what_does_not_make_sense() {
    let mut s = lab(&[]);
    assert!(s.execute("dot1x.supplicant", json!({"device": "S1", "user": "x"})).is_err());
    assert!(s.execute("dot1x.supplicant", json!({"device": "PC1"})).is_err());
    assert!(cli(&mut s, "S1", "show dot1x interface fa0/2").contains("does not control"));
    assert!(cli(&mut s, "S1", "dot1x re-authenticate interface fa0/2").contains("does not control"));
}
