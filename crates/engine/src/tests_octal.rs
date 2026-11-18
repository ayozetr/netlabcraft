//! Reverse Telnet through an octal card: a console typed on one router opens the console of the
//! device an octal cable reaches from a line of another router's card. The passwords are fakes.

use serde_json::json;

use crate::Session;

/// One line typed at a device's console; no `enable` first (a session may be open).
fn run(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

/// R1 (modular, octal card, line 1 cabled to R2's console) and R3 on one network.
fn network() -> Session {
    let mut s = Session::new();
    for (name, model) in [("R1", "Router-M2"), ("R2", "Router-3"), ("R3", "Router-3")] {
        s.execute("device.add", json!({"model": model, "name": name})).unwrap();
    }
    s.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    s.execute("device.module.add", json!({"device": "R1", "slot": "0/1", "module": "Card-8A"})).unwrap();
    s.execute("device.power", json!({"device": "R1", "on": true})).unwrap();
    s.execute("link.add", json!({"a": "R1", "b": "R2", "cable": "octal", "line": 0})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "R3", "b_port": "g0/0"})).unwrap();
    for (dev, lines) in [
        ("R1", vec!["interface g0/0", "ip address 10.0.0.1 255.255.255.0", "no shutdown", "exit"]),
        ("R3", vec!["interface g0/0", "ip address 10.0.0.2 255.255.255.0", "no shutdown", "exit"]),
    ] {
        let r = s.execute("device.configure", json!({"device": dev, "lines": lines})).unwrap();
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    }
    s
}

fn open_line_one(s: &mut Session) {
    run(s, "R1", "enable");
    for l in ["configure terminal", "line 1", "transport input telnet", "modem inout", "no exec", "exit", "exit"] {
        let out = run(s, "R1", l);
        assert!(!out.contains('%'), "{l}: {out}");
    }
}

#[test]
fn a_router_console_reaches_the_device_behind_an_octal_line() {
    let mut s = network();
    // A line takes no connection until its transport is set.
    let out = run(&mut s, "R3", "telnet 10.0.0.1 2001");
    assert!(out.contains("refused") && out.contains("transport input telnet"), "{out}");
    open_line_one(&mut s);
    let table = run(&mut s, "R1", "show line");
    assert!(table.contains("Cabled to") && table.contains("idle") && table.contains("telnet"), "{table}");
    let out = run(&mut s, "R3", "telnet 10.0.0.1 2001");
    assert!(out.contains("Open"), "{out}");
    // What is typed now runs on R2's console.
    let out = run(&mut s, "R3", "show users");
    assert!(out.contains("con 0"), "{out}");
    let users = run(&mut s, "R1", "show users");
    assert!(users.contains("reverse Telnet from R3") && users.contains("console of R2"), "{users}");
    assert!(run(&mut s, "R1", "show line").contains("in use"));
    // One session per line, also from the router's own console.
    let busy = run(&mut s, "R1", "telnet 10.0.0.1 2001");
    assert!(busy.contains("refused"), "{busy}");
    // The escape sequence closes it and frees the line.
    let out = run(&mut s, "R3", "^^x");
    assert!(out.contains("closed"), "{out}");
    assert!(run(&mut s, "R1", "show line").contains("idle"));
    // A cleared line ends the session at its next input.
    assert!(run(&mut s, "R3", "telnet 10.0.0.1 2001").contains("Open"));
    assert!(run(&mut s, "R1", "clear line 1").contains("cleared"));
    let out = run(&mut s, "R3", "show users");
    assert!(out.contains("clear line"), "{out}");
    assert!(run(&mut s, "R3", "telnet 10.0.0.1 2001").contains("Open"));
    assert!(run(&mut s, "R3", "exit").contains("closed"));
}

#[test]
fn a_reverse_telnet_asks_for_the_console_password_and_the_line_must_be_cabled() {
    let mut s = network();
    open_line_one(&mut s);
    run(&mut s, "R2", "enable");
    for l in ["configure terminal", "line console 0", "password pw-test", "login", "exit", "exit"] {
        run(&mut s, "R2", l);
    }
    run(&mut s, "R1", "configure terminal");
    run(&mut s, "R1", "line 2");
    run(&mut s, "R1", "transport input telnet");
    run(&mut s, "R1", "end");
    // Line 2 takes Telnet but has no cable.
    assert!(run(&mut s, "R3", "telnet 10.0.0.1 2002").contains("no line"));
    let out = run(&mut s, "R3", "telnet 10.0.0.1 2001");
    assert!(out.contains("Console access is password protected"), "{out}");
    assert!(run(&mut s, "R3", "wrong").contains("invalid"));
    run(&mut s, "R3", "pw-test");
    assert!(run(&mut s, "R3", "show users").contains("con 0"));
    // The router's own address reaches its lines too (a loopback is just an address).
    run(&mut s, "R3", "^^x");
    let out = run(&mut s, "R1", "telnet 10.0.0.1 2001");
    assert!(out.contains("Open"), "{out}");
}

#[test]
fn line_commands_check_the_card_and_the_range() {
    let mut s = network();
    run(&mut s, "R1", "enable");
    run(&mut s, "R1", "configure terminal");
    assert!(run(&mut s, "R1", "line 9").contains('%'));
    assert!(run(&mut s, "R1", "line 3 2").contains('%'));
    assert!(!run(&mut s, "R1", "line 1 8").contains('%'));
    run(&mut s, "R1", "transport input telnet");
    run(&mut s, "R1", "end");
    let cfg = run(&mut s, "R1", "show running-config");
    assert!(cfg.contains("line 1\n transport input telnet") && cfg.contains("line 8\n transport input telnet"), "{cfg}");
    // A router with no octal card has no lines.
    run(&mut s, "R3", "enable");
    run(&mut s, "R3", "configure terminal");
    assert!(run(&mut s, "R3", "line 1").contains("no octal card"));
    run(&mut s, "R3", "end");
    assert!(run(&mut s, "R3", "clear line 1").contains("no octal card"));
}

#[test]
fn an_octal_cable_longer_than_its_standard_refuses_the_reverse_telnet() {
    let mut s = network();
    open_line_one(&mut s);
    let b = s.execute("place.add", json!({"kind": "building", "name": "Workshop", "x": 500, "y": 220})).unwrap()["id"].as_u64().unwrap();
    let far = s.execute("place.add", json!({"kind": "closet", "parent": b})).unwrap()["id"].as_u64().unwrap();
    s.execute("device.place", json!({"device": "R2", "place": far})).unwrap();
    let out = run(&mut s, "R3", "telnet 10.0.0.1 2001");
    assert!(out.contains("cable too long") && out.contains("octal"), "{out}");
}
