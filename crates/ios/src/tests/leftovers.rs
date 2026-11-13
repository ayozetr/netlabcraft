//! Typed console tests for the switch and QoS leftovers: `mls qos` trust, shaping, loop guard
//! and BPDU filter, the rapid spanning-tree handshake, option 82 with snooping, and HSRP
//! preemption.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// The DS byte of an untagged IPv4 frame.
fn ds_of(frame: &[u8]) -> Option<u8> {
    (frame.get(12..14) == Some(&[0x08, 0x00][..])).then(|| frame.get(15).copied()).flatten()
}

/// PC1 — R1 — S1 — R2: R1 marks everything it sends S1 as EF.
fn marking_lab() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let pc = lab.add(DeviceKind::Pc);
    let r1 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(pc, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", s1, "fa0/1");
    lab.cable(r2, "g0/1", s1, "fa0/2");
    lab.run(pc, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(
        r1,
        "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip add 10.0.0.1 255.255.255.0\nno shut\nexit\n\
         policy-map MARK\nclass class-default\nset dscp ef\nexit\nexit\nint g0/1\nservice-policy output MARK\nend",
    );
    lab.run(r2, "en\nconf t\nint g0/1\nip add 10.0.0.2 255.255.255.0\nno shut\nexit\nip route 192.168.1.0 255.255.255.0 10.0.0.1\nend");
    lab.run(s1, "en");
    (lab, pc, r1, s1, r2)
}

/// The DS bytes of the echo requests S1 handed R2.
fn marks_reaching(lab: &Lab, s1: DeviceId, r2: DeviceId) -> Vec<u8> {
    lab.sim
        .trace()
        .iter()
        .filter(|e| e.from.device == s1 && e.to.device == r2 && e.summary.starts_with("ICMP echo request"))
        .filter_map(|e| ds_of(&e.frame))
        .collect()
}

#[test]
fn mls_qos_rewrites_untrusted_markings_and_trust_keeps_them() {
    let (mut lab, pc, _r1, s1, r2) = marking_lab();
    lab.run(pc, "ping 10.0.0.2");
    lab.sim.clear_trace();
    assert!(lab.run(pc, "ping 10.0.0.2").contains("4 answered"));
    // QoS off: the switch passes EF (DSCP 46, DS byte 0xb8) as it came.
    let marks = marks_reaching(&lab, s1, r2);
    assert!(!marks.is_empty() && marks.iter().all(|b| *b == 0xb8), "{marks:?}");
    assert!(lab.run(s1, "show mls qos").contains("disabled"));
    // QoS on, port untrusted: rewritten to 0.
    let note = lab.run(s1, "conf t\nmls qos");
    assert!(note.contains("rewrite"), "{note}");
    lab.run(s1, "end");
    lab.sim.clear_trace();
    assert!(lab.run(pc, "ping 10.0.0.2").contains("4 answered"));
    let marks = marks_reaching(&lab, s1, r2);
    assert!(!marks.is_empty() && marks.iter().all(|b| *b == 0), "{marks:?}");
    let show = lab.run(s1, "show mls qos interface fa0/1");
    assert!(show.contains("QoS enabled") && show.contains("Fa0/1      none") && show.contains(" 4"), "{show}");
    // Trust DSCP: kept.
    lab.run(s1, "conf t\nint fa0/1\nmls qos trust dscp\nend");
    lab.sim.clear_trace();
    assert!(lab.run(pc, "ping 10.0.0.2").contains("4 answered"));
    let marks = marks_reaching(&lab, s1, r2);
    assert!(!marks.is_empty() && marks.iter().all(|b| *b == 0xb8), "{marks:?}");
    assert!(lab.run(s1, "show mls qos interface fa0/1").contains("Fa0/1      dscp"));
    // Trust CoS: the frames carry priority 0, so DSCP 0.
    lab.run(s1, "conf t\nint fa0/1\nmls qos trust cos\nend");
    lab.sim.clear_trace();
    assert!(lab.run(pc, "ping 10.0.0.2").contains("4 answered"));
    assert!(marks_reaching(&lab, s1, r2).iter().all(|b| *b == 0));
    let run = lab.run(s1, "show running-config");
    assert!(run.contains("\nmls qos\n") && run.contains(" mls qos trust cos"), "{run}");
    // Round trip, then the `no` forms.
    lab.run(s1, "copy run start\nreload\nen");
    assert_eq!(lab.run(s1, "show running-config"), run);
    lab.run(s1, "conf t\nint fa0/1\nno mls qos trust\nexit\nno mls qos\nend");
    let run = lab.run(s1, "show running-config");
    assert!(!run.contains("mls qos"), "{run}");
    lab.run(s1, "conf t\nint fa0/1");
    assert!(lab.try_run(s1, "mls qos trust ip-precedence").contains("`dscp` or `cos`"));
}

#[test]
fn shaping_delays_pings_and_shows_its_counters() {
    let (mut lab, pc, r1, _s1, _r2) = marking_lab();
    lab.run(r1, "conf t\npolicy-map SHAPE\nclass class-default\nshape average 8000\nexit\nexit\nint g0/1\nservice-policy output SHAPE\nend");
    lab.run(pc, "ping 10.0.0.2");
    // 1428-byte echo requests a second apart at 8 kbit/s (1000 bytes a second): the bucket
    // takes the first, then each waits longer.
    let out = lab.run(pc, "ping -l 1400 -n 4 10.0.0.2");
    assert!(out.contains("4 answered") || out.contains("answered"), "{out}");
    let show = lab.run(r1, "show policy-map interface g0/1");
    assert!(show.contains("shape average 8000 bit/s, bucket 1500 bytes:"), "{show}");
    assert!(!show.contains(" 0 packets delayed"), "{show}");
    // Each shaped echo request reached S1 later than the one before it would have unshaped.
    let sent: Vec<u64> = lab
        .sim
        .trace()
        .iter()
        .filter(|e| e.from.device == r1 && e.summary.starts_with("ICMP echo request") && e.frame.len() > 1400)
        .map(|e| e.at_us)
        .collect();
    assert!(sent.len() >= 3, "{sent:?}");
    let gaps: Vec<u64> = sent.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.iter().any(|g| *g > 1_100_000), "shaping should stretch the 1 s spacing: {gaps:?}");
    let pm = lab.run(r1, "show running-config");
    assert!(pm.contains("policy-map SHAPE\n class class-default\n  shape average 8000"), "{pm}");
}

/// S1 (root), S2 and S3 in a triangle; S2 beats S3, so S3's port to S2 is the blocked one.
fn triangle() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "fa0/1", s2, "fa0/1");
    lab.cable(s1, "fa0/2", s3, "fa0/1");
    lab.cable(s2, "fa0/2", s3, "fa0/2");
    lab.run(s1, "en\nconf t\nspanning-tree vlan 1 priority 4096\nend");
    lab.run(s2, "en\nconf t\nspanning-tree vlan 1 priority 8192\nend");
    lab.run(s3, "en");
    (lab, s1, s2, s3)
}

#[test]
fn loop_guard_blocks_a_port_whose_bpdus_stop() {
    let (mut lab, _s1, s2, s3) = triangle();
    let st = lab.run(s3, "show spanning-tree vlan 1");
    assert!(st.contains("Fa0/2      Altn BLK"), "{st}");
    // S2 stops sending BPDUs on its port to S3: S3 hears nothing and, unguarded, forwards —
    // a loop.
    let note = lab.run(s2, "conf t\nint fa0/2\nspanning-tree bpdufilter enable");
    assert!(note.contains("loop guard"), "{note}");
    lab.run(s2, "end");
    lab.sim.run_for(&lab.t, 31_000_000);
    let st = lab.run(s3, "show spanning-tree vlan 1");
    assert!(st.contains("Fa0/2      Desg FWD"), "{st}");
    // Loop guard on S3's port: loop-inconsistent instead.
    lab.run(s3, "conf t\nint fa0/2\nspanning-tree guard loop\nend");
    let st = lab.run(s3, "show spanning-tree vlan 1");
    assert!(st.contains("Fa0/2      Altn BLK") && st.contains("Loop-inconsistent"), "{st}");
    let inc = lab.run(s3, "show spanning-tree inconsistentports");
    assert!(inc.contains("VLAN0001   Fa0/2        Loop inconsistent") && inc.contains("1 inconsistent port"), "{inc}");
    assert!(lab.run(s3, "show spanning-tree vlan 1 detail").contains("loop-inconsistent"));
    // BPDUs flow again: the port goes back to plain alternate.
    lab.run(s2, "conf t\nint fa0/2\nno spanning-tree bpdufilter\nend");
    let st = lab.run(s3, "show spanning-tree vlan 1");
    assert!(st.contains("Fa0/2      Altn BLK") && !st.contains("Loop-inconsistent"), "{st}");
    // Loop guard on a port that should be designated changes nothing (it sends, not hears).
    lab.run(s2, "conf t\nint fa0/2\nspanning-tree guard loop\nspanning-tree bpdufilter enable\nend");
    assert!(lab.run(s2, "show spanning-tree vlan 1").contains("Fa0/2      Desg FWD"));
    lab.run(s2, "conf t\nint fa0/2\nno spanning-tree guard\nno spanning-tree bpdufilter\nend");
    // The global default covers every switch-to-switch port.
    lab.run(s3, "conf t\nint fa0/2\nno spanning-tree guard\nexit\nspanning-tree loopguard default\nend");
    lab.run(s2, "conf t\nint fa0/2\nspanning-tree bpdufilter enable\nend");
    assert!(lab.run(s3, "show spanning-tree vlan 1").contains("Loop-inconsistent"));
    assert!(lab.run(s3, "show spanning-tree summary").contains("Loop guard default:   on"));
    let run = lab.run(s3, "show running-config");
    assert!(run.contains("spanning-tree loopguard default"), "{run}");
    let run2 = lab.run(s2, "show running-config interface fa0/2");
    assert!(run2.contains(" spanning-tree bpdufilter enable"), "{run2}");
    lab.run(s3, "copy run start\nreload\nen");
    assert_eq!(lab.run(s3, "show running-config"), run);
    lab.run(s3, "conf t\nint fa0/2");
    assert!(lab.try_run(s3, "spanning-tree guard maybe").contains("`root`, `loop` or `none`"));
    assert!(lab.try_run(s3, "spanning-tree bpdufilter perhaps").contains("`enable` or `disable`"));
}

#[test]
fn rapid_spanning_tree_handshakes_and_forwards_at_once() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "fa0/1", s2, "fa0/1");
    for s in [s1, s2] {
        lab.run(s, "en\nconf t\nspanning-tree mode rapid-pvst\nend");
    }
    lab.run(s1, "conf t\nspanning-tree vlan 1 priority 4096\nend");
    // Let the tree settle, then add a second link: the new ports come up.
    lab.sim.run_for(&lab.t, 3_000_000);
    lab.run(s1, "show spanning-tree vlan 1");
    lab.sim.clear_trace();
    lab.cable(s1, "fa0/2", s2, "fa0/2");
    lab.sim.run_for(&lab.t, 1_000_000);
    // Point-to-point, both rapid: the designated port proposes, the other side agrees, and
    // nothing waits for the timers.
    let st = lab.run(s1, "show spanning-tree vlan 1");
    assert!(st.contains("Fa0/2      Desg FWD"), "{st}");
    let rstp: Vec<String> = lab.sim.trace().iter().filter(|e| e.summary.starts_with("RSTP BPDU")).map(|e| e.summary.clone()).collect();
    assert!(rstp.iter().any(|s| s.contains("[designated, proposal]")), "{rstp:?}");
    assert!(rstp.iter().any(|s| s.contains("agreement")), "{rstp:?}");
    // The periodic BPDUs are RST BPDUs too, flagged learning and forwarding.
    lab.sim.run_for(&lab.t, 3_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("RSTP BPDU") && e.summary.contains("designated, learning, forwarding")));
    // A shared link (half duplex, or set so) gets no handshake: it waits out the timers.
    lab.cable(s1, "fa0/3", s2, "fa0/3");
    lab.run(s1, "conf t\nint fa0/3\nspanning-tree link-type shared\nend");
    lab.sim.run_for(&lab.t, 3_000_000);
    lab.run(s1, "conf t\nint fa0/4\nspanning-tree link-type shared\nend");
    lab.cable(s1, "fa0/4", s2, "fa0/4");
    lab.sim.run_for(&lab.t, 1_000_000);
    let st = lab.run(s1, "show spanning-tree vlan 1");
    assert!(st.contains("Fa0/4      Desg LIS"), "{st}");
}

#[test]
fn option_82_from_snooping_needs_the_router_to_trust_it() {
    let (mut lab, r1, s, a, _b) = super::snooping_lab();
    // Insertion is on by default once snooping runs.
    lab.run(s, "en\nconf t\nip dhcp snooping\nip dhcp snooping vlan 1\nint fa0/24\nip dhcp snooping trust\nend");
    assert!(lab.run(s, "show ip dhcp snooping").contains("Option 82 insertion:      on"));
    lab.sim.clear_trace();
    let out = lab.try_run(a, "ipconfig /renew");
    assert!(!out.contains("192.168.10."), "{out}");
    let drops = super::dropped_reasons(&lab);
    assert!(drops.iter().any(|r| r.contains("option 82") && r.contains("untrusted GigabitEthernet0/0")), "{drops:?}");
    // What the switch handed R1 carried the option: VLAN 1, port 1, the switch's MAC.
    let tagged: Vec<String> = lab
        .sim
        .trace()
        .iter()
        .filter(|e| e.from.device == s && e.to.device == r1 && e.summary.contains("option 82"))
        .map(|e| e.summary.clone())
        .collect();
    assert!(tagged.iter().any(|t| t.contains("DISCOVER") && t.contains("circuit VLAN 1 port 1")), "{tagged:?}");
    // The router trusts it: the client gets its address, and never sees the option.
    lab.run(r1, "conf t\nip dhcp relay information trust-all\nend");
    lab.sim.clear_trace();
    let out = lab.run(a, "ipconfig /renew");
    assert!(out.contains("192.168.10.10/24"), "{out}");
    let to_client: Vec<String> =
        lab.sim.trace().iter().filter(|e| e.to.device == a && e.summary.starts_with("DHCP")).map(|e| e.summary.clone()).collect();
    assert!(!to_client.is_empty() && to_client.iter().all(|t| !t.contains("option 82")), "{to_client:?}");
    assert!(lab.sim.trace().iter().any(|e| e.from.device == r1 && e.summary.contains("OFFER") && e.summary.contains("option 82")));
    assert!(lab.run(s, "show ip dhcp snooping binding").contains("192.168.10.10"));
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("ip dhcp relay information trust-all"), "{run}");
    // Per interface instead of everywhere.
    lab.run(r1, "conf t\nno ip dhcp relay information trust-all\nint g0/0\nip dhcp relay information trusted\nend");
    assert!(lab.run(r1, "show running-config interface g0/0").contains(" ip dhcp relay information trusted"));
    lab.run(a, "ipconfig /release");
    assert!(lab.run(a, "ipconfig /renew").contains("192.168.10."));
    lab.run(r1, "copy run start\nreload\nen");
    assert!(lab.run(r1, "show running-config").contains(" ip dhcp relay information trusted"));
    // Or the switch stops inserting it.
    lab.run(r1, "conf t\nint g0/0\nno ip dhcp relay information trusted\nend");
    lab.run(s, "conf t\nno ip dhcp snooping information option\nend");
    lab.run(a, "ipconfig /release");
    assert!(lab.run(a, "ipconfig /renew").contains("192.168.10."));
}

#[test]
fn hsrp_preempt_decides_whether_a_better_router_takes_over() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    lab.cable(r1, "g0/0", s1, "fa0/1");
    lab.cable(r2, "g0/0", s1, "fa0/2");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nstandby 1 ip 192.168.1.254\nstandby 1 priority 110\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 192.168.1.2 255.255.255.0\nstandby 1 ip 192.168.1.254\nno shut\nend");
    assert!(lab.run(r1, "show standby").contains("Group 1 on GigabitEthernet0/0: active (priority 110)"));
    // R1 goes away: R2 takes over.
    lab.run(r1, "conf t\nint g0/0\nshutdown\nend");
    lab.sim.run_for(&lab.t, 4_000_000);
    assert!(lab.run(r2, "show standby").contains(": active"));
    // R1 comes back with the better priority but no preempt: R2 stays active.
    lab.run(r1, "conf t\nint g0/0\nno shutdown\nend");
    lab.sim.run_for(&lab.t, 4_000_000);
    let sb = lab.run(r1, "show standby");
    assert!(sb.contains("Group 1 on GigabitEthernet0/0: standby (priority 110)"), "{sb}");
    assert!(lab.run(r2, "show standby").contains(": active"));
    // With preempt it takes the role back.
    lab.run(r1, "conf t\nint g0/0\nstandby 1 preempt\nend");
    lab.sim.run_for(&lab.t, 4_000_000);
    assert!(lab.run(r1, "show standby").contains("active (priority 110, preempt)"));
    assert!(lab.run(r2, "show standby").contains(": standby"));
}

/// R1, R2 and R3 in a triangle over IPv6 (Gigabit R1–R2 and R2–R3, a slow R1–R3 by its
/// bandwidth), each with a LAN; EIGRP for IPv6 AS 10 everywhere, router ids by hand.
fn eigrp6_triangle() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    lab.cable(r1, "g0/2", r3, "g0/1");
    lab.cable(pc, "fa0", r3, "g0/0");
    let cfg = |n: u8, links: &[(&str, &str, Option<u32>)]| {
        let mut s = format!("en\nconf t\nipv6 unicast-routing\nipv6 router eigrp 10\neigrp router-id {n}.{n}.{n}.{n}\nexit\n");
        s.push_str(&format!("int g0/0\nipv6 address 2001:db8:{n}::1/64\nipv6 eigrp 10\nno shut\n"));
        for (ifc, addr, bw) in links {
            s.push_str(&format!("int {ifc}\nipv6 address {addr}\nipv6 eigrp 10\nno shut\n"));
            if let Some(b) = bw {
                s.push_str(&format!("bandwidth {b}\n"));
            }
        }
        s.push_str("end");
        s
    };
    lab.run(r1, &cfg(1, &[("g0/1", "2001:db8:12::1/64", None), ("g0/2", "2001:db8:13::1/64", Some(10_000))]));
    lab.run(r2, &cfg(2, &[("g0/1", "2001:db8:12::2/64", None), ("g0/2", "2001:db8:23::2/64", None)]));
    lab.run(r3, &cfg(3, &[("g0/2", "2001:db8:23::3/64", None), ("g0/1", "2001:db8:13::3/64", Some(10_000))]));
    lab.run(pc, "ipv6config 2001:db8:3::10/64 2001:db8:3::1");
    (lab, r1, r2, r3, pc)
}

#[test]
fn eigrp_for_ipv6_by_typing() {
    let (mut lab, r1, _r2, r3, _pc) = eigrp6_triangle();
    assert_eq!(lab.prompt(r1), "R1#");
    // Neighbours over link-local addresses, one per link.
    let nb = lab.run(r1, "show ipv6 eigrp neighbors");
    assert!(nb.contains("EIGRP for IPv6 neighbours, AS 10") && nb.contains("fe80::") && nb.contains("Gi0/1") && nb.contains("Gi0/2"), "{nb}");
    // R3's LAN: the fast way through R2 is the successor; the direct slow link reports a
    // smaller distance than R1's feasible distance, so it is a feasible successor.
    let topo = lab.run(r1, "show ipv6 eigrp topology");
    assert!(topo.contains("router id 1.1.1.1") && topo.contains("2001:db8:3::/64  FD 3328, 1 successor"), "{topo}");
    assert!(topo.contains("successor           via fe80::") && topo.contains("feasible successor  via fe80::"), "{topo}");
    let route = lab.run(r1, "show ipv6 route eigrp");
    assert!(route.contains("D  2001:db8:3::/64") && route.contains("GigabitEthernet0/1") && route.contains("[90]"), "{route}");
    assert!(!route.contains("\nC  "), "{route}");
    // Packets follow it: R1 reaches the PC's LAN.
    let ping = lab.run(r1, "ping 2001:db8:3::10");
    assert!(!ping.contains(" 0 answered") && !ping.contains("0/"), "{ping}");
    let ifs = lab.run(r1, "show ipv6 eigrp interfaces");
    assert!(ifs.contains("Gi0/2") && ifs.contains("10000 kbit/s"), "{ifs}");
    // R2 goes away: the feasible successor takes over.
    lab.run(r1, "conf t\nint g0/1\nshutdown\nend");
    let route = lab.run(r1, "show ipv6 route eigrp");
    assert!(route.contains("2001:db8:3::/64") && route.contains("GigabitEthernet0/2"), "{route}");
    lab.run(r1, "conf t\nint g0/1\nno shutdown\nend");
    // Hellos on the wire, to FF02::A from link-local addresses.
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 6_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("EIGRPv6 hello, AS 10 (hold 15 s) fe80::") && e.summary.contains("ff02::a")));
    // Running configuration and its round trip.
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("ipv6 router eigrp 10\n eigrp router-id 1.1.1.1\n!"), "{run}");
    assert!(run.contains(" ipv6 address 2001:db8:13::1/64\n ipv6 eigrp 10"), "{run}");
    lab.run(r1, "copy run start\nreload\nen");
    assert_eq!(lab.run(r1, "show running-config"), run);
    // Passive: R1 stops talking to R3 on that link but still advertises the link.
    lab.run(r1, "conf t\nipv6 router eigrp 10\npassive-interface g0/2\nend");
    assert!(!lab.run(r1, "show ipv6 eigrp neighbors").contains("Gi0/2"));
    assert!(lab.run(r3, "show ipv6 eigrp neighbors").contains("Gi0/2"));
    // Shutdown stops the process; `no shutdown` brings it back.
    lab.run(r1, "conf t\nipv6 router eigrp 10\nshutdown\nend");
    assert!(lab.run(r1, "show ipv6 eigrp neighbors").contains("shut down"));
    assert!(lab.run(r1, "show ipv6 route eigrp").contains("(no such routes)"));
    lab.run(r1, "conf t\nipv6 router eigrp 10\nno shutdown\nend");
    assert!(lab.run(r1, "show ipv6 route eigrp").contains("2001:db8:3::/64"));
}

#[test]
fn eigrp_for_ipv6_mistakes_are_explained() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t");
    let note = lab.run(r, "ipv6 router eigrp 5");
    assert!(note.contains("ipv6 unicast-routing") && note.contains("router id"), "{note}");
    assert_eq!(lab.prompt(r), "R1(config-rtr)#");
    assert!(lab.try_run(r, "network 10.0.0.0").contains("no network statements"));
    assert!(lab.try_run(r, "auto-summary").contains("never summarises"));
    lab.run(r, "eigrp router-id 9.9.9.9\nmaximum-paths 2\nexit");
    assert!(lab.try_run(r, "ipv6 router eigrp 6").contains("already runs here in AS 5"));
    assert!(lab.try_run(r, "no ipv6 router eigrp 6").contains("No EIGRP for IPv6 process for AS 6"));
    lab.run(r, "router eigrp 1");
    assert!(lab.try_run(r, "shutdown").contains("applies to EIGRP for IPv6"));
    lab.run(r, "exit\nint g0/0");
    assert!(lab.run(r, "ipv6 eigrp 7").contains("Saved"));
    lab.run(r, "end");
    assert!(lab.run(r, "show ipv6 eigrp topology").contains("waits for `ipv6 unicast-routing`"));
    lab.run(r, "conf t\nno ipv6 router eigrp 5\nend");
    assert!(lab.run(r, "show ipv6 eigrp neighbors").contains("not running"));
}

/// PC1 and PC2 on S1 with R1 (192.168.1.1) and R2 (192.168.1.2); a server behind both on S2.
fn gateway_pair() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let pc1 = lab.add(DeviceKind::Pc);
    let pc2 = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(pc1, "fa0", s1, "fa0/1");
    lab.cable(pc2, "fa0", s1, "fa0/4");
    lab.cable(r1, "g0/0", s1, "fa0/2");
    lab.cable(r2, "g0/0", s1, "fa0/3");
    lab.cable(r1, "g0/1", s2, "fa0/2");
    lab.cable(r2, "g0/1", s2, "fa0/3");
    lab.cable(srv, "g0", s2, "fa0/1");
    lab.run(pc1, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.254");
    lab.run(pc2, "ipconfig 192.168.1.11 255.255.255.0 192.168.1.254");
    lab.run(srv, "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 192.168.1.2 255.255.255.0\nno shut\nint g0/1\nip address 10.0.0.2 255.255.255.0\nno shut\nend");
    (lab, r1, r2, pc1, pc2)
}

#[test]
fn vrrp_by_typing() {
    let (mut lab, r1, r2, pc1, _pc2) = gateway_pair();
    lab.run(r1, "conf t\nint g0/0\nvrrp 1 ip 192.168.1.254\nvrrp 1 priority 110\nint g0/1\nvrrp 10 ip 10.0.0.254\nend");
    lab.run(r2, "conf t\nint g0/0\nvrrp 1 ip 192.168.1.254\nint g0/1\nvrrp 10 ip 10.0.0.254\nend");
    let srv = lab.t.devices.iter().find(|d| d.kind == DeviceKind::Server).unwrap().id;
    lab.run(srv, "ipconfig 10.0.0.10 255.255.255.0 10.0.0.254");
    let v = lab.run(r1, "show vrrp");
    assert!(v.contains("Group 1 on GigabitEthernet0/0: master (priority 110, preempt)") && v.contains("virtual MAC 0000.5e00.0101"), "{v}");
    assert!(v.contains("VRRP version 2"), "{v}");
    assert!(lab.run(r2, "show vrrp brief").contains("backup  192.168.1.1"));
    lab.run(pc1, "ping 10.0.0.10");
    let p = lab.run(pc1, "ping 10.0.0.10");
    assert!(p.contains("4 answered"), "{p}\n{:?}\n{}", super::dropped_reasons(&lab), lab.run(pc1, "arp -a"));
    assert!(lab.run(pc1, "arp -a").contains("00:00:5e:00:01:01"));
    // A backup acting as master doesn't take packets for an address it doesn't own (accept
    // mode off, RFC 5798 §6.4.3).
    assert!(lab.run(pc1, "ping 192.168.1.254").contains("0 answered"));
    // The master advertises every second from the virtual MAC.
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 3_000_000);
    let ads: Vec<String> = lab.sim.trace().iter().filter(|e| e.summary.starts_with("VRRPv2 advertisement")).map(|e| e.summary.clone()).collect();
    assert!(ads.iter().any(|a| a.contains("group 1, priority 110, every 1 s, 192.168.1.254")), "{ads:?}");
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("VRRPv2") && e.frame.get(6..12) == Some(&[0, 0, 0x5e, 0, 1, 1][..])));
    // R1 fails: R2 takes over; R1 comes back and, preempting by default, is master again.
    lab.run(r1, "conf t\nint g0/0\nshutdown\nend");
    lab.sim.run_for(&lab.t, 2_000_000);
    assert!(lab.run(r2, "show vrrp").contains(": master"));
    lab.run(pc1, "ping 10.0.0.10");
    assert!(lab.run(pc1, "ping 10.0.0.10").contains("4 answered"));
    lab.run(r1, "conf t\nint g0/0\nno shutdown\nend");
    // Preempting waits R1's master down interval (3 s and a skew).
    let _ = lab.run(r1, "show vrrp");
    lab.sim.run_for(&lab.t, 4_000_000);
    assert!(lab.run(r1, "show vrrp").contains(": master"));
    // Without preempt it waits for R2 to fail.
    lab.run(r1, "conf t\nint g0/0\nno vrrp 1 preempt\nshutdown\nend");
    lab.sim.run_for(&lab.t, 2_000_000);
    lab.run(r1, "conf t\nint g0/0\nno shutdown\nend");
    lab.sim.run_for(&lab.t, 2_000_000);
    assert!(lab.run(r1, "show vrrp").contains(": backup (priority 110)"));
    // The address owner (its own address is the virtual one) has priority 255 and always wins.
    lab.run(r2, "conf t\nint g0/0\nvrrp 2 ip 192.168.1.2\nend");
    lab.run(r1, "conf t\nint g0/0\nvrrp 2 ip 192.168.1.2\nvrrp 2 priority 254\nend");
    assert!(lab.run(r2, "show vrrp").contains("Group 2 on GigabitEthernet0/0: master (priority 255, address owner"));
    // Version 3 on the wire.
    lab.run(r2, "conf t\nfhrp version vrrp v3\nend");
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 2_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("VRRPv3 advertisement") && e.summary.contains("every 100 cs")));
    let run = lab.run(r2, "show running-config");
    assert!(run.contains("fhrp version vrrp v3") && run.contains(" vrrp 1 ip 192.168.1.254\n vrrp 2 ip 192.168.1.2"), "{run}");
    let run1 = lab.run(r1, "show running-config interface g0/0");
    assert!(run1.contains(" vrrp 1 ip 192.168.1.254\n vrrp 1 priority 110\n no vrrp 1 preempt"), "{run1}");
    lab.run(r1, "copy run start\nreload\nen");
    assert!(lab.run(r1, "show running-config interface g0/0").contains(" no vrrp 1 preempt"));
    // Mistakes and removal.
    lab.run(r1, "conf t\nint g0/0");
    assert!(lab.try_run(r1, "vrrp 3 ip 172.16.0.1").contains("not on"));
    assert!(lab.try_run(r1, "vrrp 0 ip 192.168.1.250").contains("1-255"));
    assert!(lab.try_run(r1, "vrrp 3 priority 255").contains("1-254"));
    lab.run(r1, "no vrrp 1\nno vrrp 2\nint g0/1\nno vrrp 10\nend");
    assert!(lab.run(r1, "show vrrp").contains("No VRRP group"));
}

#[test]
fn glbp_shares_hosts_among_its_forwarders() {
    let (mut lab, r1, r2, pc1, pc2) = gateway_pair();
    lab.run(r1, "conf t\nint g0/0\nglbp 1 ip 192.168.1.254\nglbp 1 priority 110\nend");
    lab.run(r2, "conf t\nint g0/0\nglbp 1 ip 192.168.1.254\nend");
    let g = lab.run(r1, "show glbp");
    assert!(g.contains("Group 1 on GigabitEthernet0/0: active gateway (priority 110)") && g.contains("load balancing round-robin"), "{g}");
    assert!(
        g.contains("Forwarder 1: 0007.b400.0101 on 192.168.1.1 (this router)") && g.contains("Forwarder 2: 0007.b400.0102 on 192.168.1.2"),
        "{g}"
    );
    assert!(lab.run(r2, "show glbp brief").contains("standby   2"));
    // The active gateway answers each host with the next forwarder: the hosts split.
    lab.run(pc1, "ping 10.0.0.10");
    lab.run(pc2, "ping 10.0.0.10");
    let a1 = lab.run(pc1, "arp -a");
    let a2 = lab.run(pc2, "arp -a");
    assert!(a1.contains("00:07:b4:00:01:01"), "{a1}");
    assert!(a2.contains("00:07:b4:00:01:02"), "{a2}");
    lab.sim.clear_trace();
    let p = lab.run(pc2, "ping 10.0.0.10");
    let ev: Vec<String> = lab.sim.trace().iter().map(|e| format!("{:?}->{:?} {} {:?}", e.from.device, e.to.device, e.summary, e.dropped)).collect();
    assert!(p.contains("4 answered"), "{p}\n{ev:#?}");
    // PC2's requests went through R2, its forwarder.
    assert!(lab.sim.trace().iter().any(|e| e.to.device == r2 && e.summary.starts_with("ICMP echo request") && e.dropped.is_none()));
    assert!(lab.run(pc1, "ping 10.0.0.10").contains("4 answered"));
    // Host-dependent: a host keeps its forwarder.
    lab.run(r1, "conf t\nint g0/0\nglbp 1 load-balancing host-dependent\nglbp 1 preempt\nend");
    let run = lab.run(r1, "show running-config interface g0/0");
    assert!(run.contains(" glbp 1 ip 192.168.1.254\n glbp 1 priority 110\n glbp 1 preempt\n glbp 1 load-balancing host-dependent"), "{run}");
    lab.run(r1, "conf t\nint g0/0");
    assert!(lab.try_run(r1, "glbp 1 load-balancing weighted").contains("round-robin"));
    assert!(lab.try_run(r1, "glbp 2000 ip 192.168.1.253").contains("0-1023"));
    lab.run(r1, "no glbp 1\nend");
    assert!(lab.run(r1, "show glbp").contains("No GLBP group"));
}

#[test]
fn ospf_network_type_mismatch_forms_neighbours_but_routes_nothing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(r1, "en\nconf t\nint g0/1\nip add 10.0.0.1 255.255.255.0\nno shut\nint lo0\nip add 192.168.1.1 255.255.255.0\nexit\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend");
    lab.run(r2, "en\nconf t\nint g0/1\nip add 10.0.0.2 255.255.255.0\nno shut\nint lo0\nip add 192.168.2.1 255.255.255.0\nexit\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("192.168.2.1/32"), "{rt}\n{}", lab.run(r1, "show ip ospf neighbor"));
    // Point-to-point at one end only: the hellos still agree (RFC 2328 §10.5), so they are
    // neighbours, but the link is described two ways and SPF routes nothing across it.
    lab.run(r1, "conf t\nint g0/1\nip ospf network point-to-point\nend");
    let nb = lab.run(r1, "show ip ospf neighbor");
    assert!(nb.contains("10.0.0.2") && nb.contains("network type differs"), "{nb}");
    assert!(!lab.run(r1, "show ip route").contains("192.168.2.1/32"));
    assert!(!lab.run(r2, "show ip route").contains("192.168.1.1/32"));
    // Both ends alike again: routes come back.
    lab.run(r2, "conf t\nint g0/1\nip ospf network point-to-point\nend");
    assert!(lab.run(r1, "show ip route").contains("192.168.2.1/32"));
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("network type differs"));
}

/// R1 and R3 on one switch (2001:db8:13::/64), R2 as the way round (R1–R2 and R2–R3 by cable);
/// R3 has a loopback prefix. EIGRP for IPv6 AS 10 everywhere, router ids by hand.
fn eigrp6_switched() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    let sw = lab.add(DeviceKind::Switch);
    lab.cable(r1, "g0/1", sw, "fa0/1");
    lab.cable(r3, "g0/1", sw, "fa0/2");
    lab.cable(r1, "g0/2", r2, "g0/1");
    lab.cable(r2, "g0/2", r3, "g0/2");
    let cfg = |n: u8, links: &[(&str, &str)], extra: &str| {
        let mut s = format!("en\nconf t\nipv6 unicast-routing\nipv6 router eigrp 10\neigrp router-id {n}.{n}.{n}.{n}\nexit\n");
        for (ifc, addr) in links {
            s.push_str(&format!("int {ifc}\nipv6 address {addr}\nipv6 eigrp 10\nno shut\n"));
        }
        s.push_str(extra);
        s.push_str("end");
        s
    };
    lab.run(r1, &cfg(1, &[("g0/1", "2001:db8:13::1/64"), ("g0/2", "2001:db8:12::1/64")], ""));
    lab.run(r2, &cfg(2, &[("g0/1", "2001:db8:12::2/64"), ("g0/2", "2001:db8:23::2/64")], ""));
    lab.run(r3, &cfg(3, &[("g0/1", "2001:db8:13::3/64"), ("g0/2", "2001:db8:23::3/64")], "int lo0\nipv6 address 2001:db8:3::1/64\nipv6 eigrp 10\n"));
    lab.run(sw, "en");
    (lab, r1, r2, r3, sw)
}

#[test]
fn a_silent_eigrp6_neighbour_is_dropped_after_its_hold_time() {
    let (mut lab, r1, _r2, r3, sw) = eigrp6_switched();
    // The defaults first: hellos every 5 s, a 15 s hold time.
    let ifs = lab.run(r3, "show ipv6 eigrp interfaces");
    assert!(ifs.lines().any(|l| l.starts_with("Gi0/1") && l.contains(" 5 s ") && l.contains(" 15 s ")), "{ifs}");
    // R3 announces a 30-second hold time and says hello every 10 seconds.
    lab.run(r3, "conf t\nint g0/1\nipv6 hello-interval eigrp 10 10\nipv6 hold-time eigrp 10 30\nend");
    let ifs = lab.run(r3, "show ipv6 eigrp interfaces");
    assert!(ifs.lines().any(|l| l.starts_with("Gi0/1") && l.contains(" 10 s ") && l.contains(" 30 s ")), "{ifs}");
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 21_000_000);
    let summaries: Vec<String> = lab.sim.trace().iter().map(|e| e.summary.clone()).collect();
    // Each hello shows twice (leaving R3, leaving the switch towards R1): two hellos in 21 s,
    // not the four of the default interval, announcing the longer hold time.
    let from_gi01: Vec<&String> = summaries.iter().filter(|s| s.contains("EIGRPv6 hello") && s.contains("fe80::ff:fe03:1 ")).collect();
    assert_eq!(from_gi01.len(), 4, "{summaries:?}");
    assert!(from_gi01.iter().all(|s| s.contains("(hold 30 s)")), "{from_gi01:?}");
    assert!(summaries.iter().any(|s| s.contains("fe80::ff:fe03:2 ") && s.contains("(hold 15 s)")), "Gi0/2 keeps the defaults");
    let nb = lab.run(r1, "show ipv6 eigrp neighbors");
    assert!(nb.lines().any(|l| l.contains("Gi0/1") && l.contains(" 30 ")), "R1 knows R3's hold time: {nb}");
    assert_eq!(lab.run(r1, "show ipv6 route eigrp").matches("2001:db8:3::/64").count(), 1);
    lab.run(r1, "show logging");
    lab.sim.take_console(r1);
    // The switch port towards R3 goes down: R1's own link stays up, so it only notices that
    // the hellos stopped, and keeps the route meanwhile.
    lab.run(sw, "conf t\nint fa0/2\nshutdown\nend");
    let nb = lab.run(r1, "show ipv6 eigrp neighbors");
    assert!(nb.contains("no longer heard"), "{nb}");
    lab.sim.run_for(&lab.t, 15_000_000);
    assert!(lab.run(r1, "show ipv6 eigrp neighbors").contains("no longer heard"), "still within the hold time");
    assert!(lab.run(r1, "show ipv6 route").contains("2001:db8:3::/64"));
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 16_000_000);
    let nb = lab.run(r1, "show ipv6 eigrp neighbors");
    assert!(!nb.contains("no longer heard") && !nb.contains("Gi0/1"), "{nb}");
    let log = lab.run(r1, "show logging");
    assert!(log.contains("EIGRP-IPv6 AS 10: neighbour fe80::") && log.contains("is down: hold time expired"), "{log}");
    // Reconverged the long way round, and R1 told R2.
    let rt = lab.run(r1, "show ipv6 route eigrp");
    assert_eq!(rt.matches("2001:db8:3::/64").count(), 1, "{rt}");
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("EIGRPv6 update") && e.summary.contains("ff02::a")), "an update after the drop");
    // The neighbour comes back as soon as it is heard again.
    lab.run(sw, "conf t\nint fa0/2\nno shutdown\nend");
    assert!(lab.run(r1, "show ipv6 eigrp neighbors").contains("Gi0/1"));
    assert!(lab.run(r1, "show logging").contains("is up: new adjacency"));
    // The timers are part of the interface's configuration.
    let run = lab.run(r3, "show running-config");
    assert!(run.contains(" ipv6 hello-interval eigrp 10 10\n ipv6 hold-time eigrp 10 30\n"), "{run}");
    lab.run(r3, "copy run start\nreload\nen");
    assert_eq!(lab.run(r3, "show running-config"), run);
    // They are the IPv6 process's: the IPv4 timers don't change them, and bad values are refused.
    lab.run(r3, "conf t\nint g0/1");
    assert!(lab.try_run(r3, "ipv6 hold-time eigrp 10 0").starts_with("% "));
    assert!(lab.try_run(r3, "ipv6 hello-interval eigrp 0 5").starts_with("% "));
    lab.run(r3, "ip hello-interval eigrp 10 2\nend");
    let ifs = lab.run(r3, "show ipv6 eigrp interfaces");
    assert!(ifs.lines().any(|l| l.starts_with("Gi0/1") && l.contains(" 10 s ")), "{ifs}");
    lab.run(r3, "conf t\nint g0/1\nno ip hello-interval eigrp 10\nno ipv6 hold-time eigrp 10\nno ipv6 hello-interval eigrp 10\nend");
    assert!(!lab.run(r3, "show running-config").contains("eigrp 10 "));
}
