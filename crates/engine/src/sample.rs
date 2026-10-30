//! Sample labs, built with the same commands a person would type. Original content.

use std::net::Ipv4Addr;

use netlabcraft_model::{DeviceKind, End, Topology};
use netlabcraft_sim::Sim;

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
        id: "ipv6",
        title: "IPv6 with two routers",
        description: "PC1 and PC2 take their IPv6 addresses from R1 (autoconfiguration); PC3 is set by hand behind R2; static routes join the two.",
    },
    Sample {
        id: "multilayer",
        title: "Inter-VLAN routing on a multilayer switch",
        description: "A multilayer switch routes between VLAN 10 and 20 with its VLAN interfaces, and reaches a router through a routed port.",
    },
    Sample {
        id: "rip",
        title: "RIP version 2 on three routers",
        description: "Three routers in a line learn each other's networks with RIPv2; R3 hands out a default route.",
    },
    Sample {
        id: "serial-wan",
        title: "Two sites over a serial WAN link",
        description: "Two routers joined by a serial cable with PPP; R1 holds the DCE end and sets the clock rate. OSPF carries the routes.",
    },
    Sample {
        id: "activity-first-router",
        title: "Activity: your first router",
        description: "A graded activity: address the router's two interfaces, set its name and a secret, and make the two LANs reach each other. Check Results shows the score.",
    },
    Sample {
        id: "hsrp",
        title: "HSRP: a gateway that survives a router",
        description: "Two routers share the virtual gateway 192.168.1.254; R1 is active. Shut R1 down and PC1 keeps reaching the server through R2.",
    },
    Sample {
        id: "wifi",
        title: "Wi-Fi: laptops on an access point",
        description: "An access point offers the network Aula with WPA2; two Wi-Fi laptops join it and get addresses from the router's DHCP pool.",
    },
    Sample {
        id: "wlc",
        title: "Wi-Fi with a controller: lightweight APs and WPA2-Enterprise",
        description: "Two lightweight access points join the controller WLC1, which offers Staff (WPA2-Enterprise, logins checked by a RADIUS server) and Guest (WPA2-PSK).",
    },
    Sample {
        id: "firewall",
        title: "A firewall with inside, outside and DMZ",
        description: "FW1 lets the office out (sharing its outside address), keeps the Internet out of the office, and publishes the DMZ web server through a fixed address and one access rule.",
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
        description: "PCs get their addresses from the server, resolve www.netlabcraft.lab with its DNS service and open its web pages.",
    },
    Sample {
        id: "dhcp",
        title: "DHCP and basic device security",
        description: "A router hands out addresses with DHCP; console, vty and enable are password protected (console: cisco, enable: class).",
    },
    Sample {
        id: "ip-phones",
        title: "IP phones with a voice VLAN",
        description: "Two IP phones, each with a PC plugged into it, share switch ports: the phones talk in voice VLAN 100, the PCs in data VLAN 10. R1 hands out addresses and registers the phones; dial 1002 from Phone1.",
    },
    Sample {
        id: "iot",
        title: "IoT: a sensor, a fan and a rule",
        description: "A smart sensor and a smart fan register with the IoT server over HTTP; the server's rule switches the fan on when the sensor reads above 30.",
    },
    Sample {
        id: "modular",
        title: "Modular routers: serial cards for a WAN link",
        description: "Two modular routers got a 2-port serial card each (fitted while switched off) and are joined over Serial0/1/0; their configurations are saved, so a power cycle keeps them.",
    },
    Sample {
        id: "switch-card",
        title: "A switch card in a router",
        description: "R1 has a 4-port switch card: its ports are VLAN members (10 and 20) and a trunk to S1, and the router routes between its VLAN interfaces.",
    },
];

struct Builder {
    t: Topology,
    sim: Sim,
}

impl Builder {
    fn add_model(&mut self, model: &str, name: &str, x: f32, y: f32) -> Result<netlabcraft_core::DeviceId> {
        let m = netlabcraft_model::models::find(model).ok_or_else(|| EngineError::Failed(format!("no model {model}")))?;
        let id = self.t.add_model(m, Some(name), x * 1.4, y * 1.15)?;
        crate::home::factory(&mut self.t, id);
        Ok(id)
    }

    /// Fits a card in a modular router's bay (switched off and back on, as a person would).
    fn fit_card(&mut self, dev: &str, slot: &str, card: &str) -> Result<()> {
        let d = self.t.resolve(dev)?;
        if let Some(x) = self.t.device_mut(d) {
            x.power_off();
        }
        self.t.set_module(d, slot, Some(card))?;
        if let Some(x) = self.t.device_mut(d) {
            x.powered_off = false;
        }
        Ok(())
    }

    /// Places a device; sample coordinates are on a compact grid, spread out here.
    fn add(&mut self, kind: DeviceKind, name: &str, x: f32, y: f32) -> Result<netlabcraft_core::DeviceId> {
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
        let mut cli = netlabcraft_ios::Cli::default();
        for l in lines.lines().map(str::trim).filter(|l| !l.is_empty()) {
            // Samples start converged: no port spends its 30 s listening and learning.
            self.sim.topology_changed(&self.t);
            let vlans: std::collections::BTreeSet<netlabcraft_core::VlanId> = self.t.devices.iter().flat_map(|d| d.vlans.keys().copied()).collect();
            for v in vlans {
                let _ = self.sim.spanning_tree(&self.t, v);
            }
            self.sim.converge_stp();
            let o = netlabcraft_ios::execute(&mut self.t, &mut self.sim, d, &mut cli, l);
            if o.output.starts_with("% ") || o.output.contains("\n% ") || o.output.starts_with("DHCP failed") {
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
        "ipv6" => {
            b.add_model("Router-3", "R1", 240.0, 80.0)?;
            b.add_model("Router-3", "R2", 520.0, 80.0)?;
            b.add(DeviceKind::Switch, "S1", 240.0, 240.0)?;
            b.cable("R1", "g0/0", "S1", "g0/1")?;
            b.cable("R1", "g0/1", "R2", "g0/1")?;
            b.type_in(
                "R1",
                "enable\nconfigure terminal\nipv6 unicast-routing\ninterface g0/0\nipv6 address 2001:db8:1::1/64\nipv6 address fe80::1 link-local\nno shutdown\ninterface g0/1\nipv6 address 2001:db8:12::1/64\nno shutdown\nexit\nipv6 route 2001:db8:2::/64 2001:db8:12::2\nend",
            )?;
            b.type_in(
                "R2",
                "enable\nconfigure terminal\nipv6 unicast-routing\ninterface g0/0\nipv6 address 2001:db8:2::1/64\nipv6 address fe80::2 link-local\nno shutdown\ninterface g0/1\nipv6 address 2001:db8:12::2/64\nno shutdown\nexit\nipv6 route 2001:db8:1::/64 2001:db8:12::1\nend",
            )?;
            for (pc, x, port) in [("PC1", 140.0, "fa0/1"), ("PC2", 340.0, "fa0/2")] {
                b.add(DeviceKind::Pc, pc, x, 400.0)?;
                b.cable(pc, "fa0", "S1", port)?;
                b.type_in(pc, "ipv6config auto")?;
            }
            b.add(DeviceKind::Pc, "PC3", 520.0, 300.0)?;
            b.cable("PC3", "fa0", "R2", "g0/0")?;
            b.type_in("PC3", "ipv6config 2001:db8:2::10/64 fe80::2")?;
            b.t.notes = "PC1 and PC2 autoconfigure from R1's router advertisements (ipv6config). Try `ping 2001:db8:2::10` from PC1, `show ipv6 route` and `show ipv6 neighbors` on the routers, and watch NDP in Simulation mode.".into();
        }
        "multilayer" => {
            b.add_model("Switch-ML-24", "MLS1", 360.0, 190.0)?;
            b.add_model("Router-2", "R1", 360.0, 45.0)?;
            b.add(DeviceKind::Server, "SRV1", 620.0, 45.0)?;
            b.cable("MLS1", "g0/1", "R1", "g0/0")?;
            b.cable("SRV1", "g0", "R1", "g0/1")?;
            for (pc, port, x, n) in
                [("PC1", "fa0/1", 60.0, 10), ("PC2", "fa0/2", 260.0, 10), ("PC3", "fa0/3", 460.0, 20), ("PC4", "fa0/4", 660.0, 20)]
            {
                b.add(DeviceKind::Pc, pc, x, 340.0)?;
                b.cable(pc, "fa0", "MLS1", port)?;
                let host = if pc == "PC1" || pc == "PC3" { 10 } else { 11 };
                b.type_in(pc, &format!("ipconfig 192.168.{n}.{host} 255.255.255.0 192.168.{n}.1"))?;
            }
            b.type_in(
                "MLS1",
                "enable\nconfigure terminal\nip routing\nvlan 10\nname Alumnos\nvlan 20\nname Profesores\nexit\ninterface range fa0/1 - 2\nswitchport access vlan 10\ninterface range fa0/3 - 4\nswitchport access vlan 20\nexit\ninterface vlan 10\nip address 192.168.10.1 255.255.255.0\nno shutdown\ninterface vlan 20\nip address 192.168.20.1 255.255.255.0\nno shutdown\ninterface g0/1\nno switchport\nip address 10.0.0.1 255.255.255.252\nexit\nip route 0.0.0.0 0.0.0.0 10.0.0.2\nend",
            )?;
            b.type_in(
                "R1",
                "enable\nconfigure terminal\ninterface g0/0\nip address 10.0.0.2 255.255.255.252\nno shutdown\ninterface g0/1\nip address 172.16.0.1 255.255.255.0\nno shutdown\nexit\nip route 192.168.0.0 255.255.0.0 10.0.0.1\nend",
            )?;
            b.type_in("SRV1", "ipconfig 172.16.0.10 255.255.255.0 172.16.0.1")?;
            b.t.notes = "MLS1 routes between VLAN 10 and VLAN 20 (`ip routing`, interfaces Vlan10 and Vlan20) and sends everything else to R1 over a routed port (`no switchport`). Try `show ip route` on MLS1 and a ping from PC1 to PC4 or to SRV1.".into();
        }
        "rip" => {
            for (name, x) in [("R1", 120.0), ("R2", 360.0), ("R3", 600.0)] {
                b.add_model("Router-2", name, x, 80.0)?;
            }
            b.cable("R1", "g0/1", "R2", "g0/0")?;
            b.cable("R2", "g0/1", "R3", "g0/0")?;
            b.add(DeviceKind::Pc, "PC1", 120.0, 300.0)?;
            b.add(DeviceKind::Server, "SRV1", 600.0, 300.0)?;
            b.cable("PC1", "fa0", "R1", "g0/0")?;
            b.cable("SRV1", "g0", "R3", "g0/1")?;
            b.type_in("PC1", "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1")?;
            b.type_in("SRV1", "ipconfig 192.168.3.10 255.255.255.0 192.168.3.1")?;
            let routers = [
                ("R1", "192.168.1.1 255.255.255.0", "10.0.12.1 255.255.255.252"),
                ("R2", "10.0.12.2 255.255.255.252", "10.0.23.1 255.255.255.252"),
                ("R3", "10.0.23.2 255.255.255.252", "192.168.3.1 255.255.255.0"),
            ];
            for (r, a0, a1) in routers {
                let nets: Vec<String> = [a0, a1]
                    .iter()
                    .filter_map(|a| a.split_whitespace().next())
                    .map(|ip| netlabcraft_model::Rip::classful(ip.parse().unwrap_or(std::net::Ipv4Addr::UNSPECIFIED)).addr.to_string())
                    .collect();
                let mut lines = format!(
                    "enable\nconfigure terminal\ninterface g0/0\nip address {a0}\nno shutdown\ninterface g0/1\nip address {a1}\nno shutdown\nexit\nrouter rip\nversion 2\nno auto-summary\n"
                );
                for n in nets {
                    lines.push_str(&format!("network {n}\n"));
                }
                lines.push_str("end");
                b.type_in(r, &lines)?;
            }
            b.t.notes = "Every router runs RIPv2. Compare `show ip route` (R routes, hop counts in `show ip rip database`) on R1 and R3, then shut R2's g0/1 and watch the routes go.".into();
        }
        "serial-wan" => {
            b.add_model("Router-WAN", "R1", 160.0, 100.0)?;
            b.add_model("Router-WAN", "R2", 560.0, 100.0)?;
            // Cabled from R1 first: R1's end is the DCE.
            b.cable("R1", "s0/0/0", "R2", "s0/0/0")?;
            b.add(DeviceKind::Pc, "PC1", 160.0, 320.0)?;
            b.add(DeviceKind::Pc, "PC2", 560.0, 320.0)?;
            b.cable("PC1", "fa0", "R1", "g0/0")?;
            b.cable("PC2", "fa0", "R2", "g0/0")?;
            b.type_in("PC1", "ipconfig 192.168.10.10 255.255.255.0 192.168.10.1")?;
            b.type_in("PC2", "ipconfig 192.168.20.10 255.255.255.0 192.168.20.1")?;
            for (r, lan, wan, clock) in [("R1", "192.168.10.1", "10.0.0.1", "clock rate 64000\n"), ("R2", "192.168.20.1", "10.0.0.2", "")] {
                b.type_in(
                    r,
                    &format!(
                        "enable\nconfigure terminal\ninterface g0/0\nip address {lan} 255.255.255.0\nno shutdown\ninterface s0/0/0\nip address {wan} 255.255.255.252\nencapsulation ppp\n{clock}no shutdown\nexit\nrouter ospf 1\nnetwork 0.0.0.0 255.255.255.255 area 0\nend"
                    ),
                )?;
            }
            b.t.notes = "The red cable is serial; the clock marks R1's DCE end (`show controllers s0/0/0`). Remove the clock rate on R1, or change one end to `encapsulation hdlc`, and the link goes down.".into();
        }
        "activity-first-router" => {
            b.add(DeviceKind::Router, "Router0", 360.0, 80.0)?;
            b.add(DeviceKind::Switch, "S1", 160.0, 220.0)?;
            b.add(DeviceKind::Switch, "S2", 560.0, 220.0)?;
            b.cable("Router0", "g0/0", "S1", "g0/1")?;
            b.cable("Router0", "g0/1", "S2", "g0/1")?;
            for (pc, sw, x, net) in [("PC1", "S1", 160.0, 10), ("PC2", "S2", 560.0, 20)] {
                b.add(DeviceKind::Pc, pc, x, 380.0)?;
                b.cable(pc, "fa0", sw, "fa0/1")?;
                b.type_in(pc, &format!("ipconfig 192.168.{net}.10 255.255.255.0 192.168.{net}.1"))?;
            }
            // The answer: configured, captured, then wiped back to the starting state.
            let router = b.t.resolve("Router0").map_err(|e| EngineError::Failed(e.to_string()))?;
            b.type_in(
                "Router0",
                "enable\nconfigure terminal\nhostname R1\nenable secret class\ninterface g0/0\nip address 192.168.10.1 255.255.255.0\nno shutdown\ninterface g0/1\nip address 192.168.20.1 255.255.255.0\nno shutdown\nend",
            )?;
            let mut activity = netlabcraft_model::activity::Activity {
                title: "Your first router".into(),
                instructions: "R1 joins two LANs.\n\n1. Name the router R1.\n2. Protect privileged mode with the secret class.\n3. Give GigabitEthernet0/0 the address 192.168.10.1/24 and GigabitEthernet0/1 192.168.20.1/24, and turn both on.\n4. Check that PC1 reaches PC2.\n\nPress Check Results at any time.".into(),
                hidden: true,
                salt: "first-router".into(),
                ..Default::default()
            };
            activity.checks = crate::activity::capture(&b.t, true, &activity.salt).into_iter().filter(|c| c.kind.device() == router).collect();
            let pc1 = b.t.resolve("PC1").map_err(|e| EngineError::Failed(e.to_string()))?;
            activity.checks.push(netlabcraft_model::activity::Check {
                label: "PC1 reaches PC2".into(),
                points: 3,
                section: "PC1".into(),
                feedback: "Both router interfaces need their address and `no shutdown`.".into(),
                kind: netlabcraft_model::activity::CheckKind::Ping { from: pc1, to: "192.168.20.10".into(), succeed: true },
            });
            if let Some(d) = b.t.device_mut(router) {
                d.reset_config();
                d.name = "Router0".into();
            }
            b.t.activity = Some(activity);
            b.t.notes = "A graded activity: follow the Instructions window and press Check Results.".into();
        }
        "hsrp" => {
            b.add_model("Router-2", "R1", 240.0, 200.0)?;
            b.add_model("Router-2", "R2", 480.0, 200.0)?;
            b.add(DeviceKind::Switch, "S1", 360.0, 340.0)?;
            b.add(DeviceKind::Switch, "S2", 360.0, 60.0)?;
            b.cable("R1", "g0/0", "S1", "fa0/2")?;
            b.cable("R2", "g0/0", "S1", "fa0/3")?;
            b.cable("R1", "g0/1", "S2", "fa0/2")?;
            b.cable("R2", "g0/1", "S2", "fa0/3")?;
            b.add(DeviceKind::Pc, "PC1", 360.0, 480.0)?;
            b.add(DeviceKind::Server, "SRV1", 620.0, 60.0)?;
            b.cable("PC1", "fa0", "S1", "fa0/1")?;
            b.cable("SRV1", "g0", "S2", "fa0/1")?;
            b.type_in("PC1", "ipconfig 192.168.1.10 255.255.255.0 192.168.1.254")?;
            b.type_in("SRV1", "ipconfig 10.0.0.10 255.255.255.0 10.0.0.254")?;
            for (r, lan, wan, pri) in [("R1", "192.168.1.1", "10.0.0.1", 110), ("R2", "192.168.1.2", "10.0.0.2", 100)] {
                b.type_in(
                    r,
                    &format!(
                        "enable\nconfigure terminal\ninterface g0/0\nip address {lan} 255.255.255.0\nstandby 1 ip 192.168.1.254\nstandby 1 priority {pri}\nstandby 1 preempt\nno shutdown\ninterface g0/1\nip address {wan} 255.255.255.0\nstandby 2 ip 10.0.0.254\nstandby 2 priority {pri}\nstandby 2 preempt\nno shutdown\nend"
                    ),
                )?;
            }
            b.t.notes = "R1 is active for both virtual gateways (`show standby`). Watch the HSRP hellos in Simulation mode, then shut R1's interfaces and ping from PC1 again.".into();
        }
        "wifi" => {
            b.add_model("Router-2", "R1", 360.0, 20.0)?;
            b.add(DeviceKind::Switch, "S1", 360.0, 140.0)?;
            b.add_model("AccessPoint", "AP1", 560.0, 200.0)?;
            b.add(DeviceKind::Pc, "PC1", 160.0, 240.0)?;
            b.cable("R1", "g0/0", "S1", "g0/1")?;
            b.cable("AP1", "g0", "S1", "fa0/2")?;
            b.cable("PC1", "fa0", "S1", "fa0/1")?;
            b.type_in(
                "R1",
                "enable\nconfigure terminal\ninterface g0/0\nip address 192.168.10.1 255.255.255.0\nno shutdown\nexit\nip dhcp excluded-address 192.168.10.1 192.168.10.9\nip dhcp pool AULA\nnetwork 192.168.10.0 255.255.255.0\ndefault-router 192.168.10.1\nend",
            )?;
            b.type_in("PC1", "ipconfig 192.168.10.5 255.255.255.0 192.168.10.1")?;
            let wifi = netlabcraft_model::Wifi { ssid: "Aula".into(), key: Some("clase1234".into()), channel: 6, login: None };
            let ap = b.t.resolve("AP1").map_err(|e| EngineError::Failed(e.to_string()))?;
            if let Some(p) = b.t.device_mut(ap).and_then(|d| d.ports.get_mut(1)) {
                p.wifi = Some(wifi.clone());
            }
            for (name, x) in [("Laptop1", 460.0), ("Laptop2", 680.0)] {
                b.add_model("Laptop-WiFi", name, x, 320.0)?;
                let id = b.t.resolve(name).map_err(|e| EngineError::Failed(e.to_string()))?;
                if let Some(p) = b.t.device_mut(id).and_then(|d| d.ports.get_mut(0)) {
                    p.wifi = Some(wifi.clone());
                }
                b.type_in(name, "ipconfig /renew")?;
            }
            b.t.notes = "AP1 offers Aula (WPA2, passphrase clase1234). Open a laptop's Config › Wireless0 to change its network, or move it far from AP1 and watch it drop.".into();
        }
        "wlc" => {
            b.add_model("Router-2", "R1", 360.0, 30.0)?;
            b.add_model("Switch-24", "S1", 360.0, 150.0)?;
            b.add_model("WLC", "WLC1", 120.0, 90.0)?;
            b.add_model("Server", "RADIUS1", 600.0, 90.0)?;
            b.add_model("LAP", "LAP1", 200.0, 260.0)?;
            b.add_model("LAP", "LAP2", 560.0, 260.0)?;
            b.cable("R1", "g0/0", "S1", "g0/1")?;
            b.cable("WLC1", "g0/1", "S1", "fa0/1")?;
            b.cable("LAP1", "g0", "S1", "fa0/2")?;
            b.cable("LAP2", "g0", "S1", "fa0/3")?;
            b.cable("RADIUS1", "g0", "S1", "fa0/4")?;
            // Management in VLAN 1 (untagged); Staff hosts land in VLAN 10, Guest hosts in VLAN 30,
            // each with its own subnet and DHCP pool on R1 (router on a stick).
            b.type_in(
                "R1",
                "enable\nconfigure terminal\ninterface g0/0\nip address 192.168.20.1 255.255.255.0\nno shutdown\ninterface g0/0.10\nencapsulation dot1Q 10\nip address 192.168.10.1 255.255.255.0\ninterface g0/0.30\nencapsulation dot1Q 30\nip address 192.168.30.1 255.255.255.0\nexit\nip dhcp excluded-address 192.168.10.1 192.168.10.9\nip dhcp excluded-address 192.168.30.1 192.168.30.9\nip dhcp pool STAFF\nnetwork 192.168.10.0 255.255.255.0\ndefault-router 192.168.10.1\nip dhcp pool GUEST\nnetwork 192.168.30.0 255.255.255.0\ndefault-router 192.168.30.1\nend",
            )?;
            b.type_in(
                "S1",
                "enable\nconfigure terminal\nvlan 10\nname Staff\nvlan 30\nname Guest\nexit\ninterface g0/1\nswitchport mode trunk\ninterface range fa0/2 - 3\nswitchport mode trunk\nend",
            )?;
            b.type_in("RADIUS1", "ipconfig 192.168.20.5 255.255.255.0 192.168.20.1")?;
            let server = b.t.resolve("RADIUS1").map_err(|e| EngineError::Failed(e.to_string()))?;
            if let Some(d) = b.t.device_mut(server) {
                use netlabcraft_model::services::{AaaClient, AaaUser};
                d.services.aaa.on = true;
                d.services.aaa.clients.push(AaaClient {
                    name: "WLC1".into(),
                    address: "192.168.20.2".parse().map_err(|_| EngineError::Failed("address".into()))?,
                    secret: "wlcsecret".into(),
                });
                for (u, pw) in [("ana", "ana12345"), ("luis", "luis1234")] {
                    d.services.aaa.users.push(AaaUser { name: u.into(), password: pw.into() });
                }
            }
            b.type_in(
                "WLC1",
                "enable\nconfigure terminal\ninterface vlan 1\nip address 192.168.20.2 255.255.255.0\nno shutdown\nexit\nip default-gateway 192.168.20.1\nradius-server host 192.168.20.5 key wlcsecret\nwlan 1 Staff enterprise vlan 10\nwlan 2 Guest psk invitado123 vlan 30\nend",
            )?;
            for (lap, a) in [("LAP1", 3), ("LAP2", 4)] {
                b.type_in(
                    lap,
                    &format!("enable\nconfigure terminal\ninterface vlan 1\nip address 192.168.20.{a} 255.255.255.0\nno shutdown\nexit\nip default-gateway 192.168.20.1\ninterface g0\nswitchport mode trunk\nend"),
                )?;
            }
            let staff = netlabcraft_model::Wifi {
                ssid: "Staff".into(),
                key: None,
                channel: 6,
                login: Some(netlabcraft_model::wlan::WifiLogin { user: "ana".into(), password: "ana12345".into() }),
            };
            let guest = netlabcraft_model::Wifi { ssid: "Guest".into(), key: Some("invitado123".into()), channel: 6, login: None };
            for (name, x, wifi) in [("Laptop1", 160.0, staff), ("Laptop2", 600.0, guest)] {
                b.add_model("Laptop-WiFi", name, x, 400.0)?;
                let id = b.t.resolve(name).map_err(|e| EngineError::Failed(e.to_string()))?;
                if let Some(p) = b.t.device_mut(id).and_then(|d| d.ports.get_mut(0)) {
                    p.wifi = Some(wifi);
                }
                b.type_in(name, "ipconfig /renew")?;
            }
            b.t.notes = "WLC1 holds two WLANs (`show wlan`, `show capwap`): Staff checks logins with RADIUS1 (users ana/ana12345 and luis/luis1234) and lands in VLAN 10 (192.168.10.0/24); Guest uses the passphrase invitado123 and lands in VLAN 30 (192.168.30.0/24). The access points tag each WLAN on their trunk to S1, and R1 routes and hands out addresses per VLAN. In Simulation mode, reset and ping from Laptop1 to watch the 802.11 association, the EAP login, the RADIUS exchange, the 4-way handshake and then CCMP-protected data; move Laptop1 next to LAP2 and watch it reassociate, keeping its address.".into();
        }
        "firewall" => {
            b.add_model("Firewall-5", "FW1", 360.0, 120.0)?;
            b.add_model("Router-2", "ISP", 640.0, 120.0)?;
            b.add_model("Switch-24", "S1", 360.0, 280.0)?;
            b.add(DeviceKind::Server, "Web", 120.0, 120.0)?;
            b.add(DeviceKind::Pc, "Home", 880.0, 120.0)?;
            b.cable("FW1", "g1/1", "S1", "g0/1")?;
            b.cable("FW1", "g1/2", "ISP", "g0/0")?;
            b.cable("FW1", "g1/3", "Web", "g0")?;
            b.cable("Home", "fa0", "ISP", "g0/1")?;
            b.type_in(
                "FW1",
                "enable
                configure terminal
                interface g1/1
                nameif inside
                security-level 100
                ip address 192.168.10.1 255.255.255.0
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
                object network OFFICE
                subnet 192.168.10.0 255.255.255.0
                nat (inside,outside) dynamic interface
                object network DMZ_NET
                subnet 172.16.1.0 255.255.255.0
                nat (dmz,outside) dynamic interface
                object network WEB
                host 172.16.1.10
                nat (dmz,outside) static 203.0.113.10
                exit
                object-group service WEB_PORTS tcp
                port-object eq www
                port-object eq https
                exit
                access-list OUTSIDE_IN remark Publish the web server, and let it be pinged
                access-list OUTSIDE_IN extended permit tcp any object WEB object-group WEB_PORTS
                access-list OUTSIDE_IN extended permit icmp any object WEB
                access-group OUTSIDE_IN in interface outside
                icmp deny any outside
                class-map inspection_default
                match default-inspection-traffic
                policy-map global_policy
                class inspection_default
                inspect icmp
                exit
                exit
                service-policy global_policy global
                dhcpd address 192.168.10.100-192.168.10.150 inside
                dhcpd dns 172.16.1.10
                dhcpd enable inside
                end",
            )?;
            b.type_in(
                "ISP",
                "enable
                configure terminal
                interface g0/0
                ip address 203.0.113.1 255.255.255.0
                no shutdown
                interface g0/1
                ip address 198.51.100.1 255.255.255.0
                no shutdown
                end",
            )?;
            b.type_in("Web", "ipconfig 172.16.1.10 255.255.255.0 172.16.1.1")?;
            b.type_in("Home", "ipconfig 198.51.100.10 255.255.255.0 198.51.100.1")?;
            for (i, pc) in ["PC1", "PC2"].into_iter().enumerate() {
                b.add(DeviceKind::Pc, pc, 240.0 + 240.0 * i as f32, 440.0)?;
                b.cable(pc, "fa0", "S1", &format!("fa0/{}", i + 1))?;
                b.type_in(pc, "ipconfig /renew")?;
            }
            b.t.notes = "FW1 has three named interfaces: inside (level 100), dmz (50) and outside (0). The office and the DMZ reach the Internet through the outside address (`show xlate`, `show conn`); Home can't start anything towards them, except the web server, published as 203.0.113.10 by a static rule and one access rule (`show access-list` counts the hits). From Home, browse http://203.0.113.10 and ping PC1 to see the difference; `show logging` on FW1 tells why a packet was dropped. The global policy inspects ICMP, so pings from the office come back without a rule; FW1 is not a hop, so a traceroute from PC1 starts at the ISP router; and FW1 ignores pings to its outside address (`icmp deny any outside`).".into();
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
            use netlabcraft_model::services::{DnsRecord, ServerPool};
            b.add(DeviceKind::Switch, "S1", 360.0, 160.0)?;
            b.add(DeviceKind::Server, "Server0", 360.0, 20.0)?;
            b.cable("Server0", "g0", "S1", "g0/1")?;
            b.type_in("Server0", "ipconfig 192.168.100.2 255.255.255.0 192.168.100.1 192.168.100.2")?;
            let srv = b.t.resolve("Server0")?;
            if let Some(d) = b.t.device_mut(srv) {
                d.services.dns.on = true;
                d.services.dns.records.push(DnsRecord { name: "www.netlabcraft.lab".into(), address: Ipv4Addr::new(192, 168, 100, 2) });
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
            b.t.notes = "Open PC1 > Desktop > Web Browser and go to www.netlabcraft.lab. Edit the pages in Server0 > Services > HTTP. Try `ping www.netlabcraft.lab` and `nslookup www.netlabcraft.lab` in a Command Prompt.".into();
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
            // The LAN box starts under S1's name and reaches past the hosts' names; its label sits
            // at the bottom, clear of the switch.
            b.t.add_drawing(netlabcraft_model::Shape::Rectangle, 120.0, 265.0, 760.0, 240.0, Some("#e3eefa"))?;
            b.t.add_annotation(130.0, 472.0, "Office LAN - addresses from DHCP (192.168.50.21 onwards)")?;
            b.t.notes = "PCs get their addresses from R1 by DHCP (Desktop > IP Configuration > DHCP, or `ipconfig /renew`). R1's console asks for `cisco`; `enable` asks for `class`. Try `show ip dhcp binding` on R1.".into();
        }
        "modular" => {
            b.add_model("Router-M3", "HQ", 200.0, 80.0)?;
            b.add_model("Router-M2", "Branch", 520.0, 80.0)?;
            b.fit_card("HQ", "0/1", "Card-2S")?;
            b.fit_card("HQ", "0/0", "Card-1GE")?;
            b.fit_card("Branch", "0/1", "Card-2S")?;
            b.cable("HQ", "s0/1/0", "Branch", "s0/1/0")?;
            b.add(DeviceKind::Switch, "S1", 120.0, 220.0)?;
            b.add(DeviceKind::Switch, "S2", 600.0, 220.0)?;
            b.cable("HQ", "g0/0", "S1", "g0/1")?;
            b.cable("Branch", "g0/0", "S2", "g0/1")?;
            for (pc, sw, x, ip, gw) in [("PC1", "S1", 120.0, "10.1.1.10", "10.1.1.1"), ("PC2", "S2", 600.0, "10.2.2.10", "10.2.2.1")] {
                b.add(DeviceKind::Pc, pc, x, 360.0)?;
                b.cable(pc, "fa0", sw, "fa0/1")?;
                b.type_in(pc, &format!("ipconfig {ip} 255.255.255.0 {gw}"))?;
            }
            b.type_in(
                "HQ",
                "enable
                configure terminal
                interface g0/0
                ip address 10.1.1.1 255.255.255.0
                no shutdown
                interface s0/1/0
                ip address 172.16.0.1 255.255.255.252
                clock rate 64000
                no shutdown
                exit
                ip route 10.2.2.0 255.255.255.0 172.16.0.2
                end
                copy running-config startup-config",
            )?;
            b.type_in(
                "Branch",
                "enable
                configure terminal
                interface g0/0
                ip address 10.2.2.1 255.255.255.0
                no shutdown
                interface s0/1/0
                ip address 172.16.0.2 255.255.255.252
                no shutdown
                exit
                ip route 0.0.0.0 0.0.0.0 172.16.0.1
                end
                copy running-config startup-config",
            )?;
            b.t.add_annotation(300.0, 20.0, "WAN 172.16.0.0/30 over the serial cards")?;
            b.t.notes = "HQ has a 2-port serial card in bay 0/1 and a Gigabit card in bay 0/0; Branch a serial card in bay 0/1 (see each router's Physical tab). To change a card, switch the router off there first: the cable on a card that comes out is unplugged. Both configurations are saved, so switching a router off and on keeps them (try it: `show ip interface brief` before and after).".into();
        }
        "switch-card" => {
            b.add_model("Router-M2", "R1", 360.0, 60.0)?;
            b.fit_card("R1", "0/1", "Card-4SW")?;
            b.add(DeviceKind::Switch, "S1", 600.0, 220.0)?;
            b.cable("R1", "fa0/1/3", "S1", "g0/1")?;
            b.type_in(
                "R1",
                "enable
                configure terminal
                vlan 10
                name STAFF
                vlan 20
                name GUESTS
                exit
                interface fa0/1/0
                switchport access vlan 10
                interface fa0/1/1
                switchport access vlan 20
                interface fa0/1/3
                switchport mode trunk
                interface vlan 10
                ip address 192.168.10.1 255.255.255.0
                no shutdown
                interface vlan 20
                ip address 192.168.20.1 255.255.255.0
                no shutdown
                end
                copy running-config startup-config",
            )?;
            b.type_in(
                "S1",
                "enable
                configure terminal
                vlan 10
                name STAFF
                vlan 20
                name GUESTS
                exit
                interface fa0/1
                switchport access vlan 10
                interface fa0/2
                switchport access vlan 20
                interface g0/1
                switchport mode trunk
                end
                copy running-config startup-config",
            )?;
            for (pc, dev, port, x, y, ip, gw) in [
                ("PC1", "R1", "fa0/1/0", 120.0, 220.0, "192.168.10.10", "192.168.10.1"),
                ("PC2", "R1", "fa0/1/1", 300.0, 220.0, "192.168.20.10", "192.168.20.1"),
                ("PC3", "S1", "fa0/1", 520.0, 380.0, "192.168.10.11", "192.168.10.1"),
                ("PC4", "S1", "fa0/2", 680.0, 380.0, "192.168.20.11", "192.168.20.1"),
            ] {
                b.add(DeviceKind::Pc, pc, x, y)?;
                b.cable(pc, "fa0", dev, port)?;
                b.type_in(pc, &format!("ipconfig {ip} 255.255.255.0 {gw}"))?;
            }
            b.t.add_annotation(300.0, 0.0, "VLAN 10 STAFF 192.168.10.0/24 - VLAN 20 GUESTS 192.168.20.0/24")?;
            b.t.notes = "R1's 4-port switch card (bay 0/1, see its Physical tab) switches like a switch: `show vlan brief` and `show interfaces trunk` on R1. PC1 and PC3 share VLAN 10 across the trunk; PC1 reaches PC2 and PC4 through R1's VLAN interfaces (`show ip route`).".into();
        }
        "iot" => {
            b.add(DeviceKind::Switch, "S1", 360.0, 160.0)?;
            b.add(DeviceKind::Server, "IoT-Server", 360.0, 20.0)?;
            b.cable("IoT-Server", "g0", "S1", "g0/1")?;
            b.type_in("IoT-Server", "ipconfig 192.168.1.2 255.255.255.0")?;
            let srv = b.t.resolve("IoT-Server")?;
            if let Some(d) = b.t.device_mut(srv) {
                d.services.http.on = true;
                d.iot.service = true;
                d.iot.rules = vec![netlabcraft_model::iot::IotRule { sensor: "Sensor1".into(), above: 30, actuator: "Fan1".into() }];
            }
            for (i, (name, model, addr, x)) in
                [("Sensor1", "IoT-Sensor", 10, 140.0), ("Fan1", "IoT-Device", 11, 360.0), ("PC1", "PC", 20, 580.0)].into_iter().enumerate()
            {
                b.add_model(model, name, x, 300.0)?;
                b.cable(name, "fa0", "S1", &format!("fa0/{}", i + 1))?;
                b.type_in(name, &format!("ipconfig 192.168.1.{addr} 255.255.255.0"))?;
                let d = b.t.resolve(name)?;
                if let Some(dev) = b.t.device_mut(d)
                    && model != "PC"
                {
                    dev.iot.server = Some(Ipv4Addr::new(192, 168, 1, 2));
                    dev.iot.value = if model == "IoT-Sensor" { 25 } else { 0 };
                }
            }
            b.t.notes = "Sensor1 reads 25 degrees and Fan1 is off. Open Sensor1 > IoT and raise the reading above 30: the server's rule (IoT-Server > IoT) switches Fan1 on. Every report is an HTTP request to the server (see the events in Simulation mode).".into();
        }
        "ip-phones" => {
            b.add(DeviceKind::Router, "R1", 360.0, 40.0)?;
            b.add(DeviceKind::Switch, "S1", 360.0, 160.0)?;
            b.cable("R1", "g0/0", "S1", "g0/1")?;
            b.type_in(
                "S1",
                "enable
                configure terminal
                vlan 10
                name DATA
                vlan 100
                name VOICE
                exit
                interface range fa0/1 - 2
                switchport mode access
                switchport access vlan 10
                switchport voice vlan 100
                interface g0/1
                switchport mode trunk
                end",
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
                interface g0/0.100
                encapsulation dot1Q 100
                ip address 192.168.100.1 255.255.255.0
                exit
                ip dhcp excluded-address 192.168.10.1
                ip dhcp excluded-address 192.168.100.1
                ip dhcp pool DATA
                network 192.168.10.0 255.255.255.0
                default-router 192.168.10.1
                ip dhcp pool VOICE
                network 192.168.100.0 255.255.255.0
                default-router 192.168.100.1
                option 150 ip 192.168.100.1
                exit
                telephony-service
                max-ephones 4
                max-dn 4
                ip source-address 192.168.100.1 port 2000
                auto assign 1 to 4
                exit
                ephone-dn 1
                number 1001
                ephone-dn 2
                number 1002
                end",
            )?;
            for (i, x) in [(1, 160.0), (2, 560.0)] {
                let (phone, pc) = (format!("Phone{i}"), format!("PC{i}"));
                b.add_model("IP-Phone", &phone, x, 280.0)?;
                b.cable(&phone, "fa0", "S1", &format!("fa0/{i}"))?;
                b.add(DeviceKind::Pc, &pc, x, 400.0)?;
                b.cable(&pc, "fa0", &phone, "fa1")?;
                b.type_in(&phone, "ipconfig /renew")?;
                b.type_in(&pc, "ipconfig /renew")?;
            }
            b.t.add_annotation(560.0, 140.0, "Data VLAN 10 untagged, voice VLAN 100 tagged")?;
            b.t.notes = "Each phone takes an address in voice VLAN 100 and learns from DHCP option 150 where to register (R1); its PC, plugged into the phone, gets an address in data VLAN 10. Open Phone1 > Phone, dial 1002 and press Call; on Phone2 press Pick up. `show ephone` and `show ephone-dn` on R1 show the registrations and the call.".into();
        }
        other => return Err(EngineError::BadParams(format!("unknown sample `{other}`"))),
    }
    Ok(b.t)
}
