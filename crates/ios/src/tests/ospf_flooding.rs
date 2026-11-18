//! OSPF's adjacency states, database exchange, flooding and retransmission on the wire.

use std::collections::BTreeSet;

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_sim::ospf_flood::NbrState;

use super::Lab;

fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    for (r, n) in [(r1, 1), (r2, 2)] {
        lab.run(
            r,
            &format!(
                "en\nconf t\ninterface g0/0\nip address 10.0.0.{n} 255.255.255.252\nno shutdown\nip ospf network point-to-point\nexit\nrouter ospf 1\nrouter-id {n}.{n}.{n}.{n}\nnetwork 10.0.0.0 0.0.0.3 area 0\nend"
            ),
        );
    }
    (lab, r1, r2)
}

/// The OSPF packet types in the trace, in order (the frames are Ethernet II with a 20-byte IP header).
fn ospf_types(lab: &Lab) -> Vec<u8> {
    lab.sim
        .trace()
        .iter()
        .filter(|e| e.frame.get(12..14) == Some(&[0x08, 0x00]) && e.frame.get(23) == Some(&89))
        .filter_map(|e| e.frame.get(35).copied())
        .collect()
}

#[test]
fn a_neighbour_walks_to_full_with_real_packets() {
    let (mut lab, r1, r2) = pair();
    // Before any packet, the adjacency reads as converged.
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    assert!(lab.sim.ospf_neighbor_state(r1, "GigabitEthernet0/0", "2.2.2.2".parse().unwrap()).is_none());
    lab.sim.run_for(&lab.t, 60_000_000);
    for (r, other) in [(r1, "2.2.2.2"), (r2, "1.1.1.1")] {
        let state = lab.sim.ospf_neighbor_state(r, "GigabitEthernet0/0", other.parse().unwrap());
        assert_eq!(state, Some(NbrState::Full));
    }
    let types = ospf_types(&lab);
    for want in [1u8, 2, 3, 4, 5] {
        assert!(types.contains(&want), "no OSPF packet of type {want} in {types:?}");
    }
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL/P2P"));
}

#[test]
fn what_flooding_installed_is_what_the_database_shows() {
    let (mut lab, r1, r2) = pair();
    lab.sim.run_for(&lab.t, 60_000_000);
    for r in [r1, r2] {
        let held: BTreeSet<_> = lab.sim.ospf_held(r).into_iter().map(|(k, _, _)| k).collect();
        let shown: BTreeSet<_> = lab.sim.lsdb(&lab.t, r).into_iter().map(|l| (l.area, l.kind as u8, l.id, l.adv)).collect();
        assert_eq!(held, shown);
        assert!(!held.is_empty());
    }
}

#[test]
fn an_update_that_is_lost_is_sent_again_after_the_retransmit_interval() {
    let (mut lab, r1, r2) = pair();
    lab.sim.run_for(&lab.t, 30_000_000);
    lab.run(r2, "conf t\ninterface g0/0\nexit\nend");
    lab.sim.ospf_lose_updates(r2, 1);
    lab.run(r1, "conf t\ninterface g0/0\nip ospf cost 7\nend");
    lab.sim.run_for(&lab.t, 1_000_000);
    let n = lab.sim.ospf_neighbor(r1, "GigabitEthernet0/0", "2.2.2.2".parse().unwrap()).unwrap();
    assert_eq!(n.rxmt_sent, 0);
    assert!(n.pending().1 > 0, "the update waits for its acknowledgement");
    lab.sim.run_for(&lab.t, 6_000_000);
    let n = lab.sim.ospf_neighbor(r1, "GigabitEthernet0/0", "2.2.2.2".parse().unwrap()).unwrap();
    assert!(n.rxmt_sent >= 1);
    assert_eq!(n.pending().1, 0, "the second copy was acknowledged");
    let mine = lab.sim.ospf_held(r1).into_iter().find(|(k, ..)| k.3 == "1.1.1.1".parse::<std::net::Ipv4Addr>().unwrap() && k.1 == 1).unwrap().1;
    let theirs = lab.sim.ospf_held(r2).into_iter().find(|(k, ..)| k.3 == "1.1.1.1".parse::<std::net::Ipv4Addr>().unwrap() && k.1 == 1).unwrap().1;
    assert_eq!(mine, theirs);
}

#[test]
fn a_longer_retransmit_interval_waits_longer() {
    let (mut lab, r1, r2) = pair();
    lab.sim.run_for(&lab.t, 30_000_000);
    lab.run(r1, "conf t\ninterface g0/0\nip ospf retransmit-interval 20\nend");
    lab.sim.ospf_lose_updates(r2, 1);
    lab.run(r1, "conf t\ninterface g0/0\nip ospf cost 9\nend");
    lab.sim.run_for(&lab.t, 12_000_000);
    let n = lab.sim.ospf_neighbor(r1, "GigabitEthernet0/0", "2.2.2.2".parse().unwrap()).unwrap();
    assert_eq!(n.rxmt_sent, 0, "20 s have not passed");
    lab.sim.run_for(&lab.t, 12_000_000);
    let n = lab.sim.ospf_neighbor(r1, "GigabitEthernet0/0", "2.2.2.2".parse().unwrap()).unwrap();
    assert!(n.rxmt_sent >= 1);
}
