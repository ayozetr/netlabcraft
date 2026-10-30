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
    // Activities start unfinished on purpose.
    for sample in sample::SAMPLES.iter().filter(|x| !x.id.starts_with("activity-")) {
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

#[test]
fn an_activity_is_built_captured_checked_and_reset() {
    for hidden in [false, true] {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
        // The instructor: a finished lab becomes the answers.
        s.execute("activity.set", json!({"title": "Two subnets", "instructions": "Address R1 and make the PCs reach each other.", "hidden": hidden}))
            .unwrap();
        let n = s.execute("activity.capture", json!({})).unwrap()["checks"].as_u64().unwrap();
        assert!(n >= 6, "{n}");
        let pc3 = s.topology.find("PC3").unwrap().ports[0].ip.unwrap().addr.to_string();
        s.execute("activity.add_check", json!({"type": "ping", "from": "PC1", "to": pc3, "points": 3})).unwrap();
        s.execute("activity.add_check", json!({"type": "output", "device": "R1", "command": "show ip route", "contains": "10.0.2.0/24"})).unwrap();
        let r = s.execute("activity.check", json!({})).unwrap();
        assert_eq!(r["percent"], 100, "{r}");
        let shown = serde_json::to_string(&s.execute("activity.show", json!({})).unwrap()).unwrap();
        assert_eq!(shown.contains("ip address 10.0.1.1"), !hidden, "{shown}");
        // The starting lab: R1 wiped. The student's work is checked.
        let saved = {
            let mut t = s.topology.clone();
            let r1 = t.resolve("R1").unwrap();
            t.device_mut(r1).unwrap().reset_config();
            let mut st = Session::new();
            st.replace(t, None);
            st.execute("file.save", json!({})).unwrap()["json"].clone()
        };
        let mut student = Session::new();
        student.execute("file.open", json!({"json": saved})).unwrap();
        let r = student.execute("activity.check", json!({})).unwrap();
        let pct = r["percent"].as_u64().unwrap();
        assert!(pct > 0 && pct < 60, "{r}");
        assert!(r["results"].as_array().unwrap().iter().any(|x| x["passed"] == false && x["label"].as_str().unwrap().starts_with("R1")));
        // The event list stays clean: checks ping on a copy.
        assert!(student.sim.trace().is_empty());
        student
            .execute("device.configure", json!({"device": "R1", "lines": ["interface g0/0", "description LAN 1", "ip address 10.0.1.1 255.255.255.0", "no shutdown", "interface g0/1", "description LAN 2", "ip address 10.0.2.1 255.255.255.0", "no shutdown"]}))
            .unwrap();
        let r = student.execute("activity.check", json!({})).unwrap();
        assert_eq!(
            r["percent"],
            100,
            "{}",
            r["results"].as_array().unwrap().iter().filter(|x| x["passed"] == false).map(|x| x["label"].to_string()).collect::<Vec<_>>().join("; ")
        );
        student.execute("activity.reset", json!({})).unwrap();
        assert!(student.execute("activity.check", json!({})).unwrap()["percent"].as_u64().unwrap() < 60);
    }
    let mut s = Session::new();
    assert!(s.execute("activity.check", json!({})).is_err());
    assert!(s.execute("activity.add_check", json!({"type": "bogus", "device": "x"})).is_err());
}

#[test]
fn the_sample_activity_can_be_solved_by_typing() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "activity-first-router"})).unwrap();
    let start = s.execute("activity.check", json!({})).unwrap();
    assert_eq!(start["percent"], 0, "{start}");
    assert!(start.to_string().contains("no shutdown"), "the failed ping carries its hint: {start}");
    s.execute(
        "cli.script",
        json!({"device": "Router0", "lines": "en\nconf t\nhostname R1\nenable secret class\nint g0/0\nip add 192.168.10.1 255.255.255.0\nno shut\nint g0/1\nip add 192.168.20.1 255.255.255.0\nno shut\nend"}),
    )
    .unwrap();
    let done = s.execute("activity.check", json!({})).unwrap();
    assert_eq!(done["percent"], 100, "{done}");
    // Hidden answers: the file doesn't show them.
    let file = s.execute("file.save", json!({})).unwrap()["json"].to_string();
    assert!(!file.contains("192.168.10.1 255.255.255.0") || file.contains("\"ip\":\"192.168.10.1/24\""));
    assert!(!file.contains("enable secret"));
}

#[test]
fn a_locked_activity_keeps_its_checks_to_itself() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "activity-first-router"})).unwrap();
    s.execute("activity.set", json!({"time_limit": 30, "score_only": true})).unwrap();
    assert!(s.execute("activity.lock", json!({"password": ""})).is_err());
    s.execute("activity.lock", json!({"password": "teacher"})).unwrap();
    // Locked: no changes, no checks shown, the score only.
    assert!(s.execute("activity.set", json!({"title": "x"})).is_err());
    assert!(s.execute("activity.capture", json!({})).is_err());
    assert!(s.execute("activity.clear", json!({})).is_err());
    let shown = s.execute("activity.show", json!({})).unwrap();
    assert_eq!(shown["locked"], true);
    assert_eq!(shown["activity"]["checks"], json!([]), "{shown}");
    assert_eq!(shown["activity"]["time_limit"], 30);
    let r = s.execute("activity.check", json!({})).unwrap();
    assert_eq!(r["results"], json!([]));
    assert_eq!(r["detail"], false);
    assert!(r["total"].as_u64().unwrap() > 0);
    // The lock survives saving and opening.
    let file = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    assert!(!file.contains("teacher"));
    let mut student = Session::new();
    student.execute("file.open", json!({"json": file})).unwrap();
    assert!(student.execute("activity.remove_check", json!({"index": 0})).is_err());
    assert!(student.execute("activity.unlock", json!({"password": "guess"})).is_err());
    student.execute("activity.unlock", json!({"password": "teacher"})).unwrap();
    assert_eq!(student.execute("activity.check", json!({})).unwrap()["detail"], true);
    student.execute("activity.set", json!({"time_limit": 99_999})).unwrap();
    assert_eq!(student.execute("activity.show", json!({})).unwrap()["activity"]["time_limit"], 24 * 60);
}

#[test]
fn wifi_hosts_join_an_access_point() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router", "x": 0, "y": 0})).unwrap();
    s.execute("device.add", json!({"model": "AccessPoint", "x": 200, "y": 0})).unwrap();
    s.execute("device.add", json!({"model": "Laptop-WiFi", "x": 300, "y": 150})).unwrap();
    s.execute("device.add", json!({"model": "Laptop-WiFi", "x": 2000, "y": 2000})).unwrap();
    let ap = s.topology.devices[1].name.clone();
    let (l1, l2) = (s.topology.devices[2].name.clone(), s.topology.devices[3].name.clone());
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": ap, "b_port": "g0"})).unwrap();
    s.execute("device.configure", json!({"device": "R1", "lines": ["interface g0/0", "ip address 192.168.1.1 255.255.255.0", "no shutdown"]}))
        .unwrap();
    s.execute("device.wireless", json!({"device": ap, "ssid": "Aula", "key": "clase1234"})).unwrap();
    for (l, ip) in [(&l1, "192.168.1.10"), (&l2, "192.168.1.11")] {
        s.execute("cli.exec", json!({"device": l, "line": format!("ipconfig {ip} 255.255.255.0 192.168.1.1")})).unwrap();
    }
    // Wrong key: not associated.
    let r = s.execute("device.wireless", json!({"device": l1, "ssid": "Aula", "key": "otraclave"})).unwrap();
    assert!(r["associated_with"].is_null());
    let r = s.execute("device.wireless", json!({"device": l1, "ssid": "Aula", "key": "clase1234"})).unwrap();
    assert!(r["associated_with"].as_str().unwrap().contains("Wireless0"), "{r}");
    s.execute("net.ping", json!({"from": l1, "to": "192.168.1.1", "count": 2})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": l1, "to": "192.168.1.1", "count": 3})).unwrap()["marks"], "!!!");
    // Too far away: out of range.
    let r = s.execute("device.wireless", json!({"device": l2, "ssid": "Aula", "key": "clase1234"})).unwrap();
    assert!(r["associated_with"].is_null());
    assert!(s.execute("device.wireless", json!({"device": ap, "ssid": "x", "key": "short"})).is_err());
    assert!(s.execute("device.wireless", json!({"device": "R1", "ssid": "x"})).is_err());
}

#[test]
fn a_home_router_hands_out_addresses_and_translates() {
    let mut s = Session::new();
    let home = s.execute("device.add", json!({"model": "Home-Router", "x": 0, "y": 0})).unwrap()["name"].as_str().unwrap().to_string();
    let laptop = s.execute("device.add", json!({"model": "Laptop-WiFi", "x": 120, "y": 80})).unwrap()["name"].as_str().unwrap().to_string();
    let pc = s.execute("device.add", json!({"kind": "pc", "x": -100, "y": 100})).unwrap()["name"].as_str().unwrap().to_string();
    let isp = s.execute("device.add", json!({"kind": "router", "x": 300, "y": 0})).unwrap()["name"].as_str().unwrap().to_string();
    let web = s.execute("device.add", json!({"kind": "server", "x": 500, "y": 0})).unwrap()["name"].as_str().unwrap().to_string();
    s.execute("link.add", json!({"a": pc, "a_port": "fa0", "b": home, "b_port": "Ethernet1"})).unwrap();
    s.execute("link.add", json!({"a": home, "a_port": "Internet0", "b": isp, "b_port": "g0/0"})).unwrap();
    s.execute("link.add", json!({"a": isp, "a_port": "g0/1", "b": web, "b_port": "g0"})).unwrap();
    s.execute(
        "device.configure",
        json!({"device": isp, "lines": ["interface g0/0", "ip address 203.0.113.1 255.255.255.252", "no shutdown", "interface g0/1", "ip address 198.51.100.1 255.255.255.0", "no shutdown"]}),
    )
    .unwrap();
    s.execute("cli.exec", json!({"device": web, "line": "ipconfig 198.51.100.10 255.255.255.0 198.51.100.1"})).unwrap();
    // Factory settings: the running configuration says what the router does.
    s.execute("cli.exec", json!({"device": home, "line": "enable"})).unwrap();
    let run = s.execute("cli.exec", json!({"device": home, "line": "show running-config"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(
        run.contains("interface Vlan1\n ip address 192.168.0.1 255.255.255.0\n ip nat inside")
            && run.contains("ip nat inside source list 1 interface Internet0 overload"),
        "{run}"
    );
    // The laptop joins the open factory network; both hosts get addresses from .100 up.
    let r = s.execute("device.wireless", json!({"device": laptop, "ssid": "HomeNetwork"})).unwrap();
    assert!(r["associated_with"].as_str().is_some_and(|a| a.contains(&home)), "{r}");
    let got = s.execute("cli.exec", json!({"device": laptop, "line": "ipconfig /renew"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(got.contains("192.168.0.100/24") && got.contains("gateway 192.168.0.1"), "{got}");
    assert!(
        s.execute("cli.exec", json!({"device": pc, "line": "ipconfig /renew"})).unwrap()["output"].as_str().unwrap().contains("192.168.0.101/24")
    );
    // The Internet side, set from the settings page; the home LAN reaches the web server through NAT.
    s.execute("home.settings", json!({"device": home, "internet": "203.0.113.2", "internet_mask": "255.255.255.252", "gateway": "203.0.113.1"}))
        .unwrap();
    s.execute("net.ping", json!({"from": laptop, "to": "198.51.100.10", "count": 2})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": laptop, "to": "198.51.100.10", "count": 3})).unwrap()["marks"], "!!!");
    let nat = s.execute("cli.exec", json!({"device": home, "line": "show ip nat translations"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(nat.contains("203.0.113.2") && nat.contains("192.168.0.100"), "{nat}");
    // The Internet port can lease its address from the provider instead.
    s.execute(
        "device.configure",
        json!({"device": isp, "lines": ["ip dhcp excluded-address 203.0.113.1", "ip dhcp pool WAN", "network 203.0.113.0 255.255.255.252", "default-router 203.0.113.1"]}),
    )
    .unwrap();
    s.execute("home.settings", json!({"device": home, "internet_dhcp": true})).unwrap();
    let homed = s.topology.resolve(&home).unwrap();
    let inet = s.topology.device(homed).unwrap().ports.iter().find(|p| p.name == "Internet0").cloned().unwrap();
    assert!(inet.dhcp && inet.ip.is_some_and(|n| n.addr.to_string() == "203.0.113.2") && inet.dhcp_router.is_some(), "{inet:?}");
    s.execute("net.ping", json!({"from": laptop, "to": "198.51.100.10", "count": 2})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": laptop, "to": "198.51.100.10", "count": 3})).unwrap()["marks"], "!!!");
    // A new LAN: pool, exclusions and NAT's list follow.
    let r = s.execute("home.settings", json!({"device": home, "lan": "10.10.0.1", "lan_mask": "255.255.255.0", "dhcp_start": "10.10.0.50"})).unwrap();
    assert!(r["config"].as_str().unwrap().contains("access-list 1 permit 10.10.0.0 0.0.0.255"));
    assert!(s.execute("cli.exec", json!({"device": pc, "line": "ipconfig /renew"})).unwrap()["output"].as_str().unwrap().contains("10.10.0.50/24"));
    assert!(s.execute("home.settings", json!({"device": home, "dhcp_start": "172.16.0.5"})).is_err());
    assert!(s.execute("home.settings", json!({"device": isp, "dhcp": false})).is_err());
}

#[test]
fn activity_variables_differ_per_student_and_results_come_per_section() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router", "x": 0, "y": 0})).unwrap();
    s.execute("device.add", json!({"kind": "pc", "x": 0, "y": 100})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "PC1", "b_port": "fa0"})).unwrap();
    s.execute(
        "activity.set",
        json!({"title": "Your network", "instructions": "Give R1 g0/0 the address 10.{net}.0.1/24.", "variables": {"net": ["11", "22", "33", "44"]}}),
    )
    .unwrap();
    s.execute(
        "activity.add_check",
        json!({"type": "config", "device": "R1", "line": "interface gigabitethernet0/0 | ip address 10.{net}.0.1 255.255.255.0", "points": 2}),
    )
    .unwrap();
    s.execute(
        "activity.add_check",
        json!({"type": "output", "device": "R1", "command": "show ip interface brief", "contains": "10.{net}.0.1", "section": "Checks"}),
    )
    .unwrap();
    let file = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    // Each student starts with a name and gets their own network.
    let mut nets = std::collections::BTreeSet::new();
    for who in ["Ana", "Bruno", "Carla", "Diego", "Elena", "Fran", "Gema", "Hugo"] {
        let mut st = Session::new();
        st.execute("file.open", json!({"json": file})).unwrap();
        assert!(st.execute("activity.check", json!({})).is_err(), "a name first");
        let r = st.execute("activity.start", json!({"student": who})).unwrap();
        let net = r["values"]["net"].as_str().unwrap().to_string();
        assert!(r["instructions"].as_str().unwrap().contains(&format!("10.{net}.0.1/24")));
        assert!(st.execute("activity.start", json!({"student": "x"})).is_err(), "only once");
        st.execute(
            "device.configure",
            json!({"device": "R1", "lines": ["interface g0/0", format!("ip address 10.{net}.0.1 255.255.255.0"), "no shutdown"]}),
        )
        .unwrap();
        let res = st.execute("activity.check", json!({})).unwrap();
        assert_eq!(res["percent"], 100, "{who}: {res}");
        assert_eq!(
            res["sections"],
            json!([{"name": "R1", "score": 2, "total": 2, "percent": 100}, {"name": "Checks", "score": 1, "total": 1, "percent": 100}])
        );
        // Someone else's network doesn't count.
        let other = if net == "11" { "22" } else { "11" };
        st.execute("device.configure", json!({"device": "R1", "lines": ["interface g0/0", format!("ip address 10.{other}.0.1 255.255.255.0")]}))
            .unwrap();
        assert_eq!(st.execute("activity.check", json!({})).unwrap()["percent"], 0);
        nets.insert(net);
    }
    assert!(nets.len() >= 3, "{nets:?}");
}

#[test]
fn email_goes_from_one_pc_to_another_through_the_server() {
    let mut s = Session::new();
    let mut names = Vec::new();
    for (k, x) in [("switch", 0), ("pc", 100), ("pc", 200), ("server", 300)] {
        names.push(s.execute("device.add", json!({"kind": k, "x": x, "y": 0})).unwrap()["name"].as_str().unwrap().to_string());
    }
    let (sw_name, srv_name) = (names[0].clone(), names[3].clone());
    for (dev, port, sw) in [("PC1", "fa0", "fa0/1"), ("PC2", "fa0", "fa0/2"), (srv_name.as_str(), "g0", "fa0/3")] {
        s.execute("link.add", json!({"a": dev, "a_port": port, "b": sw_name, "b_port": sw})).unwrap();
    }
    for (dev, ip) in [("PC1", "10.0.0.11"), ("PC2", "10.0.0.12"), (srv_name.as_str(), "10.0.0.5")] {
        s.execute("cli.exec", json!({"device": dev, "line": format!("ipconfig {ip} 255.255.255.0")})).unwrap();
    }
    let mut sv = s.topology.device(s.topology.resolve(&srv_name).unwrap()).unwrap().services.clone();
    sv.email.domain = "lab.local".into();
    sv.email.users = vec![
        packetcraft_model::services::AaaUser { name: "ana".into(), password: "a1".into() },
        packetcraft_model::services::AaaUser { name: "bea".into(), password: "b2".into() },
    ];
    s.execute("server.services", json!({"device": srv_name, "services": sv})).unwrap();
    for (dev, user, pw) in [("PC1", "ana", "a1"), ("PC2", "bea", "b2")] {
        s.execute(
            "mail.configure",
            json!({"device": dev, "name": user, "address": format!("{user}@lab.local"), "incoming": "10.0.0.5", "outgoing": "10.0.0.5", "user": user, "password": pw}),
        )
        .unwrap();
    }
    // Spanning tree lets the new ports forward after 30 s.
    s.sim.run_for(&s.topology, 31_000_000);
    let r = s
        .execute("mail.send", json!({"device": "PC1", "to": "bea@lab.local", "subject": "Hola", "body": "Una línea\n.empieza con punto\nfin"}))
        .unwrap();
    assert!(r["answer"].as_str().unwrap().starts_with("250"), "{r}");
    let srv = s.topology.resolve(&srv_name).unwrap();
    assert_eq!(s.topology.device(srv).unwrap().services.email.mailboxes.len(), 1);
    assert!(s.execute("mail.send", json!({"device": "PC1", "to": "nadie@lab.local"})).unwrap_err().to_string().contains("550"));
    assert!(s.execute("mail.send", json!({"device": "PC1", "to": "x@otro.com"})).unwrap_err().to_string().contains("550"));
    let got = s.execute("mail.receive", json!({"device": "PC2"})).unwrap();
    assert_eq!(got["received"], 1);
    let pc2 = s.topology.resolve("PC2").unwrap();
    let m = &s.topology.device(pc2).unwrap().inbox[0];
    assert_eq!((m.from.as_str(), m.subject.as_str(), m.body.as_str()), ("ana@lab.local", "Hola", "Una línea\n.empieza con punto\nfin"));
    assert!(s.topology.device(srv).unwrap().services.email.mailboxes.is_empty(), "fetched mail leaves the server");
    assert_eq!(s.execute("mail.receive", json!({"device": "PC2"})).unwrap()["received"], 0);
    // Wrong password: refused.
    s.execute(
        "mail.configure",
        json!({"device": "PC2", "address": "bea@lab.local", "incoming": "10.0.0.5", "outgoing": "10.0.0.5", "user": "bea", "password": "x"}),
    )
    .unwrap();
    assert!(s.execute("mail.receive", json!({"device": "PC2"})).is_err());
    let protos: std::collections::BTreeSet<&str> = s.sim.trace().iter().map(|e| e.protocol.name()).collect();
    assert!(protos.contains("SMTP") && protos.contains("POP3"), "{protos:?}");
}

#[test]
fn a_pc_reads_and_sets_a_router_over_snmp() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router", "x": 0, "y": 0})).unwrap();
    s.execute("device.add", json!({"kind": "pc", "x": 0, "y": 100})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "PC1", "b_port": "fa0"})).unwrap();
    s.execute(
        "device.configure",
        json!({"device": "R1", "lines": ["interface g0/0", "ip address 10.0.0.1 255.255.255.0", "no shutdown", "exit", "snmp-server community lectura ro", "snmp-server community escritura rw", "snmp-server location Aula 3"]}),
    )
    .unwrap();
    s.execute("cli.exec", json!({"device": "PC1", "line": "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1"})).unwrap();
    let get = |s: &mut Session, community: &str, op: &str, oid: &str| {
        s.execute("snmp.request", json!({"device": "PC1", "target": "10.0.0.1", "community": community, "operation": op, "oid": oid}))
    };
    let r = get(&mut s, "lectura", "get", "1.3.6.1.2.1.1.5.0").unwrap();
    assert_eq!(r["bindings"][0]["value"], "R1", "{r}");
    assert_eq!(get(&mut s, "lectura", "get", "1.3.6.1.2.1.1.6.0").unwrap()["bindings"][0]["value"], "Aula 3");
    // Walking: get-next from the system group's start, and past the end.
    let r = get(&mut s, "lectura", "get-next", "1.3.6.1.2.1.1").unwrap();
    assert_eq!(r["bindings"][0]["oid"], "1.3.6.1.2.1.1.1.0");
    let r = get(&mut s, "lectura", "get-next", "1.3.6.1.2.1.2.2.1.2").unwrap();
    assert_eq!(r["bindings"][0]["value"], "GigabitEthernet0/0");
    assert_eq!(get(&mut s, "lectura", "get-next", "1.3.6.1.2.1.9").unwrap()["bindings"][0]["value"], "end of the MIB");
    // Sets: refused with a read-only community, applied with a read-write one.
    let set = |s: &mut Session, community: &str, v: &str| {
        s.execute(
            "snmp.request",
            json!({"device": "PC1", "target": "10.0.0.1", "community": community, "operation": "set", "oid": "1.3.6.1.2.1.1.5.0", "value": v}),
        )
    };
    assert!(set(&mut s, "lectura", "Core").unwrap()["error"].as_str().unwrap().contains("no access"));
    assert!(set(&mut s, "escritura", "Core").unwrap()["error"].is_null());
    assert!(s.topology.resolve("Core").is_ok(), "the hostname changed");
    // A wrong community: no answer.
    assert!(get(&mut s, "adivina", "get", "1.3.6.1.2.1.1.5.0").unwrap_err().to_string().contains("did not answer"));
    assert!(s.sim.trace().iter().any(|e| e.summary.starts_with("SNMP response (lectura) 1.3.6.1.2.1.1.5.0 = R1")));
}
