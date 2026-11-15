//! Engine tests for BGP communities, aggregation and timers: configured through
//! `device.configure`, run with `sim.advance`, saved and opened again.

use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn configure(s: &mut Session, dev: &str, lines: &[&str]) {
    let r = s.execute("device.configure", json!({"device": dev, "lines": lines})).unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
}

/// R1 (AS 65001) tags 172.16.1.0/24 with a community and summarises two prefixes; R2 (AS 65002)
/// matches the community.
fn pair() -> Session {
    let mut s = Session::new();
    for _ in 0..2 {
        s.execute("device.add", json!({"kind": "router"})).unwrap();
    }
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"})).unwrap();
    configure(
        &mut s,
        "R1",
        &[
            "interface g0/1",
            "ip address 10.0.12.1 255.255.255.252",
            "no shutdown",
            "interface loopback 1",
            "ip address 172.16.1.1 255.255.255.0",
            "interface loopback 2",
            "ip address 172.16.2.1 255.255.255.0",
            "exit",
            "ip prefix-list ONE permit 172.16.1.0/24",
            "route-map TAG permit 10",
            "match ip address prefix-list ONE",
            "set community 65001:100",
            "exit",
            "route-map TAG permit 20",
            "exit",
            "router bgp 65001",
            "bgp router-id 1.1.1.1",
            "neighbor 10.0.12.2 remote-as 65002",
            "neighbor 10.0.12.2 send-community",
            "neighbor 10.0.12.2 route-map TAG out",
            "neighbor 10.0.12.2 timers 10 30",
            "network 172.16.1.0 mask 255.255.255.0",
            "network 172.16.2.0 mask 255.255.255.0",
            "aggregate-address 172.16.0.0 255.255.252.0 summary-only",
        ],
    );
    configure(
        &mut s,
        "R2",
        &[
            "ip bgp-community new-format",
            "ip community-list standard TAGGED permit 65001:100",
            "route-map PREF permit 10",
            "match community TAGGED",
            "set local-preference 300",
            "exit",
            "interface g0/1",
            "ip address 10.0.12.2 255.255.255.252",
            "no shutdown",
            "exit",
            "router bgp 65002",
            "bgp router-id 2.2.2.2",
            "neighbor 10.0.12.1 remote-as 65001",
            "neighbor 10.0.12.1 route-map PREF in",
        ],
    );
    s.execute("sim.advance", json!({"seconds": 10})).unwrap();
    s
}

#[test]
fn communities_aggregation_and_timers_work_through_the_engine_and_survive_a_save() {
    let mut s = pair();
    let table = cli(&mut s, "R2", "show ip bgp");
    assert!(table.contains("172.16.0.0/22") && !table.contains("172.16.1.0/24"), "summary-only hides the specifics: {table}");
    let n = cli(&mut s, "R2", "show ip bgp neighbors 10.0.12.1");
    assert!(n.contains("Hold time 30 s, keepalive every 10 s"), "{n}");
    // The aggregate is not in the prefix list, so it carries no community and keeps the default preference.
    assert!(cli(&mut s, "R2", "show ip bgp 172.16.0.0 255.255.252.0").contains("local preference 100"));
    // Without summary-only the tagged prefix arrives with its community and a raised preference.
    configure(&mut s, "R1", &["router bgp 65001", "aggregate-address 172.16.0.0 255.255.252.0"]);
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    let one = cli(&mut s, "R2", "show ip bgp 172.16.1.0");
    assert!(one.contains("communities: 65001:100") && one.contains("local preference 300"), "{one}");
    assert!(cli(&mut s, "R2", "show ip bgp 172.16.2.0").contains("local preference 100"));
    // Saved and opened again, the configuration and the result are the same.
    let saved = s.execute("file.save", json!({})).unwrap();
    assert!(saved["json"].as_str().unwrap().contains("send_community"), "the file keeps the setting");
    let mut again = Session::new();
    again.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(again.topology, s.topology);
    again.execute("sim.advance", json!({"seconds": 10})).unwrap();
    let back = cli(&mut again, "R2", "show ip bgp 172.16.1.0");
    assert!(back.contains("communities: 65001:100") && back.contains("local preference 300"), "{back}");
    let run = cli(&mut again, "R1", "show running-config");
    assert!(run.contains(" neighbor 10.0.12.2 timers 10 30\n neighbor 10.0.12.2 send-community\n"), "{run}");
    assert!(run.contains(" aggregate-address 172.16.0.0 255.255.252.0\n"), "{run}");
}

#[test]
fn a_hostile_file_has_its_communities_and_lists_cleaned() {
    let mut s = pair();
    let saved = s.execute("file.save", json!({})).unwrap();
    let text = saved["json"].as_str().unwrap();
    let mut hostile: Value = serde_json::from_str(text).unwrap();
    hostile["topology"]["devices"][1]["community_lists"] = json!([
        {"name": "A B\nC", "expanded": true, "entries": [{"permit": true, "regex": "["}, {"permit": true, "regex": "^65001:"}]},
        {"name": "empty", "entries": []}
    ]);
    hostile["topology"]["devices"][0]["bgp"]["aggregates"] = json!([
        {"net": "10.1.2.3/8"}, {"net": "10.0.0.0/8", "as_set": true}
    ]);
    hostile["topology"]["devices"][0]["bgp"]["maximum_paths"] = json!(200);
    let mut again = Session::new();
    again.execute("file.open", json!({"json": hostile.to_string()})).unwrap();
    let r2 = again.topology.find("R2").unwrap();
    assert_eq!(r2.community_lists.len(), 1);
    assert!(r2.community_lists[0].name.chars().all(|c| !c.is_whitespace() && !c.is_control()));
    assert_eq!(r2.community_lists[0].entries.len(), 1, "the unusable expression is gone");
    let b = again.topology.find("R1").unwrap().bgp.as_ref().unwrap();
    assert_eq!((b.aggregates.len(), b.maximum_paths), (1, Some(32)));
}
