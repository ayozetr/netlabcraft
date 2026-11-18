//! BGP's IPv6 routes in the IPv6 routing table (distance 20 for external paths, 200 for internal ones)
//! and the `show bgp ipv6 unicast` views.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001, loopback 2001:db8:a::1/64) - R2 (AS 65002, loopback 2001:db8:b::1/64) over IPv6.
fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let a = lab.add(DeviceKind::Router);
    let b = lab.add(DeviceKind::Router);
    lab.cable(a, "g0/1", b, "g0/1");
    lab.run(
        a,
        "en\nconf t\nipv6 unicast-routing\nint g0/1\nipv6 address 2001:db8:12::1/64\nno shut\nexit\nint lo1\nipv6 address 2001:db8:a::1/64\nexit\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 2001:db8:12::2 remote-as 65002\naddress-family ipv6\nneighbor 2001:db8:12::2 activate\n\
         network 2001:db8:a::/64\nend",
    );
    lab.run(
        b,
        "en\nconf t\nipv6 unicast-routing\nint g0/1\nipv6 address 2001:db8:12::2/64\nno shut\nexit\nint lo1\nipv6 address 2001:db8:b::1/64\nexit\n\
         router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 2001:db8:12::1 remote-as 65001\naddress-family ipv6\nneighbor 2001:db8:12::1 activate\n\
         network 2001:db8:b::/64\nend",
    );
    advance(&mut lab, 8);
    (lab, a, b)
}

#[test]
fn an_external_path_is_a_b_route_at_distance_20_and_traffic_follows_it() {
    let (mut lab, a, b) = pair();
    let route = lab.run(a, "show ipv6 route");
    assert!(route.contains("B  2001:db8:b::/64") && route.contains("via 2001:db8:12::2, GigabitEthernet0/1 [20]"), "{route}");
    let only = lab.run(a, "show ipv6 route bgp");
    assert!(only.contains("2001:db8:b::/64") && !only.contains("2001:db8:a::/64"), "{only}");
    assert!(lab.run(a, "ping 2001:db8:b::1").contains('!'));
    assert!(lab.run(b, "ping 2001:db8:a::1").contains('!'));
}

#[test]
fn the_bgp_table_lists_the_paths_and_the_entry_view_the_next_hops() {
    let (mut lab, a, _b) = pair();
    let table = lab.run(a, "show bgp ipv6 unicast");
    assert!(table.contains("*>  2001:db8:a::/64") && table.contains("*>  2001:db8:b::/64"), "{table}");
    assert!(table.contains("65002 i"), "{table}");
    let entry = lab.run(a, "show bgp ipv6 unicast 2001:db8:b::/64");
    assert!(entry.contains("BGP routing table entry for 2001:db8:b::/64"), "{entry}");
    assert!(entry.contains("Next hop 2001:db8:12::2 (link-local fe80::"), "the neighbour's global and link-local next hops: {entry}");
    assert!(entry.contains("external, best"), "{entry}");
    // An address finds the most specific prefix that covers it.
    assert!(lab.run(a, "show bgp ipv6 unicast 2001:db8:b::77").contains("entry for 2001:db8:b::/64"));
    assert!(lab.try_run(a, "show bgp ipv6 unicast 2001:db8:ffff::/48").contains("not in the BGP IPv6 table"));
}

#[test]
fn the_neighbour_views_and_the_advertised_routes() {
    let (mut lab, a, _b) = pair();
    let n = lab.run(a, "show bgp ipv6 unicast neighbors 2001:db8:12::2");
    assert!(n.contains("BGP neighbor is 2001:db8:12::2, remote AS 65002, external link"), "{n}");
    assert!(n.contains("Session over IPv6, state Established"), "{n}");
    assert!(n.contains("negotiated, 1 prefixes received, 1 sent"), "{n}");
    let adv = lab.run(a, "show bgp ipv6 unicast neighbors 2001:db8:12::2 advertised-routes");
    assert!(adv.contains("2001:db8:a::/64") && adv.contains("65001 i"), "{adv}");
    assert!(lab.try_run(a, "show bgp ipv6 unicast neighbors 2001:db8:99::1").contains("not a neighbour"));
}

#[test]
fn an_aggregate_leaves_a_discard_route_and_a_lost_session_takes_the_routes_away() {
    let (mut lab, a, _b) = pair();
    lab.run(a, "conf t\nrouter bgp 65001\naddress-family ipv6\naggregate-address 2001:db8:a::/48 summary-only\nend");
    advance(&mut lab, 4);
    let route = lab.run(a, "show ipv6 route bgp");
    assert!(route.contains("2001:db8:a::/48") && route.contains("discarded on Null0"), "{route}");
    lab.run(a, "conf t\nrouter bgp 65001\nneighbor 2001:db8:12::2 shutdown\nend");
    advance(&mut lab, 4);
    assert!(!lab.run(a, "show ipv6 route bgp").contains("2001:db8:b::/64"));
    assert!(lab.run(a, "show bgp ipv6 unicast summary").contains("Idle (Admin)"));
}

#[test]
fn an_internal_path_has_distance_200_and_its_next_hop_must_resolve() {
    let mut lab = Lab::new();
    let a = lab.add(DeviceKind::Router);
    let b = lab.add(DeviceKind::Router);
    lab.cable(a, "g0/1", b, "g0/1");
    lab.run(
        a,
        "en\nconf t\nipv6 unicast-routing\nint g0/1\nipv6 address 2001:db8:12::1/64\nno shut\nexit\nint lo1\nipv6 address 2001:db8:a::1/64\nexit\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 2001:db8:12::2 remote-as 65001\naddress-family ipv6\nneighbor 2001:db8:12::2 activate\n\
         network 2001:db8:a::/64\nend",
    );
    lab.run(
        b,
        "en\nconf t\nipv6 unicast-routing\nint g0/1\nipv6 address 2001:db8:12::2/64\nno shut\nexit\n\
         router bgp 65001\nbgp router-id 2.2.2.2\nneighbor 2001:db8:12::1 remote-as 65001\naddress-family ipv6\nneighbor 2001:db8:12::1 activate\nend",
    );
    advance(&mut lab, 8);
    let route = lab.run(b, "show ipv6 route bgp");
    assert!(route.contains("2001:db8:a::/64") && route.contains("[200]"), "{route}");
    let entry = lab.run(b, "show bgp ipv6 unicast 2001:db8:a::/64");
    assert!(entry.contains("internal, best"), "{entry}");
}
