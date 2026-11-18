//! Each `debug` topic prints its own messages when its protocol does something (see
//! `netlabcraft_sim::debug`), and nothing once it is turned off.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// Two routers cabled on g0/0 (10.0.0.1 and 10.0.0.2) and a PC on R1's g0/1 (192.168.1.10).
fn routers() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.cable(pc, "fa0", r1, "g0/1");
    lab.run(pc, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nint g0/1\nip address 192.168.1.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 10.0.0.2 255.255.255.0\nno shut\nend");
    (lab, r1, r2, pc)
}

fn secs(lab: &mut Lab, n: u64) {
    lab.sim.run_for(&lab.t, n * 1_000_000);
}

#[test]
fn debug_arp_reports_questions_and_answers() {
    let (mut lab, r1, _r2, pc) = routers();
    lab.run(r1, "debug arp");
    lab.run(pc, "ping 192.168.1.1");
    lab.run(r1, "ping 10.0.0.2");
    let log = lab.run(r1, "show logging");
    assert!(log.contains("ARP debug: who has 192.168.1.1? 192.168.1.10"), "{log}");
    assert!(log.contains("ARP debug: asking who has 10.0.0.2 on GigabitEthernet0/0"), "{log}");
    assert!(log.contains("ARP debug: 10.0.0.2 is at "), "{log}");
    lab.run(r1, "no debug arp\nclear logging");
    lab.run(pc, "ping 192.168.1.1");
    assert!(!lab.run(r1, "show logging").contains("ARP debug"));
}

#[test]
fn debug_ip_nat_detailed_shows_the_ports() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(a, "fa0", r, "g0/0");
    lab.cable(srv, "g0", r, "g0/1");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 203.0.113.10 255.255.255.0 203.0.113.1");
    lab.run(
        r,
        "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nip nat inside\nno shut\nint g0/1\nip address 203.0.113.1 255.255.255.0\nip nat outside\nno shut\nexit\n\
         access-list 1 permit 192.168.1.0 0.0.0.255\nip nat inside source list 1 interface g0/1 overload\nend",
    );
    lab.run(r, "debug ip nat detailed");
    lab.run(a, "ping 203.0.113.10");
    let log = lab.run(r, "show logging");
    assert!(log.contains("NAT detail debug: icmp inside local 192.168.1.10:"), "{log}");
    assert!(log.contains("becomes inside global 203.0.113.1:") && log.contains("(port translation, overload)"), "{log}");
}

/// A ring of three switches on trunks; S2 is the root of VLAN 1.
fn ring() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(s1, "g0/2", s3, "g0/1");
    lab.cable(s2, "g0/2", s3, "g0/2");
    for s in [s1, s2, s3] {
        lab.run(s, "en\nconf t\ninterface range g0/1 - 2\nswitchport mode trunk\nend");
    }
    lab.run(s2, "conf t\nspanning-tree vlan 1 root primary\nend");
    secs(&mut lab, 60);
    (lab, s1, s2, s3)
}

#[test]
fn debug_spanning_tree_bpdu_describes_each_bpdu() {
    let (mut lab, s1, _s2, _s3) = ring();
    lab.run(s1, "debug spanning-tree bpdu");
    secs(&mut lab, 10);
    let log = lab.run(s1, "show logging");
    assert!(log.contains("STP BPDU debug: ") && log.contains("BPDU on Gi0/1: root 24577."), "{log}");
}

#[test]
fn debug_spanning_tree_root_and_events_follow_changes() {
    let (mut lab, s1, _s2, s3) = ring();
    lab.run(s1, "debug spanning-tree root\ndebug spanning-tree events");
    secs(&mut lab, 1);
    let log = lab.run(s1, "show logging");
    assert!(log.contains("STP root debug: VLAN 1 root: 24577."), "{log}");
    assert!(log.contains("STP event debug: VLAN 1 port Gi0/1: root port (FWD)"), "{log}");
    // S3 takes over as the root.
    lab.run(s3, "conf t\nspanning-tree vlan 1 priority 4096\nend");
    secs(&mut lab, 60);
    secs(&mut lab, 1);
    let log = lab.run(s1, "show logging");
    assert!(log.contains("VLAN 1 root changed: 24577.") && log.contains(" -> 4097."), "{log}");
    assert!(
        log.contains("VLAN 1 port Gi0/2 changed:") && log.contains("-> root port (FWD)"),
        "S1's root port moves to the link to the new root:\n{log}"
    );
}

#[test]
fn debug_cdp_events_and_packets_follow_the_neighbours() {
    let (mut lab, r1, r2, _pc) = routers();
    lab.run(r1, "debug cdp events\ndebug cdp packets");
    secs(&mut lab, 1);
    let log = lab.run(r1, "show logging");
    assert!(log.contains("CDP event debug: neighbour ") && log.contains(": their port "), "{log}");
    assert!(log.contains("CDP packet debug: advertisement received from neighbour "), "{log}");
    lab.run(r2, "conf t\nint g0/0\nshutdown\nend");
    secs(&mut lab, 1);
    assert!(lab.run(r1, "show logging").contains(" is gone (it was their port "));
}

#[test]
fn debug_lldp_reports_advertisements() {
    let (mut lab, r1, r2, _pc) = routers();
    lab.run(r1, "conf t\nlldp run\nend\ndebug lldp");
    lab.run(r2, "conf t\nlldp run\nend");
    secs(&mut lab, 70);
    let log = lab.run(r1, "show logging");
    assert!(log.contains("LLDP debug: advertisement on Gi0/0 from "), "{log}");
}

#[test]
fn debug_standby_reports_hellos_and_state_changes() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    lab.cable(r1, "g0/0", s1, "fa0/1");
    lab.cable(r2, "g0/0", s1, "fa0/2");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nstandby 1 ip 192.168.1.254\nstandby 1 priority 110\nno shut\nend");
    lab.run(r2, "en\ndebug standby\nconf t\nint g0/0\nip address 192.168.1.2 255.255.255.0\nstandby 1 ip 192.168.1.254\nno shut\nend");
    secs(&mut lab, 8);
    secs(&mut lab, 1);
    let log = lab.run(r2, "show logging");
    assert!(log.contains("HSRP debug: hello from 192.168.1.1 on GigabitEthernet0/0: the active router of group 1"), "{log}");
    assert!(log.contains("HSRP debug: group 1 on GigabitEthernet0/0"), "{log}");
    lab.run(r1, "conf t\nint g0/0\nshutdown\nend");
    secs(&mut lab, 8);
    secs(&mut lab, 1);
    let log = lab.run(r2, "show logging");
    assert!(log.contains("group 1 on GigabitEthernet0/0 changed:") && log.contains("-> active, priority 100"), "{log}");
}

#[test]
fn debug_vrrp_reports_advertisements_and_state_changes() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    lab.cable(r1, "g0/0", s1, "fa0/1");
    lab.cable(r2, "g0/0", s1, "fa0/2");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nvrrp 1 ip 192.168.1.254\nvrrp 1 priority 110\nno shut\nend");
    lab.run(r2, "en\ndebug vrrp\nconf t\nint g0/0\nip address 192.168.1.2 255.255.255.0\nvrrp 1 ip 192.168.1.254\nno shut\nend");
    secs(&mut lab, 8);
    secs(&mut lab, 1);
    let log = lab.run(r2, "show logging");
    assert!(log.contains("VRRP debug: advertisement from 192.168.1.1 on GigabitEthernet0/0: group 1, priority 110"), "{log}");
    assert!(log.contains("VRRP debug: group 1 on GigabitEthernet0/0"), "{log}");
}

#[test]
fn debug_ip_rip_events_and_ip_routing_report_route_changes() {
    let (mut lab, r1, r2, _pc) = routers();
    lab.run(r2, "debug ip rip events\ndebug ip routing");
    secs(&mut lab, 1);
    lab.run(r1, "conf t\nrouter rip\nversion 2\nnetwork 10.0.0.0\nnetwork 192.168.1.0\nno auto-summary\nend");
    lab.run(r2, "conf t\nrouter rip\nversion 2\nnetwork 10.0.0.0\nno auto-summary\nend");
    secs(&mut lab, 60);
    secs(&mut lab, 1);
    let log = lab.run(r2, "show logging");
    assert!(log.contains("RIP event debug: route 192.168.1.0/24 via 10.0.0.1 appeared: 1 hop out GigabitEthernet0/0"), "{log}");
    assert!(log.contains("Routing debug: R route 192.168.1.0/24 via 10.0.0.1 appeared: distance 120, metric 1"), "{log}");
}

#[test]
fn debug_eigrp_neighbors_and_fsm_report_what_changes() {
    let (mut lab, r1, r2, _pc) = routers();
    lab.run(r2, "debug eigrp neighbors\ndebug eigrp fsm");
    secs(&mut lab, 1);
    lab.run(r1, "conf t\nrouter eigrp 1\nnetwork 10.0.0.0\nnetwork 192.168.1.0\nend");
    lab.run(r2, "conf t\nrouter eigrp 1\nnetwork 10.0.0.0\nend");
    secs(&mut lab, 60);
    secs(&mut lab, 1);
    let log = lab.run(r2, "show logging");
    assert!(log.contains("EIGRP neighbour debug: neighbour 10.0.0.1 on GigabitEthernet0/0 appeared: up, hold time"), "{log}");
    assert!(log.contains("EIGRP route debug: route 192.168.1.0/24 via 10.0.0.1 appeared: out GigabitEthernet0/0"), "{log}");
}

#[test]
fn debug_ntp_reports_the_exchange_and_the_clock() {
    let (mut lab, r1, r2, _pc) = routers();
    lab.run(r1, "clock set 09:00:00 15 Mar 2030\nconf t\nntp master 3\nend\ndebug ntp events");
    lab.run(r2, "ping 10.0.0.1\ndebug ntp events\nconf t");
    lab.run(r2, "ntp server 10.0.0.1");
    lab.run(r2, "end");
    secs(&mut lab, 1);
    let client = lab.run(r2, "show logging");
    assert!(client.contains("NTP debug: answer from 10.0.0.1 (stratum 3)"), "{client}");
    assert!(
        client.contains("NTP debug: clock: synchronised with 10.0.0.1") || client.contains("NTP debug: clock appeared: synchronised with 10.0.0.1"),
        "{client}"
    );
    let server = lab.run(r1, "show logging");
    assert!(server.contains("NTP debug: time request from 10.0.0.2"), "{server}");
}

#[test]
fn debug_ip_ssh_reports_the_session_and_the_login() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(r, "en\ndebug ip ssh\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nexit");
    lab.run(
        r,
        "ip domain-name lab.test\nusername admin secret s3cret\nline vty 0 4\nlogin local\ntransport input ssh\nexit\ncrypto key generate rsa",
    );
    lab.run(r, "1024");
    lab.run(r, "ip ssh version 2\nend");
    assert!(lab.try_run(a, "ssh -l admin 192.168.1.1").contains("Open"));
    lab.try_run(a, "nope");
    lab.try_run(a, "admin");
    lab.try_run(a, "s3cret");
    let log = lab.run(r, "show logging");
    assert!(log.contains("SSH debug: a session is opened from "), "{log}");
    assert!(log.contains("SSH debug: login of admin is refused: wrong password or unknown user"), "{log}");
    assert!(log.contains("SSH debug: user admin is logged in"), "{log}");
}
