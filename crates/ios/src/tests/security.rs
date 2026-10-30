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

// ---- IP MTU and TCP MSS clamping ----

/// The IPv4 flags/offset word and total length of each captured IPv4 frame from `from` that
/// matches `pick`.
fn ipv4_frames(lab: &Lab, from: DeviceId) -> Vec<Vec<u8>> {
    lab.sim
        .trace()
        .iter()
        .filter(|e| e.from.device == from)
        .filter_map(|e| {
            let f = &e.frame;
            let at = if f.get(12..14) == Some(&[0x81, 0x00]) { 16 } else { 12 };
            (f.get(at..at + 2) == Some(&[0x08, 0x00])).then(|| f.get(at + 2..).unwrap_or_default().to_vec())
        })
        .collect()
}

fn is_fragment(d: &[u8]) -> bool {
    u16::from_be_bytes([d[6], d[7]]) & 0x3fff != 0
}

/// (protocol, IPv4 payload) of a datagram.
fn l4(d: &[u8]) -> (u8, &[u8]) {
    let ihl = usize::from(d[0] & 0x0f) * 4;
    let total = usize::from(u16::from_be_bytes([d[2], d[3]]));
    (d[9], &d[ihl..total])
}

/// The MSS option of a TCP SYN.
fn syn_mss(seg: &[u8]) -> Option<u16> {
    if seg[13] & 0x02 == 0 {
        return None;
    }
    let off = usize::from(seg[12] >> 4) * 4;
    let opts = &seg[20..off];
    (opts.first() == Some(&2)).then(|| u16::from_be_bytes([opts[2], opts[3]]))
}

/// The TCP checksum over the pseudo-header is right.
fn tcp_ok(d: &[u8]) -> bool {
    let (_, seg) = l4(d);
    let mut sum: u32 = 0;
    let mut add = |b: &[u8]| {
        for c in b.chunks(2) {
            sum += u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]));
        }
    };
    add(&d[12..20]);
    add(&[0, 6]);
    add(&(seg.len() as u16).to_be_bytes());
    add(seg);
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    sum == 0xffff
}

#[test]
fn ip_mtu_fragments_and_honours_df_by_typing() {
    let (mut lab, r1, a, srv, _c) = three_legs();
    assert!(ping_settled(&mut lab, a, "203.0.113.10").contains("4 answered"));
    lab.run(r1, "conf t\nint g0/1\nip mtu 576\nend");
    assert!(
        lab.run(r1, "show running-config").contains("interface GigabitEthernet0/1\n") && lab.run(r1, "show running-config").contains(" ip mtu 576\n")
    );
    // 1400 data bytes: three fragments on the 576-byte link, put back together by the server.
    lab.sim.clear_trace();
    let ping = lab.run(a, "ping -n 2 -l 1400 203.0.113.10");
    assert!(ping.contains("2 answered"), "{ping}");
    let frags: Vec<Vec<u8>> = ipv4_frames(&lab, r1).into_iter().filter(|d| is_fragment(d)).collect();
    assert!(frags.len() >= 6, "{} fragments", frags.len());
    assert!(frags.iter().all(|f| f.len() <= 576));
    // "Don't fragment": the router answers "fragmentation needed" with its MTU (RFC 1191).
    lab.sim.clear_trace();
    let ping = lab.run(a, "ping -n 1 -f -l 1400 203.0.113.10");
    assert!(ping.contains("needs fragmenting") && ping.contains("0 answered"), "{ping}");
    let needed = ipv4_frames(&lab, r1).into_iter().find_map(|d| {
        let (proto, icmp) = l4(&d);
        (proto == 1 && icmp.first() == Some(&3) && icmp.get(1) == Some(&4)).then(|| icmp.to_vec())
    });
    let needed = needed.expect("a fragmentation-needed message");
    assert_eq!(u16::from_be_bytes([needed[6], needed[7]]), 576);
    // A small packet with DF still fits.
    assert!(lab.run(a, "ping -n 1 -f -l 100 203.0.113.10").contains("1 answered"));
    // The router's own extended ping: `M` when DF is set and the packet doesn't fit.
    let p = lab.run(r1, "ping 203.0.113.10 size 1200 repeat 2 df-bit");
    assert!(p.contains("MM"), "{p}");
    let p = lab.run(r1, "ping 203.0.113.10 size 1200 repeat 2");
    assert!(p.contains("!!"), "{p}");
    lab.run(r1, "conf t\nint g0/1\nno ip mtu\nend");
    assert!(lab.run(a, "ping -n 1 -f -l 1400 203.0.113.10").contains("1 answered"));
    lab.run(r1, "conf t\nint g0/1");
    assert!(lab.try_run(r1, "ip mtu 20").contains("68-9216"));
    let _ = srv;
}

#[test]
fn tcp_adjust_mss_rewrites_syns_by_typing() {
    let (mut lab, r1, a, _srv, _c) = three_legs();
    lab.run(r1, "conf t\nint g0/0\nip tcp adjust-mss 1360\nend");
    lab.sim.clear_trace();
    let (status, _) = lab.sim.http_get(&lab.t, a, "203.0.113.10".parse().unwrap(), "203.0.113.10", "/").unwrap();
    assert!(status.contains("200"), "{status}");
    let mss_of = |frames: Vec<Vec<u8>>| -> Vec<u16> {
        frames
            .into_iter()
            .filter_map(|d| {
                let (proto, seg) = l4(&d);
                if proto == 6 { syn_mss(seg) } else { None }
            })
            .collect()
    };
    // The PC's SYN says 1460; after the router it says 1360, checksum still right.
    assert!(mss_of(ipv4_frames(&lab, a)).contains(&1460));
    let after = mss_of(ipv4_frames(&lab, r1));
    assert!(after.iter().all(|m| *m == 1360) && !after.is_empty(), "{after:?}");
    for d in ipv4_frames(&lab, r1) {
        if l4(&d).0 == 6 {
            assert!(tcp_ok(&d));
        }
    }
    assert!(lab.run(r1, "show running-config").contains(" ip tcp adjust-mss 1360\n"));
    lab.run(r1, "conf t\nint g0/0");
    assert!(lab.try_run(r1, "ip tcp adjust-mss 100").contains("500-1460"));
    lab.run(r1, "no ip tcp adjust-mss\nend");
    assert!(lab.t.device(r1).unwrap().security.adjust_mss.is_empty());
}

#[test]
fn ip_mtu_and_mss_on_a_tunnel_by_typing() {
    let (mut lab, r1, _isp, r2, a) = super::two_sites();
    lab.run(
        r1,
        "conf t\ninterface tunnel 0\nip address 10.0.0.1 255.255.255.252\ntunnel source g0/1\ntunnel destination 198.51.100.1\nip mtu 1000\nip tcp adjust-mss 960\nexit\nip route 192.168.2.0 255.255.255.0 10.0.0.2\nend",
    );
    lab.run(r2, "conf t\nint tu0\nip address 10.0.0.2 255.255.255.252\ntunnel source g0/1\ntunnel destination 203.0.113.1\nexit\nip route 192.168.1.0 255.255.255.0 tunnel 0\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    lab.sim.clear_trace();
    let ping = lab.run(a, "ping -n 1 -l 1400 192.168.2.10");
    assert!(ping.contains("1 answered"), "{ping}");
    // The inner packet was fragmented before it went into GRE.
    let gre_frags = lab.sim.trace().iter().filter(|e| e.from.device == r1 && e.summary.starts_with("GRE")).count();
    assert!(gre_frags >= 2, "{gre_frags}");
    assert!(lab.run(a, "ping -n 1 -f -l 1400 192.168.2.10").contains("needs fragmenting"));
    let run = lab.run(r1, "show running-config interface tunnel 0");
    assert!(run.contains(" ip mtu 1000") && run.contains(" ip tcp adjust-mss 960"), "{run}");
}

// ---- IKEv2 ----

fn ikev2_config(acl: &str, peer: &str, key: &str) -> String {
    format!(
        "conf t
access-list 110 permit ip {acl}
crypto ikev2 proposal PROP
encryption aes-cbc-256 aes-cbc-128
integrity sha256 sha1
group 14
exit
crypto ikev2 policy POL
proposal PROP
exit
crypto ikev2 keyring KR
peer SITE
address {peer}
pre-shared-key {key}
exit
exit
crypto ikev2 profile PROF
match identity remote address {peer} 255.255.255.255
authentication remote pre-share
authentication local pre-share
keyring local KR
exit
crypto ipsec transform-set TS esp-aes esp-sha-hmac
exit
crypto map VPN 10 ipsec-isakmp
set peer {peer}
set transform-set TS
set ikev2-profile PROF
match address 110
exit
int g0/1
crypto map VPN
end"
    )
}

#[test]
fn ikev2_site_to_site_by_typing() {
    let (mut lab, r1, isp, r2, a) = super::two_sites();
    lab.run(r1, &ikev2_config("192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255", "198.51.100.1", "v2secret"));
    lab.run(r2, &ikev2_config("192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255", "203.0.113.1", "v2secret"));
    lab.sim.clear_trace();
    let ping = ping_settled(&mut lab, a, "192.168.2.10");
    assert!(ping.contains("4 answered"), "{ping}");
    // On the wire: IKE_SA_INIT in the clear, IKE_AUTH inside an SK payload, then ESP.
    let ike: Vec<String> = lab.sim.trace().iter().filter(|e| e.summary.contains("IKEv2")).map(|e| e.summary.clone()).collect();
    assert!(ike.iter().any(|s| s.contains("IKEv2 IKE_SA_INIT request #0 [SA, KE, Nonce]")), "{ike:#?}");
    assert!(ike.iter().any(|s| s.contains("IKEv2 IKE_SA_INIT response #0 [SA, KE, Nonce]")), "{ike:#?}");
    assert!(ike.iter().any(|s| s.contains("IKEv2 IKE_AUTH request #1 [SK]")), "{ike:#?}");
    assert!(ike.iter().any(|s| s.contains("IKEv2 IKE_AUTH response #1 [SK]")), "{ike:#?}");
    assert!(lab.sim.trace().iter().filter(|e| e.to.device == isp).all(|e| !(e.summary.starts_with("ICMP") && e.summary.contains("192.168."))));
    let sa = lab.run(r1, "show crypto ikev2 sa");
    assert!(sa.contains("Peer 198.51.100.1 (local 203.0.113.1), this router started it, established"), "{sa}");
    assert!(sa.contains("AES-CBC-256") && sa.contains("PRF HMAC-SHA256") && sa.contains("DH group 14") && sa.contains("Profile: PROF"), "{sa}");
    assert!(lab.run(r2, "show crypto ikev2 sa").contains("the peer started it, established"));
    assert!(sa.contains("ESP: AES-128-CBC with HMAC-SHA1-96 (real ciphers)"), "{sa}");
    let ipsec = lab.run(r1, "show crypto ipsec sa");
    assert!(ipsec.contains("Outbound ESP SPI 0x") && count_after(&ipsec, "decapsulated ") >= 3, "{ipsec}");
    assert!(lab.run(r1, "show logging").contains("IKEv2 with 198.51.100.1: IKE_SA_INIT and IKE_AUTH complete"));
    // Reports and the configuration round trip.
    assert!(
        lab.run(r1, "show crypto ikev2 proposal").contains("Proposal PROP: encryption aes-cbc-256 aes-cbc-128; integrity sha256 sha1; DH groups 14")
    );
    assert!(lab.run(r1, "show crypto ikev2 profile").contains("Keyring: KR"));
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("crypto ikev2 keyring KR\n peer SITE\n  address 198.51.100.1\n  pre-shared-key v2secret\n!"), "{run}");
    assert!(
        run.contains("crypto map VPN 10 ipsec-isakmp\n set peer 198.51.100.1\n set transform-set TS\n match address 110\n set ikev2-profile PROF\n!"),
        "{run}"
    );
    let r3 = lab.add(DeviceKind::Router);
    lab.run(r3, "en\nconf t");
    for l in run.lines().filter(|l| !l.starts_with('!') && !l.starts_with("hostname")) {
        lab.try_run(r3, l);
    }
    assert_eq!(lab.t.device(r3).unwrap().crypto, lab.t.device(r1).unwrap().crypto);
}

fn count_after(text: &str, what: &str) -> u64 {
    text.split(what).nth(1).and_then(|t| t.trim().split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse().ok()).unwrap_or(0)
}

#[test]
fn ikev2_mismatches_are_refused_on_the_wire() {
    // Different keys: the responder's AUTH check fails.
    let (mut lab, r1, _isp, r2, a) = super::two_sites();
    lab.run(r1, &ikev2_config("192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255", "198.51.100.1", "one"));
    lab.run(r2, &ikev2_config("192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255", "203.0.113.1", "two"));
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r1, "show crypto ikev2 sa");
    assert!(sa.contains("AUTHENTICATION_FAILED"), "{sa}");
    assert!(lab.run(r2, "show crypto ikev2 sa").contains("does not match this router's pre-shared key"));
    // No proposal in common: refused at IKE_SA_INIT.
    let (mut lab, r1, _isp, r2, a) = super::two_sites();
    lab.run(r1, &ikev2_config("192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255", "198.51.100.1", "k"));
    lab.run(r2, &ikev2_config("192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255", "203.0.113.1", "k"));
    lab.run(r2, "conf t\ncrypto ikev2 proposal PROP\ngroup 19\nend");
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("IKEv2 IKE_SA_INIT response #0 [Notify]")));
    assert!(lab.run(r1, "show crypto ikev2 sa").contains("NO_PROPOSAL_CHOSEN"));
    // Traffic selectors the peer doesn't protect.
    let (mut lab, r1, _isp, r2, a) = super::two_sites();
    lab.run(r1, &ikev2_config("192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255", "198.51.100.1", "k"));
    lab.run(r2, &ikev2_config("10.9.9.0 0.0.0.255 10.8.8.0 0.0.0.255", "203.0.113.1", "k"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.run(r1, "show crypto ikev2 sa").contains("TS_UNACCEPTABLE"));
    let _ = r2;
    // Transforms without a real implementation fall back to the stand-in cipher, both ends alike.
    let (mut lab2, s1, _isp, s2, pc) = super::two_sites();
    for (r, cfg) in [
        (s1, ikev2_config("192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255", "198.51.100.1", "k")),
        (s2, ikev2_config("192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255", "203.0.113.1", "k")),
    ] {
        lab2.run(r, &cfg);
        lab2.run(r, "conf t\ncrypto ipsec transform-set TS esp-3des esp-md5-hmac\nend");
    }
    assert!(ping_settled(&mut lab2, pc, "192.168.2.10").contains("4 answered"));
    assert!(lab2.run(s1, "show crypto ikev2 sa").contains("stand-in cipher"));
    // Configuration mistakes explain themselves.
    lab.run(r1, "conf t\ncrypto ikev2 proposal P2");
    assert!(lab.try_run(r1, "encryption rot13").contains("not a cipher"));
    assert!(lab.try_run(r1, "group 99").contains("not a group"));
    lab.run(r1, "exit\ncrypto ikev2 profile P2");
    assert!(lab.try_run(r1, "authentication local rsa-sig").contains("pre-share"));
    lab.run(r1, "end");
    assert_eq!(lab.prompt(r1), "R1#");
}
