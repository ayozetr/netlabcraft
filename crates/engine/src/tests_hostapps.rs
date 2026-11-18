//! Engine tests for the Desktop's computer apps: the Telnet/SSH Client (which types into the
//! Command Prompt), the Text Editor's files and the Firewall.

use serde_json::{Value, json};

use crate::Session;

fn lab(sample: &str) -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": sample})).unwrap();
    s
}

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn ping_ok(s: &mut Session, from: &str, to: &str) -> bool {
    let r = s.execute("net.ping", json!({"from": from, "to": to, "count": 3})).unwrap();
    r["marks"].as_str().unwrap_or_default().contains('!')
}

#[test]
fn the_telnet_client_opens_a_router_session_through_the_command_prompt() {
    let mut s = lab("two-subnets");
    for line in ["enable", "configure terminal", "line vty 0 4", "password remote", "login", "end"] {
        cli(&mut s, "R1", line);
    }
    // What the Telnet/SSH Client's Open button types.
    assert!(cli(&mut s, "PC1", "telnet 10.0.1.1").contains("Open"));
    cli(&mut s, "PC1", "remote");
    assert!(cli(&mut s, "PC1", "enable").contains("No password set"), "the session is on the router");
    assert!(cli(&mut s, "PC1", "exit").contains("closed"));
    // An address nobody has opens nothing.
    assert!(!cli(&mut s, "PC1", "telnet 10.0.1.99").contains("Open"));
}

#[test]
fn text_files_are_saved_read_listed_and_deleted() {
    let mut s = lab("lan");
    assert_eq!(s.execute("editor.list", json!({"device": "PC1"})).unwrap()["files"], json!([]));
    let w = s.execute("editor.write", json!({"device": "PC1", "name": "notes.txt", "text": "first line\nsecond"})).unwrap();
    assert_eq!((w["created"].clone(), w["bytes"].clone()), (json!(true), json!(17)));
    let r = s.execute("editor.read", json!({"device": "PC1", "name": "notes.txt"})).unwrap();
    assert_eq!(r["text"], "first line\nsecond");
    // Saving again replaces the file, and undo takes that save back.
    let w = s.execute("editor.write", json!({"device": "PC1", "name": "notes.txt", "text": "v2"})).unwrap();
    assert_eq!(w["created"], json!(false));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("editor.read", json!({"device": "PC1", "name": "notes.txt"})).unwrap()["text"], "first line\nsecond");
    s.execute("editor.write", json!({"device": "PC1", "name": "notes.txt", "text": "v2"})).unwrap();
    // The Command Prompt's dir sees the file; files belong to their device.
    assert!(cli(&mut s, "PC1", "dir").contains("notes.txt"));
    let l = s.execute("editor.list", json!({"device": "PC1"})).unwrap();
    assert_eq!(l["files"], json!([{"name": "notes.txt", "bytes": 2}]));
    assert_eq!((l["max_files"].clone(), l["max_bytes"].clone()), (json!(32), json!(16384)));
    assert_eq!(s.execute("editor.list", json!({"device": "PC2"})).unwrap()["files"], json!([]));
    s.execute("editor.delete", json!({"device": "PC1", "name": "notes.txt"})).unwrap();
    assert!(s.execute("editor.read", json!({"device": "PC1", "name": "notes.txt"})).is_err());
    assert!(s.execute("editor.delete", json!({"device": "PC1", "name": "notes.txt"})).is_err());
}

#[test]
fn text_files_are_bounded_and_only_end_computers_have_them() {
    let mut s = lab("lan");
    let big = "x".repeat(16 * 1024 + 1);
    assert!(s.execute("editor.write", json!({"device": "PC1", "name": "big.txt", "text": big})).is_err());
    let max = "x".repeat(16 * 1024);
    s.execute("editor.write", json!({"device": "PC1", "name": "max.txt", "text": max})).unwrap();
    let long = "n".repeat(65);
    for bad in ["", "two words", "a/b", long.as_str()] {
        assert!(s.execute("editor.write", json!({"device": "PC1", "name": bad, "text": "t"})).is_err(), "{bad:?}");
    }
    assert!(s.execute("editor.write", json!({"device": "PC1", "name": "no-text"})).is_err());
    for i in 1..32 {
        s.execute("editor.write", json!({"device": "PC1", "name": format!("f{i}"), "text": ""})).unwrap();
    }
    // The 33rd new file is refused, but an existing one can still be saved.
    assert!(s.execute("editor.write", json!({"device": "PC1", "name": "one-too-many", "text": ""})).is_err());
    s.execute("editor.write", json!({"device": "PC1", "name": "f1", "text": "again"})).unwrap();
    // A switch has no Desktop.
    assert!(s.execute("editor.write", json!({"device": "S1", "name": "a", "text": "t"})).is_err());
    assert!(s.execute("editor.list", json!({"device": "S1"})).is_err());
    // A printer is an end device with no computer apps.
    assert!(s.execute("editor.list", json!({"device": "Printer1"})).is_err());
}

#[test]
fn the_files_survive_saving_and_opening_the_lab() {
    let mut s = lab("lan");
    s.execute("editor.write", json!({"device": "PC1", "name": "notes.txt", "text": "keep me"})).unwrap();
    s.execute("host.firewall", json!({"device": "PC1", "on": true, "add": {"action": "allow", "protocol": "icmp"}})).unwrap();
    let json_text = s.execute("file.save", json!({})).unwrap()["json"].as_str().unwrap().to_string();
    let mut again = Session::new();
    again.execute("file.open", json!({"json": json_text})).unwrap();
    assert_eq!(again.execute("editor.read", json!({"device": "PC1", "name": "notes.txt"})).unwrap()["text"], "keep me");
    let fw = again.execute("host.firewall", json!({"device": "PC1"})).unwrap();
    assert_eq!(fw["on"], json!(true));
    assert_eq!(fw["rules"].as_array().map(Vec::len), Some(1));
}

#[test]
fn the_firewall_command_builds_the_list_and_the_list_filters_pings() {
    let mut s = lab("lan");
    assert!(ping_ok(&mut s, "PC1", "192.168.1.12"));
    // Reading changes nothing: off, no rules, for both versions.
    let st = s.execute("host.firewall", json!({"device": "PC2"})).unwrap();
    assert_eq!((st["version"].clone(), st["on"].clone(), st["rules"].clone()), (json!(4), json!(false), json!([])));
    // On with no rule: the PC no longer answers pings.
    s.execute("host.firewall", json!({"device": "PC2", "on": true})).unwrap();
    assert!(!ping_ok(&mut s, "PC1", "192.168.1.12"));
    // Rules take a protocol, a remote network and a port or a range, and keep their order.
    let add = |s: &mut Session, rule: Value| s.execute("host.firewall", json!({"device": "PC2", "add": rule}));
    add(&mut s, json!({"action": "allow", "protocol": "tcp", "port": 80})).unwrap();
    add(&mut s, json!({"action": "deny", "protocol": "udp", "remote": "10.0.0.0/8", "port": "1000-2000"})).unwrap();
    let st = add(&mut s, json!({"action": "allow", "protocol": "icmp", "remote": "192.168.1.11", "at": 1})).unwrap();
    assert_eq!(
        st["rules"],
        json!([
            {"n": 1, "action": "allow", "protocol": "icmp", "remote": "192.168.1.11/32", "port": "any"},
            {"n": 2, "action": "allow", "protocol": "tcp", "remote": "any", "port": "80"},
            {"n": 3, "action": "deny", "protocol": "udp", "remote": "10.0.0.0/8", "port": "1000-2000"},
        ])
    );
    // PC1 is the allowed address; PC3 is not.
    assert!(ping_ok(&mut s, "PC1", "192.168.1.12"));
    assert!(!ping_ok(&mut s, "PC3", "192.168.1.12"));
    // The PC's own traffic is not filtered: its pings are answered by PC3, whose firewall is off.
    assert!(ping_ok(&mut s, "PC2", "192.168.1.13"));
    // Remove, clear, switch off.
    let st = s.execute("host.firewall", json!({"device": "PC2", "remove": 1})).unwrap();
    assert_eq!(st["rules"].as_array().map(Vec::len), Some(2));
    assert!(!ping_ok(&mut s, "PC1", "192.168.1.12"));
    let st = s.execute("host.firewall", json!({"device": "PC2", "clear": true, "on": false})).unwrap();
    assert_eq!((st["on"].clone(), st["rules"].clone()), (json!(false), json!([])));
    assert!(ping_ok(&mut s, "PC3", "192.168.1.12"));
}

#[test]
fn the_firewall_lists_for_ipv4_and_ipv6_are_separate_and_rules_are_checked() {
    let mut s = lab("lan");
    s.execute("host.firewall", json!({"device": "PC1", "version": 6, "on": true})).unwrap();
    s.execute(
        "host.firewall",
        json!({"device": "PC1", "version": 6, "add": {"action": "deny", "protocol": "tcp", "remote": "2001:db8::/32", "port": 22}}),
    )
    .unwrap();
    let v4 = s.execute("host.firewall", json!({"device": "PC1"})).unwrap();
    assert_eq!((v4["on"].clone(), v4["rules"].clone()), (json!(false), json!([])));
    let v6 = s.execute("host.firewall", json!({"device": "PC1", "version": "6"})).unwrap();
    assert_eq!((v6["version"].clone(), v6["on"].clone(), v6["rules"].as_array().map(Vec::len)), (json!(6), json!(true), Some(1)));
    // What makes no sense is refused and changes nothing.
    let refused = [
        json!({"add": {"action": "allow", "remote": "10.0.0.0/8"}, "version": 6}),
        json!({"add": {"action": "allow", "remote": "2001:db8::/32"}}),
        json!({"add": {"action": "allow", "remote": "10.0.0.0/33"}}),
        json!({"add": {"action": "allow", "protocol": "icmp", "port": 5}}),
        json!({"add": {"action": "allow", "protocol": "tcp", "port": "90-80"}}),
        json!({"add": {"action": "allow", "protocol": "tcp", "port": "http"}}),
        json!({"add": {"action": "maybe"}}),
        json!({"add": {"action": "allow", "protocol": "gre"}}),
        json!({"add": {"action": "allow", "at": 9}}),
        json!({"remove": 1}),
        json!({"version": 5}),
        json!({"on": "yes"}),
    ];
    for mut p in refused {
        p["device"] = json!("PC1");
        assert!(s.execute("host.firewall", p.clone()).is_err(), "{p}");
    }
    assert_eq!(s.execute("host.firewall", json!({"device": "PC1"})).unwrap()["rules"], json!([]));
    // A list holds up to 32 rules.
    for _ in 0..32 {
        s.execute("host.firewall", json!({"device": "PC1", "add": {"action": "allow"}})).unwrap();
    }
    assert!(s.execute("host.firewall", json!({"device": "PC1", "add": {"action": "allow"}})).is_err());
    // Switches, routers and printers have no firewall app.
    assert!(s.execute("host.firewall", json!({"device": "S1"})).is_err());
    assert!(s.execute("host.firewall", json!({"device": "Printer1"})).is_err());
}

#[test]
fn terminal_monitor_shows_a_routers_debug_output_in_the_remote_session() {
    let mut s = lab("two-subnets");
    for line in ["enable", "configure terminal", "enable secret letmein", "line vty 0 4", "password remote", "login", "end"] {
        cli(&mut s, "R1", line);
    }
    cli(&mut s, "PC1", "telnet 10.0.1.1");
    cli(&mut s, "PC1", "remote");
    cli(&mut s, "PC1", "enable");
    cli(&mut s, "PC1", "letmein");
    cli(&mut s, "PC1", "debug ip icmp");
    let pc1 = s.topology.resolve("PC1").unwrap();
    assert!(ping_ok(&mut s, "PC3", "10.0.1.1"));
    assert!(!s.consoles[&pc1].log.contains("ICMP debug:"), "without `terminal monitor` the session sees no log messages");
    cli(&mut s, "PC1", "terminal monitor");
    assert!(ping_ok(&mut s, "PC3", "10.0.1.1"));
    assert!(s.consoles[&pc1].log.contains("ICMP debug: echo request from 10.0.2.10"), "{}", s.consoles[&pc1].log);
    // The router's own console printed it too.
    let r1 = s.topology.resolve("R1").unwrap();
    assert!(s.consoles[&r1].log.contains("ICMP debug: echo request from 10.0.2.10"));
    // Turned off, the session is quiet again.
    cli(&mut s, "PC1", "terminal no monitor");
    let before = s.consoles[&pc1].log.matches("ICMP debug:").count();
    assert!(ping_ok(&mut s, "PC3", "10.0.1.1"));
    assert_eq!(s.consoles[&pc1].log.matches("ICMP debug:").count(), before);
}
