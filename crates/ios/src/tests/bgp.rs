//! BGP typed at the console: sessions, routes, best paths, the show commands and the running
//! configuration.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 (AS 65001) - R2 (AS 65002, next-hop-self towards R3) - R3 (AS 65002), a PC behind R1 and R3.
fn chain() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    let pc1 = lab.add(DeviceKind::Pc);
    let pc3 = lab.add(DeviceKind::Pc);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.cable(pc1, "fa0", r1, "g0/0");
    lab.cable(pc3, "fa0", r3, "g0/0");
    lab.run(
        r1,
        "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nexit\n\
         router bgp 65001\nbgp router-id 1.1.1.1\nbgp log-neighbor-changes\nneighbor 10.0.12.2 remote-as 65002\n\
         network 192.168.1.0 mask 255.255.255.0\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/2\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\n\
         router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\nneighbor 10.0.23.2 remote-as 65002\n\
         neighbor 10.0.23.2 next-hop-self\nend",
    );
    lab.run(
        r3,
        "en\nconf t\nint g0/2\nip add 10.0.23.2 255.255.255.252\nno shut\nint g0/0\nip add 192.168.3.1 255.255.255.0\nno shut\nexit\n\
         router bgp 65002\nneighbor 10.0.23.1 remote-as 65002\nnetwork 192.168.3.0 mask 255.255.255.0\nend",
    );
    lab.run(pc1, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(pc3, "ipconfig 192.168.3.10 255.255.255.0 192.168.3.1");
    (lab, r1, r2, r3, pc1, pc3)
}

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

#[test]
fn sessions_climb_the_states_and_routes_arrive() {
    let (mut lab, r1, r2, r3, pc1, _pc3) = chain();
    // Nothing is up the moment it is configured.
    let first = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(first.contains("State: Connect"), "{first}");
    assert!(!lab.run(r1, "show ip route").contains("192.168.3.0/24"));
    advance(&mut lab, 1);
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("State: OpenSent"));
    advance(&mut lab, 1);
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("State: OpenConfirm"));
    advance(&mut lab, 3);
    let n = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(n.contains("State: Established, up for 00:00:0"), "{n}");
    assert!(n.contains("remote AS 65002 (external link)") && n.contains("Neighbour router id: 2.2.2.2"), "{n}");
    assert!(n.contains("Local address: 10.0.12.1"), "{n}");
    // OPEN, KEEPALIVE and one UPDATE each way crossed the cable.
    assert!(n.contains("Messages sent 3 (opens 1, updates 1, notifications 0, keepalives 1)"), "{n}");
    assert!(n.contains("Messages received 3 (opens 1, updates 1, notifications 0, keepalives 1)"), "{n}");
    assert!(n.contains("Prefixes received 1, sent 1"), "{n}");
    let ibgp = lab.run(r3, "show ip bgp neighbors 10.0.23.1");
    assert!(ibgp.contains("State: Established") && ibgp.contains("(internal link)"), "{ibgp}");
    assert!(lab.run(r2, "show ip bgp neighbors").contains("Options: next-hop-self"));
    let summary = lab.run(r1, "show ip bgp summary");
    assert!(summary.contains("BGP router id 1.1.1.1, local AS 65001") && summary.contains("10.0.12.2") && summary.contains("65002"), "{summary}");
    // The routing table: external distance 20 at R1, internal 200 at R3.
    let routes = lab.run(r1, "show ip route bgp");
    assert!(routes.contains("B    192.168.3.0/24") && routes.contains("via 10.0.12.2, GigabitEthernet0/1 (BGP) [20/0]"), "{routes}");
    let routes3 = lab.run(r3, "show ip route bgp");
    assert!(routes3.contains("B    192.168.1.0/24") && routes3.contains("via 10.0.23.1, GigabitEthernet0/2 (BGP) [200/0]"), "{routes3}");
    assert!(lab.run(r1, "show ip route").contains("Codes: C connected, L local, S static, R RIP, D EIGRP, B BGP"));
    // The table: the local network, and what was learned with its AS path.
    let table = lab.run(r1, "show ip bgp");
    assert!(table.contains("192.168.1.0/24") && table.contains("32768"), "{table}");
    assert!(table.contains("192.168.3.0/24") && table.contains("65002 i"), "{table}");
    let one = lab.run(r1, "show ip bgp 192.168.3.0");
    assert!(one.contains("BGP entry for 192.168.3.0/24: 1 path, the best is path 1"), "{one}");
    assert!(one.contains("AS path 65002, next hop 10.0.12.2, learned from 10.0.12.2, router id 2.2.2.2"), "{one}");
    assert!(lab.try_run(r1, "show ip bgp 10.99.0.0").contains("not in the BGP table"));
    assert!(lab.run(r1, "show ip protocols").contains("Routing protocol: BGP, autonomous system 65001"));
    assert!(lab.run(r1, "show logging").contains("%BGP-5-ADJCHANGE: neighbor 10.0.12.2 Up"));
    // Traffic crosses both autonomous systems, both ways.
    let ping = lab.run(pc1, "ping 192.168.3.10");
    assert!(ping.contains("reply from 192.168.3.10"), "{ping}");
}

#[test]
fn clearing_and_shutting_a_session_takes_its_routes_away_and_it_comes_back() {
    let (mut lab, r1, _r2, r3, _pc1, _pc3) = chain();
    advance(&mut lab, 6);
    assert!(lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
    assert_eq!(lab.run(r1, "clear ip bgp *"), "1 session reset; they start again.");
    assert!(!lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
    assert!(!lab.run(r3, "show ip route bgp").contains("192.168.1.0/24"));
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("State: Connect"));
    advance(&mut lab, 5);
    assert!(lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
    assert!(lab.run(r3, "show ip route bgp").contains("192.168.1.0/24"));
    let log = lab.run(r1, "show logging");
    assert!(log.contains("neighbor 10.0.12.2 Down User reset"), "{log}");
    // A neighbour that is shut down stays configured and idle.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 shutdown\nend");
    assert!(lab.run(r1, "show ip bgp summary").contains("Idle (Admin)"));
    assert!(!lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 shutdown\nend");
    advance(&mut lab, 5);
    assert!(lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
    // A link that goes down takes the session with it at once.
    lab.run(r1, "conf t\nint g0/1\nshutdown\nend");
    assert!(!lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
    assert!(lab.run(r1, "show ip bgp summary").contains("Idle"));
    assert!(lab.try_run(r1, "clear ip bgp 9.9.9.9").contains("No neighbour"));
}

#[test]
fn a_wrong_remote_as_is_refused_and_says_why() {
    let (mut lab, r1, r2, _r3, _pc1, _pc3) = chain();
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 remote-as 65099\nend");
    advance(&mut lab, 6);
    let n = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(n.contains("State: Idle") && n.contains("Last error: sent NOTIFICATION: OPEN message error: bad peer AS"), "{n}");
    let other = lab.run(r2, "show ip bgp neighbors 10.0.12.1");
    assert!(other.contains("Last error: received NOTIFICATION: OPEN message error: bad peer AS"), "{other}");
    assert!(!lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 remote-as 65002\nend");
    advance(&mut lab, 5);
    assert!(lab.run(r1, "show ip route bgp").contains("192.168.3.0/24"));
}

#[test]
fn a_network_is_advertised_only_while_its_exact_prefix_is_in_the_routing_table() {
    let (mut lab, r1, r2, _r3, _pc1, _pc3) = chain();
    lab.run(r1, "conf t\nrouter bgp 65001\nnetwork 172.16.0.0 mask 255.255.0.0\nend");
    advance(&mut lab, 6);
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.0.0/16"));
    lab.run(r1, "conf t\ninterface loopback 5\nip address 172.16.0.1 255.255.0.0\nend");
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("172.16.0.0/16") && table.contains("65001 i"), "{table}");
    // A smaller prefix in the table is not the one the statement names.
    lab.run(r1, "conf t\ninterface loopback 5\nip address 172.16.0.1 255.255.255.0\nend");
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.0.0/16"));
}

#[test]
fn redistribute_connected_advertises_with_an_incomplete_origin() {
    let (mut lab, r1, r2, _r3, _pc1, _pc3) = chain();
    lab.run(r1, "conf t\ninterface loopback 7\nip address 10.77.0.1 255.255.255.0\nrouter bgp 65001\nredistribute connected\nend");
    advance(&mut lab, 6);
    let table = lab.run(r2, "show ip bgp");
    assert!(table.contains("10.77.0.0/24") && table.contains("65001 ?"), "{table}");
    assert!(lab.run(r1, "show running-config").contains(" redistribute connected\n"));
}

#[test]
fn best_path_prefers_the_lowest_router_id_then_weight() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    let r4 = lab.add(DeviceKind::Router);
    let pc4 = lab.add(DeviceKind::Pc);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.cable(r1, "g0/1", r3, "g0/0");
    lab.cable(r2, "g0/1", r4, "g0/0");
    lab.cable(r3, "g0/1", r4, "g0/1");
    lab.cable(pc4, "fa0", r4, "g0/2");
    let config = |lab: &mut Lab, r: DeviceId, rid: &str, asn: u32, ifs: &[(&str, &str)], neighbors: &[(&str, u32)], network: &str| {
        let mut text = String::from("en\nconf t\n");
        for (name, ip) in ifs {
            text.push_str(&format!("int {name}\nip add {ip} 255.255.255.252\nno shut\n"));
        }
        text.push_str(&format!("exit\nrouter bgp {asn}\nbgp router-id {rid}\n"));
        for (addr, remote) in neighbors {
            text.push_str(&format!("neighbor {addr} remote-as {remote}\n"));
        }
        text.push_str(network);
        text.push_str("end");
        lab.run(r, &text);
    };
    config(&mut lab, r1, "1.1.1.1", 65001, &[("g0/0", "10.0.12.1"), ("g0/1", "10.0.13.1")], &[("10.0.12.2", 65002), ("10.0.13.2", 65003)], "");
    config(&mut lab, r2, "2.2.2.2", 65002, &[("g0/0", "10.0.12.2"), ("g0/1", "10.0.24.1")], &[("10.0.12.1", 65001), ("10.0.24.2", 65004)], "");
    config(&mut lab, r3, "3.3.3.3", 65003, &[("g0/0", "10.0.13.2"), ("g0/1", "10.0.34.1")], &[("10.0.13.1", 65001), ("10.0.34.2", 65004)], "");
    config(
        &mut lab,
        r4,
        "4.4.4.4",
        65004,
        &[("g0/0", "10.0.24.2"), ("g0/1", "10.0.34.2")],
        &[("10.0.24.1", 65002), ("10.0.34.1", 65003)],
        "network 192.168.4.0 mask 255.255.255.0\n",
    );
    lab.run(r4, "conf t\nint g0/2\nip add 192.168.4.1 255.255.255.0\nno shut\nend");
    advance(&mut lab, 8);
    // Two paths of the same length: the lower router id of the neighbour that sent it wins.
    let entry = lab.run(r1, "show ip bgp 192.168.4.0");
    assert!(entry.contains("BGP entry for 192.168.4.0/24: 2 paths, the best is path 1"), "{entry}");
    assert!(entry.contains("Path 1 (best): AS path 65002 65004, next hop 10.0.12.2"), "{entry}");
    assert!(entry.contains("Path 2: AS path 65003 65004, next hop 10.0.13.2"), "{entry}");
    assert!(lab.run(r1, "show ip route bgp").contains("via 10.0.12.2"));
    // The origin never takes its own prefix back.
    assert!(lab.run(r4, "show ip bgp 192.168.4.0").contains(": 1 path,"));
    // A weight on the neighbour overrides everything after it.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.13.2 weight 50\nend");
    let entry = lab.run(r1, "show ip bgp 192.168.4.0");
    assert!(entry.contains("Path 1 (best): AS path 65003 65004, next hop 10.0.13.2"), "{entry}");
    assert!(entry.contains("weight 50"), "{entry}");
    assert!(lab.run(r1, "show ip route bgp").contains("via 10.0.13.2"));
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.13.2 weight 50\n"));
}

#[test]
fn the_configuration_round_trips_and_no_forms_remove_it() {
    let (mut lab, r1, ..) = chain();
    lab.run(
        r1,
        "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 description link to R2\nneighbor 10.0.12.2 ebgp-multihop 3\nneighbor 10.0.12.2 weight 7\n\
         bgp default local-preference 150\nredistribute ospf 1\nend",
    );
    let run = lab.run(r1, "show running-config");
    assert!(
        run.contains(
            "router bgp 65001\n bgp router-id 1.1.1.1\n bgp log-neighbor-changes\n bgp default local-preference 150\n neighbor 10.0.12.2 remote-as 65002\n \
             neighbor 10.0.12.2 description link to R2\n neighbor 10.0.12.2 ebgp-multihop 3\n neighbor 10.0.12.2 weight 7\n \
             network 192.168.1.0 mask 255.255.255.0\n redistribute ospf 1\n!"
        ),
        "{run}"
    );
    lab.run(r1, "copy run start");
    lab.run(r1, "reload");
    let after = lab.run(r1, "en\nshow running-config");
    assert_eq!(after, run);
    lab.run(
        r1,
        "conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 weight\nno neighbor 10.0.12.2 description\nno neighbor 10.0.12.2 ebgp-multihop\n\
         no bgp default local-preference\nno redistribute ospf\nno network 192.168.1.0 mask 255.255.255.0\nno bgp log-neighbor-changes\nend",
    );
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("router bgp 65001\n bgp router-id 1.1.1.1\n neighbor 10.0.12.2 remote-as 65002\n!"), "{run}");
    lab.run(r1, "conf t\nno router bgp 65001\nend");
    assert!(!lab.run(r1, "show running-config").contains("router bgp"));
    assert!(lab.run(r1, "show ip bgp summary").contains("BGP is not running"));
}

#[test]
fn bad_input_is_refused_with_a_reason() {
    let (mut lab, r1, ..) = chain();
    lab.run(r1, "conf t\nrouter bgp 65001");
    assert!(lab.try_run(r1, "neighbor 10.0.12.9 shutdown").contains("Name the neighbour first"));
    assert!(lab.try_run(r1, "neighbor 0.0.0.0 remote-as 65002").contains("not an address a neighbour can have"));
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 remote-as 0").contains("autonomous system number"));
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 remote-as 99999999999").contains("autonomous system number"));
    assert!(lab.try_run(r1, "network 10.0.0.0 mask 255.0.255.0").contains("contiguous"));
    assert!(lab.try_run(r1, "no network 10.0.0.0 mask 255.0.0.0").contains("No such network"));
    assert!(lab.try_run(r1, "redistribute ospf").contains("process number"));
    assert!(lab.try_run(r1, "redistribute rip").contains("connected, static or ospf"));
    assert!(lab.try_run(r1, "neighbor 10.0.12.2 update-source nonsense").contains("not an interface name"));
    lab.run(r1, "exit");
    assert!(lab.try_run(r1, "router bgp 65009").contains("already runs here in AS 65001"));
    assert!(lab.try_run(r1, "no router bgp 65009").contains("No BGP process"));
    // A four-octet AS number is a normal one.
    lab.run(r1, "no router bgp 65001");
    lab.run(r1, "router bgp 4200000001\nneighbor 10.0.12.2 remote-as 4200000002");
    assert!(lab.run(r1, "end\nshow running-config").contains("router bgp 4200000001\n neighbor 10.0.12.2 remote-as 4200000002\n"));
}

#[test]
fn debug_ip_bgp_reports_messages_and_session_changes() {
    let (mut lab, r1, ..) = chain();
    lab.run(r1, "debug ip bgp");
    assert!(lab.run(r1, "show debugging").contains("BGP"));
    advance(&mut lab, 6);
    let log = lab.run(r1, "show logging");
    assert!(log.contains("BGP debug: OPEN (AS 65002, hold time 180 s, id 2.2.2.2) from 10.0.12.2"), "{log}");
    assert!(log.contains("BGP debug: UPDATE (1 announced, 0 withdrawn) from 10.0.12.2"), "{log}");
    assert!(log.contains("BGP debug: neighbor 10.0.12.2 OpenConfirm -> Established"), "{log}");
    lab.run(r1, "no debug ip bgp");
    assert!(!lab.run(r1, "show debugging").contains("BGP"));
}

#[test]
fn keepalives_go_out_every_minute() {
    let (mut lab, r1, ..) = chain();
    advance(&mut lab, 6);
    let before = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    // The window starts with the session up, so the keepalive due at 60 s is scheduled.
    advance(&mut lab, 60);
    let after = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(before.contains("keepalives 1)") && after.contains("keepalives 2)"), "{before}\n{after}");
}
