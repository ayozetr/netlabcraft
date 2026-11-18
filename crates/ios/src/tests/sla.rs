//! IP SLA: the operations, their schedule and results, and a default route that follows an
//! operation through a tracked object.

use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 and R2 on 10.0.0.0/24 through a switch, so cutting R2 leaves R1's link up.
fn lab() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    lab.cable(r1, "g0/0", s, "fa0/1");
    lab.cable(r2, "g0/0", s, "fa0/2");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 10.0.0.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 10.0.0.2 255.255.255.0\nno shut\nend");
    (lab, r1, r2)
}

fn probe_round(lab: &mut Lab, secs: u64) -> bool {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
    netlabcraft_sim::sla::step(&mut lab.sim, &mut lab.t)
}

#[test]
fn an_operation_is_configured_scheduled_and_shown() {
    let (mut lab, r1, _) = lab();
    lab.run(r1, "conf t\nip sla 7");
    assert!(lab.prompt(r1).ends_with("(config-ip-sla)#"));
    lab.run(r1, "icmp-echo 10.0.0.2 source-ip 10.0.0.1");
    assert!(lab.prompt(r1).ends_with("(config-ip-sla-echo)#"));
    lab.run(r1, "frequency 10\ntimeout 2000\nthreshold 500\ntag uplink\nexit");
    lab.run(r1, "ip sla schedule 7 life forever start-time now\nip sla reaction-configuration 7 react timeout threshold-type immediate action-type trapOnly\nip sla responder\nend");
    let cfg = lab.run(r1, "show ip sla configuration 7");
    assert!(
        cfg.contains("Test: icmp-echo 10.0.0.2 source-ip 10.0.0.1") && cfg.contains("Frequency 10 s, timeout 2000 ms, threshold 500 ms"),
        "{cfg}"
    );
    assert!(cfg.contains("Tag: uplink") && cfg.contains("life forever, start now (running)") && cfg.contains("Reaction: react timeout"), "{cfg}");
    let run = lab.run(r1, "show running-config");
    for want in [
        "ip sla 7\n icmp-echo 10.0.0.2 source-ip 10.0.0.1\n frequency 10\n timeout 2000\n threshold 500\n tag uplink\n!\n",
        "ip sla schedule 7 life forever start-time now\n",
        "ip sla reaction-configuration 7 react timeout threshold-type immediate action-type trapOnly\n",
        "ip sla responder\n",
    ] {
        assert!(run.contains(want), "{run}");
    }
    assert!(lab.run(r1, "show ip sla summary").contains("icmp-echo") && lab.run(r1, "show ip sla statistics").contains("no probe has run yet"));
    lab.run(r1, "conf t\nno ip sla 7\nend");
    assert!(lab.run(r1, "show ip sla statistics 7").contains("There is no IP SLA operation 7"));
}

#[test]
fn commands_are_checked() {
    let (mut lab, r1, _) = lab();
    lab.run(r1, "conf t\nip sla 1");
    assert!(lab.try_run(r1, "icmp-echo 10.0.0").starts_with("% "));
    assert!(lab.try_run(r1, "tcp-connect 10.0.0.2 0").starts_with("% "));
    assert!(lab.try_run(r1, "http get ftp://10.0.0.2/").starts_with("% "));
    assert!(lab.try_run(r1, "http get http://example.org/").starts_with("% "), "names need DNS");
    assert!(lab.try_run(r1, "frequency 0").starts_with("% "));
    assert!(lab.try_run(r1, "threshold 99999").starts_with("% "));
    lab.run(r1, "exit");
    assert!(lab.try_run(r1, "ip sla schedule 1 start-time now").starts_with("% "), "no test yet");
    assert!(lab.try_run(r1, "ip sla schedule 9 start-time now").starts_with("% "), "no such operation");
    lab.run(r1, "ip sla 1\nhttp get http://10.0.0.2/index.html\nexit");
    assert!(lab.try_run(r1, "ip sla schedule 1 life sometimes").starts_with("% "));
    assert!(lab.try_run(r1, "ip sla schedule 1 start-time after 99:99").starts_with("% "));
    assert!(lab.try_run(r1, "ip sla reaction-configuration 1 react nothing").starts_with("% "));
    assert!(lab.try_run(r1, "no ip sla 5").starts_with("% "));
    lab.run(r1, "ip sla schedule 1 life 100 start-time pending");
    assert!(!probe_round(&mut lab, 120), "a pending operation sends nothing");
    assert!(lab.run(r1, "show ip sla statistics 1").contains("pending"));
}

#[test]
fn a_default_route_follows_an_operation_and_a_floating_route_takes_over() {
    let (mut lab, r1, r2) = lab();
    lab.run(r1, "conf t\nip sla 1\nicmp-echo 10.0.0.2\nfrequency 5\ntimeout 2000\nexit");
    lab.run(r1, "ip sla schedule 1 life forever start-time now\ntrack 1 ip sla 1 reachability\nexit");
    lab.run(r1, "ip route 0.0.0.0 0.0.0.0 10.0.0.2 track 1\nip route 0.0.0.0 0.0.0.0 10.0.0.3 200\nend");
    // Nothing has been sent yet: the object is down and the floating route carries the default.
    let routes = lab.run(r1, "show ip route");
    assert!(routes.contains("10.0.0.3") && !routes.contains("via 10.0.0.2"), "{routes}");
    assert!(probe_round(&mut lab, 0), "the first probe changes the outcome");
    let routes = lab.run(r1, "show ip route");
    assert!(routes.contains("10.0.0.2") && !routes.contains("via 10.0.0.3"), "the tracked route is back:\n{routes}");
    let stats = lab.run(r1, "show ip sla statistics 1");
    assert!(stats.contains("Latest outcome: OK") && stats.contains("1 sent, 1 answered"), "{stats}");
    // The far router goes away; the next probe fails and the default moves to the floating route.
    lab.run(r2, "conf t\nint g0/0\nshutdown\nend");
    lab.sim.topology_changed(&lab.t);
    assert!(probe_round(&mut lab, 6), "the outcome changed");
    let routes = lab.run(r1, "show ip route");
    assert!(routes.contains("10.0.0.3") && !routes.contains("via 10.0.0.2"), "{routes}");
    let stats = lab.run(r1, "show ip sla statistics 1");
    assert!(stats.contains("Latest outcome: failed") && stats.contains("1 failed"), "{stats}");
    assert!(lab.run(r1, "show track 1").contains("State is Down"));
    // And it comes back when the far router does.
    lab.run(r2, "conf t\nint g0/0\nno shutdown\nend");
    lab.sim.topology_changed(&lab.t);
    assert!(probe_round(&mut lab, 6));
    assert!(lab.run(r1, "show track 1").contains("State is Up"));
    assert!(lab.run(r1, "show ip route").contains("10.0.0.2"));
}

#[test]
fn a_udp_operation_needs_a_responder_at_the_far_end() {
    let (mut lab, r1, r2) = lab();
    lab.run(r1, "conf t\nip sla 2\nudp-echo 10.0.0.2 5000\nfrequency 5\ntimeout 2000\nexit\nip sla schedule 2 life forever start-time now\nend");
    probe_round(&mut lab, 0);
    assert!(lab.run(r1, "show ip sla statistics 2").contains("Latest outcome: failed"));
    lab.run(r2, "conf t\nip sla responder\nend");
    probe_round(&mut lab, 6);
    assert!(lab.run(r1, "show ip sla statistics 2").contains("Latest outcome: OK"));
}
