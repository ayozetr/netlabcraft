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
//! - **Queuing** (`priority`, `bandwidth`, `shape`) is kept in the configuration and its class
//!   counted, but the simulator has no link congestion model: every frame crosses its cable at
//!   once, so no queue ever builds and no queuing delay is invented.

use std::collections::BTreeMap;

use packetcraft_core::DeviceId;
use packetcraft_model::Device;
use packetcraft_model::qos::{AppProto, CLASS_DEFAULT, ClassMap, Match, PoliceAction};
use packetcraft_wire::Ipv4;
use packetcraft_wire::qos as ds;
/// DSCP names and numbers, for the console.
pub use packetcraft_wire::qos::{dscp_name, parse_dscp};

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
    if ip.protocol != packetcraft_wire::PROTO_TCP && ip.protocol != packetcraft_wire::PROTO_UDP {
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
fn classify<'a>(d: &Device, policy: &'a packetcraft_model::qos::PolicyMap, pkt: &Pkt) -> &'a str {
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
        let Some(action) = action else { return Ok(datagram) };
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
