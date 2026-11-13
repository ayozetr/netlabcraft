//! Engine tests for the cellular network: towers, the central office server, the cellular radio
//! of smartphones and tablets, and the commands that set them.

use serde_json::json;

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

/// The marks of a ping, or `no address` / `no route` when the host could not even send it.
fn marks(s: &mut Session, from: &str, to: &str) -> String {
    let _ = s.execute("net.ping", json!({"from": from, "to": to, "count": 4}));
    match s.execute("net.ping", json!({"from": from, "to": to, "count": 3})) {
        Ok(v) => v["marks"].as_str().unwrap_or_default().to_string(),
        Err(_) => "can't send".to_string(),
    }
}

fn sample() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "cellular"})).unwrap();
    s
}

#[test]
fn the_cellular_sample_attaches_both_devices_and_they_browse_over_mobile_data() {
    let mut s = sample();
    for (name, tower) in [("Smartphone1", "Tower1"), ("Tablet1", "Tower2")] {
        let st = s.execute("cell.status", json!({"device": name})).unwrap();
        assert_eq!(st["attached"], true, "{name}: {st}");
        assert_eq!(st["tower"], tower, "{name}");
        assert_eq!(st["active"], "cellular");
        assert!(st["bars"].as_u64().unwrap() >= 1);
        assert_eq!(marks(&mut s, name, "192.168.60.10"), "!!!", "{name} reaches the web server over mobile data");
        assert_eq!(marks(&mut s, "Web", st["address"].as_str().unwrap()), "!!!", "and the server reaches {name}");
    }
    let page = s.execute("browser.get", json!({"device": "Smartphone1", "url": "192.168.60.10"})).unwrap();
    assert!(page.to_string().contains("200"), "{page}");
    // The server side knows them.
    let show = cli(&mut s, "CO1", "show cellular");
    assert!(show.contains("Smartphone1") && show.contains("Tablet1") && show.contains("10.64.0.2"), "{show}");
    cli(&mut s, "CO1", "enable");
    assert!(cli(&mut s, "CO1", "show running-config").contains("cellular pool 10.64.0.0 255.255.255.0"));
    // The attach exchange is in the event list, and the capture holds its frames.
    assert!(s.sim.trace().iter().any(|e| e.protocol == netlabcraft_wire::Protocol::Cell && e.summary.contains("attach accept")));
    assert!(!s.pcap().is_empty());
}

#[test]
fn the_phone_shell_shows_and_sets_cellular_data_and_the_apn() {
    let mut s = sample();
    let status = cli(&mut s, "Smartphone1", "cellular");
    assert!(status.contains("attached") && status.contains("10.64.0.2"), "{status}");
    assert!(cli(&mut s, "Smartphone1", "ipconfig").contains("Cellular0"), "ipconfig lists the cellular port");
    // A wrong APN: the central office refuses and the phone has no address.
    let wrong = cli(&mut s, "Smartphone1", "cellular apn other");
    assert!(wrong.contains("not attached") && wrong.contains("APN"), "{wrong}");
    assert_eq!(marks(&mut s, "Smartphone1", "192.168.60.10"), "can't send");
    s.sim.settle(&s.topology);
    let refused = s.execute("cell.status", json!({"device": "Smartphone1"})).unwrap();
    assert_eq!(refused["attached"], false);
    assert!(s.sim.trace().iter().any(|e| e.summary.contains("attach reject") && e.summary.contains("APN")), "the reject crossed the network");
    cli(&mut s, "Smartphone1", "cellular apn internet");
    assert_eq!(marks(&mut s, "Smartphone1", "192.168.60.10"), "!!!");
    // Cellular Data off: no link at all.
    let off = cli(&mut s, "Smartphone1", "cellular off");
    assert!(off.contains("off"), "{off}");
    assert_eq!(s.execute("cell.status", json!({"device": "Smartphone1"})).unwrap()["active"], "none");
    cli(&mut s, "Smartphone1", "cellular on");
    assert_eq!(marks(&mut s, "Smartphone1", "192.168.60.10"), "!!!");
    assert!(cli(&mut s, "Smartphone1", "cellular apn a b c").contains("Usage"));
}

#[test]
fn commands_set_the_central_office_and_the_phone_and_undo_restores_them() {
    let mut s = sample();
    s.execute("cell.core", json!({"device": "CO1", "plan": false})).unwrap();
    assert_eq!(s.execute("cell.status", json!({"device": "Smartphone1"})).unwrap()["attached"], false);
    assert_eq!(marks(&mut s, "Smartphone1", "192.168.60.10"), "can't send");
    assert!(s.execute("cell.status", json!({"device": "CO1"})).unwrap()["refused"].as_array().unwrap().len() == 2);
    s.execute("cell.core", json!({"device": "CO1", "plan": true, "apn": "fast"})).unwrap();
    s.execute("cell.settings", json!({"device": "Smartphone1", "apn": "fast"})).unwrap();
    assert_eq!(s.execute("cell.status", json!({"device": "Smartphone1"})).unwrap()["attached"], true);
    assert_eq!(s.execute("cell.status", json!({"device": "Tablet1"})).unwrap()["attached"], false, "the tablet still asks for `internet`");
    s.execute("cell.core", json!({"device": "CO1", "apn": null, "pool": "10.99.0.0/24"})).unwrap();
    assert!(s.execute("cell.core", json!({"device": "CO1", "pool": "nonsense"})).is_err());
    assert!(s.execute("cell.core", json!({"device": "Web", "plan": false})).is_err(), "only a central office");
    assert!(s.execute("cell.settings", json!({"device": "Web", "data": false})).is_err(), "only phones and tablets");
    assert!(s.execute("cell.settings", json!({"device": "Smartphone1"})).is_err());
    // The pool changed but no interface of CO1 is in it: nobody is admitted.
    assert_eq!(s.execute("cell.status", json!({"device": "Tablet1"})).unwrap()["attached"], false);
    assert!(cli(&mut s, "CO1", "cellular pool 10.99.0.1 255.255.255.0").contains('%'), "host bits set");
    assert!(s.undo());
    assert!(s.execute("cell.status", json!({"device": "CO1"})).is_ok());
}

#[test]
fn moving_a_phone_out_of_range_loses_coverage_and_back_in_attaches_again() {
    let mut s = sample();
    let (x, y) = {
        let d = s.topology.find("Smartphone1").unwrap();
        (d.x, d.y)
    };
    s.execute("device.move", json!({"device": "Smartphone1", "x": 5000.0, "y": 5000.0})).unwrap();
    let st = s.execute("cell.status", json!({"device": "Smartphone1"})).unwrap();
    assert_eq!((st["attached"].clone(), st["bars"].clone(), st["active"].clone()), (json!(false), json!(0), json!("none")), "{st}");
    assert!(st["refused"].as_str().unwrap().contains("no cell tower in range"), "{st}");
    assert_eq!(marks(&mut s, "Smartphone1", "192.168.60.10"), "can't send");
    s.execute("device.move", json!({"device": "Smartphone1", "x": x, "y": y})).unwrap();
    assert_eq!(marks(&mut s, "Smartphone1", "192.168.60.10"), "!!!");
    // Between the towers the nearer one serves it.
    let tower2 = s.topology.find("Tower2").unwrap();
    let (tx, ty) = (tower2.x, tower2.y);
    s.execute("device.move", json!({"device": "Smartphone1", "x": tx + 30.0, "y": ty + 30.0})).unwrap();
    assert_eq!(s.execute("cell.status", json!({"device": "Smartphone1"})).unwrap()["tower"], "Tower2");
}

#[test]
fn a_new_phone_a_tower_and_a_central_office_are_placed_with_device_add_and_survive_save_and_open() {
    let mut s = Session::new();
    let tw = s.execute("device.add", json!({"model": "Cell Tower", "x": 100, "y": 100})).unwrap();
    assert_eq!(tw["name"], "Tower1");
    let co = s.execute("device.add", json!({"model": "Central Office Server", "x": 300, "y": 100})).unwrap();
    assert_eq!(co["name"], "CO1");
    s.execute("device.add", json!({"model": "Smartphone", "x": 120, "y": 160})).unwrap();
    s.execute("link.add", json!({"a": "Tower1", "a_port": "Ethernet0", "b": "CO1", "b_port": "GigabitEthernet0/0"})).unwrap();
    // The server's ports come up from the factory; it still needs an address and a pool.
    assert!(!s.topology.find("CO1").unwrap().ports[0].shutdown);
    cli(&mut s, "CO1", "enable");
    cli(&mut s, "CO1", "configure terminal");
    cli(&mut s, "CO1", "interface g0/0");
    cli(&mut s, "CO1", "ip address 10.1.0.1 255.255.255.0");
    cli(&mut s, "CO1", "exit");
    cli(&mut s, "CO1", "cellular pool 10.1.0.0 255.255.255.0");
    let st = s.execute("cell.status", json!({"device": "Smartphone1"})).unwrap();
    assert_eq!(st["address"], "10.1.0.2", "{st}");
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
    assert_eq!(s2.execute("cell.status", json!({"device": "Smartphone1"})).unwrap()["address"], "10.1.0.2");
    assert!(s2.execute("cell.status", json!({"device": "Tower1"})).unwrap()["clients"].as_array().unwrap().len() == 1);
}
