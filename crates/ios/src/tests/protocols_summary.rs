//! `show ip protocols summary` (M9.7).

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn protocols_summary_has_one_row_per_protocol_with_counts() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    assert!(lab.run(r1, "show ip protocols summary").starts_with("No routing protocol is running"));
    for (r, ip, id, lo) in [(r1, "10.0.0.1", "1.1.1.1", "172.16.1.1"), (r2, "10.0.0.2", "2.2.2.2", "172.16.2.1")] {
        lab.run(
            r,
            &format!(
                "en\nconf t\ninterface g0/0\nip address {ip} 255.255.255.0\nno shutdown\nexit\ninterface loopback 1\nip address {lo} 255.255.255.255\nexit\nrouter ospf 1\nrouter-id {id}\nnetwork 10.0.0.0 0.0.0.255 area 0\nnetwork 172.16.0.0 0.0.255.255 area 0\npassive-interface loopback 1\nexit\nrouter eigrp 100\nnetwork 10.0.0.0 0.0.0.255\nexit\nrouter rip\nversion 2\nnetwork 10.0.0.0\nend"
            ),
        );
    }
    lab.sim.run_for(&lab.t, 60_000_000);
    let s = lab.run(r1, "show ip protocols summary");
    let row = |name: &str| s.lines().find(|l| l.starts_with(name)).unwrap_or("").split_whitespace().map(str::to_string).collect::<Vec<_>>();
    assert!(s.starts_with("Protocol"), "{s}");
    // OSPF: process 1, router ID, two interfaces (one passive), one neighbour, one route (the other loopback).
    assert_eq!(row("OSPF"), ["OSPF", "1", "1.1.1.1", "2", "1", "1", "1"], "{s}");
    let eigrp = row("EIGRP");
    assert_eq!(&eigrp[..3], ["EIGRP", "AS", "100"], "{s}");
    assert!(eigrp.contains(&"1".to_string()), "{s}");
    assert_eq!(row("RIP")[..2], ["RIP", "v2"], "{s}");
    // The long form is still there, and a wrong word is refused.
    assert!(lab.run(r1, "show ip protocols").contains("Routing protocol: RIP"));
    assert_eq!(lab.run(r1, "show ip protocols sum"), s);
    assert!(lab.try_run(r1, "show ip protocols bogus").starts_with("% "));
}
