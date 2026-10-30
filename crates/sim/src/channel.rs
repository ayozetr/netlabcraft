//! EtherChannel: switch ports bundled into one logical link (IEEE 802.1AX for LACP).
//!
//! Negotiation is computed, like spanning tree: from the configured modes at both ends of each
//! cable, the ports that bundle are known at once. A bundle behaves as one port: it learns
//! addresses, floods and takes a spanning-tree role once, and spreads traffic over its members
//! by source and destination address.

use std::collections::{BTreeMap, BTreeSet};

use packetcraft_core::MacAddr;
use packetcraft_model::{ChannelGroup, Device, DeviceKind, End, Topology};

use packetcraft_wire::lacp::{self, LacpInfo, Lacpdu};

use crate::port_up;

/// A member port's state in its channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Carries the channel's traffic.
    Bundled,
    /// The other end didn't agree to bundle: the port works on its own.
    Individual,
    /// Can't join (other end not `on`, a different neighbour or settings): passes nothing.
    Suspended,
    /// Down or not cabled.
    Down,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Bundled => "bundled",
            Status::Individual => "on its own",
            Status::Suspended => "suspended",
            Status::Down => "down",
        }
    }
}

/// One `Port-channelN` of a switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub number: u16,
    /// Every port configured in the group, by index, with its state.
    pub members: Vec<(usize, Status)>,
}

impl Channel {
    pub fn bundled(&self) -> Vec<usize> {
        self.members.iter().filter(|(_, s)| *s == Status::Bundled).map(|(p, _)| *p).collect()
    }

    pub fn is_up(&self) -> bool {
        self.members.iter().any(|(_, s)| *s == Status::Bundled)
    }

    /// The protocol of the first member's mode.
    pub fn protocol(&self, d: &Device) -> &'static str {
        self.members.first().and_then(|(p, _)| d.ports.get(*p)?.channel).map_or("none", |g| g.mode.protocol())
    }
}

/// The channels of a switch, by group number.
pub fn channels(t: &Topology, d: &Device) -> Vec<Channel> {
    if d.kind != DeviceKind::Switch {
        return Vec::new();
    }
    let mut groups: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (i, p) in d.ports.iter().enumerate() {
        if let Some(g) = p.channel {
            groups.entry(g.number).or_default().push(i);
        }
    }
    groups
        .into_iter()
        .map(|(number, ports)| {
            // The first port that negotiates decides the neighbour, its group and the settings
            // every other member must share.
            let mut lead: Option<(packetcraft_core::DeviceId, u16, &packetcraft_model::Switchport)> = None;
            let members = ports
                .into_iter()
                .map(|p| {
                    let status = member_status(t, d, p, &mut lead);
                    (p, status)
                })
                .collect();
            Channel { number, members }
        })
        .collect()
}

fn member_status<'a>(
    t: &'a Topology,
    d: &'a Device,
    p: usize,
    lead: &mut Option<(packetcraft_core::DeviceId, u16, &'a packetcraft_model::Switchport)>,
) -> Status {
    let end = End { device: d.id, port: p };
    let (Some(port), Some(own)) = (d.ports.get(p), d.ports.get(p).and_then(|x| x.channel)) else { return Status::Down };
    if !port_up(t, end) {
        return Status::Down;
    }
    let far: Option<ChannelGroup> = t.peer(end).and_then(|q| {
        let pd = t.device(q.device).filter(|pd| pd.kind == DeviceKind::Switch)?;
        let g = pd.ports.get(q.port)?.channel?;
        own.mode.bundles_with(g.mode).then_some(g)
    });
    let Some(far) = far else {
        return if own.mode == packetcraft_model::ChannelMode::On { Status::Suspended } else { Status::Individual };
    };
    let Some(peer) = t.peer(end) else { return Status::Down };
    match lead {
        None => {
            *lead = Some((peer.device, far.number, &port.switchport));
            Status::Bundled
        }
        Some((dev, group, sp)) => {
            if *dev == peer.device && *group == far.number && **sp == port.switchport {
                Status::Bundled
            } else {
                Status::Suspended
            }
        }
    }
}

/// LACP every 30 s (the slow rate).
pub const LACP_EVERY_US: u64 = 30_000_000;

/// What an LACP port says about itself, and its state bits.
fn lacp_actor(t: &Topology, d: &Device, p: usize) -> Option<LacpInfo> {
    use packetcraft_model::ChannelMode;
    let g = d.ports.get(p)?.channel?;
    if !matches!(g.mode, ChannelMode::Active | ChannelMode::Passive) {
        return None;
    }
    let bundled = channels(t, d).iter().flat_map(|c| c.members.clone()).any(|(q, s)| q == p && s == Status::Bundled);
    let mut state = lacp::AGGREGATION;
    if g.mode == ChannelMode::Active {
        state |= lacp::ACTIVITY;
    }
    if bundled {
        state |= lacp::SYNC | lacp::COLLECTING | lacp::DISTRIBUTING;
    }
    Some(LacpInfo {
        system_priority: 32_768,
        system: d.ports.first().map_or(MacAddr([0; 6]), |f| f.mac),
        key: g.number,
        port_priority: 32_768,
        port: u16::try_from(p + 1).unwrap_or(u16::MAX),
        state,
    })
}

/// The LACPDU a port sends, if it sends one: active ports always, passive ports once they hear
/// an active neighbour.
pub fn lacpdu(t: &Topology, d: &Device, p: usize) -> Option<Lacpdu> {
    let actor = lacp_actor(t, d, p)?;
    let partner = t.peer(End { device: d.id, port: p }).and_then(|q| t.device(q.device).and_then(|pd| lacp_actor(t, pd, q.port)));
    if actor.state & lacp::ACTIVITY == 0 && partner.is_none_or(|x| x.state & lacp::ACTIVITY == 0) {
        return None;
    }
    Some(Lacpdu { actor, partner: partner.unwrap_or(LacpInfo { state: lacp::DEFAULTED, ..LacpInfo::default() }) })
}

/// How a switch's ports map onto logical ports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortMap {
    /// Bundled port → the channel's first bundled port, which stands for the whole bundle.
    lead: BTreeMap<usize, usize>,
    /// Lead port → every bundled member.
    bundles: BTreeMap<usize, Vec<usize>>,
    /// Lead port → group number.
    numbers: BTreeMap<usize, u16>,
    suspended: BTreeSet<usize>,
}

impl PortMap {
    pub fn new(t: &Topology, d: &Device) -> PortMap {
        let mut m = PortMap::default();
        for c in channels(t, d) {
            for (p, s) in &c.members {
                if *s == Status::Suspended {
                    m.suspended.insert(*p);
                }
            }
            let b = c.bundled();
            let Some(&first) = b.first() else { continue };
            for p in &b {
                m.lead.insert(*p, first);
            }
            m.numbers.insert(first, c.number);
            m.bundles.insert(first, b);
        }
        m
    }

    /// The logical port standing for `p` (itself unless it is bundled).
    pub fn logical(&self, p: usize) -> usize {
        self.lead.get(&p).copied().unwrap_or(p)
    }

    /// Whether `p` is a bundled member other than its channel's lead (it never acts alone).
    pub fn hidden(&self, p: usize) -> bool {
        self.logical(p) != p
    }

    pub fn suspended(&self, p: usize) -> bool {
        self.suspended.contains(&p)
    }

    /// The channel number when `p` is bundled.
    pub fn channel_of(&self, p: usize) -> Option<u16> {
        self.numbers.get(&self.logical(p)).copied()
    }

    /// The member that carries a frame sent to logical port `p`: spread by addresses, so one
    /// conversation always takes the same member and frames stay in order.
    pub fn pick(&self, p: usize, src: MacAddr, dst: MacAddr) -> usize {
        let Some(members) = self.bundles.get(&self.logical(p)) else { return p };
        let h = src.0.iter().zip(dst.0.iter()).fold(0usize, |h, (a, b)| h ^ usize::from(a ^ b));
        members.get(h % members.len().max(1)).copied().unwrap_or(p)
    }

    /// Bundled members of logical port `p` (just `p` when it isn't a channel).
    pub fn members(&self, p: usize) -> Vec<usize> {
        self.bundles.get(&self.logical(p)).cloned().unwrap_or_else(|| vec![p])
    }
}

/// The interface name people see: `Port-channel1` for a bundled port, else the port's own.
pub fn port_label(t: &Topology, d: &Device, p: usize, short: bool) -> String {
    match PortMap::new(t, d).channel_of(p) {
        Some(n) if short => format!("Po{n}"),
        Some(n) => format!("Port-channel{n}"),
        None => d.ports.get(p).map_or_else(String::new, |x| if short { packetcraft_model::ifname::short(&x.name) } else { x.name.clone() }),
    }
}
