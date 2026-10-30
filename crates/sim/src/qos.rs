//! QoS on routed interfaces: classification, marking and policing.
//!
//! A service policy is applied where a routed packet enters an interface (input, after the
//! inbound access list) or leaves it (output, on the packet actually put on the wire: the ESP
//! packet when IPsec protects it, the inner packet on a tunnel interface). The packet's first
//! matching class decides; class-default takes the rest.
//!
//! - **Marking** (`set dscp`, `set ip precedence`) rewrites the IPv4 DS field and fixes the
//!   header checksum, so captures show it; routers forward the field unchanged and copy it to
//!   GRE and ESP outer headers, so later hops classify on it.
//! - **Policing** is a single-rate token bucket per class, per interface and direction, filled
//!   by simulated time at the committed rate. A packet (counted by its IP length) conforms when
//!   the bucket holds that many bytes; exceeding packets get the exceed action, and a dropped
//!   one carries the reason "policed by policy-map X class Y".
//! - **Queuing** (`priority`, `bandwidth`) on an output policy: the interface sends at its rate
//!   (a serial link at its clock rate, else the `bandwidth` configured, else the speed of its
//!   type), modelled as a fluid queue per class. Each packet first lets the queues drain for the
//!   time since the last one: the priority classes are served first, then the other classes share
//!   what is left by weight (their `bandwidth`; class-default the rest of the link). The packet
//!   then joins its class's queue and leaves when the bytes ahead of it at its share of the link
//!   have gone, so it reaches the cable that much later. A priority class may only queue what its
//!   rate sends in 200 ms (it is policed to its rate under congestion); every other queue holds at
//!   most 64 full-size packets. A packet that doesn't fit is tail-dropped with the reason.
//!   `shape` is kept and shown, not simulated.

use std::collections::BTreeMap;

use netlabcraft_core::DeviceId;
use netlabcraft_model::Device;
use netlabcraft_model::qos::{AppProto, CLASS_DEFAULT, ClassMap, Match, PoliceAction, PolicyMap, Rate};
use netlabcraft_wire::Ipv4;
use netlabcraft_wire::qos as ds;
/// DSCP names and numbers, for the console.
pub use netlabcraft_wire::qos::{dscp_name, parse_dscp};

use crate::Sim;

/// Most class counters kept per device (interfaces × directions × classes).
const MAX_COUNTERS: usize = 4_096;

/// One class of one applied policy: what it matched and did, and its policer's bucket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassStats {
    pub packets: u64,
    pub bytes: u64,
    /// Packets whose DS field this class rewrote.
    pub marked: u64,
    pub conformed: u64,
    pub exceeded: u64,
    /// Packets dropped by the policer.
    pub dropped: u64,
    /// Policer bucket, in millionths of a byte (so slow rates refill exactly).
    tokens: u128,
    last_us: u64,
    primed: bool,
    /// Output queuing: bytes waiting now (in millionths, to drain exactly), the most that ever
    /// waited, and the packets dropped because the queue was full.
    queued: u128,
    pub max_queued: u64,
    pub tail_drops: u64,
}

impl ClassStats {
    /// Bytes waiting in this class's queue.
    pub fn queued_bytes(&self) -> u64 {
        u64::try_from(self.queued / 1_000_000).unwrap_or(u64::MAX)
    }
}

/// Bytes a non-priority queue holds at most: 64 full-size packets.
const QUEUE_LIMIT: u128 = 64 * 1_500 * 1_000_000;

/// The rate an interface sends at (bit/s): a serial link's clock rate, else the configured
/// bandwidth, else the speed of its type.
pub fn link_rate_bps(d: &Device, iface: &str) -> u64 {
    let port = d.ports.iter().find(|p| p.name == iface);
    if let Some(c) = port.filter(|p| netlabcraft_model::ifname::is_serial(&p.name)).and_then(|p| p.serial.clock_rate) {
        return u64::from(c.max(1));
    }
    let kbit = port.and_then(|p| p.bandwidth).unwrap_or_else(|| crate::eigrp::default_metrics(iface).0);
    u64::from(kbit.max(1)) * 1_000
}

/// A rate as bit/s of a link.
fn rate_bps(r: Rate, link: u64) -> u64 {
    match r {
        Rate::Kbps(k) => u64::from(k) * 1_000,
        Rate::Percent(p) => link.saturating_mul(u64::from(p.min(100))) / 100,
    }
}

/// Counters by (interface, input?, policy, class).
pub(crate) type QosCounters = BTreeMap<(String, bool, String, String), ClassStats>;

/// The fields classification looks at.
struct Pkt {
    dscp: u8,
    ip: Ipv4,
    ports: Option<(u16, u16)>,
}

fn ports(ip: &Ipv4) -> Option<(u16, u16)> {
    if ip.protocol != netlabcraft_wire::PROTO_TCP && ip.protocol != netlabcraft_wire::PROTO_UDP {
        return None;
    }
    let b = ip.payload.get(..4)?;
    Some((u16::from_be_bytes([b.first().copied()?, b.get(1).copied()?]), u16::from_be_bytes([b.get(2).copied()?, b.get(3).copied()?])))
}

fn proto_matches(p: AppProto, pkt: &Pkt) -> bool {
    let (protos, well_known) = p.signature();
    if !protos.contains(&pkt.ip.protocol) {
        return false;
    }
    well_known.is_empty() || pkt.ports.is_some_and(|(s, d)| well_known.contains(&s) || well_known.contains(&d))
}

fn line_matches(d: &Device, m: &Match, pkt: &Pkt) -> bool {
    match m {
        Match::Any => true,
        Match::Dscp(v) => v.contains(&pkt.dscp),
        Match::Precedence(v) => v.contains(&(pkt.dscp >> 3)),
        Match::AccessGroup(name) => crate::vpn::acl_permits(d, name, &pkt.ip),
        Match::Protocol(p) => proto_matches(*p, pkt),
    }
}

/// Whether a class map takes a packet. A class map without match lines takes nothing.
fn class_matches(d: &Device, cm: &ClassMap, pkt: &Pkt) -> bool {
    if cm.matches.is_empty() {
        return false;
    }
    if cm.any { cm.matches.iter().any(|m| line_matches(d, m, pkt)) } else { cm.matches.iter().all(|m| line_matches(d, m, pkt)) }
}

/// The class a packet falls into under a policy: the first class whose class map takes it,
/// else class-default (whether or not the policy lists it).
fn classify<'a>(d: &Device, policy: &'a netlabcraft_model::qos::PolicyMap, pkt: &Pkt) -> &'a str {
    for c in &policy.classes {
        if c.class == CLASS_DEFAULT {
            return CLASS_DEFAULT;
        }
        if d.qos.class_map(&c.class).is_some_and(|cm| class_matches(d, cm, pkt)) {
            return &c.class;
        }
    }
    CLASS_DEFAULT
}

impl Sim {
    /// Runs the service policy on `iface` (input or output) over a datagram: the datagram as
    /// it continues (perhaps re-marked), or why it was dropped.
    pub(crate) fn qos_apply(&mut self, d: &Device, iface: &str, input: bool, datagram: Vec<u8>) -> Result<Vec<u8>, String> {
        let Some(policy) = d.qos.policy_on(iface, input) else { return Ok(datagram) };
        let (Ok(ip), Some(dscp)) = (Ipv4::decode(&datagram), ds::dscp(&datagram)) else { return Ok(datagram) };
        let ports = ports(&ip);
        let pkt = Pkt { dscp, ip, ports };
        let class = classify(d, policy, &pkt).to_string();
        let action = policy.classes.iter().find(|c| c.class == class).cloned();
        let size = u64::try_from(datagram.len()).unwrap_or(u64::MAX);
        let now = self.now;
        let st = self.st(d.id);
        let key = (iface.to_string(), input, policy.name.clone(), class.clone());
        if !st.qos.contains_key(&key) && st.qos.len() >= MAX_COUNTERS {
            st.qos.clear();
        }
        let stats = st.qos.entry(key).or_default();
        stats.packets = stats.packets.saturating_add(1);
        stats.bytes = stats.bytes.saturating_add(size);
        let action = action.unwrap_or_else(|| netlabcraft_model::qos::PolicyClass::new(&class));
        let mut mark = action.set_dscp.map(Mark::Dscp).or(action.set_precedence.map(Mark::Precedence));
        if let Some(pol) = action.police {
            let depth = u128::from(pol.depth()).saturating_mul(1_000_000);
            if !stats.primed {
                stats.primed = true;
                stats.tokens = depth;
            } else {
                let elapsed = u128::from(now.saturating_sub(stats.last_us));
                stats.tokens = stats.tokens.saturating_add(elapsed.saturating_mul(u128::from(pol.rate_bps)) / 8).min(depth);
            }
            stats.last_us = now;
            let need = u128::from(size).saturating_mul(1_000_000);
            let verdict = if stats.tokens >= need {
                stats.tokens -= need;
                stats.conformed = stats.conformed.saturating_add(1);
                pol.conform
            } else {
                stats.exceeded = stats.exceeded.saturating_add(1);
                pol.exceed
            };
            match verdict {
                PoliceAction::Drop => {
                    stats.dropped = stats.dropped.saturating_add(1);
                    return Err(format!(
                        "policed by policy-map {} class {class} ({} {iface})",
                        policy.name,
                        if input { "input on" } else { "output on" }
                    ));
                }
                PoliceAction::SetDscpTransmit(v) => mark = Some(Mark::Dscp(v)),
                PoliceAction::Transmit => {}
            }
        }
        let mut datagram = datagram;
        if !input && policy.classes.iter().any(|c| c.priority.is_some() || c.bandwidth.is_some()) {
            let delay = self.qos_enqueue(d, iface, policy, &class, size)?;
            self.tx_delay = delay;
        }
        let st = self.st(d.id);
        let Some(stats) = st.qos.get_mut(&(iface.to_string(), input, policy.name.clone(), class.clone())) else { return Ok(datagram) };
        if let Some(m) = mark {
            let done = match m {
                Mark::Dscp(v) => ds::set_dscp(&mut datagram, v),
                Mark::Precedence(v) => ds::set_precedence(&mut datagram, v),
            };
            if done {
                stats.marked = stats.marked.saturating_add(1);
            }
        }
        Ok(datagram)
    }

    /// Output queuing (see the module notes): drains the interface's queues up to now, then
    /// queues a packet of `size` bytes in `class`. How long it waits (µs), or why it was dropped.
    fn qos_enqueue(&mut self, d: &Device, iface: &str, policy: &PolicyMap, class: &str, size: u64) -> Result<u64, String> {
        let link = link_rate_bps(d, iface);
        let now = self.now;
        let st = self.st(d.id);
        let last = st.qos_drained.get(iface).copied().unwrap_or(now);
        if !st.qos_drained.contains_key(iface) && st.qos_drained.len() >= MAX_COUNTERS {
            st.qos_drained.clear();
        }
        st.qos_drained.insert(iface.to_string(), now);
        // Every class of the policy: (name, priority rate, weight).
        let explicit: u64 = policy.classes.iter().filter_map(|c| c.bandwidth).map(|r| rate_bps(r, link)).sum();
        let mut classes: Vec<(String, Option<u64>, u64)> = policy
            .classes
            .iter()
            .map(|c| {
                let weight = c.bandwidth.map(|r| rate_bps(r, link)).unwrap_or(if c.class == CLASS_DEFAULT {
                    link.saturating_sub(explicit).max(link / 100).max(1)
                } else {
                    0
                });
                (c.class.clone(), c.priority.map(|r| rate_bps(r, link)), weight)
            })
            .collect();
        if !classes.iter().any(|c| c.0 == CLASS_DEFAULT) {
            classes.push((CLASS_DEFAULT.to_string(), None, link.saturating_sub(explicit).max(link / 100).max(1)));
        }
        let key = |c: &str| (iface.to_string(), false, policy.name.clone(), c.to_string());
        // Drain: the capacity since the last packet, priority first, then by weight.
        let mut capacity = u128::from(now.saturating_sub(last)).saturating_mul(u128::from(link)) / 8;
        for (c, prio, _) in classes.iter().filter(|c| c.1.is_some()) {
            let _ = prio;
            if let Some(q) = st.qos.get_mut(&key(c)) {
                let take = q.queued.min(capacity);
                q.queued -= take;
                capacity -= take;
            }
        }
        for _ in 0..8 {
            let active: Vec<(String, u64)> = classes
                .iter()
                .filter(|c| c.1.is_none() && c.2 > 0 && st.qos.get(&key(&c.0)).is_some_and(|q| q.queued > 0))
                .map(|c| (c.0.clone(), c.2))
                .collect();
            let total: u128 = active.iter().map(|a| u128::from(a.1)).sum();
            if capacity == 0 || total == 0 {
                break;
            }
            let mut used = 0u128;
            for (c, w) in &active {
                let share = capacity.saturating_mul(u128::from(*w)) / total;
                if let Some(q) = st.qos.get_mut(&key(c)) {
                    let take = q.queued.min(share.max(1));
                    q.queued -= take;
                    used += take;
                }
            }
            capacity = capacity.saturating_sub(used);
        }
        // Queue the packet.
        let (prio, weight) = classes.iter().find(|c| c.0 == class).map_or((None, 1), |c| (c.1, c.2.max(1)));
        let ahead_prio: u128 = classes.iter().filter(|c| c.1.is_some()).filter_map(|c| st.qos.get(&key(&c.0))).map(|q| q.queued).sum();
        let active_weight: u128 = classes
            .iter()
            .filter(|c| c.1.is_none() && (c.0 == class || st.qos.get(&key(&c.0)).is_some_and(|q| q.queued > 0)))
            .map(|c| u128::from(c.2.max(1)))
            .sum::<u128>()
            .max(1);
        let Some(q) = st.qos.get_mut(&key(class)) else { return Ok(0) };
        let bytes = u128::from(size).saturating_mul(1_000_000);
        let limit = match prio {
            // What the priority rate sends in 200 ms (at least one packet).
            Some(r) => (u128::from(r) / 8 / 5).max(1_500).saturating_mul(1_000_000),
            None => QUEUE_LIMIT,
        };
        if q.queued.saturating_add(bytes) > limit {
            q.tail_drops = q.tail_drops.saturating_add(1);
            return Err(match prio {
                Some(_) => format!("tail drop: the priority queue of class {class} (policy-map {}) is over its rate on {iface}", policy.name),
                None => format!("tail drop: the queue of class {class} (policy-map {}) is full on {iface}", policy.name),
            });
        }
        q.queued = q.queued.saturating_add(bytes);
        q.max_queued = q.max_queued.max(q.queued_bytes());
        // Its wait: the priority bytes ahead, then its own queue at its share of the link.
        let wait_bytes = match prio {
            Some(_) => ahead_prio.saturating_add(bytes),
            None => ahead_prio.saturating_add(q.queued.saturating_mul(active_weight) / u128::from(weight)),
        };
        let wait_us = wait_bytes.saturating_mul(8) / u128::from(link.max(1));
        Ok(u64::try_from(wait_us).unwrap_or(u64::MAX).min(60_000_000))
    }

    /// One class's counters on an interface's input or output policy.
    pub fn qos_stats(&self, d: DeviceId, iface: &str, input: bool, policy: &str, class: &str) -> ClassStats {
        self.state.get(&d).and_then(|s| s.qos.get(&(iface.to_string(), input, policy.to_string(), class.to_string()))).cloned().unwrap_or_default()
    }

    /// Forgets the QoS counters and refills the policers (`clear counters`).
    pub fn clear_qos(&mut self, d: DeviceId) {
        if let Some(st) = self.state.get_mut(&d) {
            st.qos.clear();
        }
    }
}

#[derive(Clone, Copy)]
enum Mark {
    Dscp(u8),
    Precedence(u8),
}

/// Copies an inner packet's DS byte to the outer header that carries it (GRE, ESP), as
/// tunnel endpoints do so the path in between can classify it (RFC 2983).
pub(crate) fn copy_ds(inner: &[u8], outer: &mut [u8]) {
    if let Some(b) = ds::ds_byte(inner).filter(|b| *b != 0) {
        ds::set_ds_byte(outer, b);
    }
}

/// The DS byte to keep when a datagram is decoded and encoded again on its way through.
pub(crate) fn keep_ds(original: &[u8], mut encoded: Vec<u8>) -> Vec<u8> {
    if let Some(b) = ds::ds_byte(original).filter(|b| *b != 0) {
        ds::set_ds_byte(&mut encoded, b);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use netlabcraft_model::qos::{PolicyClass, ServicePolicy};
    use netlabcraft_model::{DeviceKind, Topology};

    /// A router whose Gi0/1 sends at 64 kbit/s with a voice class (priority 16 kbit/s), a data
    /// class (bandwidth 32 kbit/s) and class-default.
    fn router() -> Device {
        let mut t = Topology::new();
        let r = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let mut d = t.device(r).unwrap().clone();
        let gi = d.ports.iter().position(|p| p.name == "GigabitEthernet0/1").unwrap();
        d.ports[gi].bandwidth = Some(64);
        let q = &mut d.qos;
        q.class_maps.push(ClassMap { name: "VOICE".into(), any: false, matches: vec![Match::Dscp(vec![46])] });
        q.class_maps.push(ClassMap { name: "DATA".into(), any: false, matches: vec![Match::Dscp(vec![10])] });
        let mut voice = PolicyClass::new("VOICE");
        voice.priority = Some(Rate::Kbps(16));
        let mut data = PolicyClass::new("DATA");
        data.bandwidth = Some(Rate::Kbps(32));
        q.policy_maps.push(PolicyMap { name: "WAN".into(), classes: vec![voice, data] });
        q.applied.insert("GigabitEthernet0/1".into(), ServicePolicy { input: None, output: Some("WAN".into()) });
        d
    }

    fn packet(dscp: u8) -> Vec<u8> {
        let ip = Ipv4 {
            id: 1,
            ttl: 64,
            protocol: netlabcraft_wire::PROTO_UDP,
            src: "10.0.0.1".parse().unwrap(),
            dst: "10.0.0.2".parse().unwrap(),
            payload: vec![0; 980],
        };
        let mut v = ip.encode();
        ds::set_dscp(&mut v, dscp);
        v
    }

    #[test]
    fn a_congested_link_queues_by_class_and_tail_drops() {
        let d = router();
        assert_eq!(link_rate_bps(&d, "GigabitEthernet0/1"), 64_000);
        let mut sim = Sim::new();
        let out = "GigabitEthernet0/1";
        assert_eq!(packet(10).len(), 1_000);
        // A burst of 100 data packets of 1000 bytes at once: each waits longer (125 ms per
        // packet at 64 kbit/s), and the queue fills at 96 000 bytes.
        let mut waits = Vec::new();
        let mut drops = 0;
        for _ in 0..100 {
            match sim.qos_apply(&d, out, false, packet(10)) {
                Ok(_) => waits.push(sim.tx_delay),
                Err(e) => {
                    assert!(e.starts_with("tail drop: the queue of class DATA"), "{e}");
                    drops += 1;
                }
            }
        }
        assert_eq!(waits.len(), 96);
        assert_eq!(drops, 4);
        assert!(waits.windows(2).all(|w| w[0] < w[1]), "{waits:?}");
        // Alone on the link, the data class has all of it: the first waits 125 ms.
        assert_eq!(waits[0], 125_000);
        let st = sim.qos_stats(d.id, out, false, "WAN", "DATA");
        assert_eq!((st.tail_drops, st.queued_bytes(), st.max_queued), (4, 96_000, 96_000));
        // Voice goes first however long the data queue is, but only what 16 kbit/s sends in
        // 200 ms (at least one packet) may wait.
        assert!(sim.qos_apply(&d, out, false, packet(46)).is_ok());
        assert_eq!(sim.tx_delay, 125_000);
        let e = sim.qos_apply(&d, out, false, packet(46)).unwrap_err();
        assert!(e.contains("priority queue of class VOICE"), "{e}");
        // Time drains the queues: priority first, then data.
        sim.now += 2_000_000;
        assert!(sim.qos_apply(&d, out, false, packet(0)).is_ok());
        let voice = sim.qos_stats(d.id, out, false, "WAN", "VOICE");
        assert_eq!(voice.queued_bytes(), 0);
        let data = sim.qos_stats(d.id, out, false, "WAN", "DATA");
        // 2 s at 64 kbit/s is 16 000 bytes: 1000 of voice, the rest of data.
        assert_eq!(data.queued_bytes(), 96_000 - 15_000);
    }
}
