//! Engine tests for NetFlow: a router counts and exports flows, a PC's Netflow Collector lists them.

use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn configure(s: &mut Session, lines: &[&str]) {
    let r = s.execute("device.configure", json!({"device": "R1", "lines": lines})).unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
}

/// PC1 (10.0.0.10) - R1 - PC2 (10.1.0.10, the collector); R1 counts flows on both ports and exports
/// to PC2 on port 2055.
fn lab() -> Session {
    let mut s = Session::new();
    for kind in ["pc", "router", "pc"] {
        s.execute("device.add", json!({"kind": kind})).unwrap();
    }
    s.execute("link.add", json!({"a": "PC1", "b": "R1", "b_port": "g0/0"})).unwrap();
    s.execute("link.add", json!({"a": "PC2", "b": "R1", "b_port": "g0/1"})).unwrap();
    cli(&mut s, "PC1", "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1");
    cli(&mut s, "PC2", "ipconfig 10.1.0.10 255.255.255.0 10.1.0.1");
    configure(
        &mut s,
        &[
            "interface g0/0",
            "ip address 10.0.0.1 255.255.255.0",
            "no shutdown",
            "ip flow ingress",
            "exit",
            "interface g0/1",
            "ip address 10.1.0.1 255.255.255.0",
            "no shutdown",
            "ip route-cache flow",
            "exit",
            "ip flow-export destination 10.1.0.10 2055",
        ],
    );
    s
}

fn flows(s: &mut Session) -> Value {
    s.execute("netflow.flows", json!({"device": "PC2"})).unwrap()
}

/// Pings through the router, then lets the flows go quiet long enough to be exported.
fn ping_and_wait(s: &mut Session) {
    let r = s.execute("net.ping", json!({"from": "PC1", "to": "10.1.0.10", "count": 3})).unwrap();
    assert!(r["marks"].as_str().unwrap_or_default().contains('!'), "{r}");
    s.sim.run_for(&s.topology, 20_000_000);
    s.sim.run_for(&s.topology, 2_000_000);
}

#[test]
fn a_router_exports_what_it_counted_and_the_collector_lists_it() {
    let mut s = lab();
    let st = s.execute("netflow.listen", json!({"device": "PC2"})).unwrap();
    assert_eq!(st["listening"], json!(true));
    assert_eq!(st["port"], json!(2055));
    cli(&mut s, "R1", "enable");
    ping_and_wait(&mut s);
    let got = flows(&mut s);
    assert!(got["datagrams"].as_u64().unwrap_or(0) >= 1, "{got}");
    assert_eq!(got["bad_datagrams"], json!(0));
    // One flow each way: the request on g0/0 and the answer on g0/1.
    assert_eq!(got["flows_received"], json!(2), "{got}");
    let list = got["flows"].as_array().unwrap();
    let req = list.iter().find(|f| f["src"] == "10.0.0.10").expect("the request flow");
    assert_eq!(req["dst"], json!("10.1.0.10"));
    assert_eq!(req["protocol"], json!("icmp"));
    assert_eq!(req["version"], json!(5));
    assert_eq!(req["exporter"], json!("10.1.0.1"));
    assert!(req["packets"].as_u64().unwrap_or(0) >= 2, "{req}");
    assert!(req["bytes"].as_u64().unwrap_or(0) >= 2 * 28, "{req}");
    assert_eq!(got["top_talkers"].as_array().map(Vec::len), Some(2));
    assert_eq!(got["protocols"][0]["protocol"], json!("icmp"));
    // The router's counters match what the collector saw.
    let export = cli(&mut s, "R1", "show ip flow export");
    assert!(export.contains("Flow export version 5") && export.contains("10.1.0.10 port 2055"), "{export}");
    assert!(export.contains("Exported 2 flows"), "{export}");
    // Nothing is left in the cache; clearing the collector empties it, stopping closes the port.
    assert!(cli(&mut s, "R1", "show ip cache flow").contains("0 active"));
    let cleared = s.execute("netflow.clear", json!({"device": "PC2"})).unwrap();
    assert_eq!(cleared["flows_received"], json!(0));
    assert_eq!(cleared["datagrams"], json!(0));
    let stopped = s.execute("netflow.stop", json!({"device": "PC2"})).unwrap();
    assert_eq!(stopped["listening"], json!(false));
    assert!(s.execute("netflow.stop", json!({"device": "PC2"})).is_err());
    ping_and_wait(&mut s);
    assert_eq!(flows(&mut s)["datagrams"], json!(0), "a stopped collector takes nothing");
}

#[test]
fn version_9_carries_its_template_and_the_collector_reads_it() {
    let mut s = lab();
    s.execute("netflow.listen", json!({"device": "PC2", "port": 9996})).unwrap();
    configure(&mut s, &["no ip flow-export destination 10.1.0.10 2055", "ip flow-export destination 10.1.0.10 9996", "ip flow-export version 9"]);
    ping_and_wait(&mut s);
    let got = flows(&mut s);
    assert_eq!(got["port"], json!(9996));
    assert_eq!(got["flows_received"], json!(2), "{got}");
    assert!(got["flows"].as_array().unwrap().iter().all(|f| f["version"] == json!(9)));
}

#[test]
fn the_cache_is_shown_and_cleared_and_only_counting_interfaces_count() {
    let mut s = lab();
    cli(&mut s, "R1", "enable");
    let r = s.execute("net.ping", json!({"from": "PC1", "to": "10.1.0.10", "count": 2})).unwrap();
    assert!(r["marks"].as_str().unwrap_or_default().contains('!'));
    let cache = cli(&mut s, "R1", "show ip cache flow");
    assert!(cache.contains("2 active") && cache.contains("10.0.0.10") && cache.contains("icmp"), "{cache}");
    assert!(cache.contains("GigabitEthernet0/0") && cache.contains("GigabitEthernet0/1"), "{cache}");
    assert!(cli(&mut s, "R1", "clear ip flow stats").trim().is_empty());
    assert!(cli(&mut s, "R1", "show ip cache flow").contains("0 active"));
    // Without the interface setting nothing is counted.
    configure(&mut s, &["interface g0/0", "no ip flow ingress", "exit", "interface g0/1", "no ip route-cache flow", "exit"]);
    s.execute("net.ping", json!({"from": "PC1", "to": "10.1.0.10", "count": 2})).unwrap();
    assert!(cli(&mut s, "R1", "show ip cache flow").contains("0 active"));
}

#[test]
fn the_configuration_is_written_and_read_back_and_a_collector_needs_a_desktop() {
    let mut s = lab();
    cli(&mut s, "R1", "enable");
    configure(&mut s, &["ip flow-export version 9"]);
    let run = cli(&mut s, "R1", "show running-config");
    assert!(run.contains("ip flow-export version 9\nip flow-export destination 10.1.0.10 2055"), "{run}");
    assert!(run.contains("interface GigabitEthernet0/0") && run.contains(" ip flow ingress"), "{run}");
    // Only two destinations, and a bad port or version is refused.
    configure(&mut s, &["ip flow-export destination 10.1.0.11 2055"]);
    let r = s.execute("device.configure", json!({"device": "R1", "lines": ["ip flow-export destination 10.1.0.12 2055"]})).unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(1), "{r}");
    let r = s
        .execute("device.configure", json!({"device": "R1", "lines": ["ip flow-export destination 10.1.0.12 0", "ip flow-export version 7"]}))
        .unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(2), "{r}");
    // The Collector is an app of end devices with a Desktop, not of routers; the port is saved.
    assert!(s.execute("netflow.listen", json!({"device": "R1"})).is_err());
    assert!(s.execute("netflow.listen", json!({"device": "PC2", "port": 0})).is_err());
    s.execute("netflow.listen", json!({"device": "PC2", "port": 4739})).unwrap();
    let json = netlabcraft_model::file::to_json(&s.topology).unwrap();
    let mut t = Session::new();
    t.execute("file.open", json!({"json": json})).unwrap();
    let again = t.execute("netflow.flows", json!({"device": "PC2"})).unwrap();
    assert_eq!(again["port"], json!(4739));
    assert_eq!(again["datagrams"], json!(0), "what was received is not saved");
    cli(&mut t, "R1", "enable");
    assert!(cli(&mut t, "R1", "show running-config").contains("ip flow-export destination 10.1.0.11 2055"));
}
