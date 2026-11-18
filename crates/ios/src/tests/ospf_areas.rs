//! OSPF area types (stub, NSSA, virtual links) and the sequence numbers and ages of LSAs.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 -- R2 -- R3: R1 and R2 share area 0 on 10.0.0.0/24, R2 and R3 area 1 on 10.1.0.0/24; R3 has a
/// static route 192.168.50.0/24 that it redistributes.
fn chain() -> (Lab, DeviceId, DeviceId, DeviceId) {
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
fn a_stub_area_gets_a_default_with_the_configured_cost_and_no_type_5_lsas() {
    let (mut lab, _r1, r2, r3) = chain();
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 stub\narea 1 default-cost 7\nend");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 stub\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(rt.contains("O*IA 0.0.0.0/0") && rt.contains("[110/8]"), "{rt}");
    assert!(rt.contains("O IA 10.0.0.0/24"), "{rt}");
    // The default is a summary LSA from the border router with the configured metric.
    let summary = lab.run(r3, "show ip ospf database summary");
    assert!(summary.contains("Network 0.0.0.0/0") && summary.contains("advertised by 2.2.2.2, metric 7"), "{summary}");
    // No type 5 (and no type 4) LSAs reach a router wholly inside the stub area.
    assert!(lab.run(r3, "show ip ospf database external").starts_with("No external LSAs"));
    assert!(lab.run(r3, "show ip ospf database asbr-summary").starts_with("No ASBR summary LSAs"));
    // The border router keeps them for its backbone side.
    assert!(lab.run(r2, "show ip ospf database external").contains("192.168.50.0"));
    let run = lab.run(r2, "show running-config");
    assert!(run.contains(" area 1 stub\n") && run.contains(" area 1 default-cost 7"), "{run}");
    let ospf = lab.run(r2, "show ip ospf");
    assert!(ospf.contains("Area 1: stub area") && ospf.contains("default route cost 7"), "{ospf}");
    // Totally stubby: the summaries go too, the default stays.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 stub no-summary\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(!rt.contains("10.0.0.0/24") && rt.contains("O*IA 0.0.0.0/0"), "{rt}");
    let summary = lab.run(r3, "show ip ospf database summary");
    assert!(summary.contains("Network 0.0.0.0/0") && !summary.contains("10.0.0.0"), "{summary}");
    // The default cost needs a stub area or an NSSA, and goes with it.
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "area 5 default-cost 3").starts_with("% Area 5 is not a stub area or an NSSA"));
    assert!(lab.try_run(r2, "area 1 default-cost 16777216").starts_with("% The cost is"));
    lab.run(r2, "no area 1 stub\nend");
    assert!(!lab.run(r2, "show running-config").contains("default-cost"));
}

#[test]
fn different_area_types_on_the_two_ends_prevent_the_adjacency_and_say_why() {
    let (mut lab, _r1, r2, r3) = chain();
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 nssa\nend");
    let n = lab.run(r2, "show ip ospf neighbor");
    assert!(n.contains("Not a neighbour: 10.1.0.3 on GigabitEthernet0/1"), "{n}");
    assert!(n.contains("the area is a NSSA here and a normal area there") && n.contains("E bit 0, N bit 1 here; E bit 1, N bit 0 there"), "{n}");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 stub\nend");
    let n = lab.run(r3, "show ip ospf neighbor");
    assert!(n.contains("the area is a stub area here and a NSSA there") && n.contains("E bit 0, N bit 0 here; E bit 0, N bit 1 there"), "{n}");
    // Both NSSA: neighbours again.
    lab.run(r3, "conf t\nrouter ospf 1\nno area 1 stub\narea 1 nssa\nend");
    assert!(lab.run(r3, "show ip ospf neighbor").contains("2.2.2.2"));
}

#[test]
fn a_hello_with_other_area_options_is_reported_by_debug() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    for (r, n, kind) in [(r1, 1, "stub"), (r2, 2, "nssa")] {
        lab.run(
            r,
            &format!("en\nconf t\ninterface g0/0\nip address 10.0.0.{n} 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id {n}.{n}.{n}.{n}\nnetwork 10.0.0.0 0.0.0.255 area 1\narea 1 {kind}\nend"),
        );
    }
    lab.run(r1, "debug ip ospf adj");
    lab.sim.run_for(&lab.t, 30_000_000);
    let log = lab.run(r1, "show logging");
    assert!(log.contains("OSPF adjacency: 2.2.2.2 on GigabitEthernet0/0 stays Down: the area is a stub area here and a NSSA there"), "{log}");
}

#[test]
fn an_nssa_originates_type_7_lsas_and_its_border_router_translates_them() {
    let (mut lab, r1, r2, r3) = chain();
    // Before: an ordinary external route.
    assert!(lab.run(r1, "show ip route").contains("O E2 192.168.50.0/24"));
    assert!(lab.run(r2, "show ip route").contains("O E2 192.168.50.0/24"));
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 nssa\nend");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 nssa\nend");
    // R3's redistributed route is a type 7 LSA in area 1 ...
    let t7 = lab.run(r3, "show ip ospf database nssa-external");
    assert!(t7.contains("Type 7 external link states, area 1"), "{t7}");
    assert!(t7.contains("Network 192.168.50.0/24") && t7.contains("advertised by 3.3.3.3, type 2 metric 20"), "{t7}");
    assert!(t7.contains("2.2.2.2 translates it into a type 5 LSA"), "{t7}");
    // ... that R2, the translator, sends on as a type 5 LSA of its own.
    let t5 = lab.run(r1, "show ip ospf database external");
    assert!(t5.contains("Network 192.168.50.0/24") && t5.contains("advertised by 2.2.2.2, type 2 metric 20"), "{t5}");
    assert!(lab.run(r1, "show ip ospf database nssa-external").starts_with("No NSSA external LSAs"));
    // Routes: N2 inside the area (and on the border router), E2 outside.
    assert!(lab.run(r2, "show ip route").contains("O N2 192.168.50.0/24"));
    assert!(lab.run(r1, "show ip route").contains("O E2 192.168.50.0/24"));
    assert!(lab.run(r1, "show ip route ospf").contains("O E2 192.168.50.0/24"));
    let ospf = lab.run(r2, "show ip ospf");
    assert!(ospf.contains("Area 1: NSSA") && ospf.contains("translator: this router"), "{ospf}");
    assert!(lab.run(r3, "show ip ospf").contains("translator: router 2.2.2.2"));
    // A router in the NSSA hears no type 5 LSAs.
    assert!(lab.run(r3, "show ip ospf database external").starts_with("No external LSAs"));
    // The border router can send a default route into the NSSA.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 nssa default-information-originate\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(rt.contains("O*N2 0.0.0.0/0") && rt.contains("O IA 10.0.0.0/24"), "{rt}");
    let t7 = lab.run(r3, "show ip ospf database nssa-external 0.0.0.0");
    assert!(t7.contains("Network 0.0.0.0/0") && t7.contains("advertised by 2.2.2.2, type 2 metric 1") && t7.contains("not translated"), "{t7}");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains(" area 1 nssa default-information-originate"), "{run}");
    // no-summary replaces it with a type 3 default and keeps the other areas' routes out.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 nssa no-summary\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(rt.contains("O*IA 0.0.0.0/0") && !rt.contains("O*N2 0.0.0.0") && !rt.contains("10.0.0.0/24"), "{rt}");
    assert!(lab.run(r2, "show running-config").contains(" area 1 nssa no-summary\n"));
    // The two kinds of area exclude each other, and the options are checked.
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "area 1 stub").contains("is an NSSA"));
    assert!(lab.try_run(r2, "area 1 nssa bogus").contains("not an NSSA option"));
    assert!(lab.try_run(r2, "area 0 nssa").starts_with("% The backbone"));
    // Undo one option, then the area.
    lab.run(r2, "no area 1 nssa no-summary\nend");
    assert!(lab.run(r2, "show running-config").contains(" area 1 nssa\n"));
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 nssa\nend");
    assert!(!lab.run(r2, "show running-config").contains("nssa"));
}

#[test]
fn the_running_config_lines_of_the_area_types_are_accepted_again() {
    let (mut lab, _r1, r2, _r3) = chain();
    lab.run(
        r2,
        "conf t\nrouter ospf 1\narea 1 nssa no-summary default-information-originate\narea 1 default-cost 4\narea 1 virtual-link 9.9.9.9\nend",
    );
    let run = lab.run(r2, "show running-config");
    let lines: Vec<&str> = run.lines().filter(|l| l.starts_with(" area 1 ")).collect();
    assert_eq!(lines.len(), 3, "{run}");
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 nssa\nno area 1 virtual-link 9.9.9.9\nend");
    assert!(!lab.run(r2, "show running-config").contains(" area 1 "));
    for l in lines {
        lab.run(r2, &format!("conf t\nrouter ospf 1\n{}\nend", l.trim()));
    }
    assert_eq!(lab.run(r2, "show running-config"), run);
}

/// R1 -- R2 -- R3 -- R4 with a PC behind R1 and R4: area 0 on R1-R2, area 1 on R2-R3 and area 2 on
/// R3-R4, so area 2 does not touch the backbone.
fn four_routers() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r: Vec<DeviceId> = (0..4).map(|_| lab.add(DeviceKind::Router)).collect();
    let (r1, r2, r3, r4) = (r[0], r[1], r[2], r[3]);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    lab.cable(r3, "g0/1", r4, "g0/0");
    for (router, port) in [(r1, "g0/1"), (r4, "g0/1")] {
        let pc = lab.add(DeviceKind::Pc);
        lab.cable(pc, "fa0", router, port);
        let (ip, gw) = if router == r1 { ("192.168.1.10", "192.168.1.1") } else { ("192.168.4.10", "192.168.4.1") };
        lab.run(pc, &format!("ipconfig {ip} 255.255.255.0 {gw}"));
    }
    lab.run(r1, "en\nconf t\nhostname R1\ninterface g0/0\nip address 10.0.12.1 255.255.255.0\nno shutdown\ninterface g0/1\nip address 192.168.1.1 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 1.1.1.1\nnetwork 10.0.12.0 0.0.0.255 area 0\nnetwork 192.168.1.0 0.0.0.255 area 0\nend");
    lab.run(r2, "en\nconf t\nhostname R2\ninterface g0/0\nip address 10.0.12.2 255.255.255.0\nno shutdown\ninterface g0/1\nip address 10.0.23.2 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 2.2.2.2\nnetwork 10.0.12.0 0.0.0.255 area 0\nnetwork 10.0.23.0 0.0.0.255 area 1\nend");
    lab.run(r3, "en\nconf t\nhostname R3\ninterface g0/0\nip address 10.0.23.3 255.255.255.0\nno shutdown\ninterface g0/1\nip address 10.0.34.3 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 3.3.3.3\nnetwork 10.0.23.0 0.0.0.255 area 1\nnetwork 10.0.34.0 0.0.0.255 area 2\nend");
    lab.run(r4, "en\nconf t\nhostname R4\ninterface g0/0\nip address 10.0.34.4 255.255.255.0\nno shutdown\ninterface g0/1\nip address 192.168.4.1 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 4.4.4.4\nnetwork 10.0.34.0 0.0.0.255 area 2\nnetwork 192.168.4.0 0.0.0.255 area 2\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    (lab, r1, r2, r3, r4)
}

#[test]
fn an_area_cut_off_from_the_backbone_is_joined_by_a_virtual_link() {
    let (mut lab, r1, r2, r3, r4) = four_routers();
    // Area 2 does not touch area 0: nobody outside it knows its networks, and it knows nothing of the rest.
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O IA 10.0.23.0/24") && !rt.contains("192.168.4.0") && !rt.contains("10.0.34.0"), "{rt}");
    let rt = lab.run(r4, "show ip route");
    assert!(!rt.contains("192.168.1.0") && !rt.contains("10.0.12.0"), "{rt}");
    assert!(lab.run(r3, "show ip route").contains("O IA 192.168.1.0/24"));
    assert!(lab.run(r1, "show ip ospf virtual-links").starts_with("No virtual links"));
    // One end only: the link is down and says what is missing.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 virtual-link 3.3.3.3\nend");
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("to router 3.3.3.3") && vl.contains("is down") && vl.contains("has no `area 1 virtual-link 2.2.2.2` line"), "{vl}");
    // Both ends: up across area 1.
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2\nend");
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("to router 3.3.3.3") && vl.contains("is up") && vl.contains("Transit area 1, cost 1 across it"), "{vl}");
    assert!(lab.run(r3, "show ip ospf virtual-links").contains("to router 2.2.2.2"));
    assert!(lab.run(r3, "show ip ospf").contains("1 virtual link configured, 1 up"));
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O IA 192.168.4.0/24") && rt.contains("O IA 10.0.34.0/24"), "{rt}");
    assert!(lab.run(r4, "show ip route").contains("O IA 192.168.1.0/24"));
    assert!(lab.run(r2, "show running-config").contains(" area 1 virtual-link 3.3.3.3"));
    lab.run(r1, "ping 192.168.4.10");
    assert!(lab.run(r1, "ping 192.168.4.10").contains("!!!!!"));
    // The transit area's link goes down: no path across it any more, and area 2 is cut off again.
    lab.run(r3, "conf t\ninterface g0/0\nshutdown\nend");
    let vl = lab.run(r3, "show ip ospf virtual-links");
    assert!(vl.contains("is down") && vl.contains("no OSPF interface in transit area 1"), "{vl}");
    assert!(!lab.run(r1, "show ip route").contains("192.168.4.0"));
    lab.run(r3, "conf t\ninterface g0/0\nno shutdown\nend");
    // A virtual link goes through a normal area, never the backbone, a stub area or an NSSA.
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "area 0 virtual-link 3.3.3.3").starts_with("% The backbone"));
    assert!(lab.try_run(r2, "area 1 virtual-link x").starts_with("% "));
    lab.run(r2, "no area 1 virtual-link 3.3.3.3\nend");
    assert!(lab.run(r2, "show ip ospf virtual-links").starts_with("No virtual links"));
    assert!(lab.run(r3, "show ip ospf virtual-links").contains("is down"));
}

/// The sequence number in the first LSA the output lists, from `sequence 0x…`.
fn seq_of(text: &str) -> u32 {
    let at = text.find("sequence 0x").unwrap_or_else(|| panic!("no sequence number in:\n{text}")) + "sequence 0x".len();
    u32::from_str_radix(text.get(at..at + 8).unwrap_or(""), 16).unwrap_or_else(|_| panic!("bad sequence number in:\n{text}"))
}

/// The age in seconds of the first LSA the output lists, from `age N s`.
fn age_of(text: &str) -> u64 {
    let at = text.find("age ").unwrap_or_else(|| panic!("no age in:\n{text}")) + "age ".len();
    text.get(at..).and_then(|t| t.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("bad age in:\n{text}"))
}

#[test]
fn an_lsa_gets_a_new_sequence_number_when_its_router_originates_it_again() {
    let (mut lab, r1, r2, _r3) = chain();
    let first = lab.run(r1, "show ip ospf database router self-originate");
    let seq0 = seq_of(&first);
    assert!(seq0 >= 0x8000_0001 && first.contains("Router 1.1.1.1"), "{first}");
    // Nothing changes: the number stays and the age counts simulated time.
    let age0 = age_of(&first);
    lab.sim.run_for(&lab.t, 120_000_000);
    let later = lab.run(r1, "show ip ospf database router self-originate");
    assert_eq!(seq_of(&later), seq0, "{later}");
    assert!((119..=121).contains(&(age_of(&later) - age0)), "{first}\n{later}");
    // The database table shows both.
    let table = lab.run(r1, "show ip ospf database");
    assert!(table.contains("Age") && table.contains("Seq#") && table.contains("0x8000000"), "{table}");
    // R1's only link goes down and comes back: its router LSA is gone and then originated again, with the next number.
    lab.run(r1, "conf t\ninterface g0/0\nshutdown\nend");
    lab.sim.run_for(&lab.t, 30_000_000);
    lab.run(r1, "conf t\ninterface g0/0\nno shutdown\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    let flapped = lab.run(r1, "show ip ospf database router self-originate");
    assert!(seq_of(&flapped) > seq0, "{later}\n{flapped}");
    assert!(age_of(&flapped) <= 60, "{flapped}");
    // The neighbour's copy carries the same number.
    let theirs = lab.run(r2, "show ip ospf database router 1.1.1.1");
    assert_eq!(seq_of(&theirs), seq_of(&flapped), "{theirs}");
    // An LSA is refreshed every 30 minutes with the next number and its age starts over; it never reaches MaxAge.
    let before = seq_of(&flapped);
    lab.sim.run_for(&lab.t, 1_900_000_000);
    let refreshed = lab.run(r1, "show ip ospf database router self-originate");
    assert_eq!(seq_of(&refreshed), before + 1, "{refreshed}");
    assert!(age_of(&refreshed) < 1800, "{refreshed}");
    // A change of configuration that changes the LSA is another origination.
    lab.run(r1, "conf t\ninterface g0/0\nip ospf cost 5\nend");
    lab.sim.run_for(&lab.t, 10_000_000);
    let costed = lab.run(r1, "show ip ospf database router self-originate");
    assert_eq!(seq_of(&costed), before + 2, "{costed}");
    assert!(age_of(&costed) <= 10, "{costed}");
    // `clear ip ospf process` starts the numbers over.
    lab.run(r1, "clear ip ospf process");
    lab.sim.run_for(&lab.t, 60_000_000);
    assert_eq!(seq_of(&lab.run(r1, "show ip ospf database router self-originate")), 0x8000_0001);
}

#[test]
fn an_nssa_border_router_with_no_redistribution_keeps_its_own_routes_out_of_the_area() {
    let (mut lab, r1, r2, r3) = chain();
    lab.run(r2, "conf t\nip route 172.16.0.0 255.255.0.0 10.0.0.99\nrouter ospf 1\nredistribute static subnets\narea 1 nssa\nend");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 nssa\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(rt.contains("O N2 172.16.0.0/16"), "{rt}");
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 nssa no-redistribution\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(!rt.contains("172.16.0.0"), "{rt}");
    // The rest of the network still hears it, as an ordinary external route.
    assert!(lab.run(r1, "show ip route").contains("O E2 172.16.0.0/16"));
    let t7 = lab.run(r3, "show ip ospf database nssa-external");
    assert!(!t7.contains("172.16.0.0") && t7.contains("192.168.50.0"), "{t7}");
    assert!(lab.run(r2, "show ip ospf").contains("no-redistribution"));
    assert!(lab.run(r2, "show running-config").contains(" area 1 nssa no-redistribution\n"));
    // The options combine, are checked, and are undone one at a time.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 nssa no-summary no-redistribution default-information-originate");
    assert!(lab.try_run(r2, "area 1 nssa no-r bogus").contains("not an NSSA option"));
    lab.run(r2, "no area 1 nssa no-redistribution\nend");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains(" area 1 nssa no-summary default-information-originate\n"), "{run}");
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 nssa\nend");
    assert!(!lab.run(r2, "show running-config").contains("no-redistribution"));
}

#[test]
fn a_virtual_link_has_timers_that_must_agree_and_a_command_line_that_round_trips() {
    let (mut lab, r1, r2, r3, _r4) = four_routers();
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 virtual-link 3.3.3.3 hello-interval 5\nend");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2\nend");
    // The two ends disagree on the hello interval (and so on the dead one, four times it).
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("is down") && vl.contains("the hello interval differs (5 s here, 10 s at the far end)"), "{vl}");
    assert!(!lab.run(r1, "show ip route").contains("192.168.4.0"));
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2 hello-interval 5 retransmit-interval 8 transmit-delay 2\nend");
    let vl = lab.run(r3, "show ip ospf virtual-links");
    assert!(vl.contains("is up") && vl.contains("Hello 5 s, dead 20 s, retransmit 8 s, transmit delay 2 s, no authentication"), "{vl}");
    assert!(lab.run(r1, "show ip route").contains("O IA 192.168.4.0/24"));
    // A dead interval alone is checked too.
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2 dead-interval 30\nend");
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("the dead interval differs (20 s here, 30 s at the far end)"), "{vl}");
    lab.run(r3, "conf t\nrouter ospf 1\nno area 1 virtual-link 2.2.2.2 dead-interval\nend");
    assert!(lab.run(r2, "show ip ospf virtual-links").contains("is up"));
    // Options are checked.
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "area 1 virtual-link 3.3.3.3 hello-interval 0").contains("1-65535 seconds"));
    assert!(lab.try_run(r2, "area 1 virtual-link 3.3.3.3 hello-interval").contains("1-65535 seconds"));
    assert!(lab.try_run(r2, "area 1 virtual-link 3.3.3.3 bogus").contains("not a virtual-link option"));
    assert!(lab.try_run(r2, "area 1 virtual-link 3.3.3.3 authentication-key abcdefghi").contains("1-8 characters"));
    assert!(lab.try_run(r2, "area 1 virtual-link 3.3.3.3 message-digest-key 1 sha key").contains("Only md5"));
    assert!(lab.try_run(r2, "area 1 virtual-link 3.3.3.3 message-digest-key 0 md5 key").contains("1-255"));
    // Nothing of the failed lines stuck.
    lab.run(r2, "end");
    assert!(lab.run(r2, "show ip ospf virtual-links").contains("Hello 5 s, dead 20 s, retransmit 5 s, transmit delay 1 s"));
    // The running config shows the options and accepts them again.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 virtual-link 3.3.3.3 dead-interval 25\nend");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains(" area 1 virtual-link 3.3.3.3 hello-interval 5 dead-interval 25\n"), "{run}");
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 virtual-link 3.3.3.3\nend");
    assert!(!lab.run(r2, "show running-config").contains("virtual-link"));
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 virtual-link 3.3.3.3 hello-interval 5 dead-interval 25\nend");
    assert_eq!(lab.run(r2, "show running-config"), run);
    // Taking an option off leaves the link and the other options.
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 virtual-link 3.3.3.3 dead-interval\nend");
    assert!(lab.run(r2, "show running-config").contains(" area 1 virtual-link 3.3.3.3 hello-interval 5\n"));
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 virtual-link 3.3.3.3 hello-interval\nend");
    assert!(lab.run(r2, "show running-config").contains(" area 1 virtual-link 3.3.3.3\n"));
}

#[test]
fn a_virtual_link_authenticates_with_its_own_password_or_message_digest_key() {
    let (mut lab, r1, r2, r3, _r4) = four_routers();
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 virtual-link 3.3.3.3 authentication message-digest message-digest-key 1 md5 first\nend");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2\nend");
    // One end signs, the other does not.
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("is down") && vl.contains("authentication differs: message digest here, none at the far end"), "{vl}");
    // Both sign, with different keys.
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2 authentication message-digest message-digest-key 1 md5 other\nend");
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("is down") && vl.contains("the far end has no message-digest key 1 with the same key"), "{vl}");
    // The same key: up.
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2 message-digest-key 1 md5 first\nend");
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("is up") && vl.contains("message-digest authentication"), "{vl}");
    assert!(lab.run(r1, "show ip route").contains("O IA 192.168.4.0/24"));
    // A new key on one end only is not enough: the highest key id signs.
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2 message-digest-key 2 md5 second\nend");
    assert!(lab.run(r2, "show ip ospf virtual-links").contains("this router has no message-digest key 2 with the same key"));
    assert!(lab.run(r3, "show ip ospf virtual-links").contains("the far end has no message-digest key 2 with the same key"));
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 virtual-link 3.3.3.3 message-digest-key 2 md5 second\nend");
    assert!(lab.run(r3, "show ip ospf virtual-links").contains("is up"));
    // Take a key off by its id, then all of them.
    lab.run(r3, "conf t\nrouter ospf 1\nno area 1 virtual-link 2.2.2.2 message-digest-key 2\nend");
    assert!(!lab.run(r3, "show running-config").contains("message-digest-key 2"));
    let run = lab.run(r2, "show running-config");
    assert!(
        run.contains(" area 1 virtual-link 3.3.3.3 authentication message-digest message-digest-key 1 md5 first message-digest-key 2 md5 second\n"),
        "{run}"
    );
    // A clear-text password.
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 virtual-link 3.3.3.3\narea 1 virtual-link 3.3.3.3 authentication authentication-key sesame\nend");
    lab.run(r3, "conf t\nrouter ospf 1\nno area 1 virtual-link 2.2.2.2\narea 1 virtual-link 2.2.2.2 authentication authentication-key wrong\nend");
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("is down") && vl.contains("the clear-text password differs"), "{vl}");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2 authentication-key sesame\nend");
    let vl = lab.run(r2, "show ip ospf virtual-links");
    assert!(vl.contains("is up") && vl.contains("simple password authentication"), "{vl}");
}

#[test]
fn a_virtual_link_that_is_up_is_a_row_of_show_ip_ospf_neighbor() {
    let (mut lab, _r1, r2, r3, _r4) = four_routers();
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 virtual-link 3.3.3.3\nend");
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 virtual-link 2.2.2.2\nend");
    assert!(lab.run(r2, "show ip ospf virtual-links").contains("is up"));
    let n = lab.run(r2, "show ip ospf neighbor");
    assert!(n.contains("OSPF_VL0 (area 1)") && n.contains("3.3.3.3") && n.contains("FULL/ -"), "{n}");
    // One end without the line: the link is down and its row is gone.
    lab.run(r3, "conf t\nrouter ospf 1\nno area 1 virtual-link 2.2.2.2\nend");
    assert!(!lab.run(r2, "show ip ospf neighbor").contains("OSPF_VL"));
}
