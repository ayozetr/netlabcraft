//! NetFlow: routers count the packets they forward into flows (per interface, `ip flow ingress`),
//! age the flows out and export them as real UDP datagrams (`ip flow-export destination`) in
//! NetFlow version 5 or 9; a Netflow Collector on an end device listens, decodes and lists them.
//!
//! The rules are fixed so a lab replays: a flow is exported when it has been quiet for
//! [`INACTIVE_MS`] or has run for [`ACTIVE_MS`] (shorter than a router's usual half hour, so a
//! long flow shows up in a lab), at most [`MAX_DATAGRAMS`] datagrams per destination each time the
//! simulator advances, and the cache holds at most [`MAX_FLOWS`] flows (a new flow that does not
//! fit is counted in `dropped` and not kept).

use std::collections::{BTreeMap, VecDeque};
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::netflow::{self, FlowRecord, MAX_RECORDS_V5, MAX_RECORDS_V9};
use netlabcraft_wire::{Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP};

use crate::{L3If, Sim};

/// Most flows a router's cache holds.
pub const MAX_FLOWS: usize = 256;
/// Most flows a collector keeps (the oldest go first).
pub const MAX_COLLECTED: usize = 2_000;
/// A flow quiet for this long is exported (simulated milliseconds).
pub const INACTIVE_MS: u64 = 15_000;
/// A flow that has run for this long is exported while it carries on.
pub const ACTIVE_MS: u64 = 60_000;
/// Most export datagrams per destination each time the simulator advances.
pub const MAX_DATAGRAMS: usize = 4;
const EXPORT_SRC_PORT: u16 = 49_520;

/// One flow in a router's cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    pub rec: FlowRecord,
}

/// A router's flow cache and export counters.
#[derive(Debug, Clone, Default)]
pub struct FlowCache {
    pub flows: Vec<Flow>,
    /// Flows that left the cache (exported or aged out with nowhere to go).
    pub aged: u64,
    /// New flows that did not fit in the cache.
    pub dropped: u64,
    /// Flows and datagrams exported, and the sequence number the next datagram carries.
    pub exported_flows: u64,
    pub exported_datagrams: u64,
    pub sequence: u32,
    /// Packets counted since the cache was last cleared.
    pub packets: u64,
}

/// One record a collector received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collected {
    pub at_us: u64,
    pub exporter: Ipv4Addr,
    pub version: u8,
    pub rec: FlowRecord,
}

/// What a collector has received.
#[derive(Debug, Clone, Default)]
pub struct Collector {
    pub records: VecDeque<Collected>,
    pub datagrams: u64,
    /// Datagrams that did not decode.
    pub bad: u64,
}

/// Flows, packets and bytes added up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub flows: u64,
    pub packets: u64,
    pub bytes: u64,
}

impl Tally {
    fn add(&mut self, r: &FlowRecord) {
        self.flows = self.flows.saturating_add(1);
        self.packets = self.packets.saturating_add(u64::from(r.packets));
        self.bytes = self.bytes.saturating_add(u64::from(r.octets));
    }
}

/// What a collector has received, added up by source address and by protocol (biggest first by
/// bytes, ties by address or protocol number) and in all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub talkers: Vec<(Ipv4Addr, Tally)>,
    pub protocols: Vec<(u8, Tally)>,
    pub total: Tally,
}

/// The name of an IP protocol number the way the lists show it.
pub fn protocol_name(p: u8) -> String {
    match p {
        1 => "icmp".into(),
        6 => "tcp".into(),
        17 => "udp".into(),
        n => format!("ip-{n}"),
    }
}

impl Collector {
    /// Top talkers, protocols and totals of the records held.
    pub fn summary(&self) -> Summary {
        let mut talkers: BTreeMap<Ipv4Addr, Tally> = BTreeMap::new();
        let mut protocols: BTreeMap<u8, Tally> = BTreeMap::new();
        let mut total = Tally::default();
        for c in &self.records {
            talkers.entry(c.rec.src).or_default().add(&c.rec);
            protocols.entry(c.rec.protocol).or_default().add(&c.rec);
            total.add(&c.rec);
        }
        let mut talkers: Vec<(Ipv4Addr, Tally)> = talkers.into_iter().collect();
        talkers.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(&b.0)));
        let mut protocols: Vec<(u8, Tally)> = protocols.into_iter().collect();
        protocols.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(&b.0)));
        Summary { talkers, protocols, total }
    }
}

/// The source and destination ports of a TCP or UDP datagram, the ICMP type and code as the
/// destination port, and the TCP flags.
fn ports(ip: &Ipv4) -> (u16, u16, u8) {
    let p = &ip.payload;
    let at = |i: usize| p.get(i).copied().unwrap_or(0);
    match ip.protocol {
        PROTO_TCP => (u16::from_be_bytes([at(0), at(1)]), u16::from_be_bytes([at(2), at(3)]), at(13)),
        PROTO_UDP => (u16::from_be_bytes([at(0), at(1)]), u16::from_be_bytes([at(2), at(3)]), 0),
        PROTO_ICMP => (0, u16::from_be_bytes([at(0), at(1)]), 0),
        _ => (0, 0, 0),
    }
}

fn ms(us: u64) -> u32 {
    u32::try_from(us / 1_000).unwrap_or(u32::MAX)
}

fn if_index(d: &Device, ifc: &L3If) -> u16 {
    if ifc.port < d.ports.len() { u16::try_from(ifc.port + 1).unwrap_or(0) } else { 0 }
}

impl Sim {
    /// Counts one forwarded packet into the router's flow cache when `ifc` counts flows.
    pub(crate) fn netflow_note(&mut self, d: &Device, ifc: &L3If, out: &L3If, next_hop: Ipv4Addr, ip: &Ipv4, len: usize) {
        if d.kind != DeviceKind::Router || !d.netflow.counts(&ifc.name) {
            return;
        }
        let (src_port, dst_port, flags) = ports(ip);
        let now = ms(self.now);
        let input = if_index(d, ifc);
        let octets = u32::try_from(len).unwrap_or(u32::MAX);
        let (src, dst, protocol) = (ip.src, ip.dst, ip.protocol);
        let output = if_index(d, out);
        let cache = &mut self.st(d.id).nf;
        cache.packets = cache.packets.saturating_add(1);
        if let Some(f) = cache.flows.iter_mut().find(|f| {
            let r = &f.rec;
            r.src == src && r.dst == dst && r.protocol == protocol && r.src_port == src_port && r.dst_port == dst_port && r.input == input
        }) {
            f.rec.packets = f.rec.packets.saturating_add(1);
            f.rec.octets = f.rec.octets.saturating_add(octets);
            f.rec.last = now;
            f.rec.tcp_flags |= flags;
        } else if cache.flows.len() < MAX_FLOWS {
            cache.flows.push(Flow {
                rec: FlowRecord {
                    src,
                    dst,
                    next_hop,
                    input,
                    output,
                    packets: 1,
                    octets,
                    first: now,
                    last: now,
                    src_port,
                    dst_port,
                    tcp_flags: flags,
                    protocol,
                    tos: 0,
                },
            });
        } else {
            cache.dropped = cache.dropped.saturating_add(1);
        }
    }

    /// Ages flows out of every router's cache and exports them to the router's destinations.
    pub(crate) fn flush_netflow(&mut self, t: &Topology) {
        let now = u64::from(ms(self.now));
        let ids: Vec<DeviceId> = self.state.iter().filter(|(_, s)| !s.nf.flows.is_empty()).map(|(id, _)| *id).collect();
        for id in ids {
            let Some(d) = t.device(id).cloned() else { continue };
            let v9 = d.netflow.export_version() == 9;
            let per = if v9 { MAX_RECORDS_V9 } else { MAX_RECORDS_V5 };
            let limit = per.saturating_mul(MAX_DATAGRAMS);
            let mut expired: Vec<FlowRecord> = Vec::new();
            {
                let cache = &mut self.st(id).nf;
                let mut keep = Vec::with_capacity(cache.flows.len());
                for f in std::mem::take(&mut cache.flows) {
                    let quiet = now.saturating_sub(u64::from(f.rec.last)) >= INACTIVE_MS;
                    let long = now.saturating_sub(u64::from(f.rec.first)) >= ACTIVE_MS;
                    if (quiet || long) && expired.len() < limit {
                        expired.push(f.rec);
                    } else {
                        keep.push(f);
                    }
                }
                cache.flows = keep;
                cache.aged = cache.aged.saturating_add(expired.len() as u64);
            }
            if expired.is_empty() || d.netflow.destinations.is_empty() {
                continue;
            }
            let unix = u32::try_from(self.clock(id).0.max(0)).unwrap_or(u32::MAX);
            for dest in &d.netflow.destinations {
                let Ok(src) = self.source_for(t, &d, dest.addr) else { continue };
                for chunk in expired.chunks(per) {
                    let seq = self.st(id).nf.sequence;
                    let bytes = if v9 {
                        netflow::encode_v9(ms(self.now), unix, seq, id.0, chunk)
                    } else {
                        netflow::encode_v5(ms(self.now), unix, seq, chunk)
                    };
                    let _ = self.udp_out(t, &d, src, dest.addr, EXPORT_SRC_PORT, dest.port, bytes);
                    let cache = &mut self.st(id).nf;
                    cache.sequence = cache.sequence.wrapping_add(if v9 { 1 } else { chunk.len() as u32 });
                    cache.exported_datagrams = cache.exported_datagrams.saturating_add(1);
                    cache.exported_flows = cache.exported_flows.saturating_add(chunk.len() as u64);
                }
            }
        }
    }

    /// A Netflow Collector takes a datagram: decodes it and keeps its records.
    pub(crate) fn netflow_collect(&mut self, d: &Device, exporter: Ipv4Addr, payload: &[u8]) -> Result<(), String> {
        let now = self.now;
        let col = &mut self.st(d.id).nf_in;
        match netflow::decode(payload) {
            Ok(g) => {
                col.datagrams = col.datagrams.saturating_add(1);
                let version = u8::try_from(g.version).unwrap_or(0);
                for rec in g.records {
                    if col.records.len() >= MAX_COLLECTED {
                        col.records.pop_front();
                    }
                    col.records.push_back(Collected { at_us: now, exporter, version, rec });
                }
                Ok(())
            }
            Err(e) => {
                col.bad = col.bad.saturating_add(1);
                Err(format!("not a NetFlow datagram: {e}"))
            }
        }
    }

    /// A router's flow cache.
    pub fn netflow_cache(&self, d: DeviceId) -> Option<&FlowCache> {
        self.state.get(&d).map(|s| &s.nf)
    }

    /// What a collector has received.
    pub fn netflow_collected(&self, d: DeviceId) -> Option<&Collector> {
        self.state.get(&d).map(|s| &s.nf_in)
    }

    /// `clear ip flow stats`: empties a router's cache and counters (the sequence number goes on).
    pub fn clear_netflow_cache(&mut self, d: DeviceId) {
        if let Some(s) = self.state.get_mut(&d) {
            let sequence = s.nf.sequence;
            s.nf = FlowCache { sequence, ..FlowCache::default() };
        }
    }

    /// Empties what a collector has received.
    pub fn clear_netflow_collected(&mut self, d: DeviceId) {
        if let Some(s) = self.state.get_mut(&d) {
            s.nf_in = Collector::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collected(src: [u8; 4], protocol: u8, packets: u32, octets: u32) -> Collected {
        let z = Ipv4Addr::UNSPECIFIED;
        Collected {
            at_us: 0,
            exporter: z,
            version: 5,
            rec: FlowRecord {
                src: Ipv4Addr::from(src),
                dst: z,
                next_hop: z,
                input: 0,
                output: 0,
                packets,
                octets,
                first: 0,
                last: 0,
                src_port: 0,
                dst_port: 0,
                tcp_flags: 0,
                protocol,
                tos: 0,
            },
        }
    }

    #[test]
    fn the_summary_adds_up_by_source_and_protocol_biggest_first() {
        let mut c = Collector::default();
        for x in [
            collected([10, 0, 0, 2], 6, 3, 300),
            collected([10, 0, 0, 1], 17, 1, 900),
            collected([10, 0, 0, 2], 6, 2, 200),
            collected([10, 0, 0, 3], 1, 1, 60),
        ] {
            c.records.push_back(x);
        }
        let s = c.summary();
        assert_eq!(s.total, Tally { flows: 4, packets: 7, bytes: 1_460 });
        let order: Vec<Ipv4Addr> = s.talkers.iter().map(|(a, _)| *a).collect();
        assert_eq!(order, vec![Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2), Ipv4Addr::new(10, 0, 0, 3)]);
        assert_eq!(s.talkers.get(1).map(|(_, t)| *t), Some(Tally { flows: 2, packets: 5, bytes: 500 }));
        assert_eq!(s.protocols.iter().map(|(p, _)| *p).collect::<Vec<_>>(), vec![17, 6, 1]);
        assert_eq!(protocol_name(6), "tcp");
        assert_eq!(protocol_name(47), "ip-47");
        assert_eq!(Collector::default().summary(), Summary::default());
    }

    #[test]
    fn the_cache_clear_keeps_the_sequence_number() {
        let mut sim = Sim::new();
        sim.st(DeviceId(1)).nf.sequence = 90;
        sim.st(DeviceId(1)).nf.packets = 5;
        sim.clear_netflow_cache(DeviceId(1));
        let c = sim.netflow_cache(DeviceId(1)).cloned().unwrap_or_default();
        assert_eq!((c.sequence, c.packets), (90, 0));
    }
}
