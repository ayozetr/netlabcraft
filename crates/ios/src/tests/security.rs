//! Typed console tests for the zone-based policy firewall and classic inspection.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::{Lab, ping_settled};

/// PC A (192.168.1.10) on R1 g0/0, a web server (203.0.113.10) on g0/1, PC C (172.16.0.10) on
/// g0/2. Returns (lab, R1, A, server, C).
fn three_legs() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    let c = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(srv, "g0", r1, "g0/1");
    lab.cable(c, "fa0", r1, "g0/2");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 203.0.113.10 255.255.255.0 203.0.113.1");
    lab.run(c, "ipconfig 172.16.0.10 255.255.255.0 172.16.0.1");
    lab.run(
        r1,
        "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip add 203.0.113.1 255.255.255.0\nno shut\nint g0/2\nip add 172.16.0.1 255.255.255.0\nno shut\nend",
    );
    (lab, r1, a, srv, c)
}

const ZONES: &str = "conf t
zone security INSIDE
description the office LAN
exit
zone security OUTSIDE
exit
class-map type inspect match-any IN-TRAFFIC
match protocol tcp
match protocol icmp
exit
policy-map type inspect IN-TO-OUT
class type inspect IN-TRAFFIC
inspect
class class-default
drop log
exit
exit
zone-pair security IN-OUT source INSIDE destination OUTSIDE
service-policy type inspect IN-TO-OUT
exit
int g0/0
zone-member security INSIDE
int g0/1
zone-member security OUTSIDE
end";

fn dropped_with(lab: &Lab, what: &str) -> bool {
    lab.sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.contains(what)))
}

#[test]
fn zone_firewall_by_typing() {
    let (mut lab, r1, a, srv, c) = three_legs();
    assert!(ping_settled(&mut lab, srv, "192.168.1.10").contains("4 answered"));
    lab.run(r1, ZONES);
    assert_eq!(lab.prompt(r1), "R1#");
    // Inside to outside: inspected, and the replies come back.
    let ping = ping_settled(&mut lab, a, "203.0.113.10");
    assert!(ping.contains("4 answered"), "{ping}");
    let (status, _) = lab.sim.http_get(&lab.t, a, "203.0.113.10".parse().unwrap(), "203.0.113.10", "/").unwrap();
    assert!(status.contains("200"), "{status}");
    let sessions = lab.run(r1, "show policy-map type inspect zone-pair sessions");
    assert!(sessions.contains("Zone pair IN-OUT (INSIDE → OUTSIDE)"), "{sessions}");
    assert!(sessions.contains("icmp  192.168.1.10") && sessions.contains("203.0.113.10:80") && sessions.contains("class IN-TRAFFIC"), "{sessions}");
    // Outside to inside: no zone pair that way, dropped.
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, srv, "192.168.1.10").contains(" 0 answered"));
    assert!(dropped_with(&lab, "no zone pair from OUTSIDE to INSIDE"));
    // A zone with no pair to it: dropped too.
    lab.run(r1, "conf t\nzone security DMZ\nexit\nint g0/2\nzone-member security DMZ\nend");
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "172.16.0.10").contains(" 0 answered"));
    assert!(dropped_with(&lab, "no zone pair from INSIDE to DMZ"));
    // An interface in no zone can't talk to one in a zone.
    lab.run(r1, "conf t\nint g0/2\nno zone-member security\nend");
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "172.16.0.10").contains(" 0 answered"));
    assert!(dropped_with(&lab, "is in no zone"));
    let _ = c;
    let report = lab.run(r1, "show policy-map type inspect zone-pair");
    assert!(report.contains("Class IN-TRAFFIC (match-any: protocol tcp, protocol icmp): inspect"), "{report}");
    assert!(report.contains("Class class-default: drop and log"), "{report}");
    // The self zone: the router answers from every zone until a pair says otherwise.
    assert!(ping_settled(&mut lab, srv, "203.0.113.1").contains("4 answered"));
    lab.run(
        r1,
        "conf t\nclass-map type inspect match-all ADMIN\nmatch protocol tcp\nexit\npolicy-map type inspect TO-ROUTER\nclass type inspect ADMIN\npass\nexit\nclass class-default\ndrop log\nexit\nexit\nzone-pair security OUT-SELF source OUTSIDE destination self\nservice-policy type inspect TO-ROUTER\nend",
    );
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, srv, "203.0.113.1").contains(" 0 answered"));
    assert!(dropped_with(&lab, "dropped by class class-default of zone pair OUT-SELF"));
    assert!(lab.run(r1, "show logging").contains("Zone firewall: icmp 203.0.113.10"));
    assert!(ping_settled(&mut lab, a, "192.168.1.1").contains("4 answered"));
    // Reports.
    let z = lab.run(r1, "show zone security");
    assert!(z.contains("Zone self") && z.contains("Zone INSIDE\n  Description: the office LAN\n  Members: GigabitEthernet0/0"), "{z}");
    assert!(lab.run(r1, "show zone security OUTSIDE").contains("Members: GigabitEthernet0/1"));
    let zp = lab.run(r1, "show zone-pair security");
    assert!(
        zp.contains("Zone pair IN-OUT: from INSIDE to OUTSIDE\n  Inspect policy: IN-TO-OUT") && zp.contains("OUT-SELF: from OUTSIDE to self"),
        "{zp}"
    );
    assert!(lab.run(r1, "show class-map type inspect").contains("Inspect class ADMIN (every line must match)"));
    assert!(lab.run(r1, "show policy-map type inspect").contains("Class ADMIN: pass"));
    lab.run(r1, "clear zone-pair inspect sessions");
    assert!(lab.run(r1, "show policy-map type inspect zone-pair sessions").contains("No sessions"));
    // The configuration replays onto a blank router.
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("class-map type inspect match-any IN-TRAFFIC\n match protocol tcp\n match protocol icmp\n!"), "{run}");
    assert!(
        run.contains("policy-map type inspect IN-TO-OUT\n class type inspect IN-TRAFFIC\n  inspect\n class class-default\n  drop log\n!"),
        "{run}"
    );
    assert!(run.contains("zone-pair security IN-OUT source INSIDE destination OUTSIDE\n service-policy type inspect IN-TO-OUT\n!"), "{run}");
    assert!(run.contains("interface GigabitEthernet0/0\n") && run.contains(" zone-member security INSIDE\n"), "{run}");
    let r2 = lab.add(DeviceKind::Router);
    lab.run(r2, "en\nconf t");
    for l in run.lines().filter(|l| !l.starts_with('!') && !l.starts_with("hostname")) {
        lab.try_run(r2, l);
    }
    assert_eq!(lab.t.device(r2).unwrap().security, lab.t.device(r1).unwrap().security);
    // Mistakes explain themselves.
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "zone security self").contains("router's own zone"));
    assert!(lab.try_run(r1, "no zone security INSIDE").contains("member interfaces"));
    assert!(lab.try_run(r1, "zone-pair security X source INSIDE destination NOWHERE").contains("no zone NOWHERE"));
    assert!(lab.try_run(r1, "no policy-map type inspect IN-TO-OUT").contains("used by zone pair IN-OUT"));
    lab.run(r1, "class-map type inspect C2");
    assert!(lab.try_run(r1, "match protocol gopher").contains("Unknown protocol"));
    assert!(lab.try_run(r1, "match access-group 9999").contains("1-2699"));
    lab.run(r1, "match access-group name WEB\nmatch access-group 101\nno match access-group 101");
    lab.run(r1, "exit\npolicy-map type inspect P2");
    assert!(lab.try_run(r1, "class type inspect NOPE").contains("no inspect class NOPE"));
    lab.run(r1, "class class-default");
    assert!(lab.try_run(r1, "inspect").contains("class-default can only pass or drop"));
    lab.run(r1, "end");
    let sw = lab.add(DeviceKind::Switch);
    lab.run(sw, "en\nconf t");
    assert!(lab.try_run(sw, "zone security X").contains('%'));
}

#[test]
fn zone_firewall_only_starts_tcp_with_a_syn_and_sessions_expire() {
    let (mut lab, r1, a, srv, _c) = three_legs();
    lab.run(r1, ZONES);
    assert!(ping_settled(&mut lab, a, "203.0.113.10").contains("4 answered"));
    assert!(lab.run(r1, "show policy-map type inspect zone-pair sessions").contains("icmp"));
    // ICMP sessions are forgotten after 10 idle seconds.
    lab.sim.run_for(&lab.t, 12_000_000);
    assert!(lab.run(r1, "show policy-map type inspect zone-pair sessions").contains("No sessions"));
    // With the policy gone, inside to outside is dropped as well.
    lab.run(r1, "conf t\nzone-pair security IN-OUT source INSIDE destination OUTSIDE\nno service-policy type inspect\nend");
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "203.0.113.10").contains(" 0 answered"));
    assert!(dropped_with(&lab, "zone pair IN-OUT has no inspect policy"));
    let _ = srv;
}

#[test]
fn classic_inspection_by_typing() {
    let (mut lab, r1, a, srv, _c) = three_legs();
    // The outside access list lets nothing in: replies to the inside are blocked too.
    lab.run(r1, "conf t\naccess-list 101 deny ip any any\nint g0/1\nip access-group 101 in\nend");
    assert!(ping_settled(&mut lab, a, "203.0.113.10").contains(" 0 answered"));
    // Inspection opens the way back for the sessions the inside starts.
    lab.run(r1, "conf t\nip inspect name FW icmp\nip inspect name FW tcp\nint g0/1\nip inspect FW out\nend");
    let ping = ping_settled(&mut lab, a, "203.0.113.10");
    assert!(ping.contains("4 answered"), "{ping}");
    let (status, _) = lab.sim.http_get(&lab.t, a, "203.0.113.10".parse().unwrap(), "203.0.113.10", "/").unwrap();
    assert!(status.contains("200"), "{status}");
    let s = lab.run(r1, "show ip inspect sessions");
    assert!(s.contains("icmp  192.168.1.10") && s.contains("rule FW on GigabitEthernet0/1") && s.contains("203.0.113.10:80"), "{s}");
    // The outside still can't start anything.
    assert!(ping_settled(&mut lab, srv, "192.168.1.10").contains(" 0 answered"));
    let all = lab.run(r1, "show ip inspect all");
    assert!(all.contains("Inspection rule FW: icmp, tcp") && all.contains("GigabitEthernet0/1: FW outbound"), "{all}");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("ip inspect name FW icmp\nip inspect name FW tcp\n") && run.contains(" ip inspect FW out\n"), "{run}");
    lab.run(r1, "clear ip inspect sessions\nconf t\nno ip inspect name FW tcp\nint g0/1\nno ip inspect FW out\nend");
    assert!(lab.t.device(r1).unwrap().security.inspect_applied.is_empty());
    assert!(ping_settled(&mut lab, a, "203.0.113.10").contains(" 0 answered"));
    lab.run(r1, "conf t\nno ip inspect name FW\nend");
    assert!(lab.t.device(r1).unwrap().security.inspect_rules.is_empty());
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "ip inspect name FW gopher").contains("Unknown protocol"));
    lab.run(r1, "int g0/1");
    assert!(lab.try_run(r1, "ip inspect FW sideways").contains("in or out"));
}
