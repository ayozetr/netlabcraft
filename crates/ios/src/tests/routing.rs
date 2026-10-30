//! Typed console tests for loopbacks, OSPF as the courses teach it, floating static routes and
//! routers as IPv6 clients.

use netlabcraft_model::DeviceKind;

use super::Lab;

/// Two routers joined on 10.0.0.0/30 (g0/1 to g0/1), each with a LAN on g0/0.
fn two_routers() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(r1, "en\nconf t\nint g0/1\nip add 10.0.0.1 255.255.255.252\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/1\nip add 10.0.0.2 255.255.255.252\nno shut\nend");
    (lab, r1, r2)
}

/// `show running-config` before, then after `copy run start` and `reload`, must match.
fn survives_reload(lab: &mut Lab, r: netlabcraft_core::DeviceId) -> String {
    let before = lab.run(r, "show running-config");
    lab.run(r, "copy run start");
    lab.run(r, "reload");
    let after = lab.run(r, "en\nshow running-config");
    assert_eq!(after, before);
    after
}

#[test]
fn loopbacks_by_typing() {
    let (mut lab, r1, r2) = two_routers();
    lab.run(r1, "conf t\ninterface loopback 0\nip address 1.1.1.1 255.255.255.255\ndescription router id\nexit\nint lo 1\nip add 172.16.1.1 255.255.255.0\nend");
    assert_eq!(lab.prompt(r1), "R1#");
    let brief = lab.run(r1, "show ip interface brief");
    assert!(brief.contains("Loopback0                1.1.1.1/32         up      up"), "{brief}");
    assert!(brief.contains("Loopback1                172.16.1.1/24      up      up"), "{brief}");
    let shown = lab.run(r1, "show interfaces loopback 0");
    assert!(shown.contains("Loopback0 is up, line protocol up") && shown.contains("Description: router id"), "{shown}");
    assert!(lab.run(r1, "show interfaces").contains("Loopback1 is up"));
    let routes = lab.run(r1, "show ip route");
    assert!(routes.contains("1.1.1.1/32") && routes.contains("Loopback0"), "{routes}");
    assert!(lab.run(r1, "ping 1.1.1.1").contains("!!!!!"));
    // Overlapping subnets are refused, as between any two interfaces.
    lab.run(r1, "conf t\nint lo2");
    assert!(lab.try_run(r1, "ip address 172.16.1.9 255.255.255.0").contains("overlaps"));
    lab.run(r1, "exit\nno interface loopback 2\nend");
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "no interface loopback 7").contains("no interface Loopback7"));
    lab.run(r1, "end");
    // OSPF advertises a loopback as a host route; the highest loopback is the router ID.
    lab.run(r1, "conf t\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nnetwork 1.1.1.1 0.0.0.0 area 0\nnetwork 172.16.1.0 0.0.0.255 area 0\nend");
    lab.run(r2, "conf t\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nend");
    assert!(lab.run(r1, "show ip ospf").contains("Router id 172.16.1.1"));
    let r2_routes = lab.run(r2, "show ip route");
    assert!(r2_routes.contains("1.1.1.1/32") && r2_routes.contains("172.16.1.1/32"), "{r2_routes}");
    assert!(lab.run(r2, "ping 1.1.1.1").contains('!'));
    assert!(lab.run(r2, "ping 172.16.1.1").contains('!'));
    // Shut down: down in the brief, out of the routing tables.
    lab.run(r1, "conf t\nint lo1\nshutdown\nend");
    assert!(lab.run(r1, "show ip interface brief").contains("Loopback1                172.16.1.1/24      down    down"));
    assert!(!lab.run(r2, "show ip route").contains("172.16.1.1/32"));
    lab.run(r1, "conf t\nint lo1\nno shutdown\nend");
    // RIP and EIGRP advertise it when a network statement covers it.
    lab.run(r1, "conf t\nno router ospf 1\nrouter eigrp 10\nnetwork 10.0.0.0\nnetwork 172.16.0.0\nend");
    lab.run(r2, "conf t\nno router ospf 1\nrouter eigrp 10\nnetwork 10.0.0.0\nend");
    let eigrp = lab.run(r2, "show ip route");
    assert!(eigrp.contains("172.16.1.0/24") && eigrp.contains("EIGRP"), "{eigrp}");
    lab.run(r1, "conf t\nno router eigrp 10\nrouter rip\nversion 2\nnetwork 10.0.0.0\nnetwork 1.0.0.0\nend");
    lab.run(r2, "conf t\nno router eigrp 10\nrouter rip\nversion 2\nnetwork 10.0.0.0\nend");
    let rip = lab.run(r2, "show ip route");
    assert!(rip.contains("R    1.1.1.1/32"), "{rip}");
    // IPv6 on a loopback.
    lab.run(r1, "conf t\nint lo0\nipv6 address 2001:db8:ff::1/128\nend");
    assert!(lab.run(r1, "show ipv6 interface brief").contains("Loopback0"));
    let run = survives_reload(&mut lab, r1);
    assert!(
        run.contains("interface Loopback0\n description router id\n ip address 1.1.1.1 255.255.255.255\n ipv6 address 2001:db8:ff::1/128"),
        "{run}"
    );
    assert!(run.contains("interface Loopback1\n ip address 172.16.1.1 255.255.255.0"), "{run}");
    // A switch takes loopbacks too; a PC doesn't.
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en\nconf t\ninterface loopback 0\nip address 9.9.9.9 255.255.255.255\nend");
    assert!(lab.run(s, "show ip interface brief").contains("Loopback0"));
}

#[test]
fn ospf_as_the_courses_teach_it() {
    let (mut lab, r1, r2) = two_routers();
    // `ip ospf N area A` on the interfaces, no network statements; it starts the process.
    lab.run(r1, "conf t\nint lo0\nip address 1.1.1.1 255.255.255.0\nip ospf 1 area 0\nint g0/1\nip ospf 1 area 0\nend");
    lab.run(r2, "conf t\nrouter ospf 1\nexit\nint g0/1\nip ospf 1 area 0.0.0.0\nend");
    let nb = lab.run(r1, "show ip ospf neighbor");
    assert!(nb.contains("FULL/"), "{nb}");
    // A loopback is a host route unless it is set point to point.
    assert!(lab.run(r2, "show ip route").contains("1.1.1.1/32"));
    lab.run(r1, "conf t\nint lo0\nip ospf network point-to-point\nend");
    assert!(lab.run(r2, "show ip route").contains("1.1.1.0/24"));
    // Another process number on the same router is refused.
    lab.run(r1, "conf t\nint g0/0");
    assert!(lab.try_run(r1, "ip ospf 2 area 0").contains("process 1 runs here"));
    lab.run(r1, "end");
    // Hello and dead intervals must match: a mismatch keeps them apart and says why.
    lab.run(r1, "conf t\nint g0/1\nip ospf hello-interval 5\nend");
    let nb = lab.run(r1, "show ip ospf neighbor");
    assert!(!nb.contains("FULL/") && nb.contains("hello/dead intervals differ (5/20 s here, 10/40 s there)"), "{nb}");
    lab.run(r1, "debug ip ospf hello");
    lab.sim.run_for(&lab.t, 11_000_000);
    let log = lab.run(r1, "show logging");
    assert!(log.contains("no adjacency with 10.0.0.2 on GigabitEthernet0/1: hello/dead intervals 10/40 s, here 5/20 s"), "{log}");
    lab.run(r1, "no debug ip ospf");
    lab.run(r2, "conf t\nint g0/1\nip ospf hello-interval 5\nip ospf dead-interval 20\nend");
    let nb = lab.run(r1, "show ip ospf neighbor");
    assert!(nb.contains("FULL/DR") || nb.contains("FULL/BDR"), "{nb}");
    // Point to point: no DR or BDR on the link.
    lab.run(r1, "conf t\nint g0/1\nip ospf network point-to-point\nend");
    lab.run(r2, "conf t\nint g0/1\nip ospf network point-to-point\nend");
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL/P2P"));
    let detail = lab.run(r1, "show ip ospf neighbor detail");
    assert!(detail.contains("Neighbour 10.0.0.2 (R2)") && detail.contains("Hello every 5 s, declared dead after 20 s"), "{detail}");
    // passive-interface default, then the exceptions.
    lab.run(r2, "conf t\nrouter ospf 1\npassive-interface default\nend");
    assert!(lab.run(r1, "show ip ospf neighbor").contains("No OSPF neighbours"));
    lab.run(r2, "conf t\nrouter ospf 1\nno passive-interface g0/1\nend");
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL/P2P"));
    // redistribute static: R2's static route reaches R1 as an external route.
    lab.run(
        r2,
        "conf t\nint lo5\nip address 10.5.5.5 255.255.255.0\nexit\nip route 172.31.0.0 255.255.0.0 10.5.5.9\nrouter ospf 1\nredistribute static subnets\nend",
    );
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O E2 172.31.0.0/16"), "{rt}");
    // The database: router LSAs of both (R2's ID is now its loopback), the external LSA.
    let db = lab.run(r1, "show ip ospf database");
    assert!(
        db.contains("Router LSAs (type 1), area 0") && db.contains("10.5.5.5         10.5.5.5         2") && db.contains("AS external LSAs"),
        "{db}"
    );
    assert!(db.contains("172.31.0.0       10.5.5.5         /16"), "{db}");
    // A loopback in area 1 makes R1 a border router: R2 hears of it in a summary LSA.
    lab.run(r1, "conf t\nint lo0\nip ospf 1 area 1\nend");
    let db = lab.run(r2, "show ip ospf database");
    assert!(db.contains("Summary network LSAs (type 3), area 0") && db.contains("1.1.1.0          1.1.1.1          /24"), "{db}");
    assert!(lab.run(r2, "show ip route").contains("O IA 1.1.1.0/24"));
    let run1 = survives_reload(&mut lab, r1);
    assert!(
        run1.contains("interface GigabitEthernet0/1\n ip address 10.0.0.1 255.255.255.252\n ip ospf hello-interval 5\n ip ospf network point-to-point\n ip ospf 1 area 0"),
        "{run1}"
    );
    let run2 = survives_reload(&mut lab, r2);
    assert!(
        run2.contains(" passive-interface default\n no passive-interface GigabitEthernet0/1") && run2.contains(" redistribute static subnets"),
        "{run2}"
    );
    assert!(run2.contains(" ip ospf dead-interval 20"), "{run2}");
}

#[test]
fn floating_static_routes() {
    // R1 reaches R2's LAN over two links: g0/1 with OSPF, g0/2 with a floating static route.
    let (mut lab, r1, r2) = two_routers();
    lab.cable(r1, "g0/2", r2, "g0/2");
    lab.run(r1, "conf t\nint g0/2\nip address 10.0.1.1 255.255.255.252\nno shut\nend");
    lab.run(r2, "conf t\nint g0/2\nip address 10.0.1.2 255.255.255.252\nno shut\nint lo0\nip address 192.168.2.1 255.255.255.0\nend");
    lab.run(r1, "conf t\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nexit\nip route 192.168.2.0 255.255.255.0 10.0.1.2 130\nip route 0.0.0.0 0.0.0.0 10.0.1.2 250\nend");
    lab.run(r2, "conf t\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nnetwork 192.168.2.0 0.0.0.255 area 0\nend");
    // OSPF (110) wins over the static route of distance 130, which stays out of the table.
    assert!(lab.run(r1, "show ip route").contains("O    192.168.2.1/32"));
    lab.run(r2, "conf t\nint lo0\nip ospf network point-to-point\nend");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O    192.168.2.0/24       via 10.0.0.2, GigabitEthernet0/1 (OSPF) [110/2]"), "{rt}");
    assert!(!rt.contains("S    192.168.2.0/24"), "{rt}");
    // The floating default is in (nothing else knows 0.0.0.0/0), with its distance.
    assert!(rt.contains("S    0.0.0.0/0            via 10.0.1.2 [250/0]"), "{rt}");
    assert!(lab.run(r1, "show ip route static").contains("0.0.0.0/0"));
    let tr = lab.run(r1, "traceroute 192.168.2.1");
    assert!(tr.contains("10.0.0.2") || tr.contains("192.168.2.1"), "{tr}");
    // The OSPF link fails: the floating route takes over.
    lab.run(r1, "conf t\nint g0/1\nshutdown\nend");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("S    192.168.2.0/24       via 10.0.1.2 [130/0]"), "{rt}");
    assert!(lab.run(r1, "ping 192.168.2.1").contains('!'));
    // Back up: OSPF again.
    lab.run(r1, "conf t\nint g0/1\nno shutdown\nend");
    assert!(lab.run(r1, "show ip route").contains("O    192.168.2.0/24"));
    // Typing the route again with another distance replaces it; a bad distance is refused.
    lab.run(r1, "conf t\nip route 192.168.2.0 255.255.255.0 10.0.1.2 140\nend");
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "ip route 10.9.0.0 255.255.0.0 10.0.1.2 0").contains("distance is 1-255"));
    // IPv6 alike: the route of distance 200 waits behind the one of distance 1.
    lab.run(r1, "ipv6 unicast-routing\nint g0/2\nipv6 address 2001:db8:12::1/64\nexit\nipv6 route 2001:db8:99::/64 2001:db8:12::2 200\nend");
    assert!(lab.run(r1, "show ipv6 route").contains("2001:db8:12::2"));
    lab.run(r1, "conf t\nipv6 route 2001:db8:99::/64 2001:db8:12::3\nend");
    let rt6 = lab.run(r1, "show ipv6 route");
    assert!(rt6.contains("2001:db8:12::3") && !rt6.contains("2001:db8:12::2"), "{rt6}");
    lab.run(r1, "conf t\nno ipv6 route 2001:db8:99::/64 2001:db8:12::3\nend");
    lab.run(r1, "conf t\nip route 172.20.0.0 255.255.0.0 g0/2 150\nend");
    let run = survives_reload(&mut lab, r1);
    assert!(run.contains("ip route 0.0.0.0 0.0.0.0 10.0.1.2 250"), "{run}");
    assert!(run.contains("ip route 172.20.0.0 255.255.0.0 GigabitEthernet0/2 150"), "{run}");
    assert!(run.contains("ip route 192.168.2.0 255.255.255.0 10.0.1.2 140") && !run.contains(" 10.0.1.2 130"), "{run}");
    assert!(run.contains("ipv6 route 2001:db8:99::/64 2001:db8:12::2 200"), "{run}");
}

#[test]
fn ospf_dr_election_waits_for_clear_ip_ospf_process() {
    let mut lab = Lab::new();
    let sw = lab.add(DeviceKind::Switch);
    let mut rs = Vec::new();
    for n in 1..=3u8 {
        let r = lab.add(DeviceKind::Router);
        lab.cable(r, "g0/0", sw, &format!("fa0/{n}"));
        lab.run(r, &format!("en\nconf t\nint g0/0\nip address 10.0.0.{n} 255.255.255.0\nno shut\nip ospf 1 area 0\nexit\nrouter ospf 1\nrouter-id {n}.{n}.{n}.{n}\nend"));
        rs.push(r);
    }
    let (r1, r2, r3) = (rs[0], rs[1], rs[2]);
    let nb = lab.run(r1, "show ip ospf neighbor");
    assert!(nb.contains("3.3.3.3          FULL/DR"), "{nb}");
    // A higher priority doesn't take the DR role over at once...
    lab.run(r1, "conf t\nint g0/0\nip ospf priority 255\nend");
    assert!(lab.run(r1, "show ip ospf neighbor").contains("3.3.3.3          FULL/DR"));
    // ...only once OSPF restarts.
    let out = lab.run(r1, "clear ip ospf process");
    assert!(out.contains("restarted"), "{out}");
    let nb = lab.run(r3, "show ip ospf neighbor");
    assert!(nb.contains("1.1.1.1          FULL/DR"), "{nb}");
    // When the DR goes, the BDR takes over.
    lab.run(r1, "conf t\nint g0/0\nshutdown\nend");
    let nb = lab.run(r2, "show ip ospf neighbor");
    assert!(nb.contains("3.3.3.3          FULL/DR"), "{nb}");
    assert!(lab.try_run(r1, "clear ip ospf 9 process").contains("No OSPF process 9"));
}
