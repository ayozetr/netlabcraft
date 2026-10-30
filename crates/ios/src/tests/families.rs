//! Typed console tests for the device families: the switch card in routers, power over
//! Ethernet and SIP phones' call control.

use netlabcraft_model::{DeviceKind, models};

use super::Lab;
use crate::grammar::{SPECS, Tok};

/// A modular router with a 4-port switch card in bay 0/1.
fn router_with_switch_card(lab: &mut Lab) -> netlabcraft_core::DeviceId {
    let r = lab.t.add_model(models::find("Router-M2").unwrap(), None, 0.0, 0.0).unwrap();
    lab.t.device_mut(r).unwrap().power_off();
    lab.t.set_module(r, "0/1", Some("Card-4SW")).unwrap();
    lab.t.device_mut(r).unwrap().powered_off = false;
    r
}

#[test]
fn a_switch_card_switches_vlans_and_the_router_routes_between_them() {
    let mut lab = Lab::new();
    let r = router_with_switch_card(&mut lab);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    let c = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r, "fa0/1/0");
    lab.cable(b, "fa0", r, "fa0/1/1");
    lab.cable(c, "fa0", r, "fa0/1/2");
    lab.run(
        r,
        "en
        conf t
        vlan 10
        name STAFF
        vlan 20
        exit
        int fa0/1/0
        switchport access vlan 10
        int fa0/1/2
        switchport access vlan 10
        int fa0/1/1
        switchport access vlan 20
        int vlan 10
        ip add 10.0.10.1 255.255.255.0
        no shut
        int vlan 20
        ip add 10.0.20.1 255.255.255.0
        no shut
        end",
    );
    assert_eq!(lab.prompt(r), "R1#");
    lab.run(a, "ipconfig 10.0.10.10 255.255.255.0 10.0.10.1");
    lab.run(c, "ipconfig 10.0.10.12 255.255.255.0 10.0.10.1");
    lab.run(b, "ipconfig 10.0.20.10 255.255.255.0 10.0.20.1");
    // Switched within VLAN 10, routed to VLAN 20.
    assert!(lab.run(a, "ping 10.0.10.12").contains("0% lost"));
    let out = lab.run(a, "ping 10.0.20.10");
    assert!(out.contains("reply from 10.0.20.10"), "{out}");
    let vlans = lab.run(r, "show vlan brief");
    assert!(vlans.contains("STAFF") && vlans.contains("Fa0/1/0, Fa0/1/2") && !vlans.contains("Gi0/0"), "{vlans}");
    let mac = lab.run(r, "show mac address-table");
    assert!(mac.contains("Fa0/1/0"), "{mac}");
    let brief = lab.run(r, "show ip interface brief");
    assert!(brief.contains("Vlan10") && brief.contains("10.0.10.1/24"), "{brief}");
    // A switch port takes no address; the built-in ports stay routed.
    lab.run(r, "conf t\nint fa0/1/0");
    assert!(lab.try_run(r, "ip address 10.9.9.1 255.255.255.0").contains("switch port"));
    assert!(lab.try_run(r, "no switchport").contains("% "));
    lab.run(r, "int g0/0");
    assert!(lab.try_run(r, "switchport mode trunk").contains("% "), "a built-in port is not a switch port");
    lab.run(r, "ip add 10.1.1.1 255.255.255.0\nend");
    // The configuration survives a reload, and the VLAN database lives apart from it.
    let before = lab.run(r, "show running-config");
    for l in ["interface FastEthernet0/1/0\n switchport access vlan 10", "interface Vlan10\n ip address 10.0.10.1 255.255.255.0"] {
        assert!(before.contains(l), "{l}\n{before}");
    }
    assert!(!before.contains("interface FastEthernet0/1/0\n no ip address"), "{before}");
    lab.run(r, "copy run start\nreload");
    let after = lab.run(r, "en\nshow running-config");
    assert_eq!(after, before);
    assert!(lab.run(r, "show vlan brief").contains("STAFF"));
    // A router without the card: no VLAN commands.
    let plain = lab.add(DeviceKind::Router);
    lab.run(plain, "en\nconf t");
    assert!(lab.try_run(plain, "vlan 10").contains("% "));
    assert!(lab.try_run(plain, "interface vlan 10").contains("% "));
}

#[test]
fn a_switch_card_trunks_to_a_switch() {
    let mut lab = Lab::new();
    let r = router_with_switch_card(&mut lab);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(r, "fa0/1/3", s, "g0/1");
    lab.cable(a, "fa0", r, "fa0/1/0");
    lab.cable(b, "fa0", s, "fa0/1");
    lab.run(r, "en\nconf t\nvlan 30\nint fa0/1/0\nswitchport access vlan 30\nint fa0/1/3\nswitchport mode trunk\nend");
    lab.run(s, "en\nconf t\nvlan 30\nint fa0/1\nswitchport access vlan 30\nint g0/1\nswitchport mode trunk\nend");
    lab.run(a, "ipconfig 10.3.0.1 255.255.255.0");
    lab.run(b, "ipconfig 10.3.0.2 255.255.255.0");
    lab.sim.converge_stp();
    let out = lab.run(a, "ping 10.3.0.2");
    assert!(out.contains("reply from 10.3.0.2"), "{out}");
    assert!(lab.run(r, "show interfaces trunk").contains("Fa0/1/3"));
    let sw = lab.run(r, "show interfaces fa0/1/3 switchport");
    assert!(sw.contains("trunk"), "{sw}");
}

#[test]
fn every_command_survives_hostile_arguments_on_a_router_with_a_switch_card() {
    let hostile = ["", "0", "4095", "1.2.3.4", "g9/9/9", "ä€😀", "remove 1-4094"];
    for spec in SPECS {
        for arg in hostile {
            let mut lab = Lab::new();
            let d = router_with_switch_card(&mut lab);
            let other = lab.add(DeviceKind::Pc);
            lab.cable(d, "fa0/1/0", other, "fa0");
            let line: Vec<String> = spec
                .path
                .iter()
                .map(|t| match t {
                    Tok::K(k) => k.to_string(),
                    Tok::Arg(_) | Tok::Rest(_) => arg.to_string(),
                })
                .collect();
            for setup in ["", "en", "en\nconf t", "en\nconf t\nint fa0/1/0", "en\nconf t\nint g0/0", "en\nconf t\nint vlan 5", "en\nconf t\nvlan 5"] {
                for l in setup.lines() {
                    lab.try_run(d, l);
                }
                let _ = lab.try_run(d, &line.join(" "));
                let _ = lab.try_run(d, &format!("{} ?", line.join(" ")));
                lab.clis.clear();
            }
        }
    }
}
