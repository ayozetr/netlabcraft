use serde_json::json;

use crate::*;

#[test]
fn every_command_survives_empty_and_hostile_params() {
    let hostile = [
        json!({}),
        json!(null),
        json!([]),
        json!({"device": "nope", "a": 1, "b": -1, "kind": "x", "x": "y", "line": 5}),
        json!({"device": 4_294_967_296u64, "path": "/nonexistent/dir/x"}),
        json!({"json": "{"}),
    ];
    for c in CATALOG {
        for p in &hostile {
            let mut s = Session::new();
            s.execute("file.new", json!({"sample": "vlans"})).unwrap();
            let _ = s.execute(c.id, p.clone());
        }
    }
    assert!(matches!(Session::new().execute("nope.nothing", json!({})), Err(EngineError::UnknownCommand(_))));
}

#[test]
fn every_sample_builds_and_its_hosts_reach_each_other() {
    // Labs that block some traffic on purpose.
    let blocked = [("nat-acl", "PC2", "Web")];
    for sample in sample::SAMPLES {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample.id})).unwrap();
        let hosts: Vec<(String, String)> = s
            .topology
            .devices
            .iter()
            .filter(|d| d.kind.is_host())
            .map(|d| (d.name.clone(), d.ports[0].ip.map(|a| a.addr.to_string()).unwrap_or_else(|| d.ports[0].ipv6.addresses[0].addr.to_string())))
            .collect();
        for (from, _) in &hosts {
            for (to, ip) in &hosts {
                if from == to || blocked.contains(&(sample.id, from.as_str(), to.as_str())) {
                    continue;
                }
                // Behind NAT, inside hosts can't be reached from outside.
                if sample.id == "nat-acl" && from == "Web" {
                    continue;
                }
                // The VTP lab has no router: each VLAN only reaches itself.
                let own = hosts.iter().find(|(n, _)| n == from).map(|(_, a)| a.rsplit_once('.').map(|x| x.0.to_string()));
                if sample.id == "vtp" && own != Some(ip.rsplit_once('.').map(|x| x.0.to_string())) {
                    continue;
                }
                // Warm ARP everywhere (each router drops one packet per new neighbour), then expect a clean run.
                s.execute("net.ping", json!({"from": from, "to": ip, "count": 6})).unwrap();
                let r = s.execute("net.ping", json!({"from": from, "to": ip, "count": 3})).unwrap();
                assert_eq!(r["marks"], "!!!", "{}: {from} → {to}", sample.id);
            }
        }
    }
}

#[test]
fn building_a_lab_with_commands_then_undo_redo_and_save() {
    let mut s = Session::new();
    let r = s.execute("device.add", json!({"kind": "router", "x": 10, "y": 20})).unwrap();
    assert_eq!(r["name"], "R1");
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "b": "R1", "b_port": "g0/1"})).unwrap();
    let out = s.execute("cli.script", json!({"device": "R1", "lines": "en\nconf t\nint g0/1\nip add 10.0.0.1 255.255.255.0\nno shut\nend"})).unwrap();
    assert_eq!(out["prompt"], "R1#");
    s.execute("cli.exec", json!({"device": "PC1", "line": "ipconfig 10.0.0.2 255.255.255.0 10.0.0.1"})).unwrap();
    let ping = s.execute("cli.exec", json!({"device": "PC1", "line": "ping 10.0.0.1"})).unwrap();
    assert!(ping["output"].as_str().unwrap().contains("4 answered"), "{ping}");
    assert!(s.consoles[&s.topology.resolve("PC1").unwrap()].log.contains("ping 10.0.0.1\nPinging"));
    assert!(s.dirty);
    // Undo the PC address, the router's config lines (one step each), then redo one.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.find("PC1").unwrap().ports[0].ip.is_none());
    s.execute("edit.redo", json!({})).unwrap();
    assert!(s.topology.find("PC1").unwrap().ports[0].ip.is_some());
    let saved = s.execute("file.save", json!({})).unwrap();
    assert!(!s.dirty);
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
    let ev = s.execute("sim.events", json!({"limit": 3})).unwrap();
    assert_eq!(ev.as_array().unwrap().len(), 3);
    let pcap = s.execute("file.export_pcap", json!({})).unwrap();
    assert!(pcap["bytes"].as_u64().unwrap() > 24);
    s.execute("device.remove", json!({"device": "R1"})).unwrap();
    assert!(s.topology.links.is_empty());
    assert!(!s.consoles.contains_key(&packetcraft_core::DeviceId(1)));
}

#[test]
fn save_writes_atomically_and_open_reads_back() {
    let dir = std::env::temp_dir().join(format!("packetcraft-engine-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lab.pcraft");
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "static-routing"})).unwrap();
    s.execute("file.save", json!({"path": path.to_str().unwrap()})).unwrap();
    assert!(!dir.join(".lab.pcraft.part").exists());
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"path": path.to_str().unwrap()})).unwrap();
    assert_eq!(s2.topology, s.topology);
    assert_eq!(s2.title(), "lab");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bad_params_are_errors_not_panics() {
    let mut s = Session::new();
    assert!(matches!(s.execute("device.add", json!({"kind": "toaster"})), Err(EngineError::BadParams(_))));
    assert!(s.execute("device.add", json!({"kind": "pc", "x": f64::MAX})).is_ok());
    assert!(s.execute("link.add", json!({"a": "PC1", "b": "PC1"})).is_err());
    assert!(s.execute("net.ping", json!({"from": "PC1", "to": "not-an-ip"})).is_err());
    assert!(s.execute("file.new", json!({"sample": "nope"})).is_err());
    assert!(s.topology.devices.len() == 1);
}

#[test]
fn configure_applies_lines_outside_the_console_mode() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
    s.execute("cli.exec", json!({"device": "R1", "line": "enable"})).unwrap();
    let r = s
        .execute(
            "device.configure",
            json!({"device": "R1", "lines": ["interface g0/2", "ip address 10.9.9.1 255.255.255.0", "no shutdown", "bogus words"]}),
        )
        .unwrap();
    assert_eq!(r["errors"].as_array().unwrap().len(), 1);
    assert_eq!(s.topology.find("R1").unwrap().ports[2].ip.unwrap().to_string(), "10.9.9.1/24");
    // The console stays where the person left it.
    assert_eq!(s.execute("cli.prompt", json!({"device": "R1"})).unwrap()["prompt"], "R1#");
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.topology.find("R1").unwrap().ports[2].ip.is_none());
    assert!(s.execute("device.configure", json!({"device": "PC1", "lines": "x"})).is_err());
    let t0 = s.sim.now_us();
    s.execute("sim.advance", json!({"seconds": 30})).unwrap();
    assert_eq!(s.sim.now_us(), t0 + 30_000_000);
}

#[test]
fn models_duplicates_and_notes() {
    let mut s = Session::new();
    let r = s.execute("device.add", json!({"model": "Switch-48", "x": 10, "y": 10})).unwrap();
    assert_eq!(s.topology.find("S1").unwrap().ports.len(), 50);
    assert!(s.execute("device.add", json!({"model": "Nope-1"})).is_err());
    s.execute("cli.script", json!({"device": "S1", "lines": ["en", "conf t", "vlan 30", "int fa0/5", "sw acc vlan 30", "end"]})).unwrap();
    let copy = s.execute("device.duplicate", json!({"device": r["id"]})).unwrap();
    assert_eq!(copy["name"], "S2");
    let (a, b) = (s.topology.find("S1").unwrap().clone(), s.topology.find("S2").unwrap().clone());
    assert_eq!(b.model, "Switch-48");
    assert_eq!(b.ports[4].switchport.access_vlan, a.ports[4].switchport.access_vlan);
    assert_ne!(b.ports[4].mac, a.ports[4].mac);
    assert_eq!(b.vlans, a.vlans);
    let n = s.execute("note.add", json!({"x": 5, "y": 6, "text": "Name - School"})).unwrap();
    let id = n["id"].as_u64().unwrap();
    s.execute("note.edit", json!({"id": id, "text": "Edited"})).unwrap();
    s.execute("note.move", json!({"id": id, "x": 50, "y": 60})).unwrap();
    assert_eq!(s.topology.annotations[0].text, "Edited");
    assert_eq!(s.topology.annotations[0].x, 50.0);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.annotations[0].x, 5.0);
    s.execute("note.remove", json!({"id": id})).unwrap();
    assert!(s.topology.annotations.is_empty());
    assert!(s.execute("note.edit", json!({"id": 99, "text": "x"})).is_err());
    let models = s.execute("device.models", json!({})).unwrap();
    assert!(models.as_array().unwrap().len() >= 10);
    let hub = s.execute("device.add", json!({"kind": "hub"})).unwrap();
    let out = s.execute("cli.exec", json!({"device": hub["id"], "line": "enable"})).unwrap();
    assert!(out["output"].as_str().unwrap().contains("no console"));
}

#[test]
fn port_security_writes_sticky_addresses_and_err_disabled_into_the_lab() {
    let mut s = Session::new();
    for k in ["switch", "hub", "pc", "pc", "pc"] {
        s.execute("device.add", json!({"kind": k})).unwrap();
    }
    let sw = s.topology.devices[0].name.clone();
    let hub = s.topology.devices[1].name.clone();
    s.execute("link.add", json!({"a": hub, "a_port": "e0", "b": sw, "b_port": "fa0/1"})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "b": hub, "b_port": "e1"})).unwrap();
    s.execute("link.add", json!({"a": "PC2", "b": hub, "b_port": "e2"})).unwrap();
    s.execute("link.add", json!({"a": "PC3", "b": sw, "b_port": "fa0/2"})).unwrap();
    for (pc, ip) in [("PC1", "10.0.0.1"), ("PC2", "10.0.0.2"), ("PC3", "10.0.0.3")] {
        s.execute("cli.exec", json!({"device": pc, "line": format!("ipconfig {ip} 255.255.255.0")})).unwrap();
    }
    s.execute("device.configure", json!({"device": sw, "lines": ["int fa0/1", "switchport mode access", "switchport port-security", "switchport port-security mac-address sticky"]})).unwrap();
    s.execute("cli.exec", json!({"device": "PC1", "line": "ping 10.0.0.3"})).unwrap();
    let port = |s: &Session| s.topology.find(&sw).unwrap().ports[0].clone();
    assert_eq!(port(&s).port_security.unwrap().macs.len(), 1);
    s.execute("cli.exec", json!({"device": "PC2", "line": "ping 10.0.0.3"})).unwrap();
    assert!(port(&s).err_disabled);
    let ping = s.execute("cli.exec", json!({"device": "PC1", "line": "ping 10.0.0.3"})).unwrap();
    assert!(ping["output"].as_str().unwrap().contains("0 answered"), "{ping}");
    // Saved with the lab, like the real configuration.
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
}

#[test]
fn control_traffic_shows_in_the_event_list() {
    use packetcraft_wire::Protocol;
    for (sample, proto, secs, text) in [
        ("stp", Protocol::Stp, 3, "STP BPDU from"),
        ("ospf", Protocol::Ospf, 11, "(1 neighbour)"),
        ("rip", Protocol::Rip, 31, "RIPv2 update from"),
        ("vlans", Protocol::Stp, 3, "[VLAN"),
    ] {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        s.execute("sim.advance", json!({"seconds": secs})).unwrap();
        let ev: Vec<_> = s.sim.trace().iter().filter(|e| e.protocol == proto).collect();
        assert!(ev.iter().any(|e| e.summary.contains(text)), "{sample}: {:?}", ev.iter().map(|e| &e.summary).collect::<Vec<_>>());
        let dropped: Vec<_> = ev.iter().filter_map(|e| e.dropped.as_ref().map(|d| format!("{}: {d}", e.summary))).collect();
        assert!(dropped.is_empty(), "{sample}: {dropped:?}");
    }
    // The capture holds them too.
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "rip"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 31})).unwrap();
    assert!(s.execute("file.export_pcap", json!({})).unwrap()["bytes"].as_u64().unwrap() > 200);
}

#[test]
fn device_messages_appear_on_the_console() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
    s.execute("cli.script", json!({"device": "R1", "lines": "en\nconf t\nint g0/0\nshutdown"})).unwrap();
    let log = &s.consoles[&s.topology.resolve("R1").unwrap()].log;
    assert!(log.contains("GigabitEthernet0/0: link down\nR1(config-if)#"), "{log}");
    // A far-end change shows on the switch too.
    let sw = s.topology.devices.iter().find(|d| d.name == "S1").unwrap().id;
    s.execute("cli.exec", json!({"device": "S1", "line": "enable"})).unwrap();
    s.execute("cli.exec", json!({"device": "R1", "line": "no shutdown"})).unwrap();
    assert!(s.consoles[&sw].log.contains(": link up"), "{}", s.consoles[&sw].log);
}
