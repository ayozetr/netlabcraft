//! Storm control and UDLD typed at the console: what is accepted, shown, written back and replayed.

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn storm_control_levels_are_kept_shown_and_replayed() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en\nconf t\nint fa0/1");
    lab.run(s, "storm-control broadcast level 20.5 10\nstorm-control multicast level pps 500\nstorm-control unicast level bps 1000000 500000\nstorm-control action trap");
    assert!(lab.try_run(s, "storm-control broadcast level 101").starts_with("% "));
    assert!(lab.try_run(s, "storm-control broadcast level 10 20").starts_with("% "));
    assert!(lab.try_run(s, "storm-control broadcast level pps 0").starts_with("% "));
    assert!(lab.try_run(s, "storm-control anycast level 5").starts_with("% Say one of"));
    lab.run(s, "end");
    let run = lab.run(s, "show running-config");
    assert!(run.contains(" storm-control broadcast level 20.5 10\n"), "{run}");
    assert!(run.contains(" storm-control multicast level pps 500\n"), "{run}");
    assert!(run.contains(" storm-control unicast level bps 1000000 500000\n"), "{run}");
    assert!(run.contains(" storm-control action trap\n"), "{run}");
    let shown = lab.run(s, "show storm-control");
    assert!(shown.contains("Fa0/1") && shown.contains("20.5%") && shown.contains("500 pps") && shown.contains("trap"), "{shown}");
    assert!(lab.run(s, "show storm-control multicast").contains("pps") && !lab.run(s, "show storm-control multicast").contains("bps"));
    lab.run(s, "conf t\nint fa0/1\nno storm-control broadcast level\nno storm-control action\nend");
    let run = lab.run(s, "show running-config");
    assert!(!run.contains("broadcast level") && !run.contains("storm-control action"), "{run}");
    assert!(run.contains("multicast level pps 500"), "{run}");
    // Saved and replayed on a reboot.
    lab.run(s, "copy running-config startup-config\nreload");
    let again = lab.run(s, "en\nshow running-config");
    assert!(again.contains(" storm-control unicast level bps 1000000 500000\n"), "{again}");
}

#[test]
fn storm_control_is_a_switch_command_and_the_new_causes_are_errdisable_causes() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nint g0/0");
    assert!(lab.try_run(r, "storm-control broadcast level 10").contains("% "));
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en\nconf t\nerrdisable recovery cause storm-control\nerrdisable recovery cause udld\nend");
    let shown = lab.run(s, "show errdisable recovery");
    assert!(shown.contains("storm-control") && shown.contains("udld"), "{shown}");
}

#[test]
fn udld_settings_are_kept_shown_and_replayed() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    lab.cable(s, "g0/1", s2, "g0/1");
    lab.run(s, "en\nconf t\nudld enable\nudld message time 7\nint g0/1\nudld port aggressive\nint fa0/2\nno udld port\nend");
    lab.run(s2, "en\nconf t\nudld enable\nudld message time 7\nend");
    let run = lab.run(s, "show running-config");
    assert!(run.contains("udld enable\n") && run.contains("udld message time 7\n"), "{run}");
    assert!(run.contains(" udld port aggressive\n") && run.contains(" no udld port\n"), "{run}");
    lab.run(s, "conf t");
    assert!(lab.try_run(s, "udld message time 0").starts_with("% "));
    lab.run(s, "end");
    lab.sim.run_for(&lab.t, 40_000_000);
    let shown = lab.run(s, "show udld g0/1");
    assert!(shown.contains("aggressive") && shown.contains("bidirectional"), "{shown}");
    assert!(shown.contains("S2"), "{shown}");
    let all = lab.run(s, "show udld");
    assert!(all.contains("every 7 s") && !all.contains("Fa0/2"), "{all}");
    assert!(lab.run(s, "udld reset").contains("0 ports"));
    // UDLD is for switches only.
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t");
    assert!(lab.try_run(r, "udld enable").contains("% "));
}
