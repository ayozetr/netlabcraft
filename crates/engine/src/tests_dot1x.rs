//! A Server's certificate for its RADIUS service, and a host's login that validates it.

use netlabcraft_model::services::{AaaClient, AaaUser};
use serde_json::{Value, json};

use crate::Session;

/// A controller-less enterprise network: an access point with its RADIUS server and a laptop.
fn lab() -> (Session, String, String, String) {
    let mut s = Session::new();
    let ap = s.execute("device.add", json!({"model": "AccessPoint", "x": 200, "y": 0})).unwrap()["name"].as_str().unwrap().to_string();
    let lt = s.execute("device.add", json!({"model": "Laptop-WiFi", "x": 300, "y": 150})).unwrap()["name"].as_str().unwrap().to_string();
    let srv = s.execute("device.add", json!({"kind": "server", "x": 200, "y": -100})).unwrap()["name"].as_str().unwrap().to_string();
    s.execute("link.add", json!({"a": srv, "a_port": "g0", "b": ap, "b_port": "g0"})).unwrap();
    s.execute("cli.exec", json!({"device": srv, "line": "ipconfig 10.0.0.10 255.255.255.0"})).unwrap();
    s.execute("device.configure", json!({"device": ap, "lines": ["interface vlan 1", "ip address 10.0.0.2 255.255.255.0", "no shutdown"]})).unwrap();
    let sid = s.topology.resolve(&srv).unwrap();
    if let Some(d) = s.topology.device_mut(sid) {
        d.services.aaa.on = true;
        d.services.aaa.clients.push(AaaClient { name: "AP".into(), address: "10.0.0.2".parse().unwrap(), secret: "ap-test-secret".into() });
        d.services.aaa.users.push(AaaUser { name: "ana".into(), password: "ana-test-pw".into() });
    }
    s.execute(
        "device.wireless",
        json!({"device": ap, "ssid": "Corp", "mode": "wpa2-enterprise", "radius_ip": "10.0.0.10", "radius_secret": "ap-test-secret"}),
    )
    .unwrap();
    (s, ap, lt, srv)
}

/// The laptop joins `Corp` with its login; time passes; what it reports.
fn join(s: &mut Session, lt: &str) -> Value {
    s.execute("device.wireless", json!({"device": lt, "ssid": "Corp", "mode": "wpa2-enterprise", "user": "ana", "password": "ana-test-pw"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    s.execute("dot1x.status", json!({"device": lt})).unwrap()
}

fn pem_of(s: &Session, srv: &str) -> String {
    let id = s.topology.resolve(srv).unwrap();
    s.topology.device(id).unwrap().services.aaa.cert.as_ref().unwrap().certificate.clone()
}

#[test]
fn a_server_certificate_is_set_by_command_and_a_host_that_does_not_validate_still_logs_in() {
    let (mut s, _ap, lt, srv) = lab();
    let r = join(&mut s, &lt);
    assert_eq!(r["associated"], json!(true), "{r}");
    assert_eq!(r["server_certificate"]["presented"], json!(false), "{r}");
    let c = s.execute("aaa.certificate", json!({"device": srv, "self_signed": true, "subject": "CN=radius.lab.test, O=NetLab"})).unwrap();
    assert_eq!(c["subject"], json!("CN=radius.lab.test, O=NetLab"));
    assert_eq!(c["chain_length"], json!(1));
    let r = join(&mut s, &lt);
    assert_eq!(r["associated"], json!(true), "{r}");
    let seen = &r["server_certificate"];
    assert_eq!((seen["presented"].clone(), seen["verified"].clone()), (json!(true), json!(false)), "{r}");
    assert_eq!(seen["subject"], json!("CN=radius.lab.test, O=NetLab"));
    // Back to none.
    s.execute("aaa.certificate", json!({"device": srv, "clear": true})).unwrap();
    assert_eq!(join(&mut s, &lt)["server_certificate"]["presented"], json!(false));
}

#[test]
fn a_host_that_validates_trusts_the_certificate_it_was_given_and_refuses_another() {
    let (mut s, _ap, lt, srv) = lab();
    s.execute("aaa.certificate", json!({"device": srv, "self_signed": true, "subject": "CN=radius.lab.test, O=NetLab"})).unwrap();
    let pem = pem_of(&s, &srv);
    join(&mut s, &lt);
    let t = s.execute("dot1x.trust", json!({"device": lt, "validate": true, "ca_pem": pem, "server_name": "radius.lab.test"})).unwrap();
    assert_eq!(t["validate"], json!(true));
    assert_eq!(t["trusted_cas"], json!(["CN=radius.lab.test, O=NetLab"]));
    let r = join(&mut s, &lt);
    assert_eq!(r["associated"], json!(true), "{r}");
    assert_eq!(r["server_certificate"]["verified"], json!(true), "{r}");
    // The server's certificate is replaced by one the host does not trust.
    s.execute("aaa.certificate", json!({"device": srv, "self_signed": true, "subject": "CN=impostor.lab.test, O=Other"})).unwrap();
    let r = join(&mut s, &lt);
    assert_eq!(r["associated"], json!(false), "{r}");
    assert!(r["server_certificate"]["error"].as_str().is_some_and(|e| !e.is_empty()), "{r}");
    assert_eq!(r["server_certificate"]["verified"], json!(false));
    // A name that is not the certificate's.
    s.execute("aaa.certificate", json!({"device": srv, "self_signed": true, "subject": "CN=radius.lab.test, O=NetLab"})).unwrap();
    s.execute("dot1x.trust", json!({"device": lt, "server_name": "elsewhere.test"})).unwrap();
    assert_eq!(join(&mut s, &lt)["associated"], json!(false));
    // No certificate at all where one is demanded.
    s.execute("aaa.certificate", json!({"device": srv, "clear": true})).unwrap();
    s.execute("dot1x.trust", json!({"device": lt, "server_name": ""})).unwrap();
    assert_eq!(join(&mut s, &lt)["associated"], json!(false));
}

#[test]
fn the_commands_refuse_what_does_not_make_sense() {
    let (mut s, ap, lt, srv) = lab();
    assert!(s.execute("aaa.certificate", json!({"device": ap, "self_signed": true})).is_err());
    assert!(s.execute("aaa.certificate", json!({"device": srv})).is_err());
    assert!(s.execute("aaa.certificate", json!({"device": srv, "ca": "NoSuchRouter"})).is_err());
    assert!(s.execute("aaa.certificate", json!({"device": srv, "ca": srv})).is_err());
    // A host with no login has nothing to set trust on, and a bad PEM is refused.
    assert!(s.execute("dot1x.trust", json!({"device": lt, "validate": true})).is_err());
    join(&mut s, &lt);
    assert!(s.execute("dot1x.trust", json!({"device": lt, "ca_pem": "not a certificate"})).is_err());
    assert!(s.execute("dot1x.trust", json!({"device": ap, "validate": true})).is_err());
}
