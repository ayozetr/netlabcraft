//! Engine tests for PPPoE on routers: a router as the access concentrator (a computer or a
//! router dials it) and a router as the client (it dials a concentrator router or a server).
//! The passwords in these tests are obvious fakes.

use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn configure(s: &mut Session, dev: &str, lines: &[&str]) -> Value {
    let r = s.execute("device.configure", json!({"device": dev, "lines": lines})).unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    r
}

/// R1 as a concentrator on g0/0: CHAP, accounts `ann`, the pool 10.1.1.2 to 10.1.1.9.
fn concentrator(s: &mut Session, auth: &str, unnumbered: bool) {
    let address = if unnumbered { "ip unnumbered loopback 0" } else { "ip address 10.1.1.1 255.255.255.255" };
    configure(
        s,
        "R1",
        &[
            "username ann password pw-test",
            "ip local pool POOL 10.1.1.2 10.1.1.9",
            "interface loopback 0",
            "ip address 10.255.0.1 255.255.255.255",
            "exit",
            "bba-group pppoe BBA",
            "virtual-template 1",
            "exit",
            "interface virtual-template 1",
            address,
            "peer default ip address pool POOL",
            &format!("ppp authentication {auth}"),
            "exit",
            "interface g0/0",
            "no shutdown",
            "pppoe enable group BBA",
            "exit",
        ],
    );
}

/// R2 as a client on g0/0, dialing pool 1 with the account `ann`.
fn client(s: &mut Session) -> Value {
    configure(
        s,
        "R2",
        &[
            "interface dialer 1",
            "encapsulation ppp",
            "ip address negotiated",
            "ppp chap hostname ann",
            "ppp chap password pw-test",
            "ppp pap sent-username ann password pw-test",
            "dialer pool 1",
            "exit",
            "ip route 0.0.0.0 0.0.0.0 dialer 1",
            "interface g0/0",
            "no shutdown",
            "pppoe-client dial-pool-number 1",
            "exit",
        ],
    )
}

fn two_routers() -> Session {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router"})).unwrap();
    s.execute("device.add", json!({"kind": "router"})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"})).unwrap();
    s
}

#[test]
fn a_computer_dials_a_router_concentrator_and_reaches_it_through_a_host_route() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router"})).unwrap();
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    s.execute("link.add", json!({"a": "R1", "a_port": "g0/0", "b": "PC1"})).unwrap();
    concentrator(&mut s, "chap", false);
    let st = s.execute("pppoe.connect", json!({"device": "PC1", "username": "ann", "password": "pw-test"})).unwrap();
    assert_eq!(
        (st["state"].clone(), st["address"].clone(), st["gateway"].clone(), st["server"].clone()),
        (json!("connected"), json!("10.1.1.2"), json!("10.1.1.1"), json!("R1"))
    );
    let show = cli(&mut s, "R1", "show pppoe session");
    assert!(show.contains("10.1.1.2") && show.contains("Virtual-Access1"), "{show}");
    assert!(cli(&mut s, "R1", "show ip route").contains("10.1.1.2/32"));
    assert!(cli(&mut s, "R1", "ping 10.1.1.2").contains('!'));
    let r = s.execute("net.ping", json!({"from": "PC1", "to": "10.1.1.1", "count": 2})).unwrap();
    assert!(r["marks"].as_str().unwrap_or_default().contains('!'), "{r}");
    // Ending the session removes the host route, and a wrong password gets no session.
    assert!(s.execute("pppoe.connect", json!({"device": "PC1", "username": "ann", "password": "pw-test"})).is_err());
    s.execute("pppoe.disconnect", json!({"device": "PC1"})).unwrap();
    assert!(!cli(&mut s, "R1", "show ip route").contains("10.1.1.2/32"));
    assert!(s.execute("pppoe.connect", json!({"device": "PC1", "username": "ann", "password": "wrong"})).is_err());
}

#[test]
fn a_router_client_dials_a_router_concentrator_with_chap_and_pap() {
    for auth in ["chap", "pap"] {
        let mut s = two_routers();
        concentrator(&mut s, auth, true);
        let r = client(&mut s);
        let show = cli(&mut s, "R2", "show pppoe session");
        assert!(show.contains("Dialer1") && show.contains("10.255.0.1"), "{auth}: {show} {r}");
        assert!(cli(&mut s, "R2", "show ip interface brief").contains("Dialer1"));
        assert!(cli(&mut s, "R2", "ping 10.255.0.1").contains('!'), "{auth}");
        assert!(cli(&mut s, "R1", "ping 10.1.1.2").contains('!'), "{auth}");
        let run = cli(&mut s, "R2", "show running-config");
        assert!(
            run.contains("interface Dialer1") && run.contains("ip address negotiated") && run.contains("pppoe-client dial-pool-number 1"),
            "{run}"
        );
        // Clearing the sessions ends them on both sides; the client dials again when its port is set up again.
        let out = cli(&mut s, "R1", "clear pppoe all");
        assert!(out.contains("ended"), "{out}");
        assert!(cli(&mut s, "R2", "show pppoe session").contains("dialed: 0"));
        configure(&mut s, "R2", &["interface g0/0", "pppoe-client dial-pool-number 1", "exit"]);
        assert!(cli(&mut s, "R2", "ping 10.255.0.1").contains('!'), "{auth}: dialed again");
    }
}

#[test]
fn a_wrong_secret_gives_no_session_and_a_removed_group_ends_one() {
    let mut s = two_routers();
    concentrator(&mut s, "chap", true);
    configure(
        &mut s,
        "R2",
        &[
            "interface dialer 1",
            "encapsulation ppp",
            "ip address negotiated",
            "ppp chap hostname ann",
            "ppp chap password wrong",
            "dialer pool 1",
            "exit",
            "interface g0/0",
            "no shutdown",
            "pppoe-client dial-pool-number 1",
            "exit",
        ],
    );
    assert!(cli(&mut s, "R1", "show pppoe summary").contains("0 sessions"));
    configure(&mut s, "R2", &["interface dialer 1", "ppp chap password pw-test", "exit"]);
    assert!(cli(&mut s, "R1", "show pppoe summary").contains("1 session open"));
    configure(&mut s, "R1", &["interface g0/0", "no pppoe enable", "exit"]);
    assert!(cli(&mut s, "R1", "show pppoe summary").contains("0 sessions"));
    assert!(cli(&mut s, "R2", "show pppoe session").contains("dialed: 0"));
}

#[test]
fn a_router_client_dials_a_server_and_the_configuration_round_trips() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "server"})).unwrap();
    s.execute("device.add", json!({"kind": "router"})).unwrap();
    s.execute("link.add", json!({"a": "SRV1", "b": "R1", "b_port": "g0/0"})).unwrap();
    cli(&mut s, "SRV1", "ipconfig 10.9.0.1 255.255.255.0");
    s.execute(
        "server.services",
        json!({"device": "SRV1", "services": {"pppoe": {"on": true, "auth": "chap", "pool_start": "10.9.0.100", "pool_size": 3, "users": [{"name": "ann", "password": "pw-test"}]}}}),
    )
    .unwrap();
    // The client is R1 here: reuse the client setup on it.
    configure(
        &mut s,
        "R1",
        &[
            "interface dialer 1",
            "encapsulation ppp",
            "ip address negotiated",
            "ppp chap hostname ann",
            "ppp chap password pw-test",
            "dialer pool 1",
            "exit",
            "interface g0/0",
            "no shutdown",
            "pppoe-client dial-pool-number 1",
            "exit",
        ],
    );
    assert!(cli(&mut s, "R1", "ping 10.9.0.1").contains('!'));
    let run = cli(&mut s, "R1", "show running-config");
    s.execute("device.add", json!({"kind": "router"})).unwrap();
    let lines: Vec<&str> = run.lines().filter(|l| !l.starts_with("hostname")).collect();
    let r = s.execute("device.configure", json!({"device": "R2", "lines": lines})).unwrap();
    let again = cli(&mut s, "R2", "show running-config");
    for want in ["interface Dialer1", "encapsulation ppp", "dialer pool 1", "ppp chap hostname ann", "pppoe-client dial-pool-number 1"] {
        assert!(again.contains(want), "{want}: {r}\n{again}");
    }
}
