//! BGP leftovers typed at the console: a neighbour's `distribute-list`, the address-family lines and
//! `neighbor … activate` under `no bgp default ipv4-unicast`, process settings written as `bgp …`
//! words, and the per-neighbour settings that are only kept and shown.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001, owns 172.16.1.0/24 and 172.16.2.0/24) - R2 (AS 65002), one external session.
fn pair() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nint lo2\nip add 172.16.2.1 255.255.255.0\n\
         exit\nrouter bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\n\
         network 172.16.1.0 mask 255.255.255.0\nnetwork 172.16.2.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nexit\nrouter bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2)
}

#[test]
fn a_neighbours_distribute_list_filters_what_it_sends_and_what_it_is_sent() {
    let (mut lab, r1, r2) = pair();
    let both = lab.run(r2, "show ip bgp");
    assert!(both.contains("172.16.1.0/24") && both.contains("172.16.2.0/24"), "{both}");
    lab.run(r2, "conf t\naccess-list 10 permit 172.16.1.0 0.0.0.255\nrouter bgp 65002\nneighbor 10.0.12.1 distribute-list 10 in\nend");
    advance(&mut lab, 2);
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.1.0/24") && !table.contains("172.16.2.0/24"), "inbound list: {table}");
    assert!(lab.run(r2, "show running-config").contains(" neighbor 10.0.12.1 distribute-list 10 in\n"));
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.12.1").contains("inbound access list 10"), "the neighbour view names the list");
    lab.run(r2, "conf t\nrouter bgp 65002\nno neighbor 10.0.12.1 distribute-list 10 in\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r2, "show ip bgp").contains("172.16.2.0/24"), "removed");
    // The same list on the sender, outbound.
    lab.run(r1, "conf t\naccess-list 20 permit 172.16.2.0 0.0.0.255\nrouter bgp 65001\nneighbor 10.0.12.2 distribute-list 20 out\nend");
    advance(&mut lab, 2);
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.2.0/24") && !table.contains("172.16.1.0/24"), "outbound list: {table}");
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "no neighbor 10.0.12.2 distribute-list 21 out").starts_with("% "), "only the list that is applied");
}

#[test]
fn without_the_default_ipv4_unicast_a_neighbour_must_be_activated() {
    let (mut lab, r1, r2) = pair();
    lab.run(r1, "conf t\nrouter bgp 65001\nno bgp default ipv4-unicast\nend");
    lab.run(r2, "conf t\nrouter bgp 65002\nno bgp default ipv4-unicast\nend");
    advance(&mut lab, 4);
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.1.0/24"), "nothing is exchanged before the neighbour is activated");
    lab.run(r1, "conf t\nrouter bgp 65001\naddress-family ipv4 unicast\nneighbor 10.0.12.2 activate\nexit-address-family\nend");
    lab.run(r2, "conf t\nrouter bgp 65002\naddress-family ipv4\nneighbor 10.0.12.1 activate\nexit-address-family\nend");
    advance(&mut lab, 4);
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"), "activated at both ends");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains(" no bgp default ipv4-unicast\n"), "{run}");
    assert!(run.contains(" address-family ipv4\n  neighbor 10.0.12.2 activate\n exit-address-family\n"), "{run}");
    // One end deactivates: the routes go.
    lab.run(r2, "conf t\nrouter bgp 65002\nno neighbor 10.0.12.1 activate\nend");
    advance(&mut lab, 4);
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    // Only IPv4 unicast exists.
    lab.run(r2, "conf t\nrouter bgp 65002");
    let v6 = lab.try_run(r2, "address-family ipv6 unicast");
    assert!(v6.starts_with("% Only the IPv4 unicast"), "{v6}");
}

#[test]
fn process_and_neighbour_settings_are_kept_replaced_and_written_back() {
    let (mut lab, r1, _r2) = pair();
    lab.run(r1, "conf t\nrouter bgp 65001");
    let said = lab.run(r1, "bgp deterministic-med");
    assert!(said.contains("Accepted"), "a setting with no effect says so: {said}");
    assert!(lab.run(r1, "bgp bestpath med missing-as-worst").is_empty(), "one the simulation reads says nothing");
    lab.run(r1, "bgp bestpath as-path multipath-relax\nno bgp fast-external-fallover\nno bgp client-to-client reflection\nno synchronization");
    lab.run(r1, "neighbor 10.0.12.2 version 4\nneighbor 10.0.12.2 maximum-prefix 100 80 warning-only\nneighbor 10.0.12.2 maximum-prefix 200\nneighbor 10.0.12.2 allowas-in 2");
    lab.run(r1, "neighbor 10.0.12.2 transport connection-mode passive\nneighbor 10.0.12.2 ttl-security hops 2\nneighbor 10.0.12.2 as-override");
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 maximum-prefix 0").starts_with("% "), "1 at least");
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 transport connection-mode both").starts_with("% Say one of"));
    assert!(lab.try_run(r1, "neighbor 10.0.12.9 version 4").starts_with("% "), "the neighbour must exist");
    assert!(lab.run(r1, "neighbor 10.0.12.2 send-community extended").is_empty(), "extended communities are simulated now");
    lab.run(r1, "end");
    let run = lab.run(r1, "show running-config");
    for line in [
        " bgp deterministic-med\n",
        " bgp bestpath med missing-as-worst\n",
        " bgp bestpath as-path multipath-relax\n",
        " no bgp fast-external-fallover\n",
        " no bgp client-to-client reflection\n",
        " neighbor 10.0.12.2 version 4\n",
        " neighbor 10.0.12.2 maximum-prefix 200\n",
        " neighbor 10.0.12.2 allowas-in 2\n",
        " neighbor 10.0.12.2 transport connection-mode passive\n",
        " neighbor 10.0.12.2 ttl-security hops 2\n",
        " neighbor 10.0.12.2 as-override\n",
        " neighbor 10.0.12.2 send-community extended\n",
    ] {
        assert!(run.contains(line), "{line}: {run}");
    }
    assert!(!run.contains("warning-only") && !run.contains("synchronization"), "replaced or never kept: {run}");
    lab.run(r1, "conf t\nrouter bgp 65001\nno bgp fast-external-fallover\nno bgp deterministic-med\nbgp client-to-client reflection\nno neighbor 10.0.12.2 allowas-in\nend");
    let run = lab.run(r1, "show running-config");
    assert!(!run.contains("fast-external-fallover") || run.contains("no bgp fast-external-fallover"));
    assert!(!run.contains("deterministic-med") && !run.contains("no bgp client-to-client") && !run.contains("allowas-in"), "{run}");
    // Everything survives a save and a reload.
    lab.run(r1, "copy running-config startup-config\nreload");
    lab.run(r1, "en");
    let again = lab.run(r1, "show running-config");
    assert!(again.contains(" neighbor 10.0.12.2 ttl-security hops 2\n") && again.contains(" bgp bestpath as-path multipath-relax\n"), "{again}");
}

#[test]
fn a_peer_group_hands_its_distribute_list_and_options_to_its_members() {
    let (mut lab, r1, _r2) = pair();
    lab.run(
        r1,
        "conf t\nrouter bgp 65001\nneighbor EXT peer-group\nneighbor EXT distribute-list 5 out\nneighbor EXT version 4\nneighbor 10.0.12.2 peer-group EXT\nend",
    );
    let run = lab.run(r1, "show running-config");
    assert!(run.contains(" neighbor EXT distribute-list 5 out\n") && run.contains(" neighbor EXT version 4\n"), "{run}");
    assert!(
        !run.contains(" neighbor 10.0.12.2 distribute-list") && !run.contains(" neighbor 10.0.12.2 version"),
        "a member writes only what differs: {run}"
    );
}

#[test]
fn a_prefix_limit_ends_the_session_unless_it_only_warns() {
    let (mut lab, r1, r2) = pair();
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    // The neighbour sends two prefixes: a limit of one is passed.
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 maximum-prefix 1\nend");
    advance(&mut lab, 4);
    let table = lab.run(r2, "show ip bgp");
    assert!(!table.contains("172.16.1.0/24") && !table.contains("172.16.2.0/24"), "the session ended: {table}");
    let sum = lab.run(r2, "show ip bgp summary");
    assert!(!sum.contains("Established"), "{sum}");
    let other = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(!other.contains("Established"), "the other end sees it end too: {other}");
    // A limit that only warns keeps the session.
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 maximum-prefix 1 warning-only\nend");
    advance(&mut lab, 4);
    assert!(lab.run(r2, "show ip bgp").contains("172.16.2.0/24"), "warning-only");
    // A limit that is not passed, then none at all.
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 maximum-prefix 2\nend");
    advance(&mut lab, 4);
    assert!(lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
    assert!(!lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 maximum-prefix 5").contains("does not change"));
}

#[test]
fn a_neighbours_advertised_and_accepted_routes_are_listed() {
    let (mut lab, r1, r2) = pair();
    let adv = lab.run(r1, "show ip bgp neighbors 10.0.12.2 advertised-routes");
    assert!(adv.contains("172.16.1.0/24") && adv.contains("172.16.2.0/24"), "{adv}");
    let got = lab.run(r2, "show ip bgp neighbors 10.0.12.1 routes");
    assert!(got.contains("172.16.1.0/24") && got.contains("65001"), "{got}");
    assert!(lab.try_run(r2, "show ip bgp neighbors 10.0.12.9 routes").starts_with("% No neighbour"));
    assert!(lab.try_run(r2, "show ip bgp neighbors 10.0.12.9 advertised-routes").starts_with("% No neighbour"));
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 shutdown\nend");
    advance(&mut lab, 2);
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2 advertised-routes").contains("not Established"));
}
