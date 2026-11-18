//! A router or switch resolving host names: `ip host`, `ip name-server`, `ip domain-name`,
//! `no ip domain-lookup`, and the names `ping` and `traceroute` take.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_model::services::DnsRecord;

use super::Lab;

/// R1 (10.0.0.1) and a server (10.0.0.2) that answers DNS for `www.lab.test`.
fn lab_with_dns() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(srv, "g0", r, "g0/0");
    lab.run(srv, "ipconfig 10.0.0.2 255.255.255.0 10.0.0.1");
    lab.run(r, "en\nconf t\nint g0/0\nip add 10.0.0.1 255.255.255.0\nno shut\nend");
    {
        let s = lab.t.device_mut(srv).unwrap();
        s.services.dns.on = true;
        s.services.dns.records.push(DnsRecord { name: "www.lab.test".into(), address: "10.0.0.2".parse().unwrap() });
    }
    (lab, r, srv)
}

#[test]
fn a_static_host_name_is_used_by_ping_and_traceroute_and_written_back() {
    let (mut lab, r, _srv) = lab_with_dns();
    lab.run(r, "conf t\nip host WEB 10.0.0.2\nend");
    let ping = lab.run(r, "ping WEB");
    assert!(ping.starts_with("Translating \"WEB\"...static [OK]\n") && ping.contains("!!!"), "{ping}");
    assert!(lab.run(r, "traceroute web").contains("10.0.0.2"), "names are not case sensitive");
    assert!(lab.run(r, "show running-config").contains("ip host WEB 10.0.0.2\n"));
    let hosts = lab.run(r, "show hosts");
    assert!(hosts.contains("WEB") && hosts.contains("10.0.0.2 (static)"), "{hosts}");
    lab.run(r, "conf t\nip host WEB 10.0.0.9\nend");
    assert_eq!(lab.run(r, "show hosts").matches("WEB").count(), 1, "one name has one address");
    lab.run(r, "conf t\nno ip host WEB");
    assert!(lab.try_run(r, "no ip host WEB").starts_with("% No host named WEB"));
    lab.run(r, "end");
    assert!(lab.try_run(r, "ping WEB").starts_with("% Unrecognized host"), "the name is gone");
    lab.run(r, "conf t");
    assert!(lab.try_run(r, "ip host 10.0.0.2 10.0.0.3").starts_with("% A host name is a word"));
}

#[test]
fn a_name_server_resolves_what_the_host_table_does_not() {
    let (mut lab, r, _srv) = lab_with_dns();
    assert!(lab.try_run(r, "ping www.lab.test").starts_with("% Unrecognized host"), "no server yet");
    lab.run(r, "conf t\nip name-server 10.0.0.2 10.0.0.77\nip domain-name lab.test\nend");
    let ping = lab.run(r, "ping www.lab.test");
    assert!(ping.starts_with("Translating \"www.lab.test\"...domain server (10.0.0.2) [OK]\n") && ping.contains("!!!"), "{ping}");
    let short = lab.run(r, "ping www");
    assert!(short.contains("Translating \"www\"") && short.contains("!!!"), "the domain name is added: {short}");
    assert!(lab.try_run(r, "ping nothere").contains("doesn't know this name"));
    let run = lab.run(r, "show running-config");
    assert!(run.contains("ip name-server 10.0.0.2 10.0.0.77\n"), "{run}");
    assert!(lab.run(r, "show hosts").contains("name servers 10.0.0.2, 10.0.0.77"));
    lab.run(r, "conf t\nno ip name-server 10.0.0.2\nend");
    assert!(lab.run(r, "show hosts").contains("name servers 10.0.0.77"));
    assert!(lab.try_run(r, "ping www").contains("didn't answer"), "the first server left is asked");
    lab.run(r, "conf t\nno ip name-server\nip name-server 10.0.0.2\nno ip domain-lookup\nend");
    assert!(lab.try_run(r, "ping www.lab.test").starts_with("% Unrecognized host"), "no ip domain-lookup turns the server off");
    lab.run(r, "conf t\nip host www.lab.test 10.0.0.2\nend");
    assert!(lab.run(r, "ping www.lab.test").starts_with("Translating \"www.lab.test\"...static"), "the host table still works");
}

#[test]
fn name_settings_survive_a_save_and_a_reload() {
    let (mut lab, r, _srv) = lab_with_dns();
    lab.run(r, "conf t\nip host WEB 10.0.0.2\nip name-server 10.0.0.2\nend\ncopy running-config startup-config\nreload");
    lab.run(r, "en");
    let run = lab.run(r, "show running-config");
    assert!(run.contains("ip host WEB 10.0.0.2\n") && run.contains("ip name-server 10.0.0.2\n"), "{run}");
    assert!(lab.run(r, "ping WEB").contains("static"));
}

#[test]
fn telnet_and_connect_take_a_host_name() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 10.0.0.1 255.255.255.0\nno shut\nip host CORE 10.0.0.2\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 10.0.0.2 255.255.255.0\nno shut\nline vty 0 4\npassword remote\nlogin\nend");
    let out = lab.try_run(r1, "telnet CORE");
    assert!(out.starts_with("Translating \"CORE\"...static [OK]\n") && out.contains("Open"), "{out}");
    assert_eq!(lab.prompt(r1), "Password: ");
    assert_eq!(lab.try_run(r1, "remote"), "");
    lab.run(r1, "exit");
    let again = lab.try_run(r1, "connect core");
    assert!(again.contains("Translating") && again.contains("Open"), "{again}");
    lab.run(r1, "exit");
    assert!(lab.try_run(r1, "telnet NOPE").starts_with("% Unrecognized host"));
    assert!(lab.try_run(r1, "ssh -l admin NOPE").starts_with("% Unrecognized host"));
}
