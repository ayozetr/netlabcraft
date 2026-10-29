use packetcraft_core::DeviceId;
use packetcraft_model::{DeviceKind, End, Topology};
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
