//! Engine-level tests of the debug topics that need services: SNMP, the web server and PPPoE.

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

#[test]
fn debug_snmp_tells_a_good_community_from_a_wrong_one() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router", "x": 0, "y": 0})).unwrap();
    s.execute("device.add", json!({"kind": "pc", "x": 0, "y": 100})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "PC1", "b_port": "fa0"})).unwrap();
    configure(&mut s, "R1", &["interface g0/0", "ip address 10.0.0.1 255.255.255.0", "no shutdown", "exit", "snmp-server community lectura ro"]);
    s.execute("cli.exec", json!({"device": "PC1", "line": "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1"})).unwrap();
    cli(&mut s, "R1", "debug snmp packets");
    let get = |s: &mut Session, community: &str| {
        s.execute(
            "snmp.request",
            json!({"device": "PC1", "target": "10.0.0.1", "community": community, "operation": "get", "oid": "1.3.6.1.2.1.1.5.0"}),
        )
    };
    get(&mut s, "lectura").unwrap();
    assert!(get(&mut s, "adivina").is_err());
    let log = cli(&mut s, "R1", "show logging");
    assert!(log.contains("SNMP debug: request from 10.0.0.10 accepted (read-only access)"), "{log}");
    assert!(log.contains("SNMP debug: request from 10.0.0.10 with an unknown community: no answer is sent"), "{log}");
}

#[test]
fn debug_ip_http_reports_each_request_and_its_status() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "automation"})).unwrap();
    cli(&mut s, "R1", "debug ip http");
    let p = json!({
        "pc": "PC1", "method": "GET", "url": "https://192.168.1.1/restconf/data/ietf-interfaces:interfaces",
        "user": "admin", "password": "NetLab2026"
    });
    s.execute("pc.http_request", p).unwrap();
    let log = cli(&mut s, "R1", "show logging");
    assert!(log.contains("HTTP debug: GET /restconf/data/ietf-interfaces:interfaces answered with status 200"), "{log}");
}

#[test]
fn debug_pppoe_and_ppp_follow_a_dial_in() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router"})).unwrap();
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "PC1"})).unwrap();
    configure(
        &mut s,
        "R1",
        &[
            "username ann password pw-test",
            "ip local pool POOL 10.1.1.2 10.1.1.9",
            "bba-group pppoe BBA",
            "virtual-template 1",
            "exit",
            "interface virtual-template 1",
            "ip address 10.1.1.1 255.255.255.255",
            "peer default ip address pool POOL",
            "ppp authentication chap",
            "exit",
            "interface g0/0",
            "no shutdown",
            "pppoe enable group BBA",
            "exit",
        ],
    );
    for line in ["debug pppoe events", "debug pppoe packets", "debug ppp authentication", "debug ppp negotiation"] {
        cli(&mut s, "R1", line);
    }
    s.execute("pppoe.connect", json!({"device": "PC1", "username": "ann", "password": "pw-test"})).unwrap();
    let log = cli(&mut s, "R1", "show logging");
    assert!(log.contains("PPPoE packet debug: PADI (who offers service?) from "), "{log}");
    assert!(log.contains("PPPoE event debug: a client asks for a session"), "{log}");
    assert!(log.contains("PPP authentication debug: CHAP: CHAP response (id "), "{log}");
    assert!(log.contains("PPP negotiation debug: LCP: "), "{log}");
}
