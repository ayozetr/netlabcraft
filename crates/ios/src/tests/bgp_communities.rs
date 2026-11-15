//! BGP communities, community and AS-path lists, aggregation, `maximum-paths` and neighbour
//! timers typed at the console, with what they change in the tables and in the running
//! configuration.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001, owns 172.16.1.0/24 and 172.16.2.0/24) - R2 (AS 65002) - R3 (AS 65003); every link
/// is an external session, and R1 tags 172.16.1.0/24 with 65001:100 on the way to R2.
fn chain() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nint lo2\nip add 172.16.2.1 255.255.255.0\n\
         exit\nip prefix-list ONE permit 172.16.1.0/24\nroute-map TAG permit 10\nmatch ip address prefix-list ONE\nset community 65001:100 additive\nexit\n\
         route-map TAG permit 20\nexit\nrouter bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nneighbor 10.0.12.2 route-map TAG out\n\
         network 172.16.1.0 mask 255.255.255.0\nnetwork 172.16.2.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\n\
         router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nneighbor 10.0.23.2 remote-as 65003\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nexit\nrouter bgp 65003\nbgp router-id 3.3.3.3\nneighbor 10.0.23.1 remote-as 65002\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

fn tagged() -> String {
    format!("{}", (65_001u32 << 16) | 100)
}

#[test]
fn communities_travel_only_with_send_community_and_show_in_both_formats() {
    let (mut lab, r1, r2, _r3) = chain();
    let before = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(!before.contains("communities:"), "{before}");
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 send-community\nend");
    advance(&mut lab, 2);
    let old = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(old.contains(&format!("communities: {}", tagged())), "{old}");
    assert!(!lab.run(r2, "show ip bgp 172.16.2.0").contains("communities:"));
    lab.run(r2, "conf t\nip bgp-community new-format\nend");
    assert!(lab.run(r2, "show ip bgp 172.16.1.0").contains("communities: 65001:100"));
    assert!(lab.run(r2, "show ip bgp community 65001:100").contains("172.16.1.0/24"));
    let none = lab.run(r2, "show ip bgp community 65001:100 exact-match");
    assert!(none.contains("172.16.1.0/24"), "{none}");
    let other = lab.run(r2, "show ip bgp community 65001:999");
    assert!(other.contains("no path carries 65001:999") && !other.contains("172.16.1.0/24"), "{other}");
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("Options: send-community"));
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.12.2 send-community\n"));
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 send-community\nend");
    advance(&mut lab, 2);
    assert!(!lab.run(r2, "show ip bgp 172.16.1.0").contains("communities:"));
}

#[test]
fn no_export_and_local_as_keep_a_path_inside_the_autonomous_system() {
    let (mut lab, r1, r2, r3) = chain();
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 send-community\nend");
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.23.2 send-community\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
    for word in ["no-export", "local-AS"] {
        lab.run(r1, &format!("conf t\nroute-map TAG permit 10\nset community {word}\nend"));
        advance(&mut lab, 2);
        let at_r2 = lab.run(r2, "show ip bgp 172.16.1.0");
        assert!(at_r2.contains(&format!("communities: {word}")), "{at_r2}");
        let table = lab.run(r3, "show ip bgp");
        assert!(!table.contains("172.16.1.0/24") && table.contains("172.16.2.0/24"), "{word}: {table}");
    }
    // no-advertise goes to no neighbour at all, internal or external.
    lab.run(r1, "conf t\nroute-map TAG permit 10\nset community no-advertise\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r2, "show ip bgp community no-advertise").contains("172.16.1.0/24"));
    assert!(!lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
    // A map that gives no community at all lets it travel again.
    lab.run(r1, "conf t\nroute-map TAG permit 10\nset community none\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
}

#[test]
fn a_route_map_matches_a_community_list_and_the_views_show_it() {
    let (mut lab, r1, r2, _r3) = chain();
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 send-community\nend");
    lab.run(
        r2,
        "conf t\nip bgp-community new-format\nip community-list standard TAGGED permit 65001:100\nip community-list expanded EXP permit ^65001:\n\
         ip community-list expanded NOPE permit ^9\nroute-map PREF permit 10\nmatch community TAGGED\nset local-preference 300\nexit\n\
         route-map PREF permit 20\nexit\nrouter bgp 65002\nneighbor 10.0.12.1 route-map PREF in\nend",
    );
    advance(&mut lab, 2);
    let hot = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(hot.contains("local preference 300"), "{hot}");
    let cold = lab.run(r2, "show ip bgp 172.16.2.0");
    assert!(cold.contains("local preference 100"), "{cold}");
    let by_list = lab.run(r2, "show ip bgp community-list TAGGED");
    assert!(by_list.contains("172.16.1.0/24") && !by_list.contains("172.16.2.0/24"), "{by_list}");
    let by_regex = lab.run(r2, "show ip bgp community-list EXP");
    assert!(by_regex.contains("172.16.1.0/24"), "{by_regex}");
    assert!(lab.run(r2, "show ip bgp community-list NOPE").contains("permits no path"));
    assert!(lab.try_run(r2, "show ip bgp community-list MISSING").contains("No community list named MISSING"));
    let lists = lab.run(r2, "show ip community-list");
    assert!(lists.contains("Community list TAGGED (standard): 1 line") && lists.contains("permit 65001:100"), "{lists}");
    assert!(lists.contains("Community list EXP (expanded): 1 line") && lists.contains("permit ^65001:"), "{lists}");
    assert!(lab.run(r2, "show ip community-list TAGGED").contains("permit 65001:100"));
    let map = lab.run(r2, "show route-map PREF");
    assert!(map.contains("match community list: TAGGED") && map.contains("set local-preference 300"), "{map}");
    // A missing list matches nothing, so the path falls to the next clause.
    lab.run(r2, "conf t\nroute-map PREF permit 10\nno match community TAGGED\nmatch community MISSING\nend");
    assert!(lab.run(r2, "show ip bgp 172.16.1.0").contains("local preference 100"));
    // Lines come out one at a time and the whole list goes.
    lab.run(r2, "conf t\nip community-list standard TAGGED deny 65001:5\nno ip community-list standard TAGGED permit 65001:100\nend");
    assert!(lab.run(r2, "show ip community-list TAGGED").contains("deny 65001:5"));
    lab.run(r2, "conf t\nno ip community-list standard TAGGED\nno ip community-list expanded EXP\nend");
    assert!(lab.try_run(r2, "show ip community-list TAGGED").contains("No community list named TAGGED"));
}

#[test]
fn an_as_path_list_filters_a_neighbour_and_a_route_map_matches_it() {
    let (mut lab, _r1, r2, r3) = chain();
    lab.run(r3, "conf t\nip as-path access-list 5 deny _65001$\nip as-path access-list 5 permit .*\nrouter bgp 65003\nneighbor 10.0.23.1 filter-list 5 in\nend");
    advance(&mut lab, 2);
    let table = lab.run(r3, "show ip bgp");
    assert!(!table.contains("172.16.1.0/24") && !table.contains("172.16.2.0/24"), "{table}");
    let lists = lab.run(r3, "show ip as-path-access-list");
    assert!(lists.contains("AS-path list 5: 2 lines") && lists.contains("deny _65001$"), "{lists}");
    assert!(lab.run(r3, "show ip bgp neighbors 10.0.23.1").contains("Policy: inbound AS-path list 5"));
    lab.run(r3, "conf t\nrouter bgp 65003\nno neighbor 10.0.23.1 filter-list 5 in\nend");
    assert!(lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
    lab.run(
        r3,
        "conf t\nip as-path access-list 6 permit ^65002_\nroute-map AP permit 10\nmatch as-path 6\nset local-preference 250\nexit\n\
         router bgp 65003\nneighbor 10.0.23.1 route-map AP in\nend",
    );
    assert!(lab.run(r3, "show ip bgp 172.16.1.0").contains("local preference 250"));
    assert!(lab.run(r3, "show route-map AP").contains("match AS-path list: 6"));
    // The outbound list is checked on the path as the router holds it: only its own prefixes pass.
    lab.run(r2, "conf t\nip as-path access-list 1 permit ^$\nrouter bgp 65002\nneighbor 10.0.23.2 filter-list 1 out\nend");
    assert!(!lab.run(r3, "show ip bgp").contains("172.16.1.0/24"));
    lab.run(r3, "conf t");
    assert!(lab.try_run(r3, "ip as-path access-list 600 permit x").contains("numbered 1-500"));
    assert!(lab.try_run(r3, "ip as-path access-list 7 permit [").contains("regular expression"));
    lab.run(r3, "no ip as-path access-list 6\nend");
    assert!(lab.try_run(r3, "show ip as-path-access-list 6").contains("No AS-path list numbered 6"));
}

#[test]
fn an_aggregate_replaces_the_specific_prefixes_with_summary_only_and_carries_an_as_set() {
    let (mut lab, _r1, r2, r3) = chain();
    lab.run(r2, "conf t\nrouter bgp 65002\naggregate-address 172.16.0.0 255.255.252.0 summary-only as-set\nend");
    advance(&mut lab, 2);
    let at_r3 = lab.run(r3, "show ip bgp");
    assert!(at_r3.contains("172.16.0.0/22") && at_r3.contains("65002 {65001} i"), "{at_r3}");
    assert!(!at_r3.contains("172.16.1.0/24") && !at_r3.contains("172.16.2.0/24"), "{at_r3}");
    // R2 keeps the specific prefixes itself, and says why they are not advertised.
    let own = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(own.contains("not advertised: an aggregate with summary-only covers it"), "{own}");
    let agg = lab.run(r2, "show ip bgp 172.16.0.0 255.255.252.0");
    assert!(agg.contains("originated here") && agg.contains("AS set {65001}"), "{agg}");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains(" aggregate-address 172.16.0.0 255.255.252.0 summary-only as-set\n"), "{run}");
    // Without summary-only the specific prefixes go out too.
    lab.run(r2, "conf t\nrouter bgp 65002\naggregate-address 172.16.0.0 255.255.252.0\nend");
    advance(&mut lab, 2);
    let at_r3 = lab.run(r3, "show ip bgp");
    assert!(at_r3.contains("172.16.0.0/22") && at_r3.contains("172.16.1.0/24") && !at_r3.contains("{65001}"), "{at_r3}");
    // The aggregate goes when nothing is left under it.
    lab.run(r2, "conf t\nrouter bgp 65002\nno aggregate-address 172.16.0.0 255.255.252.0\nend");
    advance(&mut lab, 2);
    assert!(!lab.run(r3, "show ip bgp").contains("172.16.0.0/22"));
    lab.run(r2, "conf t\nrouter bgp 65002");
    assert!(lab.try_run(r2, "no aggregate-address 172.16.0.0 255.255.252.0").contains("No such aggregate"));
    assert!(lab.try_run(r2, "aggregate-address 172.16.0.0 255.0.255.0").contains("contiguous"));
    assert!(lab.try_run(r2, "aggregate-address 172.16.0.0 255.255.252.0 bogus").contains("Unexpected"));
}

#[test]
fn maximum_paths_puts_equal_external_paths_in_the_routing_table() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(
        r1,
        "en\nconf t\nint g0/0\nip add 10.0.1.1 255.255.255.252\nno shut\nint g0/1\nip add 10.0.2.1 255.255.255.252\nno shut\nexit\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.1.2 remote-as 65002\nneighbor 10.0.2.2 remote-as 65002\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/0\nip add 10.0.1.2 255.255.255.252\nno shut\nint g0/1\nip add 10.0.2.2 255.255.255.252\nno shut\nint lo1\nip add 192.168.2.1 255.255.255.0\n\
         exit\nrouter bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.1.1 remote-as 65001\nneighbor 10.0.2.1 remote-as 65001\nnetwork 192.168.2.0 mask 255.255.255.0\nend",
    );
    advance(&mut lab, 8);
    let one = lab.run(r1, "show ip route bgp");
    assert!(one.contains("192.168.2.0/24") && !(one.contains("via 10.0.1.2") && one.contains("via 10.0.2.2")), "{one}");
    lab.run(r1, "conf t\nrouter bgp 65001\nmaximum-paths 2\nend");
    let two = lab.run(r1, "show ip route bgp");
    assert!(two.contains("via 10.0.1.2") && two.contains("via 10.0.2.2"), "{two}");
    let entry = lab.run(r1, "show ip bgp 192.168.2.0");
    assert!(entry.contains(": 2 paths") && entry.contains("shares the load with the best path"), "{entry}");
    assert!(lab.run(r1, "show ip bgp").contains("*m"));
    assert!(lab.run(r1, "show running-config").contains(" maximum-paths 2\n"));
    // Traffic to the prefix is still delivered over the shared paths.
    assert!(lab.run(r1, "ping 192.168.2.1").contains("!"));
    lab.run(r1, "conf t\nrouter bgp 65001\nno maximum-paths\nend");
    let back = lab.run(r1, "show ip route bgp");
    assert!(!(back.contains("via 10.0.1.2") && back.contains("via 10.0.2.2")), "{back}");
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "maximum-paths 99").contains("1-32"));
}

/// R1 (AS 65001) - R2 (no BGP) - R3 (AS 65003); the two speakers are two hops apart, reach each
/// other through static routes and have 5-second keepalives with a 15-second hold time.
fn far_apart() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nexit\nip route 10.0.23.0 255.255.255.252 10.0.12.2\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nbgp log-neighbor-changes\nneighbor 10.0.23.2 remote-as 65003\nneighbor 10.0.23.2 ebgp-multihop 2\n\
         neighbor 10.0.23.2 timers 5 15\nend",
    );
    lab.run(r2, "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nend");
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nint lo1\nip add 172.16.3.1 255.255.255.0\nexit\n\
         ip route 10.0.12.0 255.255.255.252 10.0.23.1\nrouter bgp 65003\nbgp router-id 3.3.3.3\nbgp log-neighbor-changes\nneighbor 10.0.12.1 remote-as 65001\n\
         neighbor 10.0.12.1 ebgp-multihop 2\nneighbor 10.0.12.1 timers 5 15\nnetwork 172.16.3.0 mask 255.255.255.0\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

fn keepalives_sent(text: &str) -> u32 {
    let at = text.find("Messages sent").expect("sent line");
    let tail = &text[at..];
    let k = tail.find("keepalives ").expect("keepalives");
    tail[k + "keepalives ".len()..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().expect("number")
}

#[test]
fn custom_timers_set_the_keepalive_interval_and_the_hold_time() {
    let (mut lab, r1, _r2, r3) = far_apart();
    let n = lab.run(r1, "show ip bgp neighbors 10.0.23.2");
    assert!(n.contains("State: Established") && n.contains("Hold time 15 s, keepalive every 5 s") && n.contains("timers 5 15"), "{n}");
    assert!(lab.run(r3, "show ip bgp neighbors 10.0.12.1").contains("Hold time 15 s, keepalive every 5 s"));
    assert!(lab.run(r1, "show ip route bgp").contains("172.16.3.0/24"));
    let before = keepalives_sent(&n);
    advance(&mut lab, 20);
    let after = keepalives_sent(&lab.run(r1, "show ip bgp neighbors 10.0.23.2"));
    assert!(after >= before + 3, "{before} -> {after}");
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.23.2 timers 5 15\n"));
    // The smaller hold time wins, and each side keeps keepalives to a third of it.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.23.2 timers 30 90\nend");
    let n = lab.run(r1, "show ip bgp neighbors 10.0.23.2");
    assert!(n.contains("Hold time 15 s, keepalive every 5 s"), "{n}");
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.23.2 timers\nend");
    assert!(!lab.run(r1, "show running-config").contains("timers"));
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "neighbor 10.0.23.2 timers 5 2").contains("hold time"));
    assert!(lab.try_run(r1, "neighbor 10.0.23.2 timers x 30").contains("keepalive"));
    assert!(lab.try_run(r1, "neighbor 10.0.23.2 timers 60 180").is_empty());
    assert!(!lab.run(r1, "end\nshow running-config").contains("timers"), "the defaults are not written");
}

#[test]
fn a_session_whose_path_is_lost_waits_for_its_hold_time_and_then_drops() {
    let (mut lab, r1, r2, _r3) = far_apart();
    lab.run(r2, "conf t\nint g0/2\nshutdown\nend");
    // The speakers cannot hear each other, but neither has noticed yet.
    let n = lab.run(r1, "show ip bgp neighbors 10.0.23.2");
    assert!(n.contains("State: Established"), "{n}");
    assert!(lab.run(r1, "show ip route bgp").contains("172.16.3.0/24"));
    advance(&mut lab, 10);
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.23.2").contains("State: Established"));
    advance(&mut lab, 10);
    let n = lab.run(r1, "show ip bgp neighbors 10.0.23.2");
    assert!(!n.contains("State: Established"), "{n}");
    assert!(!lab.run(r1, "show ip route bgp").contains("172.16.3.0/24"));
    let log = lab.run(r1, "show logging");
    assert!(log.contains("%BGP-5-ADJCHANGE: neighbor 10.0.23.2 Down hold timer expired"), "{log}");
    // The path comes back and the session starts again.
    lab.run(r2, "conf t\nint g0/2\nno shutdown\nend");
    advance(&mut lab, 8);
    assert!(lab.run(r1, "show ip route bgp").contains("172.16.3.0/24"));
}

#[test]
fn a_path_that_comes_back_before_the_hold_time_keeps_the_session() {
    let (mut lab, r1, r2, _r3) = far_apart();
    lab.run(r2, "conf t\nint g0/2\nshutdown\nend");
    advance(&mut lab, 6);
    lab.run(r2, "conf t\nint g0/2\nno shutdown\nend");
    advance(&mut lab, 30);
    let n = lab.run(r1, "show ip bgp neighbors 10.0.23.2");
    assert!(n.contains("State: Established"), "{n}");
    assert!(!lab.run(r1, "show logging").contains("ADJCHANGE: neighbor 10.0.23.2 Down"), "the session never dropped");
    assert!(lab.run(r1, "show ip route bgp").contains("172.16.3.0/24"));
}

#[test]
fn the_configuration_is_written_out_and_bad_input_is_refused() {
    let (mut lab, r1, ..) = chain();
    lab.run(
        r1,
        "conf t\nip bgp-community new-format\nip community-list standard C1 permit 65001:100 no-export\nip community-list expanded C2 deny _65001_\n\
         ip as-path access-list 5 permit ^65002_\nroute-map M permit 10\nmatch community C1\nmatch as-path 5\nset community 65001:7 internet additive\nexit\n\
         router bgp 65001\nneighbor 10.0.12.2 timers 10 30\nneighbor 10.0.12.2 send-community\nneighbor 10.0.12.2 filter-list 5 in\n\
         aggregate-address 172.16.0.0 255.255.252.0 summary-only\nmaximum-paths 4\nend",
    );
    let run = lab.run(r1, "show running-config");
    let expect = [
        "ip bgp-community new-format\nip community-list standard C1 permit 65001:100 no-export\nip community-list expanded C2 deny _65001_\n\
         ip as-path access-list 5 permit ^65002_\n!",
        "route-map M permit 10\n match community C1\n match as-path 5\n set community internet 65001:7 additive\n!",
        " neighbor 10.0.12.2 timers 10 30\n neighbor 10.0.12.2 send-community\n neighbor 10.0.12.2 filter-list 5 in\n neighbor 10.0.12.2 route-map TAG out\n",
        " aggregate-address 172.16.0.0 255.255.252.0 summary-only\n maximum-paths 4\n",
    ];
    for line in expect {
        assert!(run.contains(line), "{line}\n---\n{run}");
    }
    lab.run(r1, "copy run start");
    lab.run(r1, "reload");
    assert_eq!(lab.run(r1, "en\nshow running-config"), run);
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "ip community-list standard X permit bogus").contains("is not a community"));
    assert!(lab.try_run(r1, "ip community-list standard X permit").contains("Name at least one community"));
    assert!(lab.try_run(r1, "ip community-list expanded X permit [").contains("regular expression"));
    assert!(lab.try_run(r1, "ip community-list expanded C1 permit ^1").contains("already a standard"));
    assert!(lab.try_run(r1, "ip community-list 700 permit 1:1").contains("1-500"));
    assert!(lab.try_run(r1, "ip community-list frob X permit 1:1").contains("ip community-list"));
    assert!(lab.try_run(r1, "no ip community-list standard NOPE").contains("No community list named NOPE"));
    lab.run(r1, "route-map M permit 10");
    assert!(lab.try_run(r1, "match community C1 exact-match").contains("exact-match"));
    assert!(lab.try_run(r1, "set community none additive").contains("cannot be additive"));
    assert!(lab.try_run(r1, "set community 65536:1").contains("is not a community"));
    lab.run(r1, "set community none");
    assert!(lab.run(r1, "end\nshow route-map M").contains("set community none"));
    lab.run(r1, "conf t\nroute-map M permit 10\nno set community\nno match community\nno match as-path\nend");
    let map = lab.run(r1, "show route-map M");
    assert!(map.contains("no match lines") && !map.contains("set community"), "{map}");
    lab.run(r1, "conf t\nno ip bgp-community new-format\nend");
    assert!(!lab.run(r1, "show running-config").contains("new-format"));
}
