//! UDLD: ports that run it send a probe every few seconds and listen for the probes of the port
//! at the other end of the cable. A link is good when each end hears the other. When a port
//! hears its neighbour but the neighbour does not hear it (a strand is cut, see
//! `Topology::one_way`), the port is err-disabled with the cause `udld`. In aggressive mode a
//! port that heard its neighbour and then hears nothing for three intervals is shut down too.
//!
//! The probes are real frames (`netlabcraft_wire::udld`) and cross the cable like any other, so a
//! one-way cut loses them in one direction only. What each end heard is kept per receiving port.

use netlabcraft_core::{DeviceId, MacAddr};
use netlabcraft_model::l2sec::{ErrCause, UdldMode};
use netlabcraft_model::{Device, DeviceKind, End, Topology, ifname};
use netlabcraft_wire::{Frame, udld};

use crate::{Sim, port_up};

/// Intervals without a probe before a neighbour counts as gone (and a missing echo as a fault).
const DETECT_INTERVALS: u64 = 3;

/// The last probe a port heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Heard {
    pub mac: MacAddr,
    pub device: String,
    pub port: String,
    pub first_us: u64,
    pub last_us: u64,
}

/// How a port's link looks to UDLD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdldState {
    /// UDLD is off on this port.
    Disabled,
    /// The port is down, or shut by something else.
    Down,
    /// Nothing has been heard from the other end.
    NoNeighbour,
    /// Both ends hear each other.
    Bidirectional,
    /// This port hears its neighbour, which does not hear it.
    Unidirectional,
    /// The neighbour was heard and went silent.
    Silent,
    /// UDLD shut the port.
    ErrDisabled,
}

impl UdldState {
    pub fn text(self) -> &'static str {
        match self {
            UdldState::Disabled => "disabled",
            UdldState::Down => "port is down",
            UdldState::NoNeighbour => "no neighbour heard",
            UdldState::Bidirectional => "bidirectional",
            UdldState::Unidirectional => "unidirectional (the neighbour does not hear this port)",
            UdldState::Silent => "the neighbour went silent",
            UdldState::ErrDisabled => "err-disabled by UDLD",
        }
    }
}

/// The mode a port runs UDLD in: its own setting, else the switch's.
pub fn mode_of(d: &Device, port: usize) -> Option<UdldMode> {
    let own = d.ports.get(port)?.l2.udld;
    match own.or(d.l2.udld) {
        Some(UdldMode::Off) | None => None,
        Some(m) => Some(m),
    }
}

fn interval_us(d: &Device) -> u64 {
    u64::from(d.l2.udld_interval.clamp(1, 255)).saturating_mul(1_000_000)
}

impl Sim {
    /// Sends the probes due at `at` (a whole second): each port keeps to its switch's interval.
    pub(crate) fn send_udld(&mut self, t: &Topology, at: u64) {
        let mut frames = Vec::new();
        for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Switch) {
            let secs = (interval_us(d) / 1_000_000).max(1);
            if !(at / 1_000_000).is_multiple_of(secs) {
                continue;
            }
            for (i, p) in d.ports.iter().enumerate() {
                if mode_of(d, i).is_none() || p.routed {
                    continue;
                }
                let probe = udld::Probe { interval: u8::try_from(secs).unwrap_or(u8::MAX), device: d.name.clone(), port: p.name.clone() };
                let f = Frame { dst: udld::UDLD_GROUP, src: p.mac, vlan: None, ethertype: udld::ETHERTYPE_UDLD, payload: probe.encode() };
                frames.push((End { device: d.id, port: i }, f.encode()));
            }
        }
        for (from, f) in frames {
            self.queue_at(t, from, f, at);
        }
    }

    /// A probe arrived on a switch port.
    pub(crate) fn udld_in(&mut self, d: &Device, port: usize, f: &Frame) -> Result<(), String> {
        if mode_of(d, port).is_none() {
            return Ok(());
        }
        let p = udld::Probe::decode(&f.payload).map_err(|e| e.to_string())?;
        let now = self.now;
        if self.udld_heard.len() >= crate::MAX_MAC_ENTRIES && !self.udld_heard.contains_key(&(d.id, port)) {
            return Ok(());
        }
        let e = self.udld_heard.entry((d.id, port)).or_insert_with(|| Heard {
            mac: f.src,
            device: String::new(),
            port: String::new(),
            first_us: now,
            last_us: now,
        });
        if e.mac != f.src {
            // Another port answers now: the cable moved, start again.
            e.first_us = now;
            e.mac = f.src;
        }
        e.device = p.device;
        e.port = p.port;
        e.last_us = now;
        Ok(())
    }

    fn udld_fresh(&self, d: &Device, key: (DeviceId, usize)) -> Option<&Heard> {
        let detect = interval_us(d).saturating_mul(DETECT_INTERVALS);
        self.udld_heard.get(&key).filter(|h| self.now.saturating_sub(h.last_us) < detect)
    }

    /// How a port's link looks to UDLD now.
    pub fn udld_state(&self, t: &Topology, d: &Device, port: usize) -> UdldState {
        let Some(mode) = mode_of(d, port) else { return UdldState::Disabled };
        let Some(p) = d.ports.get(port) else { return UdldState::Disabled };
        if p.err_disabled || self.err_disabled_now(d.id, port) {
            return if p.l2.err_cause == Some(ErrCause::Udld) || self.udld_shut.contains(&(d.id, port)) {
                UdldState::ErrDisabled
            } else {
                UdldState::Down
            };
        }
        let end = End { device: d.id, port };
        if !port_up(t, end) {
            return UdldState::Down;
        }
        if !self.udld_heard.contains_key(&(d.id, port)) {
            return UdldState::NoNeighbour;
        }
        if self.udld_fresh(d, (d.id, port)).is_none() {
            return if mode == UdldMode::Aggressive { UdldState::Silent } else { UdldState::NoNeighbour };
        }
        let back = t
            .peer(end)
            .and_then(|q| t.device(q.device).map(|pd| (q, pd)))
            .is_some_and(|(q, pd)| self.udld_fresh(pd, (q.device, q.port)).is_some_and(|b| b.mac == p.mac));
        if back { UdldState::Bidirectional } else { UdldState::Unidirectional }
    }

    /// Forgets what a switch's ports heard (`udld reset`).
    pub fn udld_forget(&mut self, d: DeviceId) {
        self.udld_heard.retain(|k, _| k.0 != d);
        self.udld_shut.retain(|k| k.0 != d);
    }

    /// What a port last heard: its neighbour's device, port and when (µs).
    pub fn udld_neighbour(&self, d: DeviceId, port: usize) -> Option<(String, String, u64)> {
        self.udld_heard.get(&(d, port)).map(|h| (h.device.clone(), h.port.clone(), h.last_us))
    }

    /// Once per run: shuts the ports whose link is one-way (or, aggressive, silent).
    pub(crate) fn udld_check(&mut self, t: &Topology) {
        if t.devices.iter().all(|d| d.l2.udld.is_none() && d.ports.iter().all(|p| p.l2.udld.is_none())) && self.udld_heard.is_empty() {
            return;
        }
        let mut shut: Vec<(DeviceId, usize, String)> = Vec::new();
        for d in t.devices.iter().filter(|d| d.kind == DeviceKind::Switch) {
            let detect = interval_us(d).saturating_mul(DETECT_INTERVALS);
            for (i, p) in d.ports.iter().enumerate() {
                let end = End { device: d.id, port: i };
                if mode_of(d, i).is_none() || p.err_disabled || !port_up(t, end) {
                    // A port that went down starts over when it comes back.
                    self.udld_heard.remove(&(d.id, i));
                    continue;
                }
                let state = self.udld_state(t, d, i);
                let age = self.udld_heard.get(&(d.id, i)).map_or(0, |h| self.now.saturating_sub(h.first_us));
                let name = ifname::short(&p.name);
                match state {
                    UdldState::Unidirectional if age >= detect => {
                        let who = self.udld_heard.get(&(d.id, i)).map_or(String::new(), |h| format!("{} {}", h.device, ifname::short(&h.port)));
                        shut.push((d.id, i, format!("UDLD: {name} hears {who}, which does not hear it back (a one-way link)")));
                    }
                    UdldState::Silent => shut.push((d.id, i, format!("UDLD (aggressive): the neighbour of {name} stopped sending probes"))),
                    _ => {}
                }
            }
        }
        for (dev, i, why) in shut {
            if let Some(d) = t.device(dev) {
                self.udld_shut.insert((dev, i));
                self.err_disable(d, i, ErrCause::Udld, &why);
            }
        }
    }
}
