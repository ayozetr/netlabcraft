//! Error, drop and keepalive lines of `show interfaces`, the counter tables, BPDU and
//! topology-change counts in the spanning-tree views, and `show controllers` (console depth
//! round of 2026-11-16).

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// The number before `what` in the first line that holds `what` ("3 input drops" → 3).
fn before(out: &str, what: &str) -> u64 {
    let line = out.lines().find(|l| l.contains(what)).unwrap_or("");
    let head = line.split(what).next().unwrap_or("");
    head.split(|c: char| !c.is_ascii_digit()).rfind(|w| !w.is_empty()).and_then(|w| w.parse().ok()).unwrap_or(0)
}

/// A PC behind a router whose inbound access list drops every ICMP packet.
fn filtered() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.run(r, "en\nconf t\naccess-list 101 deny icmp any any\naccess-list 101 permit ip any any\ninterface g0/0\nip address 192.168.1.1 255.255.255.0\nip access-group 101 in\nno shutdown\nend");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    (lab, r, a)
}

#[test]
fn an_inbound_access_list_counts_input_drops_and_clear_counters_zeroes_them() {
    let (mut lab, r, a) = filtered();
    let quiet = lab.run(r, "show interfaces g0/0");
    assert!(quiet.contains("0 input errors, 0 input drops, 0 output drops"), "{quiet}");
    lab.run(a, "ping 192.168.1.1");
    let after = lab.run(r, "show interfaces g0/0");
    assert!(before(&after, " input drops") >= 1, "{after}");
    assert!(before(&after, " input errors") == 0, "{after}");
    // The same count is in the error table, and `show ip traffic` names the cause.
    let table = lab.run(r, "show interfaces counters errors");
    assert!(table.starts_with("Port") && table.contains("Input drops") && table.contains("Gi0/0"), "{table}");
    assert!(!lab.run(r, "show ip traffic").contains("access list 0"), "{}", lab.run(r, "show ip traffic"));
    lab.run(r, "clear counters");
    let cleared = lab.run(r, "show interfaces g0/0");
    assert!(cleared.contains("0 input errors, 0 input drops, 0 output drops"), "{cleared}");
}

#[test]
fn counter_tables_work_on_routers_and_for_one_port() {
    let (mut lab, r, a) = filtered();
    lab.run(a, "ping 192.168.1.1");
    let all = lab.run(r, "show interfaces counters");
    assert!(all.starts_with("Port") && all.contains("Bytes in") && all.contains("Bytes out"), "{all}");
    assert!(all.contains("Gi0/0") && all.contains("Gi0/1"), "{all}");
    let one = lab.run(r, "show interfaces g0/0 counters");
    assert!(one.contains("Gi0/0") && !one.contains("Gi0/1"), "{one}");
    let one_errors = lab.run(r, "show interfaces gi0/0 counters errors");
    assert!(one_errors.contains("Input errors") && one_errors.contains("Gi0/0") && !one_errors.contains("Gi0/1"), "{one_errors}");
    assert!(lab.try_run(r, "show interfaces bogus counters").starts_with("% "));
}

#[test]
fn keepalive_is_a_setting_shown_by_show_interfaces_and_kept_in_the_configuration() {
    let (mut lab, r, _a) = filtered();
    assert!(lab.run(r, "show interfaces g0/0").contains("Keepalive set (10 sec)"));
    lab.run(r, "conf t\ninterface g0/0\nkeepalive 5\nend");
    assert!(lab.run(r, "show interfaces g0/0").contains("Keepalive set (5 sec)"));
    assert!(lab.run(r, "show running-config").contains(" keepalive 5\n"));
    lab.run(r, "conf t\ninterface g0/0\nno keepalive\nend");
    assert!(lab.run(r, "show interfaces g0/0").contains("Keepalive not set"));
    assert!(lab.run(r, "show running-config").contains(" no keepalive\n"));
    lab.run(r, "conf t\ninterface g0/0\nkeepalive\nend");
    let back = lab.run(r, "show interfaces g0/0");
    assert!(back.contains("Keepalive set (10 sec)") && !lab.run(r, "show running-config").contains("keepalive"), "{back}");
    lab.run(r, "conf t\ninterface g0/0");
    assert!(lab.try_run(r, "keepalive 0").starts_with("% "));
    assert!(lab.try_run(r, "keepalive 40000").starts_with("% "));
}

#[test]
fn the_status_table_has_duplex_and_type_and_can_name_one_port() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s, "fa0/1");
    lab.run(s, "en\nconf t\ninterface fa0/1\ndescription uplink to the PC\nend");
    let all = lab.run(s, "show interfaces status");
    assert!(all.starts_with("Port") && all.contains("Duplex") && all.contains("Speed") && all.contains("Type"), "{all}");
    let row = all.lines().find(|l| l.starts_with("Fa0/1 ")).unwrap_or("");
    assert!(row.contains("connected") && row.contains("full") && row.contains("10/100BaseTX"), "{all}");
    let one = lab.run(s, "show interfaces fa0/1 status");
    assert_eq!(one.lines().count(), 2, "{one}");
    let desc = lab.run(s, "show interfaces fa0/1 description");
    assert_eq!(desc.lines().count(), 2, "{desc}");
    assert!(desc.contains("uplink to the PC"), "{desc}");
}

/// Every `BPDUs: sent N, received M` pair of a view, as (N, M).
fn bpdus(out: &str) -> Vec<(u64, u64)> {
    out.lines()
        .filter_map(|l| l.trim().strip_prefix("BPDUs: sent "))
        .filter_map(|l| {
            let (a, b) = l.split_once(", received ")?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .collect()
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
        lab.run(s, "en\nconf t\ninterface range g0/1 - 2\nswitchport mode trunk\nexit\nvlan 20\nend");
    }
    lab.run(s2, "conf t\nspanning-tree vlan 1 root primary\nend");
    lab.sim.run_for(&lab.t, 60_000_000);
    (lab, s1, s2, s3)
}

#[test]
fn spanning_tree_views_count_the_bpdus_each_port_sent_and_received() {
    let (mut lab, s1, s2, _s3) = ring();
    // The root's ports are designated: they send, and the others send nothing on them.
    let root = bpdus(&lab.run(s2, "show spanning-tree vlan 1 detail"));
    assert!(!root.is_empty() && root.iter().all(|(sent, _)| *sent > 0), "{root:?}");
    let leaf = lab.run(s1, "show spanning-tree vlan 1 detail");
    assert!(bpdus(&leaf).iter().any(|(_, got)| *got > 0), "{leaf}");
    assert!(leaf.contains("Changes:"), "{leaf}");
    // The per-port view has the same numbers, VLAN by VLAN.
    let port = lab.run(s1, "show spanning-tree interface g0/1 detail");
    assert!(bpdus(&port).len() >= 2 && port.contains("Topology changes in this VLAN"), "{port}");
    // `clear spanning-tree counters` starts them again.
    lab.run(s2, "clear spanning-tree counters");
    let cleared = bpdus(&lab.run(s2, "show spanning-tree vlan 1 detail"));
    assert!(!cleared.is_empty() && cleared.iter().all(|c| *c == (0, 0)), "{cleared:?}");
}

#[test]
fn losing_a_link_counts_a_topology_change_where_a_port_started_forwarding() {
    let (mut lab, s1, _s2, s3) = ring();
    lab.run(s1, "conf t\ninterface g0/1\nshutdown\nend");
    lab.sim.run_for(&lab.t, 5_000_000);
    let seen = [s1, s3].iter().any(|s| lab.run(*s, "show spanning-tree vlan 1 detail").contains(" topology changes; the last was "));
    assert!(seen, "{}\n{}", lab.run(s1, "show spanning-tree vlan 1 detail"), lab.run(s3, "show spanning-tree vlan 1 detail"));
}

#[test]
fn show_controllers_names_the_hardware_and_the_cable_end() {
    let mut lab = Lab::new();
    let wan = netlabcraft_model::models::find("Router-WAN").unwrap();
    let r1 = lab.t.add_model(wan, None, 0.0, 0.0).unwrap();
    let r2 = lab.t.add_model(wan, None, 0.0, 0.0).unwrap();
    lab.cable(r1, "s0/0/0", r2, "s0/0/0");
    lab.run(r1, "en\nconf t\nint s0/0/0\nclock rate 64000\nno shut\nend");
    lab.run(r2, "en\nconf t\nint s0/0/0\nno shut\nend");
    let dce = lab.run(r1, "show controllers s0/0/0");
    assert!(dce.contains("DCE end of a serial cable, clock rate 64000 bit/s") && dce.contains("Router-WAN") && dce.contains("Simulated:"), "{dce}");
    assert!(dce.contains("Encapsulation: HDLC") && dce.contains("Line: "), "{dce}");
    let dte = lab.run(r2, "show controllers serial 0/0/0");
    assert!(dte.contains("DTE end of a serial cable, clocked by the other end (64000 bit/s)"), "{dte}");
    let eth = lab.run(r1, "show controllers g0/0");
    assert!(eth.contains("Hardware: ") && eth.contains("Auto-MDIX"), "{eth}");
}
