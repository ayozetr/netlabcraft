//! `neighbor … password` (TCP MD5) and `neighbor … default-originate route-map` typed at the console:
//! a session comes up only when both ends have the same password, the password is hidden where the
//! console shows a neighbour, and the default route follows its route map.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001) - R2 (AS 65002). R1 owns 172.16.1.0/24. `r1` and `r2` are lines added under each
/// `router bgp`, so a test can give each end a password (or none) before the session starts.
fn pair(r1_extra: &str, r2_extra: &str) -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(
        r1,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nexit\n\
             router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nnetwork 172.16.1.0 mask 255.255.255.0\n{r1_extra}\nend"
        ),
    );
    lab.run(
        r2,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nexit\n\
             router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\n{r2_extra}\nend"
        ),
    );
    advance(&mut lab, 8);
    (lab, r1, r2)
}

#[test]
fn the_same_password_on_both_ends_lets_the_session_come_up() {
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 password fake-key-1", "neighbor 10.0.12.1 password fake-key-1");
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    let n = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(n.contains("State: Established") && n.contains("MD5 password"), "{n}");
    assert!(!n.contains("fake-key-1"), "{n}");
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.12.2 password fake-key-1\n"));
}

#[test]
fn different_passwords_or_one_missing_keep_the_session_down_until_they_agree() {
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 password fake-key-1", "neighbor 10.0.12.1 password fake-key-2");
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    let n = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(n.contains("State: Active") && n.contains("TCP MD5 authentication failed"), "{n}");
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 password fake-key-1\nend");
    advance(&mut lab, 8);
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));

    // A key on one end only is no better than a wrong one.
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 password fake-key-1", "");
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.12.1").contains("TCP MD5 authentication failed"));
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 password\nend");
    advance(&mut lab, 8);
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    assert!(!lab.run(r1, "show running-config").contains("neighbor 10.0.12.2 password"));
}

#[test]
fn the_password_is_obscured_with_service_password_encryption_and_reads_back() {
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 password fake-key-1", "neighbor 10.0.12.1 password fake-key-1");
    lab.run(r1, "conf t\nservice password-encryption\nend");
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    let config = lab.run(r1, "show running-config");
    assert!(config.contains(" neighbor 10.0.12.2 password 7 ") && !config.contains("fake-key-1"), "{config}");
    // The obscured text typed back gives the same key.
    let line = config.lines().find(|l| l.contains("password 7 ")).unwrap_or_default().trim().to_string();
    lab.run(r1, &format!("conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 password\n{line}\nend"));
    advance(&mut lab, 8);
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 password 7 zz").starts_with('%'));
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 password 0123456789012345678901234567890").starts_with('%'));
}

#[test]
fn a_peer_group_hands_its_password_to_the_members_and_the_view_hides_it() {
    let (mut lab, r1, r2) = pair(
        "neighbor G peer-group\nneighbor G remote-as 65002\nneighbor G password fake-key-1\nneighbor 10.0.12.2 peer-group G",
        "neighbor 10.0.12.1 password fake-key-1",
    );
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    let g = lab.run(r1, "show ip bgp peer-group G");
    assert!(g.contains("password ******") && !g.contains("fake-key-1"), "{g}");
    let config = lab.run(r1, "show running-config");
    assert_eq!(config.matches("password fake-key-1").count(), 1, "{config}");
}

/// R1 (AS 65001) - R2 (AS 65002) - R3 (AS 3000); R1 owns 172.16.1.0/24. R2 holds a route map DEF
/// that lets the default route through while a route passes the prefix list WATCH.
fn edge() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nexit\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nnetwork 172.16.1.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\n\
         ip prefix-list WATCH seq 5 permit 172.16.9.0/24\n\
         route-map DEF permit 10\nmatch ip address prefix-list WATCH\nset as-path prepend 65002\nexit\n\
         router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nneighbor 10.0.23.2 remote-as 3000\n\
         neighbor 10.0.23.2 default-originate route-map DEF\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nexit\nrouter bgp 3000\nbgp router-id 3.3.3.3\nneighbor 10.0.23.1 remote-as 65002\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

#[test]
fn default_originate_with_a_route_map_waits_for_a_route_that_passes_it() {
    let (mut lab, _r1, r2, r3) = edge();
    // No route passes WATCH yet: the default route is held back, the others flow.
    let table = lab.run(r3, "show ip bgp");
    assert!(table.contains("172.16.1.0/24") && !table.contains("0.0.0.0/0"), "{table}");
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.23.2").contains("default-originate route-map DEF"));
    assert!(lab.run(r2, "show running-config").contains(" neighbor 10.0.23.2 default-originate route-map DEF\n"));
    // R1's network now passes the list: the default is sent, with the map's prepend.
    lab.run(r2, "conf t\nip prefix-list WATCH seq 10 permit 172.16.1.0/24\nend");
    advance(&mut lab, 2);
    let table = lab.run(r3, "show ip bgp");
    assert!(table.contains("0.0.0.0/0") && table.contains("65002 65002 i"), "{table}");
    // Without the map the default is always sent, and `no` takes both away.
    lab.run(r2, "conf t\nrouter bgp 65002\nno neighbor 10.0.23.2 default-originate route-map DEF\nend");
    advance(&mut lab, 2);
    assert!(!lab.run(r3, "show ip bgp").contains("0.0.0.0/0"));
    assert!(!lab.run(r2, "show running-config").contains("default-originate"));
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.23.2 default-originate\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r3, "show ip bgp").contains("0.0.0.0/0"));
}
