//! Dynamic NAT translations age out (console depth round of 2026-11-15).

use netlabcraft_core::DeviceId;
use netlabcraft_model::{DeviceKind, NatTimeouts};

use super::Lab;

/// A PC behind a PAT router, a static entry, and a server on the outside.
fn pat_lab() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(a, "fa0", r, "g0/0");
    lab.cable(srv, "g0", r, "g0/1");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 203.0.113.10 255.255.255.0 203.0.113.1");
    lab.run(
        r,
        "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nip nat inside\nno shut\nint g0/1\nip address 203.0.113.1 255.255.255.0\nip nat outside\nno shut\nexit\n\
         access-list 1 permit 192.168.1.0 0.0.0.255\nip nat inside source list 1 interface g0/1 overload\nip nat inside source static 192.168.1.20 203.0.113.20\nend",
    );
    (lab, r, a)
}

#[test]
fn the_defaults_are_the_usual_ones_and_per_kind() {
    let t = NatTimeouts::default();
    assert_eq!((t.secs(1, true), t.secs(17, true), t.secs(6, true), t.secs(1, false), t.secs(47, true)), (60, 300, 86_400, 86_400, 86_400));
    let set = NatTimeouts { icmp: Some(5), all: Some(9), ..NatTimeouts::default() };
    assert_eq!((set.secs(1, true), set.secs(1, false), set.secs(17, true)), (5, 9, 300));
}

#[test]
fn an_idle_translation_expires_and_the_verbose_view_says_when() {
    let (mut lab, r, a) = pat_lab();
    assert!(lab.run(a, "ping 203.0.113.10").contains("answered"));
    lab.sim.run_for(&lab.t, 7_000_000);
    let early = lab.run(r, "show ip nat translations verbose");
    assert!(early.contains("created 7 s ago, last used 7 s ago, 2 packets translated; expires in 5"), "{early}");
    assert!(early.contains("static entry from the configuration, never ages out"), "{early}");
    // Past the ICMP timeout (60 s) the flows are gone; the static entry stays.
    lab.sim.run_for(&lab.t, 60_000_000);
    let late = lab.run(r, "show ip nat translations verbose");
    assert!(!late.contains("icmp") && late.contains("203.0.113.20"), "{late}");
    let stats = lab.run(r, "show ip nat statistics");
    assert!(stats.contains("Translations in use: 1 (1 static, 0 dynamic; 0 with ports)"), "{stats}");
    // Traffic makes new ones.
    lab.run(a, "ping 203.0.113.10");
    assert!(lab.run(r, "show ip nat translations").contains("icmp"));
}

#[test]
fn the_timeouts_are_commands_that_the_running_config_keeps() {
    let (mut lab, r, a) = pat_lab();
    lab.run(r, "conf t\nip nat translation icmp-timeout 10\nip nat translation timeout 120\nip nat translation udp-timeout 20\nip nat translation tcp-timeout 30\nend");
    let run = lab.run(r, "show running-config");
    for want in [
        "ip nat translation timeout 120",
        "ip nat translation udp-timeout 20",
        "ip nat translation icmp-timeout 10",
        "ip nat translation tcp-timeout 30",
    ] {
        assert!(run.contains(want), "{want}\n{run}");
    }
    lab.run(a, "ping 203.0.113.10");
    lab.sim.run_for(&lab.t, 5_000_000);
    let soon = lab.run(r, "show ip nat translations verbose");
    assert!(soon.contains("expires in 5 s") || soon.contains("expires in 4 s"), "{soon}");
    lab.sim.run_for(&lab.t, 12_000_000);
    assert!(!lab.run(r, "show ip nat translations").contains("icmp"));
    // Back to the defaults, and the lines go from the running config.
    lab.run(r, "conf t\nno ip nat translation icmp-timeout\nno ip nat translation timeout\nno ip nat translation udp-timeout\nno ip nat translation tcp-timeout\nend");
    assert!(!lab.run(r, "show running-config").contains("ip nat translation"));
    lab.run(a, "ping 203.0.113.10");
    lab.sim.run_for(&lab.t, 30_000_000);
    assert!(lab.run(r, "show ip nat translations").contains("icmp"));
    // What the console refuses.
    lab.run(r, "conf t");
    for bad in [
        "ip nat translation timeout 0",
        "ip nat translation timeout 2147484",
        "ip nat translation timeout x",
        "ip nat translation bogus 5",
        "ip nat translation timeout",
    ] {
        assert!(lab.try_run(r, bad).starts_with("% "), "{bad}");
    }
    assert!(lab.try_run(r, "no ip nat translation bogus").starts_with("% "));
}
