//! A scheduled reload happens when `sim.advance` takes the clock past it.

use serde_json::json;

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

#[test]
fn sim_advance_restarts_a_device_whose_reload_came_due() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router"})).unwrap();
    for line in ["enable", "configure terminal", "hostname KEPT", "end", "write memory", "configure terminal", "hostname LOST", "end", "reload in 2"]
    {
        cli(&mut s, "R1", line);
    }
    // Not yet.
    s.execute("sim.advance", json!({"seconds": 60})).unwrap();
    assert!(cli(&mut s, "LOST", "show reload").contains("scheduled in"));
    s.execute("sim.advance", json!({"seconds": 120})).unwrap();
    // Restarted from the saved configuration, and its console starts again.
    let shown = cli(&mut s, "KEPT", "show reload");
    assert!(shown.contains("No restart is scheduled"), "{shown}");
    assert!(s.console(s.topology.devices[0].id).log.contains("scheduled reload"));
}
