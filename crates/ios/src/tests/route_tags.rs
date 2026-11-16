//! Route tags typed at the console: `ip route … tag N`, the tag a route carries in the routing
//! table, `match tag` and `set tag` in route maps, `redistribute … tag N` under OSPF, and the tag
//! in OSPF's external LSAs, in EIGRP's external routes and in RIP's route-tag field.

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 - R2 - R3 in a line: R1-R2 is 10.0.12.0/24, R2-R3 is 10.0.23.0/24. R2 has three static routes
/// towards R1: 10.50.0.0/24 with tag 7, 10.60.0.0/24 with tag 8 and 10.70.0.0/24 with none.
fn line() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.0\nno shut\nend");
    lab.run(
        r2,
        "en\nconf t\nint g0/0\nip add 10.0.12.2 255.255.255.0\nno shut\nint g0/1\nip add 10.0.23.2 255.255.255.0\nno shut\nexit\n\
         ip route 10.50.0.0 255.255.255.0 10.0.12.1 tag 7\nip route 10.60.0.0 255.255.255.0 10.0.12.1 tag 8\n\
         ip route 10.70.0.0 255.255.255.0 10.0.12.1\nend",
    );
    lab.run(r3, "en\nconf t\nint g0/0\nip add 10.0.23.3 255.255.255.0\nno shut\nend");
    (lab, r1, r2, r3)
}

fn settle(lab: &mut Lab) {
    lab.sim.run_for(&lab.t, 60_000_000);
}

/// The line of a table that is about this network.
fn row(table: &str, net: &str) -> String {
    table.lines().find(|l| l.contains(&format!("{net} "))).unwrap_or("").to_string()
}

#[test]
fn a_static_route_carries_its_tag() {
    let (mut lab, _r1, r2, _r3) = line();
    let table = lab.run(r2, "show ip route");
    assert!(row(&table, "10.50.0.0/24").ends_with("tag 7"), "{table}");
    assert!(row(&table, "10.60.0.0/24").ends_with("tag 8"), "{table}");
    assert!(!row(&table, "10.70.0.0/24").contains("tag"), "{table}");
    let tagged = lab.run(r2, "show ip route tag 7");
    assert!(tagged.contains("10.50.0.0/24") && !tagged.contains("10.60.0.0/24"), "{tagged}");
    assert!(lab.run(r2, "show ip route tag 99").contains("No route carries tag 99"));
    assert!(lab.try_run(r2, "show ip route tag x").contains("The tag is 0-4294967295"));
    // The entry views name the tag.
    assert!(lab.run(r2, "show ip route 10.50.0.0 255.255.255.0").contains("Tag 7"));
    assert!(lab.run(r2, "show ip route 10.50.0.9").contains("Tag 7"));
    assert!(!lab.run(r2, "show ip route 10.70.0.9").contains("Tag"));
    // The running configuration writes it after the distance.
    let run = lab.run(r2, "show running-config");
    assert!(run.contains("ip route 10.50.0.0 255.255.255.0 10.0.12.1 tag 7\n"), "{run}");
    assert!(run.contains("ip route 10.70.0.0 255.255.255.0 10.0.12.1\n"), "{run}");
    lab.run(r2, "conf t\nip route 10.50.0.0 255.255.255.0 10.0.12.1 150 tag 9\nend");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains("ip route 10.50.0.0 255.255.255.0 10.0.12.1 150 tag 9\n") && !run.contains("tag 7\n"), "{run}");
    assert_eq!(run.matches("ip route 10.50.0.0").count(), 1, "{run}");
    // Typed again without a tag, it has none.
    lab.run(r2, "conf t\nip route 10.50.0.0 255.255.255.0 10.0.12.1\nend");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains("ip route 10.50.0.0 255.255.255.0 10.0.12.1\n") && !run.contains("ip route 10.50.0.0 255.255.255.0 10.0.12.1 tag"), "{run}");
    // A bad tag is refused; the `no` form takes the tag along.
    lab.run(r2, "conf t");
    assert!(lab.try_run(r2, "ip route 10.80.0.0 255.255.255.0 10.0.12.1 tag 0").contains("The tag is 1-4294967295"));
    assert!(lab.try_run(r2, "ip route 10.80.0.0 255.255.255.0 10.0.12.1 tag abc").contains("The tag is 1-4294967295"));
    assert!(lab.try_run(r2, "ip route 10.80.0.0 255.255.255.0 10.0.12.1 tag 4294967296").contains("The tag is 1-4294967295"));
    lab.run(r2, "no ip route 10.60.0.0 255.255.255.0 10.0.12.1 tag 8");
    assert!(!lab.run(r2, "end\nshow ip route").contains("10.60.0.0"));
}

#[test]
fn an_ipv6_static_route_carries_its_tag() {
    let (mut lab, _r1, r2, _r3) = line();
    lab.run(
        r2,
        "conf t\nipv6 unicast-routing\nint g0/0\nipv6 address 2001:db8:12::2/64\nexit\nipv6 route 2001:db8:50::/48 2001:db8:12::1 tag 12\nend",
    );
    let table = lab.run(r2, "show ipv6 route");
    assert!(row(&table, "2001:db8:50::/48").ends_with("tag 12"), "{table}");
    assert!(lab.run(r2, "show running-config").contains("ipv6 route 2001:db8:50::/48 2001:db8:12::1 tag 12\n"));
    // The same route typed again replaces it.
    lab.run(r2, "conf t\nipv6 route 2001:db8:50::/48 2001:db8:12::1 tag 13\nend");
    let run = lab.run(r2, "show running-config");
    assert_eq!(run.matches("ipv6 route 2001:db8:50::/48").count(), 1, "{run}");
    assert!(run.contains("tag 13\n"));
    lab.run(r2, "conf t");
    assert!(lab.try_run(r2, "ipv6 route 2001:db8:51::/48 2001:db8:12::1 tag 0").contains("The tag is 1-4294967295"));
}

#[test]
fn route_map_tag_lines_are_kept_shown_and_checked() {
    let (mut lab, _r1, r2, _r3) = line();
    lab.run(r2, "conf t\nroute-map T permit 10\nmatch tag 8 7 7\nset tag 77\nexit\nroute-map T deny 20\nend");
    let shown = lab.run(r2, "show route-map T");
    assert!(shown.contains("match tag: 7, 8") && shown.contains("set tag 77"), "{shown}");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains("route-map T permit 10\n match tag 7 8\n set tag 77\n!"), "{run}");
    // It survives a reload.
    lab.run(r2, "copy run start");
    lab.run(r2, "reload");
    assert_eq!(lab.run(r2, "en\nshow running-config"), run);
    lab.run(r2, "conf t\nroute-map T permit 10\nno match tag\nno set tag\nend");
    let shown = lab.run(r2, "show route-map T");
    assert!(!shown.contains("match tag") && !shown.contains("set tag") && shown.contains("no match lines"), "{shown}");
    lab.run(r2, "conf t\nroute-map T permit 10");
    assert!(lab.try_run(r2, "match tag x").contains("The tag is 0-4294967295"));
    assert!(lab.try_run(r2, "match tag 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17").contains("at most"));
    assert!(lab.try_run(r2, "set tag 4294967296").contains("The tag is 0-4294967295"));
}

#[test]
fn ospf_external_routes_carry_the_tag_in_their_lsa() {
    let (mut lab, _r1, r2, r3) = line();
    lab.run(r2, "conf t\nrouter ospf 1\nnetwork 10.0.23.0 0.0.0.255 area 0\nredistribute static subnets\nend");
    lab.run(r3, "conf t\nrouter ospf 1\nnetwork 10.0.23.0 0.0.0.255 area 0\nend");
    settle(&mut lab);
    // A static route's own tag goes into the LSA.
    let table = lab.run(r3, "show ip route");
    assert!(row(&table, "10.50.0.0/24").contains("O E2") && row(&table, "10.50.0.0/24").ends_with("tag 7"), "{table}");
    assert!(row(&table, "10.60.0.0/24").ends_with("tag 8"), "{table}");
    assert!(!row(&table, "10.70.0.0/24").contains("tag"), "{table}");
    let db = lab.run(r3, "show ip ospf database external 10.50.0.0");
    assert!(db.contains("type 2 metric 20, tag 7"), "{db}");
    assert!(lab.run(r3, "show ip route 10.50.0.9").contains("Tag 7"));
    assert!(lab.run(r3, "show ip route tag 8").contains("10.60.0.0/24"));
    // `redistribute … tag N` gives every route that tag.
    lab.run(r2, "conf t\nrouter ospf 1\nredistribute static metric 5 metric-type 1 tag 100 subnets\nend");
    settle(&mut lab);
    let table = lab.run(r3, "show ip route");
    for net in ["10.50.0.0/24", "10.60.0.0/24", "10.70.0.0/24"] {
        assert!(row(&table, net).ends_with("tag 100"), "{table}");
    }
    assert!(lab.run(r3, "show ip ospf database external 10.70.0.0").contains("tag 100"));
    assert!(lab.run(r2, "show running-config").contains(" redistribute static metric 5 metric-type 1 tag 100 subnets\n"));
    // A route map picks by tag and sets another; `set tag` comes before the line's `tag`.
    lab.run(
        r2,
        "conf t\nroute-map PICK permit 10\nmatch tag 7\nset tag 77\nexit\nrouter ospf 1\nredistribute static tag 5 subnets route-map PICK\nend",
    );
    settle(&mut lab);
    let table = lab.run(r3, "show ip route");
    assert!(row(&table, "10.50.0.0/24").ends_with("tag 77"), "{table}");
    assert!(row(&table, "10.60.0.0/24").is_empty() && row(&table, "10.70.0.0/24").is_empty(), "{table}");
    // Back to the routes' own tags.
    lab.run(r2, "conf t\nrouter ospf 1\nredistribute static subnets\nend");
    settle(&mut lab);
    assert!(row(&lab.run(r3, "show ip route"), "10.60.0.0/24").ends_with("tag 8"));
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "redistribute static tag x subnets").contains("The tag is 0-4294967295"));
}

#[test]
fn eigrp_external_routes_carry_the_tag() {
    let (mut lab, _r1, r2, r3) = line();
    lab.run(
        r2,
        "conf t\nroute-map PICK permit 10\nmatch tag 7\nset tag 70\nexit\nrouter eigrp 100\nnetwork 10.0.23.0 0.0.0.255\n\
         redistribute static metric 10000 100 255 1 1500 route-map PICK\nend",
    );
    lab.run(r3, "conf t\nrouter eigrp 100\nnetwork 10.0.23.0 0.0.0.255\nend");
    settle(&mut lab);
    let table = lab.run(r3, "show ip route");
    assert!(row(&table, "10.50.0.0/24").contains("D EX") && row(&table, "10.50.0.0/24").ends_with("tag 70"), "{table}");
    assert!(row(&table, "10.60.0.0/24").is_empty(), "{table}");
    // Without a map the routes keep the tags they had.
    lab.run(r2, "conf t\nrouter eigrp 100\nredistribute static metric 10000 100 255 1 1500\nend");
    settle(&mut lab);
    let table = lab.run(r3, "show ip route");
    assert!(row(&table, "10.50.0.0/24").ends_with("tag 7") && row(&table, "10.60.0.0/24").ends_with("tag 8"), "{table}");
    assert!(!row(&table, "10.70.0.0/24").contains("tag"), "{table}");
    assert!(lab.run(r3, "show ip route tag 8").contains("D EX"));
}

#[test]
fn rip_routes_carry_the_tag_in_the_route_tag_field() {
    let (mut lab, _r1, r2, r3) = line();
    lab.run(r2, "conf t\nip route 10.90.0.0 255.255.255.0 10.0.12.1 tag 70000\nroute-map PICK permit 10\nmatch tag 7\nset tag 5\nexit\nrouter rip\nversion 2\nnetwork 10.0.0.0\nredistribute static metric 2 route-map PICK\nend");
    lab.run(r3, "conf t\nrouter rip\nversion 2\nnetwork 10.0.0.0\nend");
    settle(&mut lab);
    let table = lab.run(r3, "show ip route");
    assert!(row(&table, "10.50.0.0/24").starts_with("R "), "{table}");
    assert!(row(&table, "10.50.0.0/24").ends_with("tag 5"), "{table}");
    let net: Ipv4Net = "10.50.0.0/24".parse().unwrap();
    assert_eq!(lab.sim.rip_state(&lab.t).tags.get(&(r3, net)), Some(&5));
    // Without a map the route keeps its tag; RIP's field is 16 bits, so a larger one is cut to them.
    lab.run(r2, "conf t\nrouter rip\nredistribute static metric 2\nend");
    settle(&mut lab);
    let table = lab.run(r3, "show ip route");
    assert!(row(&table, "10.50.0.0/24").ends_with("tag 7") && row(&table, "10.60.0.0/24").ends_with("tag 8"), "{table}");
    assert!(row(&table, "10.90.0.0/24").ends_with("tag 4464"), "{table}");
    assert!(!row(&table, "10.70.0.0/24").contains("tag"), "{table}");
}

#[test]
fn a_route_map_on_a_bgp_redistribution_gives_the_igp_routes_a_tag() {
    let (mut lab, r1, r2, r3) = line();
    // R1 and R2 speak BGP over 10.0.12.0/24; R2 takes R1's network into OSPF with a tag.
    lab.run(
        r1,
        "conf t\nint lo1\nip add 172.16.1.1 255.255.255.0\nexit\nrouter bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\n\
         network 172.16.1.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "conf t\nroute-map TAGGED permit 10\nset tag 4242\nexit\nrouter bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nexit\n\
         router ospf 1\nnetwork 10.0.23.0 0.0.0.255 area 0\nredistribute bgp 65002 subnets route-map TAGGED\nend",
    );
    lab.run(r3, "conf t\nrouter ospf 1\nnetwork 10.0.23.0 0.0.0.255 area 0\nend");
    lab.sim.run_for(&lab.t, 90_000_000);
    let table = lab.run(r3, "show ip route");
    assert!(row(&table, "172.16.1.0/24").ends_with("tag 4242"), "{table}");
}
