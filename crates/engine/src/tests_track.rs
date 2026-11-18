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
