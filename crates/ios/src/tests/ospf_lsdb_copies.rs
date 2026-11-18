//! Every router holds its own copy of each LSA: ages grow by one per hop, a withdrawn LSA is
//! flushed at MaxAge, SPF runs are counted and throttled, and the LSA debugs report it all.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 - R2 - R3 in area 0; R1 redistributes a static route.
fn line() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    lab.run(
        r1,
        "en\nconf t\ninterface g0/0\nip address 10.0.0.1 255.255.255.0\nno shutdown\nexit\nip route 192.168.50.0 255.255.255.0 10.0.0.99\nrouter ospf 1\nrouter-id 1.1.1.1\nnetwork 10.0.0.0 0.0.0.255 area 0\nredistribute static subnets\nend",
    );
    lab.run(
        r2,
        "en\nconf t\ninterface g0/0\nip address 10.0.0.2 255.255.255.0\nno shutdown\nexit\ninterface g0/1\nip address 10.1.0.2 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 2.2.2.2\nnetwork 10.0.0.0 0.0.0.255 area 0\nnetwork 10.1.0.0 0.0.0.255 area 0\nend",
    );
    lab.run(
        r3,
        "en\nconf t\ninterface g0/0\nip address 10.1.0.3 255.255.255.0\nno shutdown\nexit\nrouter ospf 1\nrouter-id 3.3.3.3\nnetwork 10.1.0.0 0.0.0.255 area 0\nend",
    );
    lab.sim.run_for(&lab.t, 60_000_000);
    (lab, r1, r2, r3)
}

/// The number after `age ` in the first LSA the output lists.
fn age_of(text: &str) -> u64 {
    let at = text.find("age ").unwrap_or_else(|| panic!("no age in:\n{text}")) + "age ".len();
    text.get(at..).and_then(|t| t.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("bad age in:\n{text}"))
}

fn spf_runs(lab: &mut Lab, r: DeviceId) -> u32 {
    let shown = lab.run(r, "show ip ospf");
    let at = shown.find("SPF ran ").unwrap_or_else(|| panic!("no SPF count in:\n{shown}")) + "SPF ran ".len();
    shown.get(at..).and_then(|t| t.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("bad SPF count in:\n{shown}"))
}

#[test]
fn a_copy_is_one_second_older_for_every_router_it_crossed() {
    let (mut lab, r1, r2, r3) = line();
    let ages: Vec<u64> = [r1, r2, r3].iter().map(|r| age_of(&lab.run(*r, "show ip ospf database router adv-router 1.1.1.1"))).collect();
    assert_eq!(ages[1], ages[0] + 1, "{ages:?}");
    assert_eq!(ages[2], ages[0] + 2, "{ages:?}");
    // Every copy counts on from there.
    lab.sim.run_for(&lab.t, 30_000_000);
    let later = age_of(&lab.run(r3, "show ip ospf database router adv-router 1.1.1.1"));
    assert_eq!(later, ages[2] + 30);
}

#[test]
fn a_new_instance_reaches_every_router_with_its_sequence_number() {
    let (mut lab, r1, r2, r3) = line();
    lab.run(r1, "conf t\ninterface g0/0\nip ospf cost 7\nend");
    lab.run(r1, "show ip ospf");
    lab.sim.run_for(&lab.t, 1_000_000);
    let seq = |lab: &mut Lab, r: DeviceId| {
        let shown = lab.run(r, "show ip ospf database router adv-router 1.1.1.1");
        let at = shown.find("sequence 0x").unwrap_or_else(|| panic!("no sequence in:\n{shown}")) + "sequence 0x".len();
        u32::from_str_radix(shown.get(at..at + 8).unwrap_or(""), 16).unwrap_or(0)
    };
    let (a, b, c) = (seq(&mut lab, r1), seq(&mut lab, r2), seq(&mut lab, r3));
    assert!(a > 0x8000_0001 && a == b && b == c, "{a:#x} {b:#x} {c:#x}");
}

#[test]
fn a_withdrawn_lsa_is_flushed_at_maxage_before_it_leaves_the_other_databases() {
    let (mut lab, r1, _r2, r3) = line();
    assert!(lab.run(r3, "show ip ospf database external").contains("192.168.50.0"));
    lab.run(r1, "conf t\nrouter ospf 1\nno redistribute static\nend");
    // The originator drops it at once; nothing has reached the others yet.
    assert!(lab.run(r1, "show ip ospf database external").starts_with("No external LSAs"));
    // Flooded two hops away: R3 holds it at MaxAge for a moment.
    lab.sim.run_for(&lab.t, 200_000);
    let flushing = lab.run(r3, "show ip ospf database external");
    assert!(flushing.contains("192.168.50.0") && age_of(&flushing) == 3600, "{flushing}");
    assert!(lab.run(r3, "show ip ospf database").contains("3600"));
    // Then every router has removed it.
    lab.sim.run_for(&lab.t, 3_000_000);
    assert!(lab.run(r3, "show ip ospf database external").starts_with("No external LSAs"));
}

#[test]
fn spf_runs_are_counted_throttled_and_started_over_by_clear() {
    let (mut lab, r1, r2, _r3) = line();
    let first = spf_runs(&mut lab, r2);
    assert!(first >= 1);
    let shown = lab.run(r2, "show ip ospf");
    assert!(shown.contains("SPF throttle: wait 50 ms after a change, hold 200 ms between runs, at most 5000 ms"), "{shown}");
    lab.run(r1, "conf t\ninterface g0/0\nip ospf cost 9\nend");
    assert!(spf_runs(&mut lab, r2) > first, "a change of R1's LSA runs SPF on R2");
    // The timers are set and shown, and the configuration keeps them.
    lab.run(r2, "conf t\nrouter ospf 1\ntimers throttle spf 10 20 300\nend");
    let shown = lab.run(r2, "show ip ospf");
    assert!(shown.contains("wait 10 ms after a change, hold 20 ms between runs, at most 300 ms"), "{shown}");
    assert!(lab.run(r2, "show running-config").contains(" timers throttle spf 10 20 300"));
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "timers throttle spf 10 20 5").contains("longest wait"));
    assert!(lab.try_run(r2, "timers throttle spf 10 20 700000").starts_with("% "));
    lab.run(r2, "no timers throttle spf\nend");
    assert!(lab.run(r2, "show ip ospf").contains("wait 50 ms"));
    // The process starts over.
    lab.run(r2, "clear ip ospf process");
    assert_eq!(spf_runs(&mut lab, r2), 1);
}

#[test]
fn the_lsa_debugs_report_what_is_originated_flooded_and_heard() {
    let (mut lab, r1, r2, r3) = line();
    lab.run(r1, "debug ip ospf lsa-generation");
    lab.run(r3, "debug ip ospf flood");
    lab.run(r2, "debug ip ospf packet");
    let on = lab.run(r2, "show debugging");
    assert!(on.contains("OSPF packets"), "{on}");
    assert!(lab.run(r1, "show debugging").contains("OSPF LSA generation"));
    lab.run(r1, "conf t\ninterface g0/0\nip ospf cost 7\nend");
    lab.sim.run_for(&lab.t, 12_000_000);
    let own = lab.run(r1, "show logging");
    assert!(own.contains("OSPF LSA debug: router LSA 1.1.1.1 advertised by 1.1.1.1 in area 0 originated again, sequence 0x"), "{own}");
    let far = lab.run(r3, "show logging");
    assert!(far.contains("OSPF flood debug: router LSA 1.1.1.1 advertised by 1.1.1.1 in area 0 received, 2 adjacencies from its router"), "{far}");
    let heard = lab.run(r2, "show logging");
    assert!(heard.contains("OSPF packet debug: hello from 10.0.0.1 to 224.0.0.5 on GigabitEthernet0/0"), "{heard}");
    // The flush is reported too.
    lab.run(r1, "conf t\nrouter ospf 1\nno redistribute static\nend");
    lab.sim.run_for(&lab.t, 1_000_000);
    assert!(lab.run(r1, "show logging").contains("OSPF LSA debug: external LSA 192.168.50.0 advertised by 1.1.1.1 withdrawn: flushed with the age MaxAge"));
    assert!(lab.run(r3, "show logging").contains("OSPF flood debug: external LSA 192.168.50.0 advertised by 1.1.1.1 received with MaxAge"));
    lab.run(r1, "undebug all");
    assert!(lab.run(r1, "show debugging").contains("No debugging"));
    assert!(lab.try_run(r1, "debug ip ospf nonsense").starts_with("% "));
}
