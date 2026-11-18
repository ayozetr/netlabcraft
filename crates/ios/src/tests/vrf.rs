//! VRF-lite typed at the console: the definitions, interfaces that belong to a VRF, separate
//! routing and ARP tables, static routes, OSPF and BGP inside a VRF, and isolation.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// Two routers joined by two cables, each cable a different VRF that uses the same addresses;
/// a PC behind each router in the RED VRF.
fn pair() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let pc1 = lab.add(DeviceKind::Pc);
    let pc2 = lab.add(DeviceKind::Pc);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r1, "g0/2", r2, "g0/2");
    lab.cable(pc1, "fa0", r1, "g0/0");
    lab.cable(pc2, "fa0", r2, "g0/0");
    for (r, lan, wire) in [(r1, "192.168.1.1", "10.0.12.1"), (r2, "192.168.3.1", "10.0.12.2")] {
        lab.run(
            r,
            &format!(
                "en\nconf t\nip vrf RED\nrd 65000:1\nroute-target export 65000:1\nroute-target import 65000:1\nexit\nip vrf BLUE\nrd 65000:2\nexit\n\
                 int g0/0\nip vrf forwarding RED\nip address {lan} 255.255.255.0\nno shut\n\
                 int g0/1\nip vrf forwarding RED\nip address {wire} 255.255.255.252\nno shut\n\
                 int g0/2\nip vrf forwarding BLUE\nip address {wire} 255.255.255.252\nno shut\nend"
            ),
        );
    }
    lab.run(r1, "conf t\nip route vrf RED 192.168.3.0 255.255.255.0 10.0.12.2\nend");
    lab.run(r2, "conf t\nip route vrf RED 192.168.1.0 255.255.255.0 10.0.12.1\nend");
    lab.run(pc1, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(pc2, "ipconfig 192.168.3.10 255.255.255.0 192.168.3.1");
    (lab, r1, r2, pc1, pc2)
}

#[test]
fn a_vrf_is_defined_shown_and_replayed() {
    let (mut lab, r1, ..) = pair();
    assert_eq!(lab.prompt(r1), "R1#");
    lab.run(r1, "conf t\nip vrf RED");
    assert_eq!(lab.prompt(r1), "R1(config-vrf)#");
    lab.run(r1, "description the red network\nend");
    let brief = lab.run(r1, "show ip vrf");
    assert!(brief.contains("RED") && brief.contains("65000:1") && brief.contains("Gi0/0, Gi0/1"), "{brief}");
    let detail = lab.run(r1, "show ip vrf detail");
    assert!(detail.contains("Export route targets: 65000:1") && detail.contains("the red network"), "{detail}");
    let run = lab.run(r1, "show running-config");
    assert!(
        run.contains("ip vrf RED\n description the red network\n rd 65000:1\n route-target export 65000:1\n route-target import 65000:1\n"),
        "{run}"
    );
    assert!(run.contains("interface GigabitEthernet0/0\n ip vrf forwarding RED\n ip address 192.168.1.1 255.255.255.0"), "{run}");
    assert!(run.contains("ip route vrf RED 192.168.3.0 255.255.255.0 10.0.12.2"), "{run}");
    // The same route distinguisher cannot name two VRFs.
    lab.run(r1, "conf t\nip vrf BLUE");
    assert!(lab.try_run(r1, "rd 65000:1").starts_with("% "));
    assert!(lab.try_run(r1, "route-target both nonsense").starts_with("% "));
    lab.run(r1, "end");
    // Replaying the configuration on a fresh router gives the same configuration.
    let text = lab.run(r1, "show running-config");
    let mut other = Lab::new();
    let r = other.add(DeviceKind::Router);
    let _ = crate::exec::apply_config(&mut other.t, &mut other.sim, r, &text);
    let again = other.run(r, "en\nshow running-config");
    assert!(again.contains("ip vrf forwarding RED") && again.contains("ip route vrf RED 192.168.3.0"), "{again}");
}

#[test]
fn changing_vrf_removes_the_address_and_the_global_table_does_not_see_it() {
    let (mut lab, r1, ..) = pair();
    let global = lab.run(r1, "show ip route");
    assert!(!global.contains("192.168.1.0") && !global.contains("10.0.12.0"), "{global}");
    let red = lab.run(r1, "show ip route vrf RED");
    assert!(red.contains("Routing Table: RED") && red.contains("192.168.1.0/24") && red.contains("S    192.168.3.0/24"), "{red}");
    let blue = lab.run(r1, "show ip route vrf BLUE");
    assert!(blue.contains("10.0.12.0/30") && !blue.contains("192.168"), "{blue}");
    lab.run(r1, "conf t\nint g0/0");
    let out = lab.run(r1, "ip vrf forwarding BLUE");
    assert!(out.contains("IP address removed"), "{out}");
    assert!(lab.run(r1, "ip vrf forwarding RED").is_empty());
    assert!(lab.try_run(r1, "ip vrf forwarding NOPE").starts_with("% VRF NOPE"));
    lab.run(r1, "no ip vrf forwarding\nend");
    assert!(!lab.run(r1, "show running-config").contains("interface GigabitEthernet0/0\n ip vrf"));
}

#[test]
fn hosts_reach_each_other_through_the_vrf_and_the_tables_are_apart() {
    let (mut lab, r1, _r2, pc1, _pc2) = pair();
    let out = lab.run(pc1, "ping 192.168.3.10");
    assert!(out.contains("seq 2: reply from 192.168.3.10"), "{out}");
    // Each VRF has its own ARP table, and the same address is not the same neighbour.
    let red = lab.run(r1, "show ip arp vrf RED");
    assert!(red.contains("192.168.1.10") && red.contains("10.0.12.2"), "{red}");
    let blue = lab.run(r1, "show ip arp vrf BLUE");
    assert!(!blue.contains("192.168.1.10") && !blue.contains("10.0.12.2"), "{blue}");
    assert!(!lab.run(r1, "show arp").contains("192.168.1.10"));
    // Both wires use 10.0.12.0/30 and answer on their own VRF only.
    let ok = lab.run(r1, "ping vrf RED 10.0.12.2");
    assert!(ok.contains("!!!!"), "{ok}");
    let ok = lab.run(r1, "ping vrf BLUE 10.0.12.2");
    assert!(ok.contains("!!!!"), "{ok}");
    let blue = lab.run(r1, "show ip arp vrf BLUE");
    assert!(blue.contains("10.0.12.2"), "{blue}");
    // The global table has no such route.
    assert!(!lab.try_run(r1, "ping 10.0.12.2").contains("!!!"));
}

#[test]
fn a_vrf_cannot_reach_what_only_another_vrf_knows() {
    let (mut lab, r1, ..) = pair();
    // The BLUE table has no route to the RED LAN behind R2, although the cable is the same kind.
    let out = lab.try_run(r1, "ping vrf BLUE 192.168.3.10");
    assert!(!out.contains("!") || out.starts_with("% "), "{out}");
    assert!(!out.contains("!!!"), "{out}");
    // Cutting the RED wire stops RED and leaves BLUE alone.
    lab.run(r1, "conf t\nint g0/1\nshutdown\nend");
    let red = lab.try_run(r1, "ping vrf RED 10.0.12.2");
    assert!(!red.contains("!!!"), "{red}");
    let blue = lab.run(r1, "ping vrf BLUE 10.0.12.2");
    assert!(blue.contains("!!!!"), "{blue}");
    assert!(lab.try_run(r1, "ping vrf GREEN 10.0.12.2").starts_with("% VRF GREEN"));
}

#[test]
fn ospf_runs_inside_a_vrf() {
    let (mut lab, r1, r2, ..) = pair();
    for (r, lo) in [(r1, "1.1.1.1"), (r2, "2.2.2.2")] {
        lab.run(
            r,
            &format!(
                "conf t\ninterface Loopback1\nip vrf forwarding BLUE\nip address {lo} 255.255.255.255\nexit\n\
                 router ospf 10 vrf BLUE\nnetwork 10.0.12.0 0.0.0.3 area 0\nnetwork {lo} 0.0.0.0 area 0\nend"
            ),
        );
    }
    assert_eq!(lab.run(r1, "conf t\nrouter ospf 10 vrf BLUE"), "");
    assert_eq!(lab.prompt(r1), "R1(config-router)#");
    lab.run(r1, "end");
    lab.sim.run_for(&lab.t, 60_000_000);
    let blue = lab.run(r1, "show ip route vrf BLUE");
    assert!(blue.lines().any(|l| l.starts_with("O ") && l.contains("2.2.2.2/32")), "{blue}");
    let neighbours = lab.run(r1, "show ip ospf neighbor vrf BLUE");
    assert!(neighbours.contains("2.2.2.2"), "{neighbours}");
    // Not in the global table, not in the other VRF.
    assert!(!lab.run(r1, "show ip route").contains("2.2.2.2"));
    assert!(!lab.run(r1, "show ip route vrf RED").contains("2.2.2.2"));
    let ping = lab.run(r1, "ping vrf BLUE 2.2.2.2");
    assert!(ping.contains("!!!!"), "{ping}");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("router ospf 10 vrf BLUE\n"), "{run}");
    assert!(run.contains(" network 10.0.12.0 0.0.0.3 area 0"), "{run}");
    // The global router has no process of its own.
    assert!(lab.t.device(r1).unwrap().ospf.is_none());
    assert!(lab.try_run(r1, "conf t\nrouter ospf 11 vrf BLUE").starts_with("% "));
}

#[test]
fn bgp_runs_in_a_vrf_address_family() {
    let (mut lab, r1, r2, ..) = pair();
    lab.run(
        r1,
        "conf t\nrouter bgp 65001\nbgp router-id 1.1.1.1\naddress-family ipv4 vrf BLUE\nneighbor 10.0.12.2 remote-as 65002\n\
         network 10.0.12.0 mask 255.255.255.252\nexit-address-family\nend",
    );
    lab.run(
        r2,
        "conf t\nrouter bgp 65002\nbgp router-id 2.2.2.2\naddress-family ipv4 vrf BLUE\nneighbor 10.0.12.1 remote-as 65001\n\
         network 10.0.12.0 mask 255.255.255.252\nexit-address-family\nend",
    );
    assert!(lab.t.device(r1).unwrap().bgp.as_ref().unwrap().neighbors.is_empty());
    lab.run(r1, "conf t\nrouter bgp 65001\naddress-family ipv4 vrf BLUE");
    assert_eq!(lab.prompt(r1), "R1(config-router-af)#");
    lab.run(r1, "exit-address-family");
    assert_eq!(lab.prompt(r1), "R1(config-router)#");
    lab.run(r1, "end");
    lab.sim.run_for(&lab.t, 30_000_000);
    let summary = lab.run(r1, "show ip bgp vrf BLUE summary");
    assert!(summary.contains("10.0.12.2") && !summary.contains("Connect") && !summary.contains("Idle"), "{summary}");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains(" address-family ipv4 vrf BLUE\n"), "{run}");
    assert!(run.contains("neighbor 10.0.12.2 remote-as 65002"), "{run}");
}

#[test]
fn deleting_a_vrf_returns_its_interfaces() {
    let (mut lab, r1, ..) = pair();
    lab.run(r1, "conf t\nno ip vrf BLUE\nend");
    assert!(!lab.run(r1, "show ip vrf").contains("BLUE"));
    assert!(lab.t.device(r1).unwrap().ports[2].ip.is_none());
    assert!(lab.try_run(r1, "show ip route vrf BLUE").starts_with("% VRF BLUE"));
}
