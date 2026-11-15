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

#[test]
fn summary_totals_keeps_the_settings_and_the_total_row_only() {
    let (mut lab, s1, _s2, _s3) = ring();
    let full = lab.run(s1, "show spanning-tree summary");
    let totals = lab.run(s1, "show spanning-tree summary totals");
    assert!(totals.contains("Mode:") && totals.contains("Root bridge of:"), "{totals}");
    assert!(!totals.lines().any(|l| l.starts_with("VLAN00")), "{totals}");
    // The same total row as the long form, the same counts.
    assert!(totals.lines().last().unwrap_or("").contains(" VLANs"), "{totals}");
    assert_eq!(totals.lines().last(), full.lines().last());
    assert!(full.lines().any(|l| l.starts_with("VLAN0001")) && full.lines().any(|l| l.starts_with("VLAN0020")), "{full}");
}

#[test]
fn a_vlan_brief_is_one_vlans_row_of_the_brief_table() {
    let (mut lab, s1, _s2, _s3) = ring();
    let all = lab.run(s1, "show spanning-tree brief");
    let one = lab.run(s1, "show spanning-tree vlan 1 brief");
    assert_eq!(one.lines().count(), 2, "{one}");
    assert_eq!(one.lines().next(), all.lines().next());
    assert_eq!(one.lines().nth(1), all.lines().find(|l| l.starts_with("VLAN0001")));
    assert!(lab.run(s1, "show spanning-tree vlan 20 brief").contains("VLAN0020"));
    assert!(lab.try_run(s1, "show spanning-tree vlan 77 brief").starts_with("% "));
    assert!(lab.try_run(s1, "show spanning-tree vlan 0 brief").starts_with("% "));
    // The other forms of the command are still there.
    assert!(lab.run(s1, "show spanning-tree vlan 1").contains("VLAN"));
}

#[test]
fn interface_detail_gives_each_vlan_the_roots_the_designated_bridge_and_the_cost() {
    let (mut lab, s1, s2, _s3) = ring();
    let detail = lab.run(s1, "show spanning-tree interface g0/1 detail");
    assert!(detail.starts_with("GigabitEthernet0/1: edge port no"), "{detail}");
    assert!(detail.contains("of VLAN0001 is root, forwarding") && detail.contains("of VLAN0020 is"), "{detail}");
    assert!(detail.contains("Path cost 4 (from speed), port priority 128, port id 128."), "{detail}");
    // S2 is the root of VLAN 1: it is the designated bridge across this cable, at no cost to itself.
    let s2_brief = lab.run(s2, "show spanning-tree vlan 1 brief");
    assert!(s2_brief.contains("(this switch)"), "{s2_brief}");
    let vlan1 = detail.split("of VLAN0020").next().unwrap_or("");
    assert!(vlan1.contains("Designated bridge: priority 24577") && vlan1.contains("path cost to the root 0"), "{detail}");
    assert!(vlan1.contains("Link type point-to-point (from duplex)"), "{detail}");
    // The root's own port is designated here, and says so.
    assert!(lab.run(s2, "show spanning-tree interface g0/1 detail").contains("of VLAN0001 is designated, forwarding"));
    assert!(lab.run(s2, "show spanning-tree interface g0/1 detail").contains("Designated bridge: this switch"));
    // A set cost is marked; the short form is unchanged; bad names are refused.
    lab.run(s1, "conf t\ninterface g0/1\nspanning-tree cost 30\nend");
    assert!(lab.run(s1, "show spanning-tree interface g0/1 detail").contains("Path cost 30 (set)"));
    assert!(lab.run(s1, "show spanning-tree interface g0/1").contains("VLAN0001"));
    assert!(lab.run(s1, "show spanning-tree interface fa0/5 detail").contains("in no spanning tree"));
    assert!(lab.try_run(s1, "show spanning-tree interface bogus detail").starts_with("% "));
    assert!(lab.try_run(s1, "show spanning-tree interface detail").starts_with("% "));
}
