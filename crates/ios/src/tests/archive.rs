//! The configuration archive, `configure replace` and scheduled reloads.

use netlabcraft_model::DeviceKind;

use super::Lab;

fn router() -> (Lab, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    lab.run(r1, "en\nconf t\narchive\npath flash:cfg\nmaximum 3\nend");
    (lab, r1)
}

#[test]
fn archive_config_keeps_numbered_copies_and_the_oldest_goes() {
    let (mut lab, r1) = router();
    assert!(lab.run(r1, "show archive").contains("Archive path:   flash:cfg"));
    for i in 1..=4 {
        lab.run(r1, &format!("conf t\ninterface g0/0\ndescription STEP{i}\nend"));
        assert!(lab.run(r1, "archive config").contains(&format!("flash:cfg-{i}")));
    }
    let shown = lab.run(r1, "show archive");
    assert!(!shown.contains("flash:cfg-1 ") && shown.contains("flash:cfg-2") && shown.contains("flash:cfg-4"), "{shown}");
    assert!(shown.contains("Maximum copies: 3"));
    // The settings are in the running configuration and replay.
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("archive\n path flash:cfg\n maximum 3\n!"), "{run}");
}

#[test]
fn a_copy_needs_a_path() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    lab.run(r1, "en");
    assert!(lab.try_run(r1, "archive config").starts_with("% "));
    assert!(lab.run(r1, "show archive").contains("not set up"));
}

#[test]
fn configure_replace_and_revert_bring_back_a_copy() {
    let (mut lab, r1) = router();
    lab.run(r1, "conf t\ninterface g0/0\ndescription GOOD\nend");
    lab.run(r1, "archive config");
    lab.run(r1, "conf t\ninterface g0/0\ndescription BAD\nend");
    assert!(lab.run(r1, "show running-config").contains("description BAD"));
    assert!(lab.run(r1, "configure replace flash:cfg-1").contains("copy 1"));
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("description GOOD") && !run.contains("BAD"), "{run}");
    // The archive itself survives the replacement.
    assert!(lab.run(r1, "show archive").contains("flash:cfg-1"));
    lab.run(r1, "conf t\ninterface g0/0\ndescription WORSE\nend");
    assert!(lab.run(r1, "configure revert now").contains("copy 1"));
    assert!(lab.run(r1, "show running-config").contains("description GOOD"));
    // The startup configuration is a source too.
    assert!(lab.try_run(r1, "configure replace startup-config").starts_with("% "));
    lab.run(r1, "write memory");
    lab.run(r1, "conf t\ninterface g0/0\ndescription LATER\nend");
    lab.run(r1, "configure replace startup-config");
    assert!(lab.run(r1, "show running-config").contains("description GOOD"));
    assert!(lab.try_run(r1, "configure replace nothing").starts_with("% "));
}

#[test]
fn write_memory_in_the_archive_copies_on_every_save() {
    let (mut lab, r1) = router();
    lab.run(r1, "conf t\narchive\nwrite-memory\nend");
    assert!(lab.run(r1, "write memory").contains("archived as copy 1"));
    assert!(lab.run(r1, "copy running-config startup-config").contains("copy 2"));
    lab.run(r1, "conf t\narchive\nno write-memory\nend");
    assert!(!lab.run(r1, "write memory").contains("archived"));
    // `no archive` forgets the settings, not the copies.
    lab.run(r1, "conf t\nno archive\nend");
    assert!(lab.run(r1, "show archive").contains("(not set)"));
}

#[test]
fn the_archive_mode_has_its_prompt_and_refuses_bad_values() {
    let (mut lab, r1) = router();
    lab.run(r1, "conf t\narchive");
    assert!(lab.prompt(r1).ends_with("(config-archive)#"), "{}", lab.prompt(r1));
    assert!(lab.try_run(r1, "maximum 99").starts_with("% "));
    lab.run(r1, "time-period 60\nend");
    assert!(lab.run(r1, "show running-config").contains(" time-period 60"));
}

#[test]
fn a_scheduled_reload_restarts_the_device_when_its_time_comes() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    lab.run(s1, "en\nconf t\ninterface fa0/1\ndescription KEPT\nend\nwrite memory\nconf t\ninterface fa0/1\ndescription LOST\nend");
    assert!(lab.run(s1, "reload in 5").contains("5 minutes"));
    assert!(lab.run(s1, "show reload").contains("scheduled in 4 minutes") || lab.run(s1, "show reload").contains("scheduled in 5 minutes"));
    lab.sim.run_for(&lab.t, 4 * 60_000_000);
    assert!(lab.run(s1, "show running-config").contains("description LOST"));
    lab.sim.run_for(&lab.t, 2 * 60_000_000);
    // The next command finds the device restarted, from its saved configuration.
    lab.run(s1, "en");
    let run = lab.run(s1, "show running-config");
    assert!(run.contains("description KEPT") && !run.contains("LOST"), "{run}");
    assert!(lab.run(s1, "show reload").contains("No restart"));
    // A reload can be cancelled; `reload at` takes a time of day and refuses a bad one.
    assert!(lab.run(s1, "reload at 03:30").contains("restarts in"));
    assert!(lab.run(s1, "reload cancel").contains("cancelled"));
    assert!(lab.run(s1, "reload cancel").contains("No restart"));
    assert!(lab.try_run(s1, "reload at 25:00").starts_with("% "));
    assert!(lab.try_run(s1, "reload at noon").starts_with("% "));
    assert!(lab.try_run(s1, "reload in 0").starts_with("% ") || lab.try_run(s1, "reload in 0").contains('^'));
}
