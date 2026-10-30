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
            for setup in [
                "",
                "en",
                "en\nconf t",
                "en\nconf t\nint fa0/1/0",
                "en\nconf t\nint g0/0",
                "en\nconf t\nint vlan 5",
                "en\nconf t\nvlan 5",
                "en\nconf t\nvoice register global",
                "en\nconf t\nvoice register global\nmax-dn 2\nmax-pool 2\nexit\nvoice register dn 1",
                "en\nconf t\nvoice register global\nmax-dn 2\nmax-pool 2\nexit\nvoice register pool 1",
            ] {
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

#[test]
fn power_inline_by_typing() {
    let mut lab = Lab::new();
    let s = lab.t.add_model(models::find("Switch 24FE PoE").unwrap(), None, 0.0, 0.0).unwrap();
    let ph = lab.t.add_model(models::find("IP-Phone").unwrap(), None, 0.0, 0.0).unwrap();
    lab.cable(ph, "fa0", s, "fa0/3");
    lab.run(s, "en");
    let shown = lab.run(s, "show power inline");
    assert!(shown.contains("Power budget 370.0 W, in use 6.3 W, left 363.7 W"), "{shown}");
    assert!(shown.lines().any(|l| l.starts_with("Fa0/3") && l.contains("auto") && l.contains(" on ") && l.contains("6.3")), "{shown}");
    lab.run(s, "conf t\nint fa0/3\npower inline never\nend");
    assert!(lab.t.power_problem(ph).unwrap().contains("power inline never"));
    assert!(lab.run(s, "show power inline").contains("in use 0.0 W"));
    let run = lab.run(s, "show running-config");
    assert!(run.contains("interface FastEthernet0/3\n power inline never"), "{run}");
    lab.run(s, "copy run start\nreload");
    lab.run(s, "en");
    assert!(lab.run(s, "show running-config").contains(" power inline never"));
    lab.run(s, "conf t\nint fa0/3\npower inline auto");
    assert!(lab.t.power_problem(ph).is_none());
    assert!(lab.try_run(s, "power inline sometimes").contains("% "));
    // A plain switch has no power to give.
    let plain = lab.add(DeviceKind::Switch);
    lab.run(plain, "en");
    assert!(lab.try_run(plain, "show power inline").contains("gives no power"));
    lab.run(plain, "conf t\nint fa0/1");
    assert!(lab.try_run(plain, "power inline never").contains("gives no power"));
}

#[test]
fn voice_register_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(
        r,
        "en
        conf t
        int g0/0
        ip add 10.5.0.1 255.255.255.0
        no shut
        exit
        voice register global
        mode cme
        source-address 10.5.0.1 port 5060
        max-dn 2
        max-pool 2
        exit
        voice register dn 1
        number 3001
        exit
        voice register pool 1
        id mac 0200.0000.0a01
        number 1 dn 1
        end",
    );
    assert_eq!(lab.prompt(r), "R1#");
    lab.run(r, "conf t\nvoice register pool 1");
    assert_eq!(lab.prompt(r), "R1(config-register-pool)#");
    // Telephony-service commands don't belong here, nor pool commands under a dn.
    assert!(lab.try_run(r, "button 1:1").contains("% "));
    assert!(lab.try_run(r, "number 2 dn 1").contains("% "));
    assert!(lab.try_run(r, "number 1 dn 9").contains("no voice register dn 9"));
    lab.run(r, "exit\nvoice register dn 1");
    assert!(lab.try_run(r, "id mac 0200.0000.0a02").contains("% "));
    lab.run(r, "exit\nvoice register global");
    assert!(lab.try_run(r, "max-ephones 4").contains("% "));
    lab.run(r, "exit\nvoice register dn 2\nexit");
    assert!(lab.try_run(r, "voice register dn 3").contains("max-dn is 2"));
    lab.run(r, "telephony-service");
    assert!(lab.try_run(r, "mode cme").contains("voice register global"));
    lab.run(r, "end");
    let shown = lab.run(r, "show voice register all");
    assert!(shown.contains("10.5.0.1, UDP port 5060 (mode cme)") && shown.contains("3001") && shown.contains("not registered"), "{shown}");
    let before = lab.run(r, "show running-config");
    for l in [
        "voice register global\n mode cme\n source-address 10.5.0.1 port 5060\n max-dn 2\n max-pool 2",
        "voice register pool 1\n id mac 0200.0000.0a01\n number 1 dn 1",
    ] {
        assert!(before.contains(l), "{l}\n{before}");
    }
    lab.run(r, "copy run start\nreload");
    assert_eq!(lab.run(r, "en\nshow running-config"), before);
    lab.run(r, "conf t\nno voice register pool 1\nno voice register global\nend");
    assert!(lab.run(r, "show voice register all").contains("off"));
}
