//! IP source guard: bindings from DHCP snooping and typed by hand decide who may send.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::{Lab, dropped_reasons};

/// R1 serves DHCP behind Fa0/24 (trusted); PC A takes a lease on Fa0/1; PC B has a typed
/// address on Fa0/2.
fn lab() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(r1, "g0/0", s, "fa0/24");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(b, "fa0", s, "fa0/2");
    lab.run(
        r1,
        "en\nconf t\nint g0/0\nip add 192.168.10.1 255.255.255.0\nno shut\nexit\nip dhcp excluded-address 192.168.10.1 192.168.10.9\nip dhcp pool LAN\nnetwork 192.168.10.0 255.255.255.0\ndefault-router 192.168.10.1\nend",
    );
    lab.run(
        s,
        "en\nconf t\nip dhcp snooping\nip dhcp snooping vlan 1\nno ip dhcp snooping information option\nint fa0/24\nip dhcp snooping trust\nexit\ninterface range fa0/1 - 2\nip verify source\nend",
    );
    lab.run(b, "ipconfig 192.168.10.50 255.255.255.0 192.168.10.1");
    (lab, r1, s, a, b)
}

#[test]
fn only_an_address_with_a_binding_may_send_from_a_guarded_port() {
    let (mut lab, _r1, s, a, b) = lab();
    assert!(lab.run(a, "ipconfig /renew").contains("192.168.10.10/24"));
    lab.sim.clear_trace();
    assert!(lab.run(a, "ping -n 1 192.168.10.1").contains("reply from"));
    assert!(!dropped_reasons(&lab).iter().any(|r| r.contains("IP source guard")));
    // B was never bound: its packets stop at the port.
    lab.sim.clear_trace();
    assert!(lab.run(b, "ping -n 1 192.168.10.1").contains("no answer"));
    assert!(dropped_reasons(&lab).iter().any(|r| r.contains("IP source guard") && r.contains("192.168.10.50") && r.contains("Fa0/2")));
    let v = lab.run(s, "show ip verify source");
    assert!(v.contains("Fa0/1") && v.contains("192.168.10.10") && v.contains("Fa0/2") && v.contains("deny-all"), "{v}");
    assert!(lab.run(s, "show ip dhcp snooping").contains("Source guard drops:       "));
    // A binding typed by hand lets B through, and shows with the learned one.
    lab.run(s, "conf t");
    let mac = lab.t.device(b).unwrap().ports[0].mac.dotted();
    lab.run(s, &format!("ip source binding {mac} vlan 1 192.168.10.50 interface fa0/2\nend"));
    assert!(lab.run(b, "ping -n 1 192.168.10.1").contains("reply from"));
    let sb = lab.run(s, "show ip source binding");
    assert!(sb.contains("static") && sb.contains("dhcp-snooping") && sb.contains("2 bindings"), "{sb}");
    assert!(lab.run(s, "show running-config").contains(&format!("ip source binding {mac} vlan 1 192.168.10.50 interface Fa0/2")));
    lab.run(s, &format!("conf t\nno ip source binding {mac} vlan 1\nend"));
    assert!(lab.run(b, "ping -n 1 192.168.10.1").contains("no answer"));
}

#[test]
fn port_security_mode_also_checks_the_mac_address_and_off_means_off() {
    let (mut lab, _r1, s, a, b) = lab();
    lab.run(a, "ipconfig /renew");
    let mac_a = lab.t.device(a).unwrap().ports[0].mac.dotted();
    // B is cabled to Fa0/2; a binding for B's address with A's MAC fails the MAC check there.
    lab.run(
        s,
        &format!("conf t\nip source binding {mac_a} vlan 1 192.168.10.50 interface fa0/2\ninterface fa0/2\nip verify source port-security\nend"),
    );
    assert!(lab.run(b, "ping -n 1 192.168.10.1").contains("no answer"));
    assert!(lab.run(s, "show running-config").contains(" ip verify source port-security"));
    lab.run(s, "conf t\ninterface fa0/2\nno ip verify source\nend");
    assert!(lab.run(b, "ping -n 1 192.168.10.1").contains("reply from"));
    assert!(!lab.run(s, "show running-config").contains("verify source port-security"));
}

#[test]
fn the_commands_check_their_arguments() {
    let (mut lab, _r1, s, _a, _b) = lab();
    lab.run(s, "conf t");
    assert!(lab.try_run(s, "ip source binding zz vlan 1 10.0.0.1 interface fa0/2").starts_with("% "));
    assert!(lab.try_run(s, "ip source binding 0000.0000.0001 vlan 1 224.0.0.1 interface fa0/2").starts_with("% "));
    assert!(lab.try_run(s, "ip source binding 0000.0000.0001 vlan 1 10.0.0.1 interface nope").starts_with("% "));
    assert!(lab.try_run(s, "no ip source binding 0000.0000.0001 vlan 1").starts_with("% "));
    lab.run(s, "end");
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en");
    assert!(!lab.try_run(r, "show ip verify source").contains("Filter-type"));
}
