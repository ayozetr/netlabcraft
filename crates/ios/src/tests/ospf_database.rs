//! `show ip ospf database router|network|summary|external` in full (M9.7).

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 -- R2 -- R3: R1 and R2 share area 0 on 10.0.0.0/24, R2 and R3 area 1 on 10.1.0.0/24; R3 has a
/// static route it redistributes.
fn three_routers() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    lab.run(
        r1,
        "en\nconf t\nhostname R1\ninterface g0/0\nip address 10.0.0.1 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 1.1.1.1\nnetwork 10.0.0.0 0.0.0.255 area 0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nhostname R2\ninterface g0/0\nip address 10.0.0.2 255.255.255.0\nno shutdown\nexit\ninterface g0/1\nip address 10.1.0.2 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 2.2.2.2\nnetwork 10.0.0.0 0.0.0.255 area 0\nnetwork 10.1.0.0 0.0.0.255 area 1\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nhostname R3\ninterface g0/0\nip address 10.1.0.3 255.255.255.0\nno shutdown\nexit\nip route 192.168.50.0 255.255.255.0 10.1.0.99\nrouter ospf 1\nrouter-id 3.3.3.3\nnetwork 10.1.0.0 0.0.0.255 area 1\nredistribute static subnets\nend",
    );
    lab.sim.run_for(&lab.t, 60_000_000);
    (lab, r1, r2, r3)
}

#[test]
fn database_views_show_router_network_summary_and_external_lsas_in_full() {
    let (mut lab, r1, r2, _r3) = three_routers();
    let router = lab.run(r2, "show ip ospf database router");
    assert!(router.contains("Router link states, area 0") && router.contains("Router link states, area 1"), "{router}");
    // R2 sits on two broadcast segments, each with a neighbour: a transit link per segment.
    assert!(router.contains("Router 2.2.2.2, sequence 0x80000001, 1 link"), "{router}");
    assert!(router.contains("transit network, designated router 10.0.0.2 (this router 10.0.0.2), metric 1"), "{router}");
    assert!(lab.run(r2, "show ip ospf database router 1.1.1.1").contains("Router 1.1.1.1"));
    assert!(!lab.run(r2, "show ip ospf database router 1.1.1.1").contains("Router 2.2.2.2"));
    let network = lab.run(r2, "show ip ospf database network");
    assert!(
        network.contains("Network 10.0.0.0/24, designated router 10.0.0.2") && network.contains("attached routers: 1.1.1.1, 2.2.2.2"),
        "{network}"
    );
    // R1 is only in area 0: it learns area 1's network by a summary LSA from the border router.
    let summary = lab.run(r1, "show ip ospf database summary");
    assert!(summary.contains("Summary link states, area 0"), "{summary}");
    assert!(summary.contains("Network 10.1.0.0/24, sequence 0x80000001, advertised by 2.2.2.2, metric 1"), "{summary}");
    let external = lab.run(r1, "show ip ospf database external");
    assert!(external.contains("Network 192.168.50.0/24") && external.contains("advertised by 3.3.3.3, type 2 metric 20"), "{external}");
    // Prefixes of the kind word, an unknown kind, a bad ID and nothing there.
    assert!(lab.run(r1, "show ip ospf database ext").contains("192.168.50.0"));
    assert!(lab.try_run(r1, "show ip ospf database bogus").starts_with("% "));
    assert!(lab.try_run(r1, "show ip ospf database router 1.2.3").starts_with("% "));
    assert!(lab.try_run(r1, "show ip ospf database router 9.9.9.9").starts_with("No router LSAs"));
    // The one-line-per-LSA table is still what the bare command prints.
    assert!(lab.run(r1, "show ip ospf database").contains("Link ID"));
}

#[test]
fn database_router_view_lists_point_to_point_and_stub_links() {
    let (mut lab, r1, r2, _) = three_routers();
    lab.run(r1, "conf t\ninterface g0/0\nip ospf network point-to-point\nend");
    lab.run(r2, "conf t\ninterface g0/0\nip ospf network point-to-point\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    let router = lab.run(r1, "show ip ospf database router");
    assert!(router.contains("point-to-point to router 2.2.2.2 through 10.0.0.1, metric 1"), "{router}");
    assert!(router.contains("stub network 10.0.0.0/24, metric 1"), "{router}");
    assert!(router.contains("2 links"), "{router}");
}

#[test]
fn a_router_cut_off_by_a_missing_adjacency_leaves_the_database() {
    let (mut lab, r1, r2, _r3) = three_routers();
    assert!(lab.run(r1, "show ip ospf database summary").contains("Network 10.1.0.0/24"));
    assert!(lab.run(r1, "show ip ospf database external").contains("192.168.50.0"));
    assert!(lab.run(r2, "show ip ospf database router").contains("Router 3.3.3.3"));
    // R2's link to R3 goes passive: R3 stops being a neighbour and its advertisements age out.
    lab.run(r2, "conf t\nrouter ospf 1\npassive-interface g0/1\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    assert!(!lab.run(r2, "show ip ospf database router").contains("Router 3.3.3.3"));
    // R2 still advertises the passive link's own network; nothing else of area 1 is left.
    let summary = lab.run(r1, "show ip ospf database summary");
    assert!(summary.contains("Network 10.1.0.0/24") && !summary.contains("192.168"), "{summary}");
    assert!(lab.run(r1, "show ip ospf database external").starts_with("No external LSAs"));
    assert!(!lab.run(r1, "show ip ospf database").contains("192.168.50.0"));
}
