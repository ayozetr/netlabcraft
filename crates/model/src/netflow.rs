//! NetFlow settings. On a router: where flow records are exported (`ip flow-export destination`,
//! `ip flow-export version`) and which interfaces count the traffic that comes in on them
//! (`ip flow ingress`, `ip route-cache flow`). On an end device: the port its Netflow Collector app
//! listens on, when it is started. The flows themselves are dynamic state of the simulator.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// Most export destinations a router takes.
pub const MAX_DESTINATIONS: usize = 2;
/// Most interfaces a router counts flows on.
pub const MAX_INGRESS: usize = 64;
/// Longest interface name kept.
pub const MAX_NAME: usize = 40;
/// The export format when none is set.
pub const DEFAULT_VERSION: u8 = 5;

/// A collector a router exports to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub addr: Ipv4Addr,
    pub port: u16,
}

/// What a device is set to for NetFlow.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Netflow {
    /// Routers: `ip flow-export version` (5 or 9); 0 when not set (version 5).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub version: u8,
    /// Routers: `ip flow-export destination`, in the order typed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destinations: Vec<Destination>,
    /// Routers: the interfaces that count flows (`ip flow ingress`), as named in the configuration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ingress: Vec<String>,
    /// End devices: the UDP port the Netflow Collector listens on; `None` when it is stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collector_port: Option<u16>,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl Netflow {
    pub fn is_default(&self) -> bool {
        *self == Netflow::default()
    }

    /// The export format in use: 5 unless 9 was set.
    pub fn export_version(&self) -> u8 {
        if self.version == 9 { 9 } else { DEFAULT_VERSION }
    }

    /// Whether the router counts flows that come in on `name`.
    pub fn counts(&self, name: &str) -> bool {
        self.ingress.iter().any(|n| n == name)
    }

    /// Router configuration back to the factory state; a collector keeps its port.
    pub fn reset_router(&mut self) {
        self.version = 0;
        self.destinations.clear();
        self.ingress.clear();
    }

    /// Cuts everything to its limits (hostile files).
    pub fn sanitize(&mut self) {
        if !matches!(self.version, 5 | 9) {
            self.version = 0;
        }
        self.destinations.retain(|d| d.port != 0);
        self.destinations.truncate(MAX_DESTINATIONS);
        self.ingress.retain(|n| !n.is_empty() && n.chars().count() <= MAX_NAME);
        self.ingress.truncate(MAX_INGRESS);
        if self.collector_port == Some(0) {
            self.collector_port = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_left_out_and_hostile_ones_are_cut() {
        assert!(Netflow::default().is_default());
        assert_eq!(Netflow::default().export_version(), 5);
        let mut n = Netflow {
            version: 7,
            destinations: (0..5).map(|i| Destination { addr: Ipv4Addr::new(10, 0, 0, i + 1), port: if i == 1 { 0 } else { 2055 } }).collect(),
            ingress: vec!["Gi0/0".into(), String::new(), "x".repeat(500)],
            collector_port: Some(0),
        };
        n.sanitize();
        assert_eq!(n.version, 0);
        assert_eq!(n.destinations.len(), MAX_DESTINATIONS);
        assert_eq!(n.ingress, vec!["Gi0/0".to_string()]);
        assert_eq!(n.collector_port, None);
        assert!(n.counts("Gi0/0") && !n.counts("Gi0/1"));
        n.version = 9;
        assert_eq!(n.export_version(), 9);
        n.reset_router();
        assert!(n.is_default());
    }
}
