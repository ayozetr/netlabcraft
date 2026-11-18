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
