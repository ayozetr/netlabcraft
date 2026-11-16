//! Route reflectors, confederations and the other neighbour options (`default-originate`,
//! `remove-private-as`, `soft-reconfiguration inbound`, `bgp always-compare-med`, ...) typed at the
//! console, with what they change in the tables and in the running configuration.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 - R2 - R3, all in AS 65001 and nothing else: R2 sits between the two and is the route
/// reflector once it is told so. R1 owns 172.16.1.0/24 and R3 owns 172.16.3.0/24. R3 reaches R1's
/// next hop through a static route (there is no IGP).
fn hub() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nexit\n\
         ip route 10.0.23.0 255.255.255.252 10.0.12.2\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65001\nnetwork 172.16.1.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\n\
         router bgp 65001\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nneighbor 10.0.23.2 remote-as 65001\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nint lo3\nip add 172.16.3.1 255.255.255.0\nexit\n\
         ip route 10.0.12.0 255.255.255.252 10.0.23.1\n\
         router bgp 65001\nbgp router-id 3.3.3.3\nneighbor 10.0.23.1 remote-as 65001\nnetwork 172.16.3.0 mask 255.255.255.0\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

#[test]
fn without_a_reflector_internal_paths_stop_at_the_next_router() {
    let (mut lab, _r1, r2, r3) = hub();
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    let at_r3 = lab.run(r3, "show ip bgp");
    assert!(!at_r3.contains("172.16.1.0/24") && at_r3.contains("172.16.3.0/24"), "{at_r3}");
}

#[test]
fn a_reflector_passes_a_clients_paths_on_with_the_originator_and_the_cluster() {
    let (mut lab, r1, r2, r3) = hub();
    lab.run(r2, "conf t\nrouter bgp 65001\nneighbor 10.0.12.1 route-reflector-client\nneighbor 10.0.23.2 route-reflector-client\nend");
    advance(&mut lab, 2);
    let at_r3 = lab.run(r3, "show ip bgp");
    assert!(at_r3.contains("172.16.1.0/24"), "{at_r3}");
    let entry = lab.run(r3, "show ip bgp 172.16.1.0");
    assert!(entry.contains("originator 1.1.1.1, cluster list: 2.2.2.2"), "{entry}");
    assert!(entry.contains("internal") && entry.contains("usable"), "{entry}");
    assert!(lab.run(r3, "show ip route").contains("172.16.1.0/24"));
    // The path goes the other way too, and R1 is not told about itself.
    let at_r1 = lab.run(r1, "show ip bgp");
    assert!(at_r1.contains("172.16.3.0/24"), "{at_r1}");
    assert!(lab.run(r1, "show ip bgp 172.16.1.0").contains("1 path"), "R1 gets nothing back for its own prefix");
    // A cluster id replaces the router id in the list; a number is read as an address.
    lab.run(r2, "conf t\nrouter bgp 65001\nbgp cluster-id 9.9.9.9\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r3, "show ip bgp 172.16.1.0").contains("cluster list: 9.9.9.9"));
    lab.run(r2, "conf t\nrouter bgp 65001\nbgp cluster-id 99\nend");
    assert!(lab.run(r2, "show running-config").contains(" bgp cluster-id 0.0.0.99\n"));
    // The views and the running configuration name the reflector.
    let summary = lab.run(r2, "show ip bgp summary");
    assert!(summary.contains("Route reflector for cluster 0.0.0.99, 2 clients"), "{summary}");
    let neighbors = lab.run(r2, "show ip bgp neighbors 10.0.12.1");
    assert!(neighbors.contains("Options: route-reflector-client"), "{neighbors}");
    let config = lab.run(r2, "show running-config");
    assert!(config.contains(" neighbor 10.0.12.1 route-reflector-client\n") && config.contains(" neighbor 10.0.23.2 route-reflector-client\n"));
    // Taking the client setting off stops the reflection.
    lab.run(r2, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.1 route-reflector-client\nno neighbor 10.0.23.2 route-reflector-client\nno bgp cluster-id\nend");
    advance(&mut lab, 2);
    assert!(!lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
    assert!(!lab.run(r2, "show running-config").contains("route-reflector-client"));
}

#[test]
fn a_non_client_path_is_reflected_to_clients_only() {
    let (mut lab, r1, r2, r3) = hub();
    // R1 is a client, R3 is not: R3's path goes to R1, R1's path goes to R3 (a client's path goes to everyone).
    lab.run(r2, "conf t\nrouter bgp 65001\nneighbor 10.0.12.1 route-reflector-client\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r1, "show ip bgp").contains("172.16.3.0/24"));
    assert!(lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
    // With neither a client, nothing is passed on.
    lab.run(r2, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.1 route-reflector-client\nend");
    advance(&mut lab, 2);
    assert!(!lab.run(r1, "show ip bgp").contains("172.16.3.0/24"));
    // With R3 the client and R1 not, R1's path (a non-client's) reaches R3 and R3's (a client's) reaches R1.
    lab.run(r2, "conf t\nrouter bgp 65001\nneighbor 10.0.23.2 route-reflector-client\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
    assert!(lab.run(r1, "show ip bgp").contains("172.16.3.0/24"));
}

#[test]
fn a_client_must_be_an_internal_neighbour_that_exists() {
    let (mut lab, _r1, r2, _r3) = hub();
    lab.run(r2, "conf t\nrouter bgp 65001\nneighbor 10.9.9.9 remote-as 65009");
    let outside = lab.try_run(r2, "neighbor 10.9.9.9 route-reflector-client");
    assert!(outside.contains("internal neighbour"), "{outside}");
    let unnamed = lab.try_run(r2, "neighbor 10.8.8.8 route-reflector-client");
    assert!(unnamed.contains("Name the neighbour first"), "{unnamed}");
    assert!(lab.try_run(r2, "bgp cluster-id 0").contains("cluster id is"));
    assert!(lab.try_run(r2, "bgp cluster-id 999.1.1.1").starts_with('%'));
}

/// R1, R2 and R3 in one AS in a triangle; R2 and R3 are reflectors and R1 is a client of both.
fn triangle(same_cluster: bool) -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r1, "g0/2", r3, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    let (c2, c3) =
        if same_cluster { ("bgp cluster-id 9.9.9.9", "bgp cluster-id 9.9.9.9") } else { ("bgp cluster-id 2.2.2.2", "bgp cluster-id 3.3.3.3") };
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint g0/2\nip add 10.0.13.1 255.255.255.252\nno shut\n\
         int lo1\nip add 172.16.1.1 255.255.255.0\nexit\nrouter bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65001\n\
         neighbor 10.0.13.2 remote-as 65001\nnetwork 172.16.1.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\n\
             router bgp 65001\nbgp router-id 2.2.2.2\n{c2}\nneighbor 10.0.12.1 remote-as 65001\nneighbor 10.0.12.1 route-reflector-client\n\
             neighbor 10.0.23.2 remote-as 65001\nend"
        ),
    );
    lab.run(
        r3,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.13.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nexit\n\
             router bgp 65001\nbgp router-id 3.3.3.3\n{c3}\nneighbor 10.0.13.1 remote-as 65001\nneighbor 10.0.13.1 route-reflector-client\n\
             neighbor 10.0.23.1 remote-as 65001\nend"
        ),
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

#[test]
fn a_cluster_list_stops_a_path_from_coming_back_to_the_cluster() {
    // Two reflectors with their own cluster ids each tell the other about R1's path: R3 hears it twice.
    let (mut lab, r1, _r2, r3) = triangle(false);
    let twice = lab.run(r3, "show ip bgp 172.16.1.0");
    assert!(twice.contains("2 paths") && twice.contains("cluster list: 2.2.2.2"), "{twice}");
    // R1 never takes its own path back: the originator is its own router id.
    assert!(lab.run(r1, "show ip bgp 172.16.1.0").contains("1 path"));
    // With one cluster id the reflected copy carries R3's own cluster and is dropped.
    let (mut lab, _r1, _r2, r3) = triangle(true);
    let once = lab.run(r3, "show ip bgp 172.16.1.0");
    assert!(once.contains("1 path") && !once.contains("cluster list"), "{once}");
}

/// R1 (AS 65001, in confederation 100) - R2 (AS 65002, in confederation 100) - R3 (AS 200, outside).
fn confed() -> (Lab, DeviceId, DeviceId, DeviceId) {
    confed_with(65_002, true)
}

/// [`confed`] where R1 names `r1_names` as the AS of R2, and R2 lists R1's member AS in its
/// `bgp confederation peers` only when `r2_lists_r1` is true.
fn confed_with(r1_names: u32, r2_lists_r1: bool) -> (Lab, DeviceId, DeviceId, DeviceId) {
    let peers = if r2_lists_r1 { "bgp confederation peers 65001\n" } else { "" };
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.run(
        r1,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nexit\n\
             router bgp 65001\nbgp router-id 1.1.1.1\nbgp confederation identifier 100\nbgp confederation peers 65002\n\
             neighbor 10.0.12.2 remote-as {r1_names}\nnetwork 172.16.1.0 mask 255.255.255.0\nend"
        ),
    );
    lab.run(
        r2,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\n\
             router bgp 65002\nbgp router-id 2.2.2.2\nbgp confederation identifier 100\n{peers}\
             neighbor 10.0.12.1 remote-as 65001\nneighbor 10.0.23.2 remote-as 200\nend"
        ),
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nint lo3\nip add 172.16.3.1 255.255.255.0\nexit\n\
         router bgp 200\nbgp router-id 3.3.3.3\nneighbor 10.0.23.1 remote-as 100\nnetwork 172.16.3.0 mask 255.255.255.0\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

#[test]
fn a_confederation_hides_its_member_autonomous_systems_from_the_outside() {
    let (mut lab, r1, r2, r3) = confed();
    // Inside: the member link is a confederation session and the path carries the member AS.
    let link = lab.run(r2, "show ip bgp neighbors 10.0.12.1");
    assert!(link.contains("confederation link") && link.contains("State: Established"), "{link}");
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.1.0/24") && table.contains("(65001) i"), "{table}");
    assert!(table.contains(" c "), "the status marks a path from another member AS: {table}");
    let entry = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(entry.contains("confederation") && entry.contains("confederation path: 65001"), "{entry}");
    // Its distance is the internal one.
    assert!(lab.run(r2, "show ip route").contains("[200/0]"));
    // Outside: the confederation identifier stands for it, with no member AS.
    let outside = lab.run(r3, "show ip bgp");
    assert!(outside.contains("172.16.1.0/24") && outside.contains(" 100 i") && !outside.contains("65001") && !outside.contains('('), "{outside}");
    assert!(lab.run(r3, "show ip bgp neighbors 10.0.23.1").contains("remote AS 100 (external link)"));
    // The other way: the outside AS comes in as a plain AS path, with the member AS added on the way inside.
    let inner = lab.run(r1, "show ip bgp 172.16.3.0");
    assert!(inner.contains("AS path 200") && inner.contains("confederation path: 65002"), "{inner}");
    assert!(inner.contains("local preference 100"), "{inner}");
    // The views and the running configuration.
    let summary = lab.run(r1, "show ip bgp summary");
    assert!(summary.contains("Confederation identifier 100, member AS 65001, member peers: 65002"), "{summary}");
    let config = lab.run(r1, "show running-config");
    assert!(config.contains(" bgp confederation identifier 100\n") && config.contains(" bgp confederation peers 65002\n"), "{config}");
}

#[test]
fn the_confederation_settings_decide_whether_a_member_session_comes_up() {
    let up = |lab: &mut Lab, r1: DeviceId| lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("State: Established");
    let (mut lab, r1, _r2, _r3) = confed();
    assert!(up(&mut lab, r1));
    // A neighbour that names the identifier instead of the member AS is refused.
    let (mut lab2, s1, _s2, _s3) = confed_with(100, true);
    assert!(!up(&mut lab2, s1));
    // Without the member AS in `bgp confederation peers` on one side nothing comes up either.
    let (mut lab3, t1, _t2, _t3) = confed_with(65_002, false);
    assert!(!up(&mut lab3, t1));
    // Settings that make no sense are refused.
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "bgp confederation identifier 65001").contains("must differ"));
    assert!(lab.try_run(r1, "bgp confederation peers 65001").contains("own member AS"));
    assert!(lab.try_run(r1, "bgp confederation identifier 0").starts_with('%'));
    lab.run(r1, "no bgp confederation peers\nno bgp confederation identifier\nend");
    let config = lab.run(r1, "show running-config");
    assert!(!config.contains("confederation"), "{config}");
}

/// R1 (AS 65001) - R2 (AS 65002) - R3 (AS 3000); R1 owns two prefixes.
fn edge() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nint lo2\nip add 172.16.2.1 255.255.255.0\nexit\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nnetwork 172.16.1.0 mask 255.255.255.0\n\
         network 172.16.2.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\n\
         router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nneighbor 10.0.23.2 remote-as 3000\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nexit\nrouter bgp 3000\nbgp router-id 3.3.3.3\nneighbor 10.0.23.1 remote-as 65002\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

#[test]
fn private_as_numbers_can_be_taken_out_of_the_path_sent_outside() {
    let (mut lab, _r1, r2, r3) = edge();
    assert!(lab.run(r3, "show ip bgp").contains("65002 65001 i"));
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.23.2 remove-private-as\nend");
    advance(&mut lab, 2);
    // R2's own AS is private too, but it is added after the removal.
    let table = lab.run(r3, "show ip bgp");
    assert!(table.contains("65002 i") && !table.contains("65002 65001"), "{table}");
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.23.2").contains("remove-private-as"));
    assert!(lab.run(r2, "show running-config").contains(" neighbor 10.0.23.2 remove-private-as\n"));
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.23.2 remove-private-as all\nend");
    assert!(lab.run(r2, "show running-config").contains(" neighbor 10.0.23.2 remove-private-as all\n"));
    lab.run(r2, "conf t\nrouter bgp 65002");
    assert!(lab.try_run(r2, "neighbor 10.0.23.2 remove-private-as some").starts_with('%'));
    lab.run(r2, "no neighbor 10.0.23.2 remove-private-as\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r3, "show ip bgp").contains("65002 65001 i"));
    assert!(!lab.run(r2, "show running-config").contains("remove-private-as"));
}

#[test]
fn default_originate_sends_a_default_route_whether_or_not_there_is_one() {
    let (mut lab, _r1, r2, r3) = edge();
    assert!(!lab.run(r3, "show ip bgp").contains("0.0.0.0/0"));
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.23.2 default-originate\nend");
    advance(&mut lab, 2);
    let table = lab.run(r3, "show ip bgp");
    assert!(table.contains("0.0.0.0/0") && table.contains("10.0.23.1"), "{table}");
    assert!(lab.run(r3, "show ip route").contains("0.0.0.0/0"));
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.23.2").contains("default-originate"));
    assert!(lab.run(r2, "show running-config").contains(" neighbor 10.0.23.2 default-originate\n"));
    lab.run(r2, "conf t\nrouter bgp 65002\nno neighbor 10.0.23.2 default-originate\nend");
    advance(&mut lab, 2);
    assert!(!lab.run(r3, "show ip bgp").contains("0.0.0.0/0"));
}

#[test]
fn soft_reconfiguration_keeps_what_the_inbound_policy_dropped() {
    let (mut lab, _r1, r2, _r3) = edge();
    lab.run(r2, "conf t\nip prefix-list TWO permit 172.16.2.0/24\nrouter bgp 65002\nneighbor 10.0.12.1 prefix-list TWO in\nend");
    advance(&mut lab, 2);
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.2.0/24") && !table.contains("172.16.1.0/24"), "{table}");
    // Nothing is kept until it is asked for.
    let refused = lab.try_run(r2, "show ip bgp neighbors 10.0.12.1 received-routes");
    assert!(refused.contains("soft-reconfiguration inbound"), "{refused}");
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 soft-reconfiguration inbound\nend");
    advance(&mut lab, 2);
    let kept = lab.run(r2, "show ip bgp neighbors 10.0.12.1 received-routes");
    assert!(kept.contains("172.16.1.0/24") && kept.contains("172.16.2.0/24") && kept.contains("65001 i"), "{kept}");
    // The table still follows the policy.
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    let detail = lab.run(r2, "show ip bgp neighbors 10.0.12.1");
    assert!(detail.contains("soft-reconfiguration inbound") && detail.contains("Kept before the inbound policy: 2"), "{detail}");
    assert!(lab.run(r2, "show running-config").contains(" neighbor 10.0.12.1 soft-reconfiguration inbound\n"));
    assert!(lab.try_run(r2, "show ip bgp neighbors 10.7.7.7 received-routes").contains("No neighbour"));
    lab.run(r2, "conf t\nrouter bgp 65002\nno neighbor 10.0.12.1 soft-reconfiguration inbound\nend");
    assert!(lab.try_run(r2, "show ip bgp neighbors 10.0.12.1 received-routes").starts_with('%'));
}

#[test]
fn show_ip_bgp_can_be_filtered_by_a_regular_expression_or_an_as_path_list() {
    let (mut lab, _r1, _r2, r3) = edge();
    let by_regexp = lab.run(r3, "show ip bgp regexp _65001$");
    assert!(by_regexp.contains("172.16.1.0/24") && by_regexp.contains("172.16.2.0/24"), "{by_regexp}");
    let none = lab.run(r3, "show ip bgp regexp ^65001");
    assert!(none.contains("no AS path matches") && !none.contains("172.16.1.0/24"), "{none}");
    assert!(lab.try_run(r3, "show ip bgp regexp [").contains("not an expression"));
    lab.run(r3, "conf t\nip as-path access-list 7 permit ^65002_\nend");
    let by_list = lab.run(r3, "show ip bgp filter-list 7");
    assert!(by_list.contains("172.16.1.0/24"), "{by_list}");
    assert!(lab.try_run(r3, "show ip bgp filter-list 8").contains("No AS-path list 8"));
    assert!(lab.try_run(r3, "show ip bgp filter-list 900").contains("numbered 1-500"));
}

#[test]
fn an_aggregate_leaves_a_discard_route_in_the_routing_table() {
    let (mut lab, _r1, r2, _r3) = edge();
    let before = lab.run(r2, "show ip route");
    assert!(!before.contains("Null0"), "{before}");
    lab.run(r2, "conf t\nrouter bgp 65002\naggregate-address 172.16.0.0 255.255.252.0\nend");
    advance(&mut lab, 2);
    let routes = lab.run(r2, "show ip route");
    assert!(routes.contains("172.16.0.0/22") && routes.contains("summary, discarded on Null0 (BGP) [200/0]"), "{routes}");
    // The more specific routes still lead to R1.
    assert!(routes.contains("172.16.1.0/24"), "{routes}");
    lab.run(r2, "conf t\nrouter bgp 65002\nno aggregate-address 172.16.0.0 255.255.252.0\nend");
    advance(&mut lab, 2);
    assert!(!lab.run(r2, "show ip route").contains("Null0"));
}

#[test]
fn the_best_path_options_and_internal_multipath_are_kept_in_the_running_configuration() {
    let (mut lab, r1, _r2, _r3) = edge();
    lab.run(
        r1,
        "conf t\nrouter bgp 65001\nbgp always-compare-med\nbgp bestpath as-path ignore\nmaximum-paths ibgp 4\nbgp confederation peers 65007 65008\nend",
    );
    let config = lab.run(r1, "show running-config");
    for line in [" bgp always-compare-med\n", " bgp bestpath as-path ignore\n", " maximum-paths ibgp 4\n", " bgp confederation peers 65007 65008\n"] {
        assert!(config.contains(line), "{line}: {config}");
    }
    lab.run(
        r1,
        "conf t\nrouter bgp 65001\nno bgp always-compare-med\nno bgp bestpath as-path ignore\nno maximum-paths ibgp\nno bgp confederation peers 65007\nend",
    );
    let config = lab.run(r1, "show running-config");
    assert!(!config.contains("always-compare-med") && !config.contains("as-path ignore") && !config.contains("maximum-paths"), "{config}");
    assert!(config.contains(" bgp confederation peers 65008\n"), "{config}");
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "maximum-paths ibgp 99").contains("1-32"));
}
