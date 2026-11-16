//! Wireless security settings of access points and Wi-Fi hosts: WEP, WPA and WPA2 (PSK and
//! enterprise), the encryption type and the coverage range. The keys are fakes.

use netlabcraft_model::End;
use netlabcraft_model::services::{AaaClient, AaaUser};
use serde_json::{Value, json};

use crate::Session;

/// An access point at (200, 0) and a Wi-Fi laptop about 180 workspace units away.
fn lab() -> (Session, String, String) {
    let mut s = Session::new();
    let ap = s.execute("device.add", json!({"model": "AccessPoint", "x": 200, "y": 0})).unwrap()["name"].as_str().unwrap().to_string();
    let lt = s.execute("device.add", json!({"model": "Laptop-WiFi", "x": 300, "y": 150})).unwrap()["name"].as_str().unwrap().to_string();
    (s, ap, lt)
}

fn wifi(s: &mut Session, dev: &str, mut p: Value) -> Value {
    p["device"] = json!(dev);
    s.execute("device.wireless", p).unwrap()
}

fn joined(r: &Value) -> bool {
    !r["associated_with"].is_null()
}

fn problem(r: &Value) -> String {
    r["problem"].as_str().unwrap_or_default().to_string()
}

#[test]
fn wep_needs_the_same_key_in_ascii_or_hex() {
    let (mut s, ap, lt) = lab();
    wifi(&mut s, &ap, json!({"ssid": "Old", "mode": "wep", "wep_key": "abcde"}));
    assert!(joined(&wifi(&mut s, &lt, json!({"ssid": "Old", "mode": "wep", "wep_key": "abcde"}))));
    assert!(joined(&wifi(&mut s, &lt, json!({"ssid": "Old", "mode": "wep", "wep_key": "6162636465"}))));
    let r = wifi(&mut s, &lt, json!({"ssid": "Old", "mode": "wep", "wep_key": "fghij"}));
    assert!(!joined(&r) && problem(&r).contains("wrong WEP key"), "{r}");
    let r = wifi(&mut s, &lt, json!({"ssid": "Old"}));
    assert!(!joined(&r) && problem(&r).contains("needs a WEP key"), "{r}");
    // A key of the wrong size is refused when it is set.
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Old", "mode": "wep", "wep_key": "abcd"})).is_err());
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Old", "mode": "wep"})).is_err());
}

#[test]
fn wpa_and_wpa2_and_the_encryption_type_must_match() {
    let (mut s, ap, lt) = lab();
    let net = json!({"ssid": "Lab", "mode": "wpa2-psk", "key": "clase1234", "encryption": "aes"});
    wifi(&mut s, &ap, net.clone());
    assert!(joined(&wifi(&mut s, &lt, net.clone())));
    // A host with only a passphrase (a file from before the modes) is WPA2-PSK with AES.
    assert!(joined(&wifi(&mut s, &lt, json!({"ssid": "Lab", "key": "clase1234"}))));
    let r = wifi(&mut s, &lt, json!({"ssid": "Lab", "mode": "wpa-psk", "key": "clase1234"}));
    assert!(!joined(&r) && problem(&r).contains("uses WPA2-PSK"), "{r}");
    let r = wifi(&mut s, &lt, json!({"ssid": "Lab", "mode": "wpa2-psk", "key": "clase1234", "encryption": "tkip"}));
    assert!(!joined(&r) && problem(&r).contains("encrypts with AES"), "{r}");
    let r = wifi(&mut s, &lt, json!({"ssid": "Lab", "mode": "wpa2-psk", "key": "otraclave1"}));
    assert!(!joined(&r) && problem(&r).contains("wrong WPA2 passphrase"), "{r}");
    // A TKIP network takes a TKIP host.
    wifi(&mut s, &ap, json!({"ssid": "Lab", "mode": "wpa-psk", "key": "clase1234", "encryption": "tkip"}));
    assert!(joined(&wifi(&mut s, &lt, json!({"ssid": "Lab", "mode": "wpa-psk", "key": "clase1234", "encryption": "tkip"}))));
    assert!(!joined(&wifi(&mut s, &lt, json!({"ssid": "Lab", "mode": "wpa-psk", "key": "clase1234"}))));
    // Bad settings are refused.
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Lab", "mode": "wpa3"})).is_err());
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Lab", "mode": "wpa2-psk", "key": "short"})).is_err());
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Lab", "mode": "open", "key": "clase1234"})).is_err());
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Lab", "mode": "wep", "wep_key": "abcde", "encryption": "tkip"})).is_err());
}

#[test]
fn the_coverage_range_decides_who_hears_the_access_point() {
    let (mut s, ap, lt) = lab();
    wifi(&mut s, &ap, json!({"ssid": "Lab"}));
    assert!(joined(&wifi(&mut s, &lt, json!({"ssid": "Lab"}))));
    // 50 m is 90 workspace units: the laptop, 180 away, is out of range.
    let r = wifi(&mut s, &ap, json!({"ssid": "Lab", "coverage_m": 50}));
    assert_eq!(r["coverage_m"], json!(50.0));
    assert!(s.topology.wifi_ap_of(End { device: s.topology.resolve(&lt).unwrap(), port: 0 }).is_none());
    // The range stays when the settings are sent again without it, and 250 m is the default again.
    wifi(&mut s, &ap, json!({"ssid": "Lab"}));
    assert!(s.topology.wifi_ap_of(End { device: s.topology.resolve(&lt).unwrap(), port: 0 }).is_none());
    wifi(&mut s, &ap, json!({"ssid": "Lab", "coverage_m": 250}));
    assert!(s.topology.wifi_ap_of(End { device: s.topology.resolve(&lt).unwrap(), port: 0 }).is_some());
    assert!(s.topology.device(s.topology.resolve(&ap).unwrap()).unwrap().ports[1].wifi.as_ref().unwrap().coverage_cm.is_none());
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Lab", "coverage_m": 0.5})).is_err());
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Lab", "coverage_m": 5000})).is_err());
    assert!(s.execute("device.wireless", json!({"device": lt, "ssid": "Lab", "coverage_m": 50})).is_err());
}

#[test]
fn an_enterprise_access_point_asks_its_radius_server() {
    let (mut s, ap, lt) = lab();
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
    // An enterprise access point needs its RADIUS server.
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "Corp", "mode": "wpa2-enterprise"})).is_err());
    let net = json!({"ssid": "Corp", "mode": "wpa2-enterprise", "radius_ip": "10.0.0.10", "radius_secret": "ap-test-secret"});
    wifi(&mut s, &ap, net);
    let login = |u: &str, p: &str, mode: &str| json!({"ssid": "Corp", "mode": mode, "user": u, "password": p});
    let r = wifi(&mut s, &lt, login("ana", "bad-pw", "wpa2-enterprise"));
    assert!(!joined(&r) && problem(&r).contains("refused"), "{r}");
    let r = wifi(&mut s, &lt, login("ana", "ana-test-pw", "wpa-enterprise"));
    assert!(!joined(&r) && problem(&r).contains("uses WPA2-Enterprise"), "{r}");
    let r = wifi(&mut s, &lt, login("ana", "ana-test-pw", "wpa2-enterprise"));
    assert!(joined(&r), "{r}");
    // A wrong shared secret: the server does not know the access point.
    wifi(&mut s, &ap, json!({"ssid": "Corp", "mode": "wpa2-enterprise", "radius_ip": "10.0.0.10", "radius_secret": "other-secret"}));
    let r = wifi(&mut s, &lt, login("ana", "ana-test-pw", "wpa2-enterprise"));
    assert!(!joined(&r) && problem(&r).contains("doesn't know the access point"), "{r}");
}

#[test]
fn a_wep_host_with_the_wrong_key_is_turned_away_in_the_challenge() {
    let (mut s, ap, lt) = lab();
    wifi(&mut s, &ap, json!({"ssid": "Old", "mode": "wep", "wep_key": "abcde"}));
    wifi(&mut s, &lt, json!({"ssid": "Old", "mode": "wep", "wep_key": "fghij"}));
    s.execute("sim.advance", json!({"seconds": 3})).unwrap();
    let said: String = s.sim.trace().iter().map(|e| format!("{} {}\n", e.summary, e.dropped.clone().unwrap_or_default())).collect();
    assert!(said.contains("challenge failure"), "{said}");
}
