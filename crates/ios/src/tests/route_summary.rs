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

#[test]
fn route_summary_counts_by_mask_length_and_estimates_the_memory() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(
        r1,
        "en\nconf t\ninterface g0/0\nip address 10.0.0.1 255.255.255.0\nno shutdown\nexit\nip route 172.16.0.0 255.255.0.0 10.0.0.2\nip route 172.17.0.0 255.255.0.0 10.0.0.2\nip route 0.0.0.0 0.0.0.0 10.0.0.2\nend",
    );
    lab.run(r2, "en\nconf t\ninterface g0/0\nip address 10.0.0.2 255.255.255.0\nno shutdown\nend");
    let s = lab.run(r1, "show ip route summary");
    assert!(s.contains("Routes by mask:"), "{s}");
    let masks: Vec<(u8, usize)> = s
        .lines()
        .filter_map(|l| {
            let l = l.strip_prefix("  /")?;
            let (m, n) = l.split_once(' ')?;
            Some((m.parse().ok()?, n.trim().parse().ok()?))
        })
        .collect();
    let at = |m: u8| masks.iter().find(|x| x.0 == m).map_or(0, |x| x.1);
    assert_eq!((at(0), at(16)), (1, 2), "{s}");
    assert!(at(24) >= 1, "{s}");
    // The rows add up to the total of the source table.
    let total: usize = s.lines().find(|l| l.starts_with("total")).and_then(|l| l.split_whitespace().nth(1)).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!(total > 0 && masks.iter().map(|x| x.1).sum::<usize>() == total, "{s}");
    // The memory line is a count times a fixed size, and says it is an estimate.
    assert!(s.contains(&format!("Memory: {total} routes x 160 bytes = {} bytes (simulated estimate", total * 160)), "{s}");
    // A router with nothing has an empty list and no bytes.
    let r3 = lab.add(DeviceKind::Router);
    let empty = lab.run(r3, "show ip route summary");
    assert!(empty.contains("Routes by mask:") && empty.contains("Memory: 0 routes x 160 bytes = 0 bytes"), "{empty}");
}
