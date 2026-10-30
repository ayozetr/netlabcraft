//! IoT: smart things register with an IoT service over HTTP (a server's, or a home gateway's
//! built-in one); the service lists them with their state and applies rules (conditions on the
//! sensors' readings, joined with AND or OR, switch a device on or off). Things report on a
//! timer in simulated time. The exchange is in `netlabcraft-sim`'s `iot` module.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, MAX_NAME};

/// Most rules a server keeps, and conditions in one rule.
pub const MAX_RULES: usize = 32;
pub const MAX_CONDITIONS: usize = 8;
/// The readings a sensor may report.
pub const MIN_VALUE: i32 = -1_000;
pub const MAX_VALUE: i32 = 1_000;
/// The longest report interval (seconds); a new thing reports every [`DEFAULT_INTERVAL`].
pub const MAX_INTERVAL: u32 = 3_600;
pub const DEFAULT_INTERVAL: u32 = 5;

/// How a condition compares a reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Op {
    #[default]
    #[serde(rename = ">")]
    Above,
    #[serde(rename = "<")]
    Below,
    #[serde(rename = "=")]
    Equals,
}

impl Op {
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            ">" | "above" | "gt" => Op::Above,
            "<" | "below" | "lt" => Op::Below,
            "=" | "==" | "equals" | "is" | "eq" => Op::Equals,
            _ => return None,
        })
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Op::Above => ">",
            Op::Below => "<",
            Op::Equals => "=",
        }
    }

    pub fn holds(self, reading: i32, value: i32) -> bool {
        match self {
            Op::Above => reading > value,
            Op::Below => reading < value,
            Op::Equals => reading == value,
        }
    }
}

/// `SENSOR > VALUE` (or `<`, `=`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub sensor: String,
    #[serde(default)]
    pub op: Op,
    pub value: i32,
}

/// `if CONDITION [and|or CONDITION…] then ACTUATOR on|off (else the opposite)`. Older files
/// hold one condition as `sensor` and `above`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IotRule {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sensor: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub above: i32,
    pub actuator: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    /// The conditions are joined with OR (else AND).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub any: bool,
    /// What a rule that holds does to its device: switch it on (else off).
    #[serde(default = "yes", skip_serializing_if = "Clone::clone")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

impl IotRule {
    /// The simple rule older labs keep: `SENSOR > ABOVE` switches `ACTUATOR` on.
    pub fn above(sensor: &str, above: i32, actuator: &str) -> IotRule {
        IotRule {
            sensor: String::new(),
            above: 0,
            actuator: actuator.into(),
            conditions: vec![Condition { sensor: sensor.into(), op: Op::Above, value: above }],
            any: false,
            on: true,
        }
    }

    /// The rule's conditions (an older rule's single one included).
    pub fn all_conditions(&self) -> Vec<Condition> {
        let mut out = self.conditions.clone();
        if !self.sensor.is_empty() {
            out.insert(0, Condition { sensor: self.sensor.clone(), op: Op::Above, value: self.above });
        }
        out
    }

    pub fn names(&self, sensor: &str) -> bool {
        self.all_conditions().iter().any(|c| c.sensor.eq_ignore_ascii_case(sensor))
    }

    /// Whether the rule holds for these readings (`None`: a sensor it names hasn't reported).
    pub fn holds(&self, reading: impl Fn(&str) -> Option<i32>) -> Option<bool> {
        let results: Vec<Option<bool>> = self.all_conditions().iter().map(|c| reading(&c.sensor).map(|r| c.op.holds(r, c.value))).collect();
        if self.any {
            if results.contains(&Some(true)) {
                return Some(true);
            }
            if results.contains(&None) {
                return None;
            }
            Some(false)
        } else {
            if results.contains(&Some(false)) {
                return Some(false);
            }
            if results.contains(&None) {
                return None;
            }
            Some(true)
        }
    }

    /// The rule in words: `If Smoke1 > 0 or Temp1 > 60, switch Siren1 on`.
    pub fn text(&self) -> String {
        let join = if self.any { " or " } else { " and " };
        let conds: Vec<String> = self.all_conditions().iter().map(|c| format!("{} {} {}", c.sensor, c.op.symbol(), c.value)).collect();
        format!("If {}, switch {} {}", conds.join(join), self.actuator, if self.on { "on" } else { "off" })
    }
}

/// A device's IoT settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Iot {
    /// Smart things: the IoT server they register with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Ipv4Addr>,
    /// Smart sensors: the reading (degrees, percent, 0/1…).
    #[serde(skip_serializing_if = "is_zero")]
    pub value: i32,
    /// Smart things: report every N seconds of simulated time (0: only when changed or asked).
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub interval: u32,
    /// Servers and home gateways: the IoT service is on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub service: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<IotRule>,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

impl Iot {
    pub fn is_default(&self) -> bool {
        *self == Iot::default()
    }

    /// Caps what a hostile file could hold.
    pub fn sanitize(&mut self) {
        self.value = self.value.clamp(MIN_VALUE, MAX_VALUE);
        self.interval = self.interval.min(MAX_INTERVAL);
        self.rules.truncate(MAX_RULES);
        for r in &mut self.rules {
            r.sensor = r.sensor.chars().take(MAX_NAME).collect();
            r.actuator = r.actuator.chars().take(MAX_NAME).collect();
            r.conditions.truncate(MAX_CONDITIONS);
            for c in &mut r.conditions {
                c.sensor = c.sensor.chars().take(MAX_NAME).collect();
            }
            r.conditions.retain(|c| !c.sensor.is_empty());
        }
        self.rules.retain(|r| !r.all_conditions().is_empty() && !r.actuator.is_empty());
    }
}

/// What kind of smart thing a model is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thing {
    /// The word the service lists it by (`temperature`, `motion`, `light`…).
    pub what: &'static str,
    /// A sensor reports a reading; the others are switched on and off.
    pub sensor: bool,
    /// The reading's unit, or the words for on and off.
    pub unit: &'static str,
    pub on_word: &'static str,
    pub off_word: &'static str,
}

/// The smart things, by model id.
pub fn thing_of(model_id: &str) -> Option<Thing> {
    let t = |what, sensor, unit, on_word, off_word| Some(Thing { what, sensor, unit, on_word, off_word });
    match model_id {
        "IoT-Sensor" => t("temperature", true, "degrees", "", ""),
        "IoT-Motion" => t("motion", true, "1 = movement", "", ""),
        "IoT-Smoke" => t("smoke", true, "% smoke", "", ""),
        "IoT-Device" => t("fan", false, "", "running", "stopped"),
        "IoT-Light" => t("light", false, "", "lit", "dark"),
        "IoT-Door" => t("door", false, "", "unlocked", "locked"),
        "IoT-Siren" => t("siren", false, "", "sounding", "quiet"),
        "IoT-Webcam" => t("webcam", false, "", "recording", "off"),
        _ => None,
    }
}

/// The smart thing a device is.
pub fn thing(d: &Device) -> Option<Thing> {
    (d.kind == DeviceKind::Iot).then(|| thing_of(d.catalogue_model().id)).flatten()
}

/// A smart sensor (reports a reading).
pub fn is_sensor(d: &Device) -> bool {
    thing(d).is_some_and(|t| t.sensor)
}

/// A smart device (switched on and off by the server).
pub fn is_actuator(d: &Device) -> bool {
    d.kind == DeviceKind::Iot && !is_sensor(d)
}

/// Whether a device can run the IoT service: servers, and home gateways (built in).
pub fn can_serve(d: &Device) -> bool {
    d.kind == DeviceKind::Server || d.catalogue_model().id == "Home-Gateway"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_settings_are_trimmed() {
        let mut i = Iot { value: 99_999, interval: 99_999, rules: vec![IotRule::above("", 1, "Fan")], ..Iot::default() };
        i.rules.extend((0..40).map(|n| IotRule::above(&format!("S{n}"), n, &"x".repeat(200))));
        i.sanitize();
        assert_eq!((i.value, i.interval), (MAX_VALUE, MAX_INTERVAL));
        assert!(i.rules.len() < MAX_RULES && i.rules.iter().all(|r| r.actuator.len() == MAX_NAME));
    }

    #[test]
    fn rules_join_conditions_with_and_or_and_old_rules_still_read() {
        let old: IotRule = serde_json::from_str(r#"{"sensor": "Temp", "above": 30, "actuator": "Fan"}"#).unwrap();
        assert_eq!(old.text(), "If Temp > 30, switch Fan on");
        assert_eq!(old.holds(|_| Some(31)), Some(true));
        assert_eq!(old.holds(|_| None), None);
        let and = IotRule {
            conditions: vec![
                Condition { sensor: "Motion".into(), op: Op::Equals, value: 1 },
                Condition { sensor: "Door".into(), op: Op::Below, value: 1 },
            ],
            ..IotRule::above("x", 0, "Light")
        };
        let read = |m: i32, door: Option<i32>| move |s: &str| if s == "Motion" { Some(m) } else { door };
        assert_eq!(and.holds(read(1, Some(0))), Some(true));
        assert_eq!(and.holds(read(0, None)), Some(false), "one false condition settles an AND");
        let or = IotRule { any: true, on: false, ..and.clone() };
        assert_eq!(or.holds(read(1, None)), Some(true), "one true condition settles an OR");
        assert_eq!(or.holds(read(0, Some(5))), Some(false));
        assert!(or.text().contains(" or ") && or.text().ends_with("off"));
        let back: IotRule = serde_json::from_str(&serde_json::to_string(&or).unwrap()).unwrap();
        assert_eq!(back, or);
        assert_eq!(Op::parse("above"), Some(Op::Above));
        assert_eq!(Op::parse("?"), None);
        assert!(thing_of("IoT-Webcam").is_some_and(|t| !t.sensor) && thing_of("Router-3").is_none());
    }
}
