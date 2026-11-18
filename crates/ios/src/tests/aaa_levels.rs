//! Privilege levels, `aaa authorization exec|commands` and `aaa accounting`.

use netlabcraft_model::DeviceKind;
use netlabcraft_model::services::{AaaClient, AaaProfile, AaaUser};

use super::Lab;

#[test]
fn privilege_levels_move_commands_and_enable_asks_for_the_secret_of_each_level() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(
        r,
        "en
        conf t
        enable secret topsecret
        enable secret level 7 seven-test
        privilege exec level 7 show running-config
        privilege exec level 7 configure terminal
        privilege configure level 7 hostname
        end
        disable",
    );
    assert!(lab.try_run(r, "show privilege").contains("level is 1"));
    assert!(lab.try_run(r, "show running-config").contains("Not understood"));
    lab.try_run(r, "enable 7");
    assert_eq!(lab.prompt(r), "Password: ");
    assert!(lab.try_run(r, "wrong").contains("Wrong password"));
    assert_eq!(lab.try_run(r, "seven-test"), "");
    assert!(lab.prompt(r).ends_with('#'));
    assert!(lab.try_run(r, "show privilege").contains("level is 7"));
    assert!(lab.try_run(r, "show running-config").contains("hostname"));
    assert!(lab.try_run(r, "reload").contains("needs privilege level 15"));
    lab.run(r, "configure terminal");
    lab.run(r, "hostname Edge");
    assert!(lab.try_run(r, "interface g0/0").contains("needs privilege level 15"));
    lab.run(r, "end");
    // Level 15 asks for the enable secret.
    lab.try_run(r, "enable");
    assert_eq!(lab.prompt(r), "Password: ");
    assert_eq!(lab.try_run(r, "topsecret"), "");
    assert!(lab.try_run(r, "show privilege").contains("level is 15"));
    let cfg = lab.run(r, "show running-config");
    assert!(cfg.contains("privilege exec level 7 show running-config"), "{cfg}");
    assert!(cfg.contains("enable secret level 7 "), "{cfg}");
    assert!(!cfg.contains("seven-test"), "the secret is hashed: {cfg}");
    lab.run(r, "conf t\nno privilege exec level 7 show running-config\nno enable secret level 7\nend");
    let cfg = lab.run(r, "show running-config");
    assert!(!cfg.contains("privilege exec level 7 show running-config") && !cfg.contains("enable secret level 7 "), "{cfg}");
    // Words that are not commands, and levels out of range.
    assert!(!lab.try_run(r, "configure terminal").starts_with('%'));
    assert!(lab.try_run(r, "privilege exec level 7 nosuchthing").contains("not a exec command"));
    assert!(lab.try_run(r, "privilege exec level 16 show version").starts_with('%'));
}

/// A router at 192.168.1.1 whose logins, authorization and accounting go to a Server at 192.168.1.5.
fn lab_with_server() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let pc = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(pc, "fa0", s, "fa0/1");
    lab.cable(srv, "g0", s, "fa0/2");
    lab.run(pc, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 192.168.1.5 255.255.255.0 192.168.1.1");
    {
        let aaa = &mut lab.t.device_mut(srv).unwrap().services.aaa;
        aaa.on = true;
        aaa.clients.push(AaaClient { name: "R1".into(), address: "192.168.1.1".parse().unwrap(), secret: "radkey".into() });
        aaa.users.push(AaaUser { name: "teacher".into(), password: "pt-pass".into() });
        aaa.users.push(AaaUser { name: "ops".into(), password: "ops-pass".into() });
        aaa.profiles.push(AaaProfile { user: "teacher".into(), privilege: 15, deny: vec!["reload".into()] });
        aaa.profiles.push(AaaProfile { user: "ops".into(), privilege: 7, deny: vec![] });
    }
    lab.run(
        r,
        "en
        conf t
        int g0/0
        ip add 192.168.1.1 255.255.255.0
        no shut
        exit
        aaa new-model
        tacacs-server host 192.168.1.5 key radkey
        aaa authentication login default group tacacs+ local
        aaa authorization exec default group tacacs+ local
        aaa authorization commands 15 default group tacacs+ none
        aaa accounting exec default start-stop group tacacs+
        aaa accounting commands 15 default start-stop group tacacs+
        line vty 0 4
        login authentication default
        end",
    );
    (lab, r, pc, srv)
}

#[test]
fn a_tacacs_server_grants_the_level_refuses_a_command_and_keeps_the_records() {
    let (mut lab, _r, pc, srv) = lab_with_server();
    assert!(lab.try_run(pc, "telnet 192.168.1.1").contains("Open"));
    lab.try_run(pc, "teacher");
    assert_eq!(lab.try_run(pc, "pt-pass"), "");
    assert_eq!(lab.prompt(pc), "R1#", "the profile grants level 15");
    assert!(lab.try_run(pc, "show running-config").contains("aaa authorization exec default group tacacs+ local"));
    let denied = lab.try_run(pc, "reload");
    assert!(denied.contains("Command authorization failed"), "{denied}");
    lab.try_run(pc, "exit");
    let rows = lab.sim.aaa_records(srv);
    let has = |kind: &str, detail: &str| rows.iter().any(|r| r.kind == kind && r.detail == detail && r.user == "teacher" && r.protocol == "TACACS+");
    assert!(has("start", "exec") && has("stop", "exec"), "{rows:?}");
    assert!(has("start", "command: show running-config") && has("stop", "command: show running-config"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.detail.contains("reload")), "a refused command is not recorded: {rows:?}");
    // A user whose profile says level 7 lands there.
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "ops");
    assert_eq!(lab.try_run(pc, "ops-pass"), "");
    assert!(lab.try_run(pc, "show privilege").contains("level is 7"));
    lab.try_run(pc, "exit");
}

#[test]
fn accounting_goes_to_radius_too_and_show_aaa_lists_what_is_set() {
    let (mut lab, r, pc, srv) = lab_with_server();
    lab.run(
        r,
        "conf t
        no tacacs-server host 192.168.1.5
        radius-server host 192.168.1.5 key radkey
        aaa authentication login default group radius local
        no aaa authorization exec default
        no aaa authorization commands 15 default
        no aaa accounting commands 15 default
        aaa accounting exec default start-stop group radius
        end",
    );
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "teacher");
    assert_eq!(lab.try_run(pc, "pt-pass"), "");
    lab.try_run(pc, "exit");
    let rows = lab.sim.aaa_records(srv);
    assert!(rows.iter().any(|x| x.protocol == "RADIUS" && x.kind == "start" && x.detail == "exec" && x.user == "teacher"), "{rows:?}");
    assert!(rows.iter().any(|x| x.protocol == "RADIUS" && x.kind == "stop" && x.detail == "exec"), "{rows:?}");
    let shown = lab.run(r, "show aaa accounting");
    assert!(shown.contains("aaa accounting exec default start-stop group radius") && shown.contains("acknowledged"), "{shown}");
    assert!(lab.run(r, "show aaa").contains("RADIUS server: 192.168.1.5"));
    assert!(lab.run(r, "show aaa authorization").contains("No authorization lists"));
    lab.run(r, "conf t");
    assert!(lab.try_run(r, "aaa accounting exec default start-stop group nowhere").starts_with('%'));
}
