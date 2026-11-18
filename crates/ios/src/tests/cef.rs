//! `show ip cef` as a view of the routing table, and `ip default-network`.

use netlabcraft_model::DeviceKind;

use super::Lab;

fn lab() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 192.168.1.2 255.255.255.0\nno shut\nend");
    (lab, r1, r2)
}

#[test]
fn the_forwarding_table_follows_the_routing_table() {
    let (mut lab, r1, _) = lab();
    lab.run(r1, "conf t\nip route 172.16.0.0 255.255.0.0 192.168.1.2\nend");
    let cef = lab.run(r1, "show ip cef");
    assert!(cef.contains("192.168.1.0/24") && cef.contains("attached") && cef.contains("GigabitEthernet0/0"), "{cef}");
    assert!(cef.contains("192.168.1.1/32") && cef.contains("receive"), "{cef}");
    assert!(cef.lines().any(|l| l.starts_with("172.16.0.0/16") && l.contains("192.168.1.2")), "{cef}");
    // The adjacency needs the next hop's ARP entry: none before traffic, the MAC after a ping.
    assert!(lab.run(r1, "show ip cef detail").contains("incomplete"));
    lab.run(r1, "ping 192.168.1.2");
    let detail = lab.run(r1, "show ip cef 172.16.5.5 detail");
    assert!(detail.contains("172.16.0.0/16") && detail.contains("adjacency: ") && !detail.contains("incomplete"), "{detail}");
    let one = lab.run(r1, "show ip cef 172.16.5.5");
    assert!(one.contains("172.16.0.0/16") && !one.contains("adjacency"), "{one}");
    assert!(lab.run(r1, "show ip cef 8.8.8.8").contains("No forwarding entry covers 8.8.8.8"));
    let summary = lab.run(r1, "show ip cef summary");
    assert!(summary.contains("through a next hop: 1 (1 with a resolved adjacency)") && summary.contains("default route: none"), "{summary}");
    assert!(lab.try_run(r1, "show ip cef 1.2.3").starts_with("% "));
}

#[test]
fn a_default_network_makes_a_route_to_it_the_default() {
    let (mut lab, r1, _) = lab();
    let dst: std::net::Ipv4Addr = "8.8.8.8".parse().unwrap();
    let d = lab.t.device(r1).unwrap().clone();
    assert!(lab.sim.route(&lab.t, &d, dst).is_none(), "no route at all yet");
    lab.run(r1, "conf t\nip route 172.16.0.0 255.255.0.0 192.168.1.2\nip default-network 172.16.0.0\nend");
    let d = lab.t.device(r1).unwrap().clone();
    let (out, nh) = lab.sim.route(&lab.t, &d, dst).expect("the candidate gives a way out");
    assert_eq!((out.name.as_str(), nh.to_string().as_str()), ("GigabitEthernet0/0", "192.168.1.2"));
    let routes = lab.run(r1, "show ip route");
    assert!(routes.contains("0.0.0.0/0"), "{routes}");
    assert!(lab.run(r1, "show running-config").contains("ip default-network 172.16.0.0\n"));
    // A real default route takes precedence; removing the candidate removes the default.
    lab.run(r1, "conf t\nno ip default-network 172.16.0.0\nend");
    let d = lab.t.device(r1).unwrap().clone();
    assert!(lab.sim.route(&lab.t, &d, dst).is_none());
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "no ip default-network 172.16.0.0").starts_with("% "));
    assert!(lab.try_run(r1, "ip default-network 0.0.0.0").starts_with("% "));
}
