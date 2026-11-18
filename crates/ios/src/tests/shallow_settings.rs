//! Settings that used to be only kept and shown and now change what the simulator does.

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn no_ip_unreachables_keeps_a_router_from_answering_with_unreachables() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 10.0.0.2 255.255.255.0\nno shut\nexit\nip route 0.0.0.0 0.0.0.0 10.0.0.1\nend");
    let with = lab.run(r2, "ping 10.9.9.9");
    assert!(with.contains("UUU"), "R1 has no route and says so: {with}");
    lab.run(r1, "conf t\nint g0/0\nno ip unreachables\nend");
    let without = lab.run(r2, "ping 10.9.9.9");
    assert!(!without.contains("UU"), "R1 stays silent: {without}");
    lab.run(r1, "conf t\nint g0/0\nip unreachables\nend");
    assert!(lab.run(r2, "ping 10.9.9.9").contains("UUU"), "turned back on");
}

/// Two routers on 10.0.0.0/24; R2 reaches everything else through R1.
fn pair() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 10.0.0.2 255.255.255.0\nno shut\nexit\nip route 0.0.0.0 0.0.0.0 10.0.0.1\nend");
    (lab, r1, r2)
}

#[test]
fn an_unreachable_rate_limit_spaces_the_messages() {
    let (mut lab, r1, r2) = pair();
    assert!(lab.run(r2, "ping 10.9.9.9").contains("UUU"));
    let said = lab.run(r1, "conf t\nip icmp rate-limit unreachable 60000");
    assert!(!said.contains("does not change"), "{said}");
    lab.run(r1, "end");
    let limited = lab.run(r2, "ping 10.9.9.9");
    assert!(!limited.contains("UU"), "one message in the interval: {limited}");
}

#[test]
fn without_ip_classless_a_default_route_does_not_cover_a_known_major_network() {
    let (mut lab, _r1, r2) = pair();
    let dst: std::net::Ipv4Addr = "10.9.9.9".parse().unwrap();
    let d = lab.t.device(r2).unwrap();
    assert!(netlabcraft_sim::route(&lab.t, d, dst).is_some(), "classless: the default route carries it");
    let said = lab.run(r2, "conf t\nno ip classless");
    assert!(!said.contains("does not change"), "{said}");
    lab.run(r2, "end");
    let d = lab.t.device(r2).unwrap();
    assert!(netlabcraft_sim::route(&lab.t, d, dst).is_none(), "10.0.0.0/24 is known, 10.9.9.0 is not");
    let other: std::net::Ipv4Addr = "172.16.0.1".parse().unwrap();
    assert!(netlabcraft_sim::route(&lab.t, d, other).is_some(), "another major network still uses the default");
    lab.run(r2, "conf t\nip classless\nend");
    let d = lab.t.device(r2).unwrap();
    assert!(netlabcraft_sim::route(&lab.t, d, dst).is_some());
}

#[test]
fn mac_aging_time_forgets_silent_addresses() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(b, "fa0", s, "fa0/2");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.1.11 255.255.255.0 192.168.1.1");
    lab.run(s, "en");
    lab.run(a, "ping 192.168.1.11");
    assert!(lab.run(s, "show mac address-table").contains("2 entries"));
    let said = lab.run(s, "conf t\nmac address-table aging-time 20\nend");
    assert!(!said.contains("does not change"), "{said}");
    lab.sim.run_for(&lab.t, 30_000_000);
    assert!(lab.run(s, "show mac address-table").contains("0 entries"));
    assert!(lab.run(s, "show mac address-table aging-time").contains("forgotten"));
    // Never ages with 0.
    lab.run(s, "conf t\nmac address-table aging-time 0\nend");
    lab.run(a, "ping 192.168.1.11");
    lab.sim.run_for(&lab.t, 600_000_000);
    assert!(lab.run(s, "show mac address-table").contains("2 entries"));
}

#[test]
fn lldp_timer_and_holdtime_change_the_advertisements() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nlldp run\nlldp timer 5\nlldp holdtime 20\nint g0/0\nno shut\nend");
    lab.run(r2, "en\nconf t\nlldp run\nint g0/0\nno shut\nend");
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 61_000_000);
    let from_r1 = lab.sim.trace().iter().filter(|e| e.protocol.name() == "LLDP" && e.from.device == r1).count();
    let from_r2 = lab.sim.trace().iter().filter(|e| e.protocol.name() == "LLDP" && e.from.device == r2).count();
    assert!(from_r1 >= 10 && from_r2 <= 4 && from_r1 > from_r2 * 3, "{from_r1} against {from_r2}");
    assert!(lab.run(r1, "show lldp").contains("every 5 s, kept for 20 s"));
    assert!(lab.run(r2, "show lldp neighbors detail").contains("Expires      in"));
}

#[test]
fn sequence_numbers_and_the_log_rate_limit_shape_the_log() {
    let (mut lab, r1, _r2) = pair();
    lab.run(r1, "conf t\nservice sequence-numbers\nend");
    lab.sim.log(r1, "first message");
    lab.sim.log(r1, "second message");
    let log = lab.run(r1, "show logging");
    assert!(log.contains("#000001 ") && log.contains("#000002 "), "{log}");
    lab.run(r1, "conf t\nno service sequence-numbers\nlogging rate-limit 2\nend");
    lab.run(r1, "clear logging");
    // A fresh second: the commands above counted in the one before.
    lab.sim.run_for(&lab.t, 2_000_000);
    for i in 0..5 {
        lab.sim.log(r1, &format!("burst {i}"));
    }
    let log = lab.run(r1, "show logging");
    assert!(log.contains("burst 0") && log.contains("burst 1") && !log.contains("burst 4"), "{log}");
    assert!(!log.contains("#0000"));
}

#[test]
fn cdp_timer_and_holdtime_show_on_the_device() {
    let (mut lab, r1, _r2) = pair();
    lab.run(r1, "conf t\ncdp timer 20\ncdp holdtime 60\nend");
    assert!(lab.run(r1, "show cdp").contains("every 20 s, kept for 60 s"));
}
