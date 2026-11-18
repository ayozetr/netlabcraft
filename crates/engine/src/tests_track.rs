//! Object tracking through the engine: the settled state moves along after every command.

use netlabcraft_model::DeviceKind;
use serde_json::json;

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

#[test]
fn an_object_settles_after_the_command_that_made_it() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
    let name = s.topology.devices.iter().find(|d| d.kind == DeviceKind::Router).map(|d| d.name.clone()).unwrap();
    for l in ["en", "conf t", "track 1 ip route 203.0.113.0/24 reachability", "delay up 5", "end"] {
        assert!(!cli(&mut s, &name, l).starts_with("% "), "{l}");
    }
    let d = s.topology.find(&name).unwrap();
    let r = d.track.reported.get(&1).copied().unwrap();
    assert!(!r.up, "no such route");
    let out = cli(&mut s, &name, "show track 1");
    assert!(out.contains("State is Down") && out.contains("last change"), "{out}");
}

#[test]
fn an_ip_sla_probe_is_sent_after_the_command_and_the_object_follows_it() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
    let router = s.topology.devices.iter().find(|d| d.kind == DeviceKind::Router).unwrap();
    let name = router.name.clone();
    // An address of the router itself answers at once, whatever the cabling.
    let own = router.ports.iter().find_map(|p| p.ip).unwrap().addr;
    let lines = [
        "en".to_string(),
        "conf t".into(),
        "ip sla 1".into(),
        format!("icmp-echo {own}"),
        "frequency 5".into(),
        "exit".into(),
        "ip sla schedule 1 life forever start-time now".into(),
        "track 1 ip sla 1 state".into(),
        "end".into(),
    ];
    for l in &lines {
        assert!(!cli(&mut s, &name, l).starts_with("% "), "{l}");
    }
    let stats = cli(&mut s, &name, "show ip sla statistics 1");
    assert!(stats.contains("1 sent, 1 answered") && stats.contains("Latest outcome: OK"), "{stats}");
    assert!(cli(&mut s, &name, "show track 1").contains("State is Up"));
    // Time passes: the next command that runs after it sends the next probe.
    s.execute("sim.advance", json!({"seconds": 12})).unwrap();
    let stats = cli(&mut s, &name, "show ip sla statistics 1");
    assert!(!stats.contains("1 sent"), "more than one probe was sent after twelve seconds at 5 s: {stats}");
}
