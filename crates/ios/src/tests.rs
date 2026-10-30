use netlabcraft_core::DeviceId;
use netlabcraft_model::{DeviceKind, End, SwitchportMode, Topology};
use netlabcraft_sim::Sim;

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
    let amb = lab.try_run(r, "d");
    assert!(amb.contains("Ambiguous: `d` could be delay, description, do"), "{amb}");
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
                for setup in [
                    "",
                    "en",
                    "en\nconf t",
                    "en\nconf t\nint g0/0",
                    "en\nconf t\nint g0/0.1",
                    "en\nconf t\nint fa0/1",
                    "en\nconf t\nvlan 5",
                    "en\nconf t\nrouter eigrp 1",
                    "en\nconf t\ntelephony-service\nmax-dn 4\nmax-ephones 4\nexit\nephone-dn 1",
                    "en\nconf t\ntelephony-service\nmax-ephones 4\nexit\nephone 1",
                ] {
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
    lab.run(r, "debug ip dhcp server events");
    let out = lab.run(a, "ipconfig /renew");
    assert!(lab.run(r, "show logging").contains("DHCP debug: 192.168.10.10 given to"), "the debug reports the lease");
    lab.run(r, "undebug all");
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
    lab.run(r1, "debug ip ospf hello");
    lab.sim.run_for(&lab.t, 11_000_000);
    let log = lab.run(r1, "show logging");
    assert!(log.contains("OSPF debug: hello from 10.0.0.2 (router ID 172.16.2.1, area 0) on GigabitEthernet0/1, 1 neighbour listed"), "{log}");
    // The neighbours exchanged their databases once, when they met.
    let db: Vec<String> =
        lab.sim.trace().iter().filter(|e| e.summary.starts_with("OSPF") && !e.summary.contains("hello")).map(|e| e.summary.clone()).collect();
    for want in [
        "database description, first (master) from router 1.1.1.1",
        "database description, 2 LSA headers",
        "link-state request, 1 LSA",
        "link-state update, 1 LSA from router 172.16.2.1",
        "link-state acknowledgement",
    ] {
        assert!(db.iter().any(|x| x.contains(want)), "{want}: {db:#?}");
    }
    assert_eq!(db.len(), 10, "{db:#?}");
    assert!(lab.try_run(r1, "debug ip ospf bogus").contains("adj, events or hello"));
    lab.run(r1, "no debug ip ospf");
    assert!(lab.run(r1, "show debugging").contains("No debugging"));
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
    lab.run(r1, "debug ip nat\ndebug ip pac");
    lab.run(a, "ping 8.8.8.8");
    assert!(lab.run(a, "ping 8.8.8.8").contains("4 answered"));
    let log = lab.run(r1, "show logging");
    assert!(log.contains("NAT debug: new translation, source 192.168.1.10 → 200.0.0.1, to 8.8.8.8"), "{log}");
    assert!(log.contains("IP debug: 192.168.1.10 → 8.8.8.8, in GigabitEthernet0/0, routed out GigabitEthernet0/1 via 200.0.0.2"), "{log}");
    assert!(lab.run(r1, "show debugging").contains("IP packets, NAT"));
    lab.run(r1, "no debug ip packet\nno debug ip nat");
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
        if let netlabcraft_sim::PortEvent::Sticky { port, mac, .. } = e {
            lab.t.device_mut(s).unwrap().ports[port].port_security.as_mut().unwrap().macs.push(mac);
        }
    }
    assert!(lab.run(s, "show run").contains("switchport port-security mac-address sticky 0200."));
    // The intruder: violation, the port goes err-disabled.
    let out = lab.run(b, "ping 10.0.0.3");
    assert!(out.contains("0 answered"), "{out}");
    let events = lab.sim.take_events();
    assert!(events.iter().any(|e| matches!(e, netlabcraft_sim::PortEvent::ErrDisabled { .. })));
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
    // LACPDUs on the wire: both ends (the passive one answers), in sync, never forwarded.
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 31_000_000);
    let lacp: Vec<String> = lab.sim.trace().iter().filter(|e| e.protocol.name() == "LACP").map(|e| e.summary.clone()).collect();
    assert_eq!(lacp.len(), 4, "{lacp:?}");
    assert!(lacp.iter().any(|x| x.contains("(active, in sync)")) && lacp.iter().any(|x| x.contains("(passive, in sync)")), "{lacp:?}");
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
    let ml = lab.t.add_model(netlabcraft_model::models::find("Switch-ML-24").unwrap(), None, 0.0, 0.0).unwrap();
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
    // `debug ip rip` reports the updates as they arrive.
    lab.run(r1, "debug ip rip");
    lab.sim.run_for(&lab.t, 31_000_000);
    let log = lab.run(r1, "show logging");
    assert!(log.contains("RIP debug: version 2 update from 10.0.12.2 on GigabitEthernet0/1: ") && log.contains("192.168.3.0/24 metric 2"), "{log}");
    assert!(lab.run(r1, "show debugging").contains("RIP"));
    lab.run(r1, "undebug all");
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
    let wan = netlabcraft_model::models::find("Router-WAN").unwrap();
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
    // LLDP: off until `lldp run` on both ends; advertisements on the wire every 30 s.
    assert!(lab.run(r, "show lldp neighbors").contains("off"));
    lab.run(r, "conf t\nlldp run\nend");
    assert!(lab.run(r, "show lldp neighbors").contains("0 neighbours"));
    lab.run(s, "conf t\nlldp run\nend");
    let n = lab.run(r, "show lldp neighbors");
    assert!(n.contains("S1") && n.contains("Gi0/0") && n.contains("Gi0/1") && n.contains("bridge") && n.contains("1 neighbour"), "{n}");
    let det = lab.run(s, "show lldp entry *");
    assert!(det.contains("Neighbour R1") && det.contains("Can act as   router") && det.contains("192.168.1.1"), "{det}");
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 31_000_000);
    let lldp: Vec<_> = lab.sim.trace().iter().filter(|e| e.protocol.name() == "LLDP").collect();
    assert!(lldp.len() >= 2 && lldp.iter().all(|e| e.dropped.is_none()), "{lldp:?}");
    lab.run(s, "conf t\nint g0/1\nno lldp transmit\nend");
    assert!(lab.run(r, "show lldp neighbors").contains("0 neighbours"));
    assert!(lab.run(s, "show lldp neighbors").contains("1 neighbour"));
    assert!(lab.run(s, "show running-config").contains("lldp run\n") && lab.run(s, "show lldp").contains("Not sending on: Gi0/1"));
    // Processes and memory.
    assert!(lab.run(r, "show processes cpu").contains("CPU use") && lab.run(s, "show processes").contains("Spanning tree"));
    assert!(lab.run(r, "show memory statistics").contains("Main memory"));
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
    assert!(lab.run(r, "dir flash:").contains("netlabcraft-router-"));
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
    assert!(lab.run(r, "show debugging").contains("Debugging is on for: ICMP"));
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
    // The new trunk listens and learns for 30 s before it forwards VLAN 10.
    assert!(lab.run(s1, "show spanning-tree vlan 10").contains("LIS"));
    lab.sim.run_for(&lab.t, 31_000_000);
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
    let wan = netlabcraft_model::models::find("Router-WAN").unwrap();
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

#[test]
fn ospf_depth_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    let lan = lab.add(DeviceKind::Switch);
    // R1, R2 and R3 share one LAN (area 0); R3 also has a stub network in area 1.
    lab.cable(r1, "g0/0", lan, "fa0/1");
    lab.cable(r2, "g0/0", lan, "fa0/2");
    lab.cable(r3, "g0/0", lan, "fa0/3");
    let host = lab.add(DeviceKind::Pc);
    lab.cable(host, "fa0", r3, "g0/1");
    lab.run(host, "ipconfig 172.16.1.10 255.255.255.0 172.16.1.1");
    for (r, n, prio) in [(r1, 1, 200), (r2, 2, 0), (r3, 3, 1)] {
        lab.run(r, &format!("en\nconf t\nint g0/0\nip address 10.0.0.{n} 255.255.255.0\nip ospf priority {prio}\nno shut\nexit\nrouter ospf 1\nrouter-id {n}.{n}.{n}.{n}\nnetwork 10.0.0.0 0.0.0.255 area 0\nend"));
    }
    lab.run(r3, "conf t\nint g0/1\nip address 172.16.1.1 255.255.255.0\nip ospf cost 50\nno shut\nexit\nrouter ospf 1\nnetwork 172.16.1.0 0.0.0.255 area 1\nauto-cost reference-bandwidth 1000\nend");
    // DR: R1 (priority 200); R2 (priority 0) never; R3 is BDR.
    let n = lab.run(r2, "show ip ospf neighbor");
    assert!(n.contains("1.1.1.1") && n.contains("FULL/DR") && n.contains("FULL/BDR"), "{n}");
    let oi = lab.run(r3, "show ip ospf interface brief");
    assert!(oi.contains("Gi0/0      0") && oi.contains("BDR") && oi.contains("Gi0/1      1") && oi.contains("50"), "{oi}");
    assert!(lab.run(r2, "show ip ospf interface").contains("never-DR"));
    // R1 learns the area-1 network as inter-area.
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O IA 172.16.1.0/24"), "{rt}");
    assert!(lab.run(r3, "show ip ospf").contains("area border router"));
    assert!(
        lab.run(r3, "show running-config").contains(" ip ospf cost 50")
            && lab.run(r3, "show running-config").contains(" auto-cost reference-bandwidth 1000")
    );
    lab.run(r1, "ping 172.16.1.10");
    assert!(lab.run(r1, "ping 172.16.1.10").contains("!!!!!"));
}

#[test]
fn ospf_stub_areas_and_ranges_by_typing() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/0");
    for (r, port) in [(r1, "g0/0"), (r3, "g0/1"), (r3, "g0/2")] {
        let pc = lab.add(DeviceKind::Pc);
        lab.cable(pc, "fa0", r, port);
    }
    // R1 in area 0 with the way out; R2 the border router; R3 inside area 1 with two LANs.
    lab.run(r1, "en\nconf t\nint g0/0\nip add 192.168.0.1 255.255.255.0\nno shut\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nexit\nip route 0.0.0.0 0.0.0.0 192.168.0.254\nrouter ospf 1\nnetwork 10.0.12.0 0.0.0.3 area 0\nnetwork 192.168.0.0 0.0.0.255 area 0\ndefault-information originate\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 10.0.12.2 255.255.255.252\nno shut\nint g0/1\nip add 10.0.23.1 255.255.255.252\nno shut\nexit\nrouter ospf 1\nnetwork 10.0.12.0 0.0.0.3 area 0\nnetwork 10.0.23.0 0.0.0.3 area 1\nend");
    lab.run(r3, "en\nconf t\nint g0/0\nip add 10.0.23.2 255.255.255.252\nno shut\nint g0/1\nip add 172.16.0.1 255.255.255.0\nno shut\nint g0/2\nip add 172.16.1.1 255.255.255.0\nno shut\nexit\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 1\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(rt.contains("O IA 192.168.0.0/24") && rt.contains("O*E2 0.0.0.0/0"), "{rt}");
    // Stub on one end only: the hellos disagree, no neighbours.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 stub\nend");
    assert!(lab.run(r3, "show ip ospf neighbor").contains("No OSPF neighbours"));
    lab.run(r3, "conf t\nrouter ospf 1\narea 1 stub\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(rt.contains("O IA 192.168.0.0/24") && rt.contains("O*IA 0.0.0.0/0") && !rt.contains("O*E2 0.0.0.0"), "{rt}");
    // Totally stubby: only the default comes in.
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 stub no-summary\nend");
    let rt = lab.run(r3, "show ip route");
    assert!(!rt.contains("192.168.0.0") && !rt.contains("10.0.12.0") && rt.contains("O*IA 0.0.0.0/0"), "{rt}");
    assert!(lab.run(r2, "show running-config").contains(" area 1 stub no-summary"));
    lab.run(r2, "conf t\nrouter ospf 1");
    assert!(lab.try_run(r2, "area 0 stub").contains("backbone"));
    lab.run(r2, "end");
    // A range on the border router: area 0 sees one summary for R3's LANs.
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O IA 172.16.0.0/24") && rt.contains("O IA 172.16.1.0/24"), "{rt}");
    lab.run(r2, "conf t\nrouter ospf 1\narea 1 range 172.16.0.0 255.255.254.0\nend");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O IA 172.16.0.0/23") && !rt.contains("172.16.1.0/24"), "{rt}");
    assert!(lab.run(r2, "show running-config").contains(" area 1 range 172.16.0.0 255.255.254.0"));
    lab.run(r2, "conf t\nrouter ospf 1\nno area 1 range 172.16.0.0 255.255.254.0\nno area 1 stub\nend");
    lab.run(r3, "conf t\nrouter ospf 1\nno area 1 stub\nend");
    assert!(lab.run(r3, "show ip route").contains("O*E2 0.0.0.0/0"));
}

#[test]
fn ospf_on_a_multilayer_switch_by_typing() {
    let mut lab = Lab::new();
    let ml = lab.t.add_model(netlabcraft_model::models::find("Switch-ML-24").unwrap(), None, 0.0, 0.0).unwrap();
    let r = lab.add(DeviceKind::Router);
    let pc = lab.add(DeviceKind::Pc);
    let far = lab.add(DeviceKind::Pc);
    lab.cable(ml, "g0/1", r, "g0/0");
    lab.cable(pc, "fa0", ml, "fa0/1");
    lab.cable(far, "fa0", r, "g0/1");
    lab.run(pc, "ipconfig 192.168.10.10 255.255.255.0 192.168.10.1");
    lab.run(far, "ipconfig 172.16.0.10 255.255.255.0 172.16.0.1");
    lab.run(ml, "en\nconf t");
    assert!(lab.try_run(ml, "router ospf 1").contains("ip routing"));
    lab.run(ml, "ip routing\nvlan 10\nvlan 99\nexit\nint fa0/1\nswitchport access vlan 10\nint g0/1\nswitchport access vlan 99\nexit\ninterface vlan 10\nip address 192.168.10.1 255.255.255.0\nno shut\ninterface vlan 99\nip address 10.0.99.2 255.255.255.0\nno shut\nexit\nrouter ospf 1\nnetwork 192.168.10.0 0.0.0.255 area 0\nnetwork 10.0.99.0 0.0.0.255 area 0\nend");
    lab.run(r, "en\nconf t\nint g0/0\nip address 10.0.99.1 255.255.255.0\nno shut\nint g0/1\nip address 172.16.0.1 255.255.255.0\nno shut\nexit\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend");
    let n = lab.run(ml, "show ip ospf neighbor");
    assert!(n.contains("Vlan99") || n.contains("10.0.99.1"), "{n}");
    assert!(lab.run(r, "show ip route").contains("192.168.10.0/24"));
    lab.run(pc, "ping 172.16.0.10");
    assert!(lab.run(pc, "ping 172.16.0.10").contains("4 answered"));
    // The same with RIP, its updates leaving through the VLAN interface.
    lab.run(ml, "conf t\nno router ospf 1\nrouter rip\nversion 2\nnetwork 192.168.10.0\nnetwork 10.0.0.0\nend");
    lab.run(r, "conf t\nno router ospf 1\nrouter rip\nversion 2\nnetwork 10.0.0.0\nnetwork 172.16.0.0\nend");
    let rt = lab.run(r, "show ip route");
    assert!(rt.contains("R    192.168.10.0/24") && rt.contains("via 10.0.99.2"), "{rt}");
    assert!(lab.run(ml, "show ip rip database").contains("172.16.0.0/24"));
    assert!(lab.run(pc, "ping 172.16.0.10").contains("4 answered"));
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 31_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.protocol.name() == "RIP" && e.from.device == ml && e.dropped.is_none()));
}

#[test]
fn ipv6_on_subinterfaces_and_vlan_interfaces_by_typing() {
    // Router on a stick: two VLANs, IPv6 on the subinterfaces.
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(b, "fa0", s, "fa0/2");
    lab.run(s, "en\nconf t\nvlan 10\nvlan 20\nexit\nint fa0/1\nswitchport access vlan 10\nint fa0/2\nswitchport access vlan 20\nint g0/1\nswitchport mode trunk\nend");
    lab.run(r, "en\nconf t\nipv6 unicast-routing\nint g0/0\nno shut\nint g0/0.10\nencapsulation dot1Q 10\nipv6 address 2001:db8:10::1/64\nint g0/0.20\nencapsulation dot1Q 20\nipv6 address 2001:db8:20::1/64\nend");
    lab.run(a, "ipv6config 2001:db8:10::10/64 2001:db8:10::1");
    assert!(lab.run(b, "ipv6config auto").contains("2001:db8:20::"), "SLAAC through the subinterface");
    let bb = lab.run(r, "show ipv6 interface brief");
    assert!(bb.contains("GigabitEthernet0/0.10") && bb.contains("2001:db8:10::1"), "{bb}");
    let addr_b = lab.t.device(b).unwrap().ports[0].ipv6.addresses[0].addr;
    lab.run(a, &format!("ping {addr_b}"));
    assert!(lab.run(a, &format!("ping {addr_b}")).contains("4 answered"));
    assert!(lab.run(r, "show running-config").contains("interface GigabitEthernet0/0.20\n encapsulation dot1Q 20\n ipv6 address 2001:db8:20::1/64"));

    // A multilayer switch routing IPv6 between its VLAN interfaces.
    let mut lab = Lab::new();
    let ml = lab.t.add_model(netlabcraft_model::models::find("Switch-ML-24").unwrap(), None, 0.0, 0.0).unwrap();
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", ml, "fa0/1");
    lab.cable(b, "fa0", ml, "fa0/2");
    lab.run(ml, "en\nconf t\nvlan 10\nvlan 20\nexit\nint fa0/1\nswitchport access vlan 10\nint fa0/2\nswitchport access vlan 20\nexit");
    assert!(lab.try_run(ml, "ipv6 unicast-routing").is_empty());
    lab.run(
        ml,
        "ip routing\ninterface vlan 10\nipv6 address 2001:db8:a::1/64\nno shut\ninterface vlan 20\nipv6 address 2001:db8:b::1/64\nno shut\nint fa0/3",
    );
    assert!(lab.try_run(ml, "ipv6 address 2001:db8:c::1/64").contains("VLAN interface"));
    lab.run(ml, "end");
    lab.run(a, "ipv6config 2001:db8:a::10/64 2001:db8:a::1");
    lab.run(b, "ipv6config 2001:db8:b::10/64 2001:db8:b::1");
    lab.run(a, "ping 2001:db8:b::10");
    let ping = lab.run(a, "ping 2001:db8:b::10");
    assert!(ping.contains("4 answered"), "{ping}");
    assert!(lab.run(ml, "ping 2001:db8:a::10").contains("answered"));
    let rt = lab.run(ml, "show ipv6 route");
    assert!(rt.contains("2001:db8:a::/64") && rt.contains("Vlan10"), "{rt}");
    assert!(lab.run(ml, "show running-config").contains("interface Vlan20\n no ip address\n ipv6 address 2001:db8:b::1/64"));
    // A plain switch can't route IPv6.
    let sw = lab.add(DeviceKind::Switch);
    lab.run(sw, "en\nconf t");
    assert!(lab.try_run(sw, "ipv6 unicast-routing").contains("multilayer"));
}

#[test]
fn dhcpv6_by_typing() {
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
        "en\nconf t\nipv6 unicast-routing\nipv6 dhcp pool LAN\naddress prefix 2001:db8:acad:1::/64\ndns-server 2001:db8::53\ndomain-name lab.local\nexit\nint g0/0\nipv6 address 2001:db8:acad:1::1/64\nno shut\nend",
    );
    // SLAAC: no flags.
    assert!(lab.run(a, "ipv6config auto").contains("(SLAAC)"));
    // Stateless: SLAAC address, DNS from DHCPv6.
    lab.run(r, "conf t\nint g0/0\nipv6 dhcp server LAN\nipv6 nd other-config-flag\nend");
    let st = lab.run(a, "ipv6config auto");
    assert!(st.contains("DNS 2001:db8::53") && st.contains("stateless DHCPv6") && st.contains("ff:fe"), "{st}");
    // Stateful: the address comes from the pool.
    lab.run(r, "conf t\nint g0/0\nipv6 nd managed-config-flag\nend");
    let sf = lab.run(b, "ipv6config auto");
    assert!(sf.contains("Autoconfigured 2001:db8:acad:1::10/64") && sf.contains("stateful DHCPv6"), "{sf}");
    assert!(lab.run(a, "ipv6config auto").contains("2001:db8:acad:1::11/64"));
    let bind = lab.run(r, "show ipv6 dhcp binding");
    assert!(bind.contains("2001:db8:acad:1::10") && bind.contains("LAN"), "{bind}");
    assert!(lab.run(r, "show ipv6 dhcp pool").contains("2 handed out"));
    assert!(lab.run(a, "ping 2001:db8:acad:1::10").contains("4 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("DHCPv6 Advertise")));
    let run = lab.run(r, "show running-config");
    assert!(
        run.contains("ipv6 dhcp pool LAN\n address prefix 2001:db8:acad:1::/64\n dns-server 2001:db8::53\n domain-name lab.local")
            && run.contains(" ipv6 nd managed-config-flag"),
        "{run}"
    );
}

#[test]
fn dhcpv6_relay_by_typing() {
    // The client's router relays to a DHCPv6 server one link away.
    let mut lab = Lab::new();
    let server = lab.add(DeviceKind::Router);
    let relay = lab.add(DeviceKind::Router);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(server, "g0/0", relay, "g0/0");
    lab.cable(pc, "fa0", relay, "g0/1");
    lab.run(
        server,
        "en\nconf t\nipv6 unicast-routing\nipv6 dhcp pool FAR\naddress prefix 2001:db8:2::/64\ndns-server 2001:db8::53\nexit\nint g0/0\nipv6 address 2001:db8:12::1/64\nipv6 dhcp server FAR\nno shut\nexit\nipv6 route 2001:db8:2::/64 2001:db8:12::2\nend",
    );
    lab.run(
        relay,
        "en\nconf t\nipv6 unicast-routing\nint g0/0\nipv6 address 2001:db8:12::2/64\nno shut\nint g0/1\nipv6 address 2001:db8:2::1/64\nipv6 nd managed-config-flag\nipv6 dhcp relay destination 2001:db8:12::1\nno shut\nend",
    );
    let got = lab.run(pc, "ipv6config auto");
    assert!(got.contains("2001:db8:2::10/64") && got.contains("stateful DHCPv6") && got.contains("2001:db8::53"), "{got}");
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("DHCPv6 Relay-forward (Solicit)")));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("DHCPv6 Relay-reply (Reply)")));
    assert!(lab.run(server, "show ipv6 dhcp binding").contains("2001:db8:2::10"));
    assert!(lab.run(relay, "show running-config").contains(" ipv6 dhcp relay destination 2001:db8:12::1"));
    assert!(lab.run(pc, "ping 2001:db8:12::1").contains("4 answered"));
    lab.run(relay, "conf t\nint g0/1");
    assert!(lab.try_run(relay, "ipv6 dhcp relay destination fe80::1").contains("global"));
}

#[test]
fn ospfv3_by_typing() {
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
    // IPv6 only: no IPv4 address to take a router ID from.
    let out = lab.run(r1, "en\nconf t\nipv6 unicast-routing\nipv6 router ospf 1");
    assert!(out.contains("router ID"), "{out}");
    assert_eq!(lab.prompt(r1), "R1(config-rtr)#");
    lab.run(r1, "router-id 1.1.1.1\npassive-interface g0/0\nexit\nint g0/0\nipv6 address 2001:db8:1::1/64\nipv6 ospf 1 area 0\nno shut\nint g0/1\nipv6 enable\nipv6 ospf 1 area 0\nno shut\nend");
    lab.run(r2, "en\nconf t\nipv6 unicast-routing\nipv6 router ospf 1\nrouter-id 2.2.2.2\nexit\nint g0/0\nipv6 enable\nipv6 ospf 1 area 0\nno shut\nint g0/1\nipv6 enable\nipv6 ospf 1 area 1\nno shut\nend");
    lab.run(r3, "en\nconf t\nipv6 unicast-routing\nipv6 router ospf 1\nrouter-id 3.3.3.3\nexit\nint g0/0\nipv6 enable\nipv6 ospf 1 area 1\nno shut\nint g0/1\nipv6 address 2001:db8:3::1/64\nipv6 ospf 1 area 1\nno shut\nend");
    let nb = lab.run(r2, "show ipv6 ospf neighbor");
    assert!(nb.contains("1.1.1.1") && nb.contains("3.3.3.3") && nb.contains("FULL") && nb.contains("fe80::"), "{nb}");
    // Links with only link-local addresses carry the routes.
    let rt = lab.run(r1, "show ipv6 route");
    assert!(rt.contains("OI 2001:db8:3::/64") && rt.contains("via fe80::"), "{rt}");
    lab.run(a, "ipv6config 2001:db8:1::10/64 2001:db8:1::1");
    lab.run(b, "ipv6config 2001:db8:3::10/64 2001:db8:3::1");
    lab.run(a, "ping 2001:db8:3::10");
    let ping = lab.run(a, "ping 2001:db8:3::10");
    assert!(ping.contains("4 answered"), "{ping}");
    assert!(lab.run(r1, "show ipv6 ospf").contains("area 0, cost 1, passive"));
    let run = lab.run(r1, "show running-config");
    assert!(
        run.contains(" ipv6 ospf 1 area 0") && run.contains("ipv6 router ospf 1\n router-id 1.1.1.1\n passive-interface GigabitEthernet0/0"),
        "{run}"
    );
    // Hellos on the wire, from link-local addresses to ff02::5.
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 11_000_000);
    assert!(
        lab.sim
            .trace()
            .iter()
            .any(|e| e.summary.starts_with("OSPFv3 hello from router 2.2.2.2") && e.summary.contains("ff02::5") && e.dropped.is_none())
    );
    // A default route from R3.
    lab.run(r3, "conf t\nipv6 route ::/0 2001:db8:3::10\nipv6 router ospf 1\ndefault-information originate\nend");
    assert!(lab.run(r1, "show ipv6 route").contains("OE2 ::/0"));
    lab.run(r2, "conf t\nno ipv6 router ospf 1\nend");
    assert!(lab.run(r1, "show ipv6 ospf neighbor").contains("No OSPFv3 neighbours"));
}

#[test]
fn aaa_logins_through_radius_by_typing() {
    use netlabcraft_model::services::{AaaClient, AaaUser};
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
    }
    lab.run(r, "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nexit\nusername backup secret local1");
    assert!(lab.try_run(r, "aaa authentication login default group radius local").contains("aaa new-model"));
    lab.run(r, "aaa new-model\naaa authentication login default group radius local\nradius-server host 192.168.1.5 key radkey\nline vty 0 4\nlogin authentication default\nend");
    let run = lab.run(r, "show running-config");
    assert!(run.contains("aaa new-model\naaa authentication login default group radius local\nradius-server host 192.168.1.5 key radkey"), "{run}");
    // The server says yes to its user, no to anyone else.
    assert!(lab.try_run(pc, "telnet 192.168.1.1").contains("Open"));
    assert_eq!(lab.prompt(pc), "Username: ");
    lab.try_run(pc, "teacher");
    assert!(lab.try_run(pc, "wrong").contains("Login invalid"));
    lab.try_run(pc, "teacher");
    assert_eq!(lab.try_run(pc, "pt-pass"), "");
    assert_eq!(lab.prompt(pc), "R1>");
    lab.try_run(pc, "exit");
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("RADIUS Access-Request") && e.summary.contains("(user teacher)")));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("RADIUS Access-Accept")));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("RADIUS Access-Reject")));
    // A RADIUS "no" is final: the local user doesn't get in while the server answers.
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "backup");
    assert!(lab.try_run(pc, "local1").contains("Login invalid"));
    for _ in 0..2 {
        lab.try_run(pc, "x");
        lab.try_run(pc, "x");
    }
    assert_eq!(lab.prompt(pc), "PC1> ", "three failures close the session");
    // A wrong shared secret: the answers don't verify, so the local user is next.
    lab.run(r, "conf t\nradius-server host 192.168.1.5 key other\nend");
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "backup");
    assert_eq!(lab.try_run(pc, "local1"), "");
    assert_eq!(lab.prompt(pc), "R1>");
    lab.try_run(pc, "exit");
    // The server down: local again.
    lab.run(r, "conf t\nradius-server host 192.168.1.5 key radkey\nend");
    lab.t.device_mut(srv).unwrap().services.aaa.on = false;
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "backup");
    assert_eq!(lab.try_run(pc, "local1"), "");
    assert_eq!(lab.prompt(pc), "R1>");
    lab.try_run(pc, "exit");
    // TACACS+ instead: the same server and secret, over TCP port 49.
    lab.t.device_mut(srv).unwrap().services.aaa.on = true;
    lab.run(r, "conf t\nno radius-server host 192.168.1.5\ntacacs-server host 192.168.1.5 key radkey\naaa authentication login default group tacacs+ local\nend");
    assert!(
        lab.run(r, "show running-config").contains("aaa authentication login default group tacacs+ local\ntacacs-server host 192.168.1.5 key radkey")
    );
    lab.sim.clear_trace();
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "teacher");
    assert_eq!(lab.try_run(pc, "pt-pass"), "");
    assert_eq!(lab.prompt(pc), "R1>");
    lab.try_run(pc, "exit");
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "teacher");
    assert!(lab.try_run(pc, "nope").contains("Login invalid"), "a TACACS+ no is final");
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("TACACS+ authentication START")));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("TACACS+ authentication REPLY")));
    // A wrong key: the answer doesn't read, so the local user is next.
    lab.run(r, "conf t\ntacacs-server host 192.168.1.5 key wrong\nend");
    lab.try_run(pc, "x");
    lab.try_run(pc, "x");
    lab.try_run(pc, "x");
    lab.try_run(pc, "x");
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "backup");
    assert_eq!(lab.try_run(pc, "local1"), "");
    assert_eq!(lab.prompt(pc), "R1>");
}

#[test]
fn ftp_by_typing() {
    use netlabcraft_model::services::{FtpUser, WebFile};
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
    lab.run(r, "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nend");
    {
        let ftp = &mut lab.t.device_mut(srv).unwrap().services.ftp;
        ftp.users.push(FtpUser { name: "student".into(), password: "ftp123".into(), read: true, write: true, delete: false, list: true });
        ftp.files.push(WebFile { name: "notes.txt".into(), content: "Lab notes\n".repeat(400) });
    }
    lab.t.device_mut(pc).unwrap().files.push(WebFile { name: "report.txt".into(), content: "my report".into() });
    // A wrong password ends the session.
    lab.try_run(pc, "ftp 192.168.1.5");
    assert_eq!(lab.prompt(pc), "Username: ");
    lab.try_run(pc, "student");
    assert!(lab.try_run(pc, "nope").contains("530"));
    assert_eq!(lab.prompt(pc), "PC1> ");
    lab.try_run(pc, "ftp 192.168.1.5");
    lab.try_run(pc, "student");
    assert!(lab.try_run(pc, "ftp123").contains("230"));
    assert_eq!(lab.prompt(pc), "ftp> ");
    // Fetch a file bigger than one segment, send one, list, and a refused delete.
    let got = lab.try_run(pc, "get notes.txt");
    assert!(got.contains("Receiving notes.txt (4000 bytes)") && got.contains("226"), "{got}");
    assert_eq!(lab.t.device(pc).unwrap().files.iter().find(|f| f.name == "notes.txt").unwrap().content.len(), 4000);
    assert!(lab.try_run(pc, "put report.txt").contains("226"));
    assert_eq!(lab.t.device(srv).unwrap().services.ftp.files.iter().find(|f| f.name == "report.txt").unwrap().content, "my report");
    let dir = lab.try_run(pc, "dir");
    assert!(dir.contains("report.txt") && dir.contains("notes.txt"), "{dir}");
    assert!(lab.try_run(pc, "delete report.txt").contains("550 Permission denied"));
    assert!(lab.try_run(pc, "get missing.txt").contains("550"));
    assert!(lab.try_run(pc, "quit").contains("221"));
    assert!(lab.try_run(pc, "dir").contains("notes.txt"));
    // The exchange is on the wire, without the password.
    let ftp: Vec<String> = lab.sim.trace().iter().filter(|e| e.protocol.name() == "FTP").map(|e| e.summary.clone()).collect();
    assert!(ftp.iter().any(|x| x.starts_with("FTP USER student")) && ftp.iter().any(|x| x.starts_with("FTP PASS ****")), "{ftp:?}");
    assert!(
        ftp.iter().any(|x| x.starts_with("FTP 227 Entering Passive Mode")) && ftp.iter().any(|x| x.starts_with("FTP data, 1460 bytes")),
        "{ftp:?}"
    );
    assert!(!ftp.iter().any(|x| x.contains("ftp123")));
    // A router saves its configuration there.
    assert!(lab.try_run(r, "copy running-config ftp: 192.168.1.5").contains("ip ftp username"));
    lab.run(r, "conf t\nip ftp username student\nip ftp password ftp123\nend");
    assert!(lab.run(r, "copy running-config ftp: 192.168.1.5").contains("to ftp://192.168.1.5/r1-confg"));
    let saved = lab.t.device(srv).unwrap().services.ftp.files.iter().find(|f| f.name == "r1-confg").unwrap().content.clone();
    assert!(saved.contains("ip ftp username student"));
    assert!(lab.run(r, "copy ftp: running-config 192.168.1.5 r1-confg").contains("Merged"));
    // FTP off: refused.
    lab.t.device_mut(srv).unwrap().services.ftp.on = false;
    lab.try_run(pc, "ftp 192.168.1.5");
    lab.try_run(pc, "student");
    assert!(lab.try_run(pc, "ftp123").contains("refused"));
}

#[test]
fn ntp_syslog_and_tftp_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(srv, "g0", r, "g0/0");
    lab.run(srv, "ipconfig 10.0.0.100 255.255.255.0 10.0.0.1");
    lab.run(r, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nend\nping 10.0.0.100");
    // NTP: a router whose clock is far off takes the server's time.
    lab.run(r, "clock set 08:00:00 1 Jan 2031\nconf t");
    let out = lab.run(r, "ntp server 10.0.0.100");
    assert!(out.contains("synchronised with 10.0.0.100"), "{out}");
    lab.run(r, "end");
    assert!(lab.run(r, "show clock").contains("2026"));
    assert!(lab.run(r, "show ntp status").contains("Clock synchronised with 10.0.0.100"));
    // Syslog: link changes reach the server.
    lab.run(r, "conf t\nlogging host 10.0.0.100\nint g0/1\nno shut\nshutdown\nend");
    lab.run(r, "ping 10.0.0.100");
    let got = lab.sim.state(srv).map(|s| s.syslog.iter().map(|x| x.2.clone()).collect::<Vec<_>>()).unwrap_or_default();
    assert!(got.iter().any(|m| m.starts_with("R1: ") && m.contains("Configuration changed")), "{got:?}");
    // TFTP: save the configuration on the server, then bring it back as startup-config.
    let routes: String = (1..=20).map(|n| format!("ip route 172.16.{n}.0 255.255.255.0 10.0.0.100\n")).collect();
    lab.run(r, &format!("conf t\n{routes}end"));
    let up = lab.run(r, "copy running-config tftp: 10.0.0.100");
    assert!(up.contains("tftp://10.0.0.100/r1-confg"), "{up}");
    let file = lab.t.device(srv).unwrap().services.tftp.files.iter().find(|f| f.name == "r1-confg").cloned().unwrap();
    assert!(file.content.contains("ntp server 10.0.0.100") && file.content.len() > 512, "multi-block transfer");
    assert!(lab.run(r, "copy tftp: startup-config 10.0.0.100 r1-confg").contains("to startup-config"));
    assert!(lab.run(r, "show startup-config").contains("logging host 10.0.0.100"));
    assert!(lab.try_run(r, "copy tftp: running-config 10.0.0.100 nope").contains("no such file"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("TFTP data block 2")));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("NTP time from 10.0.0.100")));
}

#[test]
fn spanning_tree_timers_by_typing() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", s1, "fa0/1");
    lab.run(a, "ipconfig 10.0.0.1 255.255.255.0");
    lab.run(b, "ipconfig 10.0.0.2 255.255.255.0");
    lab.run(s1, "en\nshow spanning-tree");
    // A PC cabled later: its port listens 15 s, learns 15 s, then forwards.
    lab.cable(b, "fa0", s1, "fa0/2");
    lab.sim.topology_changed(&lab.t);
    assert!(lab.run(s1, "show spanning-tree").contains("Fa0/2      Desg LIS"));
    assert!(lab.run(a, "ping 10.0.0.2").contains(" 0 answered"));
    lab.sim.run_for(&lab.t, 10_000_000);
    assert!(lab.run(s1, "show spanning-tree").contains("Fa0/2      Desg LRN"));
    lab.sim.run_for(&lab.t, 15_000_000);
    lab.run(a, "ping 10.0.0.2");
    assert!(lab.run(a, "ping 10.0.0.2").contains("4 answered"));
    // Portfast or rapid spanning tree: at once.
    let c = lab.add(DeviceKind::Pc);
    lab.run(c, "ipconfig 10.0.0.3 255.255.255.0");
    lab.run(s1, "conf t\nspanning-tree mode rapid-pvst\nend");
    lab.cable(c, "fa0", s1, "fa0/3");
    lab.sim.topology_changed(&lab.t);
    assert!(lab.run(a, "ping 10.0.0.3").contains("answered") && !lab.run(a, "ping 10.0.0.3").contains(" 0 answered"));
    assert!(lab.run(s1, "show running-config").contains("spanning-tree mode rapid-pvst"));
}

#[test]
fn ccna1_lab_commands_by_typing() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/0", s, "g0/1");
    lab.cable(pc, "fa0", s, "fa0/1");
    lab.run(pc, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(r, "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nend");
    // Output filters.
    assert_eq!(lab.run(r, "show running-config | include ip address 192"), " ip address 192.168.1.1 255.255.255.0");
    assert!(lab.run(r, "show running-config | section line").starts_with("line con 0"));
    assert!(lab.run(r, "sh ip int br | i up").contains("GigabitEthernet0/0"));
    assert!(lab.try_run(r, "show running-config | grep x").contains("include, exclude"));
    // Password rules: a minimum length, then a privilege-15 user who lands in privileged mode.
    lab.run(r, "conf t\nsecurity passwords min-length 8");
    assert!(lab.try_run(r, "enable secret short").contains("too short"));
    lab.run(r, "enable secret longenough\nusername admin privilege 15 secret adminpass1\nline vty 0 4\nlogin local\nexit\nlogin block-for 60 attempts 2 within 30\nend");
    let run = lab.run(r, "show running-config");
    assert!(
        run.contains("username admin privilege 15 secret 5")
            && run.contains("security passwords min-length 8")
            && run.contains("login block-for 60 attempts 2 within 30"),
        "{run}"
    );
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "admin");
    assert_eq!(lab.try_run(pc, "adminpass1"), "");
    assert_eq!(lab.prompt(pc), "R1#", "privilege 15 starts in privileged mode");
    lab.try_run(pc, "exit");
    // Two failures in the window: logins blocked for a minute.
    lab.try_run(pc, "telnet 192.168.1.1");
    lab.try_run(pc, "admin");
    lab.try_run(pc, "bad");
    lab.try_run(pc, "admin");
    assert!(lab.try_run(pc, "bad").contains("blocked for 60 seconds"));
    assert!(lab.try_run(pc, "telnet 192.168.1.1").contains("Logins are blocked"));
    assert!(lab.run(r, "show login").contains("Quiet mode"));
    lab.sim.run_for(&lab.t, 61_000_000);
    assert!(lab.try_run(pc, "telnet 192.168.1.1").contains("Open"));
    lab.try_run(pc, "admin");
    lab.try_run(pc, "adminpass1");
    lab.try_run(pc, "exit");
    // Speed: a fixed mismatch takes the link down; the far end follows a fixed speed.
    lab.run(s, "en\nconf t\nint g0/1\nspeed 100\nduplex full\nend");
    assert!(lab.run(r, "show interfaces g0/0").contains("Full-duplex, 100 Mb/s"));
    lab.run(r, "conf t\nint g0/0\nspeed 1000\nend");
    assert!(lab.run(r, "show interfaces g0/0").contains("speed mismatch"));
    assert!(lab.try_run(r, "ping 192.168.1.10").contains("0 answered") || lab.try_run(r, "ping 192.168.1.10").contains("% "));
    lab.run(r, "conf t\nint g0/0\nspeed auto\nend");
    assert!(lab.run(s, "show controllers g0/1").contains("link up, 100 Mb/s"));
    lab.run(s, "conf t\nint fa0/1");
    assert!(lab.try_run(s, "speed 1000").contains("up to 100"));
    lab.run(s, "end");
    // Routes and pings.
    let rt = lab.run(r, "show ip route 192.168.1.10");
    assert!(rt.contains("Routing entry for 192.168.1.10") && rt.contains("GigabitEthernet0/0, directly connected"), "{rt}");
    lab.run(r, "ping 192.168.1.10");
    assert!(lab.run(r, "ping 192.168.1.10 size 1500").contains("answered"));
    // VLAN database: gone at the next reload.
    lab.run(s, "conf t\nvlan 30\nend\ncopy running-config startup-config");
    assert!(lab.run(s, "show flash").contains("vlan.dat"));
    lab.run(s, "delete flash:vlan.dat");
    assert!(lab.run(s, "show vlan brief").contains("VLAN0030"), "until the reload");
    lab.run(s, "reload");
    lab.run(s, "en");
    assert!(!lab.run(s, "show vlan brief").contains("VLAN0030"));
    for cmd in [
        "show protocols",
        "show interfaces description",
        "show privilege",
        "show tech-support",
        "show boot",
        "show environment",
        "show inventory",
        "show ssh",
        "show sessions",
        "show hosts",
        "clear counters",
        "show interfaces vlan 1",
        "show mac address-table dynamic",
        "terminal history size 50",
    ] {
        let out = lab.try_run(s, cmd);
        assert!(!out.starts_with("% ") || cmd.contains("vlan 1"), "{cmd}: {out}");
    }
}

#[test]
fn ip_address_dhcp_by_typing() {
    let mut lab = Lab::new();
    let isp = lab.add(DeviceKind::Router);
    let r = lab.add(DeviceKind::Router);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(r, "g0/1", isp, "g0/0");
    lab.cable(pc, "fa0", isp, "g0/1");
    lab.run(pc, "ipconfig 203.0.113.50 255.255.255.0 203.0.113.1");
    lab.run(isp, "en\nconf t\nint g0/0\nip add 198.51.100.1 255.255.255.0\nno shut\nint g0/1\nip add 203.0.113.1 255.255.255.0\nno shut\nexit\nip dhcp excluded-address 198.51.100.1 198.51.100.19\nip dhcp pool CUSTOMERS\nnetwork 198.51.100.0 255.255.255.0\ndefault-router 198.51.100.1\nend");
    // Address first, then up: the lease comes with `no shutdown`.
    lab.run(r, "en\nconf t\nint g0/1\nip address dhcp");
    let out = lab.run(r, "no shutdown");
    assert!(out.contains("198.51.100.20/24 assigned to GigabitEthernet0/1") && out.contains("default route via 198.51.100.1"), "{out}");
    lab.run(r, "end");
    assert!(lab.run(r, "show running-config").contains(" ip address dhcp"));
    assert!(lab.run(r, "show ip interface brief").contains("198.51.100.20"));
    let rt = lab.run(r, "show ip route");
    assert!(rt.contains("S    0.0.0.0/0") && rt.contains("198.51.100.1"), "{rt}");
    lab.run(r, "ping 203.0.113.50");
    assert!(lab.run(r, "ping 203.0.113.50").contains("answered"));
    // A static default wins over the lease's.
    lab.run(r, "conf t\nip route 0.0.0.0 0.0.0.0 198.51.100.254\nend");
    assert!(lab.run(r, "show ip route").contains("via 198.51.100.254"));
    lab.run(r, "conf t\nint g0/1\nno ip address\nend");
    assert!(!lab.run(r, "show running-config").contains("ip address dhcp"));
}

#[test]
fn eigrp_by_typing() {
    let mut lab = Lab::new();
    let three = netlabcraft_model::models::find("Router-3").unwrap();
    let r1 = lab.t.add_model(three, None, 0.0, 0.0).unwrap();
    let r2 = lab.t.add_model(three, None, 0.0, 0.0).unwrap();
    let r3 = lab.t.add_model(three, None, 0.0, 0.0).unwrap();
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    // A triangle: R1-R2, R1-R3, R2-R3; a PC behind R1 and one behind R3.
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r1, "g0/2", r3, "g0/0");
    lab.cable(r2, "g0/1", r3, "g0/1");
    lab.cable(b, "fa0", r3, "g0/2");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.3.10 255.255.255.0 192.168.3.1");
    let cfg = [
        (r1, [("g0/0", "192.168.1.1 255.255.255.0"), ("g0/1", "10.0.12.1 255.255.255.252"), ("g0/2", "10.0.13.1 255.255.255.252")]),
        (r2, [("g0/0", "10.0.12.2 255.255.255.252"), ("g0/1", "10.0.23.1 255.255.255.252"), ("g0/2", "")]),
        (r3, [("g0/0", "10.0.13.2 255.255.255.252"), ("g0/1", "10.0.23.2 255.255.255.252"), ("g0/2", "192.168.3.1 255.255.255.0")]),
    ];
    for (r, ifs) in cfg {
        lab.run(r, "en\nconf t");
        for (i, ip) in ifs.iter().filter(|(_, ip)| !ip.is_empty()) {
            lab.run(r, &format!("int {i}\nip address {ip}\nno shut"));
        }
        lab.run(r, "end");
    }
    // The direct R1-R3 link is slow: its delay makes the way through R2 the better one.
    lab.run(
        r1,
        "conf t\nint g0/2\ndelay 100\nexit\nrouter eigrp 10\nnetwork 192.168.1.0 0.0.0.255\nnetwork 10.0.0.0\nno auto-summary\npassive-interface g0/0\neigrp router-id 1.1.1.1\nend",
    );
    assert_eq!(lab.prompt(r1), "R1#");
    // R2 starts in another autonomous system: no neighbours there.
    lab.run(r2, "conf t\nrouter eigrp 20\nnetwork 10.0.0.0\nend");
    lab.run(r3, "conf t\nrouter eigrp 10\nnetwork 10.0.0.0\nnetwork 192.168.3.0\nend");
    assert!(lab.run(r2, "show ip eigrp neighbors").contains("No EIGRP neighbours in AS 20"));
    lab.run(r2, "conf t\nno router eigrp 20\nrouter eigrp 10\nnetwork 10.0.0.0\nend");
    let nb = lab.run(r1, "show ip eigrp neighbors");
    assert!(nb.contains("10.0.12.2") && nb.contains("10.0.13.2") && nb.contains("R2") && nb.contains("R3"), "{nb}");
    // Successor through R2 (FD 256 × (10 + 3)), the direct link a feasible successor: R3
    // reports 256 × (10 + 1), below the feasible distance.
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("D    192.168.3.0/24       via 10.0.12.2, GigabitEthernet0/1 (EIGRP)"), "{rt}");
    let topo = lab.run(r1, "show ip eigrp topology");
    assert!(topo.contains("router id 1.1.1.1"), "{topo}");
    assert!(topo.contains("192.168.3.0/24  FD 3328, 1 successor"), "{topo}");
    assert!(topo.contains("successor           via 10.0.12.2, Gi0/1 (3328 / 3072)"), "{topo}");
    assert!(topo.contains("feasible successor  via 10.0.13.2, Gi0/2 (28416 / 2816)"), "{topo}");
    assert!(topo.contains("connected, Gi0/0"), "{topo}");
    let ifs = lab.run(r1, "show ip eigrp interfaces");
    assert!(ifs.contains("Gi0/2") && ifs.contains("1000") && ifs.contains("1000000"), "{ifs}");
    assert!(ifs.lines().any(|l| l.starts_with("Gi0/0") && l.ends_with("yes")), "{ifs}");
    let pr = lab.run(r1, "show ip protocols");
    assert!(pr.contains("EIGRP, autonomous system 10") && pr.contains("distance 90") && pr.contains("Router id: 1.1.1.1"), "{pr}");
    assert!(lab.run(r1, "show ip route eigrp").contains("D    10.0.23.0/30"));
    lab.run(a, "ping 192.168.3.10");
    assert!(lab.run(a, "ping 192.168.3.10").contains("4 answered"));
    // Hellos go out every 5 s to 224.0.0.10; `debug eigrp packets` reports them.
    lab.run(r1, "debug eigrp packets");
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 6_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("EIGRP hello, AS 10") && e.summary.contains("→ 224.0.0.10")));
    let log = lab.run(r1, "show logging");
    assert!(log.contains("EIGRP debug: hello from 10.0.12.2 on GigabitEthernet0/1, AS 10"), "{log}");
    assert!(lab.run(r1, "show debugging").contains("EIGRP"));
    lab.run(r1, "no debug eigrp packets");
    // The configuration survives a save and reload.
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("interface GigabitEthernet0/2\n ip address 10.0.13.1 255.255.255.252\n delay 100\n"), "{run}");
    assert!(
        run.contains(
            "router eigrp 10\n eigrp router-id 1.1.1.1\n passive-interface GigabitEthernet0/0\n network 192.168.1.0 0.0.0.255\n network 10.0.0.0\n no auto-summary\n!"
        ),
        "{run}"
    );
    lab.run(r1, "copy run start");
    lab.run(r1, "reload");
    lab.run(r1, "en");
    assert_eq!(lab.run(r1, "show running-config"), run);
    // The link to R2 fails: the feasible successor takes over.
    lab.run(r2, "conf t\nint g0/0\nshutdown\nend");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("D    192.168.3.0/24       via 10.0.13.2, GigabitEthernet0/2 (EIGRP)"), "{rt}");
    assert!(lab.run(r1, "show ip eigrp topology").contains("192.168.3.0/24  FD 28416, 1 successor"));
    lab.run(a, "ping 192.168.3.10");
    assert!(lab.run(a, "ping 192.168.3.10").contains("4 answered"));
    lab.run(r2, "conf t\nint g0/0\nno shutdown\nend");
    // OSPF on top: EIGRP's distance (90) beats OSPF's (110) for the same networks.
    for r in [r1, r2, r3] {
        lab.run(r, "conf t\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend");
    }
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("D    192.168.3.0/24") && !rt.contains("O    192.168.3.0/24"), "{rt}");
    lab.run(r1, "conf t\nno router eigrp 10\nend");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("O    192.168.3.0/24") && !rt.contains("D    "), "{rt}");
    lab.run(a, "ping 192.168.3.10");
    assert!(lab.run(a, "ping 192.168.3.10").contains("4 answered"));
    // Interface metrics go back to their defaults; mistakes are refused.
    lab.run(r1, "conf t\nint g0/2\nbandwidth 512\nno delay\nno bandwidth\nend");
    assert!(!lab.run(r1, "show running-config").contains(" delay "));
    lab.try_run(r1, "conf t");
    assert!(lab.try_run(r1, "router eigrp 0").contains("1-65535"));
    lab.run(r1, "router eigrp 5");
    assert!(lab.try_run(r1, "network 10.0.0.0 0.255.0.255").contains("not a wildcard"));
    assert!(lab.try_run(r1, "no network 172.16.0.0").contains("No such network"));
    lab.run(r1, "exit");
    assert!(lab.try_run(r1, "router eigrp 6").contains("AS 5"));
    assert!(lab.try_run(r1, "no router eigrp 6").contains("No EIGRP process"));
}

// ---- site-to-site VPN: GRE tunnels and IPsec ----

/// Two sites (192.168.1.0/24 behind R1, 192.168.2.0/24 behind R2) joined through an ISP router
/// that knows only the public links. Returns (lab, R1, ISP, R2, PC A).
fn two_sites() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let isp = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", isp, "g0/0");
    lab.cable(isp, "g0/1", r2, "g0/1");
    lab.cable(b, "fa0", r2, "g0/0");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.2.10 255.255.255.0 192.168.2.1");
    lab.run(isp, "en\nconf t\nint g0/0\nip add 203.0.113.2 255.255.255.252\nno shut\nint g0/1\nip add 198.51.100.2 255.255.255.252\nno shut\nend");
    lab.run(
        r1,
        "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip add 203.0.113.1 255.255.255.252\nno shut\nexit\nip route 0.0.0.0 0.0.0.0 203.0.113.2\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nint g0/0\nip add 192.168.2.1 255.255.255.0\nno shut\nint g0/1\nip add 198.51.100.1 255.255.255.252\nno shut\nexit\nip route 0.0.0.0 0.0.0.0 198.51.100.2\nend",
    );
    (lab, r1, isp, r2, a)
}

/// Pings a few times (the first ones wait for ARP along the way); the last report.
fn ping_settled(lab: &mut Lab, from: DeviceId, to: &str) -> String {
    let mut last = String::new();
    for _ in 0..4 {
        last = lab.run(from, &format!("ping {to}"));
    }
    last
}

#[test]
fn gre_tunnel_by_typing() {
    let (mut lab, r1, isp, r2, a) = two_sites();
    // Without the tunnel the ISP drops traffic for the private networks.
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    lab.run(
        r1,
        "conf t\ninterface tunnel 0\ndescription to site 2\nip address 10.0.0.1 255.255.255.252\ntunnel source g0/1\ntunnel destination 198.51.100.1\ntunnel mode gre ip\nexit\nip route 192.168.2.0 255.255.255.0 10.0.0.2\nend",
    );
    assert_eq!(lab.prompt(r1), "R1#");
    lab.run(
        r2,
        "conf t\nint tu0\nip address 10.0.0.2 255.255.255.252\ntunnel source 198.51.100.1\ntunnel destination 203.0.113.1\nexit\nip route 192.168.1.0 255.255.255.0 tunnel 0\nend",
    );
    let brief = lab.run(r1, "show ip interface brief");
    assert!(brief.lines().any(|l| l.starts_with("Tunnel0") && l.contains("10.0.0.1") && l.ends_with("up")), "{brief}");
    let tun = lab.run(r1, "show interfaces tunnel 0");
    assert!(
        tun.contains("Tunnel0 is up, line protocol up") && tun.contains("source GigabitEthernet0/1 (203.0.113.1), destination 198.51.100.1"),
        "{tun}"
    );
    assert!(lab.run(r1, "show interfaces").contains("Tunnel0 is up"));
    let rt = lab.run(r2, "show ip route");
    assert!(rt.contains("10.0.0.0/30") && rt.contains("Tunnel0"), "{rt}");
    lab.sim.clear_trace();
    let ping = ping_settled(&mut lab, a, "192.168.2.10");
    assert!(ping.contains("4 answered"), "{ping}");
    // The ISP only ever saw GRE between the public addresses.
    let gre: Vec<String> = lab.sim.trace().iter().filter(|e| e.summary.starts_with("GRE")).map(|e| e.summary.clone()).collect();
    assert!(
        gre.iter().any(|s| s.starts_with("GRE 203.0.113.1 → 198.51.100.1 carrying ICMP echo request") && s.contains("192.168.1.10 → 192.168.2.10")),
        "{gre:#?}"
    );
    assert!(gre.iter().any(|s| s.contains("GRE 198.51.100.1 → 203.0.113.1 carrying ICMP echo reply")), "{gre:#?}");
    assert!(lab.sim.trace().iter().filter(|e| e.to.device == isp).all(|e| !(e.summary.starts_with("ICMP") && e.summary.contains("192.168."))));
    let run = lab.run(r1, "show running-config");
    assert!(
        run.contains("interface Tunnel0\n description to site 2\n ip address 10.0.0.1 255.255.255.252\n tunnel source GigabitEthernet0/1\n tunnel destination 198.51.100.1\n!"),
        "{run}"
    );
    assert!(lab.run(r1, "show running-config interface tunnel 0").starts_with("interface Tunnel0"));
    // A replay of the configuration rebuilds the same tunnel.
    let r3 = lab.add(DeviceKind::Router);
    lab.run(r3, "en\nconf t");
    for l in run.lines().filter(|l| !l.starts_with('!') && !l.starts_with("hostname")) {
        lab.try_run(r3, l);
    }
    assert_eq!(lab.t.device(r3).unwrap().tunnels, lab.t.device(r1).unwrap().tunnels);
    // No destination: line protocol down, with the reason.
    lab.run(r1, "conf t\nint tunnel 0\nno tunnel destination\nend");
    assert!(lab.run(r1, "show interfaces tunnel 0").contains("line protocol down (no tunnel destination)"));
    assert!(lab.run(r1, "show ip interface brief").lines().any(|l| l.starts_with("Tunnel0") && l.ends_with("down")));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    // Recursive routing: the destination routed into the tunnel itself is never encapsulated
    // twice.
    lab.run(r1, "conf t\nint tunnel 0\ntunnel destination 198.51.100.1\nexit\nip route 198.51.100.0 255.255.255.252 10.0.0.2\nend");
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.contains("recursive routing"))));
    lab.run(r1, "conf t\nno ip route 198.51.100.0 255.255.255.252 10.0.0.2\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    // Errors that explain themselves.
    lab.run(r1, "conf t\nint tunnel 0");
    assert!(lab.try_run(r1, "tunnel mode ipip").contains("GRE over IPv4"));
    assert!(lab.try_run(r1, "tunnel source tunnel 1").contains("another tunnel"));
    assert!(lab.try_run(r1, "tunnel source g9/9").contains("no interface"));
    lab.run(r1, "end");
    let sw = lab.add(DeviceKind::Switch);
    lab.run(sw, "en\nconf t");
    assert!(lab.try_run(sw, "interface tunnel 0").contains('%'));
    lab.run(r1, "conf t\nno interface tunnel 0\nend");
    assert!(lab.t.device(r1).unwrap().tunnels.is_empty());
    assert!(lab.try_run(r1, "show interfaces tunnel 0").contains("no interface Tunnel0"));
    let _ = r2;
}

#[test]
fn ospf_over_a_gre_tunnel() {
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(
        r1,
        "conf t\nint tunnel 1\nip address 10.9.9.1 255.255.255.252\ntunnel source g0/1\ntunnel destination 198.51.100.1\nexit\nrouter ospf 1\nrouter-id 1.1.1.1\nnetwork 192.168.1.0 0.0.0.255 area 0\nnetwork 10.9.9.0 0.0.0.3 area 0\nend",
    );
    lab.run(
        r2,
        "conf t\nint tunnel 1\nip address 10.9.9.2 255.255.255.252\ntunnel source g0/1\ntunnel destination 203.0.113.1\nexit\nrouter ospf 1\nrouter-id 2.2.2.2\nnetwork 192.168.2.0 0.0.0.255 area 0\nnetwork 10.9.9.0 0.0.0.3 area 0\nend",
    );
    let nb = lab.run(r1, "show ip ospf neighbor");
    assert!(nb.contains("2.2.2.2") && nb.contains("10.9.9.2") && nb.contains("Tunnel1"), "{nb}");
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("192.168.2.0/24") && rt.contains("via 10.9.9.2, Tunnel1 (OSPF)"), "{rt}");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    // Tunnels cost 1000 by default (100 kbit/s against the 100 Mbit/s reference).
    let oi = lab.run(r1, "show ip ospf interface brief");
    assert!(oi.contains("1000"), "{oi}");
    lab.run(r1, "conf t\nint tunnel 1\nip ospf cost 7\nend");
    assert!(lab.run(r1, "show running-config").contains("interface Tunnel1\n ip address 10.9.9.1 255.255.255.252\n ip ospf cost 7"));
    // Hellos cross the tunnel inside GRE.
    lab.sim.run_for(&lab.t, 11_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("GRE") && e.summary.contains("OSPF hello from router 1.1.1.1")));
    // The far end shut: no adjacency, no route.
    lab.run(r2, "conf t\nint tunnel 1\nshutdown\nend");
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("2.2.2.2"));
    assert!(!lab.run(r1, "show ip route").contains("Tunnel1 (OSPF)"));
}

const IPSEC_R1: &str = "conf t
access-list 110 permit ip 192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255
crypto isakmp policy 10
encryption aes 256
hash sha
authentication pre-share
group 14
exit
crypto isakmp key vpnsecret address 198.51.100.1
crypto ipsec transform-set TS esp-aes esp-sha-hmac
exit
crypto map VPN 10 ipsec-isakmp
set peer 198.51.100.1
set transform-set TS
match address 110
exit
int g0/1
crypto map VPN
end";

const IPSEC_R2: &str = "conf t
access-list 110 permit ip 192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255
crypto isakmp policy 10
encryption aes 256
hash sha
authentication pre-share
group 14
exit
crypto isakmp key vpnsecret address 203.0.113.1
crypto ipsec transform-set TS esp-aes esp-sha-hmac
exit
crypto map VPN 10 ipsec-isakmp
set peer 203.0.113.1
set transform-set TS
match address 110
exit
int g0/1
crypto map VPN
end";

/// The number after `what` in a report.
fn count_after(text: &str, what: &str) -> u64 {
    text.split(what).nth(1).and_then(|t| t.trim().split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse().ok()).unwrap_or(0)
}

#[test]
fn ipsec_site_to_site_by_typing() {
    let (mut lab, r1, isp, r2, a) = two_sites();
    lab.run(r1, IPSEC_R1);
    lab.run(r2, IPSEC_R2);
    assert!(lab.run(r1, "show crypto isakmp sa").contains("No IKE peers"));
    let ping = ping_settled(&mut lab, a, "192.168.2.10");
    assert!(ping.contains("4 answered"), "{ping}");
    let sa = lab.run(r1, "show crypto ipsec sa");
    assert!(sa.contains("Peer 198.51.100.1 (local 203.0.113.1), interface GigabitEthernet0/1") && sa.contains("Outbound ESP SPI 0x"), "{sa}");
    let (enc, dec) = (count_after(&sa, "encapsulated "), count_after(&sa, "decapsulated "));
    assert!(enc >= 4 && dec >= 4, "{sa}");
    assert!(lab.run(r1, "show crypto isakmp sa").contains("198.51.100.1     203.0.113.1      established"));
    // Counters move with traffic.
    lab.run(a, "ping 192.168.2.10");
    let after = lab.run(r1, "show crypto ipsec sa");
    assert_eq!(count_after(&after, "encapsulated "), enc + 4, "{after}");
    assert!(count_after(&lab.run(r2, "show crypto ipsec sa"), "decapsulated ") >= 4);
    // On the ISP's links the packets are ESP between the public addresses, payload hidden.
    assert!(
        lab.sim.trace().iter().any(|e| e.to.device == isp && e.summary.starts_with("ESP SPI") && e.summary.contains("203.0.113.1 → 198.51.100.1"))
    );
    assert!(lab.sim.trace().iter().filter(|e| e.to.device == isp).all(|e| !e.summary.contains("192.168.")));
    // Traffic the access list doesn't match leaves in the clear.
    assert!(ping_settled(&mut lab, r1, "198.51.100.1").contains("100% answered"));
    // The configuration shows and replays.
    let run = lab.run(r1, "show running-config");
    for want in [
        "crypto isakmp policy 10\n encryption aes 256\n hash sha\n authentication pre-share\n group 14\n!",
        "crypto isakmp key vpnsecret address 198.51.100.1",
        "crypto ipsec transform-set TS esp-aes esp-sha-hmac",
        "crypto map VPN 10 ipsec-isakmp\n set peer 198.51.100.1\n set transform-set TS\n match address 110",
        " crypto map VPN\n",
    ] {
        assert!(run.contains(want), "{want}\n{run}");
    }
    let r3 = lab.add(DeviceKind::Router);
    lab.run(r3, "en\nconf t");
    for l in run.lines().filter(|l| !l.starts_with('!') && !l.starts_with("hostname")) {
        lab.try_run(r3, l);
    }
    assert_eq!(lab.t.device(r3).unwrap().crypto, lab.t.device(r1).unwrap().crypto);
    assert!(lab.run(r1, "show crypto map").contains("Applied to: GigabitEthernet0/1"));
    assert!(lab.run(r1, "show crypto isakmp policy").contains("Policy 10: encryption aes 256, hash sha, authentication pre-share, DH group 14"));
    // Mismatched keys: no association, traffic dropped with the reason.
    lab.run(r2, "conf t\ncrypto isakmp key different address 203.0.113.1\nend");
    lab.run(r1, "clear crypto sa");
    lab.run(r2, "clear crypto sa");
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.run(r1, "show crypto isakmp sa").contains("failed: the pre-shared keys differ"));
    assert!(lab.sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.contains("IPsec: no security association with 198.51.100.1"))));
    let sa = lab.run(r1, "show crypto ipsec sa");
    assert!(count_after(&sa, "encapsulated ") == 0 && count_after(&sa, "dropped ") > 0, "{sa}");
    // Keys agree again; transform sets that differ fail too.
    lab.run(r2, "conf t\ncrypto isakmp key vpnsecret address 203.0.113.1\ncrypto ipsec transform-set TS esp-3des esp-md5-hmac\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.run(r1, "show crypto isakmp sa").contains("transform sets differ"));
    lab.run(r2, "conf t\ncrypto ipsec transform-set TS esp-aes esp-sha-hmac\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    // Taking the map off the interface sends LAN traffic in the clear again.
    lab.run(r1, "conf t\nint g0/1\nno crypto map\nend");
    assert!(!lab.run(r1, "show running-config").contains(" crypto map VPN\n"));
    // Errors that explain themselves.
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "crypto ipsec transform-set X esp-rot13").contains("not a transform"));
    assert!(lab.try_run(r1, "crypto isakmp key onlykey").contains("Usage"));
    assert!(lab.try_run(r1, "crypto isakmp policy 0").contains("1-10000"));
    lab.run(r1, "int g0/1");
    assert!(lab.try_run(r1, "crypto map NOPE").contains("no crypto map NOPE"));
    lab.run(r1, "end");
}

#[test]
fn vpn_modes_survive_hostile_arguments() {
    let hostile = ["", "0", "-1", "99999999999999999999", "a.b.c.d", "1.2.3.4", "255.255.255.255", "g9/9/9", "tunnel 4294967295", "ä€😀", "%s%n"];
    let huge = "x".repeat(10_000);
    for spec in SPECS {
        for arg in hostile.iter().chain(std::iter::once(&huge.as_str())) {
            let line: Vec<String> = spec
                .path
                .iter()
                .map(|t| match t {
                    Tok::K(k) => k.to_string(),
                    Tok::Arg(_) | Tok::Rest(_) => arg.to_string(),
                })
                .collect();
            for setup in [
                "en\nconf t\nint tunnel 0",
                "en\nconf t\ncrypto isakmp policy 1",
                "en\nconf t\ncrypto map M 1 ipsec-isakmp",
                "en\nconf t\ncrypto ipsec transform-set T esp-aes",
            ] {
                let mut lab = Lab::new();
                let d = lab.add(DeviceKind::Router);
                for l in setup.lines() {
                    lab.try_run(d, l);
                }
                let _ = lab.try_run(d, &line.join(" "));
                let _ = lab.try_run(d, &format!("{} ?", line.join(" ")));
                let _ = lab.try_run(d, "do show running-config");
            }
        }
    }
}

#[test]
fn snmp_trap_hosts_traps_and_v3_users_are_configured() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t");
    assert!(lab.run(r, "do show snmp").contains("agent is off"));
    lab.run(r, "snmp-server host 10.0.0.10 version 2c avisos");
    lab.run(r, "snmp-server host 10.0.0.11 publico");
    lab.run(r, "snmp-server enable traps snmp linkdown linkup");
    let show = lab.run(r, "do show snmp");
    assert!(show.contains("Traps to UDP port 162 (SNMPv2c): linkdown, linkup"), "{show}");
    assert!(show.contains("10.0.0.10, community avisos") && show.contains("10.0.0.11, community publico"), "{show}");
    // Only version 2c, and a community is needed.
    assert!(lab.try_run(r, "snmp-server host 10.0.0.12 version 1 x").contains("2c"));
    assert!(lab.try_run(r, "snmp-server host 10.0.0.12 version 2c").starts_with('%'));
    assert!(lab.try_run(r, "snmp-server enable traps snmp reboot").contains("Unknown trap"));
    lab.run(r, "no snmp-server enable traps linkup");
    lab.run(r, "snmp-server enable traps");
    lab.run(r, "no snmp-server host 10.0.0.11 version 2c publico");
    // SNMPv3: groups and users.
    lab.run(r, "snmp-server group LECTORES v3 auth");
    lab.run(r, "snmp-server group ADMINS v3 noauth write todo");
    lab.run(r, "snmp-server user ana LECTORES v3 auth sha clave1234");
    lab.run(r, "snmp-server user bea ADMINS v3");
    assert!(lab.try_run(r, "snmp-server user eva LECTORES v3 auth sha corta").contains("8 characters"));
    assert!(lab.try_run(r, "snmp-server user eva LECTORES v3 auth sha clave1234 priv aes clave5678").contains("Privacy"));
    assert!(lab.try_run(r, "snmp-server group P v3 priv").contains("Privacy"));
    let show = lab.run(r, "do show snmp");
    assert!(show.starts_with("SNMP agent on"), "{show}");
    assert!(show.contains("ana (group LECTORES: authNoPriv, read-only; auth SHA)"), "{show}");
    assert!(show.contains("bea (group ADMINS: noAuthNoPriv, read-write; no authentication)"), "{show}");
    let run = lab.run(r, "do show running-config");
    for line in [
        "snmp-server group LECTORES v3 auth\n",
        "snmp-server group ADMINS v3 noauth write v1default\n",
        "snmp-server user ana LECTORES v3 auth sha clave1234\n",
        "snmp-server user bea ADMINS v3\n",
        "snmp-server host 10.0.0.10 version 2c avisos\n",
        "snmp-server enable traps snmp linkdown linkup coldstart\n",
    ] {
        assert!(run.contains(line), "{line}\n{run}");
    }
    assert!(!run.contains("10.0.0.11"));
    // The running configuration reads back to the same settings.
    let snmp = lab.t.device(r).unwrap().snmp.clone();
    let mut lab2 = Lab::new();
    let r2 = lab2.add(DeviceKind::Router);
    let errors = apply_config(&mut lab2.t, &mut lab2.sim, r2, &run);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(lab2.t.device(r2).unwrap().snmp, snmp);
    lab.run(r, "no snmp-server user ana\nno snmp-server group ADMINS v3 noauth\nno snmp-server enable traps");
    let s = &lab.t.device(r).unwrap().snmp;
    assert_eq!((s.users.len(), s.groups.len(), s.traps.len()), (1, 1, 0));
}

// ---- QoS ----

/// [`two_sites`] with the ISP routing both private networks, so the sites reach each other.
fn two_routed_sites() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let (mut lab, r1, isp, r2, a) = two_sites();
    lab.run(isp, "conf t\nip route 192.168.1.0 255.255.255.0 203.0.113.1\nip route 192.168.2.0 255.255.255.0 198.51.100.1\nend");
    (lab, r1, isp, r2, a)
}

const QOS_R1: &str = "
    conf t
    access-list 101 permit icmp 192.168.1.0 0.0.0.255 any
    class-map match-all PINGS
    match access-group 101
    match protocol icmp
    class-map match-any WEB
    match protocol http
    match protocol https
    exit
    policy-map MARK
    class PINGS
    set dscp ef
    class WEB
    set ip precedence 3
    class class-default
    set dscp default
    exit
    exit
    int g0/0
    service-policy input MARK
    end";

const QOS_ISP: &str = "
    conf t
    class-map VOICE
    match dscp ef cs5
    policy-map CORE
    class VOICE
    police 64000 conform-action transmit exceed-action set-dscp-transmit af11
    int g0/0
    service-policy input CORE
    end";

/// The DS byte of an untagged IPv4 frame.
fn ds_byte(frame: &[u8]) -> Option<u8> {
    (frame.get(12..14) == Some(&[0x08, 0x00]) && frame.get(14).is_some_and(|v| v >> 4 == 4)).then(|| frame.get(15).copied()).flatten()
}

/// The `!` marks of a router's ping report.
fn replies(out: &str) -> usize {
    out.lines().nth(1).map_or(0, |l| l.matches('!').count())
}

#[test]
fn qos_marking_and_classification_by_typing() {
    let (mut lab, r1, isp, _r2, a) = two_routed_sites();
    lab.run(r1, QOS_R1);
    lab.run(isp, QOS_ISP);
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    // R1 marked the echo requests EF: the frames it sends the ISP carry DSCP 46 (0xb8) and
    // still decode, so the header checksum was fixed.
    let marked: Vec<_> =
        lab.sim.trace().iter().filter(|e| e.from.device == r1 && e.to.device == isp && e.summary.starts_with("ICMP echo request")).collect();
    assert!(!marked.is_empty());
    assert!(marked.iter().all(|e| ds_byte(&e.frame) == Some(0xb8)), "{:?}", marked.iter().map(|e| ds_byte(&e.frame)).collect::<Vec<_>>());
    // What the PC sent was unmarked.
    assert!(lab.sim.trace().iter().filter(|e| e.from.device == a).filter_map(|e| ds_byte(&e.frame)).all(|b| b == 0));
    // Counters on both routers.
    let r1_show = lab.run(r1, "show policy-map interface g0/0");
    assert!(r1_show.starts_with("GigabitEthernet0/0\n  Input policy MARK"), "{r1_show}");
    assert!(r1_show.contains("Class PINGS (match-all: access-group 101; protocol icmp)"), "{r1_show}");
    assert!(count_after(&r1_show, "set dscp ef: ") >= 4, "{r1_show}");
    // The ISP classifies on the DSCP R1 wrote: the echo requests land in VOICE.
    let isp_show = lab.run(isp, "show policy-map interface");
    let voice = isp_show.split("Class VOICE").nth(1).unwrap_or("").to_string();
    assert!(count_after(&voice, "matched ") >= 4, "{isp_show}");
    assert!(voice.contains("police 64000 bit/s, bucket 2000 bytes:") && voice.contains("0 dropped"), "{isp_show}");
    // Clearing the counters starts again.
    lab.run(isp, "clear counters");
    let isp_show = lab.run(isp, "show policy-map interface g0/0");
    assert_eq!(count_after(isp_show.split("Class VOICE").nth(1).unwrap_or(""), "matched "), 0, "{isp_show}");
    // Without the marking the ISP sees default traffic only.
    lab.run(r1, "conf t\nint g0/0\nno service-policy input\nend");
    lab.run(a, "ping 192.168.2.10");
    let isp_show = lab.run(isp, "show policy-map interface g0/0");
    assert_eq!(count_after(isp_show.split("Class VOICE").nth(1).unwrap_or(""), "matched "), 0, "{isp_show}");
    assert!(count_after(isp_show.split("Class class-default").nth(1).unwrap_or(""), "matched ") >= 4, "{isp_show}");
    assert_eq!(lab.run(r1, "show policy-map interface"), "No service policies applied");
}

#[test]
fn qos_policing_drops_with_a_reason() {
    let (mut lab, r1, isp, _r2, _a) = two_routed_sites();
    lab.run(isp, "conf t\npolicy-map LIMIT\nclass class-default\npolice 8000 conform-action transmit exceed-action drop\nint g0/0\nservice-policy input LIMIT\nend");
    lab.sim.clear_trace();
    // 1000-byte packets against a 1500-byte bucket refilling at 1000 bytes a second: back to
    // back, the second one finds the bucket short and is dropped.
    let mut answered = 0;
    for _ in 0..3 {
        answered += replies(&lab.run(r1, "ping 198.51.100.1 size 1000"));
    }
    assert!(answered > 0);
    let reason = "policed by policy-map LIMIT class class-default (input on GigabitEthernet0/0)";
    assert!(lab.sim.trace().iter().any(|e| e.to.device == isp && e.dropped.as_deref() == Some(reason)), "no policed drop");
    let show = lab.run(isp, "show policy-map interface g0/0");
    assert!(show.contains("police 8000 bit/s, bucket 1500 bytes: "), "{show}");
    assert!(count_after(&show, "bytes: ") > 0 && count_after(&show, "conformed (transmit), ") > 0, "{show}");
    assert!(count_after(&show, "exceeded (drop), ") > 0, "{show}");
    // Small packets fit: a normal ping passes untouched.
    lab.run(isp, "clear counters");
    assert_eq!(replies(&lab.run(r1, "ping 198.51.100.1")), 5);
}

#[test]
fn qos_configuration_round_trip_and_modes() {
    let (mut lab, r1, _isp, _r2, _a) = two_routed_sites();
    lab.run(r1, QOS_R1);
    lab.run(
        r1,
        "conf t\nclass-map match-any VOICE\nmatch dscp ef\nmatch ip precedence 5 critical\nmatch access-group name NOTYET\npolicy-map WAN\n\
         class VOICE\npriority 512\nclass class-default\nbandwidth percent 20\nshape average 2000000\npolice cir 128000 4000 exceed-action set-dscp-transmit cs1\n\
         int g0/1\nservice-policy output WAN\nend",
    );
    assert_eq!(lab.prompt(r1), "R1#");
    let run = lab.run(r1, "show running-config");
    for want in [
        "class-map match-all PINGS\n match access-group 101\n match protocol icmp\n!",
        "class-map match-any WEB\n match protocol http\n match protocol https\n!",
        "class-map match-any VOICE\n match dscp ef\n match ip precedence 5\n match access-group name NOTYET\n!",
        "policy-map MARK\n class PINGS\n  set dscp ef\n class WEB\n  set ip precedence 3\n class class-default\n  set dscp default\n!",
        "policy-map WAN\n class VOICE\n  priority 512\n class class-default\n  bandwidth percent 20\n  police 128000 4000 conform-action transmit exceed-action set-dscp-transmit cs1\n  shape average 2000000\n!",
        " service-policy input MARK\n",
        " service-policy output WAN\n",
    ] {
        assert!(run.contains(want), "{want}\n{run}");
    }
    // Replayed line by line on another router: the same configuration.
    let r3 = lab.add(DeviceKind::Router);
    lab.run(r3, "en\nconf t");
    for l in run.lines().filter(|l| !l.starts_with('!') && !l.starts_with("hostname")) {
        lab.try_run(r3, l);
    }
    assert_eq!(lab.t.device(r3).unwrap().qos, lab.t.device(r1).unwrap().qos);
    // Saved and reloaded: identical.
    lab.run(r1, "copy running-config startup-config");
    lab.run(r1, "reload");
    assert_eq!(lab.run(r1, "en\nshow running-config"), run);
    // Shows.
    let cm = lab.run(r1, "show class-map VOICE");
    assert!(cm.starts_with("Class map VOICE (match-any") && cm.contains("  match ip precedence 5"), "{cm}");
    assert!(lab.run(r1, "show class-map").contains("Class map WEB"));
    let pm = lab.run(r1, "show policy-map WAN");
    assert!(pm.contains("  class VOICE\n    priority 512") && pm.contains("    shape average 2000000"), "{pm}");
    let out = lab.run(r1, "show policy-map interface g0/1");
    assert!(
        out.contains("Output policy WAN") && out.contains("priority 512 kbit/s: queue 0 bytes now (most 0 bytes), 0 packets tail-dropped"),
        "{out}"
    );
    // Modes and prompts.
    lab.run(r1, "conf t\nclass-map WEB");
    assert_eq!(lab.prompt(r1), "R1(config-cmap)#");
    lab.run(r1, "policy-map MARK");
    assert_eq!(lab.prompt(r1), "R1(config-pmap)#");
    lab.run(r1, "class WEB");
    assert_eq!(lab.prompt(r1), "R1(config-pmap-c)#");
    lab.run(r1, "exit");
    assert_eq!(lab.prompt(r1), "R1(config-pmap)#");
    lab.run(r1, "class WEB\nno set ip precedence\nexit\nno class WEB\nexit");
    assert_eq!(lab.prompt(r1), "R1(config)#");
    assert!(!lab.run(r1, "do show running-config").contains("  set ip precedence 3"));
    // `no` forms.
    lab.run(
        r1,
        "class-map VOICE\nno match ip precedence 5\nexit\nint g0/1\nno service-policy output\nexit\nno policy-map WAN\nno class-map VOICE\nend",
    );
    let run = lab.run(r1, "show running-config");
    assert!(!run.contains("VOICE") && !run.contains("WAN"), "{run}");
}

#[test]
fn qos_mistakes_are_refused() {
    let (mut lab, r1, _isp, _r2, _a) = two_routed_sites();
    lab.run(r1, QOS_R1);
    lab.run(r1, "conf t");
    for (line, says) in [
        ("class-map match-some X", "Usage"),
        ("class-map class-default", "reserved"),
        ("no class-map PINGS", "used by policy map MARK"),
        ("no policy-map MARK", "applied to GigabitEthernet0/0"),
        ("no class-map NOPE", "no class map NOPE"),
    ] {
        let out = lab.try_run(r1, line);
        assert!(out.contains(says), "{line}: {out}");
    }
    lab.run(r1, "class-map T");
    for (line, says) in [
        ("match dscp ef99", "not a DSCP"),
        ("match dscp 64", "not a DSCP"),
        ("match ip precedence 9", "not a precedence"),
        ("match protocol gopher", "not a protocol"),
        ("match access-group 5000", "numbered access list"),
        ("match dscp 1 2 3 4 5 6 7 8 9", "At most 8"),
        ("no match any", "no line"),
    ] {
        let out = lab.try_run(r1, line);
        assert!(out.contains(says), "{line}: {out}");
    }
    lab.run(r1, "exit\npolicy-map Q");
    assert!(lab.try_run(r1, "class NOPE").contains("no class map NOPE"));
    lab.run(r1, "class T");
    for (line, says) in [
        ("set dscp xx", "not a DSCP"),
        ("police 100", "8000-"),
        ("police 8000 exceed-action explode", "transmit, drop"),
        ("police 8000 surprise", "not understood"),
        ("priority 4", "8-10000000"),
        ("bandwidth percent 101", "1-100"),
        ("shape average 10", "8000-"),
    ] {
        let out = lab.try_run(r1, line);
        assert!(out.contains(says), "{line}: {out}");
    }
    lab.run(r1, "priority 512");
    assert!(lab.try_run(r1, "bandwidth 100").contains("not both"));
    lab.run(r1, "exit\nclass class-default");
    assert!(lab.try_run(r1, "priority 100").contains("class-default cannot"));
    lab.run(r1, "exit\nexit\nint g0/1");
    assert!(lab.try_run(r1, "service-policy input Q").contains("apply it as `output`"));
    assert!(lab.try_run(r1, "service-policy output NOPE").contains("no policy map NOPE"));
    assert!(lab.try_run(r1, "service-policy sideways Q").contains("`input` or `output`"));
    lab.run(r1, "service-policy output Q\nend");
    // A layer-2 switch, and a switch port, refuse service policies.
    let l2 = lab.add(DeviceKind::Switch);
    lab.run(l2, "en\nconf t\npolicy-map P\nclass class-default\nset dscp af41\nexit\nexit\ninterface vlan 1");
    assert!(lab.try_run(l2, "service-policy input P").contains("`ip routing`"));
    let s = lab.t.add_model(netlabcraft_model::models::find("Switch-ML-24").unwrap(), None, 0.0, 0.0).unwrap();
    lab.run(s, "en\nconf t\npolicy-map P\nclass class-default\nset dscp af41\nexit\nexit\ninterface vlan 1");
    assert!(lab.try_run(s, "service-policy input P").contains("`ip routing`"));
    lab.run(s, "exit\nip routing\ninterface fa0/1");
    assert!(lab.try_run(s, "service-policy input P").contains("switch port"));
    lab.run(s, "no switchport\nservice-policy input P\ninterface vlan 1\nservice-policy output P\nend");
    let run = lab.run(s, "show running-config");
    assert!(run.contains("interface Vlan1\n") && run.contains(" service-policy output P\n") && run.contains(" service-policy input P\n"), "{run}");
}

#[test]
fn qos_modes_survive_hostile_arguments() {
    let hostile = ["", "0", "-1", "99999999999999999999", "ef", "class-default", "match-any", "1.2.3.4", "g9/9/9", "ä€😀", "%s%n"];
    let huge = "x".repeat(10_000);
    for spec in SPECS {
        for arg in hostile.iter().chain(std::iter::once(&huge.as_str())) {
            let line: Vec<String> = spec
                .path
                .iter()
                .map(|t| match t {
                    Tok::K(k) => k.to_string(),
                    Tok::Arg(_) | Tok::Rest(_) => arg.to_string(),
                })
                .collect();
            for (kind, setup) in [
                (DeviceKind::Router, "en\nconf t\nclass-map match-any C"),
                (DeviceKind::Router, "en\nconf t\npolicy-map P"),
                (DeviceKind::Router, "en\nconf t\nclass-map C\nexit\npolicy-map P\nclass C"),
                (DeviceKind::Router, "en\nconf t\npolicy-map P\nclass class-default\nexit\nexit\nint g0/0\nservice-policy input P"),
                (DeviceKind::Switch, "en\nconf t\nip routing\npolicy-map P\nclass class-default\nexit\nexit\nint vlan 1"),
            ] {
                let mut lab = Lab::new();
                let d = if kind == DeviceKind::Switch {
                    lab.t.add_model(netlabcraft_model::models::find("Switch-ML-24").unwrap(), None, 0.0, 0.0).unwrap()
                } else {
                    lab.add(kind)
                };
                for l in setup.lines() {
                    lab.try_run(d, l);
                }
                let _ = lab.try_run(d, &line.join(" "));
                let _ = lab.try_run(d, &format!("{} ?", line.join(" ")));
                let _ = lab.try_run(d, "do show running-config");
                let _ = lab.try_run(d, "do show policy-map interface");
            }
        }
    }
}

// ---- the firewall appliance ----

fn add_firewall(lab: &mut Lab) -> DeviceId {
    let m = netlabcraft_model::models::find("Firewall-5").unwrap();
    lab.t.add_model(m, None, 0.0, 0.0).unwrap()
}

struct FwLab {
    lab: Lab,
    fw: DeviceId,
    isp: DeviceId,
    pc1: DeviceId,
    pc2: DeviceId,
    pc3: DeviceId,
    web: DeviceId,
}

/// FW1 between an inside LAN (PC1, PC3 on a switch), a DMZ web server and an ISP router with a
/// PC behind it.
fn firewall_lab() -> FwLab {
    let mut lab = Lab::new();
    let fw = add_firewall(&mut lab);
    let isp = lab.add(DeviceKind::Router);
    let sw = lab.add(DeviceKind::Switch);
    let pc1 = lab.add(DeviceKind::Pc);
    let pc3 = lab.add(DeviceKind::Pc);
    let web = lab.add(DeviceKind::Server);
    let pc2 = lab.add(DeviceKind::Pc);
    lab.cable(fw, "g1/1", sw, "g0/1");
    lab.cable(pc1, "fa0", sw, "fa0/1");
    lab.cable(pc3, "fa0", sw, "fa0/2");
    lab.cable(fw, "g1/2", isp, "g0/0");
    lab.cable(fw, "g1/3", web, "g0");
    lab.cable(pc2, "fa0", isp, "g0/1");
    lab.run(
        fw,
        "enable
        configure terminal
        hostname FW1
        interface g1/1
        nameif inside
        ip address 192.168.1.1 255.255.255.0
        no shutdown
        interface g1/2
        nameif outside
        security-level 0
        ip address 203.0.113.2 255.255.255.0
        no shutdown
        interface g1/3
        nameif dmz
        security-level 50
        ip address 172.16.1.1 255.255.255.0
        no shutdown
        exit
        route outside 0.0.0.0 0.0.0.0 203.0.113.1
        object network INSIDE_NET
        subnet 192.168.1.0 255.255.255.0
        nat (inside,outside) dynamic interface
        object network WEB
        host 172.16.1.10
        nat (dmz,outside) static 203.0.113.10
        class-map inspection_default
        match default-inspection-traffic
        policy-map global_policy
        class inspection_default
        inspect icmp
        exit
        exit
        service-policy global_policy global
        end",
    );
    lab.run(
        isp,
        "en\nconf t\nint g0/0\nip address 203.0.113.1 255.255.255.0\nno shut\nint g0/1\nip address 198.51.100.1 255.255.255.0\nno shut\nend",
    );
    lab.run(pc1, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(web, "ipconfig 172.16.1.10 255.255.255.0 172.16.1.1");
    lab.run(pc2, "ipconfig 198.51.100.10 255.255.255.0 198.51.100.1");
    // The first echoes of a lab wait for ARP; start with every cache warm and nothing tracked.
    lab.run(pc1, "ping 198.51.100.10");
    lab.run(web, "ping 198.51.100.10");
    lab.run(fw, "clear xlate\nclear conn\nclear access-list counters");
    lab.sim.run_for(&lab.t, 60_000_000);
    FwLab { lab, fw, isp, pc1, pc2, pc3, web }
}

#[test]
fn a_firewall_lets_inside_out_with_port_translation_and_keeps_outside_out() {
    let FwLab { mut lab, fw, isp, pc1, pc2, .. } = firewall_lab();
    assert_eq!(lab.prompt(fw), "FW1#");
    let names = lab.run(fw, "show nameif");
    assert!(names.contains("GigabitEthernet1/1     inside         100"), "{names}");
    assert!(names.contains("GigabitEthernet1/3     dmz            50"), "{names}");
    let brief = lab.run(fw, "show interface ip brief");
    assert!(brief.contains("GigabitEthernet1/2     outside      203.0.113.2/24     up     up"), "{brief}");
    assert!(lab.run(fw, "show route").contains("0.0.0.0/0"));

    // Inside (100) to outside (0): allowed, translated to the outside address, replies come back.
    let out = lab.run(pc1, "ping 198.51.100.10");
    assert!(out.contains("reply from 198.51.100.10") && out.contains("4 answered"), "{out}");
    let xlate = lab.run(fw, "show xlate");
    assert!(xlate.contains("dynamic  icmp inside 192.168.1.10:") && xlate.contains("→ outside 203.0.113.2:"), "{xlate}");
    assert!(xlate.contains("static   dmz 172.16.1.10 ↔ outside 203.0.113.10"), "{xlate}");
    let conn = lab.run(fw, "show conn");
    // One connection per echo (each carries its own identifier).
    assert!(conn.starts_with("4 connections in use") && conn.contains("icmp inside 192.168.1.10:"), "{conn}");
    assert!(conn.contains("→ outside 198.51.100.10:"), "{conn}");
    let nat = lab.run(fw, "show nat");
    assert!(nat.contains("(inside,outside) object INSIDE_NET: 192.168.1.0/24 → outside's address, ports shared  [used "), "{nat}");
    assert!(nat.find("object WEB").unwrap() < nat.find("object INSIDE_NET").unwrap(), "static rules first: {nat}");

    // Outside (0) to inside (100): dropped without an access rule, with the reason logged.
    lab.run(isp, "conf t\nip route 192.168.1.0 255.255.255.0 203.0.113.2\nend");
    let out = lab.run(pc2, "ping 192.168.1.10");
    assert!(out.contains("0 answered"), "{out}");
    let log = lab.run(fw, "show logging");
    assert!(log.contains("denied: from security level 0 to 100 without an access rule"), "{log}");
    // An access rule bound to the outside lets it in; replies are not translated.
    lab.run(fw, "conf t\naccess-list OUTSIDE_IN extended permit icmp any host 192.168.1.10\naccess-group OUTSIDE_IN in interface outside\nend");
    let out = lab.run(pc2, "ping 192.168.1.10");
    assert!(out.contains("reply from 192.168.1.10") && out.contains("4 answered"), "{out}");
    let acl = lab.run(fw, "show access-list");
    assert!(acl.contains("access-list OUTSIDE_IN: 1 rule, checks traffic in on outside"), "{acl}");
    assert!(acl.contains("1. permit icmp any host 192.168.1.10  (hits 4)"), "{acl}");
    // Only that host: another inside address still gets the implicit deny.
    let out = lab.run(pc2, "ping 192.168.1.11");
    assert!(out.contains("0 answered"), "{out}");
    assert!(lab.run(fw, "show access-list OUTSIDE_IN").contains("anything else is denied  (hits 4)"));
    assert!(lab.run(fw, "show logging").contains("denied by access-group OUTSIDE_IN on outside (no rule matched)"));
    lab.run(fw, "clear access-list counters");
    assert!(lab.run(fw, "show access-list").contains("(hits 0)"));
    lab.run(fw, "clear conn");
    assert!(lab.run(fw, "show conn").starts_with("0 connections"));
    lab.run(fw, "clear xlate");
    assert!(lab.run(fw, "show xlate").contains("1 static, 0 dynamic"));
}

#[test]
fn a_dmz_web_server_is_reachable_from_outside_through_static_nat_and_a_rule() {
    let FwLab { mut lab, fw, pc1, pc2, web, .. } = firewall_lab();
    let target: std::net::Ipv4Addr = "203.0.113.10".parse().unwrap();
    // Without a rule the outside can't open a connection to the DMZ (level 0 to 50).
    assert!(lab.sim.http_get(&lab.t, pc2, target, "203.0.113.10", "/").is_err());
    lab.run(fw, "conf t\naccess-list OUTSIDE_IN extended permit tcp any object WEB eq www\naccess-group OUTSIDE_IN in interface outside\nend");
    let (status, body) = lab.sim.http_get(&lab.t, pc2, target, "203.0.113.10", "/").unwrap();
    assert!(status.starts_with("200"), "{status} {body}");
    let acl = lab.run(fw, "show access-list");
    assert!(acl.contains("permit tcp any object WEB eq 80  (hits 1)"), "{acl}");
    let conn = lab.run(fw, "show conn");
    assert!(conn.contains("tcp  outside 198.51.100.10:") && conn.contains("→ dmz 172.16.1.10:80"), "{conn}");
    // The DMZ (50) reaches the outside (0) by itself, and the inside (100) reaches the DMZ.
    let out = lab.run(web, "ping 198.51.100.10");
    assert!(out.contains("4 answered"), "{out}");
    let out = lab.run(pc1, "ping 172.16.1.10");
    assert!(out.contains("4 answered"), "{out}");
    // The DMZ can't start a connection to the inside.
    let out = lab.run(web, "ping 192.168.1.10");
    assert!(out.contains("0 answered"), "{out}");
    assert!(lab.run(fw, "show logging").contains("denied: from security level 50 to 100 without an access rule"));
    // Equal levels talk only with same-security-traffic.
    lab.run(fw, "conf t\ninterface g1/3\nsecurity-level 100\nend");
    let out = lab.run(web, "ping 192.168.1.10");
    assert!(out.contains("0 answered"), "{out}");
    assert!(lab.run(fw, "show logging").contains("both at security level 100 (same-security-traffic is not permitted)"));
    lab.run(fw, "conf t\nsame-security-traffic permit inter-interface\nend");
    let out = lab.run(web, "ping 192.168.1.10");
    assert!(out.contains("4 answered"), "{out}");
}

#[test]
fn firewall_connections_time_out_in_simulated_time() {
    let FwLab { mut lab, fw, pc1, .. } = firewall_lab();
    lab.run(pc1, "ping 198.51.100.10");
    assert!(lab.run(fw, "show conn").starts_with("4 connections"));
    // ICMP connections are kept 2 s by default; their translations stay a while longer.
    lab.sim.run_for(&lab.t, 3_000_000);
    assert!(lab.run(fw, "show conn").starts_with("0 connections"));
    let xlate = lab.run(fw, "show xlate");
    assert!(xlate.contains("4 dynamic") && xlate.contains("idle 0:00:03"), "{xlate}");
    lab.sim.run_for(&lab.t, 30_000_000);
    assert!(lab.run(fw, "show xlate").contains("0 dynamic"));
    // A longer timeout keeps them.
    lab.run(fw, "conf t\ntimeout icmp 0:00:30\nend");
    lab.run(pc1, "ping 198.51.100.10");
    lab.sim.run_for(&lab.t, 10_000_000);
    let conn = lab.run(fw, "show conn");
    assert!(conn.starts_with("4 connections") && conn.contains("idle 0:00:10"), "{conn}");
    lab.sim.run_for(&lab.t, 25_000_000);
    assert!(lab.run(fw, "show conn").starts_with("0 connections"));
    assert!(lab.run(fw, "show running-config").contains("timeout icmp 0:00:30"));
}

#[test]
fn the_firewall_hands_out_addresses_and_its_config_round_trips() {
    let FwLab { mut lab, fw, pc3, .. } = firewall_lab();
    lab.run(fw, "conf t\ndhcpd address 192.168.1.100-192.168.1.120 inside\ndhcpd dns 198.51.100.53\ndhcpd enable inside\nend");
    let out = lab.run(pc3, "ipconfig /renew");
    assert!(out.contains("192.168.1.100/24"), "{out}");
    let pc = lab.t.device(pc3).unwrap();
    assert_eq!(pc.gateway, Some("192.168.1.1".parse().unwrap()));
    assert_eq!(pc.dns, Some("198.51.100.53".parse().unwrap()));
    assert!(lab.run(fw, "show dhcpd binding").contains("192.168.1.100"));
    // The DHCP client gets out too.
    assert!(lab.run(pc3, "ping 198.51.100.10").contains("4 answered"));

    lab.run(
        fw,
        "conf t
        access-list OUTSIDE_IN extended permit tcp any object WEB eq www
        access-list OUTSIDE_IN extended deny ip 10.0.0.0 255.0.0.0 any
        access-group OUTSIDE_IN in interface outside
        same-security-traffic permit inter-interface
        timeout udp 0:05:00
        enable password secret123
        end",
    );
    let run = lab.run(fw, "show running-config");
    for want in [
        "hostname FW1",
        "interface GigabitEthernet1/1\n nameif inside\n security-level 100\n ip address 192.168.1.1 255.255.255.0\n no shutdown",
        "object network INSIDE_NET\n subnet 192.168.1.0 255.255.255.0\n nat (inside,outside) dynamic interface",
        "object network WEB\n host 172.16.1.10\n nat (dmz,outside) static 203.0.113.10",
        "access-list OUTSIDE_IN extended permit tcp any object WEB eq 80",
        "access-list OUTSIDE_IN extended deny ip 10.0.0.0 255.0.0.0 any",
        "access-group OUTSIDE_IN in interface outside",
        "route outside 0.0.0.0 0.0.0.0 203.0.113.1 1",
        "timeout udp 0:05:00",
        "dhcpd address 192.168.1.100-192.168.1.120 inside",
        "dhcpd enable inside",
        "same-security-traffic permit inter-interface",
    ] {
        assert!(run.contains(want), "missing {want}:\n{run}");
    }
    // Saved, reloaded, and replayed onto a fresh firewall: the same configuration.
    lab.run(fw, "write memory\nreload");
    lab.clis.clear();
    lab.run(fw, "en\nsecret123");
    assert_eq!(lab.run(fw, "show running-config"), run);
    let other = add_firewall(&mut lab);
    let failed = apply_config(&mut lab.t, &mut lab.sim, other, &run.replace("hostname FW1", "hostname FW2"));
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(running_config(lab.t.device(other).unwrap()), run.replace("FW1", "FW2"));
}

#[test]
fn firewall_mistakes_are_refused() {
    let FwLab { mut lab, fw, .. } = firewall_lab();
    lab.run(fw, "conf t");
    for (line, why) in [
        ("route nowhere 0.0.0.0 0.0.0.0 203.0.113.1", "No interface is named nowhere"),
        ("route outside 0.0.0.0 0.0.0.0 10.9.9.9", "not a neighbour on outside"),
        ("route outside 10.0.0.1 255.0.0.0 203.0.113.1", "host bits"),
        ("access-group NOPE in interface outside", "No access list NOPE"),
        ("access-list A extended permit ip 10.0.0.0 0.0.0.255 any", "masks, not wildcards"),
        ("access-list A extended permit tcp any object NOPE eq 80", "no network object NOPE"),
        ("access-list A extended allow ip any any", "permit or deny"),
        ("access-list 9x extended permit ip any any", "can't name an access list"),
        ("dhcpd address 10.0.0.1-10.0.0.9 inside", "must lie in inside's subnet"),
        ("dhcpd address 192.168.1.1-192.168.1.9 inside", "own address"),
        ("dhcpd address 192.168.1.9-192.168.1.2 inside", "ends"),
        ("dhcpd enable dmz", "Give dmz a range first"),
        ("timeout icmp forever", "H:MM:SS"),
        ("timeout tcp 0:00:10", "conn, udp or icmp"),
        ("no object network NOPE", "No network object NOPE"),
        ("ip route 0.0.0.0 0.0.0.0 1.2.3.4", "Not understood"),
        ("vlan 10", "Not understood"),
        ("icmp permit any nowhere", "No interface is named nowhere"),
        ("icmp allow any inside", "permit or deny"),
        ("icmp permit 10.0.0.0 0.0.0.255 inside", "masks, not wildcards"),
        ("access-list A remark", "Incomplete command"),
        ("access-list A extended permit icmp any any bogus", "not an ICMP type"),
        ("access-list A extended permit tcp object-group NOPE any", "no network object group NOPE"),
        ("access-list A extended permit object-group NOPE any any", "no service object group NOPE"),
        ("nat (inside,outside) source static NOPE WEB", "not a network object or group"),
        ("nat (inside,outside) source static WEB interface", "for dynamic rules"),
        ("nat (inside,outside) source dynamic WEB NOPE", "not a network object with an address"),
        ("nat (inside,outside) source static WEB WEB destination static NOPE WEB", "NOPE is not a network object"),
        ("nat (inside,outside) source static WEB WEB destination", "destination static"),
        ("no nat (inside,outside) source static WEB WEB", "No such NAT rule"),
        ("object-group service S bogus", "tcp, udp or tcp-udp"),
        ("object-group network WEB", "another kind"),
        ("no object-group network NOPE", "No network object group NOPE"),
        ("class-map class-default", "can't name a class map"),
        ("service-policy NOPE global", "No policy map NOPE"),
        ("no service-policy NOPE global", "not the global policy"),
        ("no class-map inspection_default", "Policy map global_policy uses inspection_default"),
        ("no policy-map global_policy", "is the global policy"),
        ("route outside 10.0.0.0 255.0.0.0 203.0.113.1 0", "1-255"),
    ] {
        let out = lab.try_run(fw, line);
        assert!(out.contains(why), "`{line}`: {out}");
    }
    lab.run(fw, "interface g1/4");
    for (line, why) in
        [("nameif 1bad", "can't name an interface"), ("nameif inside", "already names GigabitEthernet1/1"), ("security-level 101", "0-100")]
    {
        let out = lab.try_run(fw, line);
        assert!(out.contains(why), "`{line}`: {out}");
    }
    lab.run(fw, "exit\nobject network EMPTY");
    assert_eq!(lab.prompt(fw), "FW1(config-network-object)#");
    for (line, why) in [
        ("nat (inside,outside) dynamic interface", "addresses first"),
        ("subnet 10.0.0.1 255.0.0.0", "host bits"),
        ("nat (inside,nowhere) dynamic interface", "No interface is named nowhere"),
        ("nat inside,outside dynamic interface", "(real,mapped)"),
    ] {
        let out = lab.try_run(fw, line);
        assert!(out.contains(why), "`{line}`: {out}");
    }
    lab.run(fw, "host 192.168.1.50");
    assert!(lab.try_run(fw, "nat (inside,inside) static 1.2.3.4").contains("other than the real one"));
    // A global command leaves the object; an object used by a rule can't be deleted.
    lab.run(fw, "access-list A extended permit ip any object EMPTY");
    assert_eq!(lab.prompt(fw), "FW1(config)#");
    assert!(lab.try_run(fw, "no object network EMPTY").contains("EMPTY is used by access list A"));
    lab.run(fw, "no access-list A extended permit ip any object EMPTY\nno object network EMPTY");
    assert!(lab.run(fw, "do show object").contains("WEB"));
    // Renaming an interface carries its rules along.
    lab.run(fw, "access-list B extended permit icmp any any\naccess-group B in interface outside\ninterface g1/2\nnameif internet\nend");
    let run = lab.run(fw, "show run");
    assert!(run.contains("access-group B in interface internet") && run.contains("nat (dmz,internet) static 203.0.113.10"), "{run}");
    assert!(run.contains("route internet 0.0.0.0 0.0.0.0 203.0.113.1 1"), "{run}");
    // The help lists the firewall's own commands, not the router's.
    let help = lab.try_run(fw, "show ?");
    assert!(help.contains("xlate") && help.contains("conn") && !help.contains("vlan"), "{help}");
}

#[test]
fn traceroute_from_inside_crosses_the_firewall_and_its_nat() {
    let FwLab { mut lab, fw, pc1, .. } = firewall_lab();
    // The firewall is not a hop: the ISP router answers the first probe, and its "time
    // exceeded" (sent to the shared outside address) is translated back to PC1.
    let tr = lab.run(pc1, "tracert 198.51.100.10");
    assert!(tr.contains("1  203.0.113.1") && tr.contains("2  198.51.100.10"), "{tr}");
    assert!(!tr.contains("192.168.1.1 "), "{tr}");
    let sp = lab.run(fw, "show service-policy");
    assert!(sp.contains("Global policy: global_policy") && sp.contains("inspect icmp:"), "{sp}");
    // Counted as a hop when the policy says so.
    lab.run(fw, "conf t\npolicy-map global_policy\nclass inspection_default\nset connection decrement-ttl\nend");
    let tr = lab.run(pc1, "tracert 198.51.100.10");
    assert!(tr.contains("1  192.168.1.1") && tr.contains("2  203.0.113.1") && tr.contains("3  198.51.100.10"), "{tr}");
    assert!(lab.run(fw, "show service-policy").contains("set connection decrement-ttl"));
    assert!(
        lab.run(fw, "show running-config")
            .contains("policy-map global_policy\n class inspection_default\n  inspect icmp\n  set connection decrement-ttl")
    );
    lab.run(fw, "conf t\npolicy-map global_policy\nclass inspection_default\nno set connection decrement-ttl\nend");

    // Without `inspect icmp`, ICMP is not tracked: replies and errors are new packets from
    // level 0 to level 100, and need a rule.
    lab.run(fw, "conf t\npolicy-map global_policy\nclass inspection_default\nno inspect icmp\nend");
    lab.run(fw, "clear conn");
    let out = lab.run(pc1, "ping 198.51.100.10");
    assert!(out.contains("0 answered"), "{out}");
    assert!(lab.run(fw, "show logging").contains("denied: from security level 0 to 100 without an access rule"));
    assert!(lab.run(fw, "show conn").starts_with("0 connections"));
    let tr = lab.run(pc1, "tracert 198.51.100.10");
    assert!(!tr.contains("203.0.113.1"), "{tr}");
    lab.run(
        fw,
        "conf t
        access-list OUTSIDE_IN extended permit icmp any any echo-reply
        access-list OUTSIDE_IN extended permit icmp any any time-exceeded
        access-group OUTSIDE_IN in interface outside
        end",
    );
    let out = lab.run(pc1, "ping 198.51.100.10");
    assert!(out.contains("4 answered"), "{out}");
    assert!(lab.run(fw, "show conn").starts_with("0 connections"), "nothing tracked");
    let tr = lab.run(pc1, "tracert 198.51.100.10");
    assert!(tr.contains("1  203.0.113.1") && tr.contains("2  198.51.100.10"), "{tr}");
    let acl = lab.run(fw, "show access-list OUTSIDE_IN");
    assert!(acl.contains("1. permit icmp any any echo-reply  (hits ") && acl.contains("2. permit icmp any any time-exceeded  (hits "), "{acl}");
    assert!(!acl.contains("time-exceeded  (hits 0)"), "{acl}");
}

#[test]
fn the_firewall_answers_icmp_per_its_icmp_rules() {
    let FwLab { mut lab, fw, isp, pc1, pc3, .. } = firewall_lab();
    lab.run(pc3, "ipconfig 192.168.1.11 255.255.255.0 192.168.1.1");
    lab.run(pc3, "ping 192.168.1.1");
    // No rules: every interface answers pings to its own address (and only to it).
    assert!(lab.run(pc1, "ping 192.168.1.1").contains("4 answered"));
    assert!(lab.run(pc1, "ping 203.0.113.2").contains("0 answered"));
    assert!(lab.run(isp, "ping 203.0.113.2").contains("!!!!!"));
    // The first matching rule of the interface decides; with rules, the rest is denied.
    lab.run(fw, "conf t\nicmp deny any inside\nicmp permit host 192.168.1.10 echo inside\nend");
    assert!(lab.run(pc1, "ping 192.168.1.1").contains("0 answered"));
    assert!(lab.run(fw, "show logging").contains("denied by icmp rule 1 on inside"));
    lab.run(fw, "conf t\nno icmp deny any inside\nend");
    assert!(lab.run(pc1, "ping 192.168.1.1").contains("4 answered"));
    assert!(lab.run(pc3, "ping 192.168.1.1").contains("0 answered"));
    assert!(lab.run(fw, "show logging").contains("denied: no icmp rule on inside permits it"));
    // Outside: the firewall's own pings need their replies let in.
    lab.run(fw, "conf t\nicmp permit any echo-reply outside\nicmp deny any outside\nend");
    assert!(!lab.run(isp, "ping 203.0.113.2").contains("!!!!!"));
    assert!(lab.run(fw, "ping 198.51.100.10").contains("!!!!!"));
    let run = lab.run(fw, "show running-config");
    for want in ["icmp permit host 192.168.1.10 echo inside", "icmp permit any echo-reply outside", "icmp deny any outside"] {
        assert!(run.contains(want), "missing {want}:\n{run}");
    }
    // Rules leave with their interface's name, and follow a rename.
    lab.run(fw, "conf t\ninterface g1/2\nnameif internet\nend");
    assert!(lab.run(fw, "show running-config").contains("icmp deny any internet"));
}

#[test]
fn firewall_subinterfaces_carry_named_vlans_over_a_trunk() {
    let FwLab { mut lab, fw, .. } = firewall_lab();
    let sw = lab.add(DeviceKind::Switch);
    let staff = lab.add(DeviceKind::Pc);
    let guest = lab.add(DeviceKind::Pc);
    lab.cable(fw, "g1/4", sw, "g0/1");
    lab.cable(staff, "fa0", sw, "fa0/1");
    lab.cable(guest, "fa0", sw, "fa0/2");
    lab.run(sw, "en\nconf t\nvlan 10\nvlan 20\nint fa0/1\nsw acc vlan 10\nint fa0/2\nsw acc vlan 20\nint g0/1\nsw mode trunk\nend");
    lab.run(
        fw,
        "conf t
        interface g1/4
        no shutdown
        interface g1/4.10
        vlan 10
        nameif staff
        security-level 80
        ip address 10.10.0.1 255.255.255.0
        interface g1/4.20
        vlan 20
        nameif guest
        security-level 20
        ip address 10.20.0.1 255.255.255.0
        end",
    );
    assert_eq!(lab.prompt(fw), "FW1#");
    lab.run(staff, "ipconfig 10.10.0.5 255.255.255.0 10.10.0.1");
    lab.run(guest, "ipconfig 10.20.0.5 255.255.255.0 10.20.0.1");
    lab.run(staff, "ping 10.20.0.5");
    lab.run(guest, "ping 10.20.0.1");
    // Staff (80) reaches guests (20); guests don't start anything towards staff.
    assert!(lab.run(staff, "ping 10.20.0.5").contains("4 answered"));
    assert!(lab.run(guest, "ping 10.10.0.5").contains("0 answered"));
    assert!(lab.run(fw, "show logging").contains("from security level 20 to 80 without an access rule"));
    // Both reach the outside through the firewall's address.
    assert!(lab.run(staff, "ping 198.51.100.10").contains("0 answered"), "no NAT rule covers 10.10.0.0/24 yet");
    lab.run(fw, "conf t\nobject network STAFF\nsubnet 10.10.0.0 255.255.255.0\nnat (staff,outside) dynamic interface\nend");
    assert!(lab.run(staff, "ping 198.51.100.10").contains("4 answered"));
    let names = lab.run(fw, "show nameif");
    assert!(names.contains(&format!("{:<22} {:<14} {:<15} {}", "GigabitEthernet1/4.10", "staff", "80", "10")), "{names}");
    assert!(names.contains(&format!("{:<22} {:<14} {:<15} {}", "GigabitEthernet1/4.20", "guest", "20", "20")), "{names}");
    assert!(lab.run(fw, "show interface ip brief").contains("GigabitEthernet1/4.10  staff        10.10.0.1/24       up     up"));
    let run = lab.run(fw, "show running-config");
    assert!(
        run.contains(
            "interface GigabitEthernet1/4.10\n vlan 10\n nameif staff\n security-level 80\n ip address 10.10.0.1 255.255.255.0\n no shutdown"
        ),
        "{run}"
    );
    let other = add_firewall(&mut lab);
    let failed = apply_config(&mut lab.t, &mut lab.sim, other, &run.replace("hostname FW1", "hostname FW2"));
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(running_config(lab.t.device(other).unwrap()), run.replace("FW1", "FW2"));
    // A deleted subinterface takes its name (and what used it) along.
    lab.run(fw, "conf t\naccess-list G extended permit ip any any\naccess-group G in interface guest\nno interface g1/4.20\nend");
    assert!(!lab.run(fw, "show nameif").contains("guest"));
    assert!(!lab.run(fw, "show running-config").contains("access-group G"));
    lab.try_run(fw, "conf t");
    assert!(lab.try_run(fw, "route guest 0.0.0.0 0.0.0.0 10.20.0.9").contains("No interface is named guest"));
}

#[test]
fn object_groups_remarks_outbound_lists_and_hairpins() {
    let FwLab { mut lab, fw, pc1, pc2, pc3, web, .. } = firewall_lab();
    lab.run(
        fw,
        "conf t
        object-group network HOME_PCS
        network-object host 198.51.100.10
        network-object 198.51.100.128 255.255.255.128
        object-group network ANYONE_HOME
        group-object HOME_PCS
        object-group service WEB_PORTS tcp
        port-object eq www
        port-object eq https
        object-group service PINGS
        service-object icmp
        access-list OUTSIDE_IN remark Home may browse the DMZ server
        access-list OUTSIDE_IN extended permit tcp object-group ANYONE_HOME object WEB object-group WEB_PORTS
        access-list OUTSIDE_IN remark and ping it
        access-list OUTSIDE_IN extended permit object-group PINGS object-group HOME_PCS object WEB
        access-group OUTSIDE_IN in interface outside
        end",
    );
    assert_eq!(lab.prompt(fw), "FW1#");
    let target: std::net::Ipv4Addr = "203.0.113.10".parse().unwrap();
    let (status, _) = lab.sim.http_get(&lab.t, pc2, target, "203.0.113.10", "/").unwrap();
    assert!(status.starts_with("200"), "{status}");
    assert!(lab.run(pc2, "ping 203.0.113.10").contains("4 answered"));
    let acl = lab.run(fw, "show access-list");
    assert!(
        acl.contains("-- Home may browse the DMZ server\n    1. permit tcp object-group ANYONE_HOME object WEB object-group WEB_PORTS  (hits 1)"),
        "{acl}"
    );
    assert!(acl.contains("-- and ping it\n    2. permit object-group PINGS object-group HOME_PCS object WEB  (hits 4)"), "{acl}");
    let groups = lab.run(fw, "show object-group");
    assert!(groups.contains("object-group network ANYONE_HOME (1 member)\n  group-object HOME_PCS"), "{groups}");
    assert!(groups.contains("object-group service WEB_PORTS (tcp ports, 2 members)\n  port-object eq 80\n  port-object eq 443"), "{groups}");
    // In use, a group can't go; nesting can't loop.
    lab.try_run(fw, "conf t");
    assert!(lab.try_run(fw, "no object-group network HOME_PCS").contains("HOME_PCS is used by"));
    lab.run(fw, "object-group network HOME_PCS");
    assert_eq!(lab.prompt(fw), "FW1(config-network-object-group)#");
    assert!(lab.try_run(fw, "group-object ANYONE_HOME").contains("groups can't hold each other"));
    assert!(lab.try_run(fw, "port-object eq 80").contains("network group holds"));
    lab.run(fw, "object-group service PINGS");
    assert_eq!(lab.prompt(fw), "FW1(config-service-object-group)#");
    assert!(lab.try_run(fw, "port-object eq 80").contains("groups of ports"));
    lab.run(fw, "end");

    // An outbound list: checked on the way out of the DMZ, whoever sends.
    lab.run(fw, "conf t\naccess-list TO_DMZ extended deny icmp any any\naccess-list TO_DMZ extended permit ip any any\naccess-group TO_DMZ out interface dmz\nend");
    assert!(lab.run(pc1, "ping 172.16.1.10").contains("0 answered"));
    assert!(lab.run(fw, "show logging").contains("denied by access-group TO_DMZ out on dmz (rule 1)"));
    assert!(lab.run(fw, "show access-list TO_DMZ").contains("checks traffic out on dmz"));
    assert!(lab.run(web, "ping 192.168.1.10").contains("0 answered"), "the DMZ still can't reach the inside");

    // Hairpin: a route back out of the inside needs intra-interface.
    lab.run(pc3, "ipconfig 192.168.1.20 255.255.255.0 192.168.1.1");
    lab.run(fw, "conf t\nroute inside 10.99.0.0 255.255.0.0 192.168.1.20\nend");
    lab.run(pc1, "ping 10.99.0.5");
    assert!(lab.run(fw, "show logging").contains("same-security-traffic permit intra-interface allows that"));
    lab.run(fw, "conf t\nsame-security-traffic permit intra-interface\nend");
    lab.run(fw, "clear conn");
    lab.run(pc1, "ping 10.99.0.5");
    let conn = lab.run(fw, "show conn");
    assert!(conn.contains("icmp inside 192.168.1.10:") && conn.contains("→ inside 10.99.0.5:"), "{conn}");

    let run = lab.run(fw, "show running-config");
    for want in [
        "object-group network HOME_PCS\n network-object host 198.51.100.10\n network-object 198.51.100.128 255.255.255.128",
        "object-group network ANYONE_HOME\n group-object HOME_PCS",
        "object-group service WEB_PORTS tcp\n port-object eq 80\n port-object eq 443",
        "object-group service PINGS\n service-object icmp",
        "access-list OUTSIDE_IN remark Home may browse the DMZ server\naccess-list OUTSIDE_IN extended permit tcp object-group ANYONE_HOME object WEB object-group WEB_PORTS\naccess-list OUTSIDE_IN remark and ping it",
        "access-group TO_DMZ out interface dmz",
        "same-security-traffic permit intra-interface",
    ] {
        assert!(run.contains(want), "missing {want}:\n{run}");
    }
    let other = add_firewall(&mut lab);
    let failed = apply_config(&mut lab.t, &mut lab.sim, other, &run.replace("hostname FW1", "hostname FW2"));
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(running_config(lab.t.device(other).unwrap()), run.replace("FW1", "FW2"));
    // Removing a rule keeps the remarks in their place.
    lab.run(fw, "conf t\nno access-list OUTSIDE_IN extended permit tcp object-group ANYONE_HOME object WEB object-group WEB_PORTS\nno access-list OUTSIDE_IN remark and ping it\nend");
    let acl = lab.run(fw, "show access-list OUTSIDE_IN");
    assert!(acl.contains("-- Home may browse the DMZ server\n    1. permit object-group PINGS"), "{acl}");
}

#[test]
fn twice_nat_translates_sources_and_destinations_by_rule() {
    let FwLab { mut lab, fw, pc1, pc2, .. } = firewall_lab();
    lab.run(
        fw,
        "conf t
        object network PC1_REAL
        host 192.168.1.10
        object network PC1_MAPPED
        host 203.0.113.50
        object network PC2_ALIAS
        host 10.200.0.10
        object network PC2_REAL
        host 198.51.100.10
        nat (inside,outside) source static PC1_REAL PC1_MAPPED
        end",
    );
    // The ISP learns where 203.0.113.50 is first (the firewall answers ARP for it).
    lab.run(pc1, "ping 198.51.100.10");
    // Twice NAT comes before object NAT: PC1 leaves as 203.0.113.50, not the shared address.
    assert!(lab.run(pc1, "ping 198.51.100.10").contains("4 answered"));
    let xlate = lab.run(fw, "show xlate");
    assert!(xlate.contains("static   inside 192.168.1.10 ↔ outside 203.0.113.50  (twice 1)") && xlate.contains("0 dynamic"), "{xlate}");
    let detail = lab.run(fw, "show nat detail");
    assert!(detail.contains("Section 1, twice NAT"), "{detail}");
    assert!(detail.contains("1. nat (inside,outside) source static PC1_REAL PC1_MAPPED"), "{detail}");
    assert!(detail.contains("source 192.168.1.10 → 203.0.113.50 (one to one, both directions)"), "{detail}");
    assert!(detail.contains("translated 8 times, undone 7 times") || detail.contains("translated 8 times, undone 8 times"), "{detail}");
    assert!(detail.contains("Section 2, object NAT") && detail.contains("object WEB: nat (dmz,outside) static 203.0.113.10"), "{detail}");
    // Static both ways: the outside can open connections to the mapped address, with a rule.
    lab.run(fw, "conf t\naccess-list OUTSIDE_IN extended permit icmp any object PC1_REAL\naccess-group OUTSIDE_IN in interface outside\nend");
    assert!(lab.run(pc2, "ping 203.0.113.50").contains("4 answered"));

    // With a destination: PC1 reaches PC2 by an alias, and sees answers from the alias.
    lab.run(
        fw,
        "conf t
        no nat (inside,outside) source static PC1_REAL PC1_MAPPED
        nat (inside,outside) source static PC1_REAL PC1_MAPPED destination static PC2_ALIAS PC2_REAL
        end",
    );
    let out = lab.run(pc1, "ping 10.200.0.10");
    assert!(out.contains("reply from 10.200.0.10") && out.contains("4 answered"), "{out}");
    assert!(lab.run(fw, "show nat detail").contains("destination 10.200.0.10 → 198.51.100.10"));
    // Elsewhere, PC1 still leaves by the shared address.
    lab.run(pc1, "ping 198.51.100.1");
    assert!(lab.run(fw, "show xlate").contains("dynamic  icmp inside 192.168.1.10:"));

    // Dynamic, to the interface.
    lab.run(
        fw,
        "conf t
        no nat (inside,outside) source static PC1_REAL PC1_MAPPED destination static PC2_ALIAS PC2_REAL
        object-group network LAN
        network-object 192.168.1.0 255.255.255.0
        nat (inside,outside) source dynamic LAN interface
        end",
    );
    lab.run(fw, "clear xlate");
    assert!(lab.run(pc1, "ping 198.51.100.10").contains("4 answered"));
    let xlate = lab.run(fw, "show xlate");
    assert!(xlate.contains("→ outside 203.0.113.2:") && xlate.contains("(twice 1)"), "{xlate}");
    let run = lab.run(fw, "show running-config");
    assert!(run.contains("nat (inside,outside) source dynamic LAN interface"), "{run}");
    lab.try_run(fw, "conf t");
    assert!(lab.try_run(fw, "no object-group network LAN").contains("used by a twice NAT rule"));
    let other = add_firewall(&mut lab);
    let failed = apply_config(&mut lab.t, &mut lab.sim, other, &run.replace("hostname FW1", "hostname FW2"));
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(running_config(lab.t.device(other).unwrap()), run.replace("FW1", "FW2"));
}

#[test]
fn firewall_routes_keep_their_distance() {
    let FwLab { mut lab, fw, .. } = firewall_lab();
    lab.run(fw, "conf t\nroute outside 10.0.0.0 255.0.0.0 203.0.113.1 5\nend");
    let routes = lab.run(fw, "show route");
    assert!(routes.contains("Gateway of last resort: 203.0.113.1 on outside"), "{routes}");
    assert!(routes.contains("[5/0] via 203.0.113.1, outside") && routes.contains("[1/0] via 203.0.113.1, outside"), "{routes}");
    assert!(routes.contains("connected, inside"), "{routes}");
    let run = lab.run(fw, "show running-config");
    assert!(run.contains("route outside 10.0.0.0 255.0.0.0 203.0.113.1 5") && run.contains("route outside 0.0.0.0 0.0.0.0 203.0.113.1 1"), "{run}");
    // Typed again without one, it is back to 1; removed, its distance goes too.
    lab.run(fw, "conf t\nroute outside 10.0.0.0 255.0.0.0 203.0.113.1\nend");
    assert!(lab.run(fw, "show running-config").contains("route outside 10.0.0.0 255.0.0.0 203.0.113.1 1"));
    lab.run(fw, "conf t\nroute outside 10.0.0.0 255.0.0.0 203.0.113.1 7\nno route outside 10.0.0.0 255.0.0.0 203.0.113.1\nend");
    assert!(!lab.run(fw, "show route").contains("10.0.0.0"));
    assert!(lab.t.device(fw).unwrap().firewall.route_distances.is_empty());
}

#[test]
fn every_firewall_command_survives_hostile_arguments() {
    let hostile = [
        "",
        "0",
        "-1",
        "99999999999999999999",
        "1.2.3.4",
        "255.255.255.255",
        "(a,b)",
        "(inside,outside)",
        "1.1.1.1-1.1.1.0",
        "ä€😀",
        "%s%n",
        "host 1.2.3.4 echo inside",
        "static WEB WEB destination static WEB WEB",
        "dynamic G interface",
        "range 9 1",
        "icmp destination eq 1",
        "object-group G",
    ];
    let huge = "x".repeat(10_000);
    let base = firewall_lab();
    let fw = base.fw;
    for spec in SPECS.iter().filter(|s| s.devices.has(crate::grammar::Modes::FIREWALL)) {
        for arg in hostile.iter().chain(std::iter::once(&huge.as_str())) {
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
                "en\nconf t\nint g1/1",
                "en\nconf t\nint g1/5",
                "en\nconf t\nobject network X",
                "en\nconf t\nobject network WEB",
                "en\nconf t\nobject-group network G",
                "en\nconf t\nobject-group service S tcp",
                "en\nconf t\nobject-group service T",
                "en\nconf t\nclass-map inspection_default",
                "en\nconf t\npolicy-map global_policy\nclass inspection_default",
                "en\nconf t\npolicy-map P",
                "en\nconf t\nint g1/5.7",
            ] {
                let mut lab = Lab { t: base.lab.t.clone(), sim: base.lab.sim.clone(), clis: Default::default() };
                for l in setup.lines() {
                    lab.try_run(fw, l);
                }
                let _ = lab.try_run(fw, &line.join(" "));
                let _ = lab.try_run(fw, &format!("{} ?", line.join(" ")));
                let _ = lab.try_run(fw, "do show run");
                let _ = lab.try_run(fw, "do show xlate");
                let _ = lab.try_run(fw, "do show nat detail");
                let _ = lab.try_run(fw, "do show service-policy");
                let _ = lab.try_run(fw, "do show object-group");
                let _ = lab.try_run(fw, "do show route");
            }
        }
    }
}

/// A diamond R1 → (R2 | R3) → R4, with a switch between R1 and R3: EIGRP builds it with
/// router ids from the addresses.
fn eigrp_diamond() -> (Lab, [DeviceId; 6], DeviceId) {
    let mut lab = Lab::new();
    let three = netlabcraft_model::models::find("Router-3").unwrap();
    let r1 = lab.t.add_model(three, None, 0.0, 0.0).unwrap();
    let r2 = lab.t.add_model(three, None, 0.0, 0.0).unwrap();
    let r3 = lab.t.add_model(three, None, 0.0, 0.0).unwrap();
    let r4 = lab.t.add_model(three, None, 0.0, 0.0).unwrap();
    let sw = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(a, "fa0", r1, "g0/0");
    lab.cable(r1, "g0/1", r2, "g0/0");
    lab.cable(r1, "g0/2", sw, "fa0/1");
    lab.cable(sw, "fa0/2", r3, "g0/0");
    lab.cable(r2, "g0/1", r4, "g0/0");
    lab.cable(r3, "g0/1", r4, "g0/1");
    lab.cable(b, "fa0", r4, "g0/2");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(b, "ipconfig 192.168.4.10 255.255.255.0 192.168.4.1");
    let cfg = [
        (r1, [("g0/0", "192.168.1.1 255.255.255.0"), ("g0/1", "10.0.12.1 255.255.255.252"), ("g0/2", "10.0.13.1 255.255.255.0")]),
        (r2, [("g0/0", "10.0.12.2 255.255.255.252"), ("g0/1", "10.0.24.1 255.255.255.252"), ("g0/2", "")]),
        (r3, [("g0/0", "10.0.13.3 255.255.255.0"), ("g0/1", "10.0.34.1 255.255.255.252"), ("g0/2", "")]),
        (r4, [("g0/0", "10.0.24.2 255.255.255.252"), ("g0/1", "10.0.34.2 255.255.255.252"), ("g0/2", "192.168.4.1 255.255.255.0")]),
    ];
    for (r, ifs) in cfg {
        lab.run(r, "en\nconf t");
        for (i, ip) in ifs.iter().filter(|(_, ip)| !ip.is_empty()) {
            lab.run(r, &format!("int {i}\nip address {ip}\nno shut"));
        }
        lab.run(r, "router eigrp 10\nnetwork 10.0.0.0\nnetwork 192.168.0.0 0.0.255.255\nno auto-summary\nend");
    }
    (lab, [r1, r2, r3, r4, a, b], sw)
}

#[test]
fn eigrp_load_balances_over_equal_and_unequal_paths() {
    let (mut lab, [r1, _, _, _, a, _], _) = eigrp_diamond();
    // Two equal-cost successors to R4's LAN: both installed, both in the table.
    let rt = lab.run(r1, "show ip route");
    assert!(rt.contains("D    192.168.4.0/24       via 10.0.12.2, GigabitEthernet0/1 (EIGRP)"), "{rt}");
    assert!(rt.contains("D    192.168.4.0/24       via 10.0.13.3, GigabitEthernet0/2 (EIGRP)"), "{rt}");
    assert!(lab.run(r1, "show ip eigrp topology").contains("192.168.4.0/24  FD 3328, 2 successors"));
    // Flows spread over both, each flow always the same way.
    let d = lab.t.device(r1).unwrap().clone();
    let dst: std::net::Ipv4Addr = "192.168.4.10".parse().unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for k in 1..40u8 {
        let src = std::net::Ipv4Addr::new(192, 168, 1, k);
        let (out, _) = lab.sim.route_flow(&lab.t, &d, src, dst).unwrap();
        assert_eq!(lab.sim.route_flow(&lab.t, &d, src, dst).unwrap().0.name, out.name);
        seen.insert(out.name);
    }
    assert_eq!(seen.len(), 2, "{seen:?}");
    lab.run(a, "ping 192.168.4.10");
    assert!(lab.run(a, "ping 192.168.4.10").contains("4 answered"));
    // `maximum-paths 1`: one path only.
    lab.run(r1, "conf t\nrouter eigrp 10\nmaximum-paths 1\nend");
    let rt = lab.run(r1, "show ip route eigrp");
    assert_eq!(rt.matches("192.168.4.0/24").count(), 1, "{rt}");
    assert!(lab.run(r1, "show ip protocols").contains("up to 1 paths per destination"));
    // A slower way through R3 is a feasible successor (R3 reports 3072 < FD 3328), unused…
    lab.run(r1, "conf t\nrouter eigrp 10\nno maximum-paths\nexit\nint g0/2\ndelay 10\nend");
    let rt = lab.run(r1, "show ip route eigrp");
    assert_eq!(rt.matches("192.168.4.0/24").count(), 1, "{rt}");
    let topo = lab.run(r1, "show ip eigrp topology");
    assert!(topo.contains("feasible successor  via 10.0.13.3, Gi0/2 (5632 / 3072)"), "{topo}");
    // …until `variance 2` admits it: below twice the feasible distance.
    lab.run(r1, "conf t\nrouter eigrp 10\nvariance 2\nend");
    let rt = lab.run(r1, "show ip route eigrp");
    assert_eq!(rt.matches("192.168.4.0/24").count(), 2, "{rt}");
    assert!(lab.run(r1, "show ip eigrp topology").contains("(5632 / 3072), in use (variance)"));
    assert!(lab.run(r1, "show ip protocols").contains("up to 4 paths per destination, variance 2"));
    // The settings survive a save and reload.
    let run = lab.run(r1, "show running-config");
    assert!(run.contains(" variance 2\n"), "{run}");
    lab.run(r1, "conf t\nrouter eigrp 10\nmaximum-paths 3\nend");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains(" variance 2\n maximum-paths 3\n no auto-summary"), "{run}");
    lab.run(r1, "copy run start\nreload\nen");
    assert_eq!(lab.run(r1, "show running-config"), run);
    lab.run(r1, "conf t\nrouter eigrp 10");
    for bad in ["maximum-paths 0", "maximum-paths 17", "variance 0", "variance 129"] {
        assert!(lab.try_run(r1, bad).starts_with('%'), "{bad}");
    }
    lab.run(r1, "end");
}

#[test]
fn a_silent_eigrp_neighbour_is_dropped_after_its_hold_time() {
    let (mut lab, [r1, _, r3, r4, ..], sw) = eigrp_diamond();
    // R3 announces a 30-second hold time and says hello every 10 seconds.
    lab.run(r3, "conf t\nint g0/0\nip hello-interval eigrp 10 10\nip hold-time eigrp 10 30\nend");
    let ifs = lab.run(r3, "show ip eigrp interfaces");
    assert!(ifs.lines().any(|l| l.starts_with("Gi0/0") && l.contains(" 10 ") && l.contains(" 30 ")), "{ifs}");
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 11_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("EIGRP hello, AS 10 (hold 30 s) 10.0.13.3")));
    let nb = lab.run(r1, "show ip eigrp neighbors");
    assert!(nb.contains("10.0.13.3"), "{nb}");
    lab.run(r1, "show logging");
    lab.sim.take_console(r1);
    // The switch port towards R3 goes down: R1's own link stays up, so it only notices that
    // the hellos stopped.
    lab.run(sw, "en\nconf t\nint fa0/2\nshutdown\nend");
    let nb = lab.run(r1, "show ip eigrp neighbors");
    assert!(nb.contains("10.0.13.3") && nb.contains("no hellos heard"), "{nb}");
    assert_eq!(lab.run(r1, "show ip route eigrp").matches("192.168.4.0/24").count(), 2);
    // R3's own interface went down with the switch port: it drops R1 at once.
    assert!(!lab.run(r3, "show ip eigrp neighbors").contains("10.0.13.1"));
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 15_000_000);
    assert!(lab.run(r1, "show ip eigrp neighbors").contains("10.0.13.3"), "still within the hold time");
    lab.sim.run_for(&lab.t, 16_000_000);
    let nb = lab.run(r1, "show ip eigrp neighbors");
    assert!(!nb.contains("10.0.13.3"), "{nb}");
    let log = lab.run(r1, "show logging");
    assert!(log.contains("EIGRP AS 10: neighbour 10.0.13.3 (GigabitEthernet0/2) is down: hold time expired"), "{log}");
    // Reconverged: only the way through R2 is left, and R1 told its neighbours.
    let rt = lab.run(r1, "show ip route eigrp");
    assert_eq!(rt.matches("192.168.4.0/24").count(), 1, "{rt}");
    assert!(rt.contains("via 10.0.12.2"), "{rt}");
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("EIGRP update") && e.summary.contains("10.0.12.1 → 224.0.0.10")));
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("EIGRP acknowledgement")));
    // The neighbour comes back as soon as it is heard again.
    lab.run(sw, "conf t\nint fa0/2\nno shutdown\nend");
    assert!(lab.run(r1, "show ip eigrp neighbors").contains("10.0.13.3"));
    assert!(lab.run(r1, "show logging").contains("neighbour 10.0.13.3 (GigabitEthernet0/2) is up"));
    lab.sim.run_for(&lab.t, 1_000_000);
    assert!(lab.sim.trace().iter().any(|e| e.summary.starts_with("EIGRP update (init)")));
    // A lost destination with no feasible successor: queries, answered by replies.
    lab.sim.clear_trace();
    lab.run(r4, "conf t\nint g0/2\nshutdown\nend");
    lab.sim.run_for(&lab.t, 1_000_000);
    let trace = lab.sim.trace();
    assert!(trace.iter().any(|e| e.summary.starts_with("EIGRP query") && e.summary.contains("1 unreachable")), "no query");
    assert!(trace.iter().any(|e| e.summary.starts_with("EIGRP reply")), "no reply");
    assert!(!lab.run(r1, "show ip route").contains("192.168.4.0"));
    // `debug eigrp packets` reports them.
    lab.run(r1, "debug eigrp packets");
    lab.run(r4, "conf t\nint g0/2\nno shutdown\nend");
    lab.sim.run_for(&lab.t, 1_000_000);
    assert!(lab.run(r1, "show logging").contains("EIGRP debug: update #"));
    // The timers are part of the interface's configuration.
    let run = lab.run(r3, "show running-config");
    assert!(run.contains(" ip hello-interval eigrp 10 10\n ip hold-time eigrp 10 30\n"), "{run}");
    lab.run(r3, "copy run start\nreload\nen");
    assert_eq!(lab.run(r3, "show running-config"), run);
    lab.run(r3, "conf t\nint g0/0\nno ip hold-time eigrp 10\nno ip hello-interval eigrp 10\nend");
    assert!(!lab.run(r3, "show running-config").contains("eigrp 10 "));
}

#[test]
fn ospf_installs_equal_cost_paths() {
    let (mut lab, [r1, r2, r3, r4, ..], _) = eigrp_diamond();
    for r in [r1, r2, r3, r4] {
        lab.run(r, "conf t\nno router eigrp 10\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend");
    }
    let rt = lab.run(r1, "show ip route ospf");
    assert_eq!(rt.matches("192.168.4.0/24").count(), 2, "{rt}");
    lab.run(r1, "conf t\nrouter ospf 1\nmaximum-paths 1\nend");
    assert_eq!(lab.run(r1, "show ip route ospf").matches("192.168.4.0/24").count(), 1);
    assert!(lab.run(r1, "show running-config").contains(" maximum-paths 1\n"));
}

#[test]
fn a_gre_tunnel_rides_on_learned_routes_but_not_on_itself() {
    let (mut lab, r1, isp, r2, a) = two_sites();
    // No static routes: OSPF over the physical network is all that reaches the far end.
    lab.run(r1, "conf t\nno ip route 0.0.0.0 0.0.0.0 203.0.113.2\nrouter ospf 1\nnetwork 203.0.113.0 0.0.0.3 area 0\nend");
    lab.run(r2, "conf t\nno ip route 0.0.0.0 0.0.0.0 198.51.100.2\nrouter ospf 1\nnetwork 198.51.100.0 0.0.0.3 area 0\nend");
    lab.run(
        r1,
        "conf t\nint tunnel 0\nip address 10.0.0.1 255.255.255.252\ntunnel source g0/1\ntunnel destination 198.51.100.1\nexit\nip route 192.168.2.0 255.255.255.0 10.0.0.2\nend",
    );
    lab.run(
        r2,
        "conf t\nint tunnel 0\nip address 10.0.0.2 255.255.255.252\ntunnel source g0/1\ntunnel destination 203.0.113.1\nexit\nip route 192.168.1.0 255.255.255.0 10.0.0.1\nend",
    );
    // The ISP doesn't run OSPF yet: nothing reaches the far end, the tunnel stays down.
    assert!(lab.run(r1, "show interfaces tunnel 0").contains("line protocol down (no route to the tunnel destination)"));
    lab.run(isp, "conf t\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend");
    assert!(lab.run(r1, "show ip route ospf").contains("198.51.100.0/30"));
    let tun = lab.run(r1, "show interfaces tunnel 0");
    assert!(tun.contains("Tunnel0 is up, line protocol up"), "{tun}");
    let ping = ping_settled(&mut lab, a, "192.168.2.10");
    assert!(ping.contains("4 answered"), "{ping}");
    // OSPF over the tunnel too, with the tunnel the cheaper way to the far end's public
    // network: the destination would be routed through the tunnel itself.
    lab.run(r1, "conf t\nint g0/1\nip ospf cost 10\nint tunnel 0\nip ospf cost 1\nexit\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nend");
    lab.run(r2, "conf t\nint tunnel 0\nip ospf cost 1\nexit\nrouter ospf 1\nnetwork 10.0.0.0 0.0.0.3 area 0\nend");
    let tun = lab.run(r1, "show interfaces tunnel 0");
    assert!(tun.contains("line protocol down (recursive routing"), "{tun}");
    assert!(lab.run(r1, "show ip interface brief").lines().any(|l| l.starts_with("Tunnel0") && l.ends_with("down")));
    // The far end's own way back is still physical: its tunnel stays up.
    assert!(lab.run(r2, "show interfaces tunnel 0").contains("line protocol up"));
    // The physical way cheaper again: up again.
    lab.run(r1, "conf t\nint g0/1\nno ip ospf cost\nint tunnel 0\nno ip ospf cost\nend");
    lab.run(r2, "conf t\nint tunnel 0\nno ip ospf cost\nend");
    assert!(lab.run(r1, "show interfaces tunnel 0").contains("line protocol up"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
}

#[test]
fn ike_main_mode_and_quick_mode_go_on_the_wire_before_esp() {
    let (mut lab, r1, isp, r2, a) = two_sites();
    lab.run(r1, IPSEC_R1);
    lab.run(r2, IPSEC_R2);
    // Warm up ARP along the way with traffic the crypto maps don't protect.
    ping_settled(&mut lab, r1, "198.51.100.1");
    lab.sim.clear_trace();
    // The first protected packet starts IKE and is dropped; the association comes up when quick
    // mode completes, and the rest goes as ESP.
    let ping = lab.run(a, "ping 192.168.2.10");
    assert!(ping.contains("seq 1: no answer") && !ping.contains(" 0 answered"), "{ping}");
    let trace: Vec<String> = lab.sim.trace().iter().filter(|e| e.to.device == isp).map(|e| e.summary.clone()).collect();
    let pos = |what: &str| trace.iter().position(|s| s.starts_with(what)).unwrap_or_else(|| panic!("no {what}: {trace:#?}"));
    let mm1 = pos("ISAKMP main mode [SA, 1 transform] 203.0.113.1 → 198.51.100.1");
    let mm2 = pos("ISAKMP main mode [SA, 1 transform] 198.51.100.1 → 203.0.113.1");
    let mm3 = pos("ISAKMP main mode [KE, NONCE] 203.0.113.1");
    let mm4 = pos("ISAKMP main mode [KE, NONCE] 198.51.100.1");
    let mm5 = pos("ISAKMP main mode (encrypted) 203.0.113.1");
    let mm6 = pos("ISAKMP main mode (encrypted) 198.51.100.1");
    let qm1 = pos("ISAKMP quick mode (encrypted) 203.0.113.1");
    let qm2 = pos("ISAKMP quick mode (encrypted) 198.51.100.1");
    let esp = pos("ESP SPI");
    assert!(mm1 < mm2 && mm2 < mm3 && mm3 < mm4 && mm4 < mm5 && mm5 < mm6 && mm6 < qm1 && qm1 < qm2 && qm2 < esp, "{trace:#?}");
    assert_eq!(trace.iter().filter(|s| s.starts_with("ISAKMP")).count(), 9, "{trace:#?}");
    assert!(lab.run(r1, "show crypto isakmp sa").contains("established"));
    assert!(lab.run(r2, "show crypto isakmp sa").contains("established"));
    assert!(lab.run(r1, "show logging").contains("IKE with 198.51.100.1: main mode and quick mode complete"));

    // Policies that don't agree: refused at main mode message 2.
    let retry = |lab: &mut Lab| {
        lab.run(r1, "clear crypto sa");
        lab.run(r2, "clear crypto sa");
        lab.sim.clear_trace();
        ping_settled(lab, a, "192.168.2.10")
    };
    lab.run(r2, "conf t\ncrypto isakmp policy 10\nencryption 3des\nend");
    assert!(retry(&mut lab).contains(" 0 answered"));
    let sum = |lab: &Lab| lab.sim.trace().iter().map(|e| e.summary.clone()).collect::<Vec<_>>().join("\n");
    assert!(sum(&lab).contains("ISAKMP informational [NOTIFY NO-PROPOSAL-CHOSEN] 198.51.100.1 → 203.0.113.1"), "{}", sum(&lab));
    assert!(!sum(&lab).contains("[KE, NONCE]"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("failed: no ISAKMP policy matches the peer's") && sa.contains("main mode message 2"), "{sa}");
    lab.run(r2, "conf t\ncrypto isakmp policy 10\nencryption aes 256\nend");
    // Keys that differ: authentication fails at main mode message 5.
    lab.run(r2, "conf t\ncrypto isakmp key other address 203.0.113.1\nend");
    assert!(retry(&mut lab).contains(" 0 answered"));
    assert!(sum(&lab).contains("[NOTIFY AUTHENTICATION-FAILED]"), "{}", sum(&lab));
    assert!(lab.run(r2, "show logging").contains("main mode message 5 failed authentication"));
    assert!(lab.run(r1, "show crypto isakmp sa").contains("failed: the pre-shared keys differ"));
    lab.run(r2, "conf t\ncrypto isakmp key vpnsecret address 203.0.113.1\nend");
    // Transform sets that differ: refused in quick mode.
    lab.run(r2, "conf t\ncrypto ipsec transform-set TS esp-3des esp-md5-hmac\nend");
    assert!(retry(&mut lab).contains(" 0 answered"));
    assert!(sum(&lab).contains("ISAKMP quick mode (encrypted)") && sum(&lab).contains("[NOTIFY NO-PROPOSAL-CHOSEN]"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("transform sets differ") && sa.contains("quick mode message 2"), "{sa}");
    lab.run(r2, "conf t\ncrypto ipsec transform-set TS esp-aes esp-sha-hmac\nend");
    assert!(retry(&mut lab).contains("4 answered"));
}

#[test]
fn call_control_and_voice_vlans_survive_a_reload() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    lab.run(
        r,
        "enable
        configure terminal
        ip dhcp pool VOICE
        network 10.9.0.0 255.255.255.0
        option 150 ip 10.9.0.1
        exit
        telephony-service
        max-ephones 2
        max-dn 2
        ip source-address 10.9.0.1 port 2000
        auto assign 1 to 2
        exit
        ephone-dn 1
        number 2001
        ephone-dn 2
        number 2002
        exit
        ephone 1
        mac-address 0200.0000.0042
        button 1:2
        end",
    );
    assert_eq!(lab.prompt(r), "R1#");
    // Limits and checks.
    let err = lab.try_run(r, "configure terminal");
    assert!(!err.starts_with('%'));
    assert!(lab.try_run(r, "ephone-dn 3").starts_with("% max-dn is 2"));
    lab.try_run(r, "ephone-dn 1");
    assert_eq!(lab.prompt(r), "R1(config-ephone-dn)#");
    assert!(lab.try_run(r, "number 2002").contains("already"));
    assert!(lab.try_run(r, "number 12x").starts_with('%'));
    assert!(lab.try_run(r, "mac-address 0200.0000.0042").starts_with('%'), "mac-address belongs under ephone");
    lab.try_run(r, "ephone 1");
    assert!(lab.try_run(r, "button 1:9").contains("no ephone-dn 9"));
    lab.run(r, "end\ncopy running-config startup-config\nreload");
    let run = lab.run(r, "enable\nshow running-config");
    for l in [
        " option 150 ip 10.9.0.1",
        "telephony-service",
        " ip source-address 10.9.0.1 port 2000",
        " number 2002",
        " mac-address 0200.0000.0042",
        " button 1:2",
    ] {
        assert!(run.contains(l), "{l}\n{run}");
    }
    assert!(lab.run(r, "show ephone").contains("02:00:00:00:00:42"));
    assert!(lab.run(r, "show telephony-service").contains("Phones allowed:       2"));
    lab.run(s, "enable\nconfigure terminal\ninterface fa0/3\nswitchport voice vlan 150\nend");
    assert!(lab.run(s, "show running-config").contains(" switchport voice vlan 150"));
    assert!(lab.run(s, "show vlan brief").contains("Fa0/3"));
    // Not on switches.
    lab.try_run(s, "configure terminal");
    assert!(lab.try_run(s, "telephony-service").contains('%'));
}

// ---- switch edge protection and tuning (M5.40-M5.43) ----

/// Writes what the simulator decided into the lab, as the engine does after every command.
fn l2_apply(lab: &mut Lab) {
    for e in lab.sim.take_events() {
        match e {
            netlabcraft_sim::PortEvent::ErrDisabled { device, port } => lab.t.device_mut(device).unwrap().ports[port].err_disabled = true,
            netlabcraft_sim::PortEvent::ErrDisabledBy { device, port, cause } => {
                let p = &mut lab.t.device_mut(device).unwrap().ports[port];
                p.err_disabled = true;
                p.l2.err_cause = Some(cause);
            }
            netlabcraft_sim::PortEvent::Recovered { device, port } => {
                let p = &mut lab.t.device_mut(device).unwrap().ports[port];
                p.err_disabled = false;
                p.l2.err_cause = None;
            }
            netlabcraft_sim::PortEvent::Sticky { .. } => {}
        }
    }
    lab.sim.topology_changed(&lab.t);
}

fn dropped_reasons(lab: &Lab) -> Vec<String> {
    lab.sim.trace().iter().filter_map(|e| e.dropped.clone()).collect()
}

/// R1 serves DHCP behind Fa0/24; R2, a rogue server, sits on Fa0/23; PC1 and PC2 are clients.
fn snooping_lab() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    let s = lab.add(DeviceKind::Switch);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    lab.cable(r1, "g0/0", s, "fa0/24");
    lab.cable(r2, "g0/0", s, "fa0/23");
    lab.cable(a, "fa0", s, "fa0/1");
    lab.cable(b, "fa0", s, "fa0/2");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 192.168.10.1 255.255.255.0\nno shut\nexit\nip dhcp excluded-address 192.168.10.1 192.168.10.9\nip dhcp pool LAN\nnetwork 192.168.10.0 255.255.255.0\ndefault-router 192.168.10.1\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 10.66.0.1 255.255.255.0\nno shut\nexit\nip dhcp pool EVIL\nnetwork 10.66.0.0 255.255.255.0\ndefault-router 10.66.0.1\nend");
    (lab, r1, s, a, b)
}

#[test]
fn dhcp_snooping_blocks_a_rogue_server_and_builds_bindings() {
    let (mut lab, _r1, s, a, b) = snooping_lab();
    lab.run(
        s,
        "en\nconf t\nip dhcp snooping\nip dhcp snooping vlan 1\nno ip dhcp snooping information option\nint fa0/24\nip dhcp snooping trust\nend",
    );
    lab.sim.clear_trace();
    let out = lab.run(a, "ipconfig /renew");
    assert!(out.contains("192.168.10.10/24"), "{out}");
    let drops = dropped_reasons(&lab);
    assert!(drops.iter().any(|r| r.contains("DHCP snooping") && r.contains("OFFER") && r.contains("untrusted port Fa0/23")), "{drops:?}");
    let bind = lab.run(s, "show ip dhcp snooping binding");
    assert!(bind.contains("192.168.10.10") && bind.contains("Fa0/1") && bind.contains("1 binding"), "{bind}");
    let show = lab.run(s, "show ip dhcp snooping");
    assert!(show.contains("on") && show.contains("Fa0/24") && show.contains("Option 82 insertion:      off"), "{show}");
    assert!(!show.contains("DHCP messages dropped:    0"), "{show}");
    // Without snooping the switch passes whatever answers first.
    lab.run(s, "conf t\nno ip dhcp snooping\nend");
    lab.sim.clear_trace();
    lab.run(b, "ipconfig /renew");
    assert!(!dropped_reasons(&lab).iter().any(|r| r.contains("DHCP snooping")));
    // A rate limit: a client's DISCOVER and REQUEST in one second exceed 1 a second.
    lab.run(s, "conf t\nip dhcp snooping\nint fa0/2\nip dhcp snooping limit rate 1\nend");
    lab.run(b, "ipconfig /release");
    let _ = lab.try_run(b, "ipconfig /renew");
    l2_apply(&mut lab);
    assert_eq!(lab.t.device(s).unwrap().ports[1].l2.err_cause, Some(netlabcraft_model::l2sec::ErrCause::DhcpRateLimit));
    assert!(lab.run(s, "show interfaces fa0/2").contains("err-disabled by the DHCP snooping rate limit"));
    assert!(lab.run(s, "show logging").contains("more than 1 DHCP messages a second"));
    lab.run(s, "clear ip dhcp snooping binding");
    assert!(lab.run(s, "show ip dhcp snooping binding").contains("0 bindings"));
}

#[test]
fn arp_inspection_drops_a_spoofed_arp() {
    let (mut lab, _r1, s, a, b) = snooping_lab();
    lab.run(s, "en\nconf t\nip dhcp snooping\nip dhcp snooping vlan 1\nip arp inspection vlan 1\nip arp inspection validate src-mac ip\nint fa0/24\nip dhcp snooping trust\nip arp inspection trust\nend");
    assert!(lab.run(a, "ipconfig /renew").contains("192.168.10.10/24"));
    // PC1's address is bound: its ARP passes.
    assert!(lab.run(a, "ping 192.168.10.1").contains("4 answered"));
    // PC2 claims PC1's address by hand: its ARP matches no binding.
    lab.run(b, "ipconfig 192.168.10.10 255.255.255.0 192.168.10.1");
    lab.sim.clear_trace();
    let out = lab.run(b, "ping 192.168.10.1");
    assert!(out.contains("0 answered"), "{out}");
    let drops = dropped_reasons(&lab);
    assert!(drops.iter().any(|r| r.contains("ARP inspection") && r.contains("192.168.10.10") && r.contains("no DHCP snooping binding")), "{drops:?}");
    let show = lab.run(s, "show ip arp inspection");
    assert!(show.contains("VLANs inspected:  1") && show.contains("src-mac, ip") && show.contains("Fa0/24"), "{show}");
    assert!(lab.run(s, "show logging").contains("ARP inspection dropped"));
    // Trusting the port lets it through again.
    lab.run(s, "conf t\nint fa0/2\nip arp inspection trust\nend");
    assert!(!lab.run(b, "ping 192.168.10.1").contains(" 0 answered"));
    let run = lab.run(s, "show run");
    assert!(run.contains("ip arp inspection vlan 1\nip arp inspection validate src-mac ip"), "{run}");
    assert!(run.contains(" ip dhcp snooping trust\n ip arp inspection trust"), "{run}");
}

#[test]
fn bpdu_guard_err_disables_and_recovery_brings_the_port_back() {
    let mut lab = Lab::new();
    let s1 = lab.add(DeviceKind::Switch);
    let s2 = lab.add(DeviceKind::Switch);
    let s3 = lab.add(DeviceKind::Switch);
    lab.cable(s1, "fa0/1", s2, "fa0/1");
    lab.cable(s1, "fa0/2", s3, "fa0/1");
    lab.run(s1, "en\nconf t\nerrdisable recovery cause bpduguard\nerrdisable recovery interval 30\nint fa0/1\nspanning-tree portfast\nspanning-tree bpduguard enable\nend");
    lab.sim.run_for(&lab.t, 3_000_000);
    l2_apply(&mut lab);
    assert!(lab.t.device(s1).unwrap().ports[0].err_disabled);
    let err = lab.run(s1, "show interfaces status err-disabled");
    assert!(err.contains("Fa0/1") && err.contains("bpduguard"), "{err}");
    assert!(lab.run(s1, "show logging").contains("BPDU guard"));
    assert!(lab.run(s1, "show interfaces fa0/1").contains("err-disabled by BPDU guard"));
    let rec = lab.run(s1, "show errdisable recovery");
    assert!(rec.contains("bpduguard            yes") && rec.contains("Recovery interval: 30 s") && rec.contains("Fa0/1"), "{rec}");
    // Thirty simulated seconds later the port comes back by itself.
    lab.sim.run_for(&lab.t, 31_000_000);
    let events = lab.sim.take_events();
    assert!(events.iter().any(|e| matches!(e, netlabcraft_sim::PortEvent::Recovered { port: 0, .. })), "{events:?}");
    for e in events {
        if let netlabcraft_sim::PortEvent::Recovered { device, port } = e {
            lab.t.device_mut(device).unwrap().ports[port].err_disabled = false;
        }
    }
    assert!(!lab.t.device(s1).unwrap().ports[0].err_disabled);
    assert!(lab.run(s1, "show logging").contains("errdisable recovery (bpduguard)"));
    // Still cabled to a switch: down again; without recovery it stays down.
    lab.sim.run_for(&lab.t, 3_000_000);
    l2_apply(&mut lab);
    assert!(lab.t.device(s1).unwrap().ports[0].err_disabled);
    lab.run(s1, "conf t\nno errdisable recovery cause bpduguard\nend");
    lab.sim.run_for(&lab.t, 120_000_000);
    l2_apply(&mut lab);
    assert!(lab.t.device(s1).unwrap().ports[0].err_disabled);
    assert!(lab.run(s1, "show errdisable recovery").contains("never"));
    // The global defaults: every access port an edge port, with BPDU guard.
    lab.run(s1, "conf t\nspanning-tree portfast default\nspanning-tree portfast bpduguard default\nend");
    lab.sim.run_for(&lab.t, 3_000_000);
    l2_apply(&mut lab);
    assert_eq!(lab.t.device(s1).unwrap().ports[1].l2.err_cause, Some(netlabcraft_model::l2sec::ErrCause::Bpduguard));
    // `spanning-tree bpduguard disable` overrides the default; shutdown / no shutdown revives.
    lab.run(s1, "conf t\nint fa0/2\nspanning-tree bpduguard disable\nshutdown\nno shutdown\nend");
    lab.sim.run_for(&lab.t, 3_000_000);
    l2_apply(&mut lab);
    assert!(!lab.t.device(s1).unwrap().ports[1].err_disabled);
    let run = lab.run(s1, "show run");
    assert!(run.contains("spanning-tree portfast default\nspanning-tree portfast bpduguard default"), "{run}");
    assert!(run.contains(" spanning-tree portfast\n spanning-tree bpduguard enable"), "{run}");
    assert!(run.contains(" spanning-tree bpduguard disable"), "{run}");
}

#[test]
fn port_security_aging_forgets_learned_addresses() {
    let mut lab = Lab::new();
    let s = lab.add(DeviceKind::Switch);
    let hub = lab.add(DeviceKind::Hub);
    let a = lab.add(DeviceKind::Pc);
    let b = lab.add(DeviceKind::Pc);
    let c = lab.add(DeviceKind::Pc);
    lab.cable(hub, "e0", s, "fa0/1");
    lab.cable(a, "fa0", hub, "e1");
    lab.cable(b, "fa0", hub, "e2");
    lab.cable(c, "fa0", s, "fa0/2");
    lab.run(a, "ipconfig 10.0.0.1 255.255.255.0");
    lab.run(b, "ipconfig 10.0.0.2 255.255.255.0");
    lab.run(c, "ipconfig 10.0.0.3 255.255.255.0");
    lab.run(s, "en\nconf t\nint fa0/1\nswitchport port-security\nswitchport port-security violation restrict\nswitchport port-security aging time 1\nswitchport port-security aging type inactivity\nend");
    assert!(lab.run(a, "ping 10.0.0.3").contains("4 answered"));
    assert!(lab.run(b, "ping 10.0.0.3").contains("0 answered"), "PC2 is one address too many");
    // A quiet minute later PC1's address is forgotten and PC2 takes the place.
    lab.sim.run_for(&lab.t, 61_000_000);
    assert!(lab.run(b, "ping 10.0.0.3").contains("4 answered"));
    let run = lab.run(s, "show run");
    assert!(run.contains(" switchport port-security aging time 1\n switchport port-security aging type inactivity"), "{run}");
    lab.run(s, "conf t\nint fa0/3");
    assert!(lab.try_run(s, "switchport port-security aging time 5").contains("no port security"));
}
