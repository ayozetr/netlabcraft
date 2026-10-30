//! The device catalogue: generic models (our own names), each a kind plus a port layout.

use crate::DeviceKind;

/// A run of ports: `prefix` followed by numbers `first..=last` (`FastEthernet0/` 1–24).
#[derive(Debug, Clone, Copy)]
pub struct PortRun {
    pub prefix: &'static str,
    pub first: u32,
    pub last: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Model {
    /// Stable id, saved in lab files (`Router-3`).
    pub id: &'static str,
    pub kind: DeviceKind,
    /// What the workspace prints under the icon: our own descriptive name (ports at a glance:
    /// `GE` Gigabit Ethernet, `FE` Fast Ethernet, `S` serial). docs/model-equivalents.md maps the
    /// models courses name to these.
    pub label: &'static str,
    pub description: &'static str,
    pub ports: &'static [PortRun],
}

impl Model {
    /// A multilayer switch: can route (`ip routing`) and have routed ports (`no switchport`).
    pub fn multilayer(&self) -> bool {
        self.id.starts_with("Switch-ML") || self.home_router()
    }

    /// A home wireless router: a multilayer switch with an Internet port, four LAN ports and a
    /// radio, set up at the factory to route, translate and hand out addresses.
    pub fn home_router(&self) -> bool {
        self.id == "Home-Router"
    }
}

const fn run(prefix: &'static str, first: u32, last: u32) -> PortRun {
    PortRun { prefix, first, last }
}

pub static MODELS: &[Model] = &[
    Model {
        id: "Router-2",
        kind: DeviceKind::Router,
        label: "Router 2GE",
        description: "Router with 2 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 1)],
    },
    Model {
        id: "Router-3",
        kind: DeviceKind::Router,
        label: "Router 3GE",
        description: "Router with 3 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 2)],
    },
    Model {
        id: "Router-4",
        kind: DeviceKind::Router,
        label: "Router 4GE",
        description: "Router with 4 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 3)],
    },
    Model {
        id: "Router-WAN",
        kind: DeviceKind::Router,
        label: "Router 2GE+2S",
        description: "Router with 2 Gigabit Ethernet ports and 2 serial WAN ports",
        ports: &[run("GigabitEthernet0/", 0, 1), run("Serial0/0/", 0, 1)],
    },
    Model {
        id: "Router-M2",
        kind: DeviceKind::Router,
        label: "Modular Router 2GE",
        description: "Modular router with 2 Gigabit Ethernet ports and 2 empty module bays (serial and Gigabit cards, fitted while powered off)",
        ports: &[run("GigabitEthernet0/", 0, 1)],
    },
    Model {
        id: "Router-M3",
        kind: DeviceKind::Router,
        label: "Modular Router 3GE",
        description: "Modular router with 3 Gigabit Ethernet ports and 4 empty module bays (serial and Gigabit cards, fitted while powered off)",
        ports: &[run("GigabitEthernet0/", 0, 2)],
    },
    Model {
        id: "Firewall-5",
        kind: DeviceKind::Router,
        label: "Firewall 5GE",
        description: "Stateful firewall with 5 Gigabit Ethernet ports: named interfaces with security levels, access rules, object NAT and a DHCP server",
        ports: &[run("GigabitEthernet1/", 1, 5)],
    },
    Model {
        id: "Switch-8",
        kind: DeviceKind::Switch,
        label: "Switch 8FE",
        description: "Switch with 8 Fast Ethernet + 1 Gigabit Ethernet ports",
        ports: &[run("FastEthernet0/", 1, 8), run("GigabitEthernet0/", 1, 1)],
    },
    Model {
        id: "Switch-24",
        kind: DeviceKind::Switch,
        label: "Switch 24FE",
        description: "Switch with 24 Fast Ethernet + 2 Gigabit Ethernet ports",
        ports: &[run("FastEthernet0/", 1, 24), run("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-ML-24",
        kind: DeviceKind::Switch,
        label: "L3 Switch 24",
        description: "Multilayer switch with 24 Fast Ethernet + 2 Gigabit Ethernet ports: switches, and routes with `ip routing`",
        ports: &[run("FastEthernet0/", 1, 24), run("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-48",
        kind: DeviceKind::Switch,
        label: "Switch 48FE",
        description: "Switch with 48 Fast Ethernet + 2 Gigabit Ethernet ports",
        ports: &[run("FastEthernet0/", 1, 48), run("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "AccessPoint",
        kind: DeviceKind::Switch,
        label: "Access Point",
        description: "Wi-Fi access point: bridges its radio and its Ethernet port (SSID, WPA2-PSK)",
        ports: &[run("GigabitEthernet", 0, 0), run("Wireless", 0, 0)],
    },
    Model {
        id: "WLC",
        kind: DeviceKind::Switch,
        label: "WLC",
        description: "Wireless LAN controller: holds the WLANs (SSID, WPA2-PSK or WPA2-Enterprise) and pushes them to the lightweight APs that join it",
        ports: &[run("GigabitEthernet0/", 1, 4)],
    },
    Model {
        id: "LAP",
        kind: DeviceKind::Switch,
        label: "Light AP",
        description: "Lightweight access point: joins a wireless LAN controller over its Ethernet port and offers the controller's WLANs",
        ports: &[run("GigabitEthernet", 0, 0), run("Wireless", 0, 0)],
    },
    Model {
        id: "Home-Router",
        kind: DeviceKind::Switch,
        label: "Home Router",
        description: "Home wireless router: Internet port, 4 LAN ports and Wi-Fi, with DHCP and NAT set up",
        ports: &[run("Internet", 0, 0), run("Ethernet", 1, 4), run("Wireless", 0, 0)],
    },
    Model {
        id: "Hub-8",
        kind: DeviceKind::Hub,
        label: "Hub 8",
        description: "Hub: repeats every frame out of its other 8 ports",
        ports: &[run("Ethernet", 0, 7)],
    },
    Model { id: "PC", kind: DeviceKind::Pc, label: "PC", description: "Desktop PC with one Fast Ethernet port", ports: &[run("FastEthernet", 0, 0)] },
    Model {
        id: "Laptop",
        kind: DeviceKind::Laptop,
        label: "Laptop",
        description: "Laptop with one Fast Ethernet port",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "Laptop-WiFi",
        kind: DeviceKind::Laptop,
        label: "Wi-Fi Laptop",
        description: "Laptop with a Wi-Fi card instead of an Ethernet port",
        ports: &[run("Wireless", 0, 0)],
    },
    Model {
        id: "Server",
        kind: DeviceKind::Server,
        label: "Server",
        description: "Server with one Gigabit Ethernet port",
        ports: &[run("GigabitEthernet", 0, 0)],
    },
    Model {
        id: "Printer",
        kind: DeviceKind::Printer,
        label: "Printer",
        description: "Network printer with one Fast Ethernet port",
        ports: &[run("FastEthernet", 0, 0)],
    },
    Model {
        id: "IP-Phone",
        kind: DeviceKind::Phone,
        label: "IP Phone",
        description: "IP phone with a built-in switch: FastEthernet0 goes to the network, FastEthernet1 to a computer behind it; takes its address and its call server by DHCP",
        ports: &[run("FastEthernet", 0, 1)],
    },
];

/// A plug-in card for a modular router's bay: its ports are named after the bay
/// (`Serial` in bay `0/1` gives `Serial0/1/0`, `Serial0/1/1`).
#[derive(Debug, Clone, Copy)]
pub struct Module {
    /// Stable id, saved in lab files (`Card-2S`).
    pub id: &'static str,
    /// What the interface shows: our own descriptive name.
    pub label: &'static str,
    pub description: &'static str,
    /// The port type word (`Serial`, `GigabitEthernet`).
    pub port_type: &'static str,
    pub port_count: u32,
}

/// The cards a modular router's bays take.
pub static MODULES: &[Module] = &[
    Module { id: "Card-2S", label: "2-port Serial card", description: "Two synchronous serial WAN ports", port_type: "Serial", port_count: 2 },
    Module { id: "Card-4S", label: "4-port Serial card", description: "Four synchronous serial WAN ports", port_type: "Serial", port_count: 4 },
    Module {
        id: "Card-1GE",
        label: "1-port Gigabit card",
        description: "One routed Gigabit Ethernet port",
        port_type: "GigabitEthernet",
        port_count: 1,
    },
    Module {
        id: "Card-2GE",
        label: "2-port Gigabit card",
        description: "Two routed Gigabit Ethernet ports",
        port_type: "GigabitEthernet",
        port_count: 2,
    },
    Module {
        id: "Card-1FE",
        label: "1-port Fast Ethernet card",
        description: "One routed Fast Ethernet port",
        port_type: "FastEthernet",
        port_count: 1,
    },
];

/// A card by its id or its shown name.
pub fn module(id: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.id.eq_ignore_ascii_case(id) || m.label.eq_ignore_ascii_case(id))
}

impl Module {
    /// The card's port names in bay `slot` (`0/1`).
    pub fn port_names(&self, slot: &str) -> Vec<String> {
        (0..self.port_count).map(|n| format!("{}{slot}/{n}", self.port_type)).collect()
    }
}

impl Model {
    /// The module bays of a modular router, in order (`0/0`, `0/1`…); empty for fixed models.
    pub fn slots(&self) -> &'static [&'static str] {
        match self.id {
            "Router-M2" => &["0/0", "0/1"],
            "Router-M3" => &["0/0", "0/1", "0/2", "0/3"],
            _ => &[],
        }
    }
}

/// A model by its id (`Router-3`, what lab files keep) or by the name the interface shows
/// (`Router 3GE`).
pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id.eq_ignore_ascii_case(id)).or_else(|| MODELS.iter().find(|m| m.label.eq_ignore_ascii_case(id)))
}

/// The model a kind gets when none is named.
pub fn default_for(kind: DeviceKind) -> &'static Model {
    let id = match kind {
        DeviceKind::Router => "Router-3",
        DeviceKind::Switch => "Switch-24",
        DeviceKind::Hub => "Hub-8",
        DeviceKind::Pc => "PC",
        DeviceKind::Laptop => "Laptop",
        DeviceKind::Server => "Server",
        DeviceKind::Printer => "Printer",
        DeviceKind::Phone => "IP-Phone",
    };
    MODELS.iter().find(|m| m.id == id).unwrap_or(&FALLBACK)
}

static FALLBACK: Model = Model { id: "PC", kind: DeviceKind::Pc, label: "PC", description: "", ports: &[run("FastEthernet", 0, 0)] };

impl Model {
    /// Port names, in order.
    pub fn port_names(&self) -> Vec<String> {
        // A single-port host is `FastEthernet0`, not `FastEthernet0/0`.
        self.ports.iter().flat_map(|r| (r.first..=r.last).map(move |n| format!("{}{n}", r.prefix))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_shown_names_are_unique_and_both_find_the_model() {
        let mut seen = std::collections::BTreeSet::new();
        for m in MODELS {
            assert!(seen.insert(m.id.to_lowercase()), "{}", m.id);
            if !m.label.eq_ignore_ascii_case(m.id) {
                assert!(seen.insert(m.label.to_lowercase()), "{}", m.label);
            }
            assert_eq!(find(m.id).map(|x| x.id), Some(m.id));
            assert_eq!(find(m.label).map(|x| x.id), Some(m.id));
        }
        assert_eq!(find("router 3ge").map(|m| m.id), Some("Router-3"));
    }

    #[test]
    fn every_kind_has_a_default_and_names_parse() {
        for k in DeviceKind::ALL {
            assert_eq!(default_for(k).kind, k);
        }
        for m in MODELS {
            let names = m.port_names();
            assert!(!names.is_empty(), "{}", m.id);
            for n in &names {
                assert!(crate::ifname::parse(n).is_some(), "{n}");
            }
        }
        assert_eq!(find("switch-48").map(|m| m.port_names().len()), Some(50));
        assert_eq!(find("Hub-8").map(|m| m.port_names()[7].clone()).as_deref(), Some("Ethernet7"));
    }

    #[test]
    fn modules_name_their_ports_after_the_bay() {
        let m = find("Modular Router 3GE").unwrap();
        assert_eq!(m.slots().len(), 4);
        assert!(find("Router-3").unwrap().slots().is_empty());
        let mut seen = std::collections::BTreeSet::new();
        for c in MODULES {
            assert!(seen.insert(c.id) && module(c.label).is_some_and(|x| x.id == c.id));
            for slot in m.slots() {
                for n in c.port_names(slot) {
                    assert!(crate::ifname::parse(&n).is_some_and(|x| x.numbers.len() == 3), "{n}");
                }
            }
        }
        assert_eq!(module("card-2s").unwrap().port_names("0/1"), ["Serial0/1/0", "Serial0/1/1"]);
    }
}
