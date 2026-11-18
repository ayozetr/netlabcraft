//! `bgp dampening` typed at the console: a route its neighbour withdraws collects a penalty that decays
//! with simulated time, is held down past the suppress limit and used again once it falls below the
//! reuse limit.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

const MINUTE: u64 = 60 * 1_000_000;

fn advance(lab: &mut Lab, micros: u64) {
    lab.sim.run_for(&lab.t, micros);
}

/// R1 (AS 65001) owns 172.16.1.0/24 and 172.16.3.0/24; R2 (AS 65002) runs `r2_extra` under `router bgp`.
fn pair(r2_extra: &str) -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nint lo3\nip add 172.16.3.1 255.255.255.0\nexit\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nnetwork 172.16.1.0 mask 255.255.255.0\nnetwork 172.16.3.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        &format!("en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nexit\nrouter bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\n{r2_extra}\nend"),
    );
    advance(&mut lab, 8_000_000);
    (lab, r1, r2)
}

/// The loopback goes down and up again, and the router that watches looks at its table in between.
fn flap(lab: &mut Lab, r1: DeviceId, r2: DeviceId, lo: &str) {
    lab.run(r1, &format!("conf t\nint {lo}\nshutdown\nend"));
    lab.run(r2, "show ip bgp");
    lab.run(r1, &format!("conf t\nint {lo}\nno shutdown\nend"));
    lab.run(r2, "show ip bgp");
}

/// The line of `show ip bgp` for a prefix, without its leading spaces.
fn row(lab: &mut Lab, dev: DeviceId, net: &str) -> String {
    lab.run(dev, "show ip bgp").lines().find(|l| l.contains(net)).unwrap_or_default().trim_start().to_string()
}

#[test]
fn a_route_that_flaps_is_held_down_and_used_again_when_its_penalty_has_decayed() {
    let (mut lab, r1, r2) = pair("bgp dampening");
    assert!(lab.run(r2, "show ip bgp flap-statistics").contains("no route has flapped"));
    flap(&mut lab, r1, r2, "lo1");
    flap(&mut lab, r1, r2, "lo1");
    // Two flaps make 2000, which does not pass the limit: the route is used, and the statistics list it.
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('*'), "{}", lab.run(r2, "show ip bgp"));
    let stats = lab.run(r2, "show ip bgp flap-statistics");
    assert!(stats.contains("172.16.1.0/24") && stats.contains("10.0.12.1") && stats.contains("65001"), "{stats}");
    assert!(lab.run(r2, "show ip bgp dampened-paths").contains("no route is held down"));
    assert!(lab.run(r2, "show ip route bgp").contains("172.16.1.0"));
    // The third passes it: the route comes back advertised but is held down.
    flap(&mut lab, r1, r2, "lo1");
    let held = row(&mut lab, r2, "172.16.1.0/24");
    assert!(held.starts_with('d'), "{held}");
    assert!(!lab.run(r2, "show ip route bgp").contains("172.16.1.0"), "a held-down route is not in the routing table");
    assert!(lab.run(r2, "show ip bgp 172.16.1.0").contains("held down by dampening"));
    let dampened = lab.run(r2, "show ip bgp dampened-paths");
    assert!(
        dampened.contains("172.16.1.0/24") && dampened.contains("65001") && (dampened.contains("00:30:") || dampened.contains("00:29:")),
        "{dampened}"
    );
    // The other prefix never flapped and is used all along.
    assert!(row(&mut lab, r2, "172.16.3.0/24").starts_with('*'));
    // Twenty minutes on, 3000 has decayed to about 1190: still above the reuse limit of 750.
    advance(&mut lab, 20 * MINUTE);
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('d'));
    let later = lab.run(r2, "show ip bgp dampened-paths");
    assert!(later.contains("00:09:"), "ten minutes to go: {later}");
    // Past thirty minutes it is below the limit: the route is used again.
    advance(&mut lab, 11 * MINUTE);
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('*'), "{}", lab.run(r2, "show ip bgp"));
    assert!(lab.run(r2, "show ip route bgp").contains("172.16.1.0"));
    assert!(lab.run(r2, "show ip bgp dampened-paths").contains("no route is held down"));
    assert!(lab.run(r2, "show ip bgp flap-statistics").contains("172.16.1.0/24"), "the history stays until it has decayed");
}

#[test]
fn clear_ip_bgp_dampening_forgets_the_penalty_at_once() {
    let (mut lab, r1, r2) = pair("bgp dampening 10 400 1200 40");
    flap(&mut lab, r1, r2, "lo1");
    flap(&mut lab, r1, r2, "lo1");
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('d'), "2000 passes a limit of 1200");
    let said = lab.run(r2, "clear ip bgp dampening 172.16.1.0 255.255.255.0");
    assert!(said.contains("1 route"), "{said}");
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('*'));
    assert!(lab.run(r2, "show ip bgp flap-statistics").contains("no route has flapped"));
    // Held down again, then everything cleared.
    flap(&mut lab, r1, r2, "lo1");
    flap(&mut lab, r1, r2, "lo1");
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('d'));
    assert!(lab.run(r2, "clear ip bgp dampening").contains("forgotten"));
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('*'));
    assert!(lab.try_run(r2, "clear ip bgp dampening 9.9.9.9").starts_with('%'), "no history for that network");
    // Without dampening configured the views say so, and `no bgp dampening` forgets everything.
    flap(&mut lab, r1, r2, "lo1");
    flap(&mut lab, r1, r2, "lo1");
    lab.run(r2, "conf t\nrouter bgp 65002\nno bgp dampening\nend");
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('*'));
    assert!(lab.run(r2, "show ip bgp dampened-paths").contains("not configured"));
}

#[test]
fn a_route_map_chooses_the_routes_and_sets_their_values() {
    let (mut lab, r1, r2) = pair("bgp dampening route-map DAMP");
    lab.run(r2, "conf t\nip prefix-list ONE seq 5 permit 172.16.1.0/24\nroute-map DAMP permit 10\nmatch ip address prefix-list ONE\nset dampening 10 400 1200 40\nend");
    flap(&mut lab, r1, r2, "lo1");
    flap(&mut lab, r1, r2, "lo1");
    flap(&mut lab, r1, r2, "lo3");
    flap(&mut lab, r1, r2, "lo3");
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('d'), "the map's own limits: 2000 passes 1200");
    assert!(row(&mut lab, r2, "172.16.3.0/24").starts_with('*'), "the map does not permit this one: never dampened");
    let stats = lab.run(r2, "show ip bgp flap-statistics");
    assert!(stats.contains("172.16.1.0/24") && !stats.contains("172.16.3.0/24"), "{stats}");
    let config = lab.run(r2, "show running-config");
    assert!(config.contains(" bgp dampening route-map DAMP\n") && config.contains(" set dampening 10 400 1200 40\n"), "{config}");
}

#[test]
fn the_values_are_written_back_and_checked() {
    let (mut lab, _r1, r2) = pair("bgp dampening 20 600 1800 80");
    assert!(lab.run(r2, "show running-config").contains(" bgp dampening 20 600 1800 80\n"));
    lab.run(r2, "conf t\nrouter bgp 65002\nbgp dampening");
    for bad in [
        "bgp dampening 0 750 2000 60",
        "bgp dampening 46 750 2000 60",
        "bgp dampening 15 2000 750 60",
        "bgp dampening 15 750 2000 0",
        "bgp dampening 15 750 2000",
    ] {
        let said = lab.try_run(r2, bad);
        assert!(said.contains('%'), "{bad}: {said}");
    }
    lab.run(r2, "end");
    let config = lab.run(r2, "show running-config");
    assert!(config.contains(" bgp dampening\n") && !config.contains("bgp dampening 20"), "the bare form replaced the values: {config}");
}
