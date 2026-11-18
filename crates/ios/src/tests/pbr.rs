//! Policy-based routing: the route-map lines, `ip policy route-map`, and where packets go.

use netlabcraft_model::DeviceKind;

use super::Lab;

/// PC1 - R1 - {R2, R3}: R1 routes by default through R2; R3 is the other way out.
fn lab() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let pc = lab.add(DeviceKind::Pc);
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(pc, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r1, "g0/2", r3, "g0/0");
    lab.run(pc, "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1");
    lab.run(
        r1,
        "en\nconf t\nint g0/0\nip add 10.0.0.1 255.255.255.0\nno shut\nint g0/1\nip add 192.168.1.1 255.255.255.0\nno shut\nint g0/2\nip add 192.168.2.1 255.255.255.0\nno shut\nexit\nip route 0.0.0.0 0.0.0.0 192.168.1.2\nend",
    );
    lab.run(r2, "en\nconf t\nint g0/0\nip add 192.168.1.2 255.255.255.0\nno shut\nend");
    lab.run(r3, "en\nconf t\nint g0/0\nip add 192.168.2.2 255.255.255.0\nno shut\nend");
    (lab, pc, r1, r2, r3)
}

/// Frames R1 sent to `to`.
fn frames_to(lab: &Lab, from: netlabcraft_core::DeviceId, to: netlabcraft_core::DeviceId) -> usize {
    lab.sim.trace().iter().filter(|e| e.from.device == from && e.to.device == to).count()
}

#[test]
fn the_route_map_lines_are_kept_shown_and_written_back() {
    let (mut lab, _, r1, _, _) = lab();
    lab.run(r1, "conf t\nip access-list extended WEB\npermit tcp any any eq 80\nexit");
    lab.run(r1, "route-map PBR permit 10\nmatch ip address WEB\nmatch length 100 1500\nset ip next-hop 192.168.2.2\nset ip default next-hop 192.168.1.2\nset interface g0/2\nset ip precedence 5\nexit");
    lab.run(r1, "route-map PBR permit 20\nset ip next-hop verify-availability 192.168.2.2 10 track 3\nset ip dscp af21\nexit");
    lab.run(r1, "int g0/0\nip policy route-map PBR\nexit\nip local policy route-map PBR\nend");
    let run = lab.run(r1, "show running-config");
    for want in [
        " match length 100 1500\n",
        " set ip next-hop 192.168.2.2\n",
        " set ip default next-hop 192.168.1.2\n",
        " set interface GigabitEthernet0/2\n",
        " set ip precedence 5\n",
        " set ip next-hop verify-availability 192.168.2.2 10 track 3\n",
        " set ip dscp 18\n",
        " ip policy route-map PBR\n",
        "ip local policy route-map PBR\n",
    ] {
        assert!(run.contains(want), "{want}\n{run}");
    }
    let show = lab.run(r1, "show ip policy");
    assert!(show.contains("GigabitEthernet0/0") && show.contains("PBR permit 10: 0 packets"), "{show}");
    let maps = lab.run(r1, "show route-map PBR");
    assert!(maps.contains("match length: 100-1500 bytes"), "{maps}");
    // Precedence replaces a DSCP and the other way round.
    lab.run(r1, "conf t\nroute-map PBR permit 10\nset ip dscp 8\nend");
    assert!(
        lab.run(r1, "show running-config").contains(" set ip dscp 8\n") && !lab.run(r1, "show running-config").contains(" set ip precedence 5\n")
    );
}

#[test]
fn a_matching_packet_takes_the_policy_next_hop_and_the_others_the_table() {
    let (mut lab, pc, r1, r2, r3) = lab();
    lab.run(r1, "conf t\naccess-list 101 permit icmp host 10.0.0.10 any\nroute-map PBR permit 10\nmatch ip address 101\nset ip next-hop 192.168.2.2\nexit\nint g0/0\nip policy route-map PBR\nend");
    let _ = lab.try_run(pc, "ping 8.8.8.8");
    assert!(frames_to(&lab, r1, r3) > 0, "the echo went to R3 (at least its ARP request)");
    assert!(lab.sim.pbr_hits(r1, "PBR", 10).0 >= 1, "the echo requests matched the clause");
    // Without the policy the same ping goes to R2.
    lab.sim.clear_trace();
    lab.run(r1, "conf t\nint g0/0\nno ip policy route-map\nend");
    let _ = lab.try_run(pc, "ping 8.8.4.4");
    assert!(frames_to(&lab, r1, r2) > 0);
    assert_eq!(frames_to(&lab, r1, r3), 0);
}

#[test]
fn a_deny_clause_and_an_unusable_next_hop_leave_the_packet_to_the_table() {
    let (mut lab, pc, r1, r2, r3) = lab();
    lab.run(r1, "conf t\nroute-map PBR deny 10\nmatch length 0 40\nexit\nroute-map PBR permit 20\nset ip next-hop 172.30.0.9\nexit\nint g0/0\nip policy route-map PBR\nend");
    let _ = lab.try_run(pc, "ping 8.8.8.8");
    assert!(frames_to(&lab, r1, r2) > 0, "172.30.0.9 is on no connected network, so the table decides");
    assert_eq!(frames_to(&lab, r1, r3), 0);
}

#[test]
fn a_default_next_hop_is_used_only_without_a_specific_route() {
    let (mut lab, pc, r1, r2, r3) = lab();
    // The table has only a default route (towards R2), so the policy's default next hop wins.
    lab.run(r1, "conf t\nroute-map PBR permit 10\nset ip default next-hop 192.168.2.2\nexit\nint g0/0\nip policy route-map PBR\nend");
    let _ = lab.try_run(pc, "ping 8.8.8.8");
    assert!(frames_to(&lab, r1, r3) > 0);
    // With a specific route to the destination the table keeps the packet.
    lab.sim.clear_trace();
    lab.run(r1, "conf t\nip route 8.8.0.0 255.255.0.0 192.168.1.2\nend");
    let _ = lab.try_run(pc, "ping 8.8.4.4");
    assert!(frames_to(&lab, r1, r2) > 0);
    assert_eq!(frames_to(&lab, r1, r3), 0);
}

#[test]
fn the_decisions_are_reported_by_debug_ip_policy() {
    let (mut lab, pc, r1, _, _) = lab();
    lab.run(r1, "conf t\nroute-map PBR permit 10\nset ip next-hop 192.168.2.2\nexit\nint g0/0\nip policy route-map PBR\nend\ndebug ip policy");
    let _ = lab.try_run(pc, "ping 8.8.8.8");
    let log = lab.run(r1, "show logging");
    assert!(log.contains("route map PBR sequence 10 matched: next hop 192.168.2.2"), "{log}");
    assert!(lab.run(r1, "show debugging").contains("IP policy routing"));
}
