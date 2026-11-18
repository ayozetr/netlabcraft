//! The P bit, the Nt bit, the forwarding address and the choice of the translator of an NSSA.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 in area 0, R2 and R4 border routers between area 0 and the NSSA (area 1), R3 inside it
/// redistributing a static route; the three NSSA routers share a switch.
fn nssa() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    let r4 = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.cable(r1, "g0/1", r4, "g0/0");
    lab.cable(r2, "g0/1", s, "g0/1");
    lab.cable(r3, "g0/0", s, "g0/2");
    lab.cable(r4, "g0/1", s, "g0/3");
    let ifs = |a: &str, b: &str| {
        format!("interface g0/0\nip address {a} 255.255.255.0\nno shutdown\nexit\ninterface g0/1\nip address {b} 255.255.255.0\nno shutdown\nexit\n")
    };
    lab.run(
        r1,
        &format!(
            "en\nconf t\n{}router ospf 1\nrouter-id 1.1.1.1\nnetwork 10.0.0.0 0.0.0.255 area 0\nnetwork 10.2.0.0 0.0.0.255 area 0\nend",
            ifs("10.0.0.1", "10.2.0.1")
        ),
    );
    lab.run(
        r2,
        &format!(
            "en\nconf t\n{}router ospf 1\nrouter-id 2.2.2.2\nnetwork 10.0.0.0 0.0.0.255 area 0\nnetwork 10.1.0.0 0.0.0.255 area 1\narea 1 nssa\nend",
            ifs("10.0.0.2", "10.1.0.2")
        ),
    );
    lab.run(
        r4,
        &format!(
            "en\nconf t\n{}router ospf 1\nrouter-id 4.4.4.4\nnetwork 10.2.0.0 0.0.0.255 area 0\nnetwork 10.1.0.0 0.0.0.255 area 1\narea 1 nssa\nend",
            ifs("10.2.0.4", "10.1.0.4")
        ),
    );
    lab.run(
        r3,
        "en\nconf t\ninterface g0/0\nip address 10.1.0.3 255.255.255.0\nno shutdown\nexit\nip route 192.168.50.0 255.255.255.0 10.1.0.99\nrouter ospf 1\nrouter-id 3.3.3.3\nnetwork 10.1.0.0 0.0.0.255 area 1\nredistribute static subnets\narea 1 nssa\nend",
    );
    lab.sim.run_for(&lab.t, 60_000_000);
    (lab, r1, r2, r3, r4)
}

#[test]
fn the_border_router_with_the_highest_router_id_translates_and_sets_the_nt_bit() {
    let (mut lab, r1, r2, r3, r4) = nssa();
    assert!(lab.run(r4, "show ip ospf").contains("translator: this router"));
    assert!(lab.run(r3, "show ip ospf").contains("translator: router 4.4.4.4"));
    let t7 = lab.run(r3, "show ip ospf database nssa-external");
    assert!(t7.contains("P bit set: the border router 4.4.4.4 translates it"), "{t7}");
    assert!(t7.contains("forwarding address 10.1.0.3"), "{t7}");
    assert!(lab.run(r4, "show ip ospf database router self-originate").contains("Nt bit set"));
    assert!(!lab.run(r2, "show ip ospf database router self-originate").contains("Nt bit set"));
    let t5 = lab.run(r1, "show ip ospf database external");
    assert!(t5.contains("advertised by 4.4.4.4") && t5.contains("forwarding address 10.1.0.3 (translated from a type 7 LSA)"), "{t5}");
}

#[test]
fn translate_type7_always_makes_a_lower_border_router_the_translator() {
    let (mut lab, r1, r2, r3, r4) = nssa();
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 nssa translate type7 always\nend");
    lab.sim.run_for(&lab.t, 5_000_000);
    assert!(lab.run(r2, "show running-config").contains(" area 1 nssa translate type7 always"));
    assert!(lab.run(r2, "show ip ospf").contains("translate type7 always"));
    assert!(lab.run(r3, "show ip ospf").contains("translator: router 2.2.2.2"));
    assert!(lab.run(r2, "show ip ospf database router self-originate").contains("Nt bit set"));
    assert!(!lab.run(r4, "show ip ospf database router self-originate").contains("Nt bit set"));
    assert!(lab.run(r1, "show ip ospf database external").contains("advertised by 2.2.2.2"));
    // Taking it off gives the election back to the highest router ID.
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 nssa translate type7 always\nend");
    lab.sim.run_for(&lab.t, 5_000_000);
    assert!(lab.run(r3, "show ip ospf").contains("translator: router 4.4.4.4"));
    assert!(!lab.run(r2, "show running-config").contains("translate type7"));
}

#[test]
fn the_translate_option_is_checked() {
    let (mut lab, _r1, r2, _r3, _r4) = nssa();
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "area 1 nssa translate").starts_with("% "));
    assert!(lab.try_run(r2, "area 1 nssa translate type7 sometimes").starts_with("% "));
    assert!(lab.try_run(r2, "area 1 nssa no-summary translate type7 always").is_empty());
    assert!(lab.run(r2, "do show running-config").contains(" area 1 nssa no-summary translate type7 always"));
}
