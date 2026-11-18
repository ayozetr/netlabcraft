//! Settings that are kept and shown but not simulated (`netlabcraft_model::knobs`): `service …`,
//! `ip classless`, interface and line options. They are checked, replaced, removed with `no`,
//! written to the running configuration and survive a save and reload.

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn a_global_setting_is_kept_replaced_shown_and_removed() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t");
    let said = lab.run(r, "service sequence-numbers");
    assert!(said.contains("Accepted"), "a setting that changes nothing says so: {said}");
    lab.run(r, "ip tcp synwait-time 40\nip tcp synwait-time 50\ncdp timer 20\nlldp reinit 3");
    lab.run(r, "end");
    let run = lab.run(r, "show running-config");
    assert!(run.contains("service sequence-numbers\n"), "{run}");
    assert!(run.contains("ip tcp synwait-time 50\n") && !run.contains("synwait-time 40"), "the value is replaced: {run}");
    assert!(run.contains("cdp timer 20\n") && run.contains("lldp reinit 3\n"), "{run}");
    lab.run(r, "conf t\nno service sequence-numbers\nno ip tcp synwait-time\nend");
    let run = lab.run(r, "show running-config");
    assert!(!run.contains("sequence-numbers") && !run.contains("synwait-time"), "{run}");
}

#[test]
fn a_setting_that_is_on_by_default_is_written_only_when_turned_off() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nip classless");
    assert!(!lab.run(r, "do show running-config").contains("classless"), "on is the default");
    lab.run(r, "no ip classless\nno service pad\nint g0/0\nno ip redirects\nno ip unreachables");
    let run = lab.run(r, "do show running-config");
    assert!(run.contains("no ip classless\n") && run.contains("no service pad\n"), "{run}");
    assert!(run.contains(" no ip redirects\n no ip unreachables\n"), "{run}");
    lab.run(r, "ip redirects\nexit\nip classless");
    let run = lab.run(r, "do show running-config");
    assert!(!run.contains("no ip classless") && !run.contains("no ip redirects") && run.contains("no ip unreachables"), "{run}");
}

#[test]
fn values_are_checked_against_their_range_or_their_words() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nint g0/0");
    assert!(lab.try_run(r, "load-interval 5").starts_with("% "), "30-600");
    assert!(lab.try_run(r, "load-interval abc").starts_with("% "));
    assert!(lab.try_run(r, "ipv6 mtu 100").starts_with("% "), "1280 at least");
    lab.run(r, "load-interval 30\nipv6 mtu 1500");
    lab.run(r, "exit\nline vty 0 4");
    assert!(lab.try_run(r, "stopbits 3").starts_with("% Say one of"), "{}", lab.try_run(r, "stopbits 3"));
    assert!(lab.try_run(r, "session-timeout 99999").starts_with("% "));
    lab.run(r, "stopbits 2\nsession-timeout 15\nno exec-banner\nrotary 5");
    lab.run(r, "end");
    let run = lab.run(r, "show running-config");
    assert!(run.contains(" load-interval 30\n") && run.contains(" ipv6 mtu 1500\n"), "{run}");
    assert!(run.contains("line vty 0 4\n") && run.contains(" stopbits 2\n") && run.contains(" session-timeout 15\n"), "{run}");
    assert!(run.contains(" no exec-banner\n") && run.contains(" rotary 5\n"), "{run}");
    let con = run.split("line con 0").nth(1).unwrap_or("").split("line vty").next().unwrap_or("");
    assert!(!con.contains("stopbits"), "the console keeps its own settings: {con}");
}

#[test]
fn a_setting_with_a_word_in_it_keeps_one_line_per_word() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en\nconf t\nint fa0/1");
    lab.run(s, "storm-control broadcast level 50\nstorm-control multicast level 20.5\nstorm-control broadcast level 40\nstorm-control action shutdown\nswitchport block unicast");
    assert!(lab.try_run(s, "storm-control broadcast level 101").starts_with("% "));
    assert!(lab.try_run(s, "storm-control anycast level 5").starts_with("% Say one of"));
    lab.run(s, "end");
    let run = lab.run(s, "show running-config");
    assert!(run.contains(" storm-control broadcast level 40\n") && !run.contains("level 50"), "{run}");
    assert!(run.contains(" storm-control multicast level 20.5\n") && run.contains(" storm-control action shutdown\n"), "{run}");
    lab.run(s, "conf t\nint fa0/1\nno storm-control broadcast level\nend");
    let run = lab.run(s, "show running-config");
    assert!(!run.contains("broadcast level") && run.contains("multicast level 20.5"), "{run}");
}

#[test]
fn settings_survive_save_and_reload_and_router_only_ones_are_refused_on_a_switch() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(
        s,
        "en\nconf t\nspanning-tree backbonefast\nmac address-table aging-time 600\nno spanning-tree extend system-id\nint vlan 5\nip unreachables",
    );
    lab.run(s, "end\ncopy running-config startup-config\nreload");
    lab.run(s, "en");
    let run = lab.run(s, "show running-config");
    assert!(run.contains("spanning-tree backbonefast\n") && run.contains("mac address-table aging-time 600\n"), "{run}");
    assert!(run.contains("no spanning-tree extend system-id\n"), "{run}");
    lab.run(s, "conf t\nint fa0/1");
    let router_only = lab.try_run(s, "arp timeout 300");
    assert!(router_only.contains("% "), "arp timeout is a router setting: {router_only}");
}

#[test]
fn no_service_dhcp_stops_the_server() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.run(
        r,
        "en\nconf t\nint g0/0\nip add 192.168.10.1 255.255.255.0\nno shut\nexit\nip dhcp excluded-address 192.168.10.1 192.168.10.9\n\
         ip dhcp pool LAN\nnetwork 192.168.10.0 255.255.255.0\ndefault-router 192.168.10.1\nexit\nno service dhcp\nend",
    );
    let off = lab.run(a, "ipconfig /renew");
    assert!(!off.contains("192.168.10.10/24"), "the server is off: {off}");
    lab.run(r, "conf t\nservice dhcp\nend");
    let on = lab.run(a, "ipconfig /renew");
    assert!(on.contains("192.168.10.10/24"), "{on}");
}
