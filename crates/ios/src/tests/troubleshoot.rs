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
fn a_dhcp_pool_shows_how_many_addresses_are_left_and_can_be_asked_for_by_name() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(
        r,
        "en\nconf t\nip dhcp excluded-address 10.0.0.1 10.0.0.10\nip dhcp excluded-address 10.0.0.5 10.0.0.20\nip dhcp pool A\nnetwork 10.0.0.0 255.255.255.0\nexit\nip dhcp pool B\nnetwork 10.9.0.0 255.255.255.252\nend",
    );
    let a = lab.run(r, "show ip dhcp pool A");
    assert!(a.contains("Pool A") && !a.contains("Pool B"), "{a}");
    // 254 usable; 1-20 excluded once even though the ranges overlap.
    assert!(a.contains("254 usable, 20 excluded, 234 free"), "{a}");
    assert!(lab.run(r, "show ip dhcp pool").contains("2 usable, 0 excluded, 2 free"));
    assert!(lab.try_run(r, "show ip dhcp pool NOPE").starts_with("% No DHCP pool NOPE"));
}

#[test]
fn nat_statistics_count_hits_and_misses_and_clear() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let before = lab.run(r, "show ip nat statistics");
    assert!(before.contains("Hits: 0 (found a translation)  Misses: 0"), "{before}");
    lab.run(r, "en\nclear ip nat statistics");
    assert!(lab.run(r, "show ip nat statistics").contains("Hits: 0"));
}

#[test]
fn spanning_tree_root_bridge_and_blockedports_views() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(s1, "g0/2", s3, "g0/1");
    lab.cable(s2, "g0/2", s3, "g0/2");
    for s in [s1, s2, s3] {
        lab.run(s, "en\nconf t\ninterface range g0/1 - 2\nswitchport mode trunk\nend");
    }
    lab.run(s2, "conf t\nspanning-tree vlan 1 root primary\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    assert!(lab.run(s2, "show spanning-tree root").contains("this switch is the root"));
    assert!(lab.run(s2, "show spanning-tree bridge").contains("24577") && lab.run(s2, "show spanning-tree bridge").contains("yes"));
    assert!(lab.run(s1, "show spanning-tree root").contains("24577") && lab.run(s1, "show spanning-tree root").contains("Gi0/1"));
    assert!(lab.run(s2, "show spanning-tree blockedports").contains("none"));
    // The ring has one blocked port, on a switch that is not the root.
    let blocked: Vec<String> = [s1, s3].iter().map(|s| lab.run(*s, "show spanning-tree blockedports")).collect();
    assert_eq!(blocked.iter().filter(|b| b.contains("1 blocked port")).count(), 1, "{blocked:?}");
    // Hosts and routers have no spanning tree to show.
    let r = lab.add(DeviceKind::Router);
    assert!(lab.try_run(r, "show spanning-tree root").starts_with("% ") || lab.try_run(r, "show spanning-tree root").contains("Not understood"));
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
