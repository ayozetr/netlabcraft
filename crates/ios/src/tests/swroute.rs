//! Typed console tests for the switching and routing leftovers: VRRP timers, tracking and
//! VRRPv3 for IPv6, EIGRP updates and summaries, RIP timers and RIPng, VTP pruning.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// PC1 on S1 with R1 (192.168.1.1) and R2 (192.168.1.2); a server behind both on S2.
fn pair() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let pc = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(pc, "fa0", s1, "fa0/1");
    lab.cable(r1, "g0/0", s1, "fa0/2");
    lab.cable(r2, "g0/0", s1, "fa0/3");
    lab.cable(r1, "g0/1", s2, "fa0/2");
    lab.cable(r2, "g0/1", s2, "fa0/3");
    lab.cable(srv, "g0", s2, "fa0/1");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 192.168.1.2 255.255.255.0\nno shut\nint g0/1\nip address 10.0.0.2 255.255.255.0\nno shut\nend");
    lab.run(s1, "en");
    (lab, r1, r2, s1, pc)
}

fn secs(lab: &mut Lab, s: f64) {
    lab.sim.run_for(&lab.t, (s * 1e6) as u64);
}

#[test]
fn vrrp_backups_wait_the_master_down_interval() {
    let (mut lab, r1, r2, s1, _pc) = pair();
    lab.run(r1, "conf t\nint g0/0\nvrrp 1 ip 192.168.1.254\nvrrp 1 priority 120\nvrrp 1 timers advertise 2\nend");
    lab.run(r2, "conf t\nint g0/0\nvrrp 1 ip 192.168.1.254\nvrrp 1 timers advertise 2\nend");
    let v = lab.run(r1, "show vrrp");
    // Version 2: 3 × 2 s plus a skew of (256 − 120) / 256 s.
    assert!(v.contains("advertisements every 2 s, master down interval 6.531 s"), "{v}");
    let v = lab.run(r2, "show vrrp");
    assert!(v.contains(": backup") && v.contains("master down interval 6.609 s"), "{v}");
    // Advertisements every two seconds on the wire.
    lab.sim.clear_trace();
    secs(&mut lab, 6.0);
    let ads = lab
        .sim
        .trace()
        .iter()
        .filter(|e| e.from.device == r1 && e.summary.starts_with("VRRPv2 advertisement, group 1, priority 120, every 2 s"))
        .count();
    assert!((2..=4).contains(&ads), "{ads}");
    // The master falls silent (its switch port shut): nobody is master until R2's interval
    // has passed.
    lab.run(s1, "conf t\nint fa0/2\nshutdown\nend");
    let v = lab.run(r2, "show vrrp");
    assert!(v.contains(": backup") && v.contains("Master: none") && v.contains("Waiting for a new master: 6.609 s left"), "{v}");
    secs(&mut lab, 3.0);
    assert!(lab.run(r2, "show vrrp").contains(": backup"));
    secs(&mut lab, 4.0);
    assert!(lab.run(r2, "show vrrp").contains(": master"));
    assert!(lab.run(r2, "show logging").contains("VRRP: group 1 on GigabitEthernet0/0: backup -> master"));
    // R1 comes back and preempts, but only after its own master down interval; R2 keeps
    // forwarding meanwhile.
    lab.run(s1, "conf t\nint fa0/2\nno shutdown\nend");
    let v = lab.run(r1, "show vrrp");
    assert!(v.contains(": backup") && v.contains("Master: 192.168.1.2") && v.contains("Waiting"), "{v}");
    secs(&mut lab, 7.0);
    assert!(lab.run(r1, "show vrrp").contains(": master"));
    assert!(lab.run(r2, "show logging").contains("master -> backup"));
    // A master that says goodbye (interface shut down): the skew time only.
    lab.run(r1, "conf t\nint g0/0\nshutdown\nend");
    assert!(lab.run(r2, "show vrrp").contains("Waiting for a new master: 0.609 s left"));
    secs(&mut lab, 1.0);
    assert!(lab.run(r2, "show vrrp").contains(": master"));
    lab.run(r1, "conf t\nint g0/0\nno shutdown\nend");
    secs(&mut lab, 7.0);
    assert!(lab.run(r1, "show vrrp").contains(": master"));
}

#[test]
fn vrrp_tracking_and_version_3_timers() {
    let (mut lab, r1, r2, _s1, _pc) = pair();
    lab.run(r1, "conf t\nfhrp version vrrp v3\nint g0/0\nvrrp 1 ip 192.168.1.254\nvrrp 1 priority 120\nvrrp 1 track g0/1 decrement 30\nvrrp 1 timers advertise msec 500\nend");
    lab.run(r2, "conf t\nfhrp version vrrp v3\nint g0/0\nvrrp 1 ip 192.168.1.254\nvrrp 1 timers advertise msec 500\nend");
    let v = lab.run(r1, "show vrrp");
    // Version 3: 3 × 0.5 s plus (256 − 120) × 0.5 / 256 s.
    assert!(v.contains(": master") && v.contains("advertisements every 0.500 s, master down interval 1.766 s"), "{v}");
    // The tracked uplink goes down: 120 − 30 = 90 < 100, R2 preempts after its interval.
    lab.run(r1, "conf t\nint g0/1\nshutdown\nend");
    let v = lab.run(r1, "show vrrp");
    assert!(v.contains("priority 90") && v.contains("Tracked interfaces down: GigabitEthernet0/1 (priority 120 lowered to 90)"), "{v}");
    assert!(v.contains(": master"), "R1 keeps the role until R2's interval passes: {v}");
    secs(&mut lab, 2.0);
    assert!(lab.run(r2, "show vrrp").contains(": master"));
    assert!(lab.run(r1, "show vrrp").contains(": backup"));
    lab.sim.clear_trace();
    secs(&mut lab, 2.0);
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("VRRPv3 advertisement, group 1, priority 100, every 50 cs")));
    // Back up: R1 preempts again.
    lab.run(r1, "conf t\nint g0/1\nno shutdown\nend");
    secs(&mut lab, 2.0);
    assert!(lab.run(r1, "show vrrp").contains(": master (priority 120"));
    // The configuration reads back and survives a reload.
    let run = lab.run(r1, "show running-config interface g0/0");
    assert!(run.contains(" vrrp 1 timers advertise msec 500\n vrrp 1 track GigabitEthernet0/1 decrement 30"), "{run}");
    let all = lab.run(r1, "show running-config");
    lab.run(r1, "copy run start\nreload\nen");
    assert_eq!(lab.run(r1, "show running-config"), all);
    lab.run(r1, "conf t\nint g0/0\nno vrrp 1 track g0/1\nno vrrp 1 timers advertise\nvrrp 1 timers advertise 3\nend");
    let run = lab.run(r1, "show running-config interface g0/0");
    assert!(!run.contains("track") && run.contains(" vrrp 1 timers advertise 3\n"), "{run}");
    // Mistakes.
    lab.run(r2, "conf t\nno fhrp version vrrp v3\nint g0/0");
    assert!(lab.try_run(r2, "vrrp 1 timers advertise msec 300").contains("version 3"));
    assert!(lab.try_run(r2, "vrrp 1 timers advertise 0").contains("1-40"));
    assert!(lab.try_run(r2, "vrrp 1 track x9 decrement 5").contains("not an interface"));
    assert!(lab.try_run(r2, "vrrp 1 track g0/1 decrement 255").contains("1-254"));
    assert!(lab.try_run(r2, "vrrp 1 ipv6 fe80::1").contains("version 3"));
}

#[test]
fn vrrp_version_3_shares_an_ipv6_link_local_gateway() {
    let (mut lab, r1, r2, _s1, pc) = pair();
    let srv = lab.t.devices.iter().find(|d| d.kind == DeviceKind::Server).unwrap().id;
    for (r, n, prio) in [(r1, 2, 120), (r2, 3, 100)] {
        lab.run(
            r,
            &format!(
                "conf t\nipv6 unicast-routing\nfhrp version vrrp v3\nint g0/0\nipv6 address 2001:db8:1::{n}/64\nvrrp 1 ipv6 FE80::1\nvrrp 1 priority {prio}\n\
                 vrrp 1 track g0/1 decrement 30\nint g0/1\nipv6 address 2001:db8:2::{n}/64\nvrrp 2 ipv6 fe80::2\nvrrp 2 priority {prio}\nvrrp 2 track g0/0 decrement 30\nend"
            ),
        );
    }
    lab.run(pc, "ipv6config 2001:db8:1::10/64 fe80::1");
    lab.run(srv, "ipv6config 2001:db8:2::10/64 fe80::2");
    let v = lab.run(r1, "show vrrp");
    assert!(v.contains("IPv6 group 1 on GigabitEthernet0/0: master (priority 120") && v.contains("virtual MAC 0000.5e00.0201"), "{v}");
    assert!(lab.run(r2, "show vrrp brief").contains("backup  fe80::"));
    lab.run(pc, "ping 2001:db8:2::10");
    let p = lab.run(pc, "ping 2001:db8:2::10");
    assert!(p.contains("4 answered"), "{p}\n{:?}", super::dropped_reasons(&lab));
    lab.sim.clear_trace();
    secs(&mut lab, 2.0);
    assert!(
        lab.sim.trace().iter().any(|e| e.summary.starts_with("VRRPv3 (IPv6) advertisement, group 1, priority 120") && e.summary.contains("ff02::12"))
    );
    assert!(lab.sim.trace().iter().any(|e| e.frame.get(6..12) == Some(&[0, 0, 0x5e, 0, 2, 1][..])));
    // R1 loses its LAN side: R2 takes group 1 after the skew (goodbye) and group 2 after its
    // interval (tracking), so the path works again through R2.
    lab.run(r1, "conf t\nint g0/0\nshutdown\nend");
    secs(&mut lab, 4.0);
    let v = lab.run(r2, "show vrrp");
    assert!(v.contains("IPv6 group 1 on GigabitEthernet0/0: master") && v.contains("IPv6 group 2 on GigabitEthernet0/1: master"), "{v}");
    let p = lab.run(pc, "ping 2001:db8:2::10");
    assert!(p.contains("answered") && !p.contains(" 0 answered"), "{p}\n{:?}", super::dropped_reasons(&lab));
    let run = lab.run(r2, "show running-config interface g0/0");
    assert!(run.contains(" vrrp 1 ipv6 fe80::1"), "{run}");
    lab.run(r2, "conf t\nint g0/0");
    assert!(lab.try_run(r2, "vrrp 1 ipv6 2001:db8::1").contains("link-local"));
    lab.run(r2, "no vrrp 1 ipv6\nend");
    assert!(!lab.run(r2, "show vrrp").contains("IPv6 group 1"));
}

/// R1 — R2 — R3 in a line (and R1 — R3 over a slow link), R3 with two loopback networks.
/// `v6` picks EIGRP for IPv6 (AS 10) or for IPv4 (AS 1).
fn eigrp_line(v6: bool) -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    lab.cable(r1, "g0/2", r3, "g0/2");
    // (router, its links (interface, subnet number, host), loopbacks)
    let plan: [(DeviceId, u8, &[(&str, u8)], &[u8]); 3] =
        [(r1, 1, &[("g0/1", 12), ("g0/2", 13)], &[]), (r2, 2, &[("g0/0", 12), ("g0/1", 23)], &[]), (r3, 3, &[("g0/0", 23), ("g0/2", 13)], &[30, 31])];
    for (r, n, links, loops) in plan {
        let mut s = String::from("en\nconf t\n");
        if v6 {
            s.push_str(&format!("ipv6 unicast-routing\nipv6 router eigrp 10\neigrp router-id {n}.{n}.{n}.{n}\nexit\n"));
        } else {
            s.push_str("router eigrp 1\nnetwork 10.0.0.0\nnetwork 172.16.0.0\nexit\n");
        }
        for (ifc, net) in links {
            if v6 {
                s.push_str(&format!("int {ifc}\nipv6 address 2001:db8:{net}::{n}/64\nipv6 eigrp 10\nno shut\n"));
            } else {
                s.push_str(&format!("int {ifc}\nip address 10.0.{net}.{n} 255.255.255.0\nno shut\n"));
            }
            if *ifc == "g0/2" {
                s.push_str("bandwidth 10000\n");
            }
        }
        for (k, l) in loops.iter().enumerate() {
            if v6 {
                s.push_str(&format!("int lo{k}\nipv6 address 2001:db8:{l}::1/64\nipv6 eigrp 10\n"));
            } else {
                s.push_str(&format!("int lo{k}\nip address 172.16.{}.1 255.255.255.0\n", l - 30));
            }
        }
        s.push_str("end");
        lab.run(r, &s);
    }
    (lab, r1, r2, r3)
}

#[test]
fn eigrp_for_ipv6_puts_dual_on_the_wire_and_summarises() {
    let (mut lab, r1, r2, r3) = eigrp_line(true);
    let route = lab.run(r1, "show ipv6 route eigrp");
    assert!(route.contains("2001:db8:30::/64") && route.contains("2001:db8:31::/64"), "{route}");
    // Variance: the slow direct link is a feasible successor; variance 128 installs it too.
    let one = route.lines().filter(|l| l.contains("2001:db8:30::/64")).count();
    assert_eq!(one, 1, "{route}");
    lab.run(r1, "conf t\nipv6 router eigrp 10\nvariance 128\nend");
    let route = lab.run(r1, "show ipv6 route eigrp");
    let paths: Vec<&str> = route.lines().filter(|l| l.contains("2001:db8:30::/64")).collect();
    assert_eq!(paths.len(), 2, "{route}");
    assert!(lab.run(r1, "show running-config").contains("ipv6 router eigrp 10\n eigrp router-id 1.1.1.1\n variance 128\n"));
    lab.run(r1, "conf t\nipv6 router eigrp 10\nno variance\nend");
    // R2 summarises R3's loopbacks towards R1.
    lab.sim.clear_trace();
    lab.run(r2, "conf t\nint g0/0\nipv6 summary-address eigrp 10 2001:db8:30::/47\nend");
    let route = lab.run(r1, "show ipv6 route eigrp");
    assert!(route.contains("2001:db8:30::/47") && route.contains("GigabitEthernet0/1"), "{route}");
    // R1 still hears the /64s over the slow link from R3, which doesn't summarise.
    assert!(route.lines().filter(|l| l.contains("2001:db8:31::/64")).all(|l| l.contains("GigabitEthernet0/2")), "{route}");
    let own = lab.run(r2, "show ipv6 route");
    assert!(own.contains("2001:db8:30::/47") && own.contains("summary, discarded on Null0 [5]"), "{own}");
    // The change went out as an update, acknowledged.
    lab.sim.run_for(&lab.t, 100_000);
    let saw = |lab: &Lab, what: &str| lab.sim.trace().iter().any(|e| e.summary.starts_with(what));
    assert!(saw(&lab, "EIGRPv6 update"), "{:?}", lab.sim.trace().iter().map(|e| e.summary.clone()).collect::<Vec<_>>());
    assert!(saw(&lab, "EIGRPv6 acknowledgement"));
    // R3 loses a loopback: it queries its neighbours, which reply, all acknowledged.
    lab.sim.clear_trace();
    lab.run(r3, "conf t\nint lo1\nshutdown\nend");
    let _ = lab.run(r3, "show ipv6 eigrp topology");
    lab.sim.run_for(&lab.t, 100_000);
    assert!(saw(&lab, "EIGRPv6 query"), "{:?}", lab.sim.trace().iter().map(|e| e.summary.clone()).collect::<Vec<_>>());
    assert!(saw(&lab, "EIGRPv6 reply"));
    assert!(saw(&lab, "EIGRPv6 acknowledgement"));
    // A new neighbour gets the whole table in an initial update.
    lab.sim.clear_trace();
    lab.run(r1, "conf t\nint g0/1\nshutdown\nend");
    let _ = lab.run(r1, "show ipv6 eigrp neighbors");
    lab.sim.clear_trace();
    lab.run(r1, "conf t\nint g0/1\nno shutdown\nend");
    let _ = lab.run(r1, "show ipv6 eigrp neighbors");
    lab.sim.run_for(&lab.t, 100_000);
    assert!(saw(&lab, "EIGRPv6 update"));
    // The configuration reads back, survives a reload and comes off.
    let run = lab.run(r2, "show running-config interface g0/0");
    assert!(run.contains(" ipv6 summary-address eigrp 10 2001:db8:30::/47"), "{run}");
    let all = lab.run(r2, "show running-config");
    lab.run(r2, "copy run start\nreload\nen");
    assert_eq!(lab.run(r2, "show running-config"), all);
    lab.run(r2, "conf t\nint g0/0\nno ipv6 summary-address eigrp 10 2001:db8:30::/47\nend");
    assert!(!lab.run(r2, "show ipv6 route").contains("/47"));
    lab.run(r2, "conf t\nint g0/0");
    assert!(lab.try_run(r2, "ipv6 summary-address eigrp 10 nonsense").contains("not an IPv6 prefix"));
    assert!(lab.try_run(r2, "ipv6 summary-address eigrp 0 2001:db8::/32").contains("1 to 65535"));
    assert!(lab.run(r2, "ipv6 summary-address eigrp 99 2001:db8::/32").contains("Saved"));
}

#[test]
fn eigrp_for_ipv4_summary_addresses() {
    let (mut lab, r1, r2, _r3) = eigrp_line(false);
    let route = lab.run(r1, "show ip route");
    assert!(route.contains("172.16.0.0/24") && route.contains("172.16.1.0/24"), "{route}");
    lab.run(r2, "conf t\nint g0/0\nip summary-address eigrp 1 172.16.0.0 255.255.254.0\nend");
    let route = lab.run(r1, "show ip route");
    assert!(route.contains("D    172.16.0.0/23") && route.contains("via 10.0.12.2"), "{route}");
    let own = lab.run(r2, "show ip route");
    assert!(own.contains("172.16.0.0/23") && own.contains("summary, discarded on Null0 (EIGRP) [5/"), "{own}");
    assert!(own.contains("172.16.1.0/24"), "R2 keeps the parts: {own}");
    let run = lab.run(r2, "show running-config interface g0/0");
    assert!(run.contains(" ip summary-address eigrp 1 172.16.0.0 255.255.254.0"), "{run}");
    let all = lab.run(r2, "show running-config");
    lab.run(r2, "copy run start\nreload\nen");
    assert_eq!(lab.run(r2, "show running-config"), all);
    lab.run(r2, "conf t\nint g0/0");
    assert!(lab.try_run(r2, "ip summary-address eigrp 1 172.16.0.0 255.0.255.0").starts_with("% "));
    lab.run(r2, "no ip summary-address eigrp 1 172.16.0.0 255.255.254.0\nend");
    assert!(!lab.run(r1, "show ip route").contains("/23"));
}
