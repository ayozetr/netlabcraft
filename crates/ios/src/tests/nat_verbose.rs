//! `show ip nat translations verbose` (M9.7).

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn verbose_translations_show_age_last_use_and_packet_counts() {
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
    lab.run(a, "ping 203.0.113.10");
    let plain = lab.run(r, "show ip nat translations");
    assert!(!plain.contains("created"), "{plain}");
    lab.sim.run_for(&lab.t, 7_000_000);
    let verbose = lab.run(r, "show ip nat translations verbose");
    assert!(verbose.contains("icmp") && verbose.contains("192.168.1.10"), "{verbose}");
    // Each echo has a translation of its own (per-flow identifiers): one out, one back.
    assert!(
        verbose.contains("dynamic, translated per flow (port or identifier); created 7 s ago, last used 7 s ago, 2 packets translated"),
        "{verbose}"
    );
    assert!(verbose.contains("static entry from the configuration, never ages out"), "{verbose}");
    // Every row of the plain table is still there, in the same order.
    for line in plain.lines() {
        assert!(verbose.contains(line), "{line}\n{verbose}");
    }
    lab.run(a, "ping 203.0.113.10");
    let again = lab.run(r, "show ip nat translations verbose");
    assert!(again.contains("last used 0 s ago"), "{again}");
    assert_eq!(lab.run(r, "show ip nat translations v"), again);
    assert!(lab.try_run(r, "show ip nat translations bogus").starts_with("% "));
    lab.run(r, "clear ip nat translation *");
    assert!(!lab.run(r, "show ip nat translations verbose").contains("created"));
}
