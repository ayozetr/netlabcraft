//! Proximity between smart things: a Bluetooth speaker pairs with a music player, an RFID reader
//! reads the cards near it, a beacon is heard by the things around it, a trip sensor sees what
//! comes close and a running car is a thing that can be moved to where it matters.
//!
//! The rules are deliberately shallow: a thing is a point, the distance is the straight line,
//! and a range is a fixed number of metres, the same on both maps. On the logical map a metre
//! is [`UNITS_PER_METRE`] workspace units (the scale the radio ranges already use); once the lab
//! has a physical layout, two things are close only when they are in the same city, a thing in a
//! building standing where the building's icon is, and the city map's units are metres. The
//! ranges are scaled for the map, not for physics (a real RFID card reads from a few
//! centimetres): they are large enough to be set up by dragging icons. Nothing here keeps
//! state, so the answer depends only on where things are now and whether they are switched on.

use netlabcraft_core::DeviceId;

use crate::cellular::city_position;
use crate::physical::CITY_UNIT_M;
use crate::{Device, DeviceKind, Topology};

/// Workspace units to a metre on the logical map (a radio's 250 m is 450 units).
pub const UNITS_PER_METRE: f32 = 1.8;
/// A Bluetooth speaker pairs with a music player at most this far away (metres).
pub const BLUETOOTH_RANGE_M: f32 = 60.0;
/// An RFID reader reads the cards at most this far away.
pub const RFID_RANGE_M: f32 = 30.0;
/// A beacon is heard by the things this near.
pub const BEACON_RANGE_M: f32 = 100.0;
/// A trip sensor sees what comes this near.
pub const TRIP_RANGE_M: f32 = 50.0;

/// The models a trip sensor takes for something crossing its line: things that move.
pub const MOBILE_MODELS: [&str; 6] = ["IoT-Car", "IoT-RFIDCard", "Smartphone", "Tablet", "Laptop", "Laptop-WiFi"];

impl Topology {
    /// How far apart two devices are, in metres (`None`: not on the same map, so not close at
    /// all).
    pub fn distance_m(&self, a: &Device, b: &Device) -> Option<f32> {
        let d = if self.implicit_layout() {
            ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() / UNITS_PER_METRE
        } else {
            let (ca, xa, ya) = city_position(self, a)?;
            let (cb, xb, yb) = city_position(self, b)?;
            if ca != cb {
                return None;
            }
            ((xa - xb).powi(2) + (ya - yb).powi(2)).sqrt() * CITY_UNIT_M
        };
        d.is_finite().then_some(d)
    }

    /// The smart things of a model, switched on, within `range_m` of a device, nearest first (a
    /// tie goes to the lower id so the answer never depends on the order of the list).
    pub fn near(&self, from: &Device, range_m: f32, wanted: impl Fn(&Device) -> bool) -> Vec<(DeviceId, f32)> {
        let mut out: Vec<(DeviceId, f32)> = self
            .devices
            .iter()
            .filter(|d| d.id != from.id && d.kind == DeviceKind::Iot && wanted(d) && !self.is_dark(d.id))
            .filter_map(|d| self.distance_m(from, d).filter(|m| *m <= range_m).map(|m| (d.id, m)))
            .collect();
        out.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        out
    }

    /// The music player a Bluetooth speaker is paired with: the nearest one within range
    /// (`None`: none is near, or the speaker is switched off at the power).
    pub fn paired_player(&self, speaker: &Device) -> Option<DeviceId> {
        if self.is_dark(speaker.id) {
            return None;
        }
        self.near(speaker, BLUETOOTH_RANGE_M, |d| d.catalogue_model().id == "IoT-Player").first().map(|x| x.0)
    }

    /// The Bluetooth speaker a music player is paired with: the nearest one within range.
    pub fn paired_speaker(&self, player: &Device) -> Option<DeviceId> {
        self.near(player, BLUETOOTH_RANGE_M, |d| d.catalogue_model().id == "IoT-BTSpeaker").first().map(|x| x.0)
    }
}

/// The id a card or a beacon announces: the one set, else its device number plus one, so two
/// new cards never share an id.
pub fn announced_id(d: &Device) -> i32 {
    if d.iot.value > 0 { d.iot.value } else { i32::try_from(d.id.0).unwrap_or(0).saturating_add(1) }
}

impl Topology {
    /// What an RFID reader reads: the id of the nearest card within range, else 0.
    pub fn rfid_reading(&self, reader: &Device) -> i32 {
        self.near(reader, RFID_RANGE_M, |d| d.catalogue_model().id == "IoT-RFIDCard").first().and_then(|x| self.device(x.0)).map_or(0, announced_id)
    }

    /// What a beacon reports next to its id: how many other smart things hear it.
    pub fn beacon_audience(&self, beacon: &Device) -> i32 {
        i32::try_from(self.near(beacon, BEACON_RANGE_M, |_| true).len()).unwrap_or(i32::MAX)
    }

    /// What a trip sensor reads: 1 while something that moves is within range, else 0.
    pub fn trip_reading(&self, sensor: &Device) -> i32 {
        let mobile = |d: &Device| MOBILE_MODELS.contains(&d.catalogue_model().id);
        // A phone, tablet or laptop is not a smart thing, so it is looked for separately.
        let walker = self.devices.iter().any(|d| {
            d.id != sensor.id
                && d.kind != DeviceKind::Iot
                && mobile(d)
                && !self.is_dark(d.id)
                && self.distance_m(sensor, d).is_some_and(|m| m <= TRIP_RANGE_M)
        });
        i32::from(walker || !self.near(sensor, TRIP_RANGE_M, mobile).is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models;

    fn add(t: &mut Topology, model: &str, x: f32) -> DeviceId {
        t.add_model(models::find(model).unwrap(), None, x, 0.0).unwrap()
    }

    #[test]
    fn a_speaker_pairs_with_the_nearest_player_in_range_and_not_beyond_it() {
        let mut t = Topology::new();
        let speaker = add(&mut t, "IoT-BTSpeaker", 0.0);
        let far = add(&mut t, "IoT-Player", 400.0);
        let near = add(&mut t, "IoT-Player", 90.0);
        let sp = t.device(speaker).unwrap();
        // 90 units are 50 m (1.8 units to the metre), inside the 60 m of Bluetooth; 400 units are not.
        assert!(t.distance_m(sp, t.device(near).unwrap()).is_some_and(|m| (m - 50.0).abs() < 0.01));
        assert_eq!(t.paired_player(sp), Some(near));
        assert_eq!(t.paired_speaker(t.device(near).unwrap()), Some(speaker));
        assert_eq!(t.paired_speaker(t.device(far).unwrap()), None);
        // Moved out of range, or switched off at the power, it pairs with nothing.
        t.device_mut(near).unwrap().x = 200.0;
        assert_eq!(t.paired_player(t.device(speaker).unwrap()), None);
        t.device_mut(near).unwrap().x = 90.0;
        t.device_mut(near).unwrap().powered_off = true;
        assert_eq!(t.paired_player(t.device(speaker).unwrap()), None);
    }

    #[test]
    fn ties_go_to_the_lower_id_and_a_new_card_announces_a_unique_id() {
        let mut t = Topology::new();
        let speaker = add(&mut t, "IoT-BTSpeaker", 0.0);
        let a = add(&mut t, "IoT-Player", 18.0);
        let b = add(&mut t, "IoT-Player", -18.0);
        assert_eq!(t.paired_player(t.device(speaker).unwrap()), Some(a.min(b)));
        let (da, db) = (t.device(a).unwrap().clone(), t.device(b).unwrap().clone());
        assert_ne!(announced_id(&da), announced_id(&db));
        let mut set = da;
        set.iot.value = 77;
        assert_eq!(announced_id(&set), 77);
    }
}
