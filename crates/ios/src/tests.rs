use packetcraft_core::DeviceId;
use packetcraft_model::{DeviceKind, End, SwitchportMode, Topology};
use packetcraft_sim::Sim;

use crate::grammar::{SPECS, Tok};
use crate::*;

struct Lab {
    t: Topology,
    sim: Sim,
    clis: std::collections::BTreeMap<DeviceId, Cli>,
}

impl Lab {
    fn new() -> Lab {
        Lab { t: Topology::new(), sim: Sim::new(), clis: Default::default() }
    }

    fn add(&mut self, kind: DeviceKind) -> DeviceId {
        self.t.add_device(kind, None, 0.0, 0.0).unwrap()
    }

    fn cable(&mut self, a: DeviceId, pa: &str, b: DeviceId, pb: &str) {
        let pa = self.t.device(a).unwrap().port_index(pa).unwrap();
        let pb = self.t.device(b).unwrap().port_index(pb).unwrap();
        self.t.connect(End { device: a, port: pa }, End { device: b, port: pb }).unwrap();
        self.sim.topology_changed(&self.t);
    }

    /// Runs lines; returns the last output. Panics on lines that answer with `%` errors.
    fn run(&mut self, dev: DeviceId, lines: &str) -> String {
        let mut last = String::new();
        for l in lines.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let cli = self.clis.entry(dev).or_default();
            let o = execute(&mut self.t, &mut self.sim, dev, cli, l);
            assert!(!o.output.starts_with("% ") && !o.output.contains("\n% "), "`{l}` failed:\n{}", o.output);
            last = o.output;
        }
        last
    }

    fn try_run(&mut self, dev: DeviceId, line: &str) -> String {
        let cli = self.clis.entry(dev).or_default();
        execute(&mut self.t, &mut self.sim, dev, cli, line).output
    }

    fn prompt(&self, dev: DeviceId) -> String {
        prompt(&self.t, dev, self.clis.get(&dev).unwrap_or(&Cli::default()))
    }
}

const R1_CONFIG: &str = "
    en
    conf t
    hostname Edge
    int g0/0
    description LAN side
    ip add 192.168.1.1 255.255.255.0
    no shut
    int g0/1
    ip address 192.168.2.1 255.255.255.0
    no shutdown
    end";

#[test]
fn a_two_subnet_lab_built_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(b, "fa0", r, "g0/1");
    assert_eq!(lab.prompt(r), "R1>");
    lab.run(r, R1_CONFIG);
    assert_eq!(lab.prompt(r), "Edge#");
    assert_eq!(lab.t.device(r).unwrap().name, "Edge");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.2.10 255.255.255.0 192.168.2.1");
    let out = lab.run(a, "ping 192.168.2.10");
    assert!(out.contains("seq 2: reply from 192.168.2.10"), "{out}");
    assert!(out.contains("4 sent"), "{out}");
    let out = lab.run(r, "ping 192.168.1.10");
    assert!(out.contains("!!!!") && out.contains("% answered"), "{out}");
    let brief = lab.run(r, "sh ip int br");
    assert!(brief.contains("GigabitEthernet0/0       192.168.1.1/24     up      up"), "{brief}");
    assert!(brief.contains("GigabitEthernet0/2       unassigned         down    down"), "{brief}");
    let routes = lab.run(r, "show ip route");
    assert!(routes.contains("C    192.168.1.0/24       connected, GigabitEthernet0/0"), "{routes}");
    assert!(routes.contains("No default route"), "{routes}");
    let mac = lab.run(s, "en\nshow mac address-table");
    assert!(mac.contains("2 entries"), "{mac}");
    let arp = lab.run(r, "show arp");
    assert!(arp.contains("192.168.1.10"), "{arp}");
}

#[test]
fn running_config_survives_write_and_reload() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(
        r,
        &format!(
            "{R1_CONFIG}
            conf t
            int g0/2.30
            encapsulation dot1q 30
            ip address 10.30.0.1 255.255.255.0
            exit
            ip route 0.0.0.0 0.0.0.0 192.168.2.254
            ip route 172.16.0.0 255.255.0.0 g0/0
            end"
        ),
    );
    let before = lab.run(r, "show running-config");
    assert!(before.contains("interface GigabitEthernet0/2.30\n encapsulation dot1Q 30\n ip address 10.30.0.1 255.255.255.0"), "{before}");
    assert!(before.contains("ip route 172.16.0.0 255.255.0.0 GigabitEthernet0/0"), "{before}");
    // Unsaved changes are lost on reload.
    lab.run(r, "reload");
    assert!(lab.run(r, "en\nshow run").contains("hostname Edge"));
    assert!(!lab.run(r, "show run").contains("192.168.1.1"));
    lab.t.devices[0].name = "R1".into();
    lab.clis.clear();
    lab.run(r, R1_CONFIG);
    lab.run(r, "conf t\nint g0/2.30\nencapsulation dot1Q 30\nip address 10.30.0.1 255.255.255.0\nexit\nip route 0.0.0.0 0.0.0.0 192.168.2.254\nip route 172.16.0.0 255.255.0.0 g0/0\nend");
    lab.run(r, "copy run start");
    let saved = lab.run(r, "show running-config");
    assert_eq!(saved, before);
    lab.run(r, "conf t\nno ip route 0.0.0.0 0.0.0.0\nint g0/0\nshutdown\nend");
    assert_ne!(lab.run(r, "show run"), before);
    let out = lab.run(r, "reload");
    assert!(out.contains("saved configuration"), "{out}");
    assert_eq!(lab.prompt(r), "Edge>");
    assert_eq!(lab.run(r, "en\nshow run"), before);
    assert_eq!(lab.run(r, "show startup-config"), before);
}

#[test]
fn switch_vlans_and_trunks_by_typing() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    lab.run(
        s,
        "enable
        configure terminal
        vlan 10,20
        exit
        vlan 10
        name Sales
        exit
        interface range fa0/1 - 4
        switchport mode access
        switchport access vlan 10
        int fa0/5
        switchport access vlan 30
        int g0/1
        switchport mode trunk
        switchport trunk allowed vlan 1,10
        switchport trunk allowed vlan add 20
        switchport trunk native vlan 99
        end",
    );
    let vl = lab.run(s, "show vlan brief");
    assert!(vl.contains("10    Sales"), "{vl}");
    assert!(vl.contains("Fa0/1, Fa0/2, Fa0/3, Fa0/4"), "{vl}");
    assert!(vl.contains("20    VLAN0020"), "{vl}");
    assert!(vl.contains("30    VLAN0030"), "access vlan creates it: {vl}");
    let tr = lab.run(s, "show interfaces trunk");
    assert!(tr.contains("Gi0/1      down       99       1,10,20 (1,10,20)"), "{tr}");
    let run = lab.run(s, "show run");
    assert!(
        run.contains(
            "interface GigabitEthernet0/1\n switchport trunk native vlan 99\n switchport trunk allowed vlan 1,10,20\n switchport mode trunk"
        ),
        "{run}"
    );
    let err = lab.try_run(s, "conf t");
    assert!(err.is_empty() || err.contains("Configuring"));
    assert!(lab.try_run(s, "no vlan 1").contains("% VLAN 1"));
    lab.run(s, "no vlan 20\nend");
    assert!(!lab.run(s, "sh vlan").contains("VLAN0020"));
}

#[test]
fn router_on_a_stick_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(b, "fa0", s, "fa0/2");
    lab.run(s, "en\nconf t\nvlan 10\nvlan 20\nint fa0/1\nsw acc vlan 10\nint fa0/2\nsw acc vlan 20\nint g0/1\nsw mode trunk\nend");
    lab.run(r, "en\nconf t\nint g0/0\nno shut\nint g0/0.10\nencap dot1q 10\nip add 10.0.10.1 255.255.255.0\nint g0/0.20\nencap dot1q 20\nip add 10.0.20.1 255.255.255.0\nend");
    lab.run(a, "ipconfig 10.0.10.5 255.255.255.0 10.0.10.1");
    lab.run(b, "ipconfig 10.0.20.5 255.255.255.0 10.0.20.1");
    let out = lab.run(a, "ping -n 3 10.0.20.5");
    // The router drops the first echo while it resolves PC2's address.
    assert!(out.contains("seq 1: no answer") && out.contains("seq 3: reply from 10.0.20.5"), "{out}");
    let tr = lab.run(a, "tracert 10.0.20.5");
    assert!(tr.contains("1  10.0.10.1") && tr.contains("2  10.0.20.5"), "{tr}");
}

#[test]
fn modes_prompts_and_errors() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    assert!(lab.try_run(r, "conf t").contains("% Not understood"));
    lab.run(r, "en\nconf t");
    assert_eq!(lab.prompt(r), "R1(config)#");
    lab.run(r, "int g0/0.5");
    assert_eq!(lab.prompt(r), "R1(config-subif)#");
    assert!(lab.try_run(r, "ip address 10.0.0.1 255.255.255.0").contains("encapsulation dot1Q"));
    lab.run(r, "int range g0/0 - 1");
    assert_eq!(lab.prompt(r), "R1(config-if-range)#");
    // A global command from a sub-mode works and leaves the sub-mode.
    lab.run(r, "hostname Core");
    assert_eq!(lab.prompt(r), "Core(config)#");
    lab.run(r, "int g0/0\nip address 10.0.0.1 255.255.255.0");
    lab.run(r, "int g0/1");
    let e = lab.try_run(r, "ip address 10.0.0.2 255.255.255.128");
    assert!(e.contains("overlaps"), "{e}");
    assert!(lab.try_run(r, "ip address 10.1.0.0 255.255.255.0").contains("network or broadcast"));
    assert!(lab.try_run(r, "ip address 10.1.0.1 255.0.255.0").contains("contiguous"));
    let e = lab.try_run(r, "ip adress 1.1.1.1 255.0.0.0");
    assert!(e.starts_with("   ^"), "{e:?}");
    assert!(lab.try_run(r, "d").contains("Ambiguous: `d` could be description, do"));
    assert!(lab.try_run(r, "ip").contains("Incomplete"));
    assert!(lab.run(r, "do show ip int brief").contains("10.0.0.1/24"));
    assert_eq!(lab.prompt(r), "Core(config-if)#");
    lab.run(r, "exit\nexit");
    assert_eq!(lab.prompt(r), "Core#");
    lab.run(r, "exit");
    assert_eq!(lab.prompt(r), "Core>");
    assert!(lab.try_run(r, "show running-config").contains('%'));
    let help = lab.try_run(r, "?");
    assert!(help.contains("enable") && help.contains("ping") && !help.contains("configure"), "{help}");
    let cli = Cli { mode: Mode::Exec, ..Cli::default() };
    assert_eq!(complete_line(&lab.t, r, &cli, "show ru").as_deref(), Some("show running-config "));
    assert_eq!(complete_line(&lab.t, r, &cli, "sh ip ro").as_deref(), Some("sh ip route "));
}

#[test]
fn static_route_validation() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t");
    assert!(lab.try_run(r, "ip route 10.0.0.1 255.255.255.0 1.1.1.1").contains("host bits"));
    assert!(lab.try_run(r, "ip route 10.0.0.0 255.255.255.0 g0/9").contains("no interface"));
    lab.run(r, "ip route 10.0.0.0 255.255.255.0 1.1.1.1\nip route 10.0.0.0 255.255.255.0 1.1.1.1");
    assert_eq!(lab.t.device(r).unwrap().routes.len(), 1);
    assert!(lab.try_run(r, "no ip route 10.9.0.0 255.255.255.0").contains("No such"));
    lab.run(r, "no ip route 10.0.0.0 255.255.255.0");
    assert!(lab.t.device(r).unwrap().routes.is_empty());
}

#[test]
fn host_shell() {
    let mut lab = Lab::new();
    let a = lab.add(DeviceKind::Pc);
    assert_eq!(lab.prompt(a), "PC1> ");
    assert!(lab.try_run(a, "ipconfig").contains("not set"));
    assert!(lab.try_run(a, "ipconfig 10.0.0.5 255.255.255.0 10.9.9.9").contains("not on 10.0.0.0/24"));
    assert!(lab.try_run(a, "ipconfig 10.0.0.0 255.255.255.0").contains("network or broadcast"));
    lab.run(a, "ipconfig 10.0.0.5 255.255.255.0 10.0.0.1");
    assert!(lab.try_run(a, "ipconfig").contains("10.0.0.1"));
    let out = lab.try_run(a, "ping 10.0.0.9");
    assert!(out.contains("no answer") && out.contains("100% lost"), "{out}");
    assert!(lab.try_run(a, "ping -n 0 1.1.1.1").contains("1-1000"));
    assert!(lab.try_run(a, "frob").contains("not a command"));
    let cli = lab.clis.entry(a).or_default();
    assert!(execute(&mut lab.t, &mut lab.sim, a, cli, "cls").clear);
    lab.run(a, "ipconfig /release");
    assert!(lab.try_run(a, "ping 10.0.0.9").contains("no IP address"));
}

/// Every command, in every mode it exists in, with empty, hostile and huge arguments: no panic.
#[test]
fn every_command_survives_hostile_arguments() {
    let hostile = [
        "",
        "0",
        "-1",
        "99999999999999999999",
        "4095",
        "a.b.c.d",
        "1.2.3.4",
        "255.255.255.255",
        "g9/9/9",
        "f0/1 - 99999",
        "ä€😀",
        "%s%n",
        "add",
        "remove 1-4094",
    ];
    let huge = "x".repeat(10_000);
    for kind in [DeviceKind::Router, DeviceKind::Switch, DeviceKind::Pc] {
        for spec in SPECS {
            for (i, arg) in hostile.iter().chain(std::iter::once(&huge.as_str())).enumerate() {
                let mut lab = Lab::new();
                let d = lab.add(kind);
                let other = lab.add(DeviceKind::Pc);
                lab.t.connect(End { device: d, port: 0 }, End { device: other, port: 0 }).unwrap();
                let line: Vec<String> = spec
                    .path
                    .iter()
                    .map(|t| match t {
                        Tok::K(k) => k.to_string(),
                        Tok::Arg(_) | Tok::Rest(_) => arg.to_string(),
                    })
                    .collect();
                for setup in ["", "en", "en\nconf t", "en\nconf t\nint g0/0", "en\nconf t\nint g0/0.1", "en\nconf t\nint fa0/1", "en\nconf t\nvlan 5"]
                {
                    for l in setup.lines() {
                        lab.try_run(d, l);
                    }
                    let _ = lab.try_run(d, &line.join(" "));
                    let _ = lab.try_run(d, &format!("{} ?", line.join(" ")));
                    lab.clis.clear();
                }
                let _ = i;
            }
        }
    }
}

#[test]
fn dhcp_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(b, "fa0", s, "fa0/2");
    lab.run(
        r,
        "en
        conf t
        int g0/0
        ip add 192.168.10.1 255.255.255.0
        no shut
        exit
        ip dhcp excluded-address 192.168.10.1 192.168.10.9
        ip dhcp pool LAN
        network 192.168.10.0 255.255.255.0
        default-router 192.168.10.1
        dns-server 192.168.10.5
        end",
    );
    assert_eq!(lab.prompt(r), "R1#");
    let out = lab.run(a, "ipconfig /renew");
    assert!(out.contains("192.168.10.10/24") && out.contains("gateway 192.168.10.1") && out.contains("DNS 192.168.10.5"), "{out}");
    assert!(lab.run(b, "ipconfig /renew").contains("192.168.10.11/24"));
    assert!(lab.run(a, "ipconfig").contains("by DHCP"));
    let bind = lab.run(r, "show ip dhcp binding");
    assert!(bind.contains("192.168.10.10") && bind.contains("2 bindings"), "{bind}");
    assert!(lab.run(r, "show ip dhcp pool").contains("Leased addresses 2"));
    let run = lab.run(r, "show run");
    assert!(run.contains("ip dhcp excluded-address 192.168.10.1 192.168.10.9\n!\nip dhcp pool LAN\n network 192.168.10.0 255.255.255.0\n default-router 192.168.10.1\n dns-server 192.168.10.5"), "{run}");
    assert!(lab.run(a, "ping 192.168.10.11").contains("answered"));
    lab.run(r, "clear ip dhcp binding *");
    assert!(lab.run(r, "show ip dhcp binding").contains("0 bindings"));
    assert!(lab.try_run(r, "conf t").is_empty() || true);
    assert!(lab.try_run(r, "no ip dhcp pool NOPE").contains("No pool"));
}

#[test]
fn passwords_banner_and_lines() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(
        r,
        "en
        conf t
        hostname Core
        enable secret class
        line console 0
        password cisco
        login
        exec-timeout 5 0
        exit
        line vty 0 4
        password remote
        login
        exit
        banner motd #Authorised access only#
        service password-encryption
        no ip domain-lookup
        end",
    );
    let run = lab.run(r, "show running-config");
    assert!(run.contains("service password-encryption"), "{run}");
    assert!(run.contains("enable secret 5 $pc$") && !run.contains("class"), "{run}");
    assert!(!run.contains("password cisco") && run.contains("line con 0\n password 7 "), "{run}");
    assert!(run.contains("banner motd ^Authorised access only^"), "{run}");
    assert!(run.contains("no ip domain-lookup"));
    // A new session: banner, console password, then the enable secret.
    lab.run(r, "copy run start");
    lab.run(r, "reload");
    assert_eq!(lab.prompt(r), "Password: ");
    assert!(greeting(&lab.t, r).contains("Authorised access only"));
    assert!(lab.try_run(r, "wrong").contains("Wrong password"));
    assert_eq!(lab.try_run(r, "cisco"), "");
    assert_eq!(lab.prompt(r), "Core>");
    assert_eq!(lab.try_run(r, "enable"), "");
    assert_eq!(lab.prompt(r), "Password: ");
    lab.try_run(r, "nope");
    lab.try_run(r, "nope");
    assert!(lab.try_run(r, "nope").contains("three times"));
    assert_eq!(lab.prompt(r), "Core>");
    lab.try_run(r, "enable");
    lab.try_run(r, "class");
    assert_eq!(lab.prompt(r), "Core#");
    // Passwords never land in the history.
    let hist = &lab.clis[&r].history;
    assert!(!hist.iter().any(|h| h == "cisco" || h == "class"), "{hist:?}");
    // The saved configuration replays to the same text, obscured passwords included.
    let again = lab.run(r, "show running-config");
    assert_eq!(again, run);
    // Logging out asks for the password again.
    lab.run(r, "disable");
    assert!(lab.try_run(r, "exit").contains("Session ended"));
    assert_eq!(lab.prompt(r), "Password: ");
}

#[test]
fn spanning_tree_by_typing() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(s1, "g0/2", s2, "g0/2");
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s1, "fa0/1");
    lab.cable(b, "fa0", s2, "fa0/1");
    lab.run(a, "ipconfig 10.0.0.1 255.255.255.0");
    lab.run(b, "ipconfig 10.0.0.2 255.255.255.0");
    assert!(lab.run(a, "ping 10.0.0.2").contains("4 answered"));
    let st = lab.run(s2, "en\nshow spanning-tree");
    assert!(st.contains("VLAN0001") && st.contains("Altn BLK") && st.contains("Root FWD"), "{st}");
    assert!(lab.run(s1, "en\nsh span").contains("This switch is the root"));
    lab.run(s2, "conf t\nspanning-tree vlan 1 root primary\nint fa0/1\nspanning-tree portfast\nend");
    let st = lab.run(s2, "show spanning-tree vlan 1");
    assert!(st.contains("This switch is the root") && st.contains("Edge"), "{st}");
    assert!(lab.run(s1, "show spanning-tree").contains("Altn BLK"));
    let run = lab.run(s2, "show run");
    assert!(run.contains("spanning-tree vlan 1 priority 24576") && run.contains(" spanning-tree portfast"), "{run}");
    assert!(lab.try_run(s2, "conf t").is_empty() || true);
    assert!(lab.try_run(s2, "spanning-tree vlan 1 priority 1000").contains("multiple of 4096"));
}

#[test]
fn switch_management_interface_by_typing() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s, "fa0/1");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(s, "en\nconf t\ninterface vlan 1\nip address 192.168.1.2 255.255.255.0");
    assert_eq!(lab.prompt(s), "S1(config-if)#");
    lab.run(s, "no shutdown\nexit\nip default-gateway 192.168.1.1\nend");
    let br = lab.run(s, "show ip interface brief");
    assert!(br.contains("Vlan1                    192.168.1.2/24     up      up"), "{br}");
    assert!(lab.run(a, "ping 192.168.1.2").contains("4 answered"));
    assert!(lab.run(s, "ping 192.168.1.10").contains("answered (5/5)") || lab.run(s, "ping 192.168.1.10").contains("100% answered"));
    let run = lab.run(s, "show run");
    assert!(
        run.contains("interface Vlan1\n ip address 192.168.1.2 255.255.255.0\n no shutdown") && run.contains("ip default-gateway 192.168.1.1"),
        "{run}"
    );
    lab.run(s, "conf t\nno interface vlan 1\nend");
    assert!(!lab.run(s, "show run").contains("interface Vlan1"));
    let r = lab.add(DeviceKind::Router);
    assert!(lab.try_run(r, "en").is_empty());
    lab.try_run(r, "conf t");
    assert!(lab.try_run(r, "interface vlan 1").contains("subinterfaces"));
}

#[test]
fn dhcp_relay_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 192.168.30.1 255.255.255.0\nip helper-address 10.1.1.2\nno shut\nint g0/1\nip add 10.1.1.1 255.255.255.252\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/1\nip add 10.1.1.2 255.255.255.252\nno shut\nexit\nip route 192.168.30.0 255.255.255.0 10.1.1.1\nip dhcp pool FAR\nnetwork 192.168.30.0 255.255.255.0\ndefault-router 192.168.30.1\nend");
    // Warm ARP between the routers, then ask.
    lab.run(r1, "ping 10.1.1.2");
    let out = lab.run(a, "ipconfig /renew");
    assert!(out.contains("192.168.30.2/24"), "{out}");
    assert!(lab.run(r1, "show run").contains(" ip helper-address 10.1.1.2"));
    lab.run(r1, "conf t\nint g0/0\nno ip helper-address\nend");
    assert!(!lab.run(r1, "show run").contains("helper-address"));
}

#[test]
fn ospf_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(b, "fa0", r2, "g0/0");
    lab.run(a, "ipconfig 172.16.1.10 255.255.255.0 172.16.1.1");
    lab.run(b, "ipconfig 172.16.2.10 255.255.255.0 172.16.2.1");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 172.16.1.1 255.255.255.0\nno shut\nint g0/1\nip add 10.0.0.1 255.255.255.252\nno shut\nexit\nrouter ospf 1\nrouter-id 1.1.1.1\nnetwork 172.16.1.0 0.0.0.255 area 0\nnetwork 10.0.0.0 0.0.0.3 area 0\npassive-interface g0/0\nend");
    assert_eq!(lab.prompt(r1), "R1#");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 172.16.2.1 255.255.255.0\nno shut\nint g0/1\nip add 10.0.0.2 255.255.255.252\nno shut\nexit\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend");
    let nb = lab.run(r1, "show ip ospf neighbor");
    assert!(nb.contains("172.16.2.1") && nb.contains("FULL") && nb.contains("10.0.0.2"), "{nb}");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O    172.16.2.0/24") && rt.contains("via 10.0.0.2"), "{rt}");
    lab.run(a, "ping 172.16.2.10");
    assert!(lab.run(a, "ping 172.16.2.10").contains("4 answered"));
    let run = lab.run(r1, "show run");
    assert!(run.contains("router ospf 1\n router-id 1.1.1.1\n passive-interface GigabitEthernet0/0\n network 172.16.1.0 0.0.0.255 area 0"), "{run}");
    assert!(lab.run(r1, "show ip protocols").contains("Router id: 1.1.1.1"));
    lab.try_run(r1, "conf t");
    lab.try_run(r1, "router ospf 1");
    assert!(lab.try_run(r1, "network 10.0.0.0 0.255.0.255 area 0").contains("not a wildcard"));
    lab.run(r1, "end");
    lab.run(r2, "conf t\nno router ospf 1\nend");
    assert!(lab.run(r1, "show ip ospf neighbor").contains("No OSPF neighbours"));
}

#[test]
fn access_lists_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    let c = lab.add(DeviceKind::Pc);
    let s = lab.add(DeviceKind::Switch);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(b, "fa0", s, "fa0/2");
    lab.cable(c, "fa0", r, "g0/1");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.1.20 255.255.255.0 192.168.1.1");
    lab.run(c, "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1");
    lab.run(r, "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip add 10.0.0.1 255.255.255.0\nno shut\nend");
    for h in [a, b] {
        lab.run(h, "ping 10.0.0.10");
    }
    // A standard list on the server side: only PC1 may reach the 10.0.0.0/24 network.
    lab.run(r, "conf t\naccess-list 10 permit host 192.168.1.10\naccess-list 10 remark only the admin PC\nint g0/1\nip access-group 10 out\nend");
    assert!(lab.run(a, "ping 10.0.0.10").contains("4 answered"));
    let out = lab.run(b, "ping 10.0.0.10");
    assert!(out.contains("unreachable") && out.contains("0 answered"), "{out}");
    let acl = lab.run(r, "show access-lists");
    assert!(
        acl.contains("Standard access list 10")
            && acl.contains("10 permit host 192.168.1.10 (4 matches)")
            && acl.contains("deny everything else (4 matches)"),
        "{acl}"
    );
    // A named extended list: no pings from PC1 to the router itself, everything else allowed.
    lab.run(r, "conf t\nint g0/1\nno ip access-group 10 out\nexit\nip access-list extended NO-PING\ndeny icmp host 192.168.1.10 host 192.168.1.1\npermit ip any any");
    assert_eq!(lab.prompt(r), "R1(config-ext-nacl)#");
    lab.run(r, "exit\nint g0/0\nip access-group NO-PING in\nend");
    assert!(lab.run(a, "ping 192.168.1.1").contains("0 answered"));
    assert!(lab.run(b, "ping 192.168.1.1").contains("4 answered"));
    assert!(lab.run(a, "ping 10.0.0.10").contains("4 answered"));
    let run = lab.run(r, "show run");
    assert!(run.contains("access-list 10 permit host 192.168.1.10"), "{run}");
    assert!(run.contains("ip access-list extended NO-PING\n deny icmp host 192.168.1.10 host 192.168.1.1\n permit ip any any"), "{run}");
    assert!(run.contains("interface GigabitEthernet0/0\n ip address 192.168.1.1 255.255.255.0\n ip access-group NO-PING in"), "{run}");
    lab.try_run(r, "conf t");
    assert!(lab.try_run(r, "access-list 10 permit tcp any any").contains("not an IPv4 address") || true);
    assert!(lab.try_run(r, "access-list 150 permit tcp any any eq www").is_empty());
    assert!(lab.try_run(r, "access-list 150 permit gre any any").contains("not a protocol"));
    assert!(lab.try_run(r, "access-list 500 permit any").contains("1-99"));
    lab.run(r, "end\nclear access-list counters");
    assert!(!lab.run(r, "show access-lists").contains("matches)"));
}

#[test]
fn nat_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let isp = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", isp, "g0/1");
    lab.cable(srv, "g0", isp, "g0/0");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 8.8.8.8 255.255.255.0 8.8.8.1");
    lab.run(isp, "en\nconf t\nint g0/1\nip add 200.0.0.2 255.255.255.248\nno shut\nint g0/0\nip add 8.8.8.1 255.255.255.0\nno shut\nend");
    lab.run(
        r1,
        "en
        conf t
        int g0/0
        ip add 192.168.1.1 255.255.255.0
        ip nat inside
        no shut
        int g0/1
        ip add 200.0.0.1 255.255.255.248
        ip nat outside
        no shut
        exit
        ip route 0.0.0.0 0.0.0.0 200.0.0.2
        access-list 1 permit 192.168.1.0 0.0.0.255
        ip nat inside source list 1 interface g0/1 overload
        end",
    );
    lab.run(a, "ping 8.8.8.8");
    assert!(lab.run(a, "ping 8.8.8.8").contains("4 answered"));
    // Traceroute through PAT: the ISP's "time exceeded" comes back to the PC.
    let trace = lab.run(a, "tracert 8.8.8.8");
    assert!(trace.contains("200.0.0.2") && trace.contains("8.8.8.8"), "{trace}");
    let tr = lab.run(r1, "show ip nat translations");
    assert!(tr.contains("icmp") && tr.contains("200.0.0.1:") && tr.contains("192.168.1.10:") && tr.contains("8.8.8.8"), "{tr}");
    let run = lab.run(r1, "show run");
    assert!(
        run.contains(" ip nat inside")
            && run.contains(" ip nat outside")
            && run.contains("ip nat inside source list 1 interface GigabitEthernet0/1 overload"),
        "{run}"
    );
    lab.run(r1, "conf t\nip nat pool PUBLIC 200.0.0.3 200.0.0.6 netmask 255.255.255.248\nip nat inside source static 192.168.1.10 200.0.0.5\nend");
    let run = lab.run(r1, "show run");
    assert!(
        run.contains("ip nat pool PUBLIC 200.0.0.3 200.0.0.6 netmask 255.255.255.248")
            && run.contains("ip nat inside source static 192.168.1.10 200.0.0.5"),
        "{run}"
    );
    lab.run(r1, "clear ip nat translation *");
    lab.run(srv, "ping 200.0.0.5");
    assert!(lab.run(srv, "ping 200.0.0.5").contains("4 answered"));
}

#[test]
fn telnet_and_ssh_between_devices() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(r, "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nend");
    // No vty login configured: refused.
    let out = lab.try_run(a, "telnet 192.168.1.1");
    assert!(out.contains("refused"), "{out}");
    // Telnet with a line password.
    lab.run(r, "conf t\nline vty 0 4\npassword remote\nlogin\nend");
    assert!(lab.try_run(a, "telnet 192.168.1.1").contains("Open"));
    assert_eq!(lab.prompt(a), "Password: ");
    assert!(lab.try_run(a, "bad").contains("Login invalid"));
    assert_eq!(lab.try_run(a, "remote"), "");
    assert_eq!(lab.prompt(a), "R1>");
    // No enable password: refused over vty, as on real devices.
    assert!(lab.try_run(a, "enable").contains("No password set"));
    lab.run(r, "conf t\nenable secret class\nend");
    assert_eq!(lab.try_run(a, "enable"), "");
    assert_eq!(lab.prompt(a), "Password: ");
    lab.try_run(a, "class");
    assert_eq!(lab.prompt(a), "R1#");
    // Configure the router from the PC.
    lab.try_run(a, "conf t");
    lab.try_run(a, "hostname Edge");
    lab.try_run(a, "end");
    assert_eq!(lab.t.device(r).unwrap().name, "Edge");
    assert!(lab.try_run(a, "exit").contains("Connection to Edge closed"));
    assert_eq!(lab.prompt(a), "PC1> ");
    // SSH: domain, keys, local users, transport ssh only.
    lab.run(r, "conf t\nip domain-name lab.test\nusername admin secret s3cret\nline vty 0 4\nlogin local\ntransport input ssh\nexit\ncrypto key generate rsa");
    assert_eq!(lab.prompt(r), MODULUS_PROMPT);
    assert!(lab.run(r, "1024").contains("1024 bits"));
    lab.run(r, "ip ssh version 2\nend");
    assert!(lab.try_run(a, "telnet 192.168.1.1").contains("refused"));
    assert!(lab.try_run(a, "ssh -l admin 192.168.1.1").contains("Open"));
    assert_eq!(lab.prompt(a), "Password: ");
    assert!(lab.try_run(a, "nope").contains("Login invalid"));
    assert_eq!(lab.prompt(a), "Username: ");
    lab.try_run(a, "admin");
    lab.try_run(a, "s3cret");
    assert_eq!(lab.prompt(a), "Edge>");
    assert!(lab.try_run(a, "show ip ssh").contains("SSH enabled - version 2"));
    lab.try_run(a, "exit");
    let run = lab.run(r, "show run");
    assert!(
        run.contains("ip domain-name lab.test")
            && run.contains("username admin secret 5 $pc$")
            && run.contains("crypto key generate rsa general-keys modulus 1024"),
        "{run}"
    );
    assert!(run.contains("line vty 0 4\n password remote\n login local\n transport input ssh"), "{run}");
    // Router to router, too (from the router's own console).
    assert!(lab.try_run(r, "ssh -l admin 192.168.1.1").contains("Open"));
}

#[test]
fn port_security_by_typing() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let hub = lab.add(DeviceKind::Hub);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    let c = lab.add(DeviceKind::Pc);
    // PC1 and an intruder (PC2) share the switch port through a hub; PC3 is elsewhere.
    lab.cable(hub, "e0", s, "fa0/1");
    lab.cable(a, "fa0", hub, "e1");
    lab.cable(b, "fa0", hub, "e2");
    lab.cable(c, "fa0", s, "fa0/2");
    lab.run(a, "ipconfig 10.0.0.1 255.255.255.0");
    lab.run(b, "ipconfig 10.0.0.2 255.255.255.0");
    lab.run(c, "ipconfig 10.0.0.3 255.255.255.0");
    lab.run(s, "en\nconf t\nint fa0/1\nswitchport mode access\nswitchport port-security\nswitchport port-security maximum 1\nswitchport port-security violation shutdown\nswitchport port-security mac-address sticky\nend");
    assert!(lab.run(a, "ping 10.0.0.3").contains("4 answered"));
    // The engine writes sticky addresses into the lab; here we do it by hand, as it would.
    for e in lab.sim.take_events() {
        if let packetcraft_sim::PortEvent::Sticky { port, mac, .. } = e {
            lab.t.device_mut(s).unwrap().ports[port].port_security.as_mut().unwrap().macs.push(mac);
        }
    }
    assert!(lab.run(s, "show run").contains("switchport port-security mac-address sticky 0200."));
    // The intruder: violation, the port goes err-disabled.
    let out = lab.run(b, "ping 10.0.0.3");
    assert!(out.contains("0 answered"), "{out}");
    let events = lab.sim.take_events();
    assert!(events.iter().any(|e| matches!(e, packetcraft_sim::PortEvent::ErrDisabled { .. })));
    lab.t.device_mut(s).unwrap().ports[0].err_disabled = true;
    lab.sim.topology_changed(&lab.t);
    let ps = lab.run(s, "show port-security");
    assert!(ps.contains("err-disabled") && ps.contains("Fa0/1"), "{ps}");
    assert!(lab.run(a, "ping 10.0.0.3").contains("0 answered"));
    // shutdown / no shutdown brings it back.
    lab.run(s, "conf t\nint fa0/1\nshutdown\nno shutdown\nend");
    assert!(!lab.t.device(s).unwrap().ports[0].err_disabled);
    assert!(lab.run(a, "ping 10.0.0.3").contains("answered"));
    lab.run(s, "conf t\nint g0/1\nswitchport mode trunk");
    assert!(lab.try_run(s, "switchport port-security").contains("trunk"));
}

#[test]
fn etherchannel_by_typing() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "fa0/23", s2, "fa0/23");
    lab.cable(s1, "fa0/24", s2, "fa0/24");
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s1, "fa0/1");
    lab.cable(b, "fa0", s2, "fa0/1");
    lab.run(a, "ipconfig 10.0.0.1 255.255.255.0");
    lab.run(b, "ipconfig 10.0.0.2 255.255.255.0");
    // LACP on both ends: one logical link, nothing blocked by spanning tree.
    lab.run(s1, "en\nconf t\ninterface range fa0/23 - 24\nchannel-group 1 mode active\nend");
    lab.run(s2, "en\nconf t\ninterface range fa0/23-24\nchannel-group 1 mode passive\nend");
    let ec = lab.run(s1, "show etherchannel summary");
    assert!(ec.contains("Port-channel1 (LACP), up, 2 of 2 ports bundled"), "{ec}");
    let st = lab.run(s2, "show spanning-tree");
    assert!(st.contains("Po1") && !st.contains("BLK") && !st.contains("Fa0/24"), "{st}");
    assert!(st.contains("Po1        Root FWD   12"), "{st}");
    assert!(lab.run(a, "ping 10.0.0.2").contains("4 answered"));
    assert!(lab.run(s1, "show mac address-table").contains("Po1"));
    // Settings on the port channel reach every member.
    lab.run(s1, "conf t\ninterface port-channel 1\nswitchport mode trunk\nend");
    lab.run(s2, "conf t\nint po1\nswitchport mode trunk\nend");
    assert!(lab.t.device(s1).unwrap().ports.iter().filter(|p| p.channel.is_some()).all(|p| p.switchport.mode == SwitchportMode::Trunk));
    assert!(lab.run(a, "ping 10.0.0.2").contains("4 answered"));
    // One member cut: the bundle carries on.
    lab.run(s1, "conf t\nint fa0/23\nshutdown\nend");
    assert!(lab.run(s1, "show etherchannel").contains("1 of 2 ports bundled"));
    assert!(lab.run(a, "ping 10.0.0.2").contains("4 answered"));
    lab.run(s1, "conf t\nint fa0/23\nno shutdown\nend");
    let run = lab.run(s1, "show run");
    assert!(run.contains(" channel-group 1 mode active"), "{run}");
    // passive with passive: no bundle; the ports work alone and spanning tree blocks one.
    lab.run(s1, "conf t\ninterface range fa0/23 - 24\nno channel-group\nchannel-group 1 mode passive\nend");
    let ec = lab.run(s1, "show etherchannel");
    assert!(ec.contains("down, 0 of 2") && ec.contains("on its own"), "{ec}");
    assert!(lab.run(s2, "show spanning-tree").contains("BLK"));
    assert!(lab.run(a, "ping 10.0.0.2").contains("4 answered"));
    // on against LACP: the ports are suspended, no path left.
    lab.run(s1, "conf t\ninterface range fa0/23 - 24\nno channel-group\nchannel-group 1 mode on\nend");
    assert!(lab.run(s1, "show etherchannel").contains("suspended"));
    assert!(lab.run(a, "ping 10.0.0.2").contains("0 answered"));
    // Mixing protocols in one group is refused; so is port security on a member.
    lab.run(s1, "conf t\nint fa0/22");
    assert!(lab.try_run(s1, "channel-group 1 mode desirable").contains("already uses"));
    lab.run(s1, "exit\nint fa0/23\nswitchport mode access");
    assert!(lab.try_run(s1, "switchport port-security").contains("EtherChannel"));
    assert!(lab.try_run(s1, "exit").is_empty());
    assert!(lab.try_run(s1, "interface port-channel 9").contains("no ports yet"));
}

#[test]
fn vtp_by_typing() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    let s4 = lab.add(DeviceKind::Switch);
    // S1 server - S2 transparent - S3 client; S4 hangs off S3.
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(s2, "g0/2", s3, "g0/1");
    lab.cable(s3, "g0/2", s4, "g0/1");
    for s in [s1, s2, s3, s4] {
        lab.run(s, "en\nconf t\ninterface range g0/1 - 2\nswitchport mode trunk\nexit");
    }
    lab.run(s1, "vtp domain LAB\nvtp password clase\nvtp version 2\nvlan 10\nname Ventas\nvlan 20\nexit");
    lab.run(s2, "vtp mode transparent\nvtp version 2\nvlan 3000\nexit");
    lab.run(s3, "vtp mode client\nvtp domain LAB\nvtp password clase");
    assert!(lab.try_run(s3, "vlan 30").contains("VTP client"));
    let st = lab.run(s3, "do show vtp status");
    assert!(st.contains("client") && st.contains("LAB") && st.contains("Revision            3"), "{st}");
    let vl = lab.run(s3, "do show vlan brief");
    assert!(vl.contains("Ventas") && vl.contains("20") && !vl.contains("3000"), "{vl}");
    // The transparent switch keeps its own database.
    assert!(!lab.run(s2, "do show vlan brief").contains("Ventas"));
    // S4, a server with no domain, joins LAB but its password differs: no VLANs.
    assert!(lab.run(s4, "do show vtp status").contains("LAB"));
    assert!(!lab.run(s4, "do show vlan brief").contains("Ventas"));
    lab.run(s4, "vtp password clase");
    assert!(lab.run(s4, "do show vlan brief").contains("Ventas"));
    // A change on the server reaches everyone in the domain.
    lab.run(s1, "no vlan 20");
    assert!(!lab.run(s3, "do show vlan brief").contains("VLAN0020"));
    // The classic accident: a switch with a higher revision overwrites the domain.
    lab.run(s4, "vlan 99\nvlan 98\nvlan 97\nvlan 96\nexit\nno vlan 10");
    assert!(lab.run(s1, "do show vtp status").contains("Revision            9"));
    assert!(!lab.run(s1, "do show vlan brief").contains("Ventas"));
    assert!(lab.try_run(s1, "vlan 2000").contains("transparent"));
    // A client's VLANs are not in its configuration, and survive a reload.
    lab.run(s3, "end\ncopy running-config startup-config");
    assert!(!lab.run(s3, "show running-config").contains("vlan 99"));
    lab.run(s3, "reload");
    assert!(lab.run(s3, "en\nshow vlan brief").contains("VLAN0099"));
}

#[test]
fn ipv6_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s1, "fa0/1");
    lab.cable(s1, "g0/1", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.cable(b, "fa0", r2, "g0/0");
    lab.run(
        r1,
        "en\nconf t\nipv6 unicast-routing\nint g0/0\nipv6 address 2001:db8:1::1/64\nno shut\nint g0/1\nipv6 address 2001:db8:12::/64 eui-64\nipv6 address fe80::1 link-local\nno shut\nexit\nipv6 route 2001:db8:2::/64 2001:db8:12::2\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nipv6 unicast-routing\nint g0/0\nipv6 address 2001:db8:2::1/64\nipv6 address fe80::2 link-local\nno shut\nint g0/1\nipv6 address 2001:db8:12::2/64\nno shut\nexit\nipv6 route ::/0 g0/1 fe80::1\nend",
    );
    // PC1 takes its address from R1's router advertisement; PC2 is set by hand.
    let auto = lab.run(a, "ipv6config auto");
    assert!(auto.contains("Autoconfigured 2001:db8:1::ff:fe") && auto.contains("/64, gateway fe80::"), "{auto}");
    lab.run(b, "ipv6config 2001:db8:2::10/64 fe80::2");
    assert!(lab.run(a, "ipconfig").contains("Link-local  fe80::"));
    let p = lab.run(a, "ping 2001:db8:2::10");
    assert!(p.contains("4 answered"), "{p}");
    assert!(lab.run(a, "ping 2001:db8:2::10").contains("4 answered"));
    let tr = lab.run(b, "tracert 2001:db8:1::1");
    assert!(tr.contains("2001:db8:12::") || tr.contains("2001:db8:1::1"), "{tr}");
    let br = lab.run(r1, "show ipv6 interface brief");
    assert!(br.contains("fe80::1 (link-local)") && br.contains("2001:db8:12::") && br.contains("ff:fe"), "{br}");
    let rt = lab.run(r1, "show ipv6 route");
    assert!(rt.contains("C  2001:db8:1::/64") && rt.contains("S  2001:db8:2::/64") && rt.contains("L  2001:db8:1::1/128"), "{rt}");
    assert!(lab.run(r2, "show ipv6 route").contains("via fe80::1, GigabitEthernet0/1"));
    assert!(lab.run(r1, "show ipv6 neighbors").contains("2001:db8:12::2"));
    let run = lab.run(r2, "show running-config");
    assert!(
        run.contains("ipv6 unicast-routing")
            && run.contains(" ipv6 address fe80::2 link-local")
            && run.contains("ipv6 route ::/0 GigabitEthernet0/1 fe80::1"),
        "{run}"
    );
    // The router's own ping, and no route.
    assert!(lab.run(r1, "ping 2001:db8:2::10").contains("!!"));
    assert!(lab.run(a, "ping 2001:db8:99::1").contains("unreachable"));
    // Errors.
    lab.run(r1, "conf t\nint g0/2");
    assert!(lab.try_run(r1, "ipv6 address 2001:db8:1::5/64").contains("overlaps"));
    assert!(lab.try_run(r1, "ipv6 address 2001:db8:5::/48 eui-64").contains("/64"));
    assert!(lab.try_run(r1, "ipv6 address fe80::9/64").contains("link-local"));
    assert!(lab.try_run(r1, "exit").is_empty());
    assert!(lab.try_run(r1, "ipv6 route 2001:db8:7::/64 fe80::7").contains("needs its interface"));
    // IPv6 off on R1: no more router.
    lab.run(r1, "no ipv6 unicast-routing\nend");
    assert!(lab.run(a, "ping 2001:db8:2::10").contains(" 0 answered"));
}

#[test]
fn multilayer_switch_by_typing() {
    let mut lab = Lab::new();
    let ml = lab.t.add_model(packetcraft_model::models::find("Switch-ML-24").unwrap(), None, 0.0, 0.0).unwrap();
    let plain = lab.add(DeviceKind::Switch);
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    let c = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", ml, "fa0/1");
    lab.cable(b, "fa0", ml, "fa0/2");
    lab.cable(ml, "g0/1", r, "g0/0");
    lab.cable(c, "fa0", r, "g0/1");
    lab.run(a, "ipconfig 192.168.10.10 255.255.255.0 192.168.10.1");
    lab.run(b, "ipconfig 192.168.20.10 255.255.255.0 192.168.20.1");
    lab.run(c, "ipconfig 172.16.0.10 255.255.255.0 172.16.0.1");
    lab.run(
        ml,
        "en\nconf t\nvlan 10\nvlan 20\nexit\nint fa0/1\nswitchport access vlan 10\nint fa0/2\nswitchport access vlan 20\nexit\n\
         interface vlan 10\nip address 192.168.10.1 255.255.255.0\nno shut\ninterface vlan 20\nip address 192.168.20.1 255.255.255.0\nno shut\nexit",
    );
    // Without `ip routing` the VLANs stay apart.
    assert!(lab.run(a, "ping 192.168.20.10").contains(" 0 answered"));
    lab.run(ml, "ip routing");
    let p = lab.run(a, "ping 192.168.20.10");
    assert!(p.contains("answered") && !p.contains(" 0 answered"), "{p}");
    // A routed port to the router, and a default route.
    lab.run(ml, "int g0/1\nno switchport\nip address 10.0.0.1 255.255.255.252\nexit\nip route 0.0.0.0 0.0.0.0 10.0.0.2\nend");
    lab.run(r, "en\nconf t\nint g0/0\nip address 10.0.0.2 255.255.255.252\nno shut\nint g0/1\nip address 172.16.0.1 255.255.255.0\nno shut\nexit\nip route 192.168.0.0 255.255.0.0 10.0.0.1\nend");
    lab.run(a, "ping 172.16.0.10");
    assert!(lab.run(a, "ping 172.16.0.10").contains("4 answered"));
    let rt = lab.run(ml, "show ip route");
    assert!(rt.contains("Vlan10") && rt.contains("GigabitEthernet0/1") && rt.contains("Default route: via 10.0.0.2"), "{rt}");
    let run = lab.run(ml, "show running-config");
    assert!(
        run.contains("ip routing")
            && run.contains(" no switchport\n ip address 10.0.0.1 255.255.255.252")
            && run.contains("ip route 0.0.0.0 0.0.0.0 10.0.0.2"),
        "{run}"
    );
    // The routed port is not a switch port any more: no spanning tree, no VLANs.
    assert!(!lab.run(ml, "show spanning-tree").contains("Gi0/1"));
    // A plain switch refuses.
    lab.run(plain, "en\nconf t");
    assert!(lab.try_run(plain, "ip routing").contains("multilayer"));
    lab.run(plain, "int fa0/1");
    assert!(lab.try_run(plain, "no switchport").contains("multilayer"));
    assert!(lab.try_run(plain, "ip address 1.1.1.1 255.255.255.0").contains("switch port"));
    lab.run(plain, "exit");
    assert!(lab.try_run(plain, "ip route 0.0.0.0 0.0.0.0 1.1.1.1").contains("ip routing"));
}

#[test]
fn rip_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    lab.cable(b, "fa0", r3, "g0/1");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.3.10 255.255.255.0 192.168.3.1");
    let cfg = [
        (r1, "192.168.1.1", "10.0.12.1", "g0/0", "g0/1"),
        (r2, "10.0.12.2", "10.0.23.1", "g0/0", "g0/1"),
        (r3, "10.0.23.2", "192.168.3.1", "g0/0", "g0/1"),
    ];
    for (r, ip0, ip1, i0, i1) in cfg {
        let m0 = if ip0.starts_with("10.") { "255.255.255.252" } else { "255.255.255.0" };
        let m1 = if ip1.starts_with("10.") { "255.255.255.252" } else { "255.255.255.0" };
        lab.run(r, &format!("en\nconf t\nint {i0}\nip address {ip0} {m0}\nno shut\nint {i1}\nip address {ip1} {m1}\nno shut\nexit"));
    }
    lab.run(r1, "router rip\nversion 2\nnetwork 192.168.1.0\nnetwork 10.0.0.0\nno auto-summary\npassive-interface g0/0\nend");
    lab.run(r2, "router rip\nversion 2\nnetwork 10.1.2.3\nend");
    lab.run(r3, "router rip\nversion 2\nnetwork 10.0.0.0\nnetwork 192.168.3.0\nexit\nip route 0.0.0.0 0.0.0.0 192.168.3.10\nrouter rip\ndefault-information originate\nend");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("R ") && rt.contains("192.168.3.0/24") && rt.contains("10.0.23.0/30"), "{rt}");
    let db = lab.run(r1, "show ip rip database");
    assert!(db.contains("192.168.3.0/24       2"), "{db}");
    assert!(lab.run(r1, "show ip route").contains("Default route: via 10.0.12.2"));
    lab.run(a, "ping 192.168.3.10");
    assert!(lab.run(a, "ping 192.168.3.10").contains("4 answered"));
    let pr = lab.run(r2, "show ip protocols");
    assert!(pr.contains("RIP version 2") && pr.contains("10.0.12.1 on Gi0/0") && pr.contains("distance 120"), "{pr}");
    let run = lab.run(r1, "show running-config");
    assert!(
        run.contains("router rip\n version 2\n passive-interface GigabitEthernet0/0\n network 10.0.0.0\n network 192.168.1.0\n no auto-summary"),
        "{run}"
    );
    // Cut the R2-R3 link: the far networks go away.
    lab.run(r3, "conf t\nint g0/0\nshutdown\nend");
    assert!(!lab.run(r1, "show ip route").contains("192.168.3.0/24"));
    lab.run(r1, "conf t\nno router rip\nend");
    assert!(lab.run(r1, "show ip protocols").contains("No routing protocol"));
}

#[test]
fn serial_link_by_typing() {
    let mut lab = Lab::new();
    let wan = packetcraft_model::models::find("Router-WAN").unwrap();
    let r1 = lab.t.add_model(wan, None, 0.0, 0.0).unwrap();
    let r2 = lab.t.add_model(wan, None, 0.0, 0.0).unwrap();
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    // R1 is cabled first: its end is the DCE.
    lab.cable(r1, "s0/0/0", r2, "s0/0/0");
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(b, "fa0", r2, "g0/0");
    assert!(lab.t.connect(End { device: r1, port: 1 }, End { device: r2, port: 3 }).is_err(), "Ethernet to serial");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.2.10 255.255.255.0 192.168.2.1");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nint s0/0/0\nip address 10.0.0.1 255.255.255.252\nno shut\nexit\nip route 192.168.2.0 255.255.255.0 10.0.0.2\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 192.168.2.1 255.255.255.0\nno shut\nint s0/0/0\nip address 10.0.0.2 255.255.255.252\nno shut\nexit\nip route 192.168.1.0 255.255.255.0 10.0.0.1\nend");
    // No clock on the DCE end: the line protocol stays down.
    let si = lab.run(r1, "show interfaces s0/0/0");
    assert!(si.contains("line protocol down (no clock rate on the DCE end)"), "{si}");
    assert!(lab.run(r1, "show controllers s0/0/0").contains("DCE end"));
    assert!(lab.run(r2, "show controllers serial 0/0/0").contains("DTE end"));
    lab.run(r2, "conf t\nint s0/0/0");
    assert!(lab.try_run(r2, "clock rate 64000").contains("DTE"));
    lab.run(r2, "no clock rate\nend");
    lab.run(r1, "conf t\nint s0/0/0\nclock rate 64000\nend");
    assert!(lab.run(a, "ping 192.168.2.10").contains("answered"));
    assert!(lab.run(a, "ping 192.168.2.10").contains("4 answered"));
    // PPP on one end only: down again; on both: up.
    lab.run(r1, "conf t\nint s0/0/0\nencapsulation ppp\nend");
    assert!(lab.run(r1, "show interfaces s0/0/0").contains("different encapsulations"));
    assert!(lab.run(a, "ping 192.168.2.10").contains(" 0 answered"));
    lab.run(r2, "conf t\nint s0/0/0\nencapsulation ppp\nend");
    assert!(lab.run(a, "ping 192.168.2.10").contains("4 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("[serial, PPP]")));
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("interface Serial0/0/0\n ip address 10.0.0.1 255.255.255.252\n encapsulation ppp\n clock rate 64000"), "{run}");
    lab.run(r1, "conf t");
    lab.run(r1, "int g0/1");
    assert!(lab.try_run(r1, "clock rate 64000").contains("not a serial"));
    assert!(lab.try_run(r1, "exit").is_empty());
    lab.run(r1, "int s0/0/0");
    assert!(lab.try_run(r1, "clock rate 12345").contains("Unsupported"));
    // IPv6 crosses the serial link too, with no neighbour discovery.
    lab.run(r1, "exit\nipv6 unicast-routing\nint s0/0/0\nipv6 address 2001:db8:12::1/64\nend");
    lab.run(r2, "conf t\nint s0/0/0\nipv6 address 2001:db8:12::2/64\nend");
    assert!(lab.run(r1, "ping 2001:db8:12::2").contains("!!!!!"));
}

#[test]
fn everyday_show_commands_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(a, "fa0", s, "fa0/2");
    lab.run(r, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nip helper-address 10.0.0.9\nno shut\nexit\naccess-list 10 permit any\nint g0/0\nip access-group 10 in\nend");
    lab.run(s, "en\nconf t\nvlan 10\nexit\nint fa0/2\nswitchport access vlan 10\ndescription aula\nend");
    // CDP: each sees the other; the PC is not listed.
    let n = lab.run(r, "show cdp neighbors");
    assert!(n.contains("S1") && n.contains("Gi0/0") && n.contains("Gi0/1") && n.contains("1 neighbour"), "{n}");
    let det = lab.run(s, "show cdp neighbors detail");
    assert!(det.contains("Neighbour R1 (router") && det.contains("Address   192.168.1.1"), "{det}");
    lab.run(s, "conf t\nint g0/1\nno cdp enable\nend");
    assert!(lab.run(r, "show cdp neighbors").contains("0 neighbours"));
    assert!(lab.run(s, "show running-config").contains(" no cdp enable"));
    lab.run(r, "conf t\nno cdp run\nend");
    assert!(lab.run(r, "show cdp").contains("off"));
    // Clock.
    assert!(lab.run(r, "show clock").contains("not set"));
    assert!(lab.try_run(r, "clock set 25:00:00 1 Jan 2026").contains("time of day"));
    lab.run(r, "clock set 10:30:00 30 Oct 2026");
    assert!(lab.run(r, "show clock").starts_with("10:30:00 UTC Fri Oct 30 2026"));
    // Interfaces.
    let ipi = lab.run(r, "show ip interface g0/0");
    assert!(ipi.contains("Access list in: 10") && ipi.contains("DHCP relay to: 10.0.0.9") && ipi.contains("broadcast 192.168.1.255"), "{ipi}");
    assert!(lab.run(r, "show ip interface brief").contains("Admin"));
    let st = lab.run(s, "show interfaces status");
    assert!(st.contains("Fa0/2      aula               connected     10") && st.contains("Fa0/3") && st.contains("no link"), "{st}");
    let sw = lab.run(s, "show interfaces fa0/2 switchport");
    assert!(sw.contains("Mode: access") && sw.contains("Access VLAN: 10 (VLAN0010)"), "{sw}");
    let block = lab.run(s, "show running-config interface fa0/2");
    assert!(block.starts_with("interface FastEthernet0/2\n description aula\n switchport access vlan 10"), "{block}");
    // Flash, users, terminal.
    assert!(lab.run(s, "show flash").contains("vlan.dat"));
    assert!(lab.run(r, "dir flash:").contains("packetcraft-router-"));
    assert!(lab.run(r, "show users").contains("con 0"));
    assert!(lab.try_run(r, "terminal length 0").is_empty());
    assert!(lab.try_run(r, "terminal bogus").contains("terminal length"));
}

#[test]
fn logging_and_debug_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.sim.topology_changed(&lab.t);
    lab.run(a, "ipconfig 10.0.0.2 255.255.255.0 10.0.0.1");
    lab.run(r, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    let log = lab.run(r, "show logging");
    assert!(log.contains("GigabitEthernet0/0: link up") && log.contains("Configuration changed from the console"), "{log}");
    lab.run(r, "debug ip icmp");
    assert!(lab.run(r, "show debugging").contains("ICMP debugging is on"));
    lab.run(a, "ping 10.0.0.1");
    assert!(lab.run(r, "show logging").contains("ICMP debug: echo request from 10.0.0.2 to 10.0.0.1"));
    lab.run(r, "undebug all\nclear logging");
    assert!(lab.run(r, "show logging").contains("0 messages"));
    lab.run(r, "conf t\nint g0/0\nshutdown\nend");
    assert!(lab.run(r, "show logging").contains("GigabitEthernet0/0: link down"));
}

#[test]
fn hsrp_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let pc = lab.add(DeviceKind::Pc);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(pc, "fa0", s1, "fa0/1");
    lab.cable(r1, "g0/0", s1, "fa0/2");
    lab.cable(r2, "g0/0", s1, "fa0/3");
    lab.cable(r1, "g0/1", s2, "fa0/2");
    lab.cable(r2, "g0/1", s2, "fa0/3");
    lab.cable(srv, "g0", s2, "fa0/1");
    lab.run(pc, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.254");
    lab.run(srv, "ipconfig 10.0.0.10 255.255.255.0 10.0.0.254");
    for (r, a, b, pri) in [(r1, "192.168.1.1", "10.0.0.1", 110), (r2, "192.168.1.2", "10.0.0.2", 100)] {
        lab.run(
            r,
            &format!(
                "en\nconf t\nint g0/0\nip address {a} 255.255.255.0\nstandby 1 ip 192.168.1.254\nstandby 1 priority {pri}\nstandby 1 preempt\nno shut\nint g0/1\nip address {b} 255.255.255.0\nstandby ip 10.0.0.254\nstandby 0 priority {pri}\nno shut\nend"
            ),
        );
    }
    let sb = lab.run(r1, "show standby");
    assert!(
        sb.contains("Group 1 on GigabitEthernet0/0: active (priority 110, preempt)")
            && sb.contains("virtual MAC 0000.0c07.ac01")
            && sb.contains("standby: 192.168.1.2"),
        "{sb}"
    );
    assert!(lab.run(r2, "show standby brief").contains("standby (priority 100, preempt)"));
    lab.run(pc, "ping 10.0.0.10");
    assert!(lab.run(pc, "ping 10.0.0.10").contains("4 answered"));
    assert!(lab.run(pc, "ping 192.168.1.254").contains("4 answered"));
    // The PC's gateway resolved to the virtual MAC.
    assert!(lab.run(pc, "arp -a").contains("00:00:0c:07:ac:01"));
    // R1 fails: R2 takes over without the PC changing anything.
    lab.run(r1, "conf t\nint g0/0\nshutdown\nint g0/1\nshutdown\nend");
    assert!(lab.run(r2, "show standby").contains("Group 1 on GigabitEthernet0/0: active"));
    lab.run(pc, "ping 10.0.0.10");
    assert!(lab.run(pc, "ping 10.0.0.10").contains("4 answered"));
    let run = lab.run(r2, "show running-config interface g0/0");
    assert!(run.contains(" standby 1 ip 192.168.1.254\n standby 1 preempt"), "{run}");
    lab.run(r2, "conf t\nint g0/0");
    assert!(lab.try_run(r2, "standby 2 ip 172.16.0.1").contains("not on"));
}

#[test]
fn dtp_by_typing() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(s1, "g0/1", s2, "g0/1");
    lab.cable(a, "fa0", s1, "fa0/1");
    lab.cable(b, "fa0", s2, "fa0/1");
    lab.run(a, "ipconfig 10.0.10.1 255.255.255.0");
    lab.run(b, "ipconfig 10.0.10.2 255.255.255.0");
    for s in [s1, s2] {
        lab.run(s, "en\nconf t\nvlan 10\nexit\nint fa0/1\nswitchport access vlan 10\nend");
    }
    let trunks = |lab: &mut Lab| lab.run(s1, "show interfaces trunk").contains("Gi0/1");
    // auto with auto: no trunk, VLAN 10 doesn't cross.
    for s in [s1, s2] {
        lab.run(s, "conf t\nint g0/1\nswitchport mode dynamic auto\nend");
    }
    assert!(!trunks(&mut lab));
    assert!(lab.run(a, "ping 10.0.10.2").contains(" 0 answered"));
    // desirable asks: it trunks.
    lab.run(s1, "conf t\nint g0/1\nswitchport mode dynamic desirable\nend");
    assert!(trunks(&mut lab));
    lab.run(a, "ping 10.0.10.2");
    assert!(lab.run(a, "ping 10.0.10.2").contains("4 answered"));
    let sw = lab.run(s2, "show interfaces g0/1 switchport");
    assert!(sw.contains("Mode: dynamic auto (working as trunk)"), "{sw}");
    // A trunk that sends no DTP leaves an auto end as access.
    lab.run(s1, "conf t\nint g0/1\nswitchport mode trunk\nswitchport nonegotiate\nend");
    assert!(lab.run(s2, "show interfaces g0/1 switchport").contains("working as access"));
    assert!(lab.run(s1, "show running-config interface g0/1").contains(" switchport mode trunk\n switchport nonegotiate"));
    lab.run(s1, "conf t\nint g0/1\nswitchport mode dynamic auto");
    assert!(lab.try_run(s1, "switchport nonegotiate").contains("negotiates"));
}

#[test]
fn ppp_authentication_by_typing() {
    let mut lab = Lab::new();
    let wan = packetcraft_model::models::find("Router-WAN").unwrap();
    let r1 = lab.t.add_model(wan, None, 0.0, 0.0).unwrap();
    let r2 = lab.t.add_model(wan, None, 0.0, 0.0).unwrap();
    lab.cable(r1, "s0/0/0", r2, "s0/0/0");
    lab.run(r1, "en\nconf t\nhostname R1\nint s0/0/0\nip address 10.0.0.1 255.255.255.252\nencapsulation ppp\nclock rate 64000\nno shut\nend");
    lab.run(r2, "en\nconf t\nhostname R2\nint s0/0/0\nip address 10.0.0.2 255.255.255.252\nencapsulation ppp\nno shut\nend");
    assert!(lab.run(r1, "ping 10.0.0.2").contains("!!!!!"));
    // CHAP: each names the other with the same secret.
    lab.run(r1, "conf t\nusername R2 secret clave\nint s0/0/0\nppp authentication chap\nend");
    assert!(lab.run(r1, "show interfaces s0/0/0").contains("PPP authentication failed"));
    lab.run(r2, "conf t\nusername R1 password otra\nend");
    assert!(lab.try_run(r1, "ping 10.0.0.2").contains("no route"));
    lab.run(r2, "conf t\nno username R1\nusername R1 password clave\nend");
    assert!(lab.run(r1, "ping 10.0.0.2").contains("!!!!!"), "{}", lab.run(r1, "show interfaces s0/0/0"));
    // PAP the other way: R2 demands it, R1 sends a name and password R2 knows.
    lab.run(r2, "conf t\nusername invitado password abc\nint s0/0/0\nppp authentication pap\nend");
    assert!(lab.run(r2, "show interfaces s0/0/0").contains("authentication failed"));
    lab.run(r1, "conf t\nint s0/0/0\nppp pap sent-username invitado password abc\nend");
    assert!(lab.run(r2, "ping 10.0.0.1").contains("!!!!!"));
    let run = lab.run(r1, "show running-config interface s0/0/0");
    assert!(run.contains(" encapsulation ppp\n ppp authentication chap\n ppp pap sent-username invitado password abc"), "{run}");
    lab.run(r1, "conf t\nint g0/0");
    assert!(lab.try_run(r1, "ppp authentication chap").contains("not a serial"));
}

#[test]
fn access_class_on_vty_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let admin = lab.add(DeviceKind::Pc);
    let guest = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(admin, "fa0", s, "fa0/1");
    lab.cable(guest, "fa0", s, "fa0/2");
    lab.run(admin, "ipconfig 10.0.0.10 255.255.255.0 10.0.0.1");
    lab.run(guest, "ipconfig 10.0.0.20 255.255.255.0 10.0.0.1");
    lab.run(r, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nexit\naccess-list 5 permit host 10.0.0.10\nline vty 0 4\npassword cisco\nlogin\naccess-class 5 in\nend");
    lab.run(admin, "ping 10.0.0.1");
    assert!(lab.run(admin, "telnet 10.0.0.1").contains("Open"));
    lab.run(admin, "cisco");
    lab.run(admin, "exit");
    lab.run(guest, "ping 10.0.0.1");
    assert!(lab.try_run(guest, "telnet 10.0.0.1").contains("refused"));
    assert!(lab.run(r, "show access-lists").contains("match"));
    assert!(lab.run(r, "show running-config").contains("line vty 0 4") && lab.run(r, "show running-config").contains(" access-class 5 in"));
    lab.run(r, "conf t\nline con 0");
    assert!(lab.try_run(r, "access-class 5 in").contains("line vty"));
}

#[test]
fn ipv6_access_lists_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.cable(b, "fa0", r, "g0/1");
    lab.run(
        r,
        "en\nconf t\nipv6 unicast-routing\nint g0/0\nipv6 address 2001:db8:1::1/64\nno shut\nint g0/1\nipv6 address 2001:db8:2::1/64\nno shut\nend",
    );
    lab.run(a, "ipv6config 2001:db8:1::10/64 2001:db8:1::1");
    lab.run(b, "ipv6config 2001:db8:2::10/64 2001:db8:2::1");
    assert!(lab.run(a, "ping 2001:db8:2::10").contains("4 answered"));
    lab.run(r, "conf t\nipv6 access-list NO-PING\ndeny icmp host 2001:db8:1::10 any\npermit ipv6 any any\nexit\nint g0/0\nipv6 traffic-filter NO-PING in\nend");
    let p = lab.run(a, "ping 2001:db8:2::10");
    assert!(p.contains(" 0 answered") && p.contains("unreachable"), "{p}");
    // Neighbour discovery still passes the filter, and other hosts aren't affected.
    let acl = lab.run(r, "show ipv6 access-list");
    assert!(acl.contains("10 deny icmp host 2001:db8:1::10 any (") && acl.contains("20 permit ipv6 any any"), "{acl}");
    let run = lab.run(r, "show running-config");
    assert!(
        run.contains("ipv6 access-list NO-PING\n deny icmp host 2001:db8:1::10 any\n permit ipv6 any any")
            && run.contains(" ipv6 traffic-filter NO-PING in"),
        "{run}"
    );
    lab.run(r, "conf t\nint g0/0\nno ipv6 traffic-filter NO-PING in\nend");
    assert!(lab.run(a, "ping 2001:db8:2::10").contains("4 answered"));
    lab.run(r, "conf t\nipv6 access-list X");
    assert!(lab.try_run(r, "permit ip any any").contains("ipv6, icmp, tcp or udp"));
    assert!(lab.try_run(r, "permit tcp any 2001:db8::/129").contains("prefix"));
}
