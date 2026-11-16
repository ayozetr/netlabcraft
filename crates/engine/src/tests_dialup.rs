//! Engine tests for Dial-up: a Dial-up PC calls a number in a telephone cloud and an access
//! server's modem line answers with PPP. The passwords are obvious fakes.

use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": "enable"})).unwrap();
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn configure(s: &mut Session, dev: &str, lines: &[&str]) -> Value {
    let r = s.execute("device.configure", json!({"device": dev, "lines": lines})).unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    r
}

/// PC1 and PC2 (modems) and the access server R1 on one telephone cloud; R1's line is 5550100.
fn network() -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Telephone Cloud"})).unwrap();
    s.execute("device.add", json!({"model": "Dial-up PC"})).unwrap();
    s.execute("device.add", json!({"model": "Dial-up PC"})).unwrap();
    s.execute("device.add", json!({"model": "Access Server"})).unwrap();
    s.execute("link.add", json!({"a": "PC1", "a_port": "ph0", "b": "PSTN1", "b_port": "ph1"})).unwrap();
    s.execute("link.add", json!({"a": "PC2", "a_port": "ph0", "b": "PSTN1", "b_port": "ph2"})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "ph0", "b": "PSTN1", "b_port": "ph0"})).unwrap();
    s.execute("cloud.number", json!({"device": "PSTN1", "port": "ph0", "number": "555-0100"})).unwrap();
    s.execute("cloud.number", json!({"device": "PSTN1", "port": "ph1", "number": "5550101"})).unwrap();
    s.execute("cloud.number", json!({"device": "PSTN1", "port": "ph2", "number": "5550102"})).unwrap();
    configure(
        &mut s,
        "R1",
        &[
            "username ann password pw-test",
            "ip local pool CALLERS 10.9.0.2 10.9.0.9",
            "interface loopback 0",
            "ip address 10.9.0.1 255.255.255.255",
            "exit",
            "bba-group pppoe LINES",
            "virtual-template 1",
            "exit",
            "interface virtual-template 1",
            "ip unnumbered loopback 0",
            "peer default ip address pool CALLERS",
            "ppp authentication chap",
            "exit",
            "interface ph0",
            "no shutdown",
            "modem inout group LINES",
            "exit",
        ],
    );
    s
}

#[test]
fn a_dial_up_pc_calls_the_access_server_and_hangs_up() {
    let mut s = network();
    let st = s.execute("dialup.connect", json!({"device": "PC1", "number": "(555) 0100", "username": "ann", "password": "pw-test"})).unwrap();
    assert_eq!(st["state"], json!("connected"), "{st}");
    assert_eq!((st["carrier"].clone(), st["number"].clone(), st["server"].clone()), (json!(true), json!("5550100"), json!("R1")));
    let pairs = s.execute("cloud.number", json!({"device": "PSTN1"})).unwrap();
    assert_eq!(pairs["calls"].as_array().map(Vec::len), Some(1), "{pairs}");
    let r = s.execute("net.ping", json!({"from": "PC1", "to": "10.9.0.1", "count": 2})).unwrap();
    assert!(r["marks"].as_str().unwrap_or_default().contains('!'), "{r}");
    assert!(cli(&mut s, "R1", "show pppoe session").contains("Virtual-Access1"));
    assert!(cli(&mut s, "R1", "show running-config").contains("modem inout group LINES"));
    // The line is busy for the second caller until the first hangs up.
    let busy = s.execute("dialup.connect", json!({"device": "PC2", "number": "5550100", "username": "ann", "password": "pw-test"}));
    assert!(busy.unwrap_err().to_string().contains("busy"));
    s.execute("dialup.disconnect", json!({"device": "PC1"})).unwrap();
    let after = s.execute("cloud.number", json!({"device": "PSTN1"})).unwrap();
    assert_eq!(after["calls"].as_array().map(Vec::len), Some(0), "{after}");
    let st = s.execute("dialup.connect", json!({"device": "PC2", "number": "5550100", "username": "ann", "password": "pw-test"})).unwrap();
    assert_eq!(st["state"], json!("connected"), "{st}");
}

#[test]
fn a_call_that_cannot_complete_leaves_no_circuit() {
    let mut s = network();
    let e = |s: &mut Session, number: &str, pw: &str| {
        s.execute("dialup.connect", json!({"device": "PC1", "number": number, "username": "ann", "password": pw})).unwrap_err().to_string()
    };
    assert!(e(&mut s, "5559999", "pw-test").contains("not a number"));
    assert!(e(&mut s, "5550101", "pw-test").contains("own number"));
    assert!(e(&mut s, "5550102", "pw-test").contains("no answer"), "a PC does not answer");
    assert!(e(&mut s, "5550100", "wrong").contains("PPP failed"));
    assert!(e(&mut s, "abc", "pw-test").contains("number"));
    let cloud = s.execute("cloud.number", json!({"device": "PSTN1"})).unwrap();
    assert_eq!(cloud["calls"].as_array().map(Vec::len), Some(0), "{cloud}");
    // A line that does not take calls does not answer.
    configure(&mut s, "R1", &["interface ph0", "no modem inout", "exit"]);
    assert!(e(&mut s, "5550100", "pw-test").contains("no answer"));
    assert!(s.execute("dialup.disconnect", json!({"device": "PC1"})).is_err());
}

#[test]
fn numbers_are_per_port_and_other_devices_do_not_dial() {
    let mut s = network();
    assert!(s.execute("cloud.number", json!({"device": "PSTN1", "port": "ph3", "number": "5550100"})).is_err());
    assert!(s.execute("cloud.number", json!({"device": "PSTN1", "port": "ph3"})).is_err());
    s.execute("cloud.number", json!({"device": "PSTN1", "port": "ph1", "number": null})).unwrap();
    assert!(s.execute("cloud.number", json!({"device": "R1"})).is_err());
    assert!(s.execute("dialup.connect", json!({"device": "R1", "number": "5550100"})).is_err());
    let out = s.execute("device.configure", json!({"device": "R1", "lines": ["interface fa0/0", "modem inout"]})).unwrap();
    assert_eq!(out["errors"].as_array().map(Vec::len), Some(1), "modem inout is for modem lines: {out}");
}
