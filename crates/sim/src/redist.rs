//! Route redistribution between the routing protocols of a router.
//!
//! A router that redistributes takes routes from its routing table, as the table holds them
//! after the administrative distances have chosen (so a route is only handed on by the protocol
//! that won it, and a route a protocol took from a redistribution is not handed back), and
//! gives them to another protocol as routes of its own: RIP as a route with the metric the line
//! or `default-metric` names, EIGRP as an external route (`D EX`, distance 170) that starts from
//! the bandwidth and delay it is given, OSPF as a type-1 or type-2 external route. A route map
//! may drop routes or change their metric (and, for OSPF, their type).
//!
//! The protocols are computed from the lab as it converges, each from its own configuration, so
//! redistribution is settled the same way: the protocols are computed with the seeds known so
//! far, the seeds are worked out again from the routing tables that came out, and the round
//! repeats until nothing changes (at most [`MAX_ROUNDS`], so a configuration that never settles,
//! such as two routers redistributing in a ring, stops where it is rather than looping).
//!
//! Simplifications, recorded in `docs/gaps.md`: BGP routes redistributed into a protocol are
//! taken as if every session that can come up were established; the neighbours kept for a hold
//! or invalid time do not count here; EIGRP reliability, load and MTU are kept but do not enter
//! the metric (K2 = K4 = K5 = 0).

use std::collections::BTreeMap;

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_model::policy::{self, Sets, Subject};
use netlabcraft_model::redist::{self as conf, RedistSource};
use netlabcraft_model::{Device, Topology};

use crate::RouteEntry;

/// Rounds of recomputation at most.
pub const MAX_ROUNDS: usize = 8;
/// Most seeds one router gives one protocol.
pub const MAX_SEEDS: usize = 1_024;

/// A route given to RIP: the network and the hop count it has on this router (one less than the
/// metric it is advertised with, as for a connected network).
pub type RipSeed = (Ipv4Net, u8);

/// A route given to EIGRP: the network, the bandwidth (kbit/s) and the delay (tens of
/// microseconds) it starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EigrpSeed {
    pub net: Ipv4Net,
    pub bandwidth: u32,
    pub delay: u64,
}

/// A route given to OSPF as an external route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OspfSeed {
    pub net: Ipv4Net,
    pub metric: u32,
    /// Type 1: the cost to the router that advertises it is added; type 2: the metric alone.
    pub type1: bool,
}

/// What each router redistributes into each protocol.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seeds {
    pub rip: BTreeMap<DeviceId, Vec<RipSeed>>,
    pub eigrp: BTreeMap<DeviceId, Vec<EigrpSeed>>,
    pub ospf: BTreeMap<DeviceId, Vec<OspfSeed>>,
}

fn wants(d: &Device) -> bool {
    d.ospf.as_ref().is_some_and(|o| !o.redistribute.is_empty())
        || d.rip.as_ref().is_some_and(|r| !r.redistribute.is_empty())
        || d.eigrp.as_ref().is_some_and(|e| !e.redistribute.is_empty())
}

fn wants_bgp(d: &Device) -> bool {
    let bgp = |l: &[conf::Redist]| l.iter().any(|r| matches!(r.source, RedistSource::Bgp(_)));
    d.ospf.as_ref().is_some_and(|o| bgp(&o.redistribute))
        || d.rip.as_ref().is_some_and(|r| bgp(&r.redistribute))
        || d.eigrp.as_ref().is_some_and(|e| bgp(&e.redistribute))
}

/// The seeds of the lab: what every router redistributes, once the protocols have settled.
pub fn seeds(t: &Topology) -> Seeds {
    if !t.devices.iter().any(wants) {
        return Seeds::default();
    }
    let mut cur = Seeds::default();
    for _ in 0..MAX_ROUNDS {
        let tables = tables(t, &cur);
        let next = derive(t, &tables);
        if next == cur {
            break;
        }
        cur = next;
    }
    cur
}

/// Every router's routing table with the protocols computed from `seeds`.
fn tables(t: &Topology, seeds: &Seeds) -> BTreeMap<DeviceId, Vec<RouteEntry>> {
    let e = if t.devices.iter().any(|x| x.eigrp.is_some()) { crate::eigrp::compute_seeded(t, &[], seeds).routes } else { BTreeMap::new() };
    let o = if t.devices.iter().any(|x| x.ospf.is_some()) { crate::ospf::compute_seeded(t, seeds).routes } else { BTreeMap::new() };
    let r = if t.devices.iter().any(|x| x.rip.is_some()) { crate::rip::compute_seeded(t, &[], seeds).routes } else { BTreeMap::new() };
    let mut base: BTreeMap<DeviceId, Vec<RouteEntry>> = BTreeMap::new();
    for d in t.devices.iter().filter(|d| d.routes_ip()) {
        let table = crate::routing_table_with(
            t,
            d,
            e.get(&d.id).map_or(&[][..], Vec::as_slice),
            o.get(&d.id).map_or(&[][..], Vec::as_slice),
            r.get(&d.id).map_or(&[][..], Vec::as_slice),
            &[],
        );
        base.insert(d.id, table);
    }
    if !t.devices.iter().any(wants_bgp) {
        return base;
    }
    let b = crate::bgp::standalone_all(t, &base);
    let mut out = BTreeMap::new();
    for d in t.devices.iter().filter(|d| d.routes_ip()) {
        let table = crate::routing_table_with(
            t,
            d,
            e.get(&d.id).map_or(&[][..], Vec::as_slice),
            o.get(&d.id).map_or(&[][..], Vec::as_slice),
            r.get(&d.id).map_or(&[][..], Vec::as_slice),
            b.get(&d.id).map_or(&[][..], Vec::as_slice),
        );
        out.insert(d.id, table);
    }
    out
}

/// Whether a routing-table row is one a `redistribute` from `src` takes.
fn from_source(d: &Device, src: RedistSource, r: &RouteEntry) -> bool {
    if r.iface.as_deref() == Some(crate::eigrp::NULL0) {
        return false;
    }
    match src {
        RedistSource::Connected => r.code == 'C',
        RedistSource::Static => r.code == 'S' && r.distance != 254,
        RedistSource::Rip => r.code == 'R',
        RedistSource::Ospf(n) => matches!(r.code, 'O' | 'I' | 'E' | 'F' | 'N' | 'M') && d.ospf.as_ref().is_some_and(|o| o.process == n),
        RedistSource::Eigrp(n) => matches!(r.code, 'D' | 'X') && d.eigrp.as_ref().is_some_and(|e| e.asn == n),
        RedistSource::Bgp(n) => r.code == 'B' && d.bgp.as_ref().is_some_and(|b| b.asn == n),
    }
}

/// What one `redistribute` line takes from a table: each network once, with what its route map
/// (if it names one) lets through and changes. Networks an earlier line took are skipped.
fn taken<'a>(d: &Device, line: &conf::Redist, table: &'a [RouteEntry], done: &mut Vec<Ipv4Net>) -> Vec<(&'a RouteEntry, Sets)> {
    let mut out = Vec::new();
    for r in table.iter().filter(|r| from_source(d, line.source, r)) {
        if done.contains(&r.net) {
            continue;
        }
        let Some(sets) = policy::apply(d, None, line.route_map.as_deref(), &Subject { net: r.net, metric: r.metric }) else { continue };
        done.push(r.net);
        out.push((r, sets));
    }
    out
}

/// The bandwidth and delay of the interface a connected or static route leaves by.
fn interface_metrics(t: &Topology, d: &Device, table: &[RouteEntry], r: &RouteEntry) -> Option<(u32, u64)> {
    let name = match (&r.iface, r.via) {
        (Some(i), _) => i.clone(),
        (None, Some(v)) => table.iter().find(|c| c.code == 'C' && c.net.contains(v)).and_then(|c| c.iface.clone())?,
        (None, None) => return None,
    };
    let ifc = crate::up_ifaces(t, d).into_iter().find(|i| i.name == name)?;
    let (bw, delay) = crate::eigrp::if_metrics(d, &ifc);
    Some((bw, u64::from(delay)))
}

fn derive(t: &Topology, tables: &BTreeMap<DeviceId, Vec<RouteEntry>>) -> Seeds {
    let mut out = Seeds::default();
    for d in t.devices.iter().filter(|d| d.routes_ip()) {
        let Some(table) = tables.get(&d.id) else { continue };
        if let Some(rip) = d.rip.as_ref().filter(|r| !r.redistribute.is_empty()) {
            let mut seeds: Vec<RipSeed> = Vec::new();
            let mut done = Vec::new();
            for line in &rip.redistribute {
                for (r, sets) in taken(d, line, table, &mut done) {
                    let own = sets.metric.or(line.metric).or(rip.default_metric.map(u32::from));
                    // Connected and static routes get a hop when nothing says otherwise; the
                    // others are unreachable without a metric.
                    let m = match own {
                        Some(m) => m,
                        None if matches!(line.source, RedistSource::Connected | RedistSource::Static) => 1,
                        None => continue,
                    };
                    let m = m.max(1);
                    if m >= u32::from(crate::rip::INFINITY) {
                        continue;
                    }
                    if let Ok(hops) = u8::try_from(m - 1)
                        && !seeds.iter().any(|(n, _)| *n == r.net)
                    {
                        seeds.push((r.net, hops));
                    }
                }
            }
            seeds.truncate(MAX_SEEDS);
            if !seeds.is_empty() {
                out.rip.insert(d.id, seeds);
            }
        }
        if let Some(e) = d.eigrp.as_ref().filter(|e| !e.redistribute.is_empty()) {
            let mut seeds: Vec<EigrpSeed> = Vec::new();
            let mut done = Vec::new();
            for line in &e.redistribute {
                for (r, _) in taken(d, line, table, &mut done) {
                    let given = line.eigrp_metric.or(e.default_metric);
                    let (bandwidth, delay) = match given {
                        Some(m) => (m.bandwidth.max(1), u64::from(m.delay)),
                        None if matches!(line.source, RedistSource::Connected | RedistSource::Static) => match interface_metrics(t, d, table, r) {
                            Some(x) => x,
                            None => continue,
                        },
                        None => continue,
                    };
                    seeds.push(EigrpSeed { net: r.net, bandwidth, delay });
                }
            }
            seeds.truncate(MAX_SEEDS);
            if !seeds.is_empty() {
                out.eigrp.insert(d.id, seeds);
            }
        }
        if let Some(o) = d.ospf.as_ref().filter(|o| !o.redistribute.is_empty()) {
            let mut seeds: Vec<OspfSeed> = Vec::new();
            let mut done = Vec::new();
            for line in &o.redistribute {
                for (r, sets) in taken(d, line, table, &mut done) {
                    // OSPF never redistributes a default route: `default-information originate` does.
                    if r.net.prefix == 0 {
                        continue;
                    }
                    let fallback = if matches!(line.source, RedistSource::Bgp(_)) { 1 } else { 20 };
                    let metric = sets.metric.or(line.metric).or(o.default_metric).unwrap_or(fallback);
                    let type1 = sets.metric_type.or(line.metric_type) == Some(1);
                    if !conf::passes(d, &o.distribute, false, None, r.net, metric) {
                        continue;
                    }
                    seeds.push(OspfSeed { net: r.net, metric, type1 });
                }
            }
            seeds.truncate(MAX_SEEDS);
            if !seeds.is_empty() {
                out.ospf.insert(d.id, seeds);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lab_without_redistribution_has_no_seeds_and_costs_nothing() {
        let t = Topology::new();
        assert_eq!(seeds(&t), Seeds::default());
    }
}
