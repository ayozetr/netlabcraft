//! Sample labs, built with the same commands a person would type. Original content.

use std::net::Ipv4Addr;

use packetcraft_model::{DeviceKind, End, Topology};
use packetcraft_sim::Sim;

use crate::{EngineError, Result};

pub struct Sample {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

pub static SAMPLES: &[Sample] = &[
    Sample { id: "lan", title: "One switch, three PCs", description: "A flat LAN: ping between the PCs and watch the switch learn their addresses." },
    Sample {
        id: "two-subnets",
        title: "Two subnets and a router",
        description: "Each subnet on its own switch; the router joins them. Try ping and tracert across.",
    },
    Sample {
        id: "vlans",
        title: "VLANs and router-on-a-stick",
        description: "One switch split into VLAN 10 and 20, routed by subinterfaces over an 802.1Q trunk.",
    },
    Sample {
        id: "static-routing",
        title: "Three routers, static routes",
        description: "A chain of routers with static and default routes between two LANs.",
    },
    Sample {
        id: "stp",
        title: "Spanning tree on a switch triangle",
        description: "Three switches wired in a loop: spanning tree blocks one port (amber light) and traffic still flows.",
    },
    Sample {
        id: "etherchannel",
        title: "EtherChannel between two switches",
        description: "Two cables bundled with LACP into one logical link: spanning tree blocks nothing, and one cable can fail without a break.",
    },
    Sample {
        id: "vtp",
        title: "VTP: VLANs from one server",
        description: "S1 is the VTP server of domain LAB; S2 and S3 are clients and learn its VLANs over the trunks.",
    },
    Sample {
        id: "ospf",
        title: "OSPF on three routers",
        description: "Three routers in a triangle learn each other's networks with OSPF; cut a link and the routes move.",
    },
    Sample {
        id: "nat-acl",
        title: "NAT to the Internet, with an access list",
        description: "An office reaches an Internet server through PAT on its router; an access list keeps one PC from pinging the server.",
    },
    Sample {
        id: "services",
        title: "A server with DHCP, DNS and a web site",
        description: "PCs get their addresses from the server, resolve www.packetcraft.lab with its DNS service and open its web pages.",
    },
    Sample {
        id: "dhcp",
        title: "DHCP and basic device security",
        description: "A router hands out addresses with DHCP; console, vty and enable are password protected (console: cisco, enable: class).",
    },
];

struct Builder {
    t: Topology,
    sim: Sim,
}

impl Builder {
    fn add_model(&mut self, model: &str, name: &str, x: f32, y: f32) -> Result<packetcraft_core::DeviceId> {
        let m = packetcraft_model::models::find(model).ok_or_else(|| EngineError::Failed(format!("no model {model}")))?;
        Ok(self.t.add_model(m, Some(name), x * 1.4, y * 1.15)?)
    }

    /// Places a device; sample coordinates are on a compact grid, spread out here.
    fn add(&mut self, kind: DeviceKind, name: &str, x: f32, y: f32) -> Result<packetcraft_core::DeviceId> {
        Ok(self.t.add_device(kind, Some(name), x * 1.4, y * 1.15)?)
    }

    fn cable(&mut self, a: &str, pa: &str, b: &str, pb: &str) -> Result<()> {
        let da = self.t.resolve(a)?;
        let db = self.t.resolve(b)?;
        let ia = self.t.device(da).and_then(|d| d.port_index(pa)).ok_or_else(|| EngineError::Failed(format!("{a} {pa}")))?;
        let ib = self.t.device(db).and_then(|d| d.port_index(pb)).ok_or_else(|| EngineError::Failed(format!("{b} {pb}")))?;
        Ok(self.t.connect(End { device: da, port: ia }, End { device: db, port: ib })?)
    }

    /// Types `lines` at a device; any error fails the build (samples must be clean).
    fn type_in(&mut self, dev: &str, lines: &str) -> Result<()> {
        let d = self.t.resolve(dev)?;
        let mut cli = packetcraft_ios::Cli::default();
        for l in lines.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let o = packetcraft_ios::execute(&mut self.t, &mut self.sim, d, &mut cli, l);
            if o.output.starts_with("% ") || o.output.contains("\n% ") {
                return Err(EngineError::Failed(format!("sample: {dev}: `{l}`: {}", o.output)));
            }
        }
        Ok(())
    }
}

pub fn build(id: &str) -> Result<Topology> {
    let mut b = Builder { t: Topology::new(), sim: Sim::new() };
    match id {
        "lan" => {
            b.add(DeviceKind::Switch, "S1", 300.0, 120.0)?;
            for (i, x) in [(1, 120.0), (2, 300.0), (3, 480.0)] {
                b.add(DeviceKind::Pc, &format!("PC{i}"), x, 300.0)?;
                b.cable(&format!("PC{i}"), "fa0", "S1", &format!("fa0/{i}"))?;
                b.type_in(&format!("PC{i}"), &format!("ipconfig 192.168.1.{} 255.255.255.0", 10 + i))?;
            }
            b.add_model("Laptop", "Laptop1", 480.0, 120.0)?;
            b.cable("Laptop1", "fa0", "S1", "fa0/10")?;
            b.type_in("Laptop1", "ipconfig 192.168.1.20 255.255.255.0")?;
            b.add_model("Printer", "Printer1", 120.0, 120.0)?;
            b.cable("Printer1", "fa0", "S1", "fa0/11")?;
            b.type_in("Printer1", "ipconfig 192.168.1.30 255.255.255.0")?;
            b.t.add_annotation(150.0, 20.0, "Office LAN 192.168.1.0/24")?;
            b.t.notes = "Ping PC3 from PC1 (`ping 192.168.1.13`), then look at `show mac address-table` on S1.".into();
        }
        "two-subnets" => {
            b.add(DeviceKind::Router, "R1", 360.0, 80.0)?;
            b.add(DeviceKind::Switch, "S1", 160.0, 220.0)?;
            b.add(DeviceKind::Switch, "S2", 560.0, 220.0)?;
            b.cable("R1", "g0/0", "S1", "g0/1")?;
            b.cable("R1", "g0/1", "S2", "g0/1")?;
            for (pc, sw, port, x, net) in [
                ("PC1", "S1", "fa0/1", 80.0, 1),
                ("PC2", "S1", "fa0/2", 240.0, 1),
                ("PC3", "S2", "fa0/1", 480.0, 2),
                ("PC4", "S2", "fa0/2", 640.0, 2),
            ] {
                b.add(DeviceKind::Pc, pc, x, 380.0)?;
                b.cable(pc, "fa0", sw, port)?;
                let host = if pc == "PC1" || pc == "PC3" { 10 } else { 11 };
                b.type_in(pc, &format!("ipconfig 10.0.{net}.{host} 255.255.255.0 10.0.{net}.1"))?;
            }
            b.type_in(
                "R1",
                "enable
                configure terminal
                interface g0/0
                description LAN 1
                ip address 10.0.1.1 255.255.255.0
                no shutdown
                interface g0/1
                description LAN 2
                ip address 10.0.2.1 255.255.255.0
                no shutdown
                end
                copy running-config startup-config",
            )?;
            b.t.notes = "From PC1: `ping 10.0.2.10` (the first request times out while R1 resolves PC3), then `tracert 10.0.2.10`.".into();
        }
        "vlans" => {
            b.add(DeviceKind::Router, "R1", 360.0, 60.0)?;
            b.add(DeviceKind::Switch, "S1", 360.0, 220.0)?;
            b.cable("R1", "g0/0", "S1", "g0/1")?;
            for (pc, port, x, vlan, host) in
                [("PC1", "fa0/1", 120.0, 10, 10), ("PC2", "fa0/2", 280.0, 10, 11), ("PC3", "fa0/11", 440.0, 20, 10), ("PC4", "fa0/12", 600.0, 20, 11)]
            {
                b.add(DeviceKind::Pc, pc, x, 380.0)?;
                b.cable(pc, "fa0", "S1", port)?;
                b.type_in(pc, &format!("ipconfig 192.168.{vlan}.{host} 255.255.255.0 192.168.{vlan}.1"))?;
            }
            b.type_in(
                "S1",
                "enable
                configure terminal
                vlan 10
                name Staff
                vlan 20
                name Guests
                exit
                interface range fa0/1 - 10
                switchport mode access
                switchport access vlan 10
                interface range fa0/11 - 20
                switchport mode access
                switchport access vlan 20
                interface g0/1
                switchport mode trunk
                switchport trunk allowed vlan 1,10,20
                end
                write memory",
            )?;
            b.type_in(
                "R1",
                "enable
                configure terminal
                interface g0/0
                no shutdown
                interface g0/0.10
                encapsulation dot1Q 10
                ip address 192.168.10.1 255.255.255.0
                interface g0/0.20
                encapsulation dot1Q 20
                ip address 192.168.20.1 255.255.255.0
                end
                write memory",
            )?;
            b.t.notes = "PC1 and PC2 share VLAN 10; PC3 and PC4 VLAN 20. Ping across VLANs and watch the 802.1Q tags in the event list.".into();
        }
        "static-routing" => {
            b.add(DeviceKind::Router, "R1", 140.0, 120.0)?;
            b.add(DeviceKind::Router, "R2", 360.0, 120.0)?;
            b.add(DeviceKind::Router, "R3", 580.0, 120.0)?;
            b.add(DeviceKind::Pc, "PC1", 140.0, 300.0)?;
            b.add(DeviceKind::Server, "SRV1", 580.0, 300.0)?;
            b.cable("PC1", "fa0", "R1", "g0/0")?;
            b.cable("R1", "g0/1", "R2", "g0/0")?;
            b.cable("R2", "g0/1", "R3", "g0/1")?;
            b.cable("SRV1", "g0", "R3", "g0/0")?;
            b.type_in("PC1", "ipconfig 172.16.1.10 255.255.255.0 172.16.1.1")?;
            b.type_in("SRV1", "ipconfig 172.16.3.10 255.255.255.0 172.16.3.1")?;
            b.type_in(
                "R1",
                "en
                conf t
                int g0/0
                ip add 172.16.1.1 255.255.255.0
                no shut
                int g0/1
                ip add 10.0.12.1 255.255.255.252
                no shut
                exit
                ip route 0.0.0.0 0.0.0.0 10.0.12.2
                end
                wr",
            )?;
            b.type_in(
                "R2",
                "en
                conf t
                int g0/0
                ip add 10.0.12.2 255.255.255.252
                no shut
                int g0/1
                ip add 10.0.23.1 255.255.255.252
                no shut
                exit
                ip route 172.16.1.0 255.255.255.0 10.0.12.1
                ip route 172.16.3.0 255.255.255.0 10.0.23.2
                end
                wr",
            )?;
            b.type_in(
                "R3",
                "en
                conf t
                int g0/0
                ip add 172.16.3.1 255.255.255.0
                no shut
                int g0/1
                ip add 10.0.23.2 255.255.255.252
                no shut
                exit
                ip route 0.0.0.0 0.0.0.0 10.0.23.1
                end
                wr",
            )?;
            b.t.notes = "PC1 reaches SRV1 through three routers. Remove a static route on R2 and watch `tracert` stop there.".into();
        }
        "stp" => {
            b.add(DeviceKind::Switch, "S1", 360.0, 60.0)?;
            b.add(DeviceKind::Switch, "S2", 180.0, 240.0)?;
            b.add(DeviceKind::Switch, "S3", 540.0, 240.0)?;
            b.cable("S1", "g0/1", "S2", "g0/1")?;
            b.cable("S1", "g0/2", "S3", "g0/1")?;
            b.cable("S2", "g0/2", "S3", "g0/2")?;
            b.add(DeviceKind::Pc, "PC1", 180.0, 400.0)?;
            b.add(DeviceKind::Pc, "PC2", 540.0, 400.0)?;
            b.cable("PC1", "fa0", "S2", "fa0/1")?;
            b.cable("PC2", "fa0", "S3", "fa0/1")?;
            b.type_in("PC1", "ipconfig 10.10.10.1 255.255.255.0")?;
            b.type_in("PC2", "ipconfig 10.10.10.2 255.255.255.0")?;
            b.type_in("S1", "enable\nconfigure terminal\nspanning-tree vlan 1 root primary\nend")?;
            b.t.notes = "S1 is the root bridge. One port on the S2-S3 link blocks (amber). Compare `show spanning-tree` on each switch, then unplug S1-S3 and watch the blocked port start forwarding.".into();
        }
        "etherchannel" => {
            b.add(DeviceKind::Switch, "S1", 180.0, 140.0)?;
            b.add(DeviceKind::Switch, "S2", 540.0, 140.0)?;
            b.cable("S1", "fa0/23", "S2", "fa0/23")?;
            b.cable("S1", "fa0/24", "S2", "fa0/24")?;
            for (pc, sw, port, x, n) in [
                ("PC1", "S1", "fa0/1", 60.0, 1),
                ("PC2", "S1", "fa0/2", 260.0, 2),
                ("PC3", "S2", "fa0/1", 460.0, 3),
                ("PC4", "S2", "fa0/2", 660.0, 4),
            ] {
                b.add(DeviceKind::Pc, pc, x, 320.0)?;
                b.cable(pc, "fa0", sw, port)?;
                b.type_in(pc, &format!("ipconfig 10.20.0.{n} 255.255.255.0"))?;
            }
            for (sw, mode) in [("S1", "active"), ("S2", "passive")] {
                b.type_in(
                    sw,
                    &format!("enable\nconfigure terminal\ninterface range fa0/23 - 24\nswitchport mode trunk\nchannel-group 1 mode {mode}\nend"),
                )?;
            }
            b.t.notes = "Fa0/23 and Fa0/24 form Port-channel1 (LACP: S1 active, S2 passive). Try `show etherchannel summary` and `show spanning-tree`, then shut one member down and ping again.".into();
        }
        "vtp" => {
            b.add(DeviceKind::Switch, "S1", 360.0, 60.0)?;
            b.add(DeviceKind::Switch, "S2", 180.0, 220.0)?;
            b.add(DeviceKind::Switch, "S3", 540.0, 220.0)?;
            b.cable("S1", "g0/1", "S2", "g0/1")?;
            b.cable("S1", "g0/2", "S3", "g0/1")?;
            for (pc, sw, x, ip) in [
                ("PC1", "S2", 100.0, "10.10.0.1"),
                ("PC2", "S2", 260.0, "10.20.0.1"),
                ("PC3", "S3", 460.0, "10.10.0.2"),
                ("PC4", "S3", 620.0, "10.20.0.2"),
            ] {
                b.add(DeviceKind::Pc, pc, x, 400.0)?;
                let port = if pc == "PC1" || pc == "PC3" { "fa0/1" } else { "fa0/2" };
                b.cable(pc, "fa0", sw, port)?;
                b.type_in(pc, &format!("ipconfig {ip} 255.255.255.0"))?;
            }
            b.type_in("S1", "enable\nconfigure terminal\nvtp domain LAB\nvtp password clase\nvlan 10\nname Ventas\nvlan 20\nname Compras\nexit\ninterface range g0/1 - 2\nswitchport mode trunk\nend")?;
            for sw in ["S2", "S3"] {
                b.type_in(
                    sw,
                    "enable\nconfigure terminal\nvtp mode client\nvtp domain LAB\nvtp password clase\ninterface g0/1\nswitchport mode trunk\ninterface fa0/1\nswitchport access vlan 10\ninterface fa0/2\nswitchport access vlan 20\nend",
                )?;
            }
            b.t.notes = "S1 creates VLANs 10 and 20; the clients S2 and S3 learn them (`show vtp status`, `show vlan brief`). Add `vlan 30` on S1 and look again on a client. PC1 reaches PC3 (VLAN 10), PC2 reaches PC4 (VLAN 20).".into();
        }
        "ospf" => {
            for (name, x, y) in [("R1", 120.0, 80.0), ("R2", 600.0, 80.0), ("R3", 360.0, 260.0)] {
                b.add_model("Router-3", name, x, y)?;
            }
            b.cable("R1", "g0/1", "R2", "g0/1")?;
            b.cable("R1", "g0/2", "R3", "g0/1")?;
            b.cable("R2", "g0/2", "R3", "g0/2")?;
            for (pc, r, x, y, n) in [("PC1", "R1", 0.0, 220.0, 1), ("PC2", "R2", 720.0, 220.0, 2), ("PC3", "R3", 360.0, 420.0, 3)] {
                b.add(DeviceKind::Pc, pc, x, y)?;
                b.cable(pc, "fa0", r, "g0/0")?;
                b.type_in(pc, &format!("ipconfig 192.168.{n}.10 255.255.255.0 192.168.{n}.1"))?;
            }
            let links = [("R1", "10.0.12.1", "10.0.13.1", 1), ("R2", "10.0.12.2", "10.0.23.1", 2), ("R3", "10.0.13.2", "10.0.23.2", 3)];
            for (r, a1, a2, n) in links {
                b.type_in(
                    r,
                    &format!(
                        "enable
                        configure terminal
                        interface g0/0
                        ip address 192.168.{n}.1 255.255.255.0
                        no shutdown
                        interface g0/1
                        ip address {a1} 255.255.255.252
                        no shutdown
                        interface g0/2
                        ip address {a2} 255.255.255.252
                        no shutdown
                        exit
                        router ospf 1
                        router-id {n}.{n}.{n}.{n}
                        network 192.168.{n}.0 0.0.0.255 area 0
                        network 10.0.0.0 0.0.255.255 area 0
                        passive-interface g0/0
                        end
                        write memory"
                    ),
                )?;
            }
            b.t.notes = "No static routes: OSPF finds every network. Try `show ip ospf neighbor` and `show ip route` on R1, then shut down R1 g0/2 and watch the route to 192.168.3.0 move to R2.".into();
        }
        "nat-acl" => {
            b.add(DeviceKind::Router, "Office", 300.0, 120.0)?;
            b.add(DeviceKind::Router, "ISP", 620.0, 120.0)?;
            b.add(DeviceKind::Switch, "S1", 300.0, 280.0)?;
            b.add(DeviceKind::Server, "Web", 860.0, 120.0)?;
            b.cable("Office", "g0/0", "S1", "g0/1")?;
            b.cable("Office", "g0/1", "ISP", "g0/1")?;
            b.cable("Web", "g0", "ISP", "g0/0")?;
            for (i, pc) in ["PC1", "PC2"].into_iter().enumerate() {
                b.add(DeviceKind::Pc, pc, 180.0 + 240.0 * i as f32, 440.0)?;
                b.cable(pc, "fa0", "S1", &format!("fa0/{}", i + 1))?;
                b.type_in(pc, &format!("ipconfig 192.168.0.{} 255.255.255.0 192.168.0.1", 10 + i))?;
            }
            b.type_in("Web", "ipconfig 93.184.216.34 255.255.255.0 93.184.216.1")?;
            b.type_in(
                "ISP",
                "enable
                configure terminal
                interface g0/1
                ip address 203.0.113.2 255.255.255.252
                no shutdown
                interface g0/0
                ip address 93.184.216.1 255.255.255.0
                no shutdown
                end",
            )?;
            b.type_in(
                "Office",
                "enable
                configure terminal
                interface g0/0
                ip address 192.168.0.1 255.255.255.0
                ip nat inside
                ip access-group GUESTS in
                no shutdown
                interface g0/1
                ip address 203.0.113.1 255.255.255.252
                ip nat outside
                no shutdown
                exit
                ip route 0.0.0.0 0.0.0.0 203.0.113.2
                access-list 1 permit 192.168.0.0 0.0.0.255
                ip nat inside source list 1 interface g0/1 overload
                ip access-list extended GUESTS
                deny icmp host 192.168.0.11 host 93.184.216.34
                permit ip any any
                end
                write memory",
            )?;
            b.t.add_annotation(560.0, 30.0, "Internet")?;
            b.t.notes = "PC1 pings the Web server through PAT (`show ip nat translations` on Office). PC2 is blocked by the GUESTS access list (`show access-lists`). The ISP has no route to 192.168.0.0/24: only NAT makes the replies come back.".into();
        }
        "services" => {
            use packetcraft_model::services::{DnsRecord, ServerPool};
            b.add(DeviceKind::Switch, "S1", 360.0, 160.0)?;
            b.add(DeviceKind::Server, "Server0", 360.0, 20.0)?;
            b.cable("Server0", "g0", "S1", "g0/1")?;
            b.type_in("Server0", "ipconfig 192.168.100.2 255.255.255.0 192.168.100.1 192.168.100.2")?;
            let srv = b.t.resolve("Server0")?;
            if let Some(d) = b.t.device_mut(srv) {
                d.services.dns.on = true;
                d.services.dns.records.push(DnsRecord { name: "www.packetcraft.lab".into(), address: Ipv4Addr::new(192, 168, 100, 2) });
                d.services.dhcp.on = true;
                d.services.dhcp.pools = vec![ServerPool {
                    name: "office".into(),
                    gateway: Ipv4Addr::new(192, 168, 100, 1),
                    dns: Ipv4Addr::new(192, 168, 100, 2),
                    start: Ipv4Addr::new(192, 168, 100, 50),
                    mask: Ipv4Addr::new(255, 255, 255, 0),
                    max_users: 100,
                }];
            }
            for (i, (name, model)) in [("PC1", "PC"), ("PC2", "PC"), ("Laptop1", "Laptop")].into_iter().enumerate() {
                b.add_model(model, name, 160.0 + 200.0 * i as f32, 300.0)?;
                b.cable(name, "fa0", "S1", &format!("fa0/{}", i + 1))?;
                b.type_in(name, "ipconfig /renew")?;
            }
            b.t.notes = "Open PC1 > Desktop > Web Browser and go to www.packetcraft.lab. Edit the pages in Server0 > Services > HTTP. Try `ping www.packetcraft.lab` and `nslookup www.packetcraft.lab` in a Command Prompt.".into();
        }
        "dhcp" => {
            b.add(DeviceKind::Router, "R1", 360.0, 60.0)?;
            b.add(DeviceKind::Switch, "S1", 360.0, 200.0)?;
            b.cable("R1", "g0/0", "S1", "g0/1")?;
            b.type_in(
                "R1",
                "enable
                configure terminal
                interface g0/0
                ip address 192.168.50.1 255.255.255.0
                no shutdown
                exit
                ip dhcp excluded-address 192.168.50.1 192.168.50.20
                ip dhcp pool OFFICE
                network 192.168.50.0 255.255.255.0
                default-router 192.168.50.1
                dns-server 192.168.50.1
                exit
                enable secret class
                line console 0
                password cisco
                login
                exit
                line vty 0 4
                password remote
                login
                exit
                banner motd #Authorised staff only#
                service password-encryption
                no ip domain-lookup
                end
                copy running-config startup-config",
            )?;
            for (i, (name, model)) in [("PC1", "PC"), ("PC2", "PC"), ("Laptop1", "Laptop")].into_iter().enumerate() {
                b.add_model(model, name, 160.0 + 200.0 * i as f32, 340.0)?;
                b.cable(name, "fa0", "S1", &format!("fa0/{}", i + 1))?;
                b.type_in(name, "ipconfig /renew")?;
            }
            b.t.add_drawing(packetcraft_model::Shape::Rectangle, 120.0, 240.0, 760.0, 210.0, Some("#e3eefa"))?;
            b.t.add_annotation(130.0, 250.0, "Office LAN - addresses from DHCP (192.168.50.21 onwards)")?;
            b.t.notes = "PCs get their addresses from R1 by DHCP (Desktop > IP Configuration > DHCP, or `ipconfig /renew`). R1's console asks for `cisco`; `enable` asks for `class`. Try `show ip dhcp binding` on R1.".into();
        }
        other => return Err(EngineError::BadParams(format!("unknown sample `{other}`"))),
    }
    Ok(b.t)
}
