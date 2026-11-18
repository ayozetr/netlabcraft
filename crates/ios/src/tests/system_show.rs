//! The system `show` commands: version, inventory, environment, processes, memory, clock detail,
//! `show ip interface` and `show interfaces switchport`.

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn version_tells_memory_licence_and_the_last_restart() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en");
    let v = lab.run(r, "show version");
    assert!(v.contains("Memory: 512 MB main, 256 MB flash, 256 KB NVRAM"), "{v}");
    assert!(v.contains("Licence:") && v.contains("Last restart: power on"), "{v}");
    lab.run(r, "reload");
    lab.run(r, "en");
    assert!(lab.run(r, "show version").contains("Last restart: reload requested"));
}

#[test]
fn inventory_lists_the_chassis_and_the_power_supplies() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en");
    let inv = lab.run(r, "show inventory");
    assert!(inv.contains("PID:") && inv.contains("Power supply 1"), "{inv}");
}

#[test]
fn environment_follows_the_labs_environment_and_the_supplies() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en");
    let all = lab.run(r, "show environment all");
    assert!(all.contains("Power supply 1: OK") && all.contains("Air inlet: 22 C"), "{all}");
    assert!(!lab.run(r, "show environment power").contains("Air inlet"));
    assert!(!lab.run(r, "show environment temperature").contains("Power supply"));
    lab.t.environment.enabled = true;
    lab.t.environment.temperature = 31;
    assert!(lab.run(r, "show environment temperature").contains("Air inlet: 31 C"));
}

#[test]
fn processes_and_memory_have_their_views() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en");
    assert!(lab.run(s, "show processes cpu sorted").contains("CPU use"));
    assert!(lab.run(s, "show processes cpu history").contains("last 60 seconds"));
    assert!(lab.run(s, "show processes memory").contains("Holding"));
    let m = lab.run(s, "show memory statistics");
    assert!(m.contains("Main memory") && m.contains("Largest free block"), "{m}");
}

#[test]
fn clock_detail_names_the_time_source() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en");
    assert!(lab.run(r, "show clock detail").contains("Time source: the simulation's own timer"));
    lab.run(r, "clock set 10:30:00 30 Oct 2026");
    assert!(lab.run(r, "show clock detail").contains("Time source: the console"));
}

#[test]
fn ip_interface_lists_every_per_interface_setting() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nend");
    let out = lab.run(r, "show ip interface g0/0");
    for want in [
        "Mask 255.255.255.0",
        "Helper addresses: none",
        "NAT: not used",
        "MTU is 1500",
        "Directed broadcast forwarding: off",
        "Destination-unreachable replies: on",
    ] {
        assert!(out.contains(want), "{want}: {out}");
    }
    lab.run(r, "conf t\nint g0/0\nip directed-broadcast\nno ip unreachables\nend");
    let out = lab.run(r, "show ip interface g0/0");
    assert!(out.contains("Directed broadcast forwarding: on") && out.contains("Destination-unreachable replies: off"), "{out}");
}

#[test]
fn switchport_without_a_port_lists_every_switch_port() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en");
    let out = lab.run(s, "show interfaces switchport");
    assert!(out.contains("Mode: access") && out.matches("Access VLAN").count() > 2, "{out}");
}

#[test]
fn tech_support_gathers_the_reports_and_stays_bounded() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en");
    let out = lab.run(s, "show tech-support");
    for want in ["show version", "show inventory", "show environment all", "show memory statistics", "show interfaces trunk", "show vlan brief"] {
        assert!(out.contains(want), "{want}");
    }
    assert!(out.len() < 60_000);
}
