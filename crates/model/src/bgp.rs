//! BGP-4 settings of a router (`router bgp N`): the process, its neighbours, the networks it
//! originates and what it redistributes. The sessions and the routes are worked out by the
//! simulator; nothing derived is kept here.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::Ipv4Net;
use serde::{Deserialize, Serialize};

/// Most neighbours under one BGP process.
pub const MAX_NEIGHBORS: usize = 64;
/// Most `network` statements under one BGP process.
pub const MAX_NETWORKS: usize = 256;
/// Longest neighbour description and update-source name kept.
pub const MAX_TEXT: usize = 80;
/// The local preference a path gets when nothing sets it.
pub const DEFAULT_LOCAL_PREF: u32 = 100;
/// The hold time and keepalive interval a session uses (seconds).
pub const HOLD_SECS: u16 = 180;
pub const KEEPALIVE_SECS: u16 = 60;

/// Where `redistribute` takes routes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Connected,
    Static,
    Ospf,
}

impl Source {
    pub fn word(self) -> &'static str {
        match self {
            Source::Connected => "connected",
            Source::Static => "static",
            Source::Ospf => "ospf",
        }
    }
}

/// One `neighbor A.B.C.D …` peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Neighbor {
    pub addr: Ipv4Addr,
    /// `remote-as N`; a neighbour without one stays idle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_as: Option<u32>,
    /// `update-source INTERFACE`: the interface whose address the session is sent from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_source: Option<String>,
    #[serde(default)]
    pub next_hop_self: bool,
    #[serde(default)]
    pub shutdown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `ebgp-multihop N`: how many routers away an external neighbour may be (1 when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ebgp_multihop: Option<u8>,
    /// `weight N`: given to every path learned from this neighbour (0 when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
    /// `prefix-list NAME in` / `out`: what the neighbour may send and what it is sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_list_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_list_out: Option<String>,
    /// `route-map NAME in` / `out`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_map_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_map_out: Option<String>,
}

impl Neighbor {
    pub fn new(addr: Ipv4Addr) -> Neighbor {
        Neighbor {
            addr,
            remote_as: None,
            update_source: None,
            next_hop_self: false,
            shutdown: false,
            description: None,
            ebgp_multihop: None,
            weight: None,
            prefix_list_in: None,
            prefix_list_out: None,
            route_map_in: None,
            route_map_out: None,
        }
    }
}

/// `router bgp N`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bgp {
    /// The router's autonomous system (16 or 32 bits).
    pub asn: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_id: Option<Ipv4Addr>,
    /// `bgp log-neighbor-changes`.
    #[serde(default)]
    pub log_neighbor_changes: bool,
    /// `bgp default local-preference N` (100 when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_local_pref: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub neighbors: Vec<Neighbor>,
    /// `network A.B.C.D mask M`: advertised while that exact prefix is in the routing table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub networks: Vec<Ipv4Net>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redistribute: Vec<Source>,
    /// `redistribute SOURCE route-map NAME`: the map, by the source's word (`connected`, …).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub redistribute_maps: BTreeMap<String, String>,
}

impl Bgp {
    pub fn new(asn: u32) -> Bgp {
        Bgp { asn, ..Bgp::default() }
    }

    pub fn neighbor(&self, addr: Ipv4Addr) -> Option<&Neighbor> {
        self.neighbors.iter().find(|n| n.addr == addr)
    }

    /// The neighbour for an address, added (unconfigured) when missing; `None` past
    /// [`MAX_NEIGHBORS`].
    pub fn neighbor_mut(&mut self, addr: Ipv4Addr) -> Option<&mut Neighbor> {
        if !self.neighbors.iter().any(|n| n.addr == addr) {
            if self.neighbors.len() >= MAX_NEIGHBORS {
                return None;
            }
            self.neighbors.push(Neighbor::new(addr));
            self.neighbors.sort_by_key(|n| n.addr);
        }
        self.neighbors.iter_mut().find(|n| n.addr == addr)
    }

    pub fn local_pref(&self) -> u32 {
        self.default_local_pref.unwrap_or(DEFAULT_LOCAL_PREF)
    }

    /// Cleans what a lab file holds: bounded lists, no duplicates, short text, sane values.
    pub fn sanitize(&mut self) {
        self.asn = self.asn.max(1);
        self.neighbors.sort_by_key(|n| n.addr);
        self.neighbors.dedup_by_key(|n| n.addr);
        self.neighbors.truncate(MAX_NEIGHBORS);
        for n in &mut self.neighbors {
            n.remote_as = n.remote_as.filter(|a| *a >= 1);
            n.ebgp_multihop = n.ebgp_multihop.filter(|m| *m >= 1);
            if let Some(d) = &mut n.description {
                *d = d.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
            }
            for name in [&mut n.prefix_list_in, &mut n.prefix_list_out, &mut n.route_map_in, &mut n.route_map_out] {
                *name = name.take().map(|x| crate::policy::clean_name(&x)).filter(|x| !x.is_empty());
            }
            if let Some(u) = &mut n.update_source {
                *u = u.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
            }
        }
        self.networks.iter_mut().for_each(|n| *n = n.network());
        self.networks.sort();
        self.networks.dedup();
        self.networks.truncate(MAX_NETWORKS);
        self.redistribute.sort();
        self.redistribute.dedup();
        let kept: Vec<String> = self.redistribute.iter().map(|r| r.word().to_string()).collect();
        self.redistribute_maps.retain(|k, _| kept.contains(k));
        for v in self.redistribute_maps.values_mut() {
            *v = crate::policy::clean_name(v);
        }
        self.redistribute_maps.retain(|_, v| !v.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_are_kept_sorted_and_bounded() {
        let mut b = Bgp::new(65_001);
        for i in (0..=u8::MAX).rev() {
            let _ = b.neighbor_mut(Ipv4Addr::new(10, 0, 0, i));
        }
        assert_eq!(b.neighbors.len(), MAX_NEIGHBORS);
        assert!(b.neighbors.windows(2).all(|w| w[0].addr < w[1].addr));
        assert!(b.neighbor_mut(Ipv4Addr::new(10, 9, 9, 9)).is_none());
    }

    #[test]
    fn sanitize_cleans_a_hostile_file() {
        let mut b = Bgp { asn: 0, ..Bgp::default() };
        let mut n = Neighbor::new(Ipv4Addr::new(10, 0, 0, 2));
        n.remote_as = Some(0);
        n.description = Some("x\ny".repeat(100));
        b.neighbors = vec![n.clone(), n];
        b.networks = vec!["10.1.2.3/16".parse().unwrap(); 3];
        b.redistribute = vec![Source::Static, Source::Static, Source::Connected];
        b.sanitize();
        assert_eq!(b.asn, 1);
        assert_eq!(b.neighbors.len(), 1);
        assert_eq!(b.neighbors[0].remote_as, None);
        assert!(b.neighbors[0].description.as_ref().is_some_and(|d| d.chars().count() <= MAX_TEXT && !d.contains('\n')));
        assert_eq!(b.networks, vec!["10.1.0.0/16".parse::<Ipv4Net>().unwrap()]);
        assert_eq!(b.redistribute, vec![Source::Connected, Source::Static]);
    }

    #[test]
    fn serde_round_trip_leaves_defaults_out() {
        let mut b = Bgp::new(65_001);
        b.neighbor_mut(Ipv4Addr::new(10, 0, 0, 2)).unwrap().remote_as = Some(65_002);
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("router_id") && !json.contains("update_source"), "{json}");
        assert_eq!(serde_json::from_str::<Bgp>(&json).unwrap(), b);
    }
}
