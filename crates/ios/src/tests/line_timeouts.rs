//! `exec-timeout`, `absolute-timeout` and `session-timeout` close sessions in simulated time.

use netlabcraft_model::DeviceKind;

use super::Lab;

const MINUTE: u64 = 60_000_000;

#[test]
fn exec_timeout_closes_an_idle_console_session() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nline console 0\nexec-timeout 2 0\nend");
    assert!(lab.run(r, "show running-config").contains("exec-timeout 2 0"));
    lab.sim.run_for(&lab.t, MINUTE);
    lab.run(r, "show clock");
    assert!(lab.prompt(r).ends_with('#'), "one minute of silence is fine");
    lab.sim.run_for(&lab.t, 3 * MINUTE);
    let out = lab.try_run(r, "show clock");
    assert!(out.contains("exec-timeout"), "{out}");
    assert!(lab.prompt(r).ends_with('>'), "back at the user prompt: {}", lab.prompt(r));
    // 0 0 turns the timer off.
    lab.run(r, "en\nconf t\nline console 0\nexec-timeout 0 0\nend");
    lab.sim.run_for(&lab.t, 60 * MINUTE);
    assert!(!lab.try_run(r, "show clock").contains("exec-timeout"));
}

#[test]
fn absolute_timeout_ends_a_busy_session_too() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nline console 0\nabsolute-timeout 3\nend");
    for _ in 0..2 {
        lab.sim.run_for(&lab.t, MINUTE);
        assert!(!lab.try_run(r, "show clock").contains("absolute-timeout"));
    }
    lab.sim.run_for(&lab.t, 2 * MINUTE);
    assert!(lab.try_run(r, "show clock").contains("absolute-timeout"));
}
