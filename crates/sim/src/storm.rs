//! Storm control: the traffic a port takes in is measured over one simulated second, kind by
//! kind (broadcast, multicast, unknown unicast), and what goes above the port's level is dropped.
//! With `storm-control action shutdown` the first excess err-disables the port; with `trap` it
//! also writes a message to the log.
//!
//! A storm starts when the traffic of the second passes the rising level and ends when a whole
//! second stays at or below the falling one (the rising one when no other is set). Percent
//! levels are a share of the link's speed, `bps` levels count bits (a frame's Ethernet header and
//! payload) and `pps` levels count frames.

use netlabcraft_core::{DeviceId, VlanId};
use netlabcraft_model::l2sec::{ErrCause, StormAction, StormKind, StormUnit};
use netlabcraft_model::{Device, End, Topology, ifname};
use netlabcraft_wire::Frame;

use crate::Sim;

/// What one port took in of one kind of traffic in the second being counted, and in the one before.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Window {
    sec: u64,
    pkts: u64,
    bits: u64,
    storming: bool,
    last_pkts: u64,
    last_bits: u64,
    dropped: u64,
    storms: u64,
}

/// A port's storm-control state for one kind, as `show storm-control` prints it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StormStat {
    /// The rate of the last full second, in packets and in bits.
    pub pkts: u64,
    pub bits: u64,
    /// Above the level now.
    pub storming: bool,
    /// Frames dropped since the port came up, and storms seen.
    pub dropped: u64,
    pub storms: u64,
}

fn value(unit: StormUnit, pkts: u64, bits: u64, speed_bps: u64) -> u64 {
    match unit {
        StormUnit::Percent => bits.saturating_mul(10_000) / speed_bps.max(1),
        StormUnit::Bps => bits,
        StormUnit::Pps => pkts,
    }
}

/// The speed storm-control percentages are taken from: the link's, else fast Ethernet's.
pub fn port_speed_bps(t: &Topology, d: DeviceId, port: usize) -> u64 {
    crate::link_speed(t, End { device: d, port }).map_or(100, |(mbps, _)| u64::from(mbps)).saturating_mul(1_000_000)
}

impl Sim {
    /// Counts a frame that came in on a switch port and says whether storm control lets it in.
    pub(crate) fn storm_in(&mut self, t: &Topology, d: &Device, port: usize, vlan: VlanId, f: &Frame) -> Result<(), String> {
        let Some(p) = d.ports.get(port) else { return Ok(()) };
        let ctl = p.l2.storm;
        if ctl.broadcast.is_none() && ctl.multicast.is_none() && ctl.unicast.is_none() {
            return Ok(());
        }
        let kind = if f.dst.is_broadcast() {
            StormKind::Broadcast
        } else if f.dst.is_group() {
            StormKind::Multicast
        } else if self.state.get(&d.id).is_some_and(|s| s.macs.contains_key(&(vlan.get(), f.dst))) {
            return Ok(());
        } else {
            StormKind::Unicast
        };
        let Some(level) = ctl.level(kind) else { return Ok(()) };
        let speed = port_speed_bps(t, d.id, port);
        let len = 14usize.saturating_add(f.payload.len()).saturating_add(if f.vlan.is_some() { 4 } else { 0 });
        let bits = u64::try_from(len).unwrap_or(0).saturating_mul(8);
        let sec = self.now / 1_000_000;
        let falling = level.falling.unwrap_or(level.rising);
        let (mut ended, mut started) = (false, false);
        let w = self.storm_win.entry((d.id, port, kind)).or_default();
        if w.sec != sec {
            let quiet = sec > w.sec.saturating_add(1);
            let (pkts, bits_before) = if quiet { (0, 0) } else { (w.pkts, w.bits) };
            if w.storming && value(level.unit, pkts, bits_before, speed) <= falling {
                w.storming = false;
                ended = true;
            }
            w.last_pkts = pkts;
            w.last_bits = bits_before;
            w.pkts = 0;
            w.bits = 0;
            w.sec = sec;
        }
        w.pkts = w.pkts.saturating_add(1);
        w.bits = w.bits.saturating_add(bits);
        if !w.storming && value(level.unit, w.pkts, w.bits, speed) > level.rising {
            w.storming = true;
            w.storms = w.storms.saturating_add(1);
            started = true;
        }
        let dropping = w.storming;
        if dropping {
            w.dropped = w.dropped.saturating_add(1);
        }
        let name = ifname::short(&p.name);
        if ended {
            self.log(d.id, &format!("{name}: {} traffic is back at or below its storm-control level", kind.name()));
        }
        if started {
            let why = format!("storm control: {} traffic above its level", kind.name());
            match ctl.action {
                StormAction::Shutdown => self.err_disable(d, port, ErrCause::StormControl, &why),
                StormAction::Trap => self.log(d.id, &format!("{name}: {why}; the excess is dropped")),
                StormAction::Filter => {}
            }
        }
        if dropping { Err(format!("storm control: {} traffic above the level on {name}; dropped", kind.name())) } else { Ok(()) }
    }

    /// What a port's storm control measured of one kind of traffic.
    pub fn storm_stat(&self, d: DeviceId, port: usize, kind: StormKind) -> StormStat {
        let Some(w) = self.storm_win.get(&(d, port, kind)) else { return StormStat::default() };
        let sec = self.now / 1_000_000;
        // A window older than the last full second says nothing about the second just ended.
        let (pkts, bits, storming) = match sec.saturating_sub(w.sec) {
            0 => (w.last_pkts, w.last_bits, w.storming),
            1 => (w.pkts, w.bits, w.storming),
            _ => (0, 0, false),
        };
        StormStat { pkts, bits, storming, dropped: w.dropped, storms: w.storms }
    }
}
