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
//!
//! **Places with their own physics** (`Environments::physics`, `environment.physics`): the
//! amounts above are the values a place *settles* at, not what it jumps to. Each place that holds
//! smart things keeps the value of every variable a running part moves, and each simulated second
//! the value covers a share of the way to its target: after `t` seconds the gap has shrunk by
//! `exp(-t / tau)`, with `tau` in minutes per variable ([`settle_minutes`]). So a heating element
//! that settles a closet 10 degrees above what it sets warms it by about 6 degrees in the first
//! `tau` (10 minutes), and when it is switched off the closet cools back the same way. The place's
//! own values are the target, flames add smoke, carbon monoxide and heat, an open window pulls the
//! variables toward the place above it (faster with wind), and a sound heard fades with distance.
//! Whatever no part moves is left to the older rules.

use std::collections::{BTreeMap, BTreeSet};

use netlabcraft_model::environment::MAX_STEP_SECONDS;
use netlabcraft_model::environments::{Source, VARS, Var, generic_whole, sensor_var, sensor_whole, var_by_id};
use netlabcraft_model::iot::{is_actuator, thing};
use netlabcraft_model::{Device, DeviceKind, Topology};

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

/// What a part does to its place's variables only when the places have their own physics: the
/// smart-home things that used to change the smart room alone, and the ones the new rules add.
fn extra_changes(what: &str) -> &'static [(&'static str, Change)] {
    match what {
        "heater" => &[("ambient_temperature", Change::Add(10.0)), ("humidity", Change::Add(-5.0))],
        "ac" => &[("ambient_temperature", Change::Add(-5.0)), ("humidity", Change::Add(-10.0))],
        "sprinkler" => &[("humidity", Change::Add(20.0)), ("soil_moisture", Change::Add(30.0)), ("fire", Change::Set(0.0))],
        "fan" => &[
            ("humidity", Change::Add(-6.0)),
            ("smoke", Change::Add(-30.0)),
            ("carbon_monoxide", Change::Add(-200.0)),
            ("carbon_dioxide", Change::Add(-0.02)),
        ],
        "light" | "smart led" | "dimmable led" | "rgb led" => &[("luminosity", Change::Floor(70.0))],
        _ => &[],
    }
}

/// How many minutes a variable takes to cover 63 % of the way to its target (`None`: it does not
/// move gradually: sound is there or not).
pub fn settle_minutes(id: &str) -> Option<f64> {
    Some(match id {
        "ambient_temperature" => 10.0,
        "humidity" => 5.0,
        "soil_moisture" => 20.0,
        "smoke" | "carbon_monoxide" | "carbon_dioxide" => 3.0,
        "fire" | "water_level" | "rain" => 1.0,
        "luminosity" => 0.5,
        _ => return None,
    })
}

/// The variables an open window couples to the place above.
fn through_windows(id: &str) -> bool {
    matches!(id, "ambient_temperature" | "humidity" | "smoke" | "carbon_monoxide" | "carbon_dioxide")
}

/// The most places that keep their own physics at once.
pub const MAX_PLACE_RUNS: usize = 64;

/// A place's values that are on their way to a target, and when they were last moved.
#[derive(Debug, Clone, Default)]
pub struct PlaceRun {
    pub last_us: u64,
    /// The value of each variable that a running part is moving or has just stopped moving.
    pub values: BTreeMap<&'static str, f64>,
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
    pub(crate) fn part_place(t: &Topology, d: &Device) -> u32 {
        t.home_of(d.id).map_or_else(|| t.default_place(), |h| h.place)
    }

    /// Whether an actuator is running (switched on and powered).
    fn part_on(&self, t: &Topology, d: &Device) -> bool {
        is_actuator(d) && !t.is_dark(d.id) && self.iot.things.get(&d.id).is_some_and(|s| s.on)
    }

    /// How many running windows (or garage doors) stand in a place.
    fn open_windows(&self, t: &Topology, place: u32) -> usize {
        t.devices
            .iter()
            .filter(|d| self.part_on(t, d) && Self::part_place(t, d) == place && thing(d).is_some_and(|x| matches!(x.what, "window" | "garage")))
            .count()
    }

    /// Whether a fire sprinkler sees flames in its place (the places' own physics): it then
    /// sprays whether or not the server asked.
    pub(crate) fn place_burning(&self, t: &Topology, d: &Device) -> bool {
        if !t.environments.physics || thing(d).is_none_or(|x| x.what != "fire sprinkler") {
            return false;
        }
        let place = Self::part_place(t, d);
        let base = t.env_value(place, "fire", self.env_time(t)).map_or(0.0, |x| x.0);
        let moving = self.iot.places.get(&place).and_then(|r| r.values.get("fire")).copied().unwrap_or(0.0);
        base.max(moving) > 0.5
    }

    /// A variable at a place and the environment clock's time now, with what the running parts
    /// of that place do to it: where it settles. With the places' own physics on, what a place
    /// holds at this moment is [`Sim::place_var`].
    pub(crate) fn place_target(&self, t: &Topology, place: u32, id: &str) -> Option<PlaceValue> {
        let var = var_by_id(id)?;
        let (base, source) = t.env_value(place, id, self.env_time(t))?;
        let physics = t.environments.physics;
        let mut value = base;
        let mut changed = false;
        if physics && matches!(id, "smoke" | "carbon_monoxide" | "ambient_temperature") {
            // Flames make smoke and carbon monoxide and warm the place.
            let flames = self.place_target(t, place, "fire").map_or(0.0, |p| p.value);
            if flames > 0.0 {
                value += flames
                    * match id {
                        "smoke" => 0.5,
                        "carbon_monoxide" => 5.0,
                        _ => 0.2,
                    };
                changed = true;
            }
        }
        for d in t.devices.iter().filter(|d| self.part_on(t, d) || self.place_burning(t, d)) {
            let Some(x) = thing(d) else { continue };
            if Self::part_place(t, d) != place {
                continue;
            }
            let more: &'static [(&'static str, Change)] = if physics { extra_changes(x.what) } else { &[] };
            for (name, change) in changes(x.what).iter().chain(more) {
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
        if physics && through_windows(id) && self.open_windows(t, place) > 0 {
            // An open window pulls the place toward the one above it.
            let outside = t.env_chain(place).get(1).and_then(|up| t.env_value(*up, id, self.env_time(t))).map_or(var.default, |x| x.0);
            value = value * 0.2 + outside * 0.8;
            changed = true;
        }
        Some(PlaceValue { value: value.clamp(var.min, var.max), set: source != Source::Default, changed })
    }

    /// A variable at a place now: where its parts settle it (the older rule), or, with the places'
    /// own physics on, how far along it has got.
    pub(crate) fn place_var(&self, t: &Topology, place: u32, id: &str) -> Option<PlaceValue> {
        let mut pv = self.place_target(t, place, id)?;
        if t.environments.physics && settle_minutes(id).is_some() {
            let stored = self.iot.places.get(&place).and_then(|r| r.values.get(id)).copied();
            if pv.changed || stored.is_some() {
                let var = var_by_id(id)?;
                let base = t.env_value(place, id, self.env_time(t)).map_or(var.default, |x| x.0).clamp(var.min, var.max);
                pv.value = stored.unwrap_or(base);
                pv.changed = true;
                pv.set = true;
            }
        }
        Some(pv)
    }

    /// What a place holds now of a variable its parts move, with the places' own physics on.
    pub fn place_live(&self, t: &Topology, place: u32, id: &str) -> Option<f64> {
        if !t.environments.physics {
            return None;
        }
        self.place_var(t, place, id).filter(|p| p.changed).map(|p| p.value)
    }

    /// Moves every place that holds smart things toward where its running parts settle it, for
    /// the simulated time since the last step. Does nothing (and forgets) while the places have no
    /// physics of their own.
    pub(crate) fn place_step(&mut self, t: &Topology) {
        if !t.environments.physics {
            self.iot.places.clear();
            return;
        }
        let now = self.now;
        let here: BTreeSet<u32> =
            t.devices.iter().filter(|d| d.kind == DeviceKind::Iot).map(|d| Self::part_place(t, d)).take(MAX_PLACE_RUNS).collect();
        self.iot.places.retain(|p, _| here.contains(p));
        let time = self.env_time(t);
        for place in here {
            let last = self.iot.places.get(&place).map_or(now, |r| r.last_us);
            let dt = now.saturating_sub(last).min(MAX_STEP_SECONDS.saturating_mul(1_000_000)) as f64 / 1_000_000.0;
            let windows = self.open_windows(t, place) > 0;
            let wind = self.place_target(t, place, "wind_speed").map_or(0.0, |p| p.value.clamp(0.0, 500.0));
            let mut next: Vec<(&'static str, Option<f64>)> = Vec::new();
            for v in VARS.iter().filter(|v| settle_minutes(v.id).is_some()) {
                let Some(target) = self.place_target(t, place, v.id) else { continue };
                let had = self.iot.places.get(&place).and_then(|r| r.values.get(v.id)).copied();
                if !target.changed && had.is_none() {
                    continue;
                }
                let base = t.env_value(place, v.id, time).map_or(v.default, |x| x.0).clamp(v.min, v.max);
                let mut tau = settle_minutes(v.id).unwrap_or(5.0) * 60.0;
                if windows && through_windows(v.id) {
                    tau /= 1.0 + wind / 20.0;
                }
                let value = target.value + (had.unwrap_or(base) - target.value) * (-dt / tau).exp();
                let settled = !target.changed && (value - target.value).abs() < 0.02;
                next.push((v.id, if settled { None } else { Some(value.clamp(v.min, v.max)) }));
            }
            let run = self.iot.places.entry(place).or_insert_with(|| PlaceRun { last_us: now, values: BTreeMap::new() });
            for (id, v) in next {
                match v {
                    Some(x) => {
                        run.values.insert(id, x);
                    }
                    None => {
                        run.values.remove(id);
                    }
                }
            }
            run.last_us = now;
        }
    }

    /// What a sensor hears: the loudest running sound of its place, fading 6 dB for every doubling
    /// of the distance from one metre (a thing that is not on the map is heard at full volume).
    /// `None`: nothing in the place is making sound.
    fn sound_at(&self, t: &Topology, listener: &Device, word: &str) -> Option<i32> {
        let place = Self::part_place(t, listener);
        let mut best: Option<(f64, f64)> = None;
        for src in t.devices.iter().filter(|s| s.id != listener.id && self.part_on(t, s) && Self::part_place(t, s) == place) {
            let Some(x) = thing(src) else { continue };
            let (mut volume, mut pitch) = (None, 0.0);
            for (name, change) in changes(x.what) {
                match (*name, change) {
                    ("sound_volume", Change::Floor(f)) => volume = Some(*f),
                    ("sound_pitch", Change::Set(p)) => pitch = *p,
                    _ => {}
                }
            }
            let Some(db) = volume else { continue };
            let metres = t.distance_m(src, listener).map_or(1.0, f64::from).max(1.0);
            let heard = db - 20.0 * metres.log10();
            if heard > 0.0 && best.is_none_or(|b| heard > b.0) {
                best = Some((heard, pitch));
            }
        }
        best.map(|(db, hz)| if word == "volume" { db.round() as i32 } else { hz.round() as i32 })
    }

    /// What the place of a sensor says it reads for a kind of reading (`temperature`,
    /// `humidity`, `smoke`, `wind`…): the value the place sets, with the running parts' changes;
    /// or, where the place sets nothing, a variable a running part changed while the room's
    /// physics is off. `None`: the room or the sensor's own number answers.
    pub(crate) fn mapped_reading(&self, t: &Topology, d: &Device, word: &str) -> Option<i32> {
        if t.environments.physics
            && matches!(word, "volume" | "pitch")
            && let Some(v) = self.sound_at(t, d, word)
        {
            return Some(v);
        }
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
                // A variable a place defines by a formula.
                if var_by_id(&d.iot.variable).is_none() && netlabcraft_model::envcustom::valid_custom_id(&d.iot.variable) {
                    let c = t.env_custom_read(place, &d.iot.variable, self.env_time(t))?;
                    let (lo, hi) = (f64::from(netlabcraft_model::iot::MIN_VALUE), f64::from(netlabcraft_model::iot::MAX_VALUE));
                    return Some(c.value.round().clamp(lo, hi) as i32);
                }
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
        for what in ["heater", "ac", "sprinkler", "fan", "light", "smart led", "dimmable led", "rgb led"] {
            assert!(!extra_changes(what).is_empty(), "{what}");
            for (id, _) in extra_changes(what) {
                assert!(var_by_id(id).is_some(), "{what}: {id}");
            }
        }
        assert!(extra_changes("buzzer").is_empty());
        for v in VARS.iter().filter(|v| settle_minutes(v.id).is_some()) {
            assert!(settle_minutes(v.id).is_some_and(|m| m > 0.0), "{}", v.id);
        }
    }
}
