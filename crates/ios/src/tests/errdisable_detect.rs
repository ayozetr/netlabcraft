//! `errdisable detect cause`: a cause that is not detected leaves the port up.

use netlabcraft_model::DeviceKind;

use super::{Lab, l2_apply};

#[test]
fn a_cause_that_is_not_detected_does_not_shut_the_port() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "fa0/1", s2, "fa0/1");
    // S2 is the root, so its port sends the BPDUs S1's guarded port hears.
    lab.run(s2, "en\nconf t\nspanning-tree vlan 1 priority 4096\nend");
    lab.run(s1, "en\nconf t\ninterface fa0/1\nspanning-tree portfast\nspanning-tree bpduguard enable\nend");
    let shown = lab.run(s1, "show errdisable detect");
    assert!(shown.contains(&format!("{:<20} yes", "bpduguard")), "{shown}");
    // Detection off: BPDUs arrive and the port stays up.
    lab.run(s1, "conf t\nno errdisable detect cause bpduguard\nend");
    assert!(lab.run(s1, "show errdisable detect").contains(&format!("{:<20} no", "bpduguard")));
    assert!(lab.run(s1, "show running-config").contains("no errdisable detect cause bpduguard\n"));
    lab.sim.run_for(&lab.t, 10_000_000);
    l2_apply(&mut lab);
    assert!(!lab.t.device(s1).unwrap().ports[0].err_disabled);
    // Detection on again: the next BPDU shuts it.
    lab.run(s1, "conf t\nerrdisable detect cause bpduguard\nend");
    lab.sim.run_for(&lab.t, 10_000_000);
    l2_apply(&mut lab);
    assert!(lab.run(s1, "show errdisable detect").contains(&format!("{:<20} yes", "bpduguard")));
    assert!(lab.t.device(s1).unwrap().ports[0].err_disabled);
    // `all` and a bad name.
    lab.run(s1, "conf t\nno errdisable detect cause all\nend");
    assert!(!lab.run(s1, "show errdisable detect").contains("yes"));
    lab.run(s1, "conf t");
    assert!(lab.try_run(s1, "errdisable detect cause nothing").starts_with("% "));
}
