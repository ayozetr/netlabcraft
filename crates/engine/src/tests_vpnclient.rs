//! Engine tests for the VPN Client: a PC against a router set up as a remote-access server.
//! The key and the password in these tests are obvious fakes.

use serde_json::{Value, json};

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn configure(s: &mut Session, dev: &str, lines: &[&str]) {
    let r = s.execute("device.configure", json!({"device": dev, "lines": lines})).unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
}

/// The router's remote-access server, in blocks; `skip` names blocks to leave out (or
/// `plain-map`: a crypto map entry that does not use the dynamic map) to build a broken one.
fn server(s: &mut Session, skip: &[&str]) {
    let blocks: [(&str, Vec<&str>); 11] = [
        ("user", vec!["username ann password pw-test"]),
        ("aaa-login", vec!["aaa authentication login vpn-auth local"]),
        ("aaa-authz", vec!["aaa authorization network vpn-auth local"]),
        ("pool", vec!["ip local pool VPNPOOL 10.9.9.1 10.9.9.5"]),
        ("group", vec!["crypto isakmp client configuration group team", "key key-test", "pool VPNPOOL", "exit"]),
        ("ts", vec!["crypto ipsec transform-set TS esp-aes 256 esp-sha-hmac", "exit"]),
        ("dyn", vec!["crypto dynamic-map DM 10", "set transform-set TS", "reverse-route", "exit"]),
        ("dyn-no-ts", vec!["crypto dynamic-map DM 10", "reverse-route", "exit"]),
        ("auth-list", vec!["crypto map CM client authentication list vpn-auth"]),
        ("authz-list", vec!["crypto map CM isakmp authorization list vpn-auth", "crypto map CM client configuration address respond"]),
        ("map", vec!["crypto map CM 10 ipsec-isakmp dynamic DM"]),
    ];
    let mut lines: Vec<&str> = Vec::new();
    for (name, body) in &blocks {
        let wanted = match *name {
            "dyn" => !skip.contains(&"dyn") && !skip.contains(&"dyn-no-ts"),
            "dyn-no-ts" => skip.contains(&"dyn-no-ts"),
            "map" => !skip.contains(&"map") && !skip.contains(&"plain-map"),
            other => !skip.contains(&other),
        };
        if wanted {
            lines.extend(body.iter().copied());
        }
    }
    if skip.contains(&"plain-map") {
        lines.extend(["crypto map CM 10 ipsec-isakmp", "exit"]);
    }
    if !skip.contains(&"map") && !skip.contains(&"apply") {
        lines.extend(["interface g0/0", "crypto map CM", "exit"]);
    }
    configure(s, "R1", &lines);
}

/// PC1 (203.0.113.2) cabled to R1's outside port and PC2 (10.0.0.10) to its inside port.
fn lab(skip: &[&str]) -> Session {
    let mut s = Session::new();
    for kind in ["pc", "router", "pc"] {
        s.execute("device.add", json!({"kind": kind})).unwrap();
    }
    s.execute("link.add", json!({"a": "PC1", "b": "R1", "b_port": "g0/0"})).unwrap();
    s.execute("link.add", json!({"a": "PC2", "b": "R1", "b_port": "g0/1"})).unwrap();
    cli(&mut s, "PC1", "ipconfig 203.0.113.2 255.255.255.0 203.0.113.1");
    cli(&mut s, "PC2", "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1");
    configure(
        &mut s,
        "R1",
        &[
            "interface g0/0",
            "ip address 203.0.113.1 255.255.255.0",
            "no shutdown",
            "exit",
            "interface g0/1",
            "ip address 10.0.0.1 255.255.255.0",
            "no shutdown",
            "exit",
        ],
    );
    server(&mut s, skip);
    s
}

fn connect(s: &mut Session, group: &str, key: &str, user: &str, password: &str) -> crate::Result<Value> {
    s.execute(
        "vpnclient.connect",
        json!({"device": "PC1", "host": "203.0.113.1", "group": group, "key": key, "username": user, "password": password}),
    )
}

fn status(s: &mut Session) -> Value {
    s.execute("vpnclient.status", json!({"device": "PC1"})).unwrap()
}

fn ping_ok(s: &mut Session, to: &str) -> bool {
    let r = s.execute("net.ping", json!({"from": "PC1", "to": to, "count": 3})).unwrap();
    r["marks"].as_str().unwrap_or_default().contains('!')
}

fn router_sessions(s: &Session) -> usize {
    let r = s.topology.resolve("R1").unwrap();
    s.sim.ra_sessions(r).len()
}

#[test]
fn a_client_connects_gets_a_pool_address_and_reaches_the_inside_through_the_tunnel() {
    let mut s = lab(&[]);
    assert_eq!(status(&mut s)["state"], json!("disconnected"));
    let st = connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    assert_eq!(st["state"], json!("connected"));
    assert_eq!(st["address"], json!("10.9.9.1"));
    assert_eq!(st["server"], json!("203.0.113.1"));
    assert!(st["esp"].as_str().unwrap_or_default().contains("AES-CBC-256"), "{st}");
    assert!(!st.to_string().contains("key-test") && !st.to_string().contains("pw-test"), "secrets are never returned");
    assert_eq!(router_sessions(&s), 1);
    // The inside host answers, and the traffic went through the tunnel both ways.
    assert!(ping_ok(&mut s, "10.0.0.10"));
    assert!(ping_ok(&mut s, "10.0.0.1"));
    let st = status(&mut s);
    assert!(st["encapsulated"].as_u64().unwrap_or(0) >= 3 && st["decapsulated"].as_u64().unwrap_or(0) >= 3, "{st}");
    // The router reports the client.
    cli(&mut s, "R1", "enable");
    let isakmp = cli(&mut s, "R1", "show crypto isakmp sa");
    assert!(isakmp.contains("remote-access client") && isakmp.contains("10.9.9.1") && isakmp.contains("203.0.113.2"), "{isakmp}");
    let ipsec = cli(&mut s, "R1", "show crypto ipsec sa");
    assert!(ipsec.contains("Remote-access client 203.0.113.2") && ipsec.contains("AES-CBC-256"), "{ipsec}");
    assert!(cli(&mut s, "R1", "show ip local pool").contains("10.9.9.1 (203.0.113.2)"));
    // A second connect is refused until a disconnect.
    assert!(connect(&mut s, "team", "key-test", "ann", "pw-test").is_err());
    let st = s.execute("vpnclient.disconnect", json!({"device": "PC1"})).unwrap();
    assert_eq!(st["state"], json!("disconnected"));
    assert_eq!(router_sessions(&s), 0, "the router was told");
    assert!(s.execute("vpnclient.disconnect", json!({"device": "PC1"})).is_err());
    // Connecting again gives the same address back.
    assert_eq!(connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap()["address"], json!("10.9.9.1"));
}

#[test]
fn the_form_is_kept_and_the_connection_is_not_part_of_the_lab() {
    let mut s = lab(&[]);
    connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    // The next connect needs nothing typed.
    s.execute("vpnclient.disconnect", json!({"device": "PC1"})).unwrap();
    let st = s.execute("vpnclient.connect", json!({"device": "PC1"})).unwrap();
    assert_eq!(st["state"], json!("connected"));
    let saved = &s.topology.device(s.topology.resolve("PC1").unwrap()).unwrap().vpn_client;
    assert_eq!((saved.host.as_str(), saved.group.as_str(), saved.username.as_str()), ("203.0.113.1", "team", "ann"));
    let json = netlabcraft_model::file::to_json(&s.topology).unwrap();
    assert!(json.contains("vpn_client"));
    // A lab opened from the file starts disconnected.
    let mut t = Session::new();
    t.execute("file.open", json!({"json": json})).unwrap();
    assert_eq!(status(&mut t)["state"], json!("disconnected"));
    assert_eq!(status(&mut t)["group"], json!("team"));
}

#[test]
fn a_wrong_group_key_user_or_password_gets_no_tunnel() {
    let mut s = lab(&[]);
    for (group, key, user, password) in [
        ("team", "wrong-key", "ann", "pw-test"),
        ("nobody", "key-test", "ann", "pw-test"),
        ("team", "key-test", "ann", "wrong"),
        ("team", "key-test", "bob", "pw-test"),
        ("team", "key-test", "", ""),
    ] {
        let e = connect(&mut s, group, key, user, password).unwrap_err().to_string();
        assert!(e.contains("refused") || e.contains("proof"), "{group}/{user}: {e}");
        assert_eq!(status(&mut s)["state"], json!("disconnected"));
        assert_eq!(router_sessions(&s), 0);
    }
    assert!(connect(&mut s, "team", "key-test", "ann", "pw-test").is_ok());
}

#[test]
fn a_server_missing_a_piece_refuses_and_says_which() {
    for (skip, why) in [
        (vec!["pool"], "no address pool"),
        (vec!["authz-list"], "isakmp authorization list"),
        (vec!["aaa-login"], "is not defined"),
        (vec!["aaa-authz"], "is not defined"),
        (vec!["plain-map"], "dynamic"),
        (vec!["dyn-no-ts"], "no transform set"),
        (vec!["apply"], "no crypto map is applied"),
    ] {
        let mut s = lab(&skip);
        let e = connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap_err().to_string();
        assert!(e.contains(why), "{skip:?}: {e}");
        assert_eq!(status(&mut s)["state"], json!("disconnected"));
    }
    // Nothing at that address answers at all.
    let mut s = lab(&[]);
    let e = s.execute("vpnclient.connect", json!({"device": "PC1", "host": "203.0.113.77", "group": "team", "key": "key-test"})).unwrap_err();
    assert!(e.to_string().contains("did not answer"), "{e}");
}

#[test]
fn the_pool_runs_out() {
    let mut s = lab(&[]);
    configure(&mut s, "R1", &["ip local pool VPNPOOL 10.9.9.1 10.9.9.1"]);
    // A second computer reaches the router on another outside port, with the map applied there too.
    s.execute("device.add", json!({"kind": "pc"})).unwrap();
    s.execute("link.add", json!({"a": "PC3", "b": "R1", "b_port": "g0/2"})).unwrap();
    cli(&mut s, "PC3", "ipconfig 198.51.100.2 255.255.255.0 198.51.100.1");
    configure(&mut s, "R1", &["interface g0/2", "ip address 198.51.100.1 255.255.255.0", "no shutdown", "crypto map CM", "exit"]);
    assert!(connect(&mut s, "team", "key-test", "ann", "pw-test").is_ok());
    let second = s
        .execute(
            "vpnclient.connect",
            json!({"device": "PC3", "host": "198.51.100.1", "group": "team", "key": "key-test", "username": "ann", "password": "pw-test"}),
        )
        .unwrap_err()
        .to_string();
    assert!(second.contains("no free address"), "{second}");
    // Once the first lets go, the second gets the address.
    s.execute("vpnclient.disconnect", json!({"device": "PC1"})).unwrap();
    let third = s
        .execute(
            "vpnclient.connect",
            json!({"device": "PC3", "host": "198.51.100.1", "group": "team", "key": "key-test", "username": "ann", "password": "pw-test"}),
        )
        .unwrap();
    assert_eq!(third["address"], json!("10.9.9.1"));
}

#[test]
fn the_running_configuration_shows_the_server_and_hostile_input_is_refused() {
    let mut s = lab(&[]);
    cli(&mut s, "R1", "enable");
    let run = cli(&mut s, "R1", "show running-config");
    for line in [
        "aaa authentication login vpn-auth local",
        "aaa authorization network vpn-auth local",
        "ip local pool VPNPOOL 10.9.9.1 10.9.9.5",
        "crypto isakmp client configuration group team",
        " key key-test",
        " pool VPNPOOL",
        "crypto dynamic-map DM 10",
        " reverse-route",
        "crypto map CM client authentication list vpn-auth",
        "crypto map CM 10 ipsec-isakmp dynamic DM",
    ] {
        assert!(run.contains(line), "missing `{line}` in\n{run}");
    }
    assert!(cli(&mut s, "R1", "show crypto map").contains("dynamic map DM"));
    // Not an address, no group, no key, a switch.
    for p in [
        json!({"device": "PC1", "host": "not-an-ip", "group": "team", "key": "key-test"}),
        json!({"device": "PC1", "host": "203.0.113.1", "group": "", "key": "key-test"}),
        json!({"device": "PC1", "host": "203.0.113.1", "group": "team", "key": ""}),
        json!({"device": "PC1", "host": 5}),
        json!({"device": "R1", "host": "203.0.113.1", "group": "team", "key": "key-test"}),
    ] {
        assert!(s.execute("vpnclient.connect", p).is_err());
    }
    assert!(s.execute("vpnclient.status", json!({"device": "R1"})).is_err());
    // A group, a pool and a map cannot be too big or share a name with a dynamic map.
    let r = s
        .execute("device.configure", json!({"device": "R1", "lines": ["ip local pool BIG 10.0.0.0 10.255.255.255", "crypto map DM 20 ipsec-isakmp"]}))
        .unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(2), "{r}");
}

/// Group lines added to the server of `lab`: `lines` are typed inside the group.
fn group_lines(s: &mut Session, lines: &[&str]) {
    let mut all = vec!["crypto isakmp client configuration group team"];
    all.extend(lines.iter().copied());
    all.push("exit");
    configure(s, "R1", &all);
}

fn encapsulated(s: &mut Session) -> u64 {
    status(s)["encapsulated"].as_u64().unwrap_or(0)
}

#[test]
fn a_split_tunnel_sends_only_the_listed_networks_through_the_tunnel() {
    let mut s = lab(&[]);
    configure(&mut s, "R1", &["access-list 10 permit 192.168.77.0 0.0.0.255"]);
    group_lines(&mut s, &["acl 10"]);
    let st = connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    assert_eq!(st["split_tunnel"], json!(["192.168.77.0 0.0.0.255"]), "{st}");
    // 10.0.0.10 is not in the list: the PC uses its normal route and nothing is sealed.
    let _ = ping_ok(&mut s, "10.0.0.10");
    assert_eq!(encapsulated(&mut s), 0);
    let ipconfig = cli(&mut s, "PC1", "ipconfig");
    assert!(ipconfig.contains("VPN address 10.9.9.1") && ipconfig.contains("192.168.77.0 0.0.0.255 (split tunnel)"), "{ipconfig}");
    // The list now names the inside network too: the same ping is sealed.
    s.execute("vpnclient.disconnect", json!({"device": "PC1"})).unwrap();
    configure(&mut s, "R1", &["access-list 10 permit 10.0.0.0 0.0.0.255"]);
    let st = connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    assert_eq!(st["split_tunnel"].as_array().map(Vec::len), Some(2), "{st}");
    assert!(ping_ok(&mut s, "10.0.0.10"));
    assert!(encapsulated(&mut s) >= 3);
    // The console writes the line back; `no acl` makes it a full tunnel again.
    cli(&mut s, "R1", "enable");
    assert!(cli(&mut s, "R1", "show running-config").contains(" acl 10"));
    s.execute("vpnclient.disconnect", json!({"device": "PC1"})).unwrap();
    group_lines(&mut s, &["no acl"]);
    assert!(!cli(&mut s, "R1", "show running-config").contains(" acl 10"));
    let st = connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    assert!(st["split_tunnel"].is_null(), "{st}");
    let ipconfig = cli(&mut s, "PC1", "ipconfig");
    assert!(ipconfig.contains("full tunnel"), "{ipconfig}");
}

#[test]
fn the_dns_server_and_domain_of_the_group_reach_the_client() {
    let mut s = lab(&[]);
    group_lines(&mut s, &["dns 10.0.0.53", "domain lab.test"]);
    let st = connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    assert_eq!(st["dns"], json!("10.0.0.53"));
    assert_eq!(st["domain"], json!("lab.test"));
    let ipconfig = cli(&mut s, "PC1", "ipconfig");
    assert!(ipconfig.contains("VPN DNS     10.0.0.53") && ipconfig.contains("VPN domain  lab.test"), "{ipconfig}");
    // Names are asked of the pushed server (nobody answers at that address in this lab).
    let lookup = cli(&mut s, "PC1", "nslookup www.lab.test");
    assert!(lookup.contains("Server:  10.0.0.53"), "{lookup}");
    // The running configuration writes the lines back; `no` removes them; bad input is refused.
    cli(&mut s, "R1", "enable");
    let run = cli(&mut s, "R1", "show running-config");
    assert!(run.contains(" dns 10.0.0.53") && run.contains(" domain lab.test"), "{run}");
    s.execute("vpnclient.disconnect", json!({"device": "PC1"})).unwrap();
    group_lines(&mut s, &["no dns", "no domain"]);
    let st = connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    assert!(st["dns"].is_null() && st["domain"].is_null(), "{st}");
    let lines = ["crypto isakmp client configuration group team", "dns 0.0.0.0", "dns nope"];
    let r = s.execute("device.configure", json!({"device": "R1", "lines": lines})).unwrap();
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(2), "{r}");
}

#[test]
fn show_crypto_session_lists_the_clients_and_empties_with_them() {
    let mut s = lab(&[]);
    cli(&mut s, "R1", "enable");
    assert_eq!(cli(&mut s, "R1", "show crypto session"), "No VPN sessions");
    connect(&mut s, "team", "key-test", "ann", "pw-test").unwrap();
    let shown = cli(&mut s, "R1", "show crypto session");
    assert!(shown.contains("1 remote-access, 0 site-to-site"), "{shown}");
    assert!(shown.contains("Peer 203.0.113.2 (local 203.0.113.1)"), "{shown}");
    assert!(shown.contains("remote-access client, group team, user ann, VPN address 10.9.9.1"), "{shown}");
    assert!(ping_ok(&mut s, "10.0.0.10"));
    assert!(cli(&mut s, "R1", "show crypto session").contains("Status: up; ESP packets encapsulated"));
    s.execute("vpnclient.disconnect", json!({"device": "PC1"})).unwrap();
    assert_eq!(cli(&mut s, "R1", "show crypto session"), "No VPN sessions");
}
