//! The interactive `ping` and `traceroute`: no target typed, the questions come one by one.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 10.0.0.1 255.255.255.0\nno shut\nip host PEER 10.0.0.2\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 10.0.0.2 255.255.255.0\nno shut\nend");
    (lab, r1, r2)
}

/// Lab::run skips empty lines, and an empty answer is how a default is taken.
fn answer(lab: &mut Lab, dev: DeviceId, text: &str) -> String {
    let out = lab.try_run(dev, text);
    assert!(!out.starts_with("% "), "`{text}` was refused: {out}");
    out
}

#[test]
fn ping_asks_its_questions_and_runs_with_the_answers() {
    let (mut lab, r1, _r2) = pair();
    lab.run(r1, "ping");
    assert_eq!(lab.prompt(r1), "Protocol [ip]: ");
    answer(&mut lab, r1, "");
    assert_eq!(lab.prompt(r1), "Target address or name: ");
    assert!(lab.try_run(r1, "").starts_with("% A target"), "a target is required");
    assert_eq!(lab.prompt(r1), "Target address or name: ", "still asking");
    lab.run(r1, "PEER");
    assert_eq!(lab.prompt(r1), "Repeat count [5]: ");
    assert!(lab.try_run(r1, "0").starts_with("% "), "1-1000");
    lab.run(r1, "6");
    answer(&mut lab, r1, "");
    answer(&mut lab, r1, "");
    assert_eq!(lab.prompt(r1), "Extended commands [n]: ");
    let out = lab.run(r1, "n");
    assert!(out.contains("Translating \"PEER\"...static [OK]") && out.contains("!!!"), "{out}");
    assert!(out.contains("6"), "the repeat count: {out}");
    assert_eq!(lab.prompt(r1), "R1#", "back at the prompt");
}

#[test]
fn the_extended_questions_and_the_sweep_run_one_ping_per_size() {
    let (mut lab, r1, _r2) = pair();
    lab.run(r1, "ping");
    for a in ["ip", "10.0.0.2", "2", "", "", "y"] {
        answer(&mut lab, r1, a);
    }
    assert_eq!(lab.prompt(r1), "Source address or interface: ");
    for a in ["10.0.0.1", "", "n", "", "y", "40", "60", "10"] {
        answer(&mut lab, r1, a);
    }
    assert_eq!(lab.prompt(r1), "R1#");
    // The last answer ran 40, 50 and 60: three pings of two echoes each.
    let again = {
        lab.run(r1, "ping");
        for a in ["", "10.0.0.2", "1", "", "", "y", "10.0.0.1", "", "", "", "y", "40", "60"] {
            answer(&mut lab, r1, a);
        }
        answer(&mut lab, r1, "20")
    };
    assert!(again.matches("% answered").count() == 2, "40 and 60: {again}");
    lab.run(r1, "ping");
    lab.run(r1, "abort");
    assert_eq!(lab.prompt(r1), "R1#");
}

#[test]
fn traceroute_asks_for_the_hop_limit_and_the_timeout() {
    let (mut lab, r1, _r2) = pair();
    lab.run(r1, "traceroute");
    for a in ["", "10.0.0.2", "5"] {
        answer(&mut lab, r1, a);
    }
    assert_eq!(lab.prompt(r1), "Timeout in seconds [2]: ");
    assert!(lab.try_run(r1, "x").starts_with("% "));
    let out = answer(&mut lab, r1, "");
    assert!(out.contains("10.0.0.2"), "{out}");
}
