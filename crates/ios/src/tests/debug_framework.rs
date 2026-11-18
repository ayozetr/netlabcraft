//! The debug framework: topics turn on and off one by one, `undebug all`, the access-list filter
//! of `debug ip packet`, the `logging` levels and the bounded queues.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_sim::{DebugKind, MAX_LOG};

use super::Lab;
use crate::exec::debug::TOPICS;

fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.run(a, "ipconfig 10.0.0.2 255.255.255.0 10.0.0.1");
    lab.run(r, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    (lab, r, a)
}

#[test]
fn every_topic_turns_on_and_off_alone() {
    let (mut lab, r, _) = pair();
    for (key, kind, _) in TOPICS {
        let on = lab.run(r, &format!("debug {key}"));
        assert!(on.contains("debugging is on"), "debug {key}: {on}");
        assert!(lab.sim.debug_on(r, *kind), "debug {key} did not set {kind:?}");
        let shown = lab.run(r, "show debugging");
        assert!(shown.contains(kind.name()), "debug {key}: {shown}");
        let off = lab.run(r, &format!("no debug {key}"));
        assert!(off.contains("debugging is off"), "no debug {key}: {off}");
        assert!(!lab.sim.debug_on(r, *kind), "no debug {key} left {kind:?} on");
        assert!(lab.run(r, "show debugging").contains("No debugging"));
    }
}

#[test]
fn no_debug_of_one_topic_leaves_the_others_on() {
    let (mut lab, r, _) = pair();
    lab.run(r, "debug ip icmp\ndebug ip rip\ndebug arp");
    lab.run(r, "no debug ip icmp");
    assert_eq!(lab.sim.debug_kinds(r), vec![DebugKind::Rip, DebugKind::Arp]);
    lab.run(r, "undebug all");
    assert!(lab.run(r, "show debugging").contains("No debugging"));
    lab.run(r, "debug ip icmp\ndebug cdp events");
    lab.run(r, "no debug all");
    assert!(lab.sim.debug_kinds(r).is_empty());
}

#[test]
fn debugging_is_per_device() {
    let (mut lab, r, a) = pair();
    let r2 = lab.add(DeviceKind::Router);
    lab.run(r, "debug ip icmp");
    assert!(!lab.sim.debug_on(r2, DebugKind::Icmp));
    lab.run(a, "ping 10.0.0.1");
    assert!(lab.run(r2, "show logging").contains("0 messages"));
}

#[test]
fn debug_ip_packet_takes_an_access_list() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.cable(b, "fa0", r, "g0/1");
    lab.run(a, "ipconfig 10.0.0.2 255.255.255.0 10.0.0.1");
    lab.run(b, "ipconfig 10.0.1.2 255.255.255.0 10.0.1.1");
    lab.run(
        r,
        "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nint g0/1\nip address 10.0.1.1 255.255.255.0\nno shut\nexit\naccess-list 10 permit host 10.0.0.2\nend",
    );
    assert!(lab.try_run(r, "debug ip packet 99").starts_with("% "), "a list that does not exist is refused");
    assert!(lab.run(r, "debug ip packet 10").contains("debugging is on"));
    assert!(lab.run(r, "show debugging").contains("list 10"));
    lab.run(a, "ping 10.0.1.2");
    lab.run(b, "ping 10.0.0.2");
    let log = lab.run(r, "show logging");
    assert!(log.contains("IP debug: 10.0.0.2 → 10.0.1.2"), "{log}");
    assert!(!log.contains("IP debug: 10.0.1.2 → 10.0.0.2"), "the list does not permit the other direction:\n{log}");
    // Without the list every packet is reported; turning the topic off forgets the list.
    lab.run(r, "no debug ip packet\ndebug ip packet\nclear logging");
    lab.run(b, "ping 10.0.0.2");
    assert!(lab.run(r, "show logging").contains("IP debug: 10.0.1.2 → 10.0.0.2"));
}

#[test]
fn logging_levels_filter_the_buffer_the_console_and_the_monitor() {
    let (mut lab, r, a) = pair();
    lab.run(r, "debug ip icmp\nconf t\nlogging buffered warnings\nlogging console warnings\nlogging monitor errors\nend");
    lab.sim.take_console(r);
    lab.sim.take_monitor(r);
    lab.run(a, "ping 10.0.0.1");
    assert!(!lab.run(r, "show logging").contains("ICMP debug"), "debugging is above warnings");
    assert!(lab.sim.take_console(r).iter().all(|l| !l.contains("ICMP debug")));
    assert!(lab.sim.take_monitor(r).iter().all(|l| !l.contains("ICMP debug")));
    let shown = lab.run(r, "show logging");
    assert!(shown.contains("monitor errors"), "{shown}");
    lab.run(r, "conf t\nlogging buffered debugging\nno logging monitor\nend");
    lab.run(a, "ping 10.0.0.1");
    assert!(lab.run(r, "show logging").contains("ICMP debug: echo request from 10.0.0.2"));
    assert!(lab.sim.take_monitor(r).iter().any(|l| l.contains("ICMP debug")), "the monitor level is back to everything");
    lab.run(r, "conf t\nlogging monitor notifications\nend");
    assert!(lab.run(r, "show running-config").contains("logging monitor notifications"));
    lab.run(r, "conf t\nno logging buffered\nend");
    assert!(!lab.run(r, "show running-config").contains("logging buffered"));
    lab.run(r, "conf t");
    assert!(lab.try_run(r, "logging monitor loud").starts_with("% "));
}

#[test]
fn a_chatty_debug_never_grows_the_queues_past_the_cap() {
    let (mut lab, r, _) = pair();
    lab.run(r, "debug ip icmp");
    for i in 0..(MAX_LOG * 4) {
        lab.sim.debug_log(r, DebugKind::Icmp, &format!("message {i}"));
    }
    assert!(lab.sim.state(r).is_some_and(|s| s.log.len() <= MAX_LOG));
    assert!(lab.sim.take_console(r).len() <= MAX_LOG);
    assert!(lab.sim.take_monitor(r).len() <= MAX_LOG);
    // A topic that is off reports nothing.
    lab.run(r, "undebug all\nclear logging");
    lab.sim.debug_log(r, DebugKind::Icmp, "quiet");
    assert!(lab.run(r, "show logging").contains("0 messages"));
}

#[test]
fn terminal_monitor_is_accepted_and_undone() {
    let (mut lab, r, _) = pair();
    assert!(lab.try_run(r, "terminal monitor").is_empty());
    assert!(lab.try_run(r, "terminal no monitor").is_empty());
    assert!(lab.try_run(r, "no terminal monitor").is_empty());
    assert!(lab.try_run(r, "terminal bogus").starts_with("% "));
}
