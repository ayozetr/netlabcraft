//! Components in their environment: what a running actuator does to the variables of the place
//! it stands in (an air cooler lowers the temperature, a sprinkler raises the humidity, the rain
//! and the water level and puts the flames out, a speaker makes sound), and what the sensors
//! that read those variables report.
//!
//! The change is made when a sensor reads, on top of what the place sets (or the variable's
//! default): nothing is stored, so turning the part off puts the variable back at once. Only the
//! parts in the same place act on each other. With the smart room's physics running, the room's
//! own heaters, coolers and sprinklers answer for the quantities it holds (the physics counts the
//! new parts too); a part changes a variable the place itself leaves unset only while the physics
//! is off, so the two never add up.

use netlabcraft_model::environments::{Source, Var, generic_whole, sensor_var, sensor_whole, var_by_id};
use netlabcraft_model::iot::{is_actuator, thing};
use netlabcraft_model::{Device, Topology};

use crate::Sim;

/// How a running part changes one variable.
#[derive(Debug, Clone, Copy)]
enum Change {
    /// Adds an amount (the variable's range keeps the result).
    Add(f64),
    /// Sets the value.
    Set(f64),
    /// Raises the value to at least this.
    Floor(f64),
}

/// What a part of this kind does to the variables of its place while it is on.
fn changes(what: &str) -> &'static [(&'static str, Change)] {
    match what {
        "air cooler" => &[("ambient_temperature", Change::Add(-5.0)), ("humidity", Change::Add(-10.0))],
        "heating element" => &[("ambient_temperature", Change::Add(10.0)), ("humidity", Change::Add(-5.0))],
        "ceiling sprinkler" => {
            &[("humidity", Change::Add(30.0)), ("rain", Change::Add(2.0)), ("water_level", Change::Add(1.0)), ("fire", Change::Set(0.0))]
        }
        "floor sprinkler" => {
            &[("humidity", Change::Add(20.0)), ("water_level", Change::Add(3.0)), ("soil_moisture", Change::Add(30.0)), ("fire", Change::Set(0.0))]
        }
        "speaker" => &[("sound_volume", Change::Floor(70.0)), ("sound_pitch", Change::Set(440.0))],
        // The second wave of things: the same variables, for a place with the physics off.
        "furnace" => &[("ambient_temperature", Change::Add(15.0)), ("humidity", Change::Add(-5.0))],
        "ceiling fan" => &[("ambient_temperature", Change::Add(-3.0))],
        "humidifier" => &[("humidity", Change::Add(20.0))],
        "fire sprinkler" => &[("humidity", Change::Add(20.0)), ("fire", Change::Set(0.0))],
        "car" => &[("carbon_monoxide", Change::Add(60.0)), ("carbon_dioxide", Change::Add(0.05))],
        "bluetooth speaker" => &[("sound_volume", Change::Floor(75.0)), ("sound_pitch", Change::Set(440.0))],
        "music player" => &[("sound_volume", Change::Floor(55.0)), ("sound_pitch", Change::Set(440.0))],
        "buzzer" => &[("sound_volume", Change::Floor(80.0)), ("sound_pitch", Change::Set(2_700.0))],
        "alarm" | "siren" => &[("sound_volume", Change::Floor(100.0)), ("sound_pitch", Change::Set(1_000.0))],
        _ => &[],
    }
}

/// A variable at a place, as the running parts leave it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaceValue {
    pub value: f64,
    /// The place (or one above it) sets the variable.
    pub set: bool,
    /// A running part in the place changed it.
    pub changed: bool,
}

impl Sim {
    /// The place a device stands in.
    fn part_place(t: &Topology, d: &Device) -> u32 {
        t.home_of(d.id).map_or_else(|| t.default_place(), |h| h.place)
    }

    /// Whether an actuator is running (switched on and powered).
    fn part_on(&self, t: &Topology, d: &Device) -> bool {
        is_actuator(d) && !t.is_dark(d.id) && self.iot.things.get(&d.id).is_some_and(|s| s.on)
    }

    /// A variable at a place and the environment clock's time now, with what the running parts
    /// of that place do to it.
    pub(crate) fn place_var(&self, t: &Topology, place: u32, id: &str) -> Option<PlaceValue> {
        let var = var_by_id(id)?;
        let (base, source) = t.env_value(place, id, self.env_time(t))?;
        let mut value = base;
        let mut changed = false;
        for d in t.devices.iter().filter(|d| self.part_on(t, d)) {
            let Some(x) = thing(d) else { continue };
            if Self::part_place(t, d) != place {
                continue;
            }
            for (name, change) in changes(x.what) {
                if *name != id {
                    continue;
                }
                value = match change {
                    Change::Add(a) => value + a,
                    Change::Set(v) => *v,
                    Change::Floor(f) => value.max(*f),
                };
                changed = true;
            }
        }
        Some(PlaceValue { value: value.clamp(var.min, var.max), set: source != Source::Default, changed })
    }

    /// What the place of a sensor says it reads for a kind of reading (`temperature`,
    /// `humidity`, `smoke`, `wind`…): the value the place sets, with the running parts' changes;
    /// or, where the place sets nothing, a variable a running part changed while the room's
    /// physics is off. `None`: the room or the sensor's own number answers.
    pub(crate) fn mapped_reading(&self, t: &Topology, d: &Device, word: &str) -> Option<i32> {
        let var = sensor_var(word)?;
        let pv = self.place_var(t, Self::part_place(t, d), var.id)?;
        (pv.set || (pv.changed && !t.environment.enabled)).then(|| sensor_whole(var, pv.value))
    }

    /// The room's reading of a quantity, while its physics runs.
    fn room_reading(&self, t: &Topology, word: &str) -> Option<i32> {
        if !t.environment.enabled {
            return None;
        }
        self.environment().and_then(|c| c.reading(word))
    }

    /// What a part reads that no plain sensor does: the generic environment sensor reads the
    /// variable it was set to, and the water detector looks at the water level and the rain.
    pub(crate) fn part_reading(&self, t: &Topology, d: &Device, what: &str) -> Option<i32> {
        let place = Self::part_place(t, d);
        match what {
            "environment" => {
                let var: &Var = var_by_id(&d.iot.variable).or_else(|| var_by_id("ambient_temperature"))?;
                let pv = self.place_var(t, place, var.id)?;
                if !pv.set
                    && !pv.changed
                    && let Some(r) = var.drives.and_then(|w| self.room_reading(t, w))
                {
                    // The room reads carbon dioxide in ppm and the variable is a percentage.
                    return Some(generic_whole(var, var.from_reading(r)));
                }
                Some(generic_whole(var, pv.value))
            }
            "water present" => {
                let wet = |id: &str| self.place_var(t, place, id).filter(|p| p.set || p.changed);
                let (level, rain) = (wet("water_level"), wet("rain"));
                if level.is_none() && rain.is_none() {
                    return None;
                }
                Some(i32::from(level.is_some_and(|p| p.value > 0.0) || rain.is_some_and(|p| p.value > 0.0)))
            }
            _ => None,
        }
    }

    /// A sensor's second reading: the humiture sensor's humidity (the other sensors keep none).
    pub fn iot_second(&self, t: &Topology, d: &Device) -> i32 {
        if thing(d).is_some_and(|x| x.what == "humiture") {
            if let Some(v) = self.mapped_reading(t, d, "humidity") {
                return v;
            }
            if let Some(v) = self.room_reading(t, "humidity") {
                return v;
            }
        }
        d.iot.second
    }

    /// What a smart device shows as its level: 0 when off, else a dimmable LED's brightness, a
    /// servo's angle or an RGB LED's colour; 1 when a device with no level is on.
    pub fn iot_level(&self, d: &Device) -> i32 {
        let st = self.iot.things.get(&d.id).cloned().unwrap_or_default();
        netlabcraft_model::iot::shown_level(thing(d).map_or("", |x| x.what), st.on, st.level)
    }

    /// Sets a smart device's level by hand (clamped to its range) and switches it on or off
    /// with it, as a board's `write` does.
    pub fn iot_set_level(&mut self, d: &Device, level: i64) {
        let what = thing(d).map_or("", |x| x.what);
        let st = self.iot.things.entry(d.id).or_default();
        match netlabcraft_model::iot::level_range(what) {
            Some((lo, hi)) => {
                let l = i32::try_from(level.clamp(i64::from(lo), i64::from(hi))).unwrap_or(lo);
                st.level = l;
                st.on = what == "servo" || l != 0;
            }
            None => st.on = level != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_change_names_a_variable_that_exists() {
        for what in [
            "air cooler",
            "heating element",
            "ceiling sprinkler",
            "floor sprinkler",
            "speaker",
            "buzzer",
            "alarm",
            "siren",
            "furnace",
            "ceiling fan",
            "humidifier",
            "fire sprinkler",
            "car",
            "bluetooth speaker",
            "music player",
        ] {
            assert!(!changes(what).is_empty(), "{what}");
            for (id, _) in changes(what) {
                assert!(var_by_id(id).is_some(), "{what}: {id}");
            }
        }
        assert!(changes("fan").is_empty());
    }
}
