//! BGP-4 settings of a router (`router bgp N`): the process, its neighbours, the networks it
//! originates and what it redistributes. The sessions and the routes are worked out by the
//! simulator; nothing derived is kept here.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::Ipv4Net;
use serde::{Deserialize, Serialize};

/// Most neighbours under one BGP process.
pub const MAX_NEIGHBORS: usize = 64;
/// Most `network` statements under one BGP process.
pub const MAX_NETWORKS: usize = 256;
/// Longest `neighbor … password` kept.
pub const MAX_PASSWORD: usize = 25;
/// Longest neighbour description and update-source name kept.
pub const MAX_TEXT: usize = 80;
/// The local preference a path gets when nothing sets it.
pub const DEFAULT_LOCAL_PREF: u32 = 100;
/// The hold time and keepalive interval a session uses (seconds).
pub const HOLD_SECS: u16 = 180;
pub const KEEPALIVE_SECS: u16 = 60;
/// Most `aggregate-address` statements, and the most paths `maximum-paths` accepts.
pub const MAX_AGGREGATES: usize = 64;
pub const MAX_MULTIPATH: u8 = 32;
/// Most member autonomous systems listed in `bgp confederation peers`.
pub const MAX_CONFED_PEERS: usize = 32;
/// Most peer groups under one BGP process.
pub const MAX_PEER_GROUPS: usize = 16;
/// Most process-wide or per-neighbour settings of the kept-and-shown kind.
pub const MAX_OPTIONS: usize = 32;

/// `aggregate-address A.B.C.D M.M.M.M [summary-only] [as-set]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aggregate {
    pub net: Ipv4Net,
    /// Do not advertise the more specific prefixes the aggregate covers.
    #[serde(default)]
    pub summary_only: bool,
    /// Carry the AS numbers of the covered paths in an AS set.
    #[serde(default)]
    pub as_set: bool,
}

/// The keepalive interval and hold time a session uses: what is configured on the neighbour, else
/// the defaults.
pub fn timers_of(n: &Neighbor) -> (u16, u16) {
    n.timers.unwrap_or((KEEPALIVE_SECS, HOLD_SECS))
}

/// `neighbor … maximum-prefix N [threshold] [restart M] [warning-only]`: the most prefixes the
/// neighbour may send, and whether going over it only warns (the session stays up).
pub fn max_prefix(n: &Neighbor) -> Option<(u32, bool)> {
    let line = n.options.get("maximum-prefix")?;
    let mut words = line.split_whitespace().skip(1);
    let limit = words.next()?.parse::<u32>().ok()?;
    Some((limit, words.any(|w| w == "warning-only")))
}

/// What two neighbours agree on: the hold time is the smaller of the two, and each side sends
/// keepalives at the interval it configured but never slower than a third of the hold time. A hold
/// time of 0 on either side turns keepalives and the hold timer off. Returns this side's keepalive
/// interval and the negotiated hold time.
pub fn negotiate(mine: (u16, u16), theirs: (u16, u16)) -> (u16, u16) {
    let hold = mine.1.min(theirs.1);
    if hold == 0 {
        return (0, 0);
    }
    (mine.0.min(hold / 3), hold)
}

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
    /// `send-community`: attach the communities a path carries when advertising it.
    #[serde(default)]
    pub send_community: bool,
    /// `send-community extended`: attach the extended communities a path carries.
    #[serde(default)]
    pub send_extended: bool,
    /// `timers KEEPALIVE HOLD` (seconds); none means 60 and 180.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timers: Option<(u16, u16)>,
    /// `filter-list N in` / `out`: an AS-path list the paths must pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_list_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_list_out: Option<String>,
    /// `route-reflector-client`: this internal neighbour is a client of the route reflector.
    #[serde(default)]
    pub route_reflector_client: bool,
    /// `default-originate`: send this neighbour a default route whether or not there is one here.
    #[serde(default)]
    pub default_originate: bool,
    /// `default-originate route-map NAME`: the default route is sent only while some route in this
    /// router's tables passes the map, and the map's `set` lines change it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_originate_map: Option<String>,
    /// `password`: the TCP MD5 key of the session; the session comes up only when both ends have
    /// the same one (or neither has one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// `remove-private-as`: take private AS numbers out of the path sent to an external neighbour
    /// (only when the whole path is private, unless `remove_private_all`).
    #[serde(default)]
    pub remove_private_as: bool,
    /// `remove-private-as all`: take every private AS number out.
    #[serde(default)]
    pub remove_private_all: bool,
    /// `soft-reconfiguration inbound`: keep what the neighbour sent before the inbound policy, so
    /// `show ip bgp neighbors A.B.C.D received-routes` can list it.
    #[serde(default)]
    pub soft_reconfiguration_in: bool,
    /// `distribute-list N in` / `out`: a standard access list that the prefixes must pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distribute_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distribute_out: Option<String>,
    /// `neighbor … activate`: the neighbour exchanges IPv4 unicast routes. Every neighbour does
    /// unless `no bgp default ipv4-unicast` is set.
    #[serde(default)]
    pub activated: bool,
    /// Settings that are kept and shown but not simulated (`version 4`, `maximum-prefix N`,
    /// `allowas-in`, ...), by the words that name them, with the line as written.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// `peer-group NAME`: the group this neighbour belongs to. Its settings hold what the group
    /// gave plus what the neighbour set itself (see [`follow`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_group: Option<String>,
}

/// `neighbor NAME peer-group`: settings the neighbours that join the group take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerGroup {
    pub name: String,
    /// The group's settings, in the shape of a neighbour's (its address is not used).
    pub conf: Neighbor,
}

impl PeerGroup {
    pub fn new(name: impl Into<String>) -> PeerGroup {
        PeerGroup { name: name.into(), conf: Neighbor::new(Ipv4Addr::UNSPECIFIED) }
    }
}

fn follow_field<T: PartialEq + Clone>(mine: &mut T, old: &T, new: &T) {
    if mine == old {
        *mine = new.clone();
    }
}

/// Moves the settings a group can give from `old` to `new` on a member: a setting that still holds
/// the value it had under `old` takes the one it has under `new`; any other is the member's own
/// and stays. A member that joins follows from the unconfigured neighbour to the group's settings,
/// a member that leaves from the group's settings to the unconfigured neighbour, and a change to the
/// group follows from the old settings to the new ones. The shutdown and the description are the
/// neighbour's alone.
pub fn follow(n: &mut Neighbor, old: &Neighbor, new: &Neighbor) {
    macro_rules! each {
        ($($f:ident),* $(,)?) => { $( follow_field(&mut n.$f, &old.$f, &new.$f); )* };
    }
    each!(
        remote_as,
        update_source,
        next_hop_self,
        ebgp_multihop,
        weight,
        prefix_list_in,
        prefix_list_out,
        route_map_in,
        route_map_out,
        send_community,
        send_extended,
        timers,
        filter_list_in,
        filter_list_out,
        route_reflector_client,
        default_originate,
        default_originate_map,
        password,
        remove_private_as,
        remove_private_all,
        soft_reconfiguration_in,
        distribute_in,
        distribute_out,
        activated,
        options,
    );
}

/// Whether an AS number is one of the private ranges (RFC 6996).
pub fn is_private_as(asn: u32) -> bool {
    (64_512..=65_534).contains(&asn) || (4_200_000_000..=4_294_967_294).contains(&asn)
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
            send_community: false,
            send_extended: false,
            timers: None,
            filter_list_in: None,
            filter_list_out: None,
            route_reflector_client: false,
            default_originate: false,
            default_originate_map: None,
            password: None,
            remove_private_as: false,
            remove_private_all: false,
            soft_reconfiguration_in: false,
            distribute_in: None,
            distribute_out: None,
            activated: false,
            options: BTreeMap::new(),
            peer_group: None,
        }
    }
}

/// Cleans one neighbour's (or peer group's) settings: bounded text, sane values.
fn sanitize_neighbor(n: &mut Neighbor) {
    n.remote_as = n.remote_as.filter(|a| *a >= 1);
    n.ebgp_multihop = n.ebgp_multihop.filter(|m| *m >= 1);
    if let Some(d) = &mut n.description {
        *d = d.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
    }
    if let Some((k, h)) = n.timers {
        n.timers = Some((k, if h == 0 { 0 } else { h.max(3) }));
    }
    n.password = n.password.take().map(|p| p.chars().filter(|c| !c.is_control()).take(MAX_PASSWORD).collect::<String>()).filter(|p| !p.is_empty());
    for name in [
        &mut n.prefix_list_in,
        &mut n.prefix_list_out,
        &mut n.route_map_in,
        &mut n.route_map_out,
        &mut n.filter_list_in,
        &mut n.filter_list_out,
        &mut n.default_originate_map,
        &mut n.distribute_in,
        &mut n.distribute_out,
    ] {
        *name = name.take().map(|x| crate::policy::clean_name(&x)).filter(|x| !x.is_empty());
    }
    if let Some(u) = &mut n.update_source {
        *u = u.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
    }
    let kept: BTreeMap<String, String> = std::mem::take(&mut n.options)
        .into_iter()
        .take(MAX_OPTIONS)
        .map(|(k, v)| {
            (k.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect(), v.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect())
        })
        .collect();
    n.options = kept;
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
    /// `timers bgp KEEPALIVE HOLD`: the timers of every neighbour that has none of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timers: Option<(u16, u16)>,
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
    /// `aggregate-address`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aggregates: Vec<Aggregate>,
    /// `maximum-paths N`: how many equal external paths enter the routing table (1 when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_paths: Option<u8>,
    /// `maximum-paths ibgp N`: how many equal internal paths enter the routing table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_paths_ibgp: Option<u8>,
    /// `bgp cluster-id A.B.C.D`: the cluster this route reflector serves (the router id when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<Ipv4Addr>,
    /// `bgp confederation identifier N`: the autonomous system the outside world sees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confed_id: Option<u32>,
    /// `bgp confederation peers N ...`: the other member autonomous systems of the confederation
    /// this router has sessions with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confed_peers: Vec<u32>,
    /// `bgp always-compare-med`: compare the MED of paths from different neighbouring ASes too.
    #[serde(default)]
    pub always_compare_med: bool,
    /// `bgp bestpath as-path ignore`: leave the AS-path length out of the best-path choice.
    #[serde(default)]
    pub as_path_ignore: bool,
    /// `neighbor NAME peer-group`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_groups: Vec<PeerGroup>,
    /// Process-wide settings that are typed as `bgp …` words (`bgp bestpath med missing-as-worst`,
    /// `bgp deterministic-med`), by the full line as written. Some change the simulation (see
    /// [`Bgp::option`]), the others are only kept and shown.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub options: BTreeSet<String>,
}

impl Bgp {
    pub fn new(asn: u32) -> Bgp {
        Bgp { asn, ..Bgp::default() }
    }

    /// The keepalive interval and hold time of a neighbour: its own, else the process's, else the
    /// defaults.
    pub fn timers_for(&self, n: &Neighbor) -> (u16, u16) {
        n.timers.or(self.timers).unwrap_or((KEEPALIVE_SECS, HOLD_SECS))
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

    /// Whether the process has the setting written exactly as `line` (for example
    /// `bgp bestpath as-path multipath-relax`, or `no bgp fast-external-fallover` for one that is
    /// on until turned off).
    pub fn option(&self, line: &str) -> bool {
        self.options.contains(line)
    }

    /// Without `no bgp default ipv4-unicast` every neighbour exchanges IPv4 routes; with it only
    /// the activated ones do.
    pub fn exchanges(&self, n: &Neighbor) -> bool {
        n.activated || !self.option("no bgp default ipv4-unicast")
    }

    pub fn group(&self, name: &str) -> Option<&PeerGroup> {
        self.peer_groups.iter().find(|g| g.name == name)
    }

    /// The members of a group.
    pub fn members<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Neighbor> + 'a {
        self.neighbors.iter().filter(move |n| n.peer_group.as_deref() == Some(name))
    }

    /// A group by name, added (with no settings) when missing; `None` past [`MAX_PEER_GROUPS`].
    pub fn group_mut(&mut self, name: &str) -> Option<&mut PeerGroup> {
        if !self.peer_groups.iter().any(|g| g.name == name) {
            if self.peer_groups.len() >= MAX_PEER_GROUPS {
                return None;
            }
            self.peer_groups.push(PeerGroup::new(name));
        }
        self.peer_groups.iter_mut().find(|g| g.name == name)
    }

    /// Changes a group's settings, and gives the change to the members that follow it. `None` when
    /// there is no such group; an error from `f` leaves the members alone.
    pub fn edit_group(&mut self, name: &str, f: impl FnOnce(&mut Neighbor) -> Result<(), String>) -> Option<Result<(), String>> {
        let at = self.peer_groups.iter().position(|g| g.name == name)?;
        let old = self.peer_groups.get(at)?.conf.clone();
        let done = f(&mut self.peer_groups.get_mut(at)?.conf);
        if done.is_ok() {
            let new = self.peer_groups.get(at)?.conf.clone();
            for n in self.neighbors.iter_mut().filter(|n| n.peer_group.as_deref() == Some(name)) {
                follow(n, &old, &new);
            }
        }
        Some(done)
    }

    /// Puts a neighbour in a group (the neighbour is added when missing): it takes every setting of
    /// the group it has not set itself. Fails when the group does not exist, the neighbour is in
    /// another group, or the neighbours are too many.
    pub fn join_group(&mut self, addr: Ipv4Addr, name: &str) -> Result<(), String> {
        let conf = self.group(name).map(|g| g.conf.clone()).ok_or_else(|| format!("no peer group named {name}"))?;
        if let Some(other) = self.neighbor(addr).and_then(|n| n.peer_group.clone()).filter(|g| g != name) {
            return Err(format!("{addr} is in peer group {other} already"));
        }
        let n = self.neighbor_mut(addr).ok_or_else(|| format!("at most {MAX_NEIGHBORS} neighbours"))?;
        follow(n, &Neighbor::new(addr), &conf);
        n.peer_group = Some(name.to_string());
        Ok(())
    }

    /// Takes a neighbour out of its group: what it only had from the group goes, what it set
    /// itself stays. `false` when it was in none.
    pub fn leave_group(&mut self, addr: Ipv4Addr) -> bool {
        let Some(name) = self.neighbor(addr).and_then(|n| n.peer_group.clone()) else { return false };
        let conf = self.group(&name).map(|g| g.conf.clone());
        if let Some(n) = self.neighbors.iter_mut().find(|n| n.addr == addr) {
            if let Some(conf) = conf {
                follow(n, &conf, &Neighbor::new(addr));
            }
            n.peer_group = None;
        }
        true
    }

    /// Deletes a group; its members leave it. `false` when there is no such group.
    pub fn remove_group(&mut self, name: &str) -> bool {
        let members: Vec<Ipv4Addr> = self.members(name).map(|n| n.addr).collect();
        for a in members {
            self.leave_group(a);
        }
        let before = self.peer_groups.len();
        self.peer_groups.retain(|g| g.name != name);
        self.peer_groups.len() != before
    }

    /// How many equal paths per prefix the routing table takes.
    pub fn multipath(&self) -> usize {
        usize::from(self.maximum_paths.unwrap_or(1).clamp(1, MAX_MULTIPATH))
    }

    /// How many equal internal paths per prefix the routing table takes.
    pub fn multipath_ibgp(&self) -> usize {
        usize::from(self.maximum_paths_ibgp.unwrap_or(1).clamp(1, MAX_MULTIPATH))
    }

    /// The autonomous system a router outside the confederation sees: the confederation
    /// identifier when there is one, else the router's own.
    pub fn outer_as(&self) -> u32 {
        self.confed_id.unwrap_or(self.asn)
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
        self.neighbors.iter_mut().for_each(sanitize_neighbor);
        self.options = std::mem::take(&mut self.options)
            .into_iter()
            .take(MAX_OPTIONS)
            .map(|o| o.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect::<String>())
            .collect();
        self.peer_groups.truncate(MAX_PEER_GROUPS);
        for g in &mut self.peer_groups {
            g.name = crate::policy::clean_name(&g.name);
            sanitize_neighbor(&mut g.conf);
            g.conf.addr = Ipv4Addr::UNSPECIFIED;
            g.conf.shutdown = false;
            g.conf.description = None;
            g.conf.peer_group = None;
        }
        self.peer_groups.retain(|g| !g.name.is_empty() && g.name.parse::<Ipv4Addr>().is_err());
        let mut seen = std::collections::BTreeSet::new();
        self.peer_groups.retain(|g| seen.insert(g.name.clone()));
        let names: Vec<String> = self.peer_groups.iter().map(|g| g.name.clone()).collect();
        for n in &mut self.neighbors {
            n.peer_group = n.peer_group.take().filter(|g| names.contains(g));
        }
        self.networks.iter_mut().for_each(|n| *n = n.network());
        self.networks.sort();
        self.networks.dedup();
        self.networks.truncate(MAX_NETWORKS);
        for a in &mut self.aggregates {
            a.net = a.net.network();
        }
        self.aggregates.sort_by_key(|a| a.net);
        self.aggregates.dedup_by_key(|a| a.net);
        self.aggregates.truncate(MAX_AGGREGATES);
        self.maximum_paths = self.maximum_paths.map(|m| m.clamp(1, MAX_MULTIPATH)).filter(|m| *m > 1);
        self.maximum_paths_ibgp = self.maximum_paths_ibgp.map(|m| m.clamp(1, MAX_MULTIPATH)).filter(|m| *m > 1);
        self.confed_id = self.confed_id.filter(|a| *a >= 1);
        let own = self.asn;
        self.confed_peers.retain(|a| *a >= 1 && *a != own);
        self.confed_peers.sort_unstable();
        self.confed_peers.dedup();
        self.confed_peers.truncate(MAX_CONFED_PEERS);
        for n in &mut self.neighbors {
            n.remove_private_all &= n.remove_private_as;
        }
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
    fn timers_are_negotiated_to_the_smaller_hold_time() {
        let mut n = Neighbor::new(Ipv4Addr::new(10, 0, 0, 2));
        assert_eq!(timers_of(&n), (60, 180));
        n.timers = Some((10, 30));
        assert_eq!(negotiate(timers_of(&n), (60, 180)), (10, 30));
        assert_eq!(negotiate((60, 180), (10, 30)), (10, 30), "a third of the hold time at most");
        assert_eq!(negotiate((60, 180), (60, 0)), (0, 0));
        assert_eq!(negotiate((60, 180), (60, 180)), (60, 180));
    }

    #[test]
    fn aggregates_and_multipath_are_cleaned() {
        let mut b = Bgp::new(65_001);
        b.aggregates = vec![
            Aggregate { net: "10.1.2.3/16".parse().unwrap(), summary_only: true, as_set: false },
            Aggregate { net: "10.1.0.0/16".parse().unwrap(), summary_only: false, as_set: true },
        ];
        b.maximum_paths = Some(200);
        b.neighbors = vec![Neighbor { timers: Some((5, 1)), ..Neighbor::new(Ipv4Addr::new(10, 0, 0, 2)) }];
        b.sanitize();
        assert_eq!(b.aggregates.len(), 1);
        assert_eq!(b.aggregates[0].net, "10.1.0.0/16".parse::<Ipv4Net>().unwrap());
        assert_eq!((b.maximum_paths, b.multipath()), (Some(MAX_MULTIPATH), usize::from(MAX_MULTIPATH)));
        assert_eq!(b.neighbors[0].timers, Some((5, 3)));
        assert_eq!(Bgp::new(1).multipath(), 1);
    }

    #[test]
    fn reflection_confederation_and_neighbour_options_are_cleaned_and_kept() {
        let mut b = Bgp::new(65_001);
        b.confed_id = Some(0);
        b.confed_peers = vec![65_001, 65_003, 0, 65_002, 65_003];
        b.maximum_paths_ibgp = Some(200);
        b.cluster_id = Some(Ipv4Addr::new(9, 9, 9, 9));
        b.neighbors = vec![Neighbor { remove_private_all: true, ..Neighbor::new(Ipv4Addr::new(10, 0, 0, 2)) }];
        b.sanitize();
        assert_eq!(b.confed_id, None);
        assert_eq!(b.confed_peers, vec![65_002, 65_003], "no zero, not our own AS, sorted, no duplicates");
        assert_eq!((b.maximum_paths_ibgp, b.multipath_ibgp()), (Some(MAX_MULTIPATH), usize::from(MAX_MULTIPATH)));
        assert!(!b.neighbors[0].remove_private_all, "`all` means nothing without remove-private-as");
        b.confed_id = Some(100);
        assert_eq!(b.outer_as(), 100);
        assert_eq!(Bgp::new(7).outer_as(), 7);
        assert!(is_private_as(64_512) && is_private_as(65_534) && !is_private_as(65_535) && !is_private_as(64_511));
        assert!(is_private_as(4_200_000_000) && !is_private_as(4_294_967_295));
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(serde_json::from_str::<Bgp>(&json).unwrap(), b);
        let plain = serde_json::to_string(&Bgp::new(1)).unwrap();
        assert!(!plain.contains("cluster_id") && !plain.contains("confed") && !plain.contains("maximum_paths_ibgp"), "{plain}");
    }

    fn grouped() -> Bgp {
        let mut b = Bgp::new(65_001);
        let g = b.group_mut("EXT").unwrap();
        g.conf.remote_as = Some(65_002);
        g.conf.send_community = true;
        g.conf.route_map_in = Some("IN".into());
        b
    }

    #[test]
    fn a_member_takes_what_it_has_not_set_and_keeps_its_own() {
        let mut b = grouped();
        let (a1, a2) = (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2));
        b.neighbor_mut(a2).unwrap().route_map_in = Some("MINE".into());
        b.join_group(a1, "EXT").unwrap();
        b.join_group(a2, "EXT").unwrap();
        let n1 = b.neighbor(a1).unwrap();
        assert_eq!((n1.remote_as, n1.send_community, n1.route_map_in.as_deref()), (Some(65_002), true, Some("IN")));
        let n2 = b.neighbor(a2).unwrap();
        assert_eq!((n2.remote_as, n2.send_community, n2.route_map_in.as_deref()), (Some(65_002), true, Some("MINE")));
        assert_eq!(b.members("EXT").count(), 2);
        assert!(b.join_group(a1, "NOPE").unwrap_err().contains("no peer group"));
        b.group_mut("OTHER").unwrap();
        assert!(b.join_group(a1, "OTHER").unwrap_err().contains("already"));
    }

    #[test]
    fn a_change_to_the_group_reaches_the_members_that_follow_it() {
        let mut b = grouped();
        let (a1, a2) = (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2));
        b.neighbor_mut(a2).unwrap().route_map_in = Some("MINE".into());
        b.join_group(a1, "EXT").unwrap();
        b.join_group(a2, "EXT").unwrap();
        let done = b.edit_group("EXT", |g| {
            g.route_map_in = Some("NEW".into());
            g.next_hop_self = true;
            Ok(())
        });
        assert_eq!(done, Some(Ok(())));
        assert_eq!(b.neighbor(a1).unwrap().route_map_in.as_deref(), Some("NEW"));
        assert_eq!(b.neighbor(a2).unwrap().route_map_in.as_deref(), Some("MINE"), "its own setting stays");
        assert!(b.neighbor(a1).unwrap().next_hop_self && b.neighbor(a2).unwrap().next_hop_self);
        assert!(b.edit_group("missing", |_| Ok(())).is_none());
        let failed = b.edit_group("EXT", |_| Err("no".into()));
        assert_eq!(failed, Some(Err("no".to_string())));
    }

    #[test]
    fn leaving_a_group_keeps_only_what_the_member_set_itself() {
        let mut b = grouped();
        let (a1, a2) = (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2));
        b.neighbor_mut(a2).unwrap().route_map_in = Some("MINE".into());
        b.join_group(a1, "EXT").unwrap();
        b.join_group(a2, "EXT").unwrap();
        assert!(b.leave_group(a1) && !b.leave_group(a1));
        let n1 = b.neighbor(a1).unwrap();
        assert_eq!((n1.remote_as, n1.send_community, n1.route_map_in.clone(), n1.peer_group.clone()), (None, false, None, None));
        assert!(b.remove_group("EXT") && !b.remove_group("EXT"));
        let n2 = b.neighbor(a2).unwrap();
        assert_eq!((n2.remote_as, n2.route_map_in.as_deref(), n2.peer_group.clone()), (None, Some("MINE"), None));
    }

    #[test]
    fn groups_are_bounded_cleaned_and_round_trip() {
        let mut b = Bgp::new(65_001);
        for i in 0..40 {
            let _ = b.group_mut(&format!("G{i}"));
        }
        assert_eq!(b.peer_groups.len(), MAX_PEER_GROUPS);
        let mut b = grouped();
        b.neighbor_mut(Ipv4Addr::new(10, 0, 0, 1)).unwrap().peer_group = Some("GONE".into());
        b.peer_groups.push(PeerGroup::new("EXT"));
        b.peer_groups.push(PeerGroup::new("10.0.0.9"));
        b.peer_groups.push(PeerGroup::new(""));
        b.sanitize();
        assert_eq!(b.peer_groups.len(), 1);
        assert_eq!(b.neighbor(Ipv4Addr::new(10, 0, 0, 1)).unwrap().peer_group, None);
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(serde_json::from_str::<Bgp>(&json).unwrap(), b);
        let plain = serde_json::to_string(&Bgp::new(1)).unwrap();
        assert!(!plain.contains("peer_group"), "{plain}");
    }

    #[test]
    fn serde_round_trip_leaves_defaults_out() {
        let mut b = Bgp::new(65_001);
        b.neighbor_mut(Ipv4Addr::new(10, 0, 0, 2)).unwrap().remote_as = Some(65_002);
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("router_id") && !json.contains("update_source"), "{json}");
        assert!(!json.contains("timers") && !json.contains("aggregates") && !json.contains("maximum_paths"), "{json}");
        assert_eq!(serde_json::from_str::<Bgp>(&json).unwrap(), b);
    }
}
