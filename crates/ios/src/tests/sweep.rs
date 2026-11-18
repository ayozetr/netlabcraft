//! Commands the sweep found missing: aliases of existing commands, `show terminal`, `show line`,
//! `show cdp interface`, the MAC aging time, the SNMP views, `more` and `pwd`.

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn aliases_run_the_commands_they_stand_for() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s, "fa0/1");
    lab.run(s, "en\nconf t\nhostname Core\nend");
    assert_eq!(lab.run(s, "write terminal"), lab.run(s, "show running-config"));
    assert_eq!(lab.run(s, "show derived-config"), lab.run(s, "show running-config"));
    assert_eq!(lab.run(s, "show mac-address-table"), lab.run(s, "show mac address-table"));
    assert_eq!(lab.run(s, "show mac-address-table dynamic"), lab.run(s, "show mac address-table dynamic"));
    lab.run(s, "copy running-config startup-config");
    assert!(lab.run(s, "show startup-config").contains("hostname Core"));
    lab.run(s, "write erase");
    assert!(lab.try_run(s, "show startup-config").starts_with("% No saved configuration"));
    lab.run(s, "copy running-config startup-config\nerase nvram:");
    assert!(lab.try_run(s, "show startup-config").starts_with("% No saved configuration"));
    lab.run(s, "copy running-config startup-config\nconf t\nhostname Other\nend\nconfigure memory");
    assert!(lab.prompt(s).starts_with("Core"), "the saved configuration is merged into the running one: {}", lab.prompt(s));
    lab.run(s, "clear mac-address-table dynamic");
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nclear arp");
}

#[test]
fn more_and_pwd_show_the_configurations_and_the_file_system() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nhostname Edge\nend");
    assert_eq!(lab.run(r, "pwd"), "flash:");
    assert_eq!(lab.run(r, "more system:running-config"), lab.run(r, "show running-config"));
    assert!(lab.try_run(r, "more nvram:startup-config").starts_with("% No saved configuration"));
    lab.run(r, "copy running-config startup-config");
    assert!(lab.run(r, "more nvram:startup-config").contains("hostname Edge"));
    assert!(lab.try_run(r, "more flash:other").starts_with("% "));
}

#[test]
fn terminal_line_and_cdp_views_read_the_configuration() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.run(r, "en\nconf t\nint g0/0\nno shut\nexit\nline vty 0 4\nlogin local\ntransport input ssh\nexit\nline con 0\npassword cisco\nlogin\nend");
    let line = lab.run(r, "show line");
    assert!(line.contains("con 0") && line.contains("password"), "{line}");
    assert!(line.contains("vty 0 4") && line.contains("local") && line.contains("ssh"), "{line}");
    assert!(lab.run(r, "show terminal").contains("History: the last 10 commands"), "{}", lab.run(r, "show terminal"));
    lab.run(r, "conf t\nline vty 0 4\nhistory size 25\nend");
    assert!(lab.run(r, "show terminal").contains("last 25 commands"));
    let cdp = lab.run(r, "show cdp interface");
    assert!(cdp.contains("GigabitEthernet0/0 is up") && cdp.contains("every 60 seconds, held for 180 seconds"), "{cdp}");
    lab.run(r, "conf t\ncdp timer 20\ncdp holdtime 60\nint g0/1\nno cdp enable\nend");
    let cdp = lab.run(r, "show cdp interface");
    assert!(cdp.contains("every 20 seconds, held for 60 seconds") && !cdp.contains("GigabitEthernet0/1"), "{cdp}");
    lab.run(r, "conf t\nno cdp run\nend");
    assert!(lab.run(r, "show cdp interface").contains("off on this device"));
}

#[test]
fn the_mac_aging_time_and_the_snmp_views_show_what_was_set() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(s, "en");
    assert!(lab.run(s, "show mac address-table aging-time").contains("300 seconds"));
    lab.run(s, "conf t\nmac address-table aging-time 600\nend");
    assert!(lab.run(s, "show mac address-table aging-time").contains("600 seconds"));
    assert!(lab.run(s, "show mac-address-table aging-time").contains("600 seconds"));
    assert!(lab.run(s, "show snmp community").contains("No communities"));
    lab.run(s, "conf t\nsnmp-server community lab ro\nsnmp-server community admin rw\nsnmp-server host 10.0.0.9 version 2c lab");
    lab.run(s, "snmp-server group LECTORES v3 auth\nsnmp-server user ana LECTORES v3 auth sha clave1234\nend");
    let comm = lab.run(s, "show snmp community");
    assert!(
        comm.contains("Community name: lab\nCommunity access: read-only") && comm.contains("Community name: admin\nCommunity access: read-write"),
        "{comm}"
    );
    assert!(lab.run(s, "show snmp host").contains("Notification host: 10.0.0.9  version: 2c  community: lab"));
    assert!(lab.run(s, "show snmp group").contains("Group name: LECTORES  security model: v3 auth  access: read-only"));
    let user = lab.run(s, "show snmp user");
    assert!(user.contains("User name: ana\nGroup: LECTORES\nAuthentication: SHA") && !user.contains("clave1234"), "{user}");
}
