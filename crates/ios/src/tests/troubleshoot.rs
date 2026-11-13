//! Typed console tests for what the troubleshooting labs (`docs/labs/21-…`) turned up.

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn show_interfaces_switchport_lists_every_switch_port_and_trunk_takes_a_port() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.run(s1, "en\nconf t\nvlan 10\nname Sales\ninterface fa0/1\nswitchport access vlan 10\ninterface g0/1\nswitchport mode trunk\nend");
    let all = lab.run(s1, "show interfaces switchport");
    assert!(all.contains("FastEthernet0/1\n  Mode: access") && all.contains("Access VLAN: 10 (Sales)"), "{all}");
    assert!(all.contains("GigabitEthernet0/1\n  Mode: trunk"), "{all}");
    assert!(all.contains("\n\nFastEthernet0/2"), "ports are separated by a blank line: {all}");
    let one = lab.run(s1, "show interfaces g0/1 trunk");
    assert!(one.contains("Gi0/1") && !one.contains("Gi0/2"), "{one}");
    assert!(lab.run(s1, "show interfaces fa0/1 trunk").ends_with("(not a trunk port)"));
    assert!(lab.try_run(s1, "show interfaces bogus trunk").starts_with("% "));
}

#[test]
fn ospf_neighbours_can_be_filtered_and_a_route_asked_for_by_network_and_mask() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    for (r, ip, id) in [(r1, "10.0.0.1", "1.1.1.1"), (r2, "10.0.0.2", "2.2.2.2")] {
        lab.run(
            r,
            &format!("en\nconf t\ninterface g0/0\nip address {ip} 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id {id}\nnetwork 10.0.0.0 0.0.0.255 area 0\nend"),
        );
    }
    lab.sim.run_for(&lab.t, 60_000_000);
    assert!(lab.run(r1, "show ip ospf neighbor 2.2.2.2").contains("FULL"));
    assert!(lab.run(r1, "show ip ospf neighbor g0/0").contains("2.2.2.2"));
    assert!(lab.try_run(r1, "show ip ospf neighbor 9.9.9.9").starts_with("% No OSPF neighbour"));
    assert!(lab.try_run(r1, "show ip ospf neighbor g0/1").starts_with("% No OSPF neighbour"));
    assert!(lab.try_run(r1, "show ip ospf neighbor bogus").starts_with("% "));
    assert!(lab.run(r1, "show ip route 10.0.0.0 255.255.255.0").starts_with("Routing entry for 10.0.0.0/24"));
    assert!(lab.run(r1, "show ip route 10.0.0.7 255.255.255.0").starts_with("Routing entry for 10.0.0.0/24"));
    assert!(lab.try_run(r1, "show ip route 10.9.0.0 255.255.0.0").starts_with("% Network 10.9.0.0/16 is not"));
    assert!(lab.try_run(r1, "show ip route 10.0.0.0 255.0.255.0").starts_with("% "));
}

#[test]
fn an_acl_line_keeps_its_log_keyword_and_counters_clear_per_list() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(
        r,
        "en\nconf t\nip access-list extended A\npermit tcp any any eq 443 log\ndeny ip any any\nexit\nip access-list standard B\ndeny 10.0.0.0 0.0.0.255 log\nend",
    );
    assert!(lab.run(r, "show access-lists A").contains("10 permit tcp any any eq 443 log"));
    assert!(lab.run(r, "show access-lists B").contains("10 deny 10.0.0.0 0.0.0.255 log"));
    assert!(lab.run(r, "show running-config").contains(" permit tcp any any eq 443 log"));
    lab.run(r, "clear ip access-list counters A");
    lab.run(r, "clear access-list counters B");
    lab.run(r, "clear ip access-list counters");
    assert!(lab.try_run(r, "clear access-list counters NOPE").starts_with("% No access list NOPE"));
}

#[test]
fn proxy_arp_can_be_turned_off_per_interface_and_shows_in_the_config() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(pc, "fa0", r, "g0/0");
    lab.run(r, "en\nconf t\ninterface g0/0\nip address 10.0.0.1 255.255.255.0\nno shutdown\nend");
    assert!(lab.run(r, "show ip interface g0/0").contains("Proxy ARP: on"));
    assert!(!lab.run(r, "show running-config interface g0/0").contains("proxy-arp"));
    lab.run(r, "conf t\ninterface g0/0\nno ip proxy-arp\nend");
    assert!(lab.run(r, "show ip interface g0/0").contains("Proxy ARP: off"));
    assert!(lab.run(r, "show running-config interface g0/0").contains(" no ip proxy-arp"));
    lab.run(r, "conf t\ninterface g0/0\nip proxy-arp\nend");
    assert!(lab.run(r, "show ip interface g0/0").contains("Proxy ARP: on"));
}
