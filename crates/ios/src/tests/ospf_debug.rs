//! `debug ip ospf adj|events|hello` print different things (M9.7).

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// Two routers cabled but with no OSPF yet: R1 has all three debugs set the way the test asks.
fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    for (r, ip) in [(r1, "10.0.0.1"), (r2, "10.0.0.2")] {
        lab.run(r, &format!("en\nconf t\ninterface g0/0\nip address {ip} 255.255.255.0\nno shutdown\nend"));
    }
    (lab, r1, r2)
}

fn start_ospf(lab: &mut Lab, r: DeviceId, id: &str, extra_if: &str) {
    lab.run(r, &format!("conf t\ninterface g0/0\n{extra_if}\nexit\nrouter ospf 1\nrouter-id {id}\nnetwork 10.0.0.0 0.0.0.255 area 0\nend"));
}

fn log_lines(lab: &mut Lab, r: DeviceId) -> Vec<String> {
    lab.run(r, "show logging").lines().map(str::to_string).collect()
}

#[test]
fn adj_events_and_hello_report_different_things() {
    for (kind, must, must_not) in [
        ("hello", vec!["OSPF hello: from 10.0.0.2"], vec!["OSPF adjacency:", "OSPF event:"]),
        ("adj", vec!["OSPF adjacency: neighbour 2.2.2.2 on GigabitEthernet0/0: Down -> Init", "Loading -> Full"], vec!["OSPF hello:", "OSPF event:"]),
        (
            "events",
            vec!["OSPF event: neighbour change on GigabitEthernet0/0: DR election gives DR 10.0.0.2, BDR 10.0.0.1", "OSPF event: SPF run"],
            vec!["OSPF hello:", "OSPF adjacency:"],
        ),
    ] {
        let (mut lab, r1, r2) = pair();
        lab.run(r1, &format!("debug ip ospf {kind}"));
        start_ospf(&mut lab, r1, "1.1.1.1", "");
        start_ospf(&mut lab, r2, "2.2.2.2", "");
        lab.sim.run_for(&lab.t, 60_000_000);
        let log = log_lines(&mut lab, r1).join("\n");
        for m in &must {
            assert!(log.contains(m), "debug ip ospf {kind}: missing `{m}`:\n{log}");
        }
        for m in &must_not {
            assert!(!log.contains(m), "debug ip ospf {kind}: unexpected `{m}`:\n{log}");
        }
    }
}

#[test]
fn adj_walks_the_states_in_order_and_names_the_master() {
    let (mut lab, r1, r2) = pair();
    lab.run(r2, "debug ip ospf adj");
    start_ospf(&mut lab, r1, "1.1.1.1", "");
    start_ospf(&mut lab, r2, "2.2.2.2", "");
    lab.sim.run_for(&lab.t, 60_000_000);
    let log = log_lines(&mut lab, r2);
    let at = |needle: &str| log.iter().position(|l| l.contains(needle));
    let order: Vec<Option<usize>> =
        ["Down -> Init", "Init -> 2-Way", "2-Way -> ExStart", "ExStart -> Exchange", "Exchange -> Loading", "Loading -> Full"]
            .iter()
            .map(|s| at(s))
            .collect();
    assert!(order.iter().all(Option::is_some) && order.windows(2).all(|w| w[0] < w[1]), "{order:?}\n{log:#?}");
    // R2 has the higher router ID.
    assert!(log.iter().any(|l| l.contains("master is this router")), "{log:#?}");
}

#[test]
fn a_neighbour_that_goes_away_is_reported_by_adj_and_events() {
    let (mut lab, r1, r2) = pair();
    start_ospf(&mut lab, r1, "1.1.1.1", "");
    start_ospf(&mut lab, r2, "2.2.2.2", "");
    lab.sim.run_for(&lab.t, 60_000_000);
    lab.run(r1, "debug ip ospf adj\ndebug ip ospf events");
    lab.run(r2, "conf t\nno router ospf 1\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    let log = log_lines(&mut lab, r1).join("\n");
    assert!(log.contains("OSPF adjacency: neighbour 2.2.2.2 on GigabitEthernet0/0: Full -> Down"), "{log}");
    assert!(log.contains("OSPF event: neighbour 2.2.2.2 on GigabitEthernet0/0 removed"), "{log}");
}

#[test]
fn a_hello_that_cannot_make_a_neighbour_says_why_under_adj_and_events() {
    let (mut lab, r1, r2) = pair();
    lab.run(r1, "debug ip ospf adj\ndebug ip ospf events\ndebug ip ospf hello");
    start_ospf(&mut lab, r1, "1.1.1.1", "ip ospf hello-interval 5");
    start_ospf(&mut lab, r2, "2.2.2.2", "");
    lab.sim.run_for(&lab.t, 30_000_000);
    let log = log_lines(&mut lab, r1).join("\n");
    assert!(log.contains("OSPF adjacency: 2.2.2.2 on GigabitEthernet0/0 stays Down: hello/dead intervals 10/40 s, here 5/20 s"), "{log}");
    assert!(log.contains("OSPF event: hello from 10.0.0.2 on GigabitEthernet0/0 rejected"), "{log}");
    assert!(log.contains("OSPF hello: from 10.0.0.2"), "{log}");
}

#[test]
fn debug_names_show_up_and_each_kind_turns_off_alone() {
    let (mut lab, r1, _) = pair();
    lab.run(r1, "debug ip ospf adj\ndebug ip ospf events\ndebug ip ospf hello");
    let on = lab.run(r1, "show debugging");
    assert!(on.contains("OSPF hellos") && on.contains("OSPF adjacencies") && on.contains("OSPF events"), "{on}");
    lab.run(r1, "no debug ip ospf events");
    let on = lab.run(r1, "show debugging");
    assert!(on.contains("OSPF hellos") && on.contains("OSPF adjacencies") && !on.contains("OSPF events"), "{on}");
    lab.run(r1, "no debug ip ospf");
    assert!(lab.run(r1, "show debugging").contains("No debugging"));
    assert!(lab.try_run(r1, "debug ip ospf bogus").starts_with("% "));
}
