//! Typed console tests for application inspection (FTP data connections, DNS, HTTP) in the zone
//! firewall and classic inspection, and for the default MTU with path MTU discovery.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_model::services::{DnsRecord, FtpUser, WebFile};
use netlabcraft_sim::ftp::FtpOp;

use super::{Lab, ping_settled, two_sites};

/// PC A (192.168.1.10) on R1 g0/0; a server (203.0.113.10) on g0/1 with FTP, DNS and the web.
fn office() -> (Lab, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(srv, "g0", r1, "g0/1");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 203.0.113.10 255.255.255.0 203.0.113.1");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip add 203.0.113.1 255.255.255.0\nno shut\nend");
    {
        let s = lab.t.device_mut(srv).unwrap();
        s.services.ftp.users.push(FtpUser { name: "ana".into(), password: "pw".into(), read: true, write: true, delete: false, list: true });
        s.services.ftp.files.push(WebFile { name: "big.txt".into(), content: "0123456789".repeat(500) });
        s.services.dns.on = true;
        s.services.dns.records.push(DnsRecord { name: "www.lab.test".into(), address: "203.0.113.10".parse().unwrap() });
    }
    lab.t.device_mut(a).unwrap().dns = Some("203.0.113.10".parse().unwrap());
    assert!(ping_settled(&mut lab, a, "203.0.113.10").contains("4 answered"));
    (lab, r1, a, srv)
}

fn zones(matches: &str) -> String {
    format!(
        "conf t
zone security INSIDE
exit
zone security OUTSIDE
exit
class-map type inspect match-any APPS
{matches}
exit
policy-map type inspect IN-TO-OUT
class type inspect APPS
inspect
class class-default
drop
exit
exit
zone-pair security IN-OUT source INSIDE destination OUTSIDE
service-policy type inspect IN-TO-OUT
exit
int g0/0
zone-member security INSIDE
int g0/1
zone-member security OUTSIDE
end"
    )
}

fn ftp_get(lab: &mut Lab, a: DeviceId) -> Result<(String, Vec<u8>), String> {
    lab.sim.ftp(&lab.t, a, "203.0.113.10".parse().unwrap(), "ana", "pw", &FtpOp::Get("big.txt".into())).map_err(|e| e.to_string())
}

#[test]
fn zone_firewall_opens_the_ftp_data_connection_it_reads_about() {
    let (mut lab, r1, a, _srv) = office();
    // Only FTP's control connection is in the class: the passive data connection gets in
    // because inspection read the server's 227 reply.
    lab.run(r1, &zones("match protocol ftp"));
    let (done, bytes) = ftp_get(&mut lab, a).unwrap();
    assert!(done.starts_with("226"), "{done}");
    assert_eq!(bytes.len(), 5000);
    let sessions = lab.run(r1, "show policy-map type inspect zone-pair sessions");
    assert!(sessions.contains("203.0.113.10:21") && sessions.contains("FTP inspected"), "{sessions}");
    assert!(lab.run(r1, "show logging").contains("FTP announced a data connection from 192.168.1.10 to 203.0.113.10:"));
    // Without FTP inspection (the control port by access list), the data connection is dropped.
    let (mut lab, r1, a, _srv) = office();
    lab.run(r1, "conf t\nip access-list extended FTP-CTL\npermit tcp any any eq 21\nend");
    lab.run(r1, &zones("match access-group name FTP-CTL"));
    assert!(ftp_get(&mut lab, a).is_err());
    assert!(lab.sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.contains("class-default"))));
}

#[test]
fn zone_firewall_inspects_dns_and_http() {
    let (mut lab, r1, a, _srv) = office();
    lab.run(r1, &zones("match protocol dns\nmatch protocol http"));
    let got = lab.sim.dns_resolve(&lab.t, a, "www.lab.test").unwrap();
    assert_eq!(got.to_string(), "203.0.113.10");
    let (status, _) = lab.sim.http_get(&lab.t, a, "203.0.113.10".parse().unwrap(), "www.lab.test", "/").unwrap();
    assert!(status.contains("200") || status.contains("404"), "{status}");
    let sessions = lab.run(r1, "show policy-map type inspect zone-pair sessions");
    assert!(sessions.contains("DNS inspected, closing"), "{sessions}");
    assert!(sessions.contains("HTTP inspected"), "{sessions}");
    // ICMP isn't in the class: dropped.
    assert!(ping_settled(&mut lab, a, "203.0.113.10").contains(" 0 answered"));
}

#[test]
fn classic_inspection_lets_ftp_data_back_through_the_access_list() {
    let (mut lab, r1, a, _srv) = office();
    lab.run(r1, "conf t\naccess-list 101 deny ip any any\nint g0/1\nip access-group 101 in\nend");
    lab.run(r1, "conf t\nip inspect name FW ftp\nip inspect name FW dns\nint g0/0\nip inspect FW in\nend");
    let (done, bytes) = ftp_get(&mut lab, a).unwrap();
    assert!(done.starts_with("226") && bytes.len() == 5000, "{done}");
    assert_eq!(lab.sim.dns_resolve(&lab.t, a, "www.lab.test").unwrap().to_string(), "203.0.113.10");
    let s = lab.run(r1, "show ip inspect sessions");
    assert!(s.contains("FTP inspected") && s.contains("DNS inspected"), "{s}");
}

#[test]
fn ethernet_has_a_default_mtu_and_hosts_learn_the_path_mtu() {
    // A full-size ping (1472 bytes of data, 1500 of IP) fits the default MTU, DF or not.
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, "conf t\nip route 192.168.2.0 255.255.255.0 203.0.113.2\nend");
    lab.run(_isp, "conf t\nip route 192.168.2.0 255.255.255.0 198.51.100.1\nip route 192.168.1.0 255.255.255.0 203.0.113.1\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    lab.sim.clear_trace();
    assert!(lab.run(a, "ping -n 1 -f -l 1472 192.168.2.10").contains("1 answered"));
    assert!(!lab.sim.trace().iter().any(|e| e.summary.contains("fragment")));
    // A narrower link on the way: R2's LAN side at 1000. A web page bigger than that comes
    // through: the server learns the path MTU from "fragmentation needed" and sends again.
    let b = lab.add(DeviceKind::Server);
    lab.cable(b, "g0", r2, "g0/2");
    lab.run(b, "ipconfig 192.168.3.10 255.255.255.0 192.168.3.1");
    lab.run(r2, "conf t\nint g0/2\nip add 192.168.3.1 255.255.255.0\nno shut\nexit\nint g0/1\nip mtu 1000\nend");
    lab.run(r1, "conf t\nip route 192.168.3.0 255.255.255.0 203.0.113.2\nend");
    lab.run(_isp, "conf t\nip route 192.168.3.0 255.255.255.0 198.51.100.1\nend");
    lab.t.device_mut(b).unwrap().services.http.files.push(WebFile { name: "big.html".into(), content: "x".repeat(6000) });
    lab.t.device_mut(b).unwrap().services.http.on = true;
    assert!(ping_settled(&mut lab, a, "192.168.3.10").contains("4 answered"));
    // R2 g0/1 faces the ISP: the server's replies go out of it.
    let (status, body) = lab.sim.http_get(&lab.t, a, "192.168.3.10".parse().unwrap(), "192.168.3.10", "/big.html").unwrap();
    assert!(status.contains("200"), "{status}");
    assert_eq!(body.len(), 6000);
    assert_eq!(lab.sim.path_mtu(b, "192.168.1.10".parse().unwrap()), Some(1000));
    // The next fetch is sent in pieces that fit from the start.
    lab.sim.clear_trace();
    let (_, body) = lab.sim.http_get(&lab.t, a, "192.168.3.10".parse().unwrap(), "192.168.3.10", "/big.html").unwrap();
    assert_eq!(body.len(), 6000);
    assert!(!lab.sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.contains("don't fragment"))));
}

#[test]
fn gre_tunnels_default_to_1476_and_tcp_follows_the_peer_mss() {
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, "conf t\ninterface tunnel 0\nip address 10.0.0.1 255.255.255.252\ntunnel source g0/1\ntunnel destination 198.51.100.1\nexit\nip route 192.168.2.0 255.255.255.0 10.0.0.2\nend");
    lab.run(r2, "conf t\nint tu0\nip address 10.0.0.2 255.255.255.252\ntunnel source g0/1\ntunnel destination 203.0.113.1\nexit\nip route 192.168.1.0 255.255.255.0 tunnel 0\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    // 1476 fits the tunnel; 1477 with DF doesn't, and the PC is told so.
    assert!(lab.run(a, "ping -n 1 -f -l 1448 192.168.2.10").contains("1 answered"));
    assert!(lab.run(a, "ping -n 1 -f -l 1449 192.168.2.10").contains("needs fragmenting"));
    // Without DF the router fragments before GRE: every GRE packet fits 1500 bytes.
    lab.sim.clear_trace();
    assert!(lab.run(a, "ping -n 1 -l 1472 192.168.2.10").contains("1 answered"));
    assert!(lab.sim.trace().iter().filter(|e| e.from.device == r1 && e.summary.starts_with("GRE")).count() >= 2);
    // A web server behind the tunnel answers in segments that fit (after PMTUD), and a client
    // whose SYN announces a smaller MSS (adjust-mss on R1) gets segments no larger.
    let srv = lab.add(DeviceKind::Server);
    lab.cable(srv, "g0", r2, "g0/2");
    lab.run(srv, "ipconfig 192.168.3.10 255.255.255.0 192.168.3.1");
    lab.run(r2, "conf t\nint g0/2\nip add 192.168.3.1 255.255.255.0\nno shut\nend");
    lab.run(r1, "conf t\nip route 192.168.3.0 255.255.255.0 10.0.0.2\nint g0/0\nip tcp adjust-mss 1200\nend");
    lab.t.device_mut(srv).unwrap().services.http.files.push(WebFile { name: "big.html".into(), content: "y".repeat(5000) });
    lab.t.device_mut(srv).unwrap().services.http.on = true;
    assert!(ping_settled(&mut lab, a, "192.168.3.10").contains("4 answered"));
    lab.sim.clear_trace();
    let (status, body) = lab.sim.http_get(&lab.t, a, "192.168.3.10".parse().unwrap(), "192.168.3.10", "/big.html").unwrap();
    assert!(status.contains("200") && body.len() == 5000, "{status}");
    // Every segment the server sent carries at most 1200 bytes, so nothing needed fragmenting.
    assert!(!lab.sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.contains("don't fragment"))));
}
