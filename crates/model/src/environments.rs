//! Environments: the surroundings of a place, as a set of named physical variables.
//!
//! Every place (the Intercity map, a city, a building, a wiring closet) may hold an environment
//! of its own. A place that sets nothing inherits from the place it stands in, so a closet
//! reads its building's values, which read their city's, which read the Intercity map's. What
//! no place sets is the variable's default: an ordinary physical constant (standard gravity,
//! sea-level pressure, the composition of dry air) or zero.
//!
//! A variable is either a constant or a list of keyframes over one day. Between two keyframes
//! the value moves in a straight line, and the day wraps: after the last keyframe it moves
//! toward the first keyframe of the next day. The environment clock says what time of day it is;
//! it follows the simulated time of the lab at a chosen ratio and can be paused.
//!
//! The lab's smart-home room (`environment`, the `iot.environment` command) is separate and
//! stays as it was: a smart sensor reads a variable of its place only when that place or one
//! above it sets the variable; otherwise it reads the room, or its own number, as before. The
//! variables with no sensor are stored and shown, nothing reads them yet (`docs/gaps.md`).
//!
//! An environment can be exported to and imported from a small JSON file of its own
//! (`docs/file-format.md`), independent of the lab file.

use serde::{Deserialize, Serialize};

use crate::{ModelError, Result, Topology};

/// Seconds in a day: keyframe times run from 0 up to, not including, this.
pub const DAY_SECONDS: u32 = 86_400;
/// The id of the top level (the Intercity map). Places are numbered from 1.
pub const INTERCITY: u32 = 0;
/// Most keyframes one variable keeps (one every five minutes).
pub const MAX_KEYS: usize = 288;
/// Longest notes text.
pub const MAX_NOTES: usize = 4_000;
/// Largest number in the clock's ratio (`N real minutes equal M simulated minutes`).
pub const MAX_SCALE: u32 = 1_440;
/// What time of day the environment clock shows when the simulation starts (08:00:00).
pub const DEFAULT_START: u32 = 8 * 3_600;
/// Largest environment file `import` accepts.
pub const MAX_IMPORT_BYTES: usize = 1_000_000;
/// The `format` tag of an exported environment, and its version.
pub const EXPORT_FORMAT: &str = "netlabcraft-environment";
pub const EXPORT_VERSION: u32 = 1;

pub const EARTH: &str = "Earth Physical Features";
pub const ELECTRICITY: &str = "Electricity";
pub const ENERGY: &str = "Energy";
pub const GASES: &str = "Gases";
pub const GRAVITY: &str = "Gravity";
pub const LIGHT: &str = "Light (Sun)";
pub const MOTION: &str = "Motion";
pub const OTHER: &str = "Other";
pub const RADIATION: &str = "Radiation";
pub const SOUND: &str = "Sound";
pub const TEMPERATURE: &str = "Temperature";
pub const WATER: &str = "Water";
pub const WIND: &str = "Wind";

/// The groups, in the order the window lists them.
pub const GROUPS: [&str; 13] = [EARTH, ELECTRICITY, ENERGY, GASES, GRAVITY, LIGHT, MOTION, OTHER, RADIATION, SOUND, TEMPERATURE, WATER, WIND];

/// One variable: what it is called, how it is measured, its default and its allowed range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Var {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub unit: &'static str,
    /// The value where no place sets one.
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// The word of the smart sensor that reads it (`temperature`, `humidity`, `soil`, `smoke`,
    /// `co`); none: nothing reads it yet.
    pub drives: Option<&'static str>,
}

const fn var(id: &'static str, label: &'static str, group: &'static str, unit: &'static str, default: f64, min: f64, max: f64) -> Var {
    Var { id, label, group, unit, default, min, max, drives: None }
}

const fn sensed(v: Var, sensor: &'static str) -> Var {
    Var { drives: Some(sensor), ..v }
}

/// Every variable, grouped. Defaults are ordinary constants: standard gravity, the International
/// Standard Atmosphere at sea level (101.325 kPa, 15 degrees), the volume shares of dry air,
/// a neutral pH of 7 and the natural radiation background; the others start at 0, or at 100 %
/// for the shares of unattenuated sunlight.
pub static VARS: &[Var] = &[
    // Earth Physical Features
    var("elevation", "Elevation", EARTH, "m", 0.0, -500.0, 9_000.0),
    var("soil_ph", "Soil pH", EARTH, "pH", 7.0, 0.0, 14.0),
    sensed(var("soil_moisture", "Soil moisture", EARTH, "%", 40.0, 0.0, 100.0), "soil"),
    var("compaction", "Soil compaction", EARTH, "%", 0.0, 0.0, 100.0),
    var("hardness", "Composition hardness", EARTH, "Mohs", 0.0, 0.0, 10.0),
    // Electricity
    var("voltage", "Voltage", ELECTRICITY, "V", 0.0, 0.0, 1_000_000.0),
    var("ohms", "Resistance", ELECTRICITY, "ohm", 0.0, 0.0, 1_000_000_000.0),
    var("watts", "Power", ELECTRICITY, "W", 0.0, 0.0, 1_000_000_000.0),
    // Energy
    var("heat", "Heat", ENERGY, "W/m2", 0.0, 0.0, 100_000.0),
    var("photon", "Photon flux", ENERGY, "umol/m2/s", 0.0, 0.0, 5_000.0),
    var("fire", "Fire", ENERGY, "%", 0.0, 0.0, 100.0),
    // Gases: the volume shares of dry air, except smoke (obscuration) and carbon monoxide (ppm)
    var("argon", "Argon", GASES, "%", 0.934, 0.0, 100.0),
    sensed(var("carbon_monoxide", "Carbon monoxide", GASES, "ppm", 0.0, 0.0, 1_000.0), "co"),
    var("carbon_dioxide", "Carbon dioxide", GASES, "%", 0.042, 0.0, 100.0),
    var("helium", "Helium", GASES, "%", 0.000_524, 0.0, 100.0),
    var("hydrogen", "Hydrogen", GASES, "%", 0.000_055, 0.0, 100.0),
    var("methane", "Methane", GASES, "%", 0.000_19, 0.0, 100.0),
    var("nitrogen", "Nitrogen", GASES, "%", 78.084, 0.0, 100.0),
    var("oxygen", "Oxygen", GASES, "%", 20.946, 0.0, 100.0),
    var("ozone", "Ozone", GASES, "%", 0.0, 0.0, 100.0),
    var("propane", "Propane", GASES, "%", 0.0, 0.0, 100.0),
    sensed(var("smoke", "Smoke", GASES, "%", 0.0, 0.0, 100.0), "smoke"),
    // Gravity
    var("gravity", "Gravity", GRAVITY, "m/s2", 9.806_65, 0.0, 100.0),
    // Light (Sun): shares of unattenuated daylight
    var("electromagnetic_radiation", "Electromagnetic radiation", LIGHT, "%", 100.0, 0.0, 100.0),
    var("infrared", "Infrared", LIGHT, "%", 100.0, 0.0, 100.0),
    var("radiant_heat", "Radiant heat", LIGHT, "%", 100.0, 0.0, 100.0),
    var("sunlight", "Sunlight", LIGHT, "%", 100.0, 0.0, 100.0),
    var("ultraviolet", "Ultraviolet", LIGHT, "%", 100.0, 0.0, 100.0),
    var("visible_light", "Visible light", LIGHT, "%", 100.0, 0.0, 100.0),
    var("luminosity", "Luminosity", LIGHT, "%", 100.0, 0.0, 100.0),
    var("moon_phase", "Moon phase", LIGHT, "% lit", 0.0, 0.0, 100.0),
    // Motion
    var("motion_direction", "Motion direction", MOTION, "deg", 0.0, 0.0, 360.0),
    var("motion_speed", "Motion speed", MOTION, "m/s", 0.0, 0.0, 1_000.0),
    var("torque", "Torque", MOTION, "N m", 0.0, 0.0, 1_000_000.0),
    // Other
    var("atmospheric_pressure", "Atmospheric pressure", OTHER, "kPa", 101.325, 0.0, 200.0),
    // Radiation
    var("radiation_level", "Radiation level", RADIATION, "mrem/h", 0.01, 0.0, 1_000_000.0),
    var("alpha", "Alpha radiation", RADIATION, "cpm", 0.0, 0.0, 1_000_000.0),
    var("beta", "Beta radiation", RADIATION, "cpm", 0.0, 0.0, 1_000_000.0),
    var("gamma", "Gamma radiation", RADIATION, "cpm", 0.0, 0.0, 1_000_000.0),
    var("neutron", "Neutron radiation", RADIATION, "cpm", 0.0, 0.0, 1_000_000.0),
    var("ionization", "Ionization", RADIATION, "%", 0.0, 0.0, 100.0),
    // Sound
    var("sound_pitch", "Sound pitch", SOUND, "Hz", 0.0, 0.0, 20_000.0),
    var("sound_volume", "Sound volume", SOUND, "dB", 0.0, 0.0, 200.0),
    var("white_noise", "White noise", SOUND, "dB", 0.0, 0.0, 200.0),
    // Temperature
    sensed(var("ambient_temperature", "Ambient temperature", TEMPERATURE, "\u{b0}C", 15.0, -100.0, 150.0), "temperature"),
    // Water
    var("clouds", "Clouds", WATER, "%", 0.0, 0.0, 100.0),
    sensed(var("humidity", "Humidity", WATER, "%", 50.0, 0.0, 100.0), "humidity"),
    var("rain", "Rain", WATER, "cm", 0.0, 0.0, 1_000.0),
    var("snow", "Snow", WATER, "cm", 0.0, 0.0, 10_000.0),
    var("water_level", "Water level", WATER, "cm", 0.0, 0.0, 1_000_000.0),
    var("water_volume", "Water volume", WATER, "L", 0.0, 0.0, 1_000_000_000.0),
    var("water_ph", "Water pH", WATER, "pH", 7.0, 0.0, 14.0),
    var("water_temperature", "Water temperature", WATER, "\u{b0}C", 15.0, -10.0, 120.0),
    var("water_speed", "Water speed", WATER, "m/s", 0.0, 0.0, 100.0),
    // Wind
    var("wind_direction", "Wind direction", WIND, "deg", 0.0, 0.0, 360.0),
    var("wind_gusts", "Wind gusts", WIND, "%", 0.0, 0.0, 100.0),
    var("wind_speed", "Wind speed", WIND, "kph", 0.0, 0.0, 500.0),
];

/// A variable by id.
pub fn var_by_id(id: &str) -> Option<&'static Var> {
    VARS.iter().find(|v| v.id == id)
}

/// A variable by id or by its label (case-insensitive, `_` or a space).
pub fn find_var(name: &str) -> Option<&'static Var> {
    let key: String = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    VARS.iter().find(|v| v.id == key || v.label.to_ascii_lowercase().replace([' ', '-'], "_") == key)
}

/// The variable a smart sensor reads (`thermostat` reads the temperature).
pub fn sensor_var(what: &str) -> Option<&'static Var> {
    let what = if what == "thermostat" { "temperature" } else { what };
    VARS.iter().find(|v| v.drives == Some(what))
}

/// One keyframe: the value at a time of day (seconds from midnight).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub t: u32,
    pub v: f64,
}

/// What a place sets for one variable: one key is a constant, several are keyframes (sorted by
/// time, one per time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    pub id: String,
    pub keys: Vec<Key>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The environment one place sets: its variables and its notes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlaceEnv {
    /// The place's id (0 is the Intercity map).
    pub place: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// The View mode of the window shows the notes.
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_notes: bool,
}

impl PlaceEnv {
    pub fn setting(&self, id: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.id == id)
    }

    fn is_empty(&self) -> bool {
        self.settings.is_empty() && self.notes.is_empty() && !self.show_notes
    }
}

/// The environment clock. The time of day follows the simulated time of the lab: every `real`
/// minutes that pass in the simulation, `sim` minutes pass in the environment. The time at
/// simulated time 0 is `start`; a paused clock shows `frozen`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clock {
    pub start: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frozen: Option<u32>,
    pub real_minutes: u32,
    pub sim_minutes: u32,
}

impl Default for Clock {
    fn default() -> Self {
        Clock { start: DEFAULT_START, frozen: None, real_minutes: 1, sim_minutes: 1 }
    }
}

impl Clock {
    pub fn paused(&self) -> bool {
        self.frozen.is_some()
    }

    /// The seconds of environment time that passed at simulated time `now_us`, within a day.
    fn elapsed(&self, now_us: u64) -> u32 {
        let secs = u128::from(now_us / 1_000_000);
        let scaled = secs.saturating_mul(u128::from(self.sim_minutes.max(1))) / u128::from(self.real_minutes.max(1));
        u32::try_from(scaled % u128::from(DAY_SECONDS)).unwrap_or(0)
    }

    /// The time of day at simulated time `now_us`, in seconds from midnight.
    pub fn time_at(&self, now_us: u64) -> u32 {
        match self.frozen {
            Some(t) => t % DAY_SECONDS,
            None => (self.start % DAY_SECONDS).saturating_add(self.elapsed(now_us)) % DAY_SECONDS,
        }
    }

    /// Makes the time at `now_us` be `time`, keeping the clock running or paused as it is.
    pub fn set_time(&mut self, now_us: u64, time: u32) {
        let time = time % DAY_SECONDS;
        if self.paused() {
            self.frozen = Some(time);
        } else {
            self.start = (time + DAY_SECONDS - self.elapsed(now_us)) % DAY_SECONDS;
        }
    }

    /// Stops the clock where it is, or starts it again from where it stopped.
    pub fn set_paused(&mut self, now_us: u64, paused: bool) {
        match (paused, self.frozen) {
            (true, None) => self.frozen = Some(self.time_at(now_us)),
            (false, Some(t)) => {
                self.frozen = None;
                self.set_time(now_us, t);
            }
            _ => {}
        }
    }

    /// Changes the ratio; the time of day shown now does not jump.
    pub fn set_scale(&mut self, now_us: u64, real_minutes: u32, sim_minutes: u32) {
        let now = self.time_at(now_us);
        self.real_minutes = real_minutes.clamp(1, MAX_SCALE);
        self.sim_minutes = sim_minutes.clamp(1, MAX_SCALE);
        self.set_time(now_us, now);
    }

    fn sanitize(&mut self) {
        self.start %= DAY_SECONDS;
        self.frozen = self.frozen.map(|t| t % DAY_SECONDS);
        self.real_minutes = self.real_minutes.clamp(1, MAX_SCALE);
        self.sim_minutes = self.sim_minutes.clamp(1, MAX_SCALE);
    }
}

/// The lab's environments: the clock and what each place sets.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Environments {
    pub clock: Clock,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<PlaceEnv>,
}

impl Environments {
    pub fn is_default(&self) -> bool {
        *self == Environments::default()
    }
}

/// Where a variable's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The place sets it.
    Own,
    /// A place above sets it (its id; 0 is the Intercity map).
    Inherited(u32),
    /// Nobody sets it: the default.
    Default,
}

/// The value of keyframes at a time of day: a straight line between the keyframes on either
/// side, wrapping around midnight. `None` for no keyframes.
pub fn interpolate(keys: &[Key], time: u32) -> Option<f64> {
    let t = i64::from(time % DAY_SECONDS);
    let first = keys.first()?;
    if keys.len() == 1 {
        return Some(first.v);
    }
    let day = i64::from(DAY_SECONDS);
    // The last key at or before the time, and the first one after it; wrapped by a day when the
    // time is before the first key or after the last.
    let before = keys.iter().rev().find(|k| i64::from(k.t) <= t).map(|k| (i64::from(k.t), k.v));
    let after = keys.iter().find(|k| i64::from(k.t) > t).map(|k| (i64::from(k.t), k.v));
    let last = keys.last().unwrap_or(first);
    let (t0, v0) = before.unwrap_or((i64::from(last.t) - day, last.v));
    let (t1, v1) = after.unwrap_or((i64::from(first.t) + day, first.v));
    if t1 <= t0 {
        return Some(v0);
    }
    let share = (t - t0) as f64 / (t1 - t0) as f64;
    Some(v0 + (v1 - v0) * share)
}

/// `07:30` or `07:30:15` to seconds from midnight (a plain number of seconds also parses).
pub fn parse_time(s: &str) -> Option<u32> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(':').collect();
    let n = |p: &str| p.trim().parse::<u32>().ok();
    let secs = match parts[..] {
        [one] => n(one)?,
        [h, m] => n(h)?.checked_mul(3_600)?.checked_add(n(m)?.checked_mul(60)?)?,
        [h, m, sec] => n(h)?.checked_mul(3_600)?.checked_add(n(m)?.checked_mul(60)?)?.checked_add(n(sec)?)?,
        _ => return None,
    };
    (secs < DAY_SECONDS).then_some(secs)
}

/// Seconds from midnight as `HH:MM:SS`.
pub fn format_time(t: u32) -> String {
    let t = t % DAY_SECONDS;
    format!("{:02}:{:02}:{:02}", t / 3_600, t / 60 % 60, t % 60)
}

fn invalid(msg: impl Into<String>) -> ModelError {
    ModelError::Invalid(msg.into())
}

impl Topology {
    /// Whether `place` can have an environment: the Intercity map or a place of the lab.
    pub fn env_place_exists(&self, place: u32) -> bool {
        place == INTERCITY || self.place(place).is_some()
    }

    /// The places whose environments `place` reads, nearest first: itself, the places it stands
    /// in, then the Intercity map.
    pub fn env_chain(&self, place: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.place_path(Some(place)).iter().rev().map(|p| p.id).collect();
        if out.is_empty() && place != INTERCITY {
            out.push(place);
        }
        out.push(INTERCITY);
        out
    }

    /// What `place` sets itself.
    pub fn place_env(&self, place: u32) -> Option<&PlaceEnv> {
        self.environments.places.iter().find(|e| e.place == place)
    }

    /// The setting a place reads for a variable: the nearest place that sets it.
    pub fn env_setting(&self, place: u32, id: &str) -> Option<(u32, &Setting)> {
        self.env_chain(place).into_iter().find_map(|p| self.place_env(p).and_then(|e| e.setting(id)).map(|s| (p, s)))
    }

    /// A variable at a place and a time of day: its value and where it came from.
    pub fn env_value(&self, place: u32, id: &str, time: u32) -> Option<(f64, Source)> {
        let v = var_by_id(id)?;
        Some(match self.env_setting(place, id) {
            Some((from, s)) => {
                let value = interpolate(&s.keys, time).unwrap_or(v.default).clamp(v.min, v.max);
                (value, if from == place { Source::Own } else { Source::Inherited(from) })
            }
            None => (v.default, Source::Default),
        })
    }

    fn place_env_mut(&mut self, place: u32) -> Result<&mut PlaceEnv> {
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        if self.place_env(place).is_none() {
            if self.environments.places.len() > crate::physical::MAX_PLACES {
                return Err(invalid("too many environments"));
            }
            self.environments.places.push(PlaceEnv { place, ..PlaceEnv::default() });
        }
        self.environments.places.iter_mut().find(|e| e.place == place).ok_or_else(|| invalid(format!("no place {place}")))
    }

    fn tidy_environments(&mut self) {
        for e in &mut self.environments.places {
            e.settings.retain(|s| !s.keys.is_empty());
            e.settings.sort_by_key(|s| VARS.iter().position(|v| v.id == s.id).unwrap_or(usize::MAX));
        }
        self.environments.places.retain(|e| !e.is_empty());
        self.environments.places.sort_by_key(|e| e.place);
    }

    /// Checks a value against a variable's range.
    fn check_value(v: &Var, value: f64) -> Result<()> {
        if !value.is_finite() || !(v.min..=v.max).contains(&value) {
            return Err(invalid(format!("{} is between {} and {} {}", v.label, v.min, v.max, v.unit)));
        }
        Ok(())
    }

    /// Sets a variable at a place to one constant value (replacing its keyframes).
    pub fn env_set(&mut self, place: u32, id: &str, value: f64) -> Result<()> {
        let v = var_by_id(id).ok_or_else(|| invalid(format!("unknown variable `{id}`")))?;
        Self::check_value(v, value)?;
        let e = self.place_env_mut(place)?;
        e.settings.retain(|s| s.id != id);
        e.settings.push(Setting { id: id.to_string(), keys: vec![Key { t: 0, v: value }] });
        self.tidy_environments();
        Ok(())
    }

    /// Adds a keyframe (or moves the value of the one at that time). A variable that had a
    /// constant keeps it as a keyframe at midnight unless the new one is there.
    pub fn env_add_key(&mut self, place: u32, id: &str, time: u32, value: f64) -> Result<()> {
        let v = var_by_id(id).ok_or_else(|| invalid(format!("unknown variable `{id}`")))?;
        Self::check_value(v, value)?;
        if time >= DAY_SECONDS {
            return Err(invalid("a keyframe time is within the day (00:00:00 to 23:59:59)"));
        }
        let e = self.place_env_mut(place)?;
        if e.setting(id).is_none() {
            e.settings.push(Setting { id: id.to_string(), keys: Vec::new() });
        }
        let s = e.settings.iter_mut().find(|s| s.id == id).ok_or_else(|| invalid("no such setting"))?;
        match s.keys.iter_mut().find(|k| k.t == time) {
            Some(k) => k.v = value,
            None => {
                if s.keys.len() >= MAX_KEYS {
                    return Err(invalid(format!("at most {MAX_KEYS} keyframes per variable")));
                }
                s.keys.push(Key { t: time, v: value });
                s.keys.sort_by_key(|k| k.t);
            }
        }
        self.tidy_environments();
        Ok(())
    }

    /// Removes the keyframe at a time; the variable goes back to being inherited when none is
    /// left.
    pub fn env_remove_key(&mut self, place: u32, id: &str, time: u32) -> Result<()> {
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        let e = self.environments.places.iter_mut().find(|e| e.place == place);
        let s = e.and_then(|e| e.settings.iter_mut().find(|s| s.id == id)).ok_or_else(|| invalid(format!("{place} sets no `{id}`")))?;
        let before = s.keys.len();
        s.keys.retain(|k| k.t != time);
        if s.keys.len() == before {
            return Err(invalid(format!("no keyframe at {}", format_time(time))));
        }
        self.tidy_environments();
        Ok(())
    }

    /// Takes one variable (or, with `None`, everything) off a place: it inherits again.
    pub fn env_clear(&mut self, place: u32, id: Option<&str>) -> Result<()> {
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        if let Some(id) = id
            && var_by_id(id).is_none()
        {
            return Err(invalid(format!("unknown variable `{id}`")));
        }
        if let Some(e) = self.environments.places.iter_mut().find(|e| e.place == place) {
            match id {
                Some(id) => e.settings.retain(|s| s.id != id),
                None => e.settings.clear(),
            }
        }
        self.tidy_environments();
        Ok(())
    }

    /// Sets a place's notes and whether View mode shows them.
    pub fn env_notes(&mut self, place: u32, text: Option<&str>, show: Option<bool>) -> Result<()> {
        let e = self.place_env_mut(place)?;
        if let Some(t) = text {
            e.notes = t.chars().take(MAX_NOTES).collect();
        }
        if let Some(b) = show {
            e.show_notes = b;
        }
        self.tidy_environments();
        Ok(())
    }

    /// What a smart sensor of this kind reads at a place and a time of day, in whole units, when
    /// the place or one above it sets the variable (`None`: nothing is set, the room answers).
    pub fn env_reading(&self, place: u32, sensor: &str, time: u32) -> Option<i32> {
        let v = sensor_var(sensor)?;
        let (_, s) = self.env_setting(place, v.id)?;
        let value = interpolate(&s.keys, time)?.clamp(v.min, v.max);
        let (lo, hi) = match v.id {
            "ambient_temperature" => (crate::environment::MIN_TEMP, crate::environment::MAX_TEMP),
            "carbon_monoxide" => (0, crate::environment::MAX_CO),
            _ => (0, 100),
        };
        // `round` of a finite value within a few thousand: the conversion cannot overflow.
        Some((value.round() as i32).clamp(lo, hi))
    }

    /// Forgets the environments of places that are gone (after a place is removed).
    pub(crate) fn prune_environments(&mut self) {
        let ids: std::collections::BTreeSet<u32> = self.places().iter().map(|p| p.id).collect();
        self.environments.places.retain(|e| e.place == INTERCITY || ids.contains(&e.place));
    }

    /// Repairs the environments of a hostile file: unknown places and variables, values out of
    /// range, unsorted or repeated keyframes, oversized lists.
    pub(crate) fn sanitize_environments(&mut self) {
        self.prune_environments();
        self.environments.clock.sanitize();
        let mut seen = std::collections::BTreeSet::new();
        self.environments.places.retain(|e| seen.insert(e.place));
        for e in &mut self.environments.places {
            e.notes = e.notes.chars().take(MAX_NOTES).collect();
            let mut ids = std::collections::BTreeSet::new();
            e.settings.retain(|s| var_by_id(&s.id).is_some() && ids.insert(s.id.clone()));
            for s in &mut e.settings {
                let Some(v) = var_by_id(&s.id) else { continue };
                s.keys.retain(|k| k.v.is_finite() && k.t < DAY_SECONDS);
                for k in &mut s.keys {
                    k.v = k.v.clamp(v.min, v.max);
                }
                s.keys.sort_by_key(|k| k.t);
                s.keys.dedup_by_key(|k| k.t);
                s.keys.truncate(MAX_KEYS);
            }
        }
        self.tidy_environments();
    }

    /// The environment a place sets, as an exportable file (its own settings and notes).
    pub fn env_export(&self, place: u32) -> Result<String> {
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        let own = self.place_env(place).cloned().unwrap_or_default();
        let mut variables = serde_json::Map::new();
        for v in VARS {
            if let Some(s) = own.setting(v.id) {
                let keys: Vec<serde_json::Value> = s.keys.iter().map(|k| serde_json::json!({"t": format_time(k.t), "v": k.v})).collect();
                variables.insert(v.id.to_string(), serde_json::Value::Array(keys));
            }
        }
        let doc = serde_json::json!({
            "format": EXPORT_FORMAT,
            "version": EXPORT_VERSION,
            "notes": own.notes,
            "show_notes": own.show_notes,
            "variables": variables,
        });
        serde_json::to_string_pretty(&doc).map_err(|e| invalid(format!("couldn't write the environment: {e}")))
    }

    /// Reads an exported environment into a place: its settings replace the place's own (with
    /// `merge`, only the variables the file names are replaced). Variables this version does not
    /// know are skipped; the count of what was read and skipped comes back.
    pub fn env_import(&mut self, place: u32, text: &str, merge: bool) -> Result<(usize, usize)> {
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        if text.len() > MAX_IMPORT_BYTES {
            return Err(invalid("the file is too large to be an environment"));
        }
        let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| invalid(format!("not a NetLabCraft environment: {e}")))?;
        if doc.get("format").and_then(serde_json::Value::as_str) != Some(EXPORT_FORMAT) {
            return Err(invalid("not a NetLabCraft environment (its `format` is not `netlabcraft-environment`)"));
        }
        if doc.get("version").and_then(serde_json::Value::as_u64).is_none_or(|v| v > u64::from(EXPORT_VERSION)) {
            return Err(invalid("this environment was saved by a newer NetLabCraft"));
        }
        let vars = doc.get("variables").and_then(serde_json::Value::as_object).ok_or_else(|| invalid("the file has no `variables`"))?;
        let mut settings: Vec<Setting> = Vec::new();
        let mut skipped = 0;
        for (name, list) in vars {
            let (Some(v), Some(list)) = (find_var(name), list.as_array()) else {
                skipped += 1;
                continue;
            };
            let mut keys: Vec<Key> = Vec::new();
            for k in list.iter().take(MAX_KEYS) {
                let t = match k.get("t") {
                    Some(serde_json::Value::String(s)) => parse_time(s),
                    Some(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n < DAY_SECONDS),
                    None => None,
                };
                let value = k.get("v").and_then(serde_json::Value::as_f64).filter(|x| x.is_finite());
                match (t, value) {
                    (Some(t), Some(value)) => keys.push(Key { t, v: value.clamp(v.min, v.max) }),
                    _ => return Err(invalid(format!("a keyframe of `{name}` needs a time (HH:MM:SS) and a number"))),
                }
            }
            keys.sort_by_key(|k| k.t);
            keys.dedup_by_key(|k| k.t);
            if !keys.is_empty() && !settings.iter().any(|s| s.id == v.id) {
                settings.push(Setting { id: v.id.to_string(), keys });
            }
        }
        let read = settings.len();
        let notes = doc.get("notes").and_then(serde_json::Value::as_str).map(|s| s.chars().take(MAX_NOTES).collect::<String>());
        let show = doc.get("show_notes").and_then(serde_json::Value::as_bool);
        let e = self.place_env_mut(place)?;
        if !merge {
            e.settings.clear();
        }
        for s in settings {
            e.settings.retain(|x| x.id != s.id);
            e.settings.push(s);
        }
        if !merge || notes.is_some() {
            e.notes = notes.unwrap_or_default();
        }
        if let Some(b) = show {
            e.show_notes = b;
        } else if !merge {
            e.show_notes = false;
        }
        self.tidy_environments();
        Ok((read, skipped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::PlaceKind;

    fn key(t: u32, v: f64) -> Key {
        Key { t, v }
    }

    fn lab() -> (Topology, u32, u32, u32) {
        let mut t = Topology::new();
        let city = t.add_place(PlaceKind::City, Some("Metropolis"), None, 0.0, 0.0).unwrap();
        let building = t.add_place(PlaceKind::Building, Some("Tower"), Some(city), 0.0, 0.0).unwrap();
        let closet = t.add_place(PlaceKind::Closet, Some("Rack room"), Some(building), 0.0, 0.0).unwrap();
        (t, city, building, closet)
    }

    #[test]
    fn variables_have_unique_ids_defaults_in_range_and_known_groups() {
        let mut ids = std::collections::BTreeSet::new();
        for v in VARS {
            assert!(ids.insert(v.id), "{} twice", v.id);
            assert!(GROUPS.contains(&v.group), "{}", v.id);
            assert!(v.min <= v.default && v.default <= v.max, "{} default {} outside {}..{}", v.id, v.default, v.min, v.max);
        }
        for g in GROUPS {
            assert!(VARS.iter().any(|v| v.group == g), "{g} is empty");
        }
        assert_eq!(var_by_id("gravity").map(|v| v.default), Some(9.806_65));
        assert_eq!(var_by_id("atmospheric_pressure").map(|v| v.default), Some(101.325));
        assert_eq!(find_var("Wind speed").map(|v| v.id), Some("wind_speed"));
        assert_eq!(sensor_var("thermostat").map(|v| v.id), Some("ambient_temperature"));
        assert_eq!(sensor_var("soil").map(|v| v.id), Some("soil_moisture"));
        assert!(sensor_var("motion").is_none());
    }

    #[test]
    fn keyframes_interpolate_in_a_line_and_wrap_around_midnight() {
        let keys = [key(6 * 3_600, 10.0), key(18 * 3_600, 30.0)];
        assert_eq!(interpolate(&keys, 6 * 3_600), Some(10.0));
        assert_eq!(interpolate(&keys, 12 * 3_600), Some(20.0));
        assert_eq!(interpolate(&keys, 18 * 3_600), Some(30.0));
        // Midnight is halfway down the way from 30 at 18:00 to 10 at 06:00 the next day.
        assert_eq!(interpolate(&keys, 0), Some(20.0));
        assert_eq!(interpolate(&keys, 21 * 3_600), Some(25.0));
        assert_eq!(interpolate(&[key(40_000, 7.0)], 5), Some(7.0), "one keyframe is a constant");
        assert_eq!(interpolate(&[], 5), None);
    }

    #[test]
    fn a_place_inherits_from_the_places_above_and_overrides_its_own() {
        let (mut t, city, building, closet) = lab();
        assert_eq!(t.env_chain(closet), [closet, building, city, INTERCITY]);
        assert_eq!(t.env_value(closet, "ambient_temperature", 0), Some((15.0, Source::Default)));
        t.env_set(city, "ambient_temperature", 22.0).unwrap();
        assert_eq!(t.env_value(closet, "ambient_temperature", 0), Some((22.0, Source::Inherited(city))));
        t.env_set(INTERCITY, "gravity", 1.62).unwrap();
        assert_eq!(t.env_value(building, "gravity", 0), Some((1.62, Source::Inherited(INTERCITY))));
        t.env_set(closet, "ambient_temperature", 18.5).unwrap();
        assert_eq!(t.env_value(closet, "ambient_temperature", 0), Some((18.5, Source::Own)));
        assert_eq!(t.env_value(building, "ambient_temperature", 0), Some((22.0, Source::Inherited(city))));
        t.env_clear(closet, Some("ambient_temperature")).unwrap();
        assert_eq!(t.env_value(closet, "ambient_temperature", 0), Some((22.0, Source::Inherited(city))));
        assert!(t.env_value(closet, "nope", 0).is_none());
    }

    #[test]
    fn keyframes_add_replace_remove_and_stay_sorted() {
        let (mut t, _, _, closet) = lab();
        t.env_add_key(closet, "humidity", 43_200, 70.0).unwrap();
        t.env_add_key(closet, "humidity", 0, 30.0).unwrap();
        t.env_add_key(closet, "humidity", 43_200, 80.0).unwrap();
        let keys = t.place_env(closet).unwrap().setting("humidity").unwrap().keys.clone();
        assert_eq!(keys, [key(0, 30.0), key(43_200, 80.0)]);
        assert_eq!(t.env_value(closet, "humidity", 21_600).map(|x| x.0), Some(55.0));
        assert!(t.env_add_key(closet, "humidity", DAY_SECONDS, 1.0).is_err());
        assert!(t.env_add_key(closet, "humidity", 5, 101.0).is_err(), "out of range");
        assert!(t.env_add_key(closet, "humidity", 5, f64::NAN).is_err());
        assert!(t.env_add_key(closet, "nope", 5, 1.0).is_err());
        assert!(t.env_remove_key(closet, "humidity", 77).is_err());
        t.env_remove_key(closet, "humidity", 43_200).unwrap();
        t.env_remove_key(closet, "humidity", 0).unwrap();
        assert!(t.place_env(closet).is_none(), "an empty environment is not kept");
        assert_eq!(t.env_value(closet, "humidity", 0), Some((50.0, Source::Default)));
        for n in 0..=MAX_KEYS {
            let r = t.env_add_key(closet, "rain", u32::try_from(n).unwrap() * 60, 1.0);
            assert_eq!(r.is_ok(), n < MAX_KEYS);
        }
    }

    #[test]
    fn the_clock_follows_simulated_time_pauses_and_changes_its_ratio_without_jumping() {
        let mut c = Clock::default();
        assert_eq!(c.time_at(0), DEFAULT_START);
        assert_eq!(c.time_at(90_000_000), DEFAULT_START + 90);
        // 1 minute of simulation equals 60 minutes of environment time.
        c.set_scale(0, 1, 60);
        assert_eq!(c.time_at(60_000_000), DEFAULT_START + 3_600);
        // A change at one simulated minute keeps the time shown then.
        c.set_scale(60_000_000, 1, 1);
        assert_eq!(c.time_at(60_000_000), DEFAULT_START + 3_600);
        assert_eq!(c.time_at(120_000_000), DEFAULT_START + 3_660);
        c.set_paused(120_000_000, true);
        assert!(c.paused());
        assert_eq!(c.time_at(999_000_000), DEFAULT_START + 3_660);
        c.set_time(999_000_000, 5 * 3_600);
        assert_eq!(c.time_at(0), 5 * 3_600);
        c.set_paused(999_000_000, false);
        assert_eq!(c.time_at(999_000_000), 5 * 3_600);
        assert_eq!(c.time_at(1_059_000_000), 5 * 3_600 + 60);
        // The day wraps.
        c.set_time(0, DAY_SECONDS - 10);
        assert_eq!(c.time_at(20_000_000), 10);
        c.set_scale(0, 0, 100_000);
        assert_eq!((c.real_minutes, c.sim_minutes), (1, MAX_SCALE));
    }

    #[test]
    fn times_parse_and_print() {
        assert_eq!(parse_time("07:30"), Some(27_000));
        assert_eq!(parse_time("07:30:15"), Some(27_015));
        assert_eq!(parse_time("86399"), Some(86_399));
        assert_eq!(parse_time("24:00:00"), None);
        assert_eq!(parse_time("a:b"), None);
        assert_eq!(parse_time("1:2:3:4"), None);
        assert_eq!(format_time(27_015), "07:30:15");
        assert_eq!(format_time(DAY_SECONDS + 5), "00:00:05");
    }

    #[test]
    fn a_sensor_reads_a_variable_only_where_one_is_set() {
        let (mut t, city, _, closet) = lab();
        assert_eq!(t.env_reading(closet, "temperature", 0), None);
        t.env_set(city, "ambient_temperature", 21.6).unwrap();
        assert_eq!(t.env_reading(closet, "thermostat", 0), Some(22));
        t.env_add_key(closet, "carbon_monoxide", 0, 0.0).unwrap();
        t.env_add_key(closet, "carbon_monoxide", 43_200, 400.0).unwrap();
        assert_eq!(t.env_reading(closet, "co", 21_600), Some(200));
        assert_eq!(t.env_reading(closet, "motion", 0), None);
        assert_eq!(t.env_reading(closet, "humidity", 0), None);
    }

    #[test]
    fn a_removed_place_takes_its_environment_with_it() {
        let (mut t, _, building, closet) = lab();
        t.env_set(closet, "ozone", 1.0).unwrap();
        t.env_set(building, "ozone", 2.0).unwrap();
        t.remove_place(closet).unwrap();
        t.prune_environments();
        assert!(t.place_env(closet).is_none());
        assert!(t.place_env(building).is_some());
        assert!(t.env_set(closet, "ozone", 1.0).is_err());
        assert!(t.env_set(9_999, "ozone", 1.0).is_err());
    }

    #[test]
    fn export_then_import_round_trips_and_merge_keeps_the_rest() {
        let (mut t, city, _, closet) = lab();
        t.env_add_key(closet, "ambient_temperature", 21_600, 12.0).unwrap();
        t.env_add_key(closet, "ambient_temperature", 50_400, 24.5).unwrap();
        t.env_set(closet, "gravity", 3.71).unwrap();
        t.env_notes(closet, Some("Cold aisle"), Some(true)).unwrap();
        let text = t.env_export(closet).unwrap();
        assert!(text.contains("netlabcraft-environment") && text.contains("06:00:00"), "{text}");
        t.env_import(city, &text, false).unwrap();
        assert_eq!(t.place_env(city).unwrap().settings, t.place_env(closet).unwrap().settings);
        assert_eq!(t.place_env(city).unwrap().notes, "Cold aisle");
        t.env_set(city, "ozone", 4.0).unwrap();
        let (read, skipped) = t.env_import(city, &text, true).unwrap();
        assert_eq!((read, skipped), (2, 0));
        assert!(t.place_env(city).unwrap().setting("ozone").is_some(), "merge keeps what the file does not name");
        t.env_import(city, &text, false).unwrap();
        assert!(t.place_env(city).unwrap().setting("ozone").is_none(), "replace drops it");
    }

    #[test]
    fn import_refuses_what_is_not_an_environment_and_skips_unknown_variables() {
        let (mut t, _, _, closet) = lab();
        for bad in [
            "",
            "{",
            "[]",
            r#"{"format":"other","version":1,"variables":{}}"#,
            r#"{"format":"netlabcraft-environment","version":99,"variables":{}}"#,
            r#"{"format":"netlabcraft-environment","version":1}"#,
            r#"{"format":"netlabcraft-environment","version":1,"variables":{"gravity":[{"t":"xx","v":1}]}}"#,
            r#"{"format":"netlabcraft-environment","version":1,"variables":{"gravity":[{"t":"00:00","v":"1"}]}}"#,
        ] {
            assert!(t.env_import(closet, bad, false).is_err(), "{bad}");
        }
        assert!(t.env_import(closet, &"x".repeat(MAX_IMPORT_BYTES + 1), false).is_err());
        assert!(t.env_import(9_999, r#"{"format":"netlabcraft-environment","version":1,"variables":{}}"#, false).is_err());
        let ok = r#"{"format":"netlabcraft-environment","version":1,"variables":{"Wind speed":[{"t":"00:00:00","v":9999}],"unicorns":[{"t":0,"v":1}],"gravity":5}}"#;
        let (read, skipped) = t.env_import(closet, ok, false).unwrap();
        assert_eq!((read, skipped), (1, 2));
        assert_eq!(t.env_value(closet, "wind_speed", 0).map(|x| x.0), Some(500.0), "clamped to the range");
    }

    #[test]
    fn a_hostile_file_is_repaired() {
        let (mut t, _, _, closet) = lab();
        t.environments.clock = Clock { start: 999_999, frozen: Some(999_999), real_minutes: 0, sim_minutes: u32::MAX };
        t.environments.places = vec![
            PlaceEnv {
                place: closet,
                settings: vec![
                    Setting {
                        id: "gravity".into(),
                        keys: vec![key(500, 1.0), key(10, f64::INFINITY), key(10, 2.0), key(10, 3.0), key(DAY_SECONDS, 4.0)],
                    },
                    Setting { id: "gravity".into(), keys: vec![key(1, 1.0)] },
                    Setting { id: "bogus".into(), keys: vec![key(1, 1.0)] },
                    Setting { id: "rain".into(), keys: Vec::new() },
                ],
                notes: "n".repeat(MAX_NOTES + 10),
                show_notes: false,
            },
            PlaceEnv { place: closet, ..PlaceEnv::default() },
            PlaceEnv { place: 777, settings: vec![Setting { id: "gravity".into(), keys: vec![key(1, 1.0)] }], ..PlaceEnv::default() },
        ];
        t.sanitize_environments();
        assert_eq!(t.environments.places.len(), 1);
        let e = &t.environments.places[0];
        assert_eq!(e.settings.len(), 1);
        assert_eq!(e.settings[0].keys, [key(10, 2.0), key(500, 1.0)]);
        assert_eq!(e.notes.chars().count(), MAX_NOTES);
        let c = t.environments.clock;
        assert!(c.start < DAY_SECONDS && c.frozen.is_some_and(|f| f < DAY_SECONDS) && c.real_minutes == 1 && c.sim_minutes == MAX_SCALE);
    }

    #[test]
    fn it_survives_a_save_and_an_old_lab_has_none() {
        let (mut t, _, _, closet) = lab();
        assert!(t.environments.is_default());
        let plain = crate::file::to_json(&t).unwrap();
        assert!(!plain.contains("environments"), "nothing set, nothing written");
        t.env_add_key(closet, "wind_speed", 3_600, 12.0).unwrap();
        t.environments.clock.set_paused(0, true);
        let back = crate::file::from_json(&crate::file::to_json(&t).unwrap()).unwrap();
        assert_eq!(back.environments, t.environments);
    }
}
