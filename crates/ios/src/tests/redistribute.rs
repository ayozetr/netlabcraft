//! `redistribute`, `default-metric` and `distribute-list` under RIP, EIGRP and OSPF, typed at
//! the console: where the routes come out in the routing tables, the metrics they get and the
//! filters that keep some of them out.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 (RIP) - R2 (RIP and EIGRP, redistributing both ways) - R3 (EIGRP).
///
/// R1 owns 172.16.1.0/24 and 172.16.2.0/24; R3 owns 192.168.33.0/24; R1-R2 is 10.0.12.0/24 and
/// R2-R3 is 192.168.23.0/24.
fn chain() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.0\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nint lo2\nip add 172.16.2.1 255.255.255.0\n\
         exit\nrouter rip\nversion 2\nnetwork 10.0.0.0\nnetwork 172.16.0.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/0\nip add 10.0.12.2 255.255.255.0\nno shut\nint g0/1\nip add 192.168.23.2 255.255.255.0\nno shut\nexit\n\
         router rip\nversion 2\nnetwork 10.0.0.0\nexit\nrouter eigrp 100\nnetwork 192.168.23.0 0.0.0.255\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/0\nip add 192.168.23.3 255.255.255.0\nno shut\nint lo1\nip add 192.168.33.1 255.255.255.0\nexit\n\
         router eigrp 100\nnetwork 192.168.0.0 0.0.255.255\nend",
    );
    (lab, r1, r2, r3)
}

fn settle(lab: &mut Lab) {
    lab.sim.run_for(&lab.t, 60_000_000);
}

#[test]
fn nothing_crosses_between_protocols_until_it_is_redistributed() {
    let (mut lab, r1, _r2, r3) = chain();
    settle(&mut lab);
    assert!(!lab.run(r3, "show ip route").contains("172.16.1.0"), "RIP routes stay in RIP");
    assert!(!lab.run(r1, "show ip route").contains("192.168.33.0"), "EIGRP routes stay in EIGRP");
}

#[test]
fn rip_and_eigrp_exchange_routes_both_ways() {
    let (mut lab, r1, r2, r3) = chain();
    // Without a metric the routes of another protocol are not advertised, and the console says so.
    let note = lab.run(r2, "conf t\nrouter eigrp 100\nredistribute rip");
    assert!(note.contains("needs a metric"), "{note}");
    settle(&mut lab);
    assert!(!lab.run(r3, "show ip route").contains("172.16.1.0"));
    // With one they arrive as external EIGRP routes (distance 170).
    lab.run(r2, "redistribute rip metric 10000 100 255 1 1500\nredistribute connected metric 10000 100 255 1 1500\nexit");
    lab.run(r2, "router rip\nredistribute eigrp 100 metric 3\nend");
    settle(&mut lab);
    let r3_routes = lab.run(r3, "show ip route");
    assert!(r3_routes.contains("D EX 172.16.1.0/24") && r3_routes.contains("D EX 172.16.2.0/24"), "{r3_routes}");
    assert!(r3_routes.contains("D EX 10.0.12.0/24"), "connected redistributed too: {r3_routes}");
    assert!(r3_routes.contains("[170/"), "{r3_routes}");
    // RIP gets them with the metric it was told: 3 hops.
    let r1_routes = lab.run(r1, "show ip route");
    assert!(r1_routes.contains("R    192.168.33.0/24") && r1_routes.contains("[120/3]"), "{r1_routes}");
    // The data plane follows the tables, there and back.
    let ping = lab.run(r1, "ping 192.168.33.1");
    assert!(ping.contains("!!"), "{ping}");
    // A route a protocol took from the other is not handed back: R2 keeps its own RIP route.
    let r2_routes = lab.run(r2, "show ip route");
    assert!(r2_routes.contains("R    172.16.1.0/24") && !r2_routes.contains("D EX 172.16.1.0"), "{r2_routes}");
    let proto = lab.run(r2, "show ip protocols");
    assert!(proto.contains("Redistributing: connected, rip") && proto.contains("Redistributing: eigrp 100"), "{proto}");
}

#[test]
fn default_metric_is_used_by_lines_that_name_none() {
    let (mut lab, r1, r2, _r3) = chain();
    lab.run(r2, "conf t\nrouter rip\nredistribute eigrp 100\nend");
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip route").contains("192.168.33.0"), "no metric, no route");
    lab.run(r2, "conf t\nrouter rip\ndefault-metric 5\nend");
    settle(&mut lab);
    let routes = lab.run(r1, "show ip route");
    assert!(routes.contains("192.168.33.0/24") && routes.contains("[120/5]"), "{routes}");
    assert!(lab.run(r2, "show ip protocols").contains("Default redistribution metric: 5"));
    // A metric on the line wins over the default; a metric of 16 is unreachable.
    lab.run(r2, "conf t\nrouter rip\nredistribute eigrp 100 metric 16\nend");
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip route").contains("192.168.33.0"));
}

#[test]
fn a_route_map_chooses_and_changes_what_is_redistributed() {
    let (mut lab, _r1, r2, r3) = chain();
    lab.run(
        r2,
        "conf t\nip prefix-list ONE permit 172.16.1.0/24\nroute-map PICK permit 10\nmatch ip address prefix-list ONE\nexit\n\
         router eigrp 100\nredistribute rip metric 10000 100 255 1 1500 route-map PICK\nend",
    );
    settle(&mut lab);
    let routes = lab.run(r3, "show ip route");
    assert!(routes.contains("D EX 172.16.1.0/24") && !routes.contains("172.16.2.0"), "{routes}");
    assert!(lab.run(r2, "show running-config").contains(" redistribute rip metric 10000 100 255 1 1500 route-map PICK"));
}

#[test]
fn distribute_lists_filter_what_is_installed_and_what_is_advertised() {
    let (mut lab, r1, r2, r3) = chain();
    lab.run(r2, "conf t\nrouter eigrp 100\nredistribute rip metric 10000 100 255 1 1500\nexit\nrouter rip\nredistribute eigrp 100 metric 2\nend");
    settle(&mut lab);
    assert!(lab.run(r1, "show ip route").contains("192.168.33.0/24"));
    // Out under RIP on R2, for the interface towards R1: the EIGRP network is kept out of its updates.
    lab.run(
        r2,
        "conf t\nip prefix-list NO33 deny 192.168.33.0/24\nip prefix-list NO33 permit 0.0.0.0/0 le 32\nrouter rip\ndistribute-list prefix NO33 out g0/0\nend",
    );
    settle(&mut lab);
    let routes = lab.run(r1, "show ip route");
    assert!(!routes.contains("192.168.33.0"), "{routes}");
    assert!(routes.contains("172.16.1.0/24"), "other routes are untouched: {routes}");
    // In under EIGRP on R3: a standard access list keeps 172.16.2.0 out of its table.
    lab.run(r3, "conf t\naccess-list 7 deny 172.16.2.0 0.0.0.255\naccess-list 7 permit any\nrouter eigrp 100\ndistribute-list 7 in\nend");
    settle(&mut lab);
    let routes = lab.run(r3, "show ip route");
    assert!(routes.contains("D EX 172.16.1.0/24") && !routes.contains("172.16.2.0"), "{routes}");
    let proto = lab.run(r3, "show ip protocols");
    assert!(proto.contains("Incoming route filter: access list 7 on every interface"), "{proto}");
    let run = lab.run(r3, "show running-config");
    assert!(run.contains(" distribute-list 7 in\n"), "{run}");
    let run2 = lab.run(r2, "show running-config");
    assert!(run2.contains(" distribute-list prefix NO33 out GigabitEthernet0/0\n"), "{run2}");
    // A list replaces the one for the same direction and interface, and `no` removes it.
    lab.run(r3, "conf t\nrouter eigrp 100\nno distribute-list 7 in\nend");
    settle(&mut lab);
    assert!(lab.run(r3, "show ip route").contains("172.16.2.0/24"));
}

#[test]
fn ospf_external_routes_take_their_metric_and_type_from_the_line_and_the_map() {
    let (mut lab, _r1, r2, r3) = chain();
    // R2 and R3 run OSPF instead of EIGRP on their link.
    lab.run(r2, "conf t\nno router eigrp 100\nrouter ospf 1\nrouter-id 2.2.2.2\nnetwork 192.168.23.0 0.0.0.255 area 0\nend");
    lab.run(r3, "conf t\nno router eigrp 100\nrouter ospf 1\nrouter-id 3.3.3.3\nnetwork 192.168.23.0 0.0.0.255 area 0\nend");
    lab.run(r2, "conf t\nrouter ospf 1\nredistribute rip metric 7 metric-type 1 subnets\nend");
    settle(&mut lab);
    let routes = lab.run(r3, "show ip route");
    assert!(routes.contains("O E1 172.16.1.0/24") && routes.contains("[110/8]"), "type 1 adds the way to R2: {routes}");
    // A route map changes the metric and the type of what it lets through.
    lab.run(
        r2,
        "conf t\nroute-map EXT permit 10\nset metric 40\nset metric-type type-2\nexit\nrouter ospf 1\nredistribute rip route-map EXT subnets\nend",
    );
    settle(&mut lab);
    let routes = lab.run(r3, "show ip route");
    assert!(routes.contains("O E2 172.16.1.0/24") && routes.contains("[110/40]"), "{routes}");
    let db = lab.run(r3, "show ip ospf database external");
    assert!(db.contains("172.16.1.0"), "{db}");
    assert!(lab.run(r2, "show route-map EXT").contains("metric-type type-2"));
    // An OSPF distribute list keeps routes out of the table.
    lab.run(
        r3,
        "conf t\nip prefix-list NO1 deny 172.16.1.0/24\nip prefix-list NO1 permit 0.0.0.0/0 le 32\nrouter ospf 1\ndistribute-list prefix NO1 in\nend",
    );
    settle(&mut lab);
    let routes = lab.run(r3, "show ip route");
    assert!(!routes.contains("172.16.1.0") && routes.contains("172.16.2.0/24"), "{routes}");
}

#[test]
fn bad_lines_are_refused_and_the_old_static_form_still_reads() {
    let (mut lab, _r1, r2, _r3) = chain();
    lab.run(r2, "conf t\nrouter ospf 1");
    for bad in [
        "redistribute",
        "redistribute ospf 1",
        "redistribute bgp",
        "redistribute static metric",
        "redistribute static metric-type 3",
        "redistribute static route-map",
        "redistribute static nonsense",
        "distribute-list prefix",
        "distribute-list prefix X sideways",
        "no redistribute connected",
    ] {
        assert!(lab.try_run(r2, bad).starts_with('%') || lab.try_run(r2, bad).contains("Incomplete"), "`{bad}` should be refused");
    }
    lab.run(r2, "redistribute static subnets route-map M\nredistribute connected metric 9 metric-type 1 subnets");
    let run = lab.run(r2, "end\nshow running-config");
    assert!(run.contains(" redistribute connected metric 9 metric-type 1 subnets\n redistribute static subnets route-map M\n"), "{run}");
    lab.run(r2, "conf t\nrouter ospf 1\nno redistribute static\nno redistribute connected");
    assert!(!lab.run(r2, "end\nshow running-config").contains("redistribute"));
}
