//! Port mirroring typed at the console: `monitor session`, its checks, `show monitor`, the
//! running configuration, and what the mirror does to a sniffer cabled to the destination.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_wire::Protocol;

use super::Lab;

fn switch() -> (Lab, DeviceId) {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en\nconf t");
    (lab, s)
}

#[test]
fn a_session_is_built_shown_and_written_to_the_running_configuration() {
    let (mut lab, s) = switch();
    lab.run(s, "monitor session 1 source interface fa0/1 - 3, fa0/5 rx");
    lab.run(s, "monitor session 1 source interface fa0/4");
    lab.run(s, "monitor session 1 destination interface fa0/24");
    lab.run(s, "end");
    let shown = lab.run(s, "show monitor");
    assert!(shown.contains("Monitor session 1 (local)"), "{shown}");
    assert!(shown.contains("Source ports:  both Fa0/4; rx Fa0/1 - 3, Fa0/5"), "{shown}");
    assert!(shown.contains("Destination:   Fa0/24 (link down)"), "no cable on the destination yet: {shown}");
    assert!(shown.contains("State:         copying"), "{shown}");
    assert_eq!(lab.run(s, "show monitor session 1"), shown);
    assert_eq!(lab.run(s, "show monitor session all"), shown);
    let run = lab.run(s, "show running-config");
    assert!(
        run.contains(
            "monitor session 1 source interface FastEthernet0/4\nmonitor session 1 source interface FastEthernet0/1 - 3, FastEthernet0/5 rx\n\
             monitor session 1 destination interface FastEthernet0/24\n!"
        ),
        "{run}"
    );
    // The lines read back into another switch give the same configuration, and the lab file keeps it.
    let body: String = run.lines().filter(|l| !l.starts_with("hostname")).map(|l| format!("{l}\n")).collect();
    let s2 = lab.add(DeviceKind::Switch);
    let failed = crate::exec::apply_config(&mut lab.t, &mut lab.sim, s2, &body);
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(lab.t.device(s2).unwrap().l2.span, lab.t.device(s).unwrap().l2.span);
    let back = netlabcraft_model::file::from_json(&netlabcraft_model::file::to_json(&lab.t).unwrap()).unwrap();
    assert_eq!(back.device(s).unwrap().l2.span, lab.t.device(s).unwrap().l2.span);
    // Saved with `write memory`, the session comes back after a reload.
    lab.run(s, "write memory\nreload");
    let again = lab.run(s, "en\nshow monitor");
    assert!(again.contains("Source ports:  both Fa0/4; rx Fa0/1 - 3, Fa0/5") && again.contains("Destination:   Fa0/24"), "{again}");
    // A vlan session on the other switch; a reload forgets what was not saved.
    lab.run(s2, "en\nconf t\nno monitor session all\nmonitor session 2 source vlan 10,20-21 tx\nmonitor session 2 destination interface fa0/2\nend");
    let shown = lab.run(s2, "show monitor");
    assert!(shown.contains("Source VLANs:  tx 10,20-21") && shown.contains("Source ports:  none"), "{shown}");
    assert!(lab.run(s2, "show running-config").contains("monitor session 2 source vlan 10,20-21 tx"));
    lab.run(s2, "reload");
    assert!(lab.run(s2, "en\nshow monitor").contains("No monitor session is configured."));
}

#[test]
fn the_no_forms_take_a_session_or_part_of_one_away() {
    let (mut lab, s) = switch();
    lab.run(s, "monitor session 1 source interface fa0/1 - 2\nmonitor session 1 destination interface fa0/9");
    lab.run(s, "monitor session 2 source vlan 5\nmonitor session 2 destination interface fa0/10");
    lab.run(s, "no monitor session 1 source interface fa0/1");
    let one = lab.run(s, "end\nshow monitor session 1");
    assert!(one.contains("Source ports:  both Fa0/2") && !one.contains("Fa0/1"), "{one}");
    lab.run(s, "conf t");
    assert!(lab.try_run(s, "no monitor session 1 source interface fa0/7").contains("None of those"));
    lab.run(s, "no monitor session 1 destination interface fa0/9");
    assert!(lab.run(s, "end\nshow monitor session 1").contains("State:         waiting: no destination yet"));
    lab.run(s, "conf t\nno monitor session 1 source interface fa0/2");
    assert!(lab.try_run(s, "no monitor session 1").contains("not configured"), "an emptied session is gone");
    lab.run(s, "no monitor session 2 source vlan 5");
    lab.run(s, "no monitor session 2");
    assert!(lab.run(s, "end\nshow monitor").contains("No monitor session is configured."));
    lab.run(s, "conf t\nmonitor session 1 source interface fa0/1\nmonitor session 2 source interface fa0/2");
    lab.run(s, "no monitor session all");
    assert!(lab.run(s, "end\nshow monitor").contains("No monitor session is configured."));
}

#[test]
fn the_checks_keep_a_switchs_sessions_consistent() {
    let (mut lab, s) = switch();
    assert!(lab.try_run(s, "monitor session 0 source interface fa0/1").contains("1-66"));
    assert!(lab.try_run(s, "monitor session 67 source interface fa0/1").contains("1-66"));
    assert!(lab.try_run(s, "monitor session 1 source interface fa0/99").contains("%"));
    assert!(lab.try_run(s, "monitor session 1 source interface rx").contains("%"));
    assert!(lab.try_run(s, "monitor session 1 source interface fa0/1 fa0/2").contains("%"));
    assert!(lab.try_run(s, "monitor session 1 source vlan 1002").contains("reserved"));
    assert!(lab.try_run(s, "monitor session 1 source vlan abc").contains("VLAN numbers"));
    lab.run(s, "monitor session 1 source interface fa0/1\nmonitor session 1 destination interface fa0/24");
    // A destination is not a source, and a source is not a destination, in any session.
    assert!(lab.try_run(s, "monitor session 1 source interface fa0/24").contains("cannot also be a source"));
    assert!(lab.try_run(s, "monitor session 2 source interface fa0/24").contains("cannot also be a source"));
    assert!(lab.try_run(s, "monitor session 1 destination interface fa0/1").contains("cannot also be the destination"));
    assert!(lab.try_run(s, "monitor session 2 destination interface fa0/1").contains("cannot also be the destination"));
    // One session per source port, one per destination port, ports or VLANs but not both.
    lab.run(s, "monitor session 2 source interface fa0/2");
    assert!(lab.try_run(s, "monitor session 2 source interface fa0/1").contains("already feeds session 1"));
    assert!(lab.try_run(s, "monitor session 2 destination interface fa0/24").contains("already receives session 1"));
    assert!(lab.try_run(s, "monitor session 2 source vlan 10").contains("ports or VLANs"));
    assert!(lab.try_run(s, "monitor session 1 source vlan 10").contains("ports or VLANs"));
    // A switch holds two sessions.
    assert!(lab.try_run(s, "monitor session 3 source interface fa0/3").contains("2 monitor sessions"));
    assert!(lab.try_run(s, "monitor session 3 destination interface fa0/3").contains("2 monitor sessions"));
    // Moving a session's destination is allowed.
    lab.run(s, "monitor session 1 destination interface fa0/23");
    assert!(lab.run(s, "end\nshow monitor session 1").contains("Fa0/23"));
    // VLAN sources: one session per VLAN too.
    lab.run(s, "conf t\nno monitor session all\nmonitor session 1 source vlan 10\nmonitor session 2 source vlan 20");
    assert!(lab.try_run(s, "monitor session 2 source vlan 10").contains("already feeds session 1"));
    // Routers have no monitor sessions.
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t");
    assert!(lab.try_run(r, "monitor session 1 source interface g0/0").contains("%"));
}

#[test]
fn a_sniffer_behind_the_destination_hears_the_mirrored_traffic_and_the_destination_forwards_nothing_else() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    let c = lab.add(DeviceKind::Pc);
    let sn = lab.t.add_model(netlabcraft_model::models::find("Sniffer").unwrap(), None, 0.0, 0.0).unwrap();
    for (i, d) in [a, b, c].into_iter().enumerate() {
        lab.cable(d, "fa0", s, &format!("fa0/{}", i + 1));
    }
    lab.cable(sn, "fa0", s, "fa0/4");
    for (d, n) in [(a, 1), (b, 2), (c, 3)] {
        lab.run(d, &format!("ipconfig 10.0.0.{n} 255.255.255.0"));
    }
    // Without a mirror the sniffer behind a switch hears only the flooded ARP request.
    lab.run(a, "ping 10.0.0.2");
    let heard = |lab: &Lab| lab.sim.trace().iter().filter(|e| e.to.device == sn).count();
    let icmp = |lab: &Lab| lab.sim.trace().iter().filter(|e| e.to.device == sn && e.protocol == Protocol::Icmp).count();
    assert!(heard(&lab) >= 1 && icmp(&lab) == 0, "the flooded broadcast, and no echo");
    // Mirror PC1's port to the sniffer's.
    lab.run(s, "en\nconf t\nmonitor session 1 source interface fa0/1\nmonitor session 1 destination interface fa0/4\nend");
    assert!(lab.run(s, "show monitor").contains("Destination:   Fa0/4 (link up)"));
    lab.sim.clear_trace();
    lab.run(a, "ping 10.0.0.2");
    assert!(icmp(&lab) >= 8, "four echo requests and four replies, copied: {}", icmp(&lab));
    // PC2 and PC3 talk where the mirror does not reach: nothing of it comes out of the destination.
    lab.sim.clear_trace();
    lab.run(b, "ping 10.0.0.3");
    lab.run(b, "ping 10.0.0.3");
    assert_eq!(icmp(&lab), 0, "unicast between the other ports is not copied");
    // The sniffer sends nothing, whatever it hears.
    assert!(lab.sim.trace().iter().all(|e| e.from.device != sn));
    // Removing the session puts the sniffer back where it was.
    lab.run(s, "conf t\nno monitor session 1\nend");
    lab.sim.clear_trace();
    lab.run(a, "ping 10.0.0.2");
    assert_eq!(icmp(&lab), 0);
}
