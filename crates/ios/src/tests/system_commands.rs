//! Command aliases and scheduled reloads.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn router() -> (Lab, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    lab.run(r1, "en\nconf t\nhostname BEFORE\nend\ncopy running-config startup-config");
    (lab, r1)
}

#[test]
fn an_alias_stands_for_a_command_in_its_own_mode_only() {
    let (mut lab, r1) = router();
    lab.run(r1, "conf t\nalias exec sib show ip interface brief\nalias exec rc show running-config\nalias configure hn hostname\nend");
    let by_alias = lab.run(r1, "sib");
    let direct = lab.run(r1, "show ip interface brief");
    assert_eq!(by_alias, direct, "the alias runs the whole command");
    assert!(lab.run(r1, "rc").contains("hostname BEFORE"), "arguments and other aliases");
    let cfg = lab.run(r1, "show running-config");
    assert!(cfg.contains("alias exec sib show ip interface brief\n"), "written back: {cfg}");
    assert!(lab.run(r1, "show aliases").contains("sib"));
    // `hn` is a configure-mode alias: the arguments after it are kept.
    lab.run(r1, "conf t\nhn RENAMED\nend");
    assert!(lab.run(r1, "show running-config").contains("hostname RENAMED"));
    assert!(lab.try_run(r1, "hn").contains("% "), "not an exec alias");
    lab.run(r1, "conf t\nno alias exec sib\nend");
    assert!(lab.try_run(r1, "sib").contains("% "), "removed");
    assert!(lab.try_run(r1, "alias exec x").contains("% "), "a command text is needed");
}

#[test]
fn a_scheduled_reload_happens_when_its_time_comes() {
    let (mut lab, r1) = router();
    lab.run(r1, "conf t\nhostname AFTER\nend");
    let said = lab.run(r1, "reload in 5 maintenance window");
    assert!(said.contains("5 minutes"), "{said}");
    let shown = lab.run(r1, "show reload");
    assert!(shown.contains("maintenance window") && shown.contains("minutes"), "{shown}");
    // Not yet.
    lab.sim.run_for(&lab.t, 200 * 1_000_000);
    assert!(lab.run(r1, "show running-config").contains("hostname AFTER"));
    lab.sim.run_for(&lab.t, 200 * 1_000_000);
    let first = lab.run(r1, "show reload");
    assert!(first.contains("scheduled reload took place"), "{first}");
    lab.run(r1, "en");
    assert!(lab.run(r1, "show running-config").contains("hostname BEFORE"), "back to the saved configuration");
    assert!(lab.run(r1, "show reload").contains("No reload is scheduled"));
}

#[test]
fn a_scheduled_reload_can_be_cancelled_and_its_times_are_checked() {
    let (mut lab, r1) = router();
    lab.run(r1, "clock set 10:00:00 1 Mar 2026");
    lab.run(r1, "reload at 10:30");
    assert!(lab.run(r1, "show reload").contains("30 minutes"));
    assert!(lab.run(r1, "reload cancel").contains("cancelled"));
    assert!(lab.run(r1, "reload cancel").contains("No reload"));
    assert!(lab.try_run(r1, "reload at 25:00").starts_with("% "));
    assert!(lab.try_run(r1, "reload in 0").starts_with("% "));
    assert!(lab.try_run(r1, "reload in soon").starts_with("% "));
    assert!(lab.run(r1, "reload in 1:30").contains("1 hour 30 minutes"));
}
