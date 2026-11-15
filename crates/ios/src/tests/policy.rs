//! Prefix lists and route maps typed at the console, and where they apply: the inbound and
//! outbound policy of a BGP neighbour, the route map of a redistribution, the views and the
//! running configuration.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 (AS 65001) and R2 (AS 65002) on 10.0.12.0/30. R1 owns three loopback networks and
/// advertises them with `network`: 172.16.1.0/24, 172.16.2.0/24 and 10.99.0.0/24.
fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nint lo2\nip add 172.16.2.1 255.255.255.0\n\
         int lo3\nip add 10.99.0.1 255.255.255.0\nexit\nrouter bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\n\
         network 172.16.1.0 mask 255.255.255.0\nnetwork 172.16.2.0 mask 255.255.255.0\nnetwork 10.99.0.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nexit\nrouter bgp 65002\nbgp router-id 2.2.2.2\n\
         neighbor 10.0.12.1 remote-as 65001\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2)
}

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

#[test]
fn prefix_lists_are_numbered_by_five_and_shown() {
    let (mut lab, _r1, r2) = pair();
    lab.run(r2, "conf t\nip prefix-list NETS permit 10.0.0.0/8 le 24\nip prefix-list NETS deny 0.0.0.0/0 le 32\nip prefix-list NETS seq 7 permit 192.168.0.0/16 ge 24\nend");
    let shown = lab.run(r2, "show ip prefix-list NETS");
    assert!(shown.contains("Prefix list NETS: 3 lines"), "{shown}");
    let first = shown.find("seq 5 permit 10.0.0.0/8 le 24").expect("line 5");
    let second = shown.find("seq 7 permit 192.168.0.0/16 ge 24").expect("line 7");
    let third = shown.find("seq 10 deny 0.0.0.0/0 le 32").expect("line 10");
    assert!(first < second && second < third, "{shown}");
    assert!(lab.run(r2, "show ip prefix-list").contains("Prefix list NETS"));
    assert!(lab.try_run(r2, "show ip prefix-list NOPE").contains("No prefix list named NOPE"));
    // The running configuration writes the numbers out.
    let run = lab.run(r2, "show running-config");
    assert!(run.contains("ip prefix-list NETS seq 5 permit 10.0.0.0/8 le 24\nip prefix-list NETS seq 7 permit 192.168.0.0/16 ge 24\nip prefix-list NETS seq 10 deny 0.0.0.0/0 le 32\n!"), "{run}");
    // Deleting one line by number, one by its text, then the list.
    lab.run(r2, "conf t\nno ip prefix-list NETS seq 7");
    lab.run(r2, "no ip prefix-list NETS deny 0.0.0.0/0 le 32\nend");
    let left = lab.run(r2, "show ip prefix-list NETS");
    assert!(left.contains("1 line") && left.contains("seq 5 permit 10.0.0.0/8 le 24") && !left.contains("seq 7"), "{left}");
    lab.run(r2, "conf t\nno ip prefix-list NETS");
    assert!(lab.try_run(r2, "no ip prefix-list NETS").contains("No prefix list named NETS"));
    assert!(lab.run(r2, "end\nshow ip prefix-list").contains("no prefix lists"));
}

#[test]
fn prefix_list_input_is_checked() {
    let (mut lab, _r1, r2) = pair();
    lab.run(r2, "conf t");
    assert!(lab.try_run(r2, "ip prefix-list P permit 10.0.0.0").contains("A.B.C.D/LEN"));
    assert!(lab.try_run(r2, "ip prefix-list P permit 10.0.0.0/33").contains("%"));
    assert!(lab.try_run(r2, "ip prefix-list P allow 10.0.0.0/8").contains("permit or deny"));
    assert!(lab.try_run(r2, "ip prefix-list P permit 10.0.0.0/8 ge 8").contains("Invalid range"));
    assert!(lab.try_run(r2, "ip prefix-list P permit 10.0.0.0/8 ge 24 le 16").contains("Invalid range"));
    assert!(lab.try_run(r2, "ip prefix-list P permit 10.0.0.0/8 le 40").contains("0-32"));
    assert!(lab.try_run(r2, "ip prefix-list P permit 10.0.0.0/8 le 24 le 25").contains("Unexpected"));
    assert!(lab.try_run(r2, "ip prefix-list P seq 0 permit 10.0.0.0/8").contains("sequence number"));
    assert!(lab.try_run(r2, "ip prefix-list P seq 5").contains("permit or deny"));
    // None of the refused lines left an empty list behind.
    assert!(lab.run(r2, "end\nshow ip prefix-list").contains("no prefix lists"));
    lab.run(r2, "conf t\nip prefix-list P seq 5 permit 10.0.0.0/8");
    assert!(lab.try_run(r2, "ip prefix-list P seq 5 permit 11.0.0.0/8").contains("already in use"));
    assert!(lab.try_run(r2, "ip prefix-list P seq 9 permit 10.0.0.0/8").contains("already in"));
}

#[test]
fn an_inbound_prefix_list_keeps_out_what_it_does_not_permit() {
    let (mut lab, _r1, r2) = pair();
    let all = lab.run(r2, "show ip bgp");
    assert!(all.contains("172.16.1.0/24") && all.contains("172.16.2.0/24") && all.contains("10.99.0.0/24"), "{all}");
    lab.run(r2, "conf t\nip prefix-list ONLY172 permit 172.16.0.0/16 le 24\nrouter bgp 65002\nneighbor 10.0.12.1 prefix-list ONLY172 in\nend");
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.1.0/24") && table.contains("172.16.2.0/24") && !table.contains("10.99.0.0/24"), "{table}");
    assert!(!lab.run(r2, "show ip route bgp").contains("10.99.0.0/24"));
    let n = lab.run(r2, "show ip bgp neighbors 10.0.12.1");
    assert!(n.contains("Policy: inbound prefix list ONLY172") && n.contains("Prefixes received 2"), "{n}");
    // The session stayed up while the policy changed.
    assert!(n.contains("State: Established"), "{n}");
    // A name with no list behind it lets everything through.
    lab.run(r2, "conf t\nno ip prefix-list ONLY172\nend");
    assert!(lab.run(r2, "show ip bgp").contains("10.99.0.0/24"));
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.12.1").contains("not defined: lets everything through"));
    // The soft reset only says so: nothing is reset.
    let soft = lab.run(r2, "clear ip bgp 10.0.12.1 soft in");
    assert!(soft.contains("No session was reset"), "{soft}");
    assert!(lab.try_run(r2, "clear ip bgp 9.9.9.9 soft").contains("No neighbour"));
    // The no form takes the policy off, and only the one that is there.
    lab.run(r2, "conf t\nrouter bgp 65002");
    assert!(lab.try_run(r2, "no neighbor 10.0.12.1 prefix-list OTHER in").contains("not applied"));
    assert!(lab.try_run(r2, "no neighbor 10.0.12.1 prefix-list ONLY172 out").contains("not applied"));
    lab.run(r2, "no neighbor 10.0.12.1 prefix-list ONLY172 in");
    assert!(!lab.run(r2, "end\nshow running-config").contains("prefix-list ONLY172"));
}

#[test]
fn an_inbound_route_map_sets_attributes_and_drops_what_no_clause_matches() {
    let (mut lab, _r1, r2) = pair();
    lab.run(
        r2,
        "conf t\nip prefix-list ONE permit 172.16.1.0/24\nroute-map FROM-R1 permit 10\nmatch ip address prefix-list ONE\nset local-preference 200\nset metric 50\n\
         set weight 7\nset origin egp\nexit\nrouter bgp 65002\nneighbor 10.0.12.1 route-map FROM-R1 in\nend",
    );
    // Only clause 10 exists, so everything it does not match is dropped.
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.1.0/24") && !table.contains("172.16.2.0/24") && !table.contains("10.99.0.0/24"), "{table}");
    let one = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(one.contains("origin EGP, metric 50, local preference 200, weight 7, external"), "{one}");
    // A second clause with no match lines takes the rest, with another preference.
    lab.run(r2, "conf t\nroute-map FROM-R1 permit 20\nset local-preference 50\nend");
    let two = lab.run(r2, "show ip bgp 172.16.2.0");
    assert!(two.contains("local preference 50"), "{two}");
    assert!(lab.run(r2, "show ip bgp 172.16.1.0").contains("local preference 200"));
    // A deny clause in front drops what it matches.
    lab.run(r2, "conf t\nip prefix-list NINE permit 10.99.0.0/24\nroute-map FROM-R1 deny 5\nmatch ip address prefix-list NINE\nend");
    let table = lab.run(r2, "show ip bgp");
    assert!(!table.contains("10.99.0.0/24") && table.contains("172.16.2.0/24"), "{table}");
    let map = lab.run(r2, "show route-map FROM-R1");
    assert!(map.contains("Route map FROM-R1: 3 clauses") && map.contains("deny, sequence 5"), "{map}");
    assert!(map.contains("set local-preference 200") && map.contains("match prefix list: ONE"), "{map}");
}

#[test]
fn an_outbound_route_map_prepends_and_sets_the_metric() {
    let (mut lab, r1, r2) = pair();
    lab.run(
        r1,
        "conf t\nroute-map TO-R2 permit 10\nset as-path prepend 65001 65001\nset metric 77\nexit\nrouter bgp 65001\nneighbor 10.0.12.2 route-map TO-R2 out\nend",
    );
    let one = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(one.contains("AS path 65001 65001 65001, next hop 10.0.12.1"), "{one}");
    assert!(one.contains("metric 77"), "{one}");
    // An outbound prefix list keeps prefixes from being sent at all.
    lab.run(r1, "conf t\nip prefix-list SEND permit 172.16.1.0/24\nrouter bgp 65001\nneighbor 10.0.12.2 prefix-list SEND out\nend");
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.1.0/24") && !table.contains("172.16.2.0/24") && !table.contains("10.99.0.0/24"), "{table}");
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("Prefixes received 0, sent 1"));
    advance(&mut lab, 2);
    assert!(lab.run(r2, "show ip route bgp").contains("172.16.1.0/24"));
}

#[test]
fn a_redistribution_route_map_filters_and_sets_attributes() {
    let (mut lab, r1, r2) = pair();
    lab.run(r1, "conf t\nrouter bgp 65001\nno network 172.16.1.0 mask 255.255.255.0\nno network 172.16.2.0 mask 255.255.255.0\nno network 10.99.0.0 mask 255.255.255.0\nend");
    lab.run(
        r1,
        "conf t\nip prefix-list LO1 permit 172.16.1.0/24\nroute-map LOOP permit 10\nmatch ip address prefix-list LO1\nset metric 30\nset local-preference 333\nexit\n\
         router bgp 65001\nredistribute connected route-map LOOP\nend",
    );
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.1.0/24") && table.contains("65001 ?") && !table.contains("10.99.0.0/24"), "{table}");
    assert!(lab.run(r2, "show ip bgp 172.16.1.0").contains("metric 30"));
    let mine = lab.run(r1, "show ip bgp 172.16.1.0");
    assert!(mine.contains("local preference 333") && mine.contains("originated here"), "{mine}");
    assert!(lab.run(r1, "show running-config").contains(" redistribute connected route-map LOOP\n"));
    // Without the map every connected network goes.
    lab.run(r1, "conf t\nrouter bgp 65001\nredistribute connected\nend");
    assert!(lab.run(r2, "show ip bgp").contains("10.99.0.0/24"));
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "redistribute connected route-map").contains("one name"));
}

#[test]
fn a_route_map_can_match_an_access_list_and_a_metric() {
    let (mut lab, _r1, r2) = pair();
    lab.run(
        r2,
        "conf t\naccess-list 10 permit 10.99.0.0 0.0.0.255\nroute-map ACL permit 10\nmatch ip address 10\nset local-preference 400\nexit\n\
         route-map ACL permit 20\nmatch metric 5\nset local-preference 5\nexit\nrouter bgp 65002\nneighbor 10.0.12.1 route-map ACL in\nend",
    );
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("10.99.0.0/24") && !table.contains("172.16.1.0/24"), "{table}");
    assert!(lab.run(r2, "show ip bgp 10.99.0.0").contains("local preference 400"));
    let map = lab.run(r2, "show route-map ACL");
    assert!(map.contains("match access list: 10") && map.contains("match metric: 5"), "{map}");
}

#[test]
fn route_map_mode_and_its_commands() {
    let (mut lab, _r1, r2) = pair();
    lab.run(r2, "conf t\nroute-map M deny 30");
    assert_eq!(lab.prompt(r2), "R2(config-route-map)#");
    lab.run(r2, "match metric 3\nset ip next-hop 10.0.12.9\nset as-path prepend 1 2 3\nset weight 9\nset origin incomplete");
    assert!(lab.try_run(r2, "set local-preference x").contains("0-4294967295"));
    assert!(lab.try_run(r2, "set weight 70000").contains("0-65535"));
    assert!(lab.try_run(r2, "set origin nowhere").contains("igp, egp or incomplete"));
    assert!(lab.try_run(r2, "set ip next-hop 0.0.0.0").contains("not an address"));
    assert!(lab.try_run(r2, "set as-path prepend 0").contains("AS number"));
    assert!(lab.try_run(r2, "match ip address prefix-list").contains("at least one"));
    lab.run(r2, "no set weight\nno match metric");
    // A global command leaves the mode; `exit` goes back one level.
    lab.run(r2, "exit");
    assert_eq!(lab.prompt(r2), "R2(config)#");
    let run = lab.run(r2, "end\nshow running-config");
    assert!(run.contains("route-map M deny 30\n set as-path prepend 1 2 3\n set ip next-hop 10.0.12.9\n set origin incomplete\n!"), "{run}");
    // Without a sequence number the only clause is edited; with several the number is needed.
    lab.run(r2, "conf t\nroute-map M");
    assert_eq!(lab.prompt(r2), "R2(config-route-map)#");
    lab.run(r2, "exit\nroute-map M permit 40\nexit");
    assert!(lab.try_run(r2, "route-map M").contains("several clauses"));
    assert!(lab.try_run(r2, "route-map M permit 0").contains("sequence number"));
    assert!(lab.try_run(r2, "no route-map M permit 99").contains("no clause 99"));
    lab.run(r2, "no route-map M permit 40\nend");
    assert!(lab.run(r2, "show route-map").contains("Route map M: 1 clause"));
    lab.run(r2, "conf t\nno route-map M");
    assert!(lab.try_run(r2, "no route-map M").contains("No route map named M"));
    assert!(lab.run(r2, "end\nshow route-map").contains("no route maps"));
}

#[test]
fn the_configuration_survives_a_reload() {
    let (mut lab, r1, r2) = pair();
    lab.run(
        r2,
        "conf t\nip prefix-list A seq 10 permit 172.16.0.0/16 le 24\nip prefix-list A seq 20 deny 0.0.0.0/0 le 32\nip prefix-list B permit 10.0.0.0/8\n\
         access-list 20 permit any\nroute-map IN permit 10\nmatch ip address prefix-list A B\nmatch ip address 20\nmatch metric 0\nset local-preference 120\n\
         set metric 5\nset weight 3\nset as-path prepend 65002\nset ip next-hop 10.0.12.1\nset origin igp\nroute-map IN deny 20\nexit\n\
         router bgp 65002\nneighbor 10.0.12.1 prefix-list A in\nneighbor 10.0.12.1 route-map IN in\nneighbor 10.0.12.1 route-map IN out\nend",
    );
    lab.run(r1, "conf t\nrouter bgp 65001\nredistribute static route-map X\nredistribute connected route-map LOOP\nend");
    for r in [r1, r2] {
        let before = lab.run(r, "show running-config");
        lab.run(r, "copy run start");
        lab.run(r, "reload");
        let after = lab.run(r, "en\nshow running-config");
        assert_eq!(after, before);
    }
    let run = lab.run(r2, "show running-config");
    assert!(
        run.contains(" neighbor 10.0.12.1 prefix-list A in\n neighbor 10.0.12.1 route-map IN in\n neighbor 10.0.12.1 route-map IN out\n"),
        "{run}"
    );
    assert!(run.contains("route-map IN permit 10\n match ip address prefix-list A B\n match ip address 20\n match metric 0\n set local-preference 120\n set metric 5\n set weight 3\n set as-path prepend 65002\n set ip next-hop 10.0.12.1\n set origin igp\n!\nroute-map IN deny 20\n!"), "{run}");
    assert!(lab.run(r1, "show running-config").contains(" redistribute connected route-map LOOP\n redistribute static route-map X\n"));
    // A factory reset forgets them.
    lab.run(r2, "erase startup-config");
    lab.run(r2, "reload");
    assert!(lab.run(r2, "show route-map").contains("no route maps"));
    assert!(lab.run(r2, "show ip prefix-list").contains("no prefix lists"));
}

#[test]
fn ospf_redistribution_takes_a_route_map() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(r1, "en\nconf t\nint g0/1\nip add 10.0.0.1 255.255.255.252\nno shut\nexit\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nend");
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.0.2 255.255.255.252\nno shut\nint lo5\nip address 10.5.5.5 255.255.255.0\nexit\n\
         ip route 172.31.0.0 255.255.0.0 10.5.5.9\nip route 172.30.0.0 255.255.0.0 10.5.5.9\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nredistribute static subnets\nend",
    );
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O E2 172.31.0.0/16") && rt.contains("O E2 172.30.0.0/16"), "{rt}");
    lab.run(
        r2,
        "conf t\nip prefix-list S31 permit 172.31.0.0/16\nroute-map STATICS permit 10\nmatch ip address prefix-list S31\nexit\nrouter ospf 1\n\
         redistribute static subnets route-map STATICS\nend",
    );
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O E2 172.31.0.0/16") && !rt.contains("172.30.0.0/16"), "{rt}");
    assert!(lab.run(r2, "show running-config").contains(" redistribute static subnets route-map STATICS\n"));
    lab.run(r2, "conf t\nrouter ospf 1\nredistribute static subnets\nend");
    assert!(lab.run(r1, "show ip route").contains("O E2 172.30.0.0/16"));
}
