//! What a place's environment can have besides its keyframes: a name of its own (a custom
//! environment), variables of its own computed by a formula from other variables and the time of
//! day, the choice of what the window shows ("shows", also copied from the place above), how the
//! keyframes are joined, and a few bundled environments of ours to start from.
//!
//! Formulas are the bounded language of [`crate::expr`]. A custom variable is read by the generic
//! environment sensor and shown by `environment.get`; it is computed from the values the places
//! *set* (not from what parts of the place do to them), so it can never depend on a smart thing.

use serde::{Deserialize, Serialize};

use crate::environments::{DAY_SECONDS, INTERCITY, MAX_IMPORT_BYTES, VARS, var_by_id};
use crate::expr;
use crate::{ModelError, Result, Topology};

/// The most custom variables one place defines, the longest name of a custom environment, and the
/// most names a place leaves out of its shows.
pub const MAX_CUSTOM: usize = 16;
pub const MAX_NAME: usize = 60;
pub const MAX_HIDDEN: usize = 400;
/// How deep custom variables may refer to custom variables (a longer chain, or a loop, is an error
/// and the variable reads its default).
const MAX_CHAIN: usize = 6;

/// How the keyframes of a place are joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interp {
    /// A straight line between two keyframes (the default).
    #[default]
    Linear,
    /// The value of the last keyframe until the next one.
    Step,
    /// A half cosine between two keyframes: flat at each, steepest between.
    Smooth,
}

impl Interp {
    pub fn id(self) -> &'static str {
        match self {
            Interp::Linear => "linear",
            Interp::Step => "step",
            Interp::Smooth => "smooth",
        }
    }

    pub fn parse(s: &str) -> Option<Interp> {
        match s.trim().to_ascii_lowercase().as_str() {
            "linear" => Some(Interp::Linear),
            "step" => Some(Interp::Step),
            "smooth" => Some(Interp::Smooth),
            _ => None,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Interp::Linear
    }
}

/// A variable a place defines for itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomVar {
    /// `a` to `z`, digits and `_`, up to 32 characters, and not the id of a built-in variable.
    pub id: String,
    pub label: String,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    /// What it reads when its formula is empty or cannot be computed.
    pub default: f64,
    pub formula: String,
}

impl Default for CustomVar {
    fn default() -> Self {
        CustomVar {
            id: String::new(),
            label: String::new(),
            unit: String::new(),
            min: -1_000_000.0,
            max: 1_000_000.0,
            default: 0.0,
            formula: String::new(),
        }
    }
}

/// Whether text can be the id of a custom variable.
pub fn valid_custom_id(id: &str) -> bool {
    let mut cs = id.chars();
    cs.next().is_some_and(|c| c.is_ascii_lowercase())
        && id.chars().count() <= 32
        && cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A bundled environment of ours: values that are plausible for the kind of place, not data of any
/// real place.
pub struct Bundled {
    pub id: &'static str,
    pub label: &'static str,
    /// An exported environment (`docs/file-format.md`).
    pub text: &'static str,
}

pub static BUNDLED: [Bundled; 5] = [
    Bundled {
        id: "desert",
        label: "Desert",
        text: r#"{"format":"netlabcraft-environment","version":1,"notes":"A hot dry desert: cold nights, a scorching afternoon, almost no water.","show_notes":true,"variables":{
            "ambient_temperature":[{"t":"05:00","v":22},{"t":"15:00","v":42}],"humidity":[{"t":"00:00","v":12}],"soil_moisture":[{"t":"00:00","v":3}],
            "wind_speed":[{"t":"00:00","v":18}],"atmospheric_pressure":[{"t":"00:00","v":99}],"elevation":[{"t":"00:00","v":300}],
            "clouds":[{"t":"00:00","v":5}],"rain":[{"t":"00:00","v":0}]}}"#,
    },
    Bundled {
        id: "tropical",
        label: "Tropical",
        text: r#"{"format":"netlabcraft-environment","version":1,"notes":"A warm, wet rainforest: steady heat, heavy air and daily rain.","show_notes":true,"variables":{
            "ambient_temperature":[{"t":"04:00","v":24},{"t":"14:00","v":31}],"humidity":[{"t":"06:00","v":90},{"t":"14:00","v":75}],
            "soil_moisture":[{"t":"00:00","v":75}],"rain":[{"t":"00:00","v":0.4}],"clouds":[{"t":"00:00","v":70}],
            "wind_speed":[{"t":"00:00","v":10}],"atmospheric_pressure":[{"t":"00:00","v":101}]}}"#,
    },
    Bundled {
        id: "arctic",
        label: "Arctic",
        text: r#"{"format":"netlabcraft-environment","version":1,"notes":"Polar winter: deep cold, snow on the ground, a low pale sun and a steady wind.","show_notes":true,"variables":{
            "ambient_temperature":[{"t":"04:00","v":-32},{"t":"14:00","v":-20}],"humidity":[{"t":"00:00","v":70}],"snow":[{"t":"00:00","v":35}],
            "wind_speed":[{"t":"00:00","v":30}],"atmospheric_pressure":[{"t":"00:00","v":100.5}],"sunlight":[{"t":"00:00","v":40}],
            "soil_moisture":[{"t":"00:00","v":10}],"clouds":[{"t":"00:00","v":50}]}}"#,
    },
    Bundled {
        id: "indoor-office",
        label: "Indoor office",
        text: r#"{"format":"netlabcraft-environment","version":1,"notes":"An air-conditioned office: still air, steady temperature, some carbon dioxide from the people in it.","show_notes":true,"variables":{
            "ambient_temperature":[{"t":"06:00","v":20},{"t":"13:00","v":23},{"t":"20:00","v":20}],"humidity":[{"t":"00:00","v":45}],
            "wind_speed":[{"t":"00:00","v":0}],"luminosity":[{"t":"00:00","v":60}],"carbon_dioxide":[{"t":"00:00","v":0.07}],
            "atmospheric_pressure":[{"t":"00:00","v":101.3}]}}"#,
    },
    Bundled {
        id: "greenhouse",
        label: "Greenhouse",
        text: r#"{"format":"netlabcraft-environment","version":1,"notes":"A glasshouse for plants: warm and humid by day, rich in carbon dioxide, moist soil and no wind.","show_notes":true,"variables":{
            "ambient_temperature":[{"t":"00:00","v":22},{"t":"13:00","v":30}],"humidity":[{"t":"00:00","v":75}],"soil_moisture":[{"t":"00:00","v":60}],
            "carbon_dioxide":[{"t":"00:00","v":0.08}],"luminosity":[{"t":"00:00","v":70}],"photon":[{"t":"00:00","v":350}],
            "soil_ph":[{"t":"00:00","v":6.5}],"wind_speed":[{"t":"00:00","v":0}]}}"#,
    },
];

fn invalid(msg: impl Into<String>) -> ModelError {
    ModelError::Invalid(msg.into())
}

/// A custom variable as the window shows it: the definition and what it reads now.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomRead {
    pub def: CustomVar,
    /// The place that defines it.
    pub from: u32,
    pub value: f64,
    /// Why the formula could not be computed (the default is shown instead).
    pub error: Option<String>,
}

impl Topology {
    /// The custom variables a place reads: its own and those of the places above (a place's own
    /// definition of an id hides the one above), each with the place that defines it.
    pub fn env_customs(&self, place: u32) -> Vec<(u32, CustomVar)> {
        let mut out: Vec<(u32, CustomVar)> = Vec::new();
        for p in self.env_chain(place) {
            for c in self.place_env(p).map(|e| e.custom.as_slice()).unwrap_or_default() {
                if !out.iter().any(|(_, x)| x.id == c.id) {
                    out.push((p, c.clone()));
                }
            }
        }
        out
    }

    /// A name a formula may use at a place: the time (`time` in seconds, `hour`, `day` from 0 to
    /// 1), a built-in variable's value, or a custom variable (computed in turn).
    fn env_name_value(&self, place: u32, name: &str, time: u32, depth: usize) -> Option<f64> {
        match name {
            "time" => return Some(f64::from(time)),
            "hour" => return Some(f64::from(time) / 3_600.0),
            "day" => return Some(f64::from(time) / f64::from(DAY_SECONDS)),
            _ => {}
        }
        if let Some(v) = var_by_id(name) {
            return self.env_value(place, v.id, time).map(|x| x.0);
        }
        let (_, def) = self.env_customs(place).into_iter().find(|(_, c)| c.id == name)?;
        self.env_custom_compute(place, &def, time, depth + 1).ok()
    }

    fn env_custom_compute(&self, place: u32, def: &CustomVar, time: u32, depth: usize) -> std::result::Result<f64, String> {
        if depth > MAX_CHAIN {
            return Err("variables refer to each other too deeply (or in a circle)".into());
        }
        if def.formula.trim().is_empty() {
            return Ok(def.default);
        }
        let value = expr::parse(&def.formula)?
            .eval(&|n| self.env_name_value(place, n, time, depth))
            .map_err(|e| if e.ends_with("has no value") { format!("{e}: a variable it reads fails or refers back to it") } else { e })?;
        Ok(value.clamp(def.min.min(def.max), def.max.max(def.min)))
    }

    /// What a custom variable reads at a place and a time of day (`None`: no such variable there).
    pub fn env_custom_read(&self, place: u32, id: &str, time: u32) -> Option<CustomRead> {
        let (from, def) = self.env_customs(place).into_iter().find(|(_, c)| c.id == id)?;
        let (value, error) = match self.env_custom_compute(place, &def, time, 0) {
            Ok(v) => (v, None),
            Err(e) => (def.default, Some(e)),
        };
        Some(CustomRead { def, from, value, error })
    }

    /// Defines (or redefines) a custom variable of a place. The formula may name `time`, `hour`,
    /// `day`, any built-in variable and any custom variable the place reads (including this one's
    /// neighbours, but a name that exists nowhere is refused).
    pub fn env_custom_set(&mut self, place: u32, def: CustomVar) -> Result<()> {
        if !valid_custom_id(&def.id) {
            return Err(invalid("a variable id is lowercase letters, digits and `_`, starting with a letter, up to 32 characters"));
        }
        if var_by_id(&def.id).is_some() || matches!(def.id.as_str(), "time" | "hour" | "day" | "pi" | "e") {
            return Err(invalid(format!("`{}` is already the name of something else", def.id)));
        }
        if !(def.min.is_finite() && def.max.is_finite() && def.default.is_finite()) || def.min > def.max {
            return Err(invalid("min, max and default are numbers with min not above max"));
        }
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        if !def.formula.trim().is_empty() {
            let known: Vec<String> = self.env_customs(place).into_iter().map(|(_, c)| c.id).collect();
            for n in expr::parse(&def.formula).map_err(invalid)?.names() {
                let ok = matches!(n.as_str(), "time" | "hour" | "day") || var_by_id(&n).is_some() || n == def.id || known.contains(&n);
                if !ok {
                    return Err(invalid(format!("`{n}` is not a variable: use time, hour, day, a built-in variable or a custom one")));
                }
            }
        }
        let own = self.place_env(place).map_or(0, |e| e.custom.iter().filter(|c| c.id != def.id).count());
        if own >= MAX_CUSTOM {
            return Err(invalid(format!("a place defines at most {MAX_CUSTOM} variables")));
        }
        let def = CustomVar {
            label: def.label.chars().filter(|c| !c.is_control()).take(40).collect(),
            unit: def.unit.chars().filter(|c| !c.is_control()).take(16).collect(),
            ..def
        };
        let e = self.place_env_mut(place)?;
        e.custom.retain(|c| c.id != def.id);
        e.custom.push(def);
        self.tidy_environments();
        Ok(())
    }

    /// Removes a custom variable a place defines.
    pub fn env_custom_remove(&mut self, place: u32, id: &str) -> Result<()> {
        let had = self.place_env(place).is_some_and(|e| e.custom.iter().any(|c| c.id == id));
        if !had {
            return Err(invalid(format!("this place does not define `{id}`")));
        }
        let e = self.place_env_mut(place)?;
        e.custom.retain(|c| c.id != id);
        self.tidy_environments();
        Ok(())
    }

    /// Gives a place a custom environment of its own with a name.
    pub fn env_create(&mut self, place: u32, name: &str) -> Result<()> {
        let name: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_NAME).collect();
        if name.is_empty() {
            return Err(invalid("a custom environment needs a name"));
        }
        self.place_env_mut(place)?.name = name;
        self.tidy_environments();
        Ok(())
    }

    /// The names a place's window leaves out: the nearest place that says (itself first), `None`
    /// when none does.
    pub fn env_hidden(&self, place: u32) -> Option<(u32, Vec<String>)> {
        self.env_chain(place).into_iter().find_map(|p| self.place_env(p).and_then(|e| e.hidden.clone()).map(|h| (p, h)))
    }

    fn known_name(&self, place: u32, id: &str) -> bool {
        var_by_id(id).is_some() || self.env_customs(place).iter().any(|(_, c)| c.id == id)
    }

    /// Says what the window leaves out for a place (`None`: it reads the place above again).
    pub fn env_set_hidden(&mut self, place: u32, hidden: Option<Vec<String>>) -> Result<()> {
        let cleaned = match hidden {
            None => None,
            Some(list) => {
                let mut out: Vec<String> = Vec::new();
                for id in list {
                    if !self.known_name(place, &id) {
                        return Err(invalid(format!("unknown variable `{id}`")));
                    }
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
                out.truncate(MAX_HIDDEN);
                Some(out)
            }
        };
        self.place_env_mut(place)?.hidden = cleaned;
        self.tidy_environments();
        Ok(())
    }

    /// Copy Shows from Parent: the place says what the place above says.
    pub fn env_copy_shows_from_parent(&mut self, place: u32) -> Result<()> {
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        let parent = self.env_chain(place).get(1).copied().unwrap_or(INTERCITY);
        let shows = self.env_hidden(parent).map(|x| x.1).unwrap_or_default();
        self.env_set_hidden(place, Some(shows))
    }

    /// Show Only Actives: the place leaves out every variable that no place from it up sets.
    pub fn env_show_only_actives(&mut self, place: u32) -> Result<()> {
        if !self.env_place_exists(place) {
            return Err(invalid(format!("no place {place}")));
        }
        let hidden: Vec<String> = VARS.iter().filter(|v| self.env_setting(place, v.id).is_none()).map(|v| v.id.to_string()).collect();
        self.env_set_hidden(place, Some(hidden))
    }

    /// How a place's keyframes are joined.
    pub fn env_set_interpolation(&mut self, place: u32, mode: Interp) -> Result<()> {
        self.place_env_mut(place)?.interpolation = mode;
        self.tidy_environments();
        Ok(())
    }

    /// Applies one of the bundled environments to a place, as an import would (what the place set
    /// is replaced) and names the custom environment after it. Returns how many variables it set.
    pub fn env_apply_bundled(&mut self, place: u32, id: &str) -> Result<usize> {
        let b = BUNDLED
            .iter()
            .find(|b| b.id.eq_ignore_ascii_case(id.trim()))
            .ok_or_else(|| invalid(format!("no bundled environment `{id}` ({})", BUNDLED.iter().map(|b| b.id).collect::<Vec<_>>().join(", "))))?;
        debug_assert!(b.text.len() < MAX_IMPORT_BYTES);
        let (read, _) = self.env_import(place, b.text, false)?;
        self.place_env_mut(place)?.name = b.label.to_string();
        self.tidy_environments();
        Ok(read)
    }

    /// Repairs the custom parts of a hostile file's environments.
    pub(crate) fn sanitize_custom_environments(&mut self) {
        self.sanitize_custom_places();
        // A sensor that reads a custom variable no place defines reads its own number again.
        let defined: std::collections::BTreeSet<String> =
            self.environments.places.iter().flat_map(|e| e.custom.iter().map(|c| c.id.clone())).collect();
        for d in &mut self.devices {
            if !d.iot.variable.is_empty() && var_by_id(&d.iot.variable).is_none() && !defined.contains(&d.iot.variable) {
                d.iot.variable.clear();
            }
        }
    }

    fn sanitize_custom_places(&mut self) {
        for e in &mut self.environments.places {
            e.name = e.name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
            let mut seen: Vec<String> = Vec::new();
            e.custom.retain(|c| {
                valid_custom_id(&c.id) && var_by_id(&c.id).is_none() && !matches!(c.id.as_str(), "time" | "hour" | "day" | "pi" | "e") && {
                    let fresh = !seen.contains(&c.id);
                    seen.push(c.id.clone());
                    fresh
                }
            });
            e.custom.truncate(MAX_CUSTOM);
            for c in &mut e.custom {
                c.label = c.label.chars().filter(|c| !c.is_control()).take(40).collect();
                c.unit = c.unit.chars().filter(|c| !c.is_control()).take(16).collect();
                for v in [&mut c.min, &mut c.max, &mut c.default] {
                    if !v.is_finite() {
                        *v = 0.0;
                    }
                }
                if c.min > c.max {
                    std::mem::swap(&mut c.min, &mut c.max);
                }
                c.default = c.default.clamp(c.min, c.max);
                if c.formula.chars().count() > expr::MAX_FORMULA || expr::parse(&c.formula).is_err() {
                    c.formula.clear();
                }
            }
            if let Some(h) = &mut e.hidden {
                h.retain(|id| valid_custom_id(id) || var_by_id(id).is_some());
                h.dedup();
                h.truncate(MAX_HIDDEN);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::PlaceKind;

    fn lab() -> (Topology, u32, u32) {
        let mut t = Topology::new();
        let city = t.add_place(PlaceKind::City, Some("Harbor"), None, 0.0, 0.0).unwrap();
        let building = t.add_place(PlaceKind::Building, Some("Annex"), Some(city), 0.0, 0.0).unwrap();
        let closet = t.add_place(PlaceKind::Closet, Some("Cold room"), Some(building), 0.0, 0.0).unwrap();
        (t, city, closet)
    }

    fn def(id: &str, formula: &str) -> CustomVar {
        CustomVar { id: id.into(), formula: formula.into(), min: -100.0, max: 100.0, ..CustomVar::default() }
    }

    #[test]
    fn a_formula_reads_the_time_other_variables_and_other_custom_variables() {
        let (mut t, city, closet) = lab();
        t.env_set(city, "ambient_temperature", 10.0).unwrap();
        t.env_custom_set(city, def("feels", "ambient_temperature - wind_speed / 5")).unwrap();
        t.env_custom_set(closet, def("double", "feels * 2")).unwrap();
        t.env_custom_set(closet, def("wave", "20 + 8 * sin((hour - 9) / 24 * 2 * pi)")).unwrap();
        t.env_set(INTERCITY, "wind_speed", 10.0).unwrap();
        let read = |t: &Topology, id: &str, time: u32| t.env_custom_read(closet, id, time).unwrap();
        assert_eq!(read(&t, "feels", 0).value, 8.0, "inherited from the city, computed at the closet");
        assert_eq!(read(&t, "feels", 0).from, city);
        assert_eq!(read(&t, "double", 0).value, 16.0);
        assert!((read(&t, "wave", 15 * 3_600).value - 28.0).abs() < 1e-9);
        assert!((read(&t, "wave", 3 * 3_600).value - 12.0).abs() < 1e-9);
        // The city does not see the closet's variables.
        assert!(t.env_custom_read(city, "double", 0).is_none());
        // A place's own definition hides the one above it.
        t.env_custom_set(closet, def("feels", "99")).unwrap();
        assert_eq!(read(&t, "feels", 0).value, 99.0);
        // The value is kept within the variable's range.
        t.env_custom_set(closet, CustomVar { max: 50.0, ..def("capped", "1000") }).unwrap();
        assert_eq!(read(&t, "capped", 0).value, 50.0);
    }

    #[test]
    fn mistakes_loops_and_hostile_files_are_refused_or_repaired() {
        let (mut t, _, closet) = lab();
        for (id, formula, why) in [
            ("Bad", "1", "lowercase"),
            ("humidity", "1", "already the name"),
            ("time", "1", "already the name"),
            ("x", "1 +", "ends too soon"),
            ("x", "nowhere + 1", "not a variable"),
            ("x", "1 / 0 +", "ends too soon"),
        ] {
            let e = t.env_custom_set(closet, def(id, formula)).unwrap_err().to_string();
            assert!(e.contains(why), "{id} {formula}: {e}");
        }
        assert!(t.env_custom_set(closet, CustomVar { min: 5.0, max: 1.0, ..def("x", "1") }).is_err());
        assert!(t.env_custom_set(closet, CustomVar { default: f64::NAN, ..def("x", "1") }).is_err());
        // A loop reads its default and says why.
        t.env_custom_set(closet, def("a", "b + 1")).unwrap_err();
        t.env_custom_set(closet, def("b", "1")).unwrap();
        t.env_custom_set(closet, def("a", "b + 1")).unwrap();
        t.env_custom_set(closet, def("b", "a + 1")).unwrap();
        let r = t.env_custom_read(closet, "a", 0).unwrap();
        assert!(r.error.as_deref().is_some_and(|e| e.contains("refers back")), "{r:?}");
        assert_eq!(r.value, 0.0);
        // At most sixteen.
        for i in 0..MAX_CUSTOM {
            let _ = t.env_custom_set(closet, def(&format!("v{i}"), "1"));
        }
        assert!(t.env_custom_set(closet, def("onetoomany", "1")).unwrap_err().to_string().contains("at most"));
        t.env_custom_set(closet, def("v0", "2")).unwrap();
        assert!(t.env_custom_remove(closet, "nope").is_err());
        t.env_custom_remove(closet, "v0").unwrap();
        // A hostile file.
        let mut bad = t.clone();
        let e = bad.environments.places.iter_mut().find(|e| e.place == closet).unwrap();
        e.custom = vec![
            CustomVar {
                id: "ok".into(),
                formula: "((((((((((((((((1))))))))))))))))".into(),
                min: 9.0,
                max: -9.0,
                default: f64::NAN,
                ..CustomVar::default()
            },
            CustomVar { id: "ok".into(), ..CustomVar::default() },
            CustomVar { id: "humidity".into(), ..CustomVar::default() },
            CustomVar { id: "Not Valid".into(), ..CustomVar::default() },
        ];
        e.hidden = Some(vec!["humidity".into(), "humidity".into(), "Not Valid!".into()]);
        e.name = "x\n".repeat(100);
        bad.sanitize_environments();
        let e = bad.place_env(closet).unwrap();
        assert_eq!(e.custom.len(), 1);
        assert_eq!((e.custom[0].min, e.custom[0].max, e.custom[0].default, e.custom[0].formula.as_str()), (-9.0, 9.0, 0.0, ""));
        assert_eq!(e.hidden, Some(vec!["humidity".to_string()]));
        assert!(e.name.chars().count() <= MAX_NAME && !e.name.contains('\n'));
    }

    #[test]
    fn a_place_gets_a_name_shows_from_its_parent_and_a_choice_of_interpolation() {
        let (mut t, city, closet) = lab();
        assert!(t.env_create(closet, "  ").is_err());
        t.env_create(closet, "Cold room environment").unwrap();
        assert_eq!(t.place_env(closet).unwrap().name, "Cold room environment");
        // Shows: the city leaves out the gases; the closet copies that.
        t.env_set_hidden(city, Some(vec!["argon".into(), "nitrogen".into()])).unwrap();
        assert_eq!(t.env_hidden(closet).map(|x| x.0), Some(city), "inherited");
        t.env_set_hidden(closet, Some(vec![])).unwrap();
        assert_eq!(t.env_hidden(closet), Some((closet, vec![])), "its own, showing everything");
        t.env_copy_shows_from_parent(closet).unwrap();
        assert_eq!(t.env_hidden(closet), Some((closet, vec!["argon".to_string(), "nitrogen".to_string()])));
        assert!(t.env_set_hidden(closet, Some(vec!["unicorns".into()])).is_err());
        t.env_set(closet, "humidity", 55.0).unwrap();
        t.env_show_only_actives(closet).unwrap();
        let hidden = t.env_hidden(closet).unwrap().1;
        assert!(!hidden.contains(&"humidity".to_string()) && hidden.contains(&"gravity".to_string()), "only what is set is shown");
        t.env_set_hidden(closet, None).unwrap();
        assert_eq!(t.env_hidden(closet).map(|x| x.0), Some(city));
        // Interpolation.
        t.env_clear(closet, Some("humidity")).unwrap();
        t.env_add_key(closet, "humidity", 0, 20.0).unwrap();
        t.env_add_key(closet, "humidity", 10_000, 60.0).unwrap();
        let at = |t: &Topology| t.env_value(closet, "humidity", 5_000).unwrap().0;
        assert_eq!(at(&t), 40.0);
        t.env_set_interpolation(closet, Interp::Step).unwrap();
        assert_eq!(at(&t), 20.0);
        t.env_set_interpolation(closet, Interp::Smooth).unwrap();
        assert!((at(&t) - 40.0).abs() < 1e-9, "halfway is halfway on a half cosine too");
        let quarter = t.env_value(closet, "humidity", 2_500).unwrap().0;
        assert!(quarter < 30.0 && quarter > 20.0, "flatter near the keyframe: {quarter}");
        assert_eq!(Interp::parse("STEP"), Some(Interp::Step));
        assert_eq!(Interp::parse("cubic"), None);
    }

    #[test]
    fn every_bundled_environment_imports_with_plausible_values_in_range() {
        let (mut t, _, closet) = lab();
        assert_eq!(BUNDLED.len(), 5);
        for b in &BUNDLED {
            let read = t.env_apply_bundled(closet, b.id).unwrap();
            assert!(read >= 6, "{}: {read} variables", b.id);
            assert_eq!(t.place_env(closet).unwrap().name, b.label);
            let doc: serde_json::Value = serde_json::from_str(b.text).unwrap();
            for (name, keys) in doc["variables"].as_object().unwrap() {
                let v = var_by_id(name).unwrap_or_else(|| panic!("{}: unknown variable {name}", b.id));
                for k in keys.as_array().unwrap() {
                    let x = k["v"].as_f64().unwrap();
                    assert!((v.min..=v.max).contains(&x), "{}: {name} = {x} is outside {}..{}", b.id, v.min, v.max);
                }
            }
        }
        assert!(t.env_value(closet, "ambient_temperature", 0).unwrap().0 < 40.0);
        t.env_apply_bundled(closet, "DESERT").unwrap();
        assert_eq!(t.env_value(closet, "humidity", 0).unwrap().0, 12.0);
        assert!(t.env_value(closet, "ambient_temperature", 15 * 3_600).unwrap().0 > 40.0, "a hot afternoon");
        assert!(t.env_apply_bundled(closet, "atlantis").unwrap_err().to_string().contains("desert"));
    }
}
