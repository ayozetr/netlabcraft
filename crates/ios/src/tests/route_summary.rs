//! `show ip route summary` (M9.7).

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn route_summary_counts_routes_by_source() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(
        r1,
        "en\nconf t\ninterface g0/0\nip address 10.0.0.1 255.255.255.0\nno shutdown\nexit\ninterface loopback 1\nip address 192.168.1.1 255.255.255.255\nexit\nip route 172.16.0.0 255.255.0.0 10.0.0.2\nip route 0.0.0.0 0.0.0.0 10.0.0.2\nrouter ospf 1\nrouter-id 1.1.1.1\nnetwork 10.0.0.0 0.0.0.255 area 0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\ninterface g0/0\nip address 10.0.0.2 255.255.255.0\nno shutdown\nexit\ninterface loopback 1\nip address 192.168.2.1 255.255.255.255\nexit\nrouter ospf 1\nrouter-id 2.2.2.2\nnetwork 10.0.0.0 0.0.0.255 area 0\nnetwork 192.168.2.0 0.0.0.255 area 0\nend",
    );
    lab.sim.run_for(&lab.t, 60_000_000);
    let s = lab.run(r1, "show ip route summary");
    assert!(s.starts_with("Source"), "{s}");
    let row = |name: &str| s.lines().find(|l| l.starts_with(name)).unwrap_or("").to_string();
    assert!(row("connected").contains(" 2 "), "{s}");
    assert!(row("local").contains(" 1 "), "{s}");
    assert!(row("static").contains(" 2 "), "{s}");
    assert!(row("OSPF").contains("intra-area 1, inter-area 0, external 0"), "{s}");
    assert!(s.contains("Default route: via 10.0.0.2"), "{s}");
    // The total counts every route of the table: the sum of the rows.
    let table = lab.run(r1, "show ip route");
    let routes = table.lines().filter(|l| l.len() > 5 && l.get(5..).is_some_and(|r| r.contains('/'))).count();
    assert!(row("total").contains(&format!(" {routes} ")), "{s}\n{table}");
    // A short form, and the other forms are unchanged.
    assert_eq!(lab.run(r1, "show ip route sum"), s);
    assert!(lab.run(r1, "show ip route static").contains("172.16.0.0"));
    // A router with nothing says so.
    let r3 = lab.add(DeviceKind::Router);
    let empty = lab.run(r3, "show ip route summary");
    assert!(empty.contains("No default route") && empty.contains("total"), "{empty}");
}
