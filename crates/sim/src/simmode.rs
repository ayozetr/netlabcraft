//! Simulation mode's options and records that are not about any one protocol:
//!
//! - **Constant delay**: every hop takes the same fixed simulated time, whatever queues would
//!   add, so the event list's times read as whole steps (for teaching).
//! - **Capture filter**: only events of the chosen protocols are recorded in the event list
//!   (and so in the pcap capture); the others still happen, they just aren't kept.
//! - **The origin device's decisions**: a packet a device raises itself (a ping, a PDU, a
//!   protocol message) records the sender's routing lookup and ARP decision on the first frame
//!   it puts on the wire, as receivers record theirs (see [`crate::explain`]).

use std::collections::{BTreeMap, VecDeque};
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;

use crate::explain::{MAX_STEPS, Step};
use crate::{L3If, LINK_DELAY_US, Sim, TraceEvent};

/// The hop time constant delay uses unless told otherwise (1 ms).
pub const DEFAULT_CONSTANT_DELAY_US: u64 = 1_000;
/// The longest constant hop time (1 s).
pub const MAX_CONSTANT_DELAY_US: u64 = 1_000_000;
/// Most protocols a capture filter names.
pub const MAX_CAPTURE_PROTOCOLS: usize = 64;
/// Most origin records waiting for their frame's delivery.
const MAX_ORIGINS: usize = 256;
/// Most deliveries a PDU watches for its answer.
const MAX_WATCHED: usize = 512;

/// Simulation-mode options and bookkeeping.
#[derive(Debug, Clone, Default)]
pub(crate) struct SimMode {
    /// Every hop takes this long (µs), when set.
    pub(crate) constant_delay: Option<u64>,
    /// Only events of these protocols (by name, upper case) are kept, when set.
    pub(crate) capture: Option<Vec<String>>,
    /// The sender's decisions while it raises a packet itself.
    pub(crate) origin: Option<Vec<Step>>,
    /// Decisions waiting for the queued frame (by its queue sequence number) to be delivered.
    pub(crate) origin_for: BTreeMap<u64, Vec<Step>>,
    /// Deliveries to the device sending a PDU, kept whatever the capture filter says.
    pub(crate) watch: Option<(DeviceId, VecDeque<TraceEvent>)>,
}

impl SimMode {
    /// The options to keep across a simulation reset (the bookkeeping goes).
    pub(crate) fn options(&self) -> SimMode {
        SimMode { constant_delay: self.constant_delay, capture: self.capture.clone(), ..SimMode::default() }
    }
}

impl Sim {
    /// Turns constant delay on (every hop takes `us` µs, clamped to 1 µs-1 s) or off.
    pub fn set_constant_delay(&mut self, us: Option<u64>) {
        self.mode.constant_delay = us.map(|u| u.clamp(1, MAX_CONSTANT_DELAY_US));
    }

    /// The constant hop time, when constant delay is on.
    pub fn constant_delay(&self) -> Option<u64> {
        self.mode.constant_delay
    }

    /// How long the next hop takes: the constant delay, or the link's own time.
    pub(crate) fn hop_delay(&self) -> u64 {
        self.mode.constant_delay.unwrap_or(LINK_DELAY_US)
    }

    /// The hop time for a frame that also waited `queued` µs in an output queue: constant
    /// delay leaves the wait out.
    pub(crate) fn hop_delay_after(&self, queued: u64) -> u64 {
        match self.mode.constant_delay {
            Some(c) => c,
            None => LINK_DELAY_US.saturating_add(queued),
        }
    }

    /// Records only events of these protocols from now on (names as [`netlabcraft_wire::Protocol::name`]
    /// gives them, any case), or everything (`None`).
    pub fn set_capture_filter(&mut self, protocols: Option<Vec<String>>) {
        self.mode.capture = protocols.map(|v| {
            let mut v: Vec<String> = v.into_iter().map(|p| p.trim().to_ascii_uppercase()).filter(|p| !p.is_empty()).collect();
            v.sort();
            v.dedup();
            v.truncate(MAX_CAPTURE_PROTOCOLS);
            v
        });
    }

    /// The protocols recorded, when a capture filter is on.
    pub fn capture_filter(&self) -> Option<&[String]> {
        self.mode.capture.as_deref()
    }

    /// Whether an event of this protocol is recorded.
    pub fn captures(&self, protocol: netlabcraft_wire::Protocol) -> bool {
        self.mode.capture.as_ref().is_none_or(|c| c.iter().any(|p| p.eq_ignore_ascii_case(protocol.name())))
    }

    /// Drops the recorded events the capture filter would not have kept. Returns how many.
    pub fn prune_to_capture_filter(&mut self) -> usize {
        let before = self.trace.len();
        if self.mode.capture.is_some() {
            let keep: Vec<TraceEvent> = std::mem::take(&mut self.trace).into_iter().filter(|e| self.captures(e.protocol)).collect();
            self.trace = keep.into();
        }
        before.saturating_sub(self.trace.len())
    }

    /// After an event was handled: kept for a watching PDU, and dropped from the list when the
    /// capture filter leaves its protocol out.
    pub(crate) fn after_delivery(&mut self) {
        let Some(last) = self.trace.back() else { return };
        if let Some((dev, seen)) = &mut self.mode.watch
            && last.to.device == *dev
        {
            if seen.len() >= MAX_WATCHED {
                seen.pop_front();
            }
            seen.push_back(last.clone());
        }
        if !self.captures(last.protocol) {
            self.trace.pop_back();
        }
    }

    /// Starts recording the decisions of a device raising a packet itself. Returns whether it
    /// did (not while a received frame is being handled: those decisions are the receiver's).
    pub(crate) fn begin_origin(&mut self) -> bool {
        if self.explaining.is_some() || self.mode.origin.is_some() {
            return false;
        }
        self.mode.origin = Some(Vec::new());
        true
    }

    /// Stops recording what [`Sim::begin_origin`] started.
    pub(crate) fn end_origin(&mut self, started: bool) {
        if started {
            self.mode.origin = None;
        }
    }

    /// A frame queued now: the sender's decisions so far go with it.
    pub(crate) fn origin_queued(&mut self, seq: u64) {
        if self.cause.is_some() {
            return;
        }
        let Some(steps) = self.mode.origin.as_mut().filter(|v| !v.is_empty()).map(std::mem::take) else { return };
        if self.mode.origin_for.len() >= MAX_ORIGINS {
            self.mode.origin_for.pop_first();
        }
        self.mode.origin_for.insert(seq, steps);
    }

    /// The sender's decisions for the frame with queue sequence number `seq`, when it had any.
    pub(crate) fn origin_of(&mut self, seq: u64) -> Vec<Step> {
        self.mode.origin_for.remove(&seq).unwrap_or_default()
    }

    /// Records a sender's decision (outside a delivery).
    pub(crate) fn note_origin(&mut self, step: Step) -> bool {
        match &mut self.mode.origin {
            Some(v) => {
                if v.len() < MAX_STEPS {
                    v.push(step);
                }
                true
            }
            None => false,
        }
    }

    /// The sender's routing decision for a packet it raises.
    pub(crate) fn note_sends(&mut self, dst: Ipv4Addr, out: &L3If, nh: Ipv4Addr, ttl: u8) {
        if self.mode.origin.is_some() {
            self.note_origin(Step::Sends { dst, out: out.name.clone(), via: (nh != dst).then_some(nh), ttl });
        }
    }

    /// Starts keeping the deliveries to `dev` (a PDU waiting for its answer).
    pub(crate) fn watch(&mut self, dev: DeviceId) {
        self.mode.watch = Some((dev, VecDeque::new()));
    }

    pub(crate) fn unwatch(&mut self) {
        self.mode.watch = None;
    }

    /// The deliveries kept since [`Sim::watch`], with ids from `since` on.
    pub(crate) fn watched(&self, since: u64) -> impl Iterator<Item = &TraceEvent> {
        self.mode.watch.iter().flat_map(|(_, v)| v.iter()).filter(move |e| e.id >= since)
    }
}

#[cfg(test)]
mod tests {
    use netlabcraft_model::{End, Topology, models};
    use netlabcraft_wire::Protocol;

    use crate::{PingOptions, Probe, Sim};

    /// PC1 - S1 - R1 - PC2, two subnets.
    fn lab() -> (Topology, netlabcraft_core::DeviceId) {
        let mut t = Topology::new();
        let pc1 = t.add_model(models::find("PC").unwrap(), Some("PC1"), 0.0, 0.0).unwrap();
        let sw = t.add_model(models::find("Switch-24").unwrap(), Some("S1"), 100.0, 0.0).unwrap();
        let r = t.add_model(models::find("Router-2").unwrap(), Some("R1"), 200.0, 0.0).unwrap();
        let pc2 = t.add_model(models::find("PC").unwrap(), Some("PC2"), 300.0, 0.0).unwrap();
        t.connect(End { device: pc1, port: 0 }, End { device: sw, port: 0 }).unwrap();
        t.connect(End { device: sw, port: 1 }, End { device: r, port: 0 }).unwrap();
        t.connect(End { device: r, port: 1 }, End { device: pc2, port: 0 }).unwrap();
        let d = t.device_mut(pc1).unwrap();
        d.ports[0].ip = Some("10.0.1.10/24".parse().unwrap());
        d.gateway = Some("10.0.1.1".parse().unwrap());
        let d = t.device_mut(pc2).unwrap();
        d.ports[0].ip = Some("10.0.2.10/24".parse().unwrap());
        d.gateway = Some("10.0.2.1".parse().unwrap());
        let d = t.device_mut(r).unwrap();
        d.ports[0].ip = Some("10.0.1.1/24".parse().unwrap());
        d.ports[0].shutdown = false;
        d.ports[1].ip = Some("10.0.2.1/24".parse().unwrap());
        d.ports[1].shutdown = false;
        (t, pc1)
    }

    #[test]
    fn constant_delay_makes_every_hop_the_same_length() {
        let (t, pc1) = lab();
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.set_constant_delay(Some(5_000));
        assert_eq!(sim.constant_delay(), Some(5_000));
        // (The first echo may be lost while R1 resolves PC2, as on a real router.)
        let r = sim.ping(&t, pc1, "10.0.2.10".parse().unwrap(), PingOptions { count: 3, ..PingOptions::default() }).unwrap();
        assert!(r.iter().skip(1).all(|p| matches!(p, Probe::Reply { .. })), "{r:?}");
        // Each frame took exactly 5 ms from the moment its sender handled the frame that
        // caused it.
        let tr = sim.trace();
        let caused: Vec<(u64, u64)> =
            tr.iter().filter_map(|e| e.cause.and_then(|c| tr.iter().find(|x| x.id == c)).map(|c| (c.at_us, e.at_us))).collect();
        assert!(caused.len() > 4);
        assert!(caused.iter().all(|(a, b)| b - a == 5_000), "{caused:?}");
        // A round trip: 6 hops of 5 ms.
        let Some(Probe::Reply { rtt_us, .. }) = r.get(2) else { panic!() };
        assert_eq!(*rtt_us, 6 * 5_000);
        // Off again: the link's own time; a reset keeps the option.
        sim.reset();
        assert_eq!(sim.constant_delay(), Some(5_000));
        sim.set_constant_delay(None);
        sim.topology_changed(&t);
        let r = sim.ping(&t, pc1, "10.0.2.10".parse().unwrap(), PingOptions { count: 2, ..PingOptions::default() }).unwrap();
        let Some(Probe::Reply { rtt_us, .. }) = r.get(1) else { panic!() };
        assert_eq!(*rtt_us, 6 * crate::LINK_DELAY_US);
        sim.set_constant_delay(Some(u64::MAX));
        assert_eq!(sim.constant_delay(), Some(super::MAX_CONSTANT_DELAY_US));
    }

    #[test]
    fn a_capture_filter_keeps_only_its_protocols_and_the_pcap_follows() {
        let (t, pc1) = lab();
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.set_capture_filter(Some(vec!["icmp".into()]));
        let r = sim.ping(&t, pc1, "10.0.2.10".parse().unwrap(), PingOptions { count: 3, ..PingOptions::default() }).unwrap();
        // ARP happened (the pings got through) but was not recorded.
        assert!(r.iter().skip(1).all(|p| matches!(p, Probe::Reply { .. })), "{r:?}");
        assert!(sim.trace().len() >= 12, "two echoes and two replies, three hops each");
        assert!(sim.trace().iter().all(|e| e.protocol == Protocol::Icmp));
        let pcap = sim.pcap();
        // 24-byte header, then 16 bytes per record plus the frame.
        let bytes: usize = sim.trace().iter().map(|e| 16 + e.frame.len()).sum();
        assert_eq!(pcap.len(), 24 + bytes);
        // Everything again; pruning a broader list keeps what matches.
        sim.set_capture_filter(None);
        sim.ping(&t, pc1, "10.0.2.10".parse().unwrap(), PingOptions { count: 1, ..PingOptions::default() }).unwrap();
        sim.reset();
        sim.topology_changed(&t);
        sim.ping(&t, pc1, "10.0.2.10".parse().unwrap(), PingOptions { count: 1, ..PingOptions::default() }).unwrap();
        assert!(sim.trace().iter().any(|e| e.protocol == Protocol::Arp));
        sim.set_capture_filter(Some(vec!["ARP".into()]));
        let removed = sim.prune_to_capture_filter();
        assert!(removed > 0 && sim.trace().iter().all(|e| e.protocol == Protocol::Arp));
        assert_eq!(sim.capture_filter(), Some(&["ARP".to_string()][..]));
    }

    #[test]
    fn the_sender_records_its_own_routing_and_arp_decisions() {
        let (t, pc1) = lab();
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        sim.ping(&t, pc1, "10.0.2.10".parse().unwrap(), PingOptions { count: 3, ..PingOptions::default() }).unwrap();
        let from_pc: Vec<&crate::TraceEvent> = sim.trace().iter().filter(|e| e.from.device == pc1 && e.cause.is_none()).collect();
        // The first frame (an ARP request for the gateway) carries the lookup and the ARP miss;
        // the second echo goes straight out with the gateway's MAC known.
        let first = from_pc.first().unwrap();
        assert_eq!(first.protocol, Protocol::Arp);
        let text: Vec<String> = first.origin.iter().map(crate::explain::Step::describe).collect();
        assert!(text.iter().any(|s| s.contains("The device sends a packet to 10.0.2.10: its routing table gives next hop 10.0.1.1")), "{text:?}");
        assert!(text.iter().any(|s| s.contains("10.0.1.1 is not in the ARP table")), "{text:?}");
        let echo = from_pc.iter().find(|e| e.protocol == Protocol::Icmp).unwrap();
        assert!(echo.origin.iter().any(|s| matches!(s, crate::explain::Step::ArpKnown { .. })), "{:?}", echo.origin);
        // Receivers keep theirs, and nothing they forwarded carries sender decisions.
        assert!(sim.trace().iter().filter(|e| e.cause.is_some()).all(|e| e.origin.is_empty()));
        // The OSI view shows the sender's side.
        let osi = crate::explain::osi(&t, &sim, echo);
        assert!(osi.origin.iter().flatten().any(|l| l.contains("routing table gives next hop")), "{:?}", osi.origin);
        assert!(osi.origin.get(2).is_some_and(|l| l.iter().any(|x| x.contains("IP packet from 10.0.1.10 to 10.0.2.10"))));
    }
}
