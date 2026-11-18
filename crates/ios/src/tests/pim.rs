//! Multicast routing: `ip multicast-routing`, PIM neighbours and modes, rendezvous points, the
//! trees receivers build, and data forwarded over them.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// What crossed the cables, and why each frame was dropped (for a failing assertion).
fn dump(lab: &Lab) -> String {
    lab.sim
        .trace()
        .iter()
        .map(|e| format!("{:?} -> {:?}: {} [{}]", e.from.device, e.to.device, e.summary, e.dropped.clone().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("\n")
}

struct Net {
    lab: Lab,
    r1: DeviceId,
    r2: DeviceId,
    src: DeviceId,
    rcv: DeviceId,
}

/// SRC (10.1.1.10) - R1 - R2 - RCV (10.2.2.10); R1's loopback 1.1.1.1 is the rendezvous point.
/// Static routes carry the unicast traffic; `mode` is the PIM mode typed on every link.
fn net(mode: &str) -> Net {
    let mut lab = Lab::new();
    let src = lab.add(DeviceKind::Pc);
    let rcv = lab.add(DeviceKind::Pc);
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(src, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/0", rcv, "fa0");
    lab.run(src, "ipconfig 10.1.1.10 255.255.255.0 10.1.1.1");
    lab.run(rcv, "ipconfig 10.2.2.10 255.255.255.0 10.2.2.1");
    lab.run(
        r1,
        "en\nconf t\nhostname R1\ninterface g0/0\nip address 10.1.1.1 255.255.255.0\nno shutdown\nexit\ninterface g0/1\nip address 10.0.12.1 255.255.255.0\nno shutdown\nexit\ninterface loopback0\nip address 1.1.1.1 255.255.255.255\nexit\nip route 10.2.2.0 255.255.255.0 10.0.12.2\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nhostname R2\ninterface g0/0\nip address 10.2.2.1 255.255.255.0\nno shutdown\nexit\ninterface g0/1\nip address 10.0.12.2 255.255.255.0\nno shutdown\nexit\nip route 10.1.1.0 255.255.255.0 10.0.12.1\nip route 1.1.1.1 255.255.255.255 10.0.12.1\nend",
    );
    for r in [r1, r2] {
        lab.run(
            r,
            &format!(
                "conf t\nip multicast-routing\nip pim rp-address 1.1.1.1\ninterface g0/0\nip pim {mode}\nexit\ninterface g0/1\nip pim {mode}\nend"
            ),
        );
    }
    lab.run(r1, "conf t\ninterface loopback0\nip pim sparse-mode\nend");
    Net { lab, r1, r2, src, rcv }
}

#[test]
fn neighbours_interfaces_and_the_rendezvous_point_are_listed() {
    let mut n = net("sparse-mode");
    let nb1 = n.lab.run(n.r1, "show ip pim neighbor");
    assert!(nb1.contains("10.0.12.2") && nb1.contains("GigabitEthernet0/1"), "{nb1}");
    let nb2 = n.lab.run(n.r2, "show ip pim neighbor");
    assert!(nb2.contains("10.0.12.1"), "{nb2}");
    // The router with the higher address is the designated router of the shared link.
    assert!(nb1.contains("(designated router)") && !nb2.contains("(designated router)"), "{nb1}\n{nb2}");
    let ifs = n.lab.run(n.r1, "show ip pim interface");
    assert!(ifs.contains("Sparse") && ifs.contains("10.1.1.1") && ifs.contains("Loopback0"), "{ifs}");
    let rp = n.lab.run(n.r2, "show ip pim rp mapping");
    assert!(rp.contains("1.1.1.1") && rp.contains("all groups"), "{rp}");
    let run = n.lab.run(n.r1, "show running-config");
    assert!(run.contains("\nip multicast-routing\n") && run.contains("ip pim rp-address 1.1.1.1") && run.contains(" ip pim sparse-mode"), "{run}");
}

#[test]
fn hellos_cross_the_wire() {
    let mut n = net("sparse-mode");
    n.lab.sim.run_for(&n.lab.t, 61_000_000);
    let hello = n.lab.sim.trace().iter().any(|e| e.summary.starts_with("PIM hello"));
    assert!(hello, "{:?}", n.lab.sim.trace().iter().map(|e| e.summary.clone()).collect::<Vec<_>>());
}

#[test]
fn a_receiver_builds_the_shared_tree_and_data_follows_the_source_tree() {
    let mut n = net("sparse-mode");
    n.lab.run(n.rcv, "igmp join 239.1.1.1");
    let groups = n.lab.run(n.r2, "show ip igmp groups");
    assert!(groups.contains("239.1.1.1") && groups.contains("GigabitEthernet0/0") && groups.contains("10.2.2.10"), "{groups}");
    let m2 = n.lab.run(n.r2, "show ip mroute");
    assert!(m2.contains("(*, 239.1.1.1)") && m2.contains("RP 1.1.1.1"), "{m2}");
    assert!(m2.contains("Incoming interface: GigabitEthernet0/1") && m2.contains("GigabitEthernet0/0, Forward/Sparse"), "{m2}");
    let m1 = n.lab.run(n.r1, "show ip mroute");
    assert!(m1.contains("(*, 239.1.1.1)") && m1.contains("GigabitEthernet0/1, Forward/Sparse") && m1.contains("Incoming interface: Null"), "{m1}");
    // The source sends: every member answers.
    let ping = n.lab.run(n.src, "ping -n 1 239.1.1.1");
    assert!(ping.contains("reply from 10.2.2.10"), "{ping}\n{}", dump(&n.lab));
    let m1 = n.lab.run(n.r1, "show ip mroute");
    assert!(m1.contains("(10.1.1.10, 239.1.1.1)") && m1.contains("the source is on this link"), "{m1}");
    let m2 = n.lab.run(n.r2, "show ip mroute 239.1.1.1");
    assert!(m2.contains("(10.1.1.10, 239.1.1.1)") && m2.contains("RPF neighbour 10.0.12.1"), "{m2}");
    let count = n.lab.run(n.r2, "show ip mroute count");
    assert!(count.contains("Source 10.1.1.10") && count.contains("1 packets forwarded"), "{count}");
    let summary = n.lab.run(n.r2, "show ip mroute summary");
    assert!(summary.contains("1 (*,G) entry and 1 (S,G) entry"), "{summary}");
    // A leave prunes the tree.
    n.lab.run(n.rcv, "igmp leave 239.1.1.1");
    assert!(
        n.lab.run(n.r2, "show ip mroute").contains("The multicast routing table is empty")
            || !n.lab.run(n.r2, "show ip mroute").contains("(*, 239.1.1.1)")
    );
    assert!(n.lab.run(n.src, "ping -n 1 239.1.1.1").contains("no answer"));
    n.lab.run(n.r2, "clear ip mroute *");
    assert!(n.lab.run(n.r2, "show ip mroute count").contains("No multicast traffic"));
}

#[test]
fn a_source_with_no_receiver_is_pruned_at_its_first_router() {
    let mut n = net("sparse-mode");
    assert!(n.lab.run(n.src, "ping -n 1 239.1.1.1").contains("no answer"));
    let m1 = n.lab.run(n.r1, "show ip mroute");
    assert!(m1.contains("(10.1.1.10, 239.1.1.1)") && m1.contains("flags: STFP") && m1.contains("Outgoing interface list: Null"), "{m1}");
    assert!(n.lab.run(n.r2, "show ip mroute").contains("table is empty"));
}

#[test]
fn without_a_rendezvous_point_sparse_mode_carries_nothing() {
    let mut n = net("sparse-mode");
    n.lab.run(n.rcv, "igmp join 239.1.1.1");
    n.lab.run(n.r1, "conf t\nno ip pim rp-address 1.1.1.1\nend");
    n.lab.run(n.r2, "conf t\nno ip pim rp-address 1.1.1.1\nend");
    assert!(n.lab.run(n.src, "ping -n 1 239.1.1.1").contains("no answer"));
    assert!(n.lab.run(n.r2, "show ip pim rp mapping").contains("No rendezvous point"));
    assert!(n.lab.run(n.r2, "show ip mroute").contains("table is empty"));
}

#[test]
fn a_link_without_pim_breaks_the_tree() {
    let mut n = net("sparse-mode");
    n.lab.run(n.rcv, "igmp join 239.1.1.1");
    n.lab.run(n.r2, "conf t\ninterface g0/1\nno ip pim sparse-mode\nend");
    assert!(n.lab.run(n.src, "ping -n 1 239.1.1.1").contains("no answer"));
    assert!(n.lab.run(n.r1, "show ip pim neighbor").contains("No PIM neighbours"));
}

#[test]
fn dense_mode_floods_where_there_are_receivers() {
    let mut n = net("dense-mode");
    n.lab.run(n.rcv, "igmp join 239.5.5.5");
    let ping = n.lab.run(n.src, "ping -n 1 239.5.5.5");
    assert!(ping.contains("reply from 10.2.2.10"), "{ping}\n{}", dump(&n.lab));
    let m2 = n.lab.run(n.r2, "show ip mroute");
    assert!(m2.contains("(10.1.1.10, 239.5.5.5)") && m2.contains("Forward/Dense"), "{m2}");
    assert!(!m2.contains("(*, 239.5.5.5)"), "{m2}");
    assert!(n.lab.run(n.r1, "show ip pim interface").contains("Dense"));
}

#[test]
fn a_router_interface_can_join_a_group() {
    let mut n = net("sparse-mode");
    n.lab.run(n.r2, "conf t\ninterface g0/0\nip igmp join-group 239.9.9.9\nend");
    let ping = n.lab.run(n.src, "ping -n 1 239.9.9.9");
    assert!(ping.contains("reply from 10.0.12.2"), "{ping}\n{}", dump(&n.lab));
    assert!(n.lab.run(n.r2, "show ip igmp groups").contains("this router"));
    assert!(n.lab.run(n.r2, "show running-config").contains(" ip igmp join-group 239.9.9.9"));
}

#[test]
fn the_commands_check_their_arguments_and_say_when_routing_is_off() {
    let mut n = net("sparse-mode");
    n.lab.run(n.r1, "conf t");
    assert!(n.lab.try_run(n.r1, "ip pim rp-address 239.1.1.1").contains("unicast"));
    assert!(n.lab.try_run(n.r1, "ip pim rp-address 0.0.0.0").contains("unicast"));
    assert!(n.lab.try_run(n.r1, "no ip pim rp-address 9.9.9.9").starts_with("% "));
    n.lab.run(n.r1, "interface g0/0");
    assert!(n.lab.try_run(n.r1, "ip pim dr-priority 4294967295").starts_with("% "));
    assert!(n.lab.try_run(n.r1, "ip igmp join-group 224.0.0.5").starts_with("% "));
    n.lab.run(n.r1, "ip pim dr-priority 50\nend");
    assert!(n.lab.run(n.r1, "show running-config").contains(" ip pim dr-priority 50"));
    assert!(n.lab.run(n.r1, "show ip pim interface").contains("50"));
    n.lab.run(n.r1, "conf t\nno ip multicast-routing\nend");
    assert!(n.lab.try_run(n.r1, "show ip mroute").starts_with("% "));
    assert!(n.lab.try_run(n.r1, "show ip pim neighbor").starts_with("% "));
    // The interface lines stay for when routing comes back.
    assert!(n.lab.run(n.r1, "show running-config").contains(" ip pim sparse-mode"));
}
