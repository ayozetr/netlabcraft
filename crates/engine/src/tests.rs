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
        // Side by side: the answer next to the lab, never the answer when it's hidden.
        let items = r["results"].as_array().unwrap().clone();
        let answer_shown = items.iter().any(|x| x["expected"].as_str().unwrap().contains("10.0.1.1"));
        assert_eq!(answer_shown, !hidden, "{r}");
        let ping = items.iter().find(|x| x["points"] == 3).unwrap();
        assert_eq!((ping["expected"].as_str().unwrap(), ping["actual"].as_str().unwrap()), ("replies", "no replies"));
        student.execute("device.configure", json!({"device": "R1", "lines": ["interface g0/0", "ip address 10.0.9.1 255.255.255.0"]})).unwrap();
        let r = student.execute("activity.check", json!({})).unwrap();
        // A hidden answer gives no place to look, so only open activities show the lab's line.
        let shown =
            r["results"].as_array().unwrap().iter().any(|x| x["actual"] == "interface gigabitethernet0/0 | ip address 10.0.9.1 255.255.255.0");
        assert_eq!(shown, !hidden, "{r}");
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
    // Another domain: taken for relaying (and bounced back, with no DNS to find it).
    assert!(s.execute("mail.send", json!({"device": "PC1", "to": "x@otro.com"})).unwrap()["relayed"][0]["error"].is_string());
    let got = s.execute("mail.receive", json!({"device": "PC2"})).unwrap();
    assert_eq!(got["received"], 1);
    let pc2 = s.topology.resolve("PC2").unwrap();
    let m = &s.topology.device(pc2).unwrap().inbox[0];
    assert_eq!((m.from.as_str(), m.subject.as_str(), m.body.as_str()), ("ana@lab.local", "Hola", "Una línea\n.empieza con punto\nfin"));
    // (Only the bounce for ana is left.)
    assert!(s.topology.device(srv).unwrap().services.email.mailboxes.iter().all(|(u, _)| u == "ana"), "fetched mail leaves the server");
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

/// R1 between PC1 (10.0.0.10, the manager) and PC2 (10.1.0.10).
fn snmp_lab(extra: &[&str]) -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router", "x": 0, "y": 0})).unwrap();
    s.execute("device.add", json!({"kind": "pc", "x": 0, "y": 100})).unwrap();
    s.execute("device.add", json!({"kind": "pc", "x": 100, "y": 100})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "PC1", "b_port": "fa0"})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/1", "b": "PC2", "b_port": "fa0"})).unwrap();
    let mut lines = vec![
        "interface g0/0",
        "ip address 10.0.0.1 255.255.255.0",
        "no shutdown",
        "interface g0/1",
        "ip address 10.1.0.1 255.255.255.0",
        "no shutdown",
        "exit",
    ];
    lines.extend_from_slice(extra);
    s.execute("device.configure", json!({"device": "R1", "lines": lines})).unwrap();
    s.execute("cli.exec", json!({"device": "PC1", "line": "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1"})).unwrap();
    s.execute("cli.exec", json!({"device": "PC2", "line": "ipconfig 10.1.0.10 255.255.255.0 10.1.0.1"})).unwrap();
    s.sim.run_for(&s.topology, 1_000_000);
    s
}

#[test]
fn routers_send_link_and_cold_start_traps_to_a_pc() {
    let mut s = snmp_lab(&[
        "snmp-server community public ro",
        "snmp-server host 10.0.0.10 version 2c avisos",
        "snmp-server enable traps snmp linkdown linkup coldstart",
    ]);
    let traps = |s: &mut Session| s.execute("snmp.traps", json!({"device": "PC1"})).unwrap()["traps"].as_array().unwrap().clone();
    // The ports coming up when the lab was built were reported too.
    assert!(traps(&mut s).iter().all(|x| x["trap"] == "linkUp"));
    s.execute("snmp.traps", json!({"device": "PC1", "clear": true})).unwrap();
    s.execute("device.configure", json!({"device": "R1", "lines": ["interface g0/1", "shutdown"]})).unwrap();
    s.sim.run_for(&s.topology, 2_000_000);
    let got = traps(&mut s);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["trap"], "linkDown");
    assert_eq!(got[0]["from"], "10.0.0.1");
    assert_eq!(got[0]["community"], "avisos");
    let b = got[0]["bindings"].as_array().unwrap();
    assert!(b.iter().any(|x| x["oid"] == "1.3.6.1.2.1.2.2.1.2.2" && x["value"] == "GigabitEthernet0/1"), "{b:?}");
    assert!(b.iter().any(|x| x["oid"] == "1.3.6.1.2.1.2.2.1.7.2" && x["value"] == "2"), "admin down: {b:?}");
    s.execute("device.configure", json!({"device": "R1", "lines": ["interface g0/1", "no shutdown"]})).unwrap();
    s.sim.run_for(&s.topology, 2_000_000);
    assert_eq!(traps(&mut s).last().unwrap()["trap"], "linkUp");
    assert!(s.sim.trace().iter().any(|e| e.summary.starts_with("SNMP trap (avisos) linkDown")), "the capture names the trap");
    let show = s.execute("cli.exec", json!({"device": "R1", "line": "show snmp"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(show.contains("4 trap(s) sent"), "two at the start, then down and up: {show}");
    // A reload with the saved configuration: coldStart.
    let out = s.execute("cli.script", json!({"device": "R1", "lines": ["enable", "copy running-config startup-config", "reload"]})).unwrap();
    assert!(out["outputs"][2].as_str().unwrap().contains("saved configuration"), "{out}");
    s.sim.run_for(&s.topology, 3_000_000);
    assert_eq!(traps(&mut s).last().unwrap()["trap"], "coldStart");
    // Only the traps turned on go out.
    s.execute("device.configure", json!({"device": "R1", "lines": ["no snmp-server enable traps linkdown"]})).unwrap();
    let before = traps(&mut s).len();
    s.execute("device.configure", json!({"device": "R1", "lines": ["interface g0/1", "shutdown"]})).unwrap();
    s.sim.run_for(&s.topology, 2_000_000);
    assert_eq!(traps(&mut s).len(), before);
    assert_eq!(s.execute("snmp.traps", json!({"device": "PC1", "clear": true})).unwrap()["traps"], json!([]));
    assert!(traps(&mut s).is_empty());
}

#[test]
fn snmpv3_users_authenticate_with_md5_or_sha() {
    let mut s = snmp_lab(&[
        "snmp-server group LECTORES v3 auth",
        "snmp-server group ADMINS v3 noauth write todo",
        "snmp-server user ana LECTORES v3 auth sha clave1234",
        "snmp-server user leo LECTORES v3 auth md5 otraclave",
        "snmp-server user bea ADMINS v3",
    ]);
    let v3 = |s: &mut Session, user: &str, auth: &str, pw: &str, op: &str, value: Option<&str>| {
        let mut p = json!({"device": "PC1", "target": "10.0.0.1", "version": "3", "user": user, "auth": auth, "password": pw, "operation": op, "oid": "1.3.6.1.2.1.1.6.0"});
        if let Some(v) = value {
            p["value"] = json!(v);
        }
        s.execute("snmp.request", p)
    };
    s.execute("device.configure", json!({"device": "R1", "lines": ["snmp-server location Aula 3"]})).unwrap();
    let r = v3(&mut s, "ana", "sha", "clave1234", "get", None).unwrap();
    assert_eq!(r["bindings"][0]["value"], "Aula 3", "{r}");
    assert_eq!(v3(&mut s, "leo", "md5", "otraclave", "get", None).unwrap()["bindings"][0]["value"], "Aula 3");
    let err = |r: Result<serde_json::Value>| r.unwrap_err().to_string();
    assert!(err(v3(&mut s, "ana", "sha", "equivocada", "get", None)).contains("authentication failed"));
    assert!(err(v3(&mut s, "ana", "md5", "clave1234", "get", None)).contains("authentication failed"));
    assert!(err(v3(&mut s, "ana", "none", "", "get", None)).contains("security level"));
    assert!(err(v3(&mut s, "nadie", "none", "", "get", None)).contains("unknown user"));
    // Read-only group: no sets; a group with a write view: sets apply.
    assert!(v3(&mut s, "ana", "sha", "clave1234", "set", Some("Sala")).unwrap()["error"].as_str().unwrap().contains("no access"));
    assert!(v3(&mut s, "bea", "none", "", "set", Some("Sala 2")).unwrap()["error"].is_null());
    let r1 = s.topology.resolve("R1").unwrap();
    assert_eq!(s.topology.device(r1).unwrap().snmp.location, "Sala 2");
    assert!(s.execute("snmp.request", json!({"device": "PC1", "target": "10.0.0.1", "version": "3", "user": "ana", "auth": "sha", "password": "corta", "oid": "1.3.6.1.2.1.1.5.0"})).is_err());
    assert!(s.execute("snmp.request", json!({"device": "PC1", "target": "10.0.0.1", "version": "1", "oid": "1.3.6.1.2.1.1.5.0"})).is_err());
    assert!(s.sim.trace().iter().any(|e| e.summary.starts_with("SNMPv3 response (user ana, authenticated)")), "the capture reads v3");
    assert!(s.sim.trace().iter().any(|e| e.summary.starts_with("SNMPv3 report")));
}

#[test]
fn mail_for_another_domain_is_relayed_to_its_server() {
    use packetcraft_model::services::{AaaUser, DnsRecord};
    let mut s = Session::new();
    let mut names = Vec::new();
    for (k, x) in [("switch", 0), ("pc", 100), ("pc", 200), ("server", 300), ("server", 400), ("server", 500)] {
        names.push(s.execute("device.add", json!({"kind": k, "x": x, "y": 0})).unwrap()["name"].as_str().unwrap().to_string());
    }
    let sw = names[0].clone();
    let (a, b, dns) = (names[3].clone(), names[4].clone(), names[5].clone());
    for (i, dev) in names.iter().skip(1).enumerate() {
        let port = if dev.starts_with("PC") { "fa0" } else { "g0" };
        s.execute("link.add", json!({"a": dev, "a_port": port, "b": sw, "b_port": format!("fa0/{}", i + 1)})).unwrap();
    }
    for (dev, ip) in [("PC1", "10.0.0.11"), ("PC2", "10.0.0.12"), (a.as_str(), "10.0.0.5"), (b.as_str(), "10.0.0.6"), (dns.as_str(), "10.0.0.53")] {
        s.execute("cli.exec", json!({"device": dev, "line": format!("ipconfig {ip} 255.255.255.0 10.0.0.1 10.0.0.53")})).unwrap();
    }
    let set_mail = |s: &mut Session, srv: &str, domain: &str, user: &str, pw: &str| {
        let mut sv = s.topology.device(s.topology.resolve(srv).unwrap()).unwrap().services.clone();
        sv.email.domain = domain.into();
        sv.email.users = vec![AaaUser { name: user.into(), password: pw.into() }];
        s.execute("server.services", json!({"device": srv, "services": sv})).unwrap();
    };
    set_mail(&mut s, &a, "a.lab", "ana", "a1");
    set_mail(&mut s, &b, "b.lab", "bea", "b2");
    let mut sv = s.topology.device(s.topology.resolve(&dns).unwrap()).unwrap().services.clone();
    sv.dns.on = true;
    sv.dns.records = vec![
        DnsRecord { name: "a.lab".into(), address: "10.0.0.5".parse().unwrap() },
        DnsRecord { name: "b.lab".into(), address: "10.0.0.6".parse().unwrap() },
    ];
    s.execute("server.services", json!({"device": dns, "services": sv})).unwrap();
    for (dev, user, pw, srv) in [("PC1", "ana", "a1", "10.0.0.5"), ("PC2", "bea", "b2", "10.0.0.6")] {
        s.execute(
            "mail.configure",
            json!({"device": dev, "address": format!("{user}@{}", if user == "ana" { "a.lab" } else { "b.lab" }), "incoming": srv, "outgoing": srv, "user": user, "password": pw}),
        )
        .unwrap();
    }
    s.sim.run_for(&s.topology, 31_000_000);
    let r = s.execute("mail.send", json!({"device": "PC1", "to": "bea@b.lab", "subject": "Entre dominios", "body": "Hola Bea"})).unwrap();
    assert!(r["answer"].as_str().unwrap().starts_with("250"), "{r}");
    assert_eq!(r["relayed"][0]["server"], a.as_str(), "{r}");
    assert!(r["relayed"][0]["answer"].as_str().unwrap().starts_with("250"), "{r}");
    assert_eq!(s.execute("mail.receive", json!({"device": "PC2"})).unwrap()["received"], 1);
    let pc2 = s.topology.resolve("PC2").unwrap();
    let m = &s.topology.device(pc2).unwrap().inbox[0];
    assert_eq!((m.from.as_str(), m.subject.as_str(), m.body.as_str()), ("ana@a.lab", "Entre dominios", "Hola Bea"));
    // No such user on the other server: the message comes back to the sender.
    let r = s.execute("mail.send", json!({"device": "PC1", "to": "nadie@b.lab", "subject": "Perdido"})).unwrap();
    assert!(r["relayed"][0]["error"].as_str().unwrap().contains("550"), "{r}");
    // A domain nobody knows: the same.
    let r = s.execute("mail.send", json!({"device": "PC1", "to": "x@otro.com"})).unwrap();
    assert!(r["relayed"][0]["error"].as_str().unwrap().contains("doesn't know"), "{r}");
    assert_eq!(s.execute("mail.receive", json!({"device": "PC1"})).unwrap()["received"], 2);
    let pc1 = s.topology.resolve("PC1").unwrap();
    let inbox = &s.topology.device(pc1).unwrap().inbox;
    assert!(inbox.iter().all(|m| m.from == "postmaster@a.lab" && m.subject.starts_with("Undelivered")), "{inbox:?}");
    // Server B won't relay for someone outside its domain.
    s.execute(
        "mail.configure",
        json!({"device": "PC1", "address": "ana@a.lab", "incoming": "10.0.0.5", "outgoing": "10.0.0.6", "user": "ana", "password": "a1"}),
    )
    .unwrap();
    assert!(s.execute("mail.send", json!({"device": "PC1", "to": "x@a.lab"})).unwrap_err().to_string().contains("relaying denied"));
}

#[test]
fn freehand_and_polygon_drawings_are_simplified_moved_and_undone() {
    let mut s = Session::new();
    // A slightly wobbly straight stroke, as a hand draws it: simplified to a few points.
    let stroke: Vec<serde_json::Value> = (0..400).map(|i| json!([i as f32, if i % 2 == 0 { 0.0 } else { 0.3 }])).collect();
    let r = s.execute("note.add", json!({"shape": "freehand", "points": stroke, "color": "#336699"})).unwrap();
    let id = r["id"].as_u64().unwrap() as u32;
    let a = s.topology.annotations.iter().find(|a| a.id == id).unwrap();
    assert_eq!(a.shape, packetcraft_model::Shape::Freehand);
    assert!(a.points.len() <= 4, "{:?}", a.points);
    assert_eq!(a.color.as_deref(), Some("#336699"));
    let r = s.execute("note.add", json!({"shape": "polygon", "points": [{"x": 10, "y": 10}, {"x": 60, "y": 10}, {"x": 35, "y": 50}]})).unwrap();
    let poly = r["id"].as_u64().unwrap() as u32;
    s.execute("note.move", json!({"id": poly, "x": 100, "y": 100})).unwrap();
    s.execute("note.color", json!({"id": poly, "color": "#aabbcc"})).unwrap();
    let a = s.topology.annotations.iter().find(|a| a.id == poly).unwrap();
    assert_eq!((a.x, a.y, a.points.len()), (100.0, 100.0, 3));
    assert!(s.execute("note.color", json!({"id": poly, "color": "red"})).is_err());
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.annotations.iter().find(|a| a.id == poly).unwrap().x, 10.0);
    s.execute("note.remove", json!({"id": id})).unwrap();
    assert_eq!(s.topology.annotations.len(), 1);
    // Saved and read back.
    let json = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    let mut b = Session::new();
    b.execute("file.open", json!({"json": json})).unwrap();
    assert_eq!(b.topology.annotations, s.topology.annotations);
    // Hostile points.
    let huge: Vec<serde_json::Value> = (0..100_000).map(|i| json!([i, -i])).collect();
    assert!(s.execute("note.add", json!({"shape": "freehand", "points": huge})).unwrap()["id"].is_number());
    for bad in [
        json!({"shape": "polygon", "points": [[0, 0], [1, 1]]}),
        json!({"shape": "polygon", "points": (0..65).map(|i| json!([i, i * i])).collect::<Vec<_>>()}),
        json!({"shape": "polygon"}),
        json!({"shape": "freehand", "points": "x"}),
        json!({"shape": "freehand", "points": [["a", 0], [0, 0]]}),
        json!({"shape": "freehand", "points": [[0, 0], [0, 0]]}),
        json!({"shape": "polygon", "points": [null, 1, {}]}),
    ] {
        assert!(s.execute("note.add", bad.clone()).is_err(), "{bad}");
    }
    // Out-of-range numbers are clamped, never infinite.
    s.execute("note.add", json!({"shape": "freehand", "points": [[1e308, 0], [0, -1e308]]})).unwrap();
    assert!(s.topology.annotations.iter().all(|a| a.x.is_finite() && a.points.iter().all(|p| p[0].is_finite() && p[1].is_finite())));
}

#[test]
fn clusters_group_devices_for_the_view_only() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "vlans"})).unwrap();
    s.execute("net.ping", json!({"from": "PC1", "to": "192.168.20.10", "count": 6})).unwrap();
    let before = s.execute("net.ping", json!({"from": "PC1", "to": "192.168.20.10", "count": 2})).unwrap()["marks"].clone();
    assert_eq!(before, "!!");
    let r = s.execute("cluster.create", json!({"devices": ["PC1", "PC2"], "name": "Hosts"})).unwrap();
    let id = r["id"].as_u64().unwrap() as u32;
    assert_eq!(r["name"], "Hosts");
    let pc1 = s.topology.resolve("PC1").unwrap();
    assert_eq!(s.topology.cluster_of(pc1), Some(id));
    // The network doesn't notice.
    assert_eq!(s.execute("net.ping", json!({"from": "PC1", "to": "192.168.20.10", "count": 2})).unwrap()["marks"], before);
    s.execute("cluster.rename", json!({"id": id, "name": "Classroom"})).unwrap();
    s.execute("cluster.move", json!({"id": id, "x": 400, "y": 300})).unwrap();
    assert_eq!(s.topology.cluster(id).unwrap().name, "Classroom");
    // Inside the cluster, new devices and drawings join it.
    let v = s.execute("view.enter_cluster", json!({"id": id})).unwrap();
    assert_eq!(v["path"][0]["name"], "Classroom");
    let pc = s.execute("device.add", json!({"kind": "pc", "x": 5, "y": 5})).unwrap();
    let pc = DeviceId(pc["id"].as_u64().unwrap() as u32);
    assert_eq!(s.topology.cluster_of(pc), Some(id));
    let n = s.execute("note.add", json!({"shape": "rectangle", "x": 0, "y": 0})).unwrap();
    assert_eq!(s.topology.annotations.iter().find(|a| Some(a.id as u64) == n["id"].as_u64()).unwrap().cluster, Some(id));
    // A nested cluster, then back up.
    let inner = s.execute("cluster.create", json!({"devices": [pc.0]})).unwrap()["id"].as_u64().unwrap() as u32;
    assert_eq!(s.topology.cluster(inner).unwrap().parent, Some(id));
    s.execute("view.enter_cluster", json!({"id": inner})).unwrap();
    assert_eq!(s.execute("view.up", json!({})).unwrap()["cluster"], id);
    assert_eq!(s.execute("view.root", json!({})).unwrap()["cluster"], serde_json::Value::Null);
    // Dissolving puts the devices back; undo brings the cluster back.
    s.execute("view.enter_cluster", json!({"id": id})).unwrap();
    s.execute("cluster.dissolve", json!({"id": id})).unwrap();
    assert_eq!(s.view, None);
    assert_eq!(s.topology.cluster_of(pc1), None);
    assert_eq!(s.topology.cluster(inner).unwrap().parent, None);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.cluster_of(pc1), Some(id));
    // Undo past the cluster's creation while looking inside it: the view falls back to the top.
    s.execute("view.enter_cluster", json!({"id": id})).unwrap();
    for _ in 0..10 {
        s.execute("edit.undo", json!({})).unwrap();
    }
    assert!(s.topology.clusters.is_empty());
    assert_eq!(s.view, None);
    // Saved and read back.
    s.execute("edit.redo", json!({})).unwrap();
    let json = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    let mut b = Session::new();
    b.execute("file.open", json!({"json": json})).unwrap();
    assert_eq!(b.topology.clusters, s.topology.clusters);
    assert!(!b.topology.clusters.is_empty());
}

#[test]
fn cluster_commands_refuse_hostile_params() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "vlans"})).unwrap();
    for p in [
        json!({"devices": []}),
        json!({"devices": "PC1"}),
        json!({"devices": [null, {}, -1]}),
        json!({"devices": ["PC1"], "clusters": [99]}),
        json!({"devices": ["PC1"], "clusters": "x"}),
        json!({"devices": ["PC1"], "clusters": [-5, 1.5]}),
        json!({"devices": [4_294_967_296u64]}),
    ] {
        assert!(s.execute("cluster.create", p.clone()).is_err(), "{p}");
    }
    assert!(s.topology.clusters.is_empty());
    let id = s.execute("cluster.create", json!({"devices": ["PC1"], "name": "\u{0}\u{7}x".repeat(100)})).unwrap()["id"].as_u64().unwrap();
    assert!(s.topology.clusters[0].name.chars().count() <= packetcraft_model::MAX_NAME);
    assert!(s.topology.clusters[0].name.chars().all(|c| !c.is_control()));
    for (cmd, p) in [
        ("cluster.rename", json!({"id": id, "name": "   "})),
        ("cluster.rename", json!({"id": 77, "name": "x"})),
        ("cluster.move", json!({"id": id, "x": "a", "y": 1})),
        ("cluster.dissolve", json!({"id": "1"})),
        ("view.enter_cluster", json!({"id": 4_294_967_296u64})),
        ("view.enter_cluster", json!({})),
    ] {
        assert!(s.execute(cmd, p.clone()).is_err(), "{cmd} {p}");
    }
    s.execute("cluster.move", json!({"id": id, "x": 1e300, "y": -1e300})).unwrap();
    let c = &s.topology.clusters[0];
    assert!(c.x.is_finite() && c.y.is_finite());
    // Up to the cap, each new cluster nesting the last one.
    let mut t = packetcraft_model::Topology::new();
    let d = t.add_device(packetcraft_model::DeviceKind::Pc, None, 0.0, 0.0).unwrap();
    for _ in 0..packetcraft_model::MAX_CLUSTERS {
        t.create_cluster(&[d], &[], None).unwrap();
    }
    assert!(t.create_cluster(&[d], &[], None).is_err());
    let inner = t.clusters.last().map(|c| c.id);
    assert_eq!(t.shown_as(d, None), Some(packetcraft_model::Shown::Cluster(1)));
    assert_eq!(t.shown_as(d, inner), Some(packetcraft_model::Shown::Device));
    assert_eq!(t.cluster_path(inner).len(), packetcraft_model::MAX_CLUSTERS);
}

fn trace_text(s: &Session) -> String {
    s.sim
        .trace()
        .iter()
        .map(|e| format!("{} {}{}\n", e.protocol.name(), e.summary, e.dropped.as_ref().map(|d| format!(" DROPPED: {d}")).unwrap_or_default()))
        .collect()
}

#[test]
fn a_controller_pushes_wlans_to_its_access_points_and_checks_logins_with_radius() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wlc"})).unwrap();
    let end = |s: &Session, n: &str| packetcraft_model::End { device: s.topology.resolve(n).unwrap(), port: 0 };
    // Both laptops got addresses through their access points.
    for l in ["Laptop1", "Laptop2"] {
        assert!(s.topology.find(l).unwrap().ports[0].ip.is_some(), "{l} has no address");
    }
    s.execute("cli.exec", json!({"device": "WLC1", "line": "enable"})).unwrap();
    let wlan = s.execute("cli.exec", json!({"device": "WLC1", "line": "show wlan"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(wlan.contains("Staff") && wlan.contains("WPA2-Enterprise") && wlan.contains("Laptop1") && wlan.contains("LAP2"), "{wlan}");
    let capwap = s.execute("cli.exec", json!({"device": "WLC1", "line": "show capwap"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(capwap.contains("Access points joined: 2"), "{capwap}");
    let run = s.execute("cli.exec", json!({"device": "WLC1", "line": "show running-config"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(run.contains("wlan 1 Staff enterprise\nwlan 2 Guest psk invitado123"), "{run}");

    // Simulation: the association, the 802.1X login and the RADIUS exchange.
    s.execute("sim.reset", json!({})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Laptop1", "to": "192.168.20.1", "count": 3})).unwrap()["marks"], "!!!");
    let tr = trace_text(&s);
    for want in [
        "802.11 probe request: \"Staff\"?",
        "802.11 probe response: \"Staff\"",
        "802.11 authentication response: success",
        "802.11 association response: success",
        "EAP request: identity?",
        "EAP response: identity \"ana\"",
        "CAPWAP data carrying",
        "RADIUS",
        "EAP success",
        "[802.11]",
    ] {
        assert!(tr.contains(want), "missing {want}:\n{tr}");
    }
    assert!(s.sim.trace().iter().any(|e| e.protocol == packetcraft_wire::Protocol::Wlan && e.air.is_some() && e.frame.is_empty()));
    // The Ethernet capture skips management frames; the 802.11 capture has every air hop.
    let wl = s.sim.pcap_wlan();
    assert_eq!(&wl[20..24], &105u32.to_le_bytes());
    assert!(s.sim.pcap().len() > 24 && wl.len() > 24);
    let r = s.execute("file.export_pcap", json!({"link": "802.11"})).unwrap();
    assert_eq!(r["bytes"].as_u64(), Some(wl.len() as u64));
    assert!(r["frames"].as_u64().unwrap() > 10);
    assert!(s.execute("file.export_pcap", json!({"link": "token-ring"})).is_err());

    // WPA2-Personal: the 4-way handshake, signed with the passphrase.
    s.execute("sim.reset", json!({})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Laptop2", "to": "192.168.20.1", "count": 2})).unwrap()["marks"], "!!");
    let tr = trace_text(&s);
    assert!(tr.contains("4-way handshake message 4 of 4") && !tr.contains("bad MIC"), "{tr}");

    // A wrong password: RADIUS refuses, the host is turned away.
    let r = s.execute("device.wireless", json!({"device": "Laptop1", "ssid": "Staff", "user": "ana", "password": "nope"})).unwrap();
    assert!(r["associated_with"].is_null() && r["problem"].as_str().unwrap().contains("refused"), "{r}");
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    let tr = trace_text(&s);
    assert!(tr.contains("EAP failure") && tr.contains("IEEE 802.1X authentication failed"), "{tr}");
    // A wrong passphrase fails the handshake's MIC.
    s.execute("device.wireless", json!({"device": "Laptop2", "ssid": "Guest", "key": "otraclave1"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert!(trace_text(&s).contains("bad MIC (wrong passphrase)"), "{}", trace_text(&s));

    // WLANs through commands: removing Guest drops it from the APs.
    s.execute("device.wireless", json!({"device": "Laptop2", "ssid": "Guest", "key": "invitado123"})).unwrap();
    assert!(s.topology.wifi_ap_of(end(&s, "Laptop2")).is_some());
    s.execute("wlc.wlan", json!({"device": "WLC1", "id": 2, "remove": true})).unwrap();
    assert!(s.topology.wifi_ap_of(end(&s, "Laptop2")).is_none());
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert!(trace_text(&s).contains("delete 1 WLAN(s)"), "{}", trace_text(&s));
    s.execute("wlc.wlan", json!({"device": "WLC1", "id": 3, "ssid": "Guest", "security": "open"})).unwrap();
    s.execute("device.wireless", json!({"device": "Laptop2", "ssid": "Guest"})).unwrap();
    assert!(s.topology.wifi_ap_of(end(&s, "Laptop2")).is_some());
    assert!(s.execute("wlc.wlan", json!({"device": "WLC1", "id": 4, "ssid": "X", "security": "psk", "key": "short"})).is_err());
    assert!(s.execute("wlc.wlan", json!({"device": "LAP1", "id": 4, "ssid": "X"})).is_err());

    // An access point pointed at an address with no controller leaves it.
    s.execute("device.wireless", json!({"device": "Laptop1", "ssid": "Staff", "user": "ana", "password": "ana12345"})).unwrap();
    assert!(s.topology.wifi_ap_of(end(&s, "Laptop1")).is_some());
    let r = s.execute("lap.controller", json!({"device": "LAP1", "address": "192.168.20.99"})).unwrap();
    assert!(r["joined"].is_null(), "{r}");
    assert!(s.topology.wifi_ap_of(end(&s, "Laptop1")).is_none());
    let r = s.execute("lap.controller", json!({"device": "LAP1", "address": "192.168.20.2"})).unwrap();
    assert_eq!(r["joined"], "WLC1");
    s.execute("sim.reset", json!({})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    let tr = trace_text(&s);
    for want in
        ["CAPWAP discovery request from AP \"LAP1\"", "CAPWAP join response from controller \"WLC1\": success", "802.11 WLAN configuration request"]
    {
        assert!(tr.contains(want), "missing {want}:\n{tr}");
    }
    assert!(s.execute("lap.controller", json!({"device": "WLC1", "address": "1.2.3.4"})).is_err());
}

#[test]
fn the_physical_view_places_devices_and_long_copper_goes_down() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "lan"})).unwrap();
    // An old lab shows the implicit layout, every device in one closet, short cables.
    let list = s.execute("place.list", json!({})).unwrap();
    assert_eq!(list["implicit"], true);
    assert_eq!(list["places"].as_array().unwrap().len(), 3);
    assert_eq!(list["places"][2]["devices"].as_array().unwrap().len(), s.topology.devices.len());
    assert!(list["cables"].as_array().unwrap().iter().all(|c| c["meters"] == 3 && c["problem"].is_null()));
    // Into the view, a building, back up.
    let v = s.execute("view.physical", json!({})).unwrap();
    assert_eq!((v["physical"].as_bool(), v["place"].is_null()), (Some(true), true));
    let v = s.execute("view.enter_place", json!({"id": 2})).unwrap();
    assert_eq!(v["path"].as_array().unwrap().len(), 2);
    assert_eq!(v["kind"], "building");
    assert_eq!(s.execute("view.place_up", json!({})).unwrap()["place"], 1);
    assert!(s.execute("view.intercity", json!({})).unwrap()["path"].as_array().unwrap().is_empty());
    assert!(s.execute("view.enter_place", json!({"id": 99})).is_err());
    // A second building 200 m away (the parent is the only city), and PC3 moves there.
    let b = s.execute("place.add", json!({"kind": "building", "name": "Workshop", "x": 500, "y": 220})).unwrap();
    assert_eq!(b["parent"], 1);
    let b = b["id"].as_u64().unwrap();
    assert!(s.execute("place.add", json!({"kind": "closet", "parent": 1})).is_err());
    assert!(s.execute("device.place", json!({"device": "S1", "place": b})).is_err());
    s.execute("device.place", json!({"device": "PC3", "place": b, "x": 40, "y": 50})).unwrap();
    let pc3 = s.topology.resolve("PC3").unwrap();
    assert_eq!(s.topology.home_of(pc3).map(|h| (h.place, h.x)), Some((b as u32, 40.0)));
    let list = s.execute("place.list", json!({})).unwrap();
    let long = list["cables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["a"].as_str().unwrap().starts_with("PC3") || c["b"].as_str().unwrap().starts_with("PC3"))
        .unwrap()
        .clone();
    assert_eq!(long["meters"], 230);
    assert_eq!(long["problem"], "cable too long: 230 m copper, max 100 m");
    // The link is down, says why, and carries nothing.
    let out = s.execute("cli.exec", json!({"device": "S1", "line": "show interfaces"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(out.contains("is down (cable too long: 230 m copper, max 100 m)"), "{out}");
    let devs = s.execute("device.list", json!({})).unwrap();
    let pc3_port = &devs.as_array().unwrap().iter().find(|d| d["name"] == "PC3").unwrap()["ports"][0];
    assert_eq!((pc3_port["up"].as_bool(), pc3_port["problem"].as_str().is_some()), (Some(false), true));
    let pc3_ip = s.topology.find("PC3").unwrap().ports[0].ip.unwrap().addr.to_string();
    let ping = s.execute("net.ping", json!({"from": "PC1", "to": pc3_ip})).unwrap();
    assert_eq!(ping["replies"], 0, "{ping}");
    // A hand-set 90 m run brings it back; undo takes it away again.
    let port = s.topology.find("PC3").unwrap().ports[0].name.clone();
    let r = s.execute("link.length", json!({"device": "PC3", "port": port, "meters": 90})).unwrap();
    assert_eq!((r["meters"].as_u64(), r["manual"].as_bool(), r["problem"].is_null()), (Some(90), Some(true), true));
    assert!(s.execute("link.length", json!({"device": "PC3", "port": port})).is_err());
    assert!(s.execute("link.length", json!({"device": "PC3", "port": port, "meters": -4})).is_err());
    // (The switch port comes up through the spanning-tree states first.)
    s.execute("sim.advance", json!({"seconds": 60})).unwrap();
    let ping = s.execute("net.ping", json!({"from": "PC1", "to": pc3_ip})).unwrap();
    assert!(ping["replies"].as_u64().unwrap() > 0, "{ping}");
    s.execute("edit.undo", json!({})).unwrap();
    assert!(!packetcraft_sim::port_up(&s.topology, packetcraft_model::End { device: pc3, port: 0 }));
    s.execute("edit.redo", json!({})).unwrap();
    // Rename, move, save and load.
    s.execute("place.rename", json!({"id": b, "name": "Shop"})).unwrap();
    s.execute("place.move", json!({"id": b, "x": 480, "y": 200, "transient": true})).unwrap();
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
    assert_eq!(s2.topology.place(b as u32).unwrap().name, "Shop");
    // Removing the building sends PC3 back to the default closet; undo restores it.
    s.execute("view.enter_place", json!({"id": b})).unwrap();
    s.execute("place.remove", json!({"id": b})).unwrap();
    assert_eq!(s.place_view, Some(1));
    assert_eq!(s.topology.home_of(pc3).unwrap().place, 3);
    assert!(s.execute("place.remove", json!({"id": 1})).is_err());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.topology.home_of(pc3).unwrap().place, b as u32);
    // Undoing back past the first change returns to the implicit layout.
    for _ in 0..10 {
        let _ = s.execute("edit.undo", json!({}));
    }
    assert!(s.topology.implicit_layout());
    s.execute("view.logical", json!({})).unwrap();
    assert!(!s.physical);
}

#[test]
fn a_device_made_in_a_closet_stands_there() {
    let mut s = Session::new();
    let c = s.execute("place.add", json!({"kind": "closet", "name": "Rack B"})).unwrap()["id"].as_u64().unwrap();
    s.execute("view.enter_place", json!({"id": c})).unwrap();
    s.execute("device.add", json!({"kind": "switch", "x": 50, "y": 60})).unwrap();
    let sw = s.topology.resolve("S1").unwrap();
    assert_eq!(s.topology.home_of(sw).map(|h| (h.place, h.x, h.y)), Some((c as u32, 50.0, 60.0)));
}
