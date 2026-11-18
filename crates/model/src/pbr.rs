//! Policy-based routing: which route map decides where the packets of an interface (or the
//! device's own packets) go before the routing table is asked. The route map itself, with its
//! `match` and `set` lines, is an ordinary one (`netlabcraft_model::policy`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::policy::clean_name;

/// Most interfaces that can have a policy.
pub const MAX_POLICIES: usize = 512;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pbr {
    /// `ip policy route-map NAME` on an interface, by interface name.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub interfaces: BTreeMap<String, String>,
    /// `ip local policy route-map NAME`: for the packets this device sends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
}

impl Pbr {
    pub fn is_empty(&self) -> bool {
        *self == Pbr::default()
    }

    /// The route map that decides for packets arriving on `iface`.
    pub fn for_interface(&self, iface: &str) -> Option<&str> {
        self.interfaces.get(iface).map(String::as_str)
    }

    pub fn sanitize(&mut self) {
        self.interfaces = std::mem::take(&mut self.interfaces)
            .into_iter()
            .map(|(k, v)| (clean_name(&k), clean_name(&v)))
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .take(MAX_POLICIES)
            .collect();
        self.local = self.local.take().map(|n| clean_name(&n)).filter(|n| !n.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_drops_empty_names_and_caps_the_table() {
        let mut p = Pbr::default();
        for i in 0..2_000 {
            p.interfaces.insert(format!("Gi{i}"), "RM".into());
        }
        p.interfaces.insert(" ".into(), "RM".into());
        p.local = Some("  ".into());
        p.sanitize();
        assert!(p.interfaces.len() <= MAX_POLICIES && !p.interfaces.contains_key(""));
        assert_eq!(p.local, None);
        assert_eq!(p.for_interface("Gi1"), Some("RM"));
    }
}
