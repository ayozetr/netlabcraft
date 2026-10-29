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
    /// What the workspace prints under the icon.
    pub label: &'static str,
    pub description: &'static str,
    pub ports: &'static [PortRun],
}

impl Model {
    /// A multilayer switch: can route (`ip routing`) and have routed ports (`no switchport`).
    pub fn multilayer(&self) -> bool {
        self.id.starts_with("Switch-ML")
    }
}

const fn run(prefix: &'static str, first: u32, last: u32) -> PortRun {
    PortRun { prefix, first, last }
}

pub static MODELS: &[Model] = &[
    Model {
        id: "Router-2",
        kind: DeviceKind::Router,
        label: "Router-2",
        description: "Router with 2 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 1)],
    },
    Model {
        id: "Router-3",
        kind: DeviceKind::Router,
        label: "Router-3",
        description: "Router with 3 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 2)],
    },
    Model {
        id: "Router-4",
        kind: DeviceKind::Router,
        label: "Router-4",
        description: "Router with 4 Gigabit Ethernet ports",
        ports: &[run("GigabitEthernet0/", 0, 3)],
    },
    Model {
        id: "Switch-8",
        kind: DeviceKind::Switch,
        label: "Switch-8",
        description: "Switch with 8 Fast Ethernet + 1 Gigabit Ethernet ports",
        ports: &[run("FastEthernet0/", 1, 8), run("GigabitEthernet0/", 1, 1)],
    },
    Model {
        id: "Switch-24",
        kind: DeviceKind::Switch,
        label: "Switch-24",
        description: "Switch with 24 Fast Ethernet + 2 Gigabit Ethernet ports",
        ports: &[run("FastEthernet0/", 1, 24), run("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-ML-24",
        kind: DeviceKind::Switch,
        label: "Switch-ML-24",
        description: "Multilayer switch with 24 Fast Ethernet + 2 Gigabit Ethernet ports: switches, and routes with `ip routing`",
        ports: &[run("FastEthernet0/", 1, 24), run("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Switch-48",
        kind: DeviceKind::Switch,
        label: "Switch-48",
        description: "Switch with 48 Fast Ethernet + 2 Gigabit Ethernet ports",
        ports: &[run("FastEthernet0/", 1, 48), run("GigabitEthernet0/", 1, 2)],
    },
    Model {
        id: "Hub-8",
        kind: DeviceKind::Hub,
        label: "Hub-8",
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
];

pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id.eq_ignore_ascii_case(id))
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
}
