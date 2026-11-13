//! `show spanning-tree brief|detail|interface X` (M9.7).

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// A ring of three switches on trunks; S2 is the root of VLAN 1.
fn ring() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(s1, "g0/2", s3, "g0/1");
    lab.cable(s2, "g0/2", s3, "g0/2");
    for s in [s1, s2, s3] {
        lab.run(s, "en\nconf t\ninterface range g0/1 - 2\nswitchport mode trunk\nexit\nvlan 20\nend");
    }
    lab.run(s2, "conf t\nspanning-tree vlan 1 root primary\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    (lab, s1, s2, s3)
}

#[test]
fn brief_has_a_row_per_vlan_with_the_root_cost_root_port_and_port_counts() {
    let (mut lab, s1, s2, s3) = ring();
    let root = lab.run(s2, "show spanning-tree brief");
    assert!(root.starts_with("VLAN") && root.contains("(this switch)"), "{root}");
    let row1 = root.lines().find(|l| l.starts_with("VLAN0001")).unwrap_or("").to_string();
    // Root: no root port; both trunks forward.
    assert!(row1.contains("24577") && row1.split_whitespace().rev().take(3).collect::<Vec<_>>() == ["0", "0", "2"], "{root}");
    let other = lab.run(s1, "show spanning-tree brief");
    let row = other.lines().find(|l| l.starts_with("VLAN0001")).unwrap_or("").to_string();
    assert!(row.contains("Gi0/1") && !row.contains("this switch"), "{other}");
    assert!(other.lines().any(|l| l.starts_with("VLAN0020")), "{other}");
    // One of the two non-root switches blocks a port in the ring.
    let blocking =
        [s1, s3].iter().filter(|s| lab.run(**s, "show spanning-tree brief").lines().any(|l| l.starts_with("VLAN0001") && l.ends_with(" 1"))).count();
    assert_eq!(blocking, 1);
}

#[test]
fn detail_lists_every_vlan_port_by_port() {
    let (mut lab, s1, _s2, _s3) = ring();
    let all = lab.run(s1, "show spanning-tree detail");
    assert!(all.contains("VLAN0001 (IEEE 802.1D spanning tree)") && all.contains("VLAN0020"), "{all}");
    assert_eq!(
        all.matches("(port id").count(),
        lab.run(s1, "show spanning-tree vlan 1 detail").matches("(port id").count()
            + lab.run(s1, "show spanning-tree vlan 20 detail").matches("(port id").count()
    );
}

#[test]
fn interface_view_gives_a_ports_role_and_state_per_vlan() {
    let (mut lab, s1, s2, _s3) = ring();
    let root_port = lab.run(s1, "show spanning-tree interface g0/1");
    assert!(root_port.starts_with("GigabitEthernet0/1: edge port no; BPDU guard off; root guard off"), "{root_port}");
    assert!(root_port.contains("VLAN0001   root") && root_port.contains("forwarding") && root_port.contains("VLAN0020"), "{root_port}");
    // The root's ports are designated.
    assert!(lab.run(s2, "show spanning-tree interface gi0/1").contains("designated"));
    // Settings show up: root guard on.
    lab.run(s2, "conf t\ninterface g0/1\nspanning-tree guard root\nend");
    assert!(lab.run(s2, "show spanning-tree interface g0/1").contains("root guard on"));
    // A port with nothing cabled is in no tree; a name that is no port is refused.
    assert!(lab.run(s1, "show spanning-tree interface fa0/5").contains("in no spanning tree"));
    assert!(lab.try_run(s1, "show spanning-tree interface bogus").starts_with("% "));
    assert!(lab.try_run(s1, "show spanning-tree interface").starts_with("% "));
    // Routers have no spanning tree.
    let r = lab.add(DeviceKind::Router);
    assert!(!lab.try_run(r, "show spanning-tree interface g0/0").contains("Role"));
}
