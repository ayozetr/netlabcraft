//! `show interfaces` says "line protocol is up/down" (M9.7).

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn show_interfaces_gives_status_and_line_protocol_for_each_state() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(pc, "fa0", s, "fa0/1");
    lab.run(s, "en\nconf t\ninterface fa0/3\nshutdown\nexit\ninterface vlan 1\nip address 10.0.0.2 255.255.255.0\nno shutdown\nend");
    let line = |lab: &mut Lab, cmd: &str| lab.run(s, cmd).lines().next().unwrap_or("").to_string();
    assert_eq!(line(&mut lab, "show interfaces fa0/1"), "FastEthernet0/1 is up, line protocol is up");
    assert_eq!(line(&mut lab, "show interfaces fa0/2"), "FastEthernet0/2 is down (no cable), line protocol is down");
    assert_eq!(line(&mut lab, "show interfaces fa0/3"), "FastEthernet0/3 is administratively down, line protocol is down");
    // The filter students type finds one line per port.
    let all = lab.run(s, "show interfaces | include line protocol");
    assert!(all.lines().count() >= 24 && all.lines().all(|l| l.contains(", line protocol is ")), "{all}");
    assert!(all.contains("FastEthernet0/1 is up, line protocol is up"), "{all}");
    assert!(lab.run(s, "show interfaces | include line protocol is up").lines().all(|l| l.ends_with("line protocol is up")));
    // A VLAN interface comes up with a port of its VLAN.
    assert!(line(&mut lab, "show interfaces vlan 1").starts_with("Vlan1 is up, line protocol is up"));
    lab.run(s, "conf t\ninterface vlan 1\nshutdown\nend");
    assert_eq!(line(&mut lab, "show interfaces vlan 1"), "Vlan1 is administratively down, line protocol is down");
}

#[test]
fn router_interfaces_subinterfaces_and_tunnels_use_the_same_wording() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.run(
        r,
        "en\nconf t\ninterface g0/0\nno shutdown\nexit\ninterface g0/0.10\nencapsulation dot1q 10\nip address 10.0.10.1 255.255.255.0\nexit\ninterface g0/1\nshutdown\nexit\ninterface tunnel 0\nshutdown\nend",
    );
    let all = lab.run(r, "show interfaces");
    assert!(all.contains("GigabitEthernet0/0 is up, line protocol is up"), "{all}");
    assert!(all.contains("GigabitEthernet0/0.10 is up, line protocol is up"), "{all}");
    assert!(all.contains("GigabitEthernet0/1 is administratively down, line protocol is down"), "{all}");
    assert!(all.contains("Tunnel0 is administratively down, line protocol is down"), "{all}");
    // Taking the port down at the far end takes the line protocol down with the status.
    lab.run(s, "en\nconf t\ninterface g0/1\nshutdown\nend");
    let one = lab.run(r, "show interfaces g0/0");
    assert!(one.starts_with("GigabitEthernet0/0 is down (far end shut down), line protocol is down"), "{one}");
    assert!(lab.run(r, "show interfaces g0/0.10").starts_with("GigabitEthernet0/0.10 is down, line protocol is down"));
}
