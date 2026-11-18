//! Multiple spanning tree: the configuration mode, the region, a tree per instance, the BPDUs on
//! the wire and the edge of a region.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_wire::{Frame, mst::MstBpdu};

use super::Lab;

const REGION: &str =
    "spanning-tree mode mst\nspanning-tree mst configuration\nname LAB\nrevision 3\ninstance 1 vlan 10,20\ninstance 2 vlan 30\nexit\n";

/// A ring of three switches in one region; S1 roots instance 1 and S2 roots instance 2.
fn ring() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(s1, "g0/2", s3, "g0/1");
    lab.cable(s2, "g0/2", s3, "g0/2");
    for s in [s1, s2, s3] {
        lab.run(s, "en\nconf t\ninterface range g0/1 - 2\nswitchport mode trunk\nexit\nvlan 10\nvlan 20\nvlan 30\nend");
        lab.run(s, &format!("conf t\n{REGION}end"));
    }
    lab.run(s1, "conf t\nspanning-tree mst 0 root primary\nspanning-tree mst 1 root primary\nend");
    lab.run(s2, "conf t\nspanning-tree mst 2 root primary\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    (lab, s1, s2, s3)
}

/// The ports a switch blocks in one instance, as `device/port` words.
fn blocking(lab: &mut Lab, s: DeviceId, inst: u16) -> Vec<String> {
    let shown = lab.run(s, &format!("show spanning-tree mst {inst}"));
    shown.lines().filter(|l| l.contains(" BLK ")).map(|l| format!("{s:?}/{}", l.split_whitespace().next().unwrap_or(""))).collect()
}

#[test]
fn the_configuration_mode_edits_the_region_and_replays() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    lab.run(s1, "en\nconf t\nspanning-tree mode mst\nspanning-tree mst configuration");
    assert!(lab.prompt(s1).ends_with("(config-mst)#"), "{}", lab.prompt(s1));
    lab.run(s1, "name LAB\nrevision 3\ninstance 1 vlan 10,20\ninstance 2 vlan 30\nexit");
    assert!(lab.prompt(s1).ends_with("(config)#"), "{}", lab.prompt(s1));
    lab.run(s1, "spanning-tree mst 1 priority 24576\nend");
    let cfg = lab.run(s1, "show spanning-tree mst configuration");
    for want in ["Region name:  LAB", "Revision:     3", "1         10,20", "2         30", "Instances:    2"] {
        assert!(cfg.contains(want), "{want}\n{cfg}");
    }
    assert!(cfg.contains("0         1-9,11-19,21-29,31-4094"), "{cfg}");
    let run = lab.run(s1, "show running-config");
    for want in [
        "spanning-tree mode mst\n",
        "spanning-tree mst 1 priority 24576\n",
        "spanning-tree mst configuration\n name LAB\n revision 3\n instance 1 vlan 10,20\n instance 2 vlan 30\n",
    ] {
        assert!(run.contains(want), "{want}\n{run}");
    }
    // A VLAN is in one instance: mapping it again moves it.
    lab.run(s1, "conf t\nspanning-tree mst configuration\ninstance 2 vlan 20\nend");
    let cfg = lab.run(s1, "show spanning-tree mst configuration");
    assert!(cfg.contains("1         10\n") && cfg.contains("2         20,30"), "{cfg}");
    lab.run(s1, "conf t\nspanning-tree mst configuration\nno instance 2\nend");
    assert!(lab.run(s1, "show spanning-tree mst configuration").contains("Instances:    1"));
    // The digest depends on the table alone.
    let a = lab.run(s1, "show spanning-tree mst configuration");
    lab.run(s1, "conf t\nspanning-tree mst configuration\nname OTHER\nend");
    let b = lab.run(s1, "show spanning-tree mst configuration");
    let digest = |s: &str| s.lines().find(|l| l.starts_with("Digest:")).unwrap_or("").to_string();
    assert_eq!(digest(&a), digest(&b));
    lab.run(s1, "conf t\nno spanning-tree mst configuration\nend");
    assert!(lab.run(s1, "show spanning-tree mst configuration").contains("Instances:    0"));
}

#[test]
fn bad_values_are_refused() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    lab.run(s1, "en\nconf t\nspanning-tree mst configuration");
    for line in ["instance 16 vlan 10", "instance 0 vlan 10", "instance 1 vlan 5000", "name has space", "revision 70000", "no instance 99"] {
        assert!(lab.try_run(s1, line).starts_with("% ") || lab.try_run(s1, line).contains('^'), "{line}");
    }
    lab.run(s1, "exit");
    assert!(lab.try_run(s1, "spanning-tree mst 1 priority 100").starts_with("% "));
    assert!(lab.try_run(s1, "spanning-tree mst 1 root best").starts_with("% "));
    assert!(lab.try_run(s1, "spanning-tree mode bogus").starts_with("% "));
    // Without MST the show says so.
    lab.run(s1, "spanning-tree mode rapid-pvst\nend");
    assert!(lab.run(s1, "show spanning-tree mst").contains("not running"));
}

#[test]
fn each_instance_has_its_own_root_and_its_own_blocked_link() {
    let (mut lab, s1, s2, s3) = ring();
    assert!(lab.run(s1, "show spanning-tree mst 1").contains("Root:   this switch"));
    assert!(lab.run(s2, "show spanning-tree mst 2").contains("Root:   this switch"));
    assert!(!lab.run(s2, "show spanning-tree mst 1").contains("Root:   this switch"));
    assert!(lab.run(s3, "show spanning-tree mst 1").contains("path cost"));
    // One port blocks in a ring, and not the same one for both instances.
    let one: Vec<String> = [s1, s2, s3].iter().flat_map(|s| blocking(&mut lab, *s, 1)).collect();
    let two: Vec<String> = [s1, s2, s3].iter().flat_map(|s| blocking(&mut lab, *s, 2)).collect();
    assert_eq!(one.len(), 1, "{one:?}");
    assert_eq!(two.len(), 1, "{two:?}");
    assert_ne!(one, two);
    // The summary names the mode, and every instance is listed.
    assert!(lab.run(s1, "show spanning-tree summary").contains("Mode:                 mst"));
    let all = lab.run(s1, "show spanning-tree mst");
    assert!(all.contains("MST0") && all.contains("MST1") && all.contains("MST2"), "{all}");
}

#[test]
fn a_per_instance_cost_and_port_priority_move_the_root_port() {
    let (mut lab, s1, _s2, s3) = ring();
    let before = lab.run(s3, "show spanning-tree mst 1");
    // S3 reaches S1 directly (g0/1); make that link dear for instance 1 and the path via S2 wins.
    lab.run(s3, "conf t\ninterface g0/1\nspanning-tree mst 1 cost 1000\nspanning-tree mst 1 port-priority 64\nend");
    let after = lab.run(s3, "show spanning-tree mst 1");
    assert_ne!(before, after);
    assert!(lab.run(s3, "show running-config").contains(" spanning-tree mst 1 cost 1000\n spanning-tree mst 1 port-priority 64\n"));
    // Instance 2 does not see the setting.
    assert!(!lab.run(s3, "show spanning-tree mst 2").contains("1000"));
    lab.run(s3, "conf t\ninterface g0/1\nno spanning-tree mst 1 cost\nno spanning-tree mst 1 port-priority\nend");
    assert_eq!(lab.run(s3, "show spanning-tree mst 1"), before);
    let _ = s1;
}

#[test]
fn mst_bpdus_carry_the_region_and_a_record_per_instance() {
    let (mut lab, s1, ..) = ring();
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 10_000_000);
    let mine: Vec<MstBpdu> = lab
        .sim
        .trace()
        .iter()
        .filter(|e| e.from.device == s1 && e.summary.starts_with("MST BPDU"))
        .filter_map(|e| Frame::decode(&e.frame).ok())
        .filter_map(|f| MstBpdu::decode(&f.payload).ok())
        .collect();
    assert!(!mine.is_empty());
    let b = &mine[0];
    assert_eq!((b.config.name.as_str(), b.config.revision, b.msti.len()), ("LAB", 3, 2));
    assert_eq!(
        b.config.digest,
        netlabcraft_wire::mst::config_digest(|v| match v {
            10 | 20 => 1,
            30 => 2,
            _ => 0,
        })
    );
    // No per-VLAN BPDUs from a switch running MST.
    assert!(!lab.sim.trace().iter().any(|e| e.from.device == s1 && e.summary.starts_with("RSTP BPDU")));
}

#[test]
fn the_edge_of_a_region_is_a_boundary_and_the_inside_is_internal() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    let s4 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(s2, "g0/2", s3, "g0/1");
    lab.cable(s2, "fa0/3", s4, "fa0/3");
    for s in [s1, s2, s3, s4] {
        lab.run(s, "en\nconf t\ninterface range g0/1 - 2\nswitchport mode trunk\nexit\nvlan 10\nend");
    }
    for s in [s1, s2] {
        lab.run(s, &format!("conf t\n{REGION}end"));
    }
    lab.run(s3, "conf t\nspanning-tree mode mst\nspanning-tree mst configuration\nname OTHER\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    let shown = lab.run(s2, "show spanning-tree mst 0");
    assert!(shown.contains("Internal") && shown.contains("Boundary(MST)"), "{shown}");
    // A switch on classic spanning tree is a boundary too.
    lab.run(s4, "conf t\ninterface fa0/3\nswitchport mode trunk\nend");
    lab.run(s2, "conf t\ninterface fa0/3\nswitchport mode trunk\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    assert!(lab.run(s2, "show spanning-tree mst 0").contains("Boundary(STP)"));
    // Regions do not match: the digests and names differ, the same-region test says so.
    assert!(!lab.t.device(s2).unwrap().mst.same_region(&lab.t.device(s3).unwrap().mst));
    assert!(lab.t.device(s1).unwrap().mst.same_region(&lab.t.device(s2).unwrap().mst));
}
