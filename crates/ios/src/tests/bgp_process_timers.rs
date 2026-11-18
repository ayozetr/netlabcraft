//! `timers bgp KEEPALIVE HOLD`: the timers of every neighbour that has none of its own.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(r1, "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nexit\nrouter bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nend");
    lab.run(r2, "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nexit\nrouter bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nend");
    advance(&mut lab, 8);
    (lab, r1, r2)
}

#[test]
fn the_process_timers_apply_unless_a_neighbour_has_its_own() {
    let (mut lab, r1, r2) = pair();
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("Hold time 180 s, keepalive every 60 s"));
    lab.run(r1, "conf t\nrouter bgp 65001\ntimers bgp 10 30\nend");
    lab.run(r2, "conf t\nrouter bgp 65002\ntimers bgp 10 30\nend");
    advance(&mut lab, 4);
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("Hold time 30 s, keepalive every 10 s"));
    assert!(lab.run(r1, "show running-config").contains(" timers bgp 10 30\n"));
    // A neighbour's own timers win; the negotiated hold time is the smaller one.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 timers 5 15\nend");
    advance(&mut lab, 4);
    let shown = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(shown.contains("Hold time 15 s, keepalive every 5 s"), "{shown}");
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 timers\nno timers bgp\nend");
    lab.run(r2, "conf t\nrouter bgp 65002\nno timers bgp\nend");
    advance(&mut lab, 4);
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("Hold time 180 s, keepalive every 60 s"));
    assert!(!lab.run(r1, "show running-config").contains("timers bgp"));
    assert!(lab.try_run(r1, "timers bgp 10 2").starts_with("% "), "outside the router bgp mode, and a hold time under 3");
}
