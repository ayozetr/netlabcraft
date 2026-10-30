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
