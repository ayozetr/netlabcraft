//! IGMP snooping: members, router ports, the querier, and multicast that reaches only those who
//! asked for it.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// One switch with four PCs on 10.0.0.1-4; PC4 sends.
fn lan() -> (Lab, DeviceId, [DeviceId; 4]) {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let mut pcs = [s1; 4];
    for (i, slot) in pcs.iter_mut().enumerate() {
        let pc = lab.add(DeviceKind::Pc);
        lab.cable(pc, "fa0", s1, &format!("fa0/{}", i + 1));
        lab.run(pc, &format!("ipconfig 10.0.0.{} 255.255.255.0", i + 1));
        *slot = pc;
    }
    lab.run(s1, "en");
    (lab, s1, pcs)
}

/// Whether a frame of the echo request reached a device.
fn reached(lab: &Lab, dev: DeviceId) -> bool {
    lab.sim.trace().iter().any(|e| e.to.device == dev && e.summary.starts_with("ICMP echo request"))
}

#[test]
fn multicast_floods_until_snooping_is_on_then_goes_to_members_only() {
    let (mut lab, s1, [pc1, pc2, pc3, pc4]) = lan();
    lab.run(pc1, "igmp join 239.1.1.1");
    // Snooping off: every port gets the frame.
    lab.sim.clear_trace();
    assert!(lab.run(pc4, "ping -n 1 239.1.1.1").contains("reply from 10.0.0.1"));
    assert!(reached(&lab, pc2) && reached(&lab, pc3));
    // Snooping on: the join has to be heard again, so join again.
    lab.run(s1, "conf t\nip igmp snooping\nend");
    lab.run(pc1, "igmp join 239.1.1.1");
    let groups = lab.run(s1, "show ip igmp snooping groups");
    assert!(groups.contains("239.1.1.1") && groups.contains("Fa0/1"), "{groups}");
    lab.sim.clear_trace();
    assert!(lab.run(pc4, "ping -n 1 239.1.1.1").contains("reply from 10.0.0.1"));
    assert!(reached(&lab, pc1));
    assert!(!reached(&lab, pc2) && !reached(&lab, pc3), "{:?}", lab.sim.trace().iter().map(|e| e.summary.clone()).collect::<Vec<_>>());
    // A second member joins and answers too; a leave takes a member out.
    lab.run(pc2, "igmp join 239.1.1.1");
    assert!(lab.run(s1, "show ip igmp snooping groups").contains("Fa0/2"));
    lab.run(pc1, "igmp leave 239.1.1.1");
    let groups = lab.run(s1, "show ip igmp snooping groups");
    assert!(!groups.contains("Fa0/1") && groups.contains("Fa0/2"), "{groups}");
    lab.run(pc2, "igmp leave 239.1.1.1");
    assert!(lab.run(s1, "show ip igmp snooping groups").contains("No groups"));
    assert!(lab.run(pc4, "ping -n 1 239.1.1.1").contains("no answer"));
}

#[test]
fn a_vlan_with_snooping_off_still_floods() {
    let (mut lab, s1, [pc1, pc2, _pc3, pc4]) = lan();
    lab.run(s1, "conf t\nip igmp snooping\nno ip igmp snooping vlan 1\nend");
    lab.run(pc1, "igmp join 239.1.1.1");
    lab.sim.clear_trace();
    lab.run(pc4, "ping -n 1 239.1.1.1");
    assert!(reached(&lab, pc2));
    assert!(lab.run(s1, "show ip igmp snooping").contains("disabled"));
    lab.run(s1, "conf t\nip igmp snooping vlan 1\nend");
    assert!(lab.run(s1, "show ip igmp snooping").contains("IGMP snooping:  enabled"));
}

#[test]
fn the_querier_asks_hosts_answer_and_the_next_switch_finds_its_router_port() {
    let (mut lab, s1, [pc1, ..]) = lan();
    let s2 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.run(s1, "conf t\ninterface g0/1\nswitchport mode trunk\nexit\nip igmp snooping\nip igmp snooping querier\nip igmp snooping querier address 10.0.0.250\nend");
    lab.run(s2, "en\nconf t\ninterface g0/1\nswitchport mode trunk\nexit\nip igmp snooping\nend");
    lab.run(pc1, "igmp join 239.1.1.1");
    lab.sim.run_for(&lab.t, 130_000_000);
    let q = lab.run(s1, "show ip igmp snooping querier");
    assert!(q.contains("10.0.0.250"), "{q}");
    let m = lab.run(s2, "show ip igmp snooping mrouter");
    assert!(m.contains("Gi0/1"), "{m}");
    // Forget what was learned: the host's answer to the next query teaches it again.
    lab.run(s1, "clear ip igmp snooping");
    assert!(lab.run(s1, "show ip igmp snooping groups").contains("No groups"));
    lab.sim.run_for(&lab.t, 61_000_000);
    let g = lab.run(s1, "show ip igmp snooping groups");
    assert!(g.contains("239.1.1.1") && g.contains("Fa0/1"), "{g}");
}

#[test]
fn settings_are_kept_shown_and_checked() {
    let (mut lab, s1, [pc1, ..]) = lan();
    lab.run(s1, "conf t\nip igmp snooping\nno ip igmp snooping vlan 20\nip igmp snooping querier\nip igmp snooping querier address 10.0.0.250\nend");
    let run = lab.run(s1, "show running-config");
    for want in ["ip igmp snooping\n", "no ip igmp snooping vlan 20\n", "ip igmp snooping querier\n", "ip igmp snooping querier address 10.0.0.250\n"]
    {
        assert!(run.contains(want), "{want}\n{run}");
    }
    lab.run(s1, "conf t");
    assert!(lab.try_run(s1, "ip igmp snooping querier address 224.0.0.1").starts_with("% "));
    assert!(lab.try_run(s1, "ip igmp snooping vlan 5000").starts_with("% "));
    // The host side.
    assert!(lab.run(pc1, "igmp").contains("no multicast group"));
    assert!(lab.run(pc1, "igmp join 10.0.0.9").contains("not a group"));
    assert!(lab.run(pc1, "igmp join 224.0.0.5").contains("not a group"));
    assert!(lab.run(pc1, "igmp leave 239.9.9.9").contains("has not joined"));
    lab.run(pc1, "igmp join 239.1.1.1");
    assert!(lab.run(pc1, "igmp groups").contains("239.1.1.1"));
}
