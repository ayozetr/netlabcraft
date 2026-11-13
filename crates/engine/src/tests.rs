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
            // Things wired only to a board's pins are on no network.
            .filter(|d| d.kind.is_host() && s.topology.board_of(d.id).is_none())
            .map(|d| (d.name.clone(), d.ports[0].ip.map(|a| a.addr.to_string()).unwrap_or_else(|| d.ports[0].ipv6.addresses[0].addr.to_string())))
            .collect();
        for (from, _) in &hosts {
            for (to, ip) in &hosts {
                if from == to || blocked.contains(&(sample.id, from.as_str(), to.as_str())) {
                    continue;
                }
                // Behind NAT, inside hosts can't be reached from outside.
                if (sample.id == "nat-acl" || sample.id == "home-internet") && from == "Web" {
                    continue;
                }
                // The firewall keeps lower security levels from starting connections to higher ones.
                if sample.id == "firewall" && (from == "Home" || (from == "Web" && to.starts_with("PC"))) {
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
    assert!(!s.consoles.contains_key(&netlabcraft_core::DeviceId(1)));
}

#[test]
fn save_writes_atomically_and_open_reads_back() {
    let dir = std::env::temp_dir().join(format!("netlabcraft-engine-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lab.nlcraft");
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "static-routing"})).unwrap();
    s.execute("file.save", json!({"path": path.to_str().unwrap()})).unwrap();
    assert!(!dir.join(".lab.nlcraft.part").exists());
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
    use netlabcraft_wire::Protocol;
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
        netlabcraft_model::services::AaaUser { name: "ana".into(), password: "a1".into() },
        netlabcraft_model::services::AaaUser { name: "bea".into(), password: "b2".into() },
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

#[test]
fn a_syslog_server_lists_what_it_received_as_a_command() {
    // Found by the lab book: the messages were only visible in the Services tab.
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Router-2", "name": "R1"})).unwrap();
    s.execute("device.add", json!({"model": "Server", "name": "Log"})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "Log"})).unwrap();
    s.execute("server.services", json!({"device": "Log", "services": {"syslog": {"on": true}}})).unwrap();
    s.execute("cli.exec", json!({"device": "Log", "line": "ipconfig 10.0.0.100 255.255.255.0 10.0.0.1"})).unwrap();
    s.execute(
        "device.configure",
        json!({"device": "R1", "lines": ["interface g0/0", "ip address 10.0.0.1 255.255.255.0", "no shutdown", "exit", "logging host 10.0.0.100"]}),
    )
    .unwrap();
    s.execute("net.ping", json!({"from": "R1", "to": "10.0.0.100"})).unwrap();
    s.execute("cli.script", json!({"device": "R1", "lines": ["enable", "configure terminal", "hostname R1", "end"]})).unwrap();
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    let got = s.execute("server.syslog", json!({"device": "Log"})).unwrap();
    let msgs = got["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m["from"] == "10.0.0.1" && m["text"].as_str().unwrap().starts_with("R1: ")), "{got}");
    assert_eq!(s.execute("server.syslog", json!({"device": "Log", "clear": true})).unwrap()["messages"], json!([]));
    assert_eq!(s.execute("server.syslog", json!({"device": "Log"})).unwrap()["messages"], json!([]));
    assert!(s.execute("server.syslog", json!({"device": "R1"})).is_err());
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
    use netlabcraft_model::services::{AaaUser, DnsRecord};
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
    assert_eq!(a.shape, netlabcraft_model::Shape::Freehand);
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
    assert!(s.topology.clusters[0].name.chars().count() <= netlabcraft_model::MAX_NAME);
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
    let mut t = netlabcraft_model::Topology::new();
    let d = t.add_device(netlabcraft_model::DeviceKind::Pc, None, 0.0, 0.0).unwrap();
    for _ in 0..netlabcraft_model::MAX_CLUSTERS {
        t.create_cluster(&[d], &[], None).unwrap();
    }
    assert!(t.create_cluster(&[d], &[], None).is_err());
    let inner = t.clusters.last().map(|c| c.id);
    assert_eq!(t.shown_as(d, None), Some(netlabcraft_model::Shown::Cluster(1)));
    assert_eq!(t.shown_as(d, inner), Some(netlabcraft_model::Shown::Device));
    assert_eq!(t.cluster_path(inner).len(), netlabcraft_model::MAX_CLUSTERS);
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
    let end = |s: &Session, n: &str| netlabcraft_model::End { device: s.topology.resolve(n).unwrap(), port: 0 };
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
    assert!(run.contains("wlan 1 Staff enterprise vlan 10\nwlan 2 Guest psk invitado123 vlan 30"), "{run}");

    // Simulation: the association, the 802.1X login and the RADIUS exchange.
    s.execute("sim.reset", json!({})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Laptop1", "to": "192.168.10.1", "count": 3})).unwrap()["marks"], "!!!");
    let tr = trace_text(&s);
    for want in [
        "802.11 probe request: \"Staff\"?",
        "802.11 probe response: \"Staff\"",
        "802.11 authentication response: success",
        "802.11 association response: success",
        "EAP request: identity?",
        "EAP response: identity \"ana\"",
        "EAP request: PEAP, start",
        "EAP response: PEAP, TLS handshake (client hello)",
        "CAPWAP data carrying",
        "RADIUS",
        "EAP success",
        "4-way handshake message 4 of 4",
        "[802.11, CCMP]",
    ] {
        assert!(tr.contains(want), "missing {want}:\n{tr}");
    }
    assert!(s.sim.trace().iter().any(|e| e.protocol == netlabcraft_wire::Protocol::Wlan && e.air.is_some() && e.frame.is_empty()));
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
    assert_eq!(s.execute("net.ping", json!({"from": "Laptop2", "to": "192.168.30.1", "count": 2})).unwrap()["marks"], "!!");
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
fn each_wlan_lands_in_its_own_vlan_and_hosts_roam_between_access_points() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wlc"})).unwrap();
    let ip = |s: &Session, n: &str| s.topology.find(n).unwrap().ports[0].ip.unwrap();
    // DHCP per VLAN: Staff in 192.168.10.0/24, Guest in 192.168.30.0/24.
    let (staff, guest) = (ip(&s, "Laptop1"), ip(&s, "Laptop2"));
    assert!(staff.to_string().starts_with("192.168.10."), "{staff}");
    assert!(guest.to_string().starts_with("192.168.30."), "{guest}");
    s.execute("cli.exec", json!({"device": "WLC1", "line": "enable"})).unwrap();
    let wlan = s.execute("cli.exec", json!({"device": "WLC1", "line": "show wlan"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(wlan.contains("VLAN") && wlan.contains(" 10 ") && wlan.contains(" 30 "), "{wlan}");
    s.execute("cli.exec", json!({"device": "LAP1", "line": "enable"})).unwrap();
    let capwap = s.execute("cli.exec", json!({"device": "LAP1", "line": "show capwap"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(capwap.contains("WLAN VLANs 10,30: tagged on the trunk uplink"), "{capwap}");
    // Between the WLANs, through R1: tagged 10 on LAP1's uplink, 30 on LAP2's.
    s.execute("sim.reset", json!({})).unwrap();
    let to = guest.addr.to_string();
    // (The first may be lost while R1 resolves Laptop2's address, as on a real router.)
    let marks = s.execute("net.ping", json!({"from": "Laptop1", "to": to, "count": 3})).unwrap()["marks"].as_str().unwrap().to_string();
    assert!(marks.ends_with("!!"), "{marks}");
    let tags: std::collections::BTreeSet<u16> =
        s.sim.trace().iter().filter_map(|e| netlabcraft_wire::Frame::decode(&e.frame).ok().and_then(|f| f.vlan)).collect();
    assert!(tags.contains(&10) && tags.contains(&30), "{tags:?}");
    // A Staff broadcast stays in VLAN 10: Laptop2 never hears Laptop1's ARP.
    let l2 = netlabcraft_model::End { device: s.topology.resolve("Laptop2").unwrap(), port: 0 };
    let l1mac = s.topology.find("Laptop1").unwrap().ports[0].mac;
    assert!(!s.sim.trace().iter().any(|e| e.to == l2 && netlabcraft_wire::Frame::decode(&e.frame).is_ok_and(|f| f.src == l1mac)));
    // Without the VLAN, Staff is back in the management subnet, where R1 has no pool for it.
    s.execute("wlc.wlan", json!({"device": "WLC1", "id": 1, "ssid": "Staff", "security": "enterprise"})).unwrap();
    let run = s.execute("cli.exec", json!({"device": "WLC1", "line": "show running-config"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(run.contains("wlan 1 Staff enterprise\n"), "{run}");
    s.execute("wlc.wlan", json!({"device": "WLC1", "id": 1, "ssid": "Staff", "security": "enterprise", "vlan": 10})).unwrap();
    assert!(s.execute("wlc.wlan", json!({"device": "WLC1", "id": 1, "ssid": "Staff", "security": "enterprise", "vlan": 5000})).is_err());
    assert!(s.execute("wlc.wlan", json!({"device": "WLC1", "id": 1, "ssid": "Staff", "security": "enterprise", "vlan": "x"})).is_err());

    // Laptop1 walks over to LAP2: it reassociates, logs in again and keeps its address.
    s.execute("sim.reset", json!({})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Laptop1", "to": "192.168.10.1", "count": 1})).unwrap()["marks"], "!");
    s.execute("device.move", json!({"device": "Laptop1", "x": 540.0, "y": 400.0})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Laptop1", "to": "192.168.10.1", "count": 2})).unwrap()["marks"], "!!");
    let tr = trace_text(&s);
    assert!(tr.contains("802.11 reassociation request: \"Staff\"") && tr.contains("802.11 reassociation response: success"), "{tr}");
    assert_eq!(ip(&s, "Laptop1"), staff);
    let lap2 = s.topology.resolve("LAP2").unwrap();
    assert_eq!(
        s.topology.wifi_ap_of(netlabcraft_model::End { device: s.topology.resolve("Laptop1").unwrap(), port: 0 }).map(|e| e.device),
        Some(lap2)
    );
    // Beacons show up while time passes.
    s.execute("sim.advance", json!({"seconds": 5})).unwrap();
    assert!(trace_text(&s).contains("802.11 beacon: \"Staff\""));
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
    assert!(!netlabcraft_sim::port_up(&s.topology, netlabcraft_model::End { device: pc3, port: 0 }));
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

#[test]
fn the_firewall_sample_publishes_its_web_server_and_keeps_the_office_private() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "firewall"})).unwrap();
    let ip = |s: &Session, n: &str| s.topology.find(n).unwrap().ports[0].ip.unwrap().addr.to_string();
    assert!(ip(&s, "PC1").starts_with("192.168.10.1"), "PC1 got its address from FW1's DHCP server");
    let cli = |s: &mut Session, dev: &str, line: &str| {
        s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap().to_string()
    };
    cli(&mut s, "FW1", "enable");
    // The office gets out, sharing FW1's outside address.
    s.execute("net.ping", json!({"from": "PC1", "to": "198.51.100.10", "count": 4})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "PC1", "to": "198.51.100.10", "count": 2})).unwrap()["marks"], "!!");
    let xlate = cli(&mut s, "FW1", "show xlate");
    assert!(xlate.contains("→ outside 203.0.113.2:"), "{xlate}");
    assert!(cli(&mut s, "FW1", "show conn").contains("icmp inside"));
    // Home reaches the web server's published address, by web and by ping.
    let page = s.execute("browser.get", json!({"device": "Home", "url": "http://203.0.113.10/"})).unwrap();
    assert!(page["status"].as_str().unwrap().starts_with("200"), "{page}");
    s.execute("net.ping", json!({"from": "Home", "to": "203.0.113.10", "count": 4})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Home", "to": "203.0.113.10", "count": 2})).unwrap()["marks"], "!!");
    let acl = cli(&mut s, "FW1", "show access-list");
    assert!(acl.contains("-- Publish the web server, and let it be pinged"), "{acl}");
    assert!(acl.contains("permit tcp any object WEB object-group WEB_PORTS  (hits 1)"), "{acl}");
    // FW1 is not a hop: PC1's traceroute starts at the ISP, whose answers find their way back
    // through the shared address.
    let tr = cli(&mut s, "PC1", "tracert 198.51.100.10");
    assert!(tr.contains("1  203.0.113.1") && tr.contains("2  198.51.100.10") && !tr.contains("192.168.10.1 "), "{tr}");
    let sp = cli(&mut s, "FW1", "show service-policy");
    assert!(sp.contains("inspect icmp:") && !sp.contains("inspect icmp: 0 "), "{sp}");
    assert!(cli(&mut s, "FW1", "show nat detail").contains("Section 2, object NAT"));
    // FW1 doesn't answer pings on its outside address.
    s.execute("sim.reset", json!({})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Home", "to": "203.0.113.2", "count": 2})).unwrap()["marks"], "..");
    assert!(trace_text(&s).contains("DROPPED: denied by icmp rule 1 on outside"), "{}", trace_text(&s));
    // But not the office, even with a route to it: the drop says why.
    cli(&mut s, "ISP", "enable");
    cli(&mut s, "ISP", "configure terminal");
    cli(&mut s, "ISP", "ip route 192.168.10.0 255.255.255.0 203.0.113.2");
    cli(&mut s, "ISP", "end");
    s.execute("sim.reset", json!({})).unwrap();
    let pc1 = ip(&s, "PC1");
    assert_eq!(s.execute("net.ping", json!({"from": "Home", "to": pc1, "count": 2})).unwrap()["marks"], "..");
    let tr = trace_text(&s);
    assert!(tr.contains("DROPPED: denied by access-group OUTSIDE_IN on outside (no rule matched)"), "{tr}");
    // Unbound, the security levels decide.
    cli(&mut s, "FW1", "configure terminal");
    cli(&mut s, "FW1", "no access-group OUTSIDE_IN in interface outside");
    cli(&mut s, "FW1", "end");
    s.execute("sim.reset", json!({})).unwrap();
    s.execute("net.ping", json!({"from": "Home", "to": pc1, "count": 3})).unwrap();
    assert!(trace_text(&s).contains("DROPPED: denied: from security level 0 to 100 without an access rule"), "{}", trace_text(&s));
    let run = cli(&mut s, "FW1", "show running-config");
    assert!(run.contains("nat (dmz,outside) static 203.0.113.10") && run.contains("dhcpd enable inside"), "{run}");
}

#[test]
fn cables_have_kinds_that_fit_their_ports() {
    let mut s = Session::new();
    for (name, model) in [("R1", "Router-M2"), ("R2", "Router-3"), ("S1", "Switch-24"), ("F1", "Switch-24-SFP"), ("PC1", "PC"), ("PC2", "PC")] {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
    }
    // A fiber cable needs fiber at both ends; the error says which port is not.
    let e = s.execute("link.add", json!({"a": "F1", "a_port": "g0/1", "b": "S1", "b_port": "fa0/1", "cable": "fiber"})).unwrap_err().to_string();
    assert!(e.contains("a fiber cable needs a fiber port on both ends: S1 Fa0/1 is copper"), "{e}");
    let e = s.execute("link.add", json!({"a": "R2", "b": "S1", "cable": "rope"})).unwrap_err().to_string();
    assert!(e.contains("`cable` must be one of"), "{e}");
    // With no port named, the first port the cable fits is taken (the combo uplink on S1).
    let v = s.execute("link.add", json!({"a": "F1", "b": "S1", "cable": "fiber"})).unwrap();
    assert_eq!(v["a"], "F1 GigabitEthernet0/1");
    assert_eq!(v["b"], "S1 GigabitEthernet0/1");
    assert_eq!(v["resolved"], "fiber");
    let v = s.execute("link.add", json!({"a": "PC1", "b": "S1", "b_port": "fa0/2"})).unwrap();
    assert_eq!((v["cable"].as_str(), v["resolved"].as_str()), (Some("auto"), Some("straight-through")));
    // PC to PC with a straight-through: cabled, but down with the reason.
    s.execute("link.add", json!({"a": "PC2", "b": "R2", "cable": "straight-through"})).unwrap();
    s.execute("link.remove", json!({"device": "PC2", "port": "Fa0"})).unwrap();
    s.execute("device.add", json!({"model": "PC", "name": "PC3"})).unwrap();
    s.execute("link.add", json!({"a": "PC2", "b": "PC3", "cable": "straight-through"})).unwrap();
    let list = s.execute("link.list", json!({})).unwrap();
    let pcs = list.as_array().unwrap().iter().find(|l| l["a"] == "PC2 FastEthernet0").unwrap();
    assert_eq!(pcs["up"], false);
    assert!(pcs["problem"].as_str().unwrap().contains("cross-over"));
    let out = s.execute("cli.exec", json!({"device": "S1", "line": "show interfaces fa0/2"})).unwrap();
    assert!(out["output"].as_str().unwrap().contains("automatic cable, working as straight-through"), "{out}");
    // A serial DTE cable: the far end is the DCE.
    s.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    s.execute("device.module.add", json!({"device": "R1", "slot": "0/0", "module": "Card-2S"})).unwrap();
    s.execute("device.module.add", json!({"device": "R1", "slot": "0/1", "module": "Card-1GF"})).unwrap();
    s.execute("device.power", json!({"device": "R1", "on": true})).unwrap();
    let e =
        s.execute("link.add", json!({"a": "R1", "a_port": "s0/0/0", "b": "R2", "b_port": "g0/1", "cable": "serial-dte"})).unwrap_err().to_string();
    assert!(e.contains("serial port on both ends") && e.contains("R2 Gi0/1 is copper"), "{e}");
    s.execute("link.add", json!({"a": "R1", "a_port": "s0/0/0", "b": "R1", "b_port": "s0/0/1", "cable": "serial-dte"})).unwrap();
    let r1 = s.topology.find("R1").unwrap().id;
    assert_eq!(s.topology.dce_of(netlabcraft_model::End { device: r1, port: 2 }), Some(netlabcraft_model::End { device: r1, port: 3 }));
    // The fiber card's port takes fiber only.
    let e = s
        .execute("link.add", json!({"a": "R1", "a_port": "g0/1/0", "b": "S1", "b_port": "fa0/5", "cable": "straight-through"}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("R1 Gi0/1/0 is fiber"), "{e}");
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/1/0", "b": "S1", "b_port": "g0/2"})).unwrap();
    let ports = s.execute("device.list", json!({})).unwrap();
    let r1j = ports.as_array().unwrap().iter().find(|d| d["name"] == "R1").unwrap();
    let card = r1j["ports"].as_array().unwrap().iter().find(|p| p["name"] == "GigabitEthernet0/1/0").unwrap();
    assert_eq!((card["medium"].as_str(), card["cable"].as_str()), (Some("fiber"), Some("auto")));
    assert_eq!(r1j["console"]["port"], "Console");
}

#[test]
fn old_lab_files_load_with_automatic_cables_and_kinds_round_trip() {
    // A lab saved before cable types: links are a pair of ends.
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "lan"})).unwrap();
    let saved = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    assert!(!saved.contains("\"cable\"") && !saved.contains("consoles"));
    let mut old = Session::new();
    old.execute("file.open", json!({"json": saved})).unwrap();
    assert!(old.topology.links.iter().all(|l| l.cable == netlabcraft_model::cable::Cable::Auto));
    assert_eq!(old.topology.links, s.topology.links);
    // Kinds and console cables are saved and read back.
    s.execute("device.add", json!({"model": "Switch-24", "name": "S9"})).unwrap();
    s.execute("device.add", json!({"model": "PC", "name": "PC9"})).unwrap();
    s.execute("link.add", json!({"a": "PC9", "b": "S9", "cable": "cross-over"})).unwrap();
    s.execute("link.add", json!({"a": "S9", "b": "PC9", "cable": "console"})).unwrap();
    let saved = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    assert!(saved.contains("\"cross-over\"") && saved.contains("consoles"));
    let mut back = Session::new();
    back.execute("file.open", json!({"json": saved})).unwrap();
    assert_eq!(back.topology.links, s.topology.links);
    assert_eq!(back.topology.consoles, s.topology.consoles);
}

#[test]
fn a_pc_terminal_reaches_the_console_of_the_device_its_cable_leads_to() {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Router-3", "name": "R1"})).unwrap();
    s.execute("device.add", json!({"model": "Laptop", "name": "L1"})).unwrap();
    s.execute("device.add", json!({"model": "PC", "name": "PC2"})).unwrap();
    s.execute("device.add", json!({"model": "Server", "name": "Srv"})).unwrap();
    let e = s.execute("pc.terminal", json!({"pc": "L1"})).unwrap_err().to_string();
    assert!(e.contains("no console cable"), "{e}");
    assert!(s.execute("pc.terminal", json!({"pc": "R1"})).is_err());
    let e = s.execute("link.add", json!({"a": "Srv", "b": "R1", "cable": "console"})).unwrap_err().to_string();
    assert!(e.contains("Srv has no console or RS-232 port"), "{e}");
    let v = s.execute("link.add", json!({"a": "R1", "a_port": "Console", "b": "L1", "b_port": "RS-232"})).unwrap();
    assert_eq!((v["a"].as_str(), v["b"].as_str()), (Some("L1 RS-232"), Some("R1 Console")));
    // One console port, one terminal.
    let e = s.execute("link.add", json!({"a": "PC2", "b": "R1", "cable": "console"})).unwrap_err().to_string();
    assert!(e.contains("R1 Console is already cabled"), "{e}");
    // No address needed: the terminal is the router's own console session.
    let t = s.execute("pc.terminal", json!({"pc": "L1"})).unwrap();
    assert_eq!(t["device"], "R1");
    assert_eq!(t["prompt"], "R1>");
    s.execute("cli.exec", json!({"device": t["device"], "line": "enable"})).unwrap();
    s.execute("cli.exec", json!({"device": "R1", "line": "configure terminal"})).unwrap();
    s.execute("cli.exec", json!({"device": "R1", "line": "hostname Core"})).unwrap();
    let t = s.execute("pc.terminal", json!({"pc": "L1"})).unwrap();
    assert_eq!(t["device"], "Core");
    assert_eq!(t["prompt"], "Core(config)#");
    // Not a network interface: not in the interface table, carries no frames.
    let brief = s.execute("cli.exec", json!({"device": "Core", "line": "do show ip interface brief"})).unwrap();
    assert!(!brief["output"].as_str().unwrap().contains("Console"));
    let list = s.execute("link.list", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["cable"], "console");
    s.execute("link.remove", json!({"device": "L1", "port": "RS-232"})).unwrap();
    assert!(s.execute("pc.terminal", json!({"pc": "L1"})).is_err());
    assert!(s.execute("link.remove", json!({"device": "L1", "port": "RS-232"})).is_err());
}

#[test]
fn wifi_range_follows_the_buildings_of_the_physical_view() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wifi"})).unwrap();
    let ping = s.execute("net.ping", json!({"from": "PC1", "to": "192.168.10.10", "count": 2})).unwrap();
    assert_ne!(ping["replies"], 0, "{ping}");
    // Laptop1 moves to another building: the access point's walls are between them.
    let b = s.execute("place.add", json!({"kind": "building", "name": "Annex", "x": 500, "y": 220})).unwrap()["id"].as_u64().unwrap();
    s.execute("device.place", json!({"device": "Laptop1", "place": b})).unwrap();
    let ping = s.execute("net.ping", json!({"from": "PC1", "to": "192.168.10.10", "count": 2})).unwrap();
    assert_eq!(ping["replies"], 0, "{ping}");
    // Back in the main building it joins again.
    s.execute("device.place", json!({"device": "Laptop1", "place": 2})).unwrap();
    let ping = s.execute("net.ping", json!({"from": "PC1", "to": "192.168.10.10", "count": 4})).unwrap();
    assert_ne!(ping["replies"], 0, "{ping}");
}

/// The RADIUS packets of the event list (UDP 1812 either way), decoded, with their direction.
fn radius_packets(s: &Session) -> Vec<(bool, netlabcraft_wire::radius::Radius)> {
    use netlabcraft_wire::radius::{RADIUS_PORT, Radius};
    s.sim
        .trace()
        .iter()
        .filter_map(|e| {
            let f = netlabcraft_wire::Frame::decode(&e.frame).ok()?;
            let ip = netlabcraft_wire::Ipv4::decode(&f.payload).ok()?;
            let u = netlabcraft_wire::Udp::decode(&ip.payload, ip.src, ip.dst).ok()?;
            if u.dst_port != RADIUS_PORT && u.src_port != RADIUS_PORT {
                return None;
            }
            Some((u.dst_port == RADIUS_PORT, Radius::decode(&u.payload).ok()?))
        })
        .collect()
}

#[test]
fn peap_runs_inside_radius_with_message_authenticators_and_keys_the_handshake() {
    use netlabcraft_wire::radius::{ACCESS_ACCEPT, ACCESS_CHALLENGE, ACCESS_REQUEST};
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wlc"})).unwrap();
    s.execute("sim.reset", json!({})).unwrap();
    assert_eq!(s.execute("net.ping", json!({"from": "Laptop1", "to": "192.168.10.1", "count": 2})).unwrap()["marks"], "!!");
    let tr = trace_text(&s);
    // The EAP exchange, in order: identity, the tunnel's handshake, the protected inner login,
    // success, then the 4-way handshake keyed by the tunnel's session key.
    let order = [
        "EAP request: identity?",
        "EAP response: identity \"ana\"",
        "EAP request: PEAP, start",
        "EAP response: PEAP, TLS handshake (client hello)",
        "EAP request: PEAP, TLS handshake (server hello, finished)",
        "EAP response: PEAP, TLS handshake (finished)",
        "EAP request: PEAP, TLS application data",
        "EAP response: PEAP, TLS application data",
        "EAP success",
        "4-way handshake message 1 of 4",
        "4-way handshake message 4 of 4",
    ];
    let mut at = 0;
    for want in order {
        let found = tr.get(at..).and_then(|rest| rest.find(want)).unwrap_or_else(|| panic!("missing {want} after the previous step:\n{tr}"));
        at += found + want.len();
    }
    assert!(!tr.contains("bad MIC") && !tr.contains("DROPPED: RADIUS"), "{tr}");
    // The password is nowhere on the air or the wire, in any form the capture shows.
    for e in s.sim.trace() {
        for bytes in [Some(&e.frame), e.air.as_ref()].into_iter().flatten() {
            assert!(!bytes.windows(8).any(|w| w == b"ana12345"), "the password crossed in the clear: {}", e.summary);
        }
    }
    // RADIUS carries the EAP: requests from the controller and challenges, then an accept with
    // the MPPE key; every packet signed with a Message-Authenticator that checks out.
    let packets = radius_packets(&s);
    assert!(packets.len() >= 12, "{}", packets.len());
    let mut last_request = None;
    for (to_server, r) in &packets {
        assert!(r.eap_message().is_some(), "{r:?}");
        assert!(r.attribute(netlabcraft_wire::radius::MESSAGE_AUTHENTICATOR).is_some_and(|m| m.len() == 16));
        if *to_server {
            assert_eq!(r.code, ACCESS_REQUEST);
            assert!(r.checks_message(&r.authenticator, "wlcsecret"));
            assert!(!r.checks_message(&r.authenticator, "not-the-secret"));
            assert!(r.hidden_password().is_none(), "no PAP shortcut");
            last_request = Some(r.authenticator);
        } else {
            let auth = last_request.expect("an answer before any request");
            assert!(r.verifies(&auth, "wlcsecret") && r.checks_message(&auth, "wlcsecret"));
            assert!(matches!(r.code, ACCESS_CHALLENGE | ACCESS_ACCEPT), "{}", r.name());
            if r.code == ACCESS_CHALLENGE {
                assert!(r.state().is_some());
            } else {
                assert_eq!(r.mppe_recv_key(&auth, "wlcsecret").map(|k| k.len()), Some(32));
            }
        }
    }
    assert!(packets.iter().any(|(_, r)| r.code == ACCESS_ACCEPT));
    assert!(tr.contains("RADIUS Access-Challenge") && tr.contains("carrying EAP"), "{tr}");

    // A wrong password: the server says so inside the tunnel, then rejects; the host is sent away.
    s.execute("sim.reset", json!({})).unwrap();
    s.execute("device.wireless", json!({"device": "Laptop1", "ssid": "Staff", "user": "ana", "password": "nope-nope"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    let tr = trace_text(&s);
    let reject = tr.find("RADIUS Access-Reject").unwrap_or_else(|| panic!("no reject:\n{tr}"));
    let failure = tr.find("EAP failure").unwrap_or_else(|| panic!("no EAP failure:\n{tr}"));
    assert!(tr.find("TLS application data").is_some_and(|a| a < reject) && reject < failure, "{tr}");
    assert!(tr.contains("IEEE 802.1X authentication failed"), "{tr}");
}

#[test]
fn a_centrally_switched_wlan_tunnels_its_hosts_through_the_controller_into_its_vlan() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wlc"})).unwrap();
    // The controller's port becomes a trunk (as a controller's uplink is), and Staff switches
    // centrally into VLAN 10.
    s.execute("device.configure", json!({"device": "S1", "lines": ["interface fa0/1", "switchport mode trunk"]})).unwrap();
    s.execute("device.configure", json!({"device": "WLC1", "lines": ["interface g0/1", "switchport mode trunk"]})).unwrap();
    let r = s
        .execute("wlc.wlan", json!({"device": "WLC1", "id": 1, "ssid": "Staff", "security": "enterprise", "vlan": 10, "switching": "central"}))
        .unwrap();
    assert_eq!(r["config"], "wlan 1 Staff enterprise vlan 10 central");
    assert!(s.execute("wlc.wlan", json!({"device": "WLC1", "id": 1, "ssid": "Staff", "switching": "sideways"})).is_err());
    s.execute("cli.exec", json!({"device": "WLC1", "line": "enable"})).unwrap();
    let run = s.execute("cli.exec", json!({"device": "WLC1", "line": "show running-config"})).unwrap()["output"].as_str().unwrap().to_string();
    assert!(run.contains("wlan 1 Staff enterprise vlan 10 central\n"), "{run}");
    // The APs no longer carry VLAN 10 themselves.
    let lap1 = s.topology.resolve("LAP1").unwrap();
    assert!(!s.topology.lap_vlans(lap1).iter().any(|v| v.get() == 10));
    // A fresh address in VLAN 10 through the tunnel, then the gateway answers.
    s.execute("sim.reset", json!({})).unwrap();
    s.execute("cli.exec", json!({"device": "Laptop1", "line": "ipconfig /renew"})).unwrap();
    let ip = s.topology.find("Laptop1").unwrap().ports[0].ip.unwrap();
    assert!(ip.to_string().starts_with("192.168.10."), "{ip}");
    let marks = s.execute("net.ping", json!({"from": "Laptop1", "to": "192.168.10.1", "count": 3})).unwrap()["marks"].as_str().unwrap().to_string();
    assert!(marks.ends_with("!!"), "{marks}\n{}", trace_text(&s));
    let tr = trace_text(&s);
    assert!(tr.contains("CAPWAP data (central switching) carrying"), "{tr}");
    assert!(tr.contains("add WLAN 1 \"Staff\" (WPA2-Enterprise, central switching)") || tr.contains("central switching"), "{tr}");
    // The echo requests cross the LAP's uplink inside CAPWAP (UDP 5247 to the controller), and
    // leave the controller's own port tagged 10; the LAP itself sends nothing tagged 10.
    let wlc = s.topology.resolve("WLC1").unwrap();
    let wlc_addr: std::net::Ipv4Addr = "192.168.20.2".parse().unwrap();
    let mut tunnelled = 0;
    let mut from_wlc_tagged = 0;
    for e in s.sim.trace() {
        let Ok(f) = netlabcraft_wire::Frame::decode(&e.frame) else { continue };
        if e.from.device == lap1 || e.to.device == lap1 {
            if e.air.is_none() && e.from.device == lap1 {
                assert_ne!(f.vlan, Some(10), "the LAP bridged a VLAN 10 frame onto its uplink: {}", e.summary);
            }
            if let Ok(ip) = netlabcraft_wire::Ipv4::decode(&f.payload)
                && let Ok(u) = netlabcraft_wire::Udp::decode(&ip.payload, ip.src, ip.dst)
                && u.dst_port == netlabcraft_wire::capwap::DATA_PORT
                && ip.dst == wlc_addr
                && let Ok((inner, false)) = netlabcraft_wire::capwap::data_frame(&u.payload)
            {
                let inner = netlabcraft_wire::Frame::decode(&inner).unwrap();
                assert_eq!(inner.vlan, Some(10));
                tunnelled += 1;
            }
        }
        if e.from.device == wlc && f.vlan == Some(10) && f.ethertype == netlabcraft_wire::ETHERTYPE_IPV4 {
            from_wlc_tagged += 1;
        }
    }
    assert!(tunnelled >= 3, "{tunnelled}");
    assert!(from_wlc_tagged >= 3, "{from_wlc_tagged}");
    // Back to local switching: the AP bridges again, tagging VLAN 10 on its own trunk.
    s.execute("wlc.wlan", json!({"device": "WLC1", "id": 1, "ssid": "Staff", "security": "enterprise", "vlan": 10, "switching": "local"})).unwrap();
    assert!(s.topology.lap_vlans(lap1).iter().any(|v| v.get() == 10));
    s.execute("sim.reset", json!({})).unwrap();
    let marks = s.execute("net.ping", json!({"from": "Laptop1", "to": "192.168.10.1", "count": 3})).unwrap()["marks"].as_str().unwrap().to_string();
    assert!(marks.ends_with("!!"), "{marks}");
    assert!(!trace_text(&s).contains("central switching) carrying"));
}

#[test]
fn resizing_a_drawing_is_bounded_normalised_and_undone() {
    let mut s = Session::new();
    let id = s.execute("note.add", json!({"shape": "rectangle", "x": 10, "y": 10, "w": 50, "h": 40})).unwrap()["id"].as_u64().unwrap();
    let get = |s: &Session| s.topology.annotations.iter().find(|a| a.id as u64 == id).map(|a| (a.x, a.y, a.w, a.h)).unwrap();
    s.execute("note.resize", json!({"id": id, "x": 0, "y": 0, "w": 120, "h": 90})).unwrap();
    assert_eq!(get(&s), (0.0, 0.0, 120.0, 90.0));
    // Dragged past the opposite corner: flipped and normalised.
    s.execute("note.resize", json!({"id": id, "x": 100, "y": 100, "w": -60, "h": -30})).unwrap();
    assert_eq!(get(&s), (40.0, 70.0, 60.0, 30.0));
    // Minimum and maximum size.
    s.execute("note.resize", json!({"id": id, "x": 5, "y": 5, "w": 0, "h": 0.5})).unwrap();
    assert_eq!(get(&s), (5.0, 5.0, 4.0, 4.0));
    s.execute("note.resize", json!({"id": id, "x": 0, "y": 0, "w": 1e300, "h": 1e9})).unwrap();
    assert_eq!(get(&s), (0.0, 0.0, 100_000.0, 100_000.0));
    for bad in [json!({"id": id}), json!({"id": id, "x": "a", "y": 0, "w": 1, "h": 1}), json!({"id": 999, "x": 0, "y": 0, "w": 9, "h": 9})] {
        assert!(s.execute("note.resize", bad).is_err());
    }
    // A transient resize adds no undo step.
    s.execute("note.resize", json!({"id": id, "x": 1, "y": 1, "w": 20, "h": 20, "transient": true})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(get(&s), (5.0, 5.0, 4.0, 4.0));
    // Lines keep direction and need some reach.
    let line = s.execute("note.add", json!({"shape": "line", "x": 0, "y": 0, "w": 30, "h": -30})).unwrap()["id"].as_u64().unwrap();
    s.execute("note.resize", json!({"id": line, "x": 10, "y": 10, "w": -50, "h": 0})).unwrap();
    assert!(s.execute("note.resize", json!({"id": line, "x": 0, "y": 0, "w": 1, "h": 1})).is_err());
    // Text and paths are refused.
    let text = s.execute("note.add", json!({"x": 0, "y": 0, "text": "hi"})).unwrap()["id"].as_u64().unwrap();
    assert!(s.execute("note.resize", json!({"id": text, "x": 0, "y": 0, "w": 10, "h": 10})).is_err());
}
