//! ICMP type and code in access-list entries (M9.7).

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// PC -- R1 -- R2 -- Server: R1 and R2 are routers on 10.0.12.0/24, so a trace from the PC
/// crosses two hops.
fn chain() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let pc = lab.add(DeviceKind::Pc);
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(pc, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r2, "g0/1", srv, "g0");
    lab.run(pc, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 192.168.2.10 255.255.255.0 192.168.2.1");
    lab.run(
        r1,
        "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip address 10.0.12.1 255.255.255.0\nno shut\nexit\nip route 192.168.2.0 255.255.255.0 10.0.12.2\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/0\nip address 10.0.12.2 255.255.255.0\nno shut\nint g0/1\nip address 192.168.2.1 255.255.255.0\nno shut\nexit\nip route 192.168.1.0 255.255.255.0 10.0.12.1\nend",
    );
    (lab, pc, r1, r2)
}

#[test]
fn icmp_type_and_code_are_typed_shown_and_kept_in_the_configuration() {
    let (mut lab, _pc, r1, _) = chain();
    lab.run(
        r1,
        "conf t\nip access-list extended E\npermit icmp any any 3 4\npermit icmp any any echo\npermit icmp any any host-unreachable\n\
         permit icmp any any unreachable\npermit icmp any any 17 9 log\ndeny icmp any any ttl-exceeded\nend",
    );
    let show = lab.run(r1, "show access-lists E");
    // `3 4` is the name of that type and code; `3` alone is every code, and so is `unreachable`.
    assert!(show.contains("10 permit icmp any any packet-too-big"), "{show}");
    assert!(show.contains("20 permit icmp any any echo"), "{show}");
    assert!(show.contains("30 permit icmp any any host-unreachable"), "{show}");
    assert!(show.contains("40 permit icmp any any unreachable"), "{show}");
    assert!(show.contains("50 permit icmp any any 17 9 log"), "{show}");
    assert!(show.contains("60 deny icmp any any ttl-exceeded"), "{show}");
    let cfg = lab.run(r1, "show running-config");
    assert!(cfg.contains(" permit icmp any any packet-too-big") && cfg.contains(" permit icmp any any 17 9 log"), "{cfg}");
    // A code after a name is not accepted, and a word that is neither is refused.
    lab.run(r1, "conf t\nip access-list extended E");
    for bad in
        ["permit icmp any any echo 4", "permit icmp any any 3 4 5", "permit icmp any any 300", "permit icmp any any bogus", "permit tcp any any 3 4"]
    {
        assert!(lab.try_run(r1, bad).starts_with("% "), "{bad}");
    }
}

#[test]
fn an_acl_that_drops_time_exceeded_breaks_traceroute_but_not_ping() {
    let (mut lab, pc, r1, _) = chain();
    lab.run(pc, "ping 192.168.2.10");
    assert!(lab.run(pc, "ping 192.168.2.10").contains("4 answered"));
    let before = lab.run(pc, "tracert 192.168.2.10");
    assert!(before.contains("192.168.1.1") && before.contains("10.0.12.2"), "{before}");
    // Replies come back into R1 on g0/1: drop the TTL-expired ones there, let the rest in.
    lab.run(
        r1,
        "conf t\nip access-list extended FROM-CORE\ndeny icmp any any time-exceeded\npermit ip any any\nexit\nint g0/1\nip access-group FROM-CORE in\nend",
    );
    assert!(lab.run(pc, "ping 192.168.2.10").contains("4 answered"));
    let after = lab.run(pc, "tracert 192.168.2.10");
    assert!(after.contains("192.168.2.10"), "the destination still answers: {after}");
    assert!(!after.contains("10.0.12.2"), "the router behind R1 is no longer named: {after}");
    let show = lab.run(r1, "show access-lists FROM-CORE");
    assert!(show.contains("10 deny icmp any any time-exceeded (") && show.contains("matches)"), "{show}");
    // Only that type is dropped: the other ICMP messages are not.
    lab.run(r1, "conf t\nip access-list extended FROM-CORE\nno 10\n10 deny icmp any any 11 1\nend");
    assert!(lab.run(pc, "tracert 192.168.2.10").contains("10.0.12.2"), "code 1 is not what traceroute provokes");
}

#[test]
fn a_code_only_matches_that_code() {
    let (mut lab, pc, r1, _) = chain();
    lab.run(pc, "ping 192.168.2.10");
    assert!(lab.run(pc, "ping 192.168.2.10").contains("4 answered"), "control: the path works before the list");
    lab.run(
        r1,
        "conf t\nip access-list extended FROM-CORE\ndeny icmp any any 0 0\npermit ip any any\nexit\nint g0/1\nip access-group FROM-CORE in\nend",
    );
    // Echo replies are type 0, code 0: the ping dies, the trace's replies are other types.
    assert!(lab.run(pc, "ping 192.168.2.10").contains("0 answered"));
    lab.run(r1, "conf t\nip access-list extended FROM-CORE\nno 10\n10 deny icmp any any 0 5\nend");
    assert!(lab.run(pc, "ping 192.168.2.10").contains("4 answered"));
}
