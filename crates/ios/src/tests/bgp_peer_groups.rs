//! BGP peer groups typed at the console: what a member takes from its group, what it keeps of its
//! own, how a change to the group reaches the members, the views and the running configuration.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001) with two neighbours in AS 65002, R2 and R3, that are not connected to each other.
/// R1 puts both in the peer group EXT. Each of the three owns one network: 172.16.1.0/24,
/// 172.16.2.0/24 and 172.16.3.0/24.
fn hub() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r1, "g0/2", r3, "g0/2");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint g0/2\nip add 10.0.13.1 255.255.255.252\nno shut\n\
         int lo1\nip add 172.16.1.1 255.255.255.0\nexit\nrouter bgp 65001\nbgp router-id 1.1.1.1\nneighbor EXT peer-group\n\
         neighbor EXT remote-as 65002\nneighbor EXT send-community\nneighbor 10.0.12.2 peer-group EXT\nneighbor 10.0.13.2 peer-group EXT\n\
         network 172.16.1.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint lo2\nip add 172.16.2.1 255.255.255.0\nexit\n\
         router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nnetwork 172.16.2.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.13.2 255.255.255.252\nno shut\nint lo3\nip add 172.16.3.1 255.255.255.0\nexit\n\
         router bgp 65002\nbgp router-id 3.3.3.3\nneighbor 10.0.13.1 remote-as 65001\nnetwork 172.16.3.0 mask 255.255.255.0\nend",
    );
    advance(&mut lab, 8);
    (lab, r1, r2, r3)
}

#[test]
fn members_take_the_remote_as_and_the_options_of_their_group() {
    let (mut lab, r1, ..) = hub();
    let table = lab.run(r1, "show ip bgp");
    assert!(table.contains("172.16.2.0/24") && table.contains("172.16.3.0/24"), "{table}");
    let summary = lab.run(r1, "show ip bgp summary");
    assert!(summary.contains("10.0.12.2") && summary.contains("10.0.13.2") && summary.contains("65002"), "{summary}");
    // The member lines say the group, the options come from it.
    let n = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(n.contains("Member of peer group EXT") && n.contains("Options: send-community") && n.contains("State: Established"), "{n}");
    // The group view lists its settings and its members with their state.
    let g = lab.run(r1, "show ip bgp peer-group");
    assert!(g.contains("BGP peer group EXT") && g.contains("remote-as 65002") && g.contains("send-community"), "{g}");
    assert!(g.contains("Members: 2") && g.contains("10.0.12.2 Established") && g.contains("10.0.13.2 Established"), "{g}");
    assert_eq!(lab.run(r1, "show ip bgp peer-group EXT"), g);
    assert!(lab.try_run(r1, "show ip bgp peer-group NOPE").contains("No peer group named NOPE"));
}

#[test]
fn a_change_to_the_group_reaches_the_members_and_a_member_can_keep_its_own() {
    let (mut lab, r1, ..) = hub();
    lab.run(r1, "conf t\nip prefix-list ONLY2 permit 172.16.2.0/24\nip prefix-list ALL permit 0.0.0.0/0 le 32\nrouter bgp 65001\nneighbor EXT prefix-list ONLY2 in\nend");
    let table = lab.run(r1, "show ip bgp");
    assert!(table.contains("172.16.2.0/24") && !table.contains("172.16.3.0/24"), "the group's list applies to both members: {table}");
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.13.2").contains("Policy: inbound prefix list ONLY2"));
    // One member sets its own list for the same direction.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.13.2 prefix-list ALL in\nend");
    let table = lab.run(r1, "show ip bgp");
    assert!(table.contains("172.16.2.0/24") && table.contains("172.16.3.0/24"), "{table}");
    let g = lab.run(r1, "show ip bgp peer-group EXT");
    assert!(g.contains("10.0.13.2 Established (its own: prefix-list ALL in)"), "{g}");
    // The group changes its list: the member with its own keeps it, the other follows.
    lab.run(r1, "conf t\nip prefix-list NONE permit 10.255.0.0/16\nrouter bgp 65001\nneighbor EXT prefix-list NONE in\nend");
    let table = lab.run(r1, "show ip bgp");
    assert!(!table.contains("172.16.2.0/24") && table.contains("172.16.3.0/24"), "{table}");
    // Taking the member's own list off gives it the group's again.
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.13.2 prefix-list ALL in\nend");
    assert!(!lab.run(r1, "show ip bgp").contains("172.16.3.0/24"));
    // Taking the group's list off clears it from the members that followed it.
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor EXT prefix-list NONE in\nend");
    let table = lab.run(r1, "show ip bgp");
    assert!(table.contains("172.16.2.0/24") && table.contains("172.16.3.0/24"), "{table}");
    // A name that is no neighbour and no group is refused.
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "neighbor NOPE next-hop-self").contains("No neighbour or peer group named NOPE"));
    assert!(lab.try_run(r1, "neighbor 10.9.9.9 next-hop-self").contains("Name the neighbour first"));
}

#[test]
fn the_group_carries_timers_a_route_map_and_the_other_options() {
    let (mut lab, r1, ..) = hub();
    lab.run(
        r1,
        "conf t\nroute-map PREF permit 10\nset local-preference 250\nexit\nrouter bgp 65001\nneighbor EXT timers 10 30\nneighbor EXT route-map PREF in\n\
         neighbor EXT next-hop-self\nneighbor EXT weight 9\nneighbor EXT soft-reconfiguration inbound\nneighbor EXT ebgp-multihop 2\nend",
    );
    let n = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(n.contains("Hold time 30 s, keepalive every 10 s"), "{n}");
    assert!(n.contains("timers 10 30") && n.contains("next-hop-self") && n.contains("weight 9") && n.contains("ebgp-multihop 2"), "{n}");
    assert!(n.contains("soft-reconfiguration inbound") && n.contains("Policy: inbound route map PREF"), "{n}");
    // A member's timers win over the group's.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.13.2 timers 5 15\nend");
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.13.2").contains("keepalive every 5 s"));
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("keepalive every 10 s"));
    // A group's route reflector client must be internal.
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "neighbor EXT route-reflector-client").contains("internal neighbour"));
}

#[test]
fn the_configuration_names_the_group_and_writes_only_what_a_member_adds() {
    let (mut lab, r1, ..) = hub();
    lab.run(r1, "conf t\nip prefix-list ALL permit 0.0.0.0/0 le 32\nrouter bgp 65001\nneighbor 10.0.13.2 prefix-list ALL out\nneighbor 10.0.13.2 description second\nend");
    let run = lab.run(r1, "show running-config");
    assert!(
        run.contains(
            " neighbor EXT peer-group\n neighbor EXT remote-as 65002\n neighbor EXT send-community\n neighbor 10.0.12.2 peer-group EXT\n \
             neighbor 10.0.13.2 peer-group EXT\n neighbor 10.0.13.2 description second\n neighbor 10.0.13.2 prefix-list ALL out\n"
        ),
        "{run}"
    );
    assert!(!run.contains("neighbor 10.0.12.2 remote-as") && !run.contains("neighbor 10.0.13.2 send-community"), "{run}");
    // It reads back the same after a reload.
    lab.run(r1, "copy run start");
    lab.run(r1, "reload");
    assert_eq!(lab.run(r1, "en\nshow running-config"), run);
    advance(&mut lab, 8);
    assert!(lab.run(r1, "show ip bgp").contains("172.16.3.0/24"));
}

#[test]
fn leaving_and_deleting_a_group_keep_what_the_members_set_themselves() {
    let (mut lab, r1, ..) = hub();
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.13.2 weight 40\nno neighbor 10.0.13.2 peer-group EXT\nend");
    let run = lab.run(r1, "show running-config");
    // The member left: it has no remote AS from the group, only its own weight.
    assert!(
        run.contains(" neighbor 10.0.13.2 weight 40\n")
            && !run.contains("neighbor 10.0.13.2 remote-as")
            && !run.contains("neighbor 10.0.13.2 peer-group"),
        "{run}"
    );
    assert!(lab.run(r1, "show ip bgp peer-group EXT").contains("Members: 1"));
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "no neighbor 10.0.13.2 peer-group EXT").contains("not in peer group EXT"));
    lab.run(r1, "end");
    // Deleting the group leaves its remaining member with nothing from it.
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor EXT peer-group\nend");
    let run = lab.run(r1, "show running-config");
    assert!(!run.contains("peer-group") && !run.contains("neighbor 10.0.12.2 remote-as"), "{run}");
    assert!(lab.run(r1, "show ip bgp peer-group").contains("no peer groups"));
    advance(&mut lab, 2);
    assert!(!lab.run(r1, "show ip bgp").contains("172.16.2.0/24"), "no remote AS, no session");
}

#[test]
fn group_input_is_checked() {
    let (mut lab, r1, ..) = hub();
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "neighbor 10.0.12.9 peer-group EXT").is_empty(), "a neighbour is added by joining");
    assert!(lab.try_run(r1, "neighbor 10.0.12.9 peer-group NOPE").contains("no peer group named NOPE"));
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 peer-group OTHER").contains("no peer group"));
    lab.run(r1, "neighbor OTHER peer-group");
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 peer-group OTHER").contains("is in peer group EXT already"));
    assert!(lab.try_run(r1, "neighbor 10.1.1.1 peer-group").contains("cannot be an address"));
    assert!(lab.try_run(r1, "no neighbor NOPE peer-group").contains("No peer group named NOPE"));
    assert!(lab.try_run(r1, "neighbor EXT remote-as 0").contains("autonomous system number"));
    for i in 0..20 {
        let out = lab.try_run(r1, &format!("neighbor G{i} peer-group"));
        assert!(out.is_empty() || out.contains("At most 16 peer groups"), "{out}");
    }
    assert!(lab.try_run(r1, "neighbor TOOMANY peer-group").contains("At most 16 peer groups"));
}
