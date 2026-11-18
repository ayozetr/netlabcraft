//! Route redistribution (`redistribute`) and distribute lists (`distribute-list`), as the
//! routing processes of a router keep them.
//!
//! A redistribution names where routes come from and, for the protocol that takes them, the
//! metric they get, the OSPF external type and a route map that may drop or change them. A
//! distribute list names a prefix list, an access list or a route map that decides which routes
//! a process installs (`in`) or advertises (`out`), for every interface or for one.
//!
//! The model only keeps the configuration; the simulator reads it (`netlabcraft_sim::redist`).

use netlabcraft_core::Ipv4Net;
use serde::{Deserialize, Serialize};

use crate::Device;
use crate::acl::PacketKey;
use crate::policy::{self, Subject};

/// Most `redistribute` lines under one routing process.
pub const MAX_REDISTRIBUTE: usize = 16;
/// Most `distribute-list` lines under one routing process.
pub const MAX_DIST_LISTS: usize = 32;
/// Longest interface name a distribute list keeps.
pub const MAX_IFACE_NAME: usize = 40;

/// Where a `redistribute` line takes routes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RedistSource {
    Connected,
    Static,
    Rip,
    /// An OSPF process (its number).
    Ospf(u16),
    /// An EIGRP autonomous system.
    Eigrp(u16),
    /// A BGP autonomous system.
    Bgp(u32),
}

impl RedistSource {
    /// The words the source is written with: `connected`, `ospf 1`, `bgp 65001`.
    pub fn words(self) -> String {
        match self {
            RedistSource::Connected => "connected".to_string(),
            RedistSource::Static => "static".to_string(),
            RedistSource::Rip => "rip".to_string(),
            RedistSource::Ospf(n) => format!("ospf {n}"),
            RedistSource::Eigrp(n) => format!("eigrp {n}"),
            RedistSource::Bgp(n) => format!("bgp {n}"),
        }
    }

    /// Whether the source is another routing protocol (connected and static routes are not).
    pub fn is_protocol(self) -> bool {
        !matches!(self, RedistSource::Connected | RedistSource::Static)
    }
}

/// EIGRP's five metric values (`metric BANDWIDTH DELAY RELIABILITY LOAD MTU`, and
/// `default-metric` with the same five). The simulator's K values are the usual ones (K1 = K3 =
/// 1), so only the bandwidth and the delay decide the composite metric; the other three are kept
/// and shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EigrpMetric {
    /// Kbit/s.
    pub bandwidth: u32,
    /// Tens of microseconds.
    pub delay: u32,
    pub reliability: u8,
    pub load: u8,
    pub mtu: u16,
}

impl EigrpMetric {
    /// The values as typed: `10000 100 255 1 1500`.
    pub fn words(&self) -> String {
        format!("{} {} {} {} {}", self.bandwidth, self.delay, self.reliability, self.load, self.mtu)
    }

    /// Keeps the values a router would accept.
    pub fn sanitize(&mut self) {
        self.bandwidth = self.bandwidth.max(1);
        self.load = self.load.max(1);
        self.mtu = self.mtu.max(1);
    }
}

/// One `redistribute SOURCE …` line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redist {
    pub source: RedistSource,
    /// `metric N`: RIP hops (1-16) or the OSPF external metric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<u32>,
    /// `metric BANDWIDTH DELAY RELIABILITY LOAD MTU` under EIGRP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eigrp_metric: Option<EigrpMetric>,
    /// `metric-type 1|2` under OSPF (2 when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_type: Option<u8>,
    /// `route-map NAME`: which routes pass, and what the map changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_map: Option<String>,
    /// `tag N` under OSPF: the tag the external routes get (a route map's `set tag` comes first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<u32>,
    /// `nssa-only` under OSPF: inside a not-so-stubby area the type 7 LSA carries a clear P bit, so
    /// it stays in the area and no border router translates it into a type 5 LSA.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nssa_only: bool,
}

impl Redist {
    pub fn new(source: RedistSource) -> Redist {
        Redist { source, metric: None, eigrp_metric: None, metric_type: None, route_map: None, tag: None, nssa_only: false }
    }
}

/// The line for a redistribution, as the running configuration writes it (without the leading
/// space): `redistribute ospf 1 metric 5 metric-type 1 tag 7 subnets route-map M`.
pub fn redistribute_line(r: &Redist, ospf: bool) -> String {
    let mut s = format!("redistribute {}", r.source.words());
    if let Some(m) = &r.eigrp_metric {
        s.push_str(&format!(" metric {}", m.words()));
    } else if let Some(m) = r.metric {
        s.push_str(&format!(" metric {m}"));
    }
    if ospf {
        if let Some(t) = r.metric_type {
            s.push_str(&format!(" metric-type {t}"));
        }
        if let Some(t) = r.tag {
            s.push_str(&format!(" tag {t}"));
        }
        s.push_str(" subnets");
        if r.nssa_only {
            s.push_str(" nssa-only");
        }
    }
    if let Some(m) = &r.route_map {
        s.push_str(&format!(" route-map {m}"));
    }
    s
}

/// What a distribute list names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DistKind {
    Prefix,
    Acl,
    RouteMap,
}

/// One `distribute-list` line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistList {
    pub kind: DistKind,
    /// The prefix list, access list or route map.
    pub name: String,
    /// `in` (what is installed) or `out` (what is advertised).
    pub inbound: bool,
    /// The interface it applies to; none: every interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iface: Option<String>,
}

impl DistList {
    /// The line as the running configuration writes it (without the leading space).
    pub fn line(&self) -> String {
        let dir = if self.inbound { "in" } else { "out" };
        let head = match self.kind {
            DistKind::Prefix => format!("distribute-list prefix {} {dir}", self.name),
            DistKind::Acl => format!("distribute-list {} {dir}", self.name),
            DistKind::RouteMap => format!("distribute-list route-map {} {dir}", self.name),
        };
        match &self.iface {
            Some(i) => format!("{head} {i}"),
            None => head,
        }
    }

    /// Whether the list lets a route through. A list that is named but not defined lets
    /// everything through (a name can be typed before its list is); a prefix list denies what no
    /// line matches, an access list likewise, a route map drops what no clause permits.
    pub fn permits(&self, d: &Device, net: Ipv4Net, metric: u32) -> bool {
        match self.kind {
            DistKind::Prefix => d.prefix_list(&self.name).is_none_or(|l| l.permits(net)),
            DistKind::Acl => {
                let key = PacketKey::bare(0, net.addr, net.mask());
                d.acls.iter().find(|a| a.name == self.name).is_none_or(|a| a.evaluate(&key).0)
            }
            DistKind::RouteMap => d.route_map(&self.name).is_none_or(|m| m.run(d, &Subject { net, metric }).is_some()),
        }
    }
}

/// Whether the distribute lists of one direction let a route through. A list set for the
/// interface decides for it; without one, the lists with no interface do.
pub fn passes(d: &Device, lists: &[DistList], inbound: bool, iface: Option<&str>, net: Ipv4Net, metric: u32) -> bool {
    let specific: Vec<&DistList> = match iface {
        Some(i) => lists.iter().filter(|l| l.inbound == inbound && l.iface.as_deref() == Some(i)).collect(),
        None => Vec::new(),
    };
    if !specific.is_empty() {
        return specific.iter().all(|l| l.permits(d, net, metric));
    }
    lists.iter().filter(|l| l.inbound == inbound && l.iface.is_none()).all(|l| l.permits(d, net, metric))
}

/// Cleans the redistribution lines a lab file holds: one per source, bounded, valid values.
pub fn sanitize_redistribute(list: &mut Vec<Redist>) {
    let mut seen = std::collections::BTreeSet::new();
    list.retain(|r| seen.insert(r.source));
    list.truncate(MAX_REDISTRIBUTE);
    for r in list.iter_mut() {
        r.route_map = r.route_map.take().map(|m| policy::clean_name(&m)).filter(|m| !m.is_empty());
        r.metric_type = r.metric_type.filter(|t| (1..=2).contains(t));
        if let Some(m) = &mut r.eigrp_metric {
            m.sanitize();
        }
    }
}

/// Cleans the distribute lists a lab file holds: one per direction and interface, bounded.
pub fn sanitize_dist_lists(list: &mut Vec<DistList>) {
    for l in list.iter_mut() {
        l.name = policy::clean_name(&l.name);
        l.iface = l.iface.take().map(|i| i.chars().filter(|c| !c.is_control()).take(MAX_IFACE_NAME).collect::<String>()).filter(|i| !i.is_empty());
        if l.kind == DistKind::RouteMap {
            l.inbound = true;
        }
    }
    list.retain(|l| !l.name.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    list.retain(|l| seen.insert((l.inbound, l.iface.clone())));
    list.truncate(MAX_DIST_LISTS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;
    use crate::policy::{PrefixEntry, PrefixList, RouteMap};

    fn net(s: &str) -> Ipv4Net {
        s.parse().unwrap()
    }

    fn router() -> Device {
        let mut t = crate::Topology::new();
        let id = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        t.device(id).unwrap().clone()
    }

    fn list(kind: DistKind, name: &str, inbound: bool, iface: Option<&str>) -> DistList {
        DistList { kind, name: name.into(), inbound, iface: iface.map(String::from) }
    }

    #[test]
    fn source_words_and_lines_read_back_as_typed() {
        assert_eq!(RedistSource::Ospf(1).words(), "ospf 1");
        assert_eq!(RedistSource::Bgp(65_001).words(), "bgp 65001");
        let mut r = Redist::new(RedistSource::Ospf(1));
        r.metric = Some(5);
        r.metric_type = Some(1);
        r.route_map = Some("M".into());
        assert_eq!(redistribute_line(&r, true), "redistribute ospf 1 metric 5 metric-type 1 subnets route-map M");
        r.tag = Some(7);
        assert_eq!(redistribute_line(&r, true), "redistribute ospf 1 metric 5 metric-type 1 tag 7 subnets route-map M");
        assert_eq!(redistribute_line(&Redist::new(RedistSource::Static), false), "redistribute static");
        let mut e = Redist::new(RedistSource::Rip);
        e.eigrp_metric = Some(EigrpMetric { bandwidth: 10_000, delay: 100, reliability: 255, load: 1, mtu: 1500 });
        assert_eq!(redistribute_line(&e, false), "redistribute rip metric 10000 100 255 1 1500");
    }

    #[test]
    fn a_distribute_list_names_what_decides() {
        let mut d = router();
        let mut pl = PrefixList::new("ten");
        pl.add(PrefixEntry { seq: 5, permit: true, net: net("10.0.0.0/8"), ge: None, le: Some(24) }).unwrap();
        d.prefix_lists.push(pl);
        let p = list(DistKind::Prefix, "ten", true, None);
        assert!(p.permits(&d, net("10.1.0.0/16"), 0) && !p.permits(&d, net("192.168.0.0/24"), 0));
        // A name with nothing behind it lets everything through.
        assert!(list(DistKind::Prefix, "nope", true, None).permits(&d, net("192.168.0.0/24"), 0));
        assert!(list(DistKind::Acl, "nope", false, None).permits(&d, net("192.168.0.0/24"), 0));
        assert!(list(DistKind::RouteMap, "nope", true, None).permits(&d, net("192.168.0.0/24"), 0));
        // A route map drops what no clause permits.
        let mut m = RouteMap::new("rm");
        m.clause_mut(10, true).unwrap().prefix_lists.push("ten".into());
        d.route_maps.push(m);
        let r = list(DistKind::RouteMap, "rm", true, None);
        assert!(r.permits(&d, net("10.1.0.0/16"), 0) && !r.permits(&d, net("172.16.0.0/16"), 0));
    }

    #[test]
    fn an_interface_list_replaces_the_general_one_for_that_interface() {
        let mut d = router();
        let mut pl = PrefixList::new("ten");
        pl.add(PrefixEntry { seq: 5, permit: true, net: net("10.0.0.0/8"), ge: None, le: Some(32) }).unwrap();
        d.prefix_lists.push(pl);
        let lists = vec![list(DistKind::Prefix, "ten", true, None), list(DistKind::Prefix, "none-defined", true, Some("GigabitEthernet0/1"))];
        let other = net("192.168.0.0/24");
        assert!(!passes(&d, &lists, true, Some("GigabitEthernet0/0"), other, 0), "the general list applies");
        assert!(passes(&d, &lists, true, Some("GigabitEthernet0/1"), other, 0), "the interface's own list decides there");
        assert!(passes(&d, &lists, false, None, other, 0), "no list in the other direction");
    }

    #[test]
    fn a_hostile_file_is_cleaned() {
        let mut r = vec![Redist::new(RedistSource::Static), Redist::new(RedistSource::Static), Redist::new(RedistSource::Ospf(1))];
        r[0].metric_type = Some(9);
        r[0].route_map = Some("a b\n".repeat(50));
        r[2].eigrp_metric = Some(EigrpMetric { bandwidth: 0, delay: 0, reliability: 0, load: 0, mtu: 0 });
        sanitize_redistribute(&mut r);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].metric_type, None);
        assert!(r[0].route_map.as_ref().is_some_and(|m| m.chars().count() <= policy::MAX_NAME && !m.contains(['\n', ' '])));
        assert_eq!(r[1].eigrp_metric.map(|m| (m.bandwidth, m.load, m.mtu)), Some((1, 1, 1)));
        let mut l = vec![
            list(DistKind::Prefix, "x", true, None),
            list(DistKind::Acl, "y", true, None),
            list(DistKind::Prefix, "", false, None),
            list(DistKind::RouteMap, "m", false, Some("Gi0/0")),
        ];
        sanitize_dist_lists(&mut l);
        assert_eq!(l.len(), 2);
        assert!(l[1].inbound, "a route map filters only what comes in");
    }

    #[test]
    fn the_old_static_only_form_becomes_a_redistribute_line() {
        let mut o = crate::Ospf { process: 1, redistribute_static: true, redistribute_map: Some("M".into()), ..crate::Ospf::default() };
        o.sanitize();
        assert!(!o.redistribute_static && o.redistribute_map.is_none());
        assert_eq!(o.redistribute.len(), 1);
        assert_eq!(o.redistribute[0].source, RedistSource::Static);
        assert_eq!(o.redistribute[0].route_map.as_deref(), Some("M"));
        // Read again, it adds nothing.
        o.sanitize();
        assert_eq!(o.redistribute.len(), 1);
        assert!(o.redistributes(RedistSource::Static).is_some() && o.redistributes(RedistSource::Rip).is_none());
    }

    #[test]
    fn serde_leaves_defaults_out() {
        let r = Redist::new(RedistSource::Ospf(1));
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("metric") && !json.contains("route_map"), "{json}");
        assert_eq!(serde_json::from_str::<Redist>(&json).unwrap(), r);
        let l = list(DistKind::Prefix, "P", false, Some("Serial0/0/0"));
        assert_eq!(serde_json::from_str::<DistList>(&serde_json::to_string(&l).unwrap()).unwrap(), l);
        assert_eq!(l.line(), "distribute-list prefix P out Serial0/0/0");
    }
}
