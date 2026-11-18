//! What proximity needs to remember: where the things that move were at the last step (so a
//! crossing of a trip sensor's wire is told from standing near it) and who heard each beacon (so
//! a beacon can keep a log of who came and went).
//!
//! Positions are looked at when the simulator steps its smart things (`Sim::env_step`, which runs
//! as simulated time moves and whenever a sensor reports), so a thing that is moved across a wire
//! between two steps counts as having crossed it, and one that is moved back before the next step
//! does not.

use std::collections::{BTreeMap, BTreeSet};

use netlabcraft_core::DeviceId;
use netlabcraft_model::iot::thing;
use netlabcraft_model::proximity::{BEACON_RANGE_M, TRIP_HOLD_S, segments_cross};
use netlabcraft_model::{Device, Topology};

use crate::Sim;

/// The most events a beacon keeps.
pub const MAX_BEACON_LOG: usize = 60;

/// One thing coming into range of a beacon or leaving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeaconEvent {
    /// Simulated microseconds.
    pub at_us: u64,
    /// The thing that came or went.
    pub thing: DeviceId,
    /// `true`: it came into range; `false`: it went out of range (or off).
    pub heard: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProximityState {
    /// Where each thing that moves was at the last step: its map and position in metres.
    positions: BTreeMap<DeviceId, (u32, f64, f64)>,
    /// A trip sensor reads 1 until this simulated time (microseconds).
    pub(crate) trips: BTreeMap<DeviceId, u64>,
    /// The things that heard each beacon at the last step.
    audience: BTreeMap<DeviceId, BTreeSet<DeviceId>>,
    log: BTreeMap<DeviceId, Vec<BeaconEvent>>,
}

impl Sim {
    /// Looks at where things are now: starts the hold of every trip sensor whose wire something
    /// crossed since the last step, and logs who came into range of a beacon or left it.
    pub(crate) fn prox_step(&mut self, t: &Topology) {
        let now = self.now;
        let mobile: Vec<(DeviceId, (u32, f64, f64))> =
            t.devices.iter().filter(|d| t.is_mobile(d)).filter_map(|d| t.position_m(d).map(|p| (d.id, p))).collect();
        let sensors: Vec<&Device> =
            t.devices.iter().filter(|d| d.iot.crossing && !t.is_dark(d.id) && thing(d).is_some_and(|x| x.what == "trip")).collect();
        for s in sensors {
            let Some((place, a, b)) = t.trip_wire(s) else { continue };
            for (id, (map, x, y)) in &mobile {
                let Some(before) = self.iot.prox.positions.get(id) else { continue };
                if before.0 == place.0 && *map == place.0 && segments_cross((before.1, before.2), (*x, *y), a, b) {
                    self.iot.prox.trips.insert(s.id, now.saturating_add(TRIP_HOLD_S.saturating_mul(1_000_000)));
                }
            }
        }
        self.iot.prox.positions = mobile.into_iter().collect();
        self.iot.prox.trips.retain(|id, until| *until > now && t.device(*id).is_some());
        // Beacons: who is in range now, against who was.
        let beacons: Vec<&Device> = t.devices.iter().filter(|d| !t.is_dark(d.id) && thing(d).is_some_and(|x| x.what == "beacon")).collect();
        let live: BTreeSet<DeviceId> = beacons.iter().map(|d| d.id).collect();
        self.iot.prox.audience.retain(|id, _| live.contains(id));
        self.iot.prox.log.retain(|id, _| t.device(*id).is_some());
        for b in beacons {
            let heard: BTreeSet<DeviceId> = t.near(b, BEACON_RANGE_M, |_| true).into_iter().map(|x| x.0).collect();
            let was = self.iot.prox.audience.get(&b.id).cloned();
            if let Some(was) = was {
                let log = self.iot.prox.log.entry(b.id).or_default();
                for id in heard.difference(&was) {
                    log.push(BeaconEvent { at_us: now, thing: *id, heard: true });
                }
                for id in was.difference(&heard) {
                    log.push(BeaconEvent { at_us: now, thing: *id, heard: false });
                }
                if log.len() > MAX_BEACON_LOG {
                    let extra = log.len() - MAX_BEACON_LOG;
                    log.drain(..extra);
                }
            } else {
                // The first look: everything in range is heard from now on, with a line for each.
                let log = self.iot.prox.log.entry(b.id).or_default();
                for id in &heard {
                    log.push(BeaconEvent { at_us: now, thing: *id, heard: true });
                }
                log.truncate(MAX_BEACON_LOG);
            }
            self.iot.prox.audience.insert(b.id, heard);
        }
    }

    /// What a beacon keeps: the things that came into its range and went out of it, oldest first.
    pub fn beacon_log(&self, beacon: DeviceId) -> &[BeaconEvent] {
        self.iot.prox.log.get(&beacon).map_or(&[], Vec::as_slice)
    }

    /// The things that hear a beacon now, by id (as of the last step).
    pub fn beacon_audience(&self, beacon: DeviceId) -> Vec<DeviceId> {
        self.iot.prox.audience.get(&beacon).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }
}
