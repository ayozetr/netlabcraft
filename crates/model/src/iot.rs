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
/// The largest level of an RGB LED: red, green and blue as one number (`0xRRGGBB`).
pub const MAX_RGB: i32 = 0xFF_FFFF;

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
    /// A sensor with a second reading (the humiture sensor's humidity), by hand.
    #[serde(skip_serializing_if = "is_zero")]
    pub second: i32,
    /// The generic environment sensor: the id of the environment variable it reads.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub variable: String,
    /// Servers and home gateways: the IoT service is on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub service: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<IotRule>,
    /// Smart LEDs: how bright it shines, in percent (`None`: full).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// Smart LEDs: the colour it shines, red, green, blue (`None`: warm white).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour: Option<[u8; 3]>,
    /// Signal generators: the frequency it outputs, in hertz.
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub frequency: u32,
}

/// The highest frequency a signal generator outputs (Hz).
pub const MAX_FREQUENCY: u32 = 20_000;

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
        self.second = self.second.clamp(MIN_VALUE, MAX_VALUE);
        if crate::environments::var_by_id(&self.variable).is_none() && !crate::envcustom::valid_custom_id(&self.variable) {
            self.variable.clear();
        }
        self.interval = self.interval.min(MAX_INTERVAL);
        self.frequency = self.frequency.min(MAX_FREQUENCY);
        self.brightness = self.brightness.map(|b| b.min(100));
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
    /// A second reading some sensors report next to the first (`humidity` beside a humiture
    /// monitor's temperature); empty: none. The word names the quantity, like `what`.
    pub also: &'static str,
    pub also_unit: &'static str,
}

/// The smart things, by model id.
pub fn thing_of(model_id: &str) -> Option<Thing> {
    let t = |what, sensor, unit, on_word, off_word| Some(Thing { what, sensor, unit, on_word, off_word, also: "", also_unit: "" });
    // A sensor that reports two readings.
    let t2 = |what, unit, also, also_unit| Some(Thing { what, sensor: true, unit, on_word: "", off_word: "", also, also_unit });
    match model_id {
        "IoT-Sensor" => t("temperature", true, "degrees", "", ""),
        "IoT-Motion" => t("motion", true, "1 = movement", "", ""),
        "IoT-Smoke" => t("smoke", true, "% smoke", "", ""),
        "IoT-Device" => t("fan", false, "", "running", "stopped"),
        "IoT-Light" => t("light", false, "", "lit", "dark"),
        "IoT-Door" => t("door", false, "", "unlocked", "locked"),
        "IoT-Siren" => t("siren", false, "", "sounding", "quiet"),
        "IoT-Webcam" => t("webcam", false, "", "recording", "off"),
        "IoT-Thermostat" => t("thermostat", true, "degrees", "", ""),
        "IoT-Heater" => t("heater", false, "", "heating", "off"),
        "IoT-AC" => t("ac", false, "", "cooling", "off"),
        "IoT-Humidity" => t("humidity", true, "% humidity", "", ""),
        "IoT-Window" => t("window", false, "", "open", "closed"),
        "IoT-Soil" => t("soil", true, "% moisture", "", ""),
        "IoT-Sprinkler" => t("sprinkler", false, "", "watering", "off"),
        "IoT-CO" => t("co", true, "ppm CO", "", ""),
        "IoT-Plug" => t("plug", false, "", "powered", "off"),
        "IoT-Garage" => t("garage", false, "", "open", "closed"),
        "IoT-TV" | "IoT-TVE" => t("tv", false, "", "playing", "standby"),
        "IoT-Speaker" => t("speaker", false, "", "playing", "quiet"),
        "IoT-Water" => t("water level", true, "% level", "", ""),
        "IoT-Streetlight" => t("street light", false, "", "lit", "dark"),
        "IoT-Traffic" => t("traffic light", false, "", "green", "red"),
        "IoT-Parking" => t("parking", true, "1 = occupied", "", ""),
        "IoT-Drain" => t("drain level", true, "% full", "", ""),
        "IoT-Air" => t("air quality", true, "index", "", ""),
        "IoT-Blower" => t("blower", false, "", "running", "stopped"),
        "IoT-Battery" => t("battery", true, "% charge", "", ""),
        "IoT-Meter" => t("power meter", true, "W", "", ""),
        "IoT-Solar" => t("solar panel", true, "W", "", ""),
        "IoT-Wind" => t("wind turbine", true, "rpm", "", ""),
        "IoT-Appliance" => t("appliance", false, "", "running", "off"),
        "IoT-BTSpeaker" => t("bluetooth speaker", false, "", "playing", "quiet"),
        "IoT-CeilingFan" => t("ceiling fan", false, "", "turning", "stopped"),
        "IoT-Furnace" => t("furnace", false, "", "burning", "off"),
        "IoT-Humidifier" => t("humidifier", false, "", "misting", "off"),
        "IoT-Humiture" => t("humiture", true, "degrees", "", ""),
        "IoT-Player" => t("music player", false, "", "playing", "stopped"),
        "IoT-SoundDet" => t2("pitch", "Hz", "volume", "dB"),
        "IoT-Pressure" => t("pressure", true, "kPa", "", ""),
        "IoT-Beacon" => t2("beacon", "id announced", "audience", "things in range"),
        "IoT-CO2" => t("co2", true, "ppm CO2", "", ""),
        "IoT-Fire" => t("fire", true, "% flames", "", ""),
        "IoT-FireSprinkler" => t("fire sprinkler", false, "", "spraying", "ready"),
        "IoT-Car" => t("car", false, "", "engine on", "engine off"),
        "IoT-RFIDCard" => t("rfid card", true, "card id", "", ""),
        "IoT-RFIDReader" => t("rfid reader", true, "card id (0: none)", "", ""),
        "IoT-LED" => t("smart led", false, "", "shining", "dark"),
        "IoT-Trip" => t("trip", true, "1 = something near", "", ""),
        "IoT-WindDet" => t("wind", true, "kph", "", ""),
        "IoT-Signal" => t2("signal", "level", "frequency", "Hz"),
        "IoT-Thing" => t("thing", true, "reading", "", ""),
        "Part-LED" => t("led", false, "", "lit", "dark"),
        "Part-Buzzer" => t("buzzer", false, "", "sounding", "quiet"),
        "Part-Motor" => t("motor", false, "", "running", "stopped"),
        "Part-Relay" => t("relay", false, "", "closed", "open"),
        "Part-LCD" => t("display", false, "", "showing", "blank"),
        "Part-Button" => t("button", true, "1 = pressed", "", ""),
        "Part-Pot" => t("potentiometer", true, "% turned", "", ""),
        "Part-Ultrasonic" => t("distance", true, "cm", "", ""),
        "Part-Light" => t("light level", true, "% brightness", "", ""),
        "Part-Switch" => t("toggle", true, "1 = on", "", ""),
        "Part-AirCooler" => t("air cooler", false, "", "cooling", "off"),
        "Part-Alarm" => t("alarm", false, "", "sounding", "quiet"),
        "Part-CeilingSprinkler" => t("ceiling sprinkler", false, "", "watering", "off"),
        "Part-FloorSprinkler" => t("floor sprinkler", false, "", "watering", "off"),
        "Part-DimLED" => t("dimmable led", false, "", "lit", "dark"),
        "Part-HeatingElement" => t("heating element", false, "", "heating", "off"),
        "Part-RGBLED" => t("rgb led", false, "", "lit", "dark"),
        "Part-Servo" => t("servo", false, "", "holding an angle", "idle"),
        "Part-Speaker" => t("speaker", false, "", "playing", "quiet"),
        "Part-Flex" => t("flex", true, "% bent", "", ""),
        "Part-EnvSensor" => t("environment", true, "reading", "", ""),
        "Part-Generic" => t("analog", true, "reading", "", ""),
        "Part-Humidity" => t("humidity", true, "% humidity", "", ""),
        "Part-Humiture" => t("humiture", true, "degrees", "", ""),
        "Part-Membrane" => t("membrane", true, "% along the strip", "", ""),
        "Part-Metal" => t("metal", true, "1 = metal near", "", ""),
        "Part-Motion" => t("motion", true, "1 = movement", "", ""),
        "Part-LatchSwitch" => t("latching switch", true, "1 = on", "", ""),
        "Part-Rocker" => t("rocker switch", true, "1 = on", "", ""),
        "Part-PushToggle" => t("push toggle", true, "1 = on", "", ""),
        "Part-Smoke" => t("smoke", true, "% smoke", "", ""),
        "Part-Temp" => t("temperature", true, "degrees", "", ""),
        "Part-Trip" => t("trip wire", true, "1 = crossed", "", ""),
        "Part-WaterDetector" => t("water present", true, "1 = water", "", ""),
        "Part-WaterLevel" => t("water depth", true, "cm", "", ""),
        "Part-Wind" => t("wind", true, "kph", "", ""),
        _ => None,
    }
}

/// The levels an actuator takes when a board writes its pin (`None`: it is only on or off). A
/// dimmable LED is 0-100 %, a servo 0-180 degrees, an RGB LED one number, `0xRRGGBB`.
pub fn level_range(what: &str) -> Option<(i32, i32)> {
    match what {
        "dimmable led" => Some((0, 100)),
        "servo" => Some((0, 180)),
        "rgb led" => Some((0, MAX_RGB)),
        _ => None,
    }
}

/// What an actuator shows for its state: 0 when it is off, else its level (the top of its range
/// until one is set); a servo always holds an angle. Actuators with no level show 1 when on.
pub fn shown_level(what: &str, on: bool, level: i32) -> i32 {
    match level_range(what) {
        None => i32::from(on),
        Some((lo, hi)) => {
            let level = level.clamp(lo, hi);
            if what == "servo" {
                level
            } else if !on {
                0
            } else if level == 0 {
                hi
            } else {
                level
            }
        }
    }
}

/// The readings a sensor can hold: flags are 0 or 1, strips and bends 0-100 %, every other
/// sensor `MIN_VALUE` to `MAX_VALUE`.
pub fn sensor_range(what: &str) -> (i32, i32) {
    match what {
        "metal" | "trip wire" | "water present" | "latching switch" | "rocker switch" | "push toggle" => (0, 1),
        "flex" | "membrane" => (0, 100),
        _ => (MIN_VALUE, MAX_VALUE),
    }
}

/// A sensor that flips between 0 and 1 each time it is pressed (`iot.configure` with `press`).
pub fn is_pressed_toggle(what: &str) -> bool {
    matches!(what, "latching switch" | "push toggle")
}

/// The word the smart room's physics uses for what a sensor reads (`thermostat` and `humiture`
/// read the temperature, a trip wire the motion).
pub fn room_word(what: &str) -> &str {
    match what {
        "thermostat" | "humiture" => "temperature",
        "trip wire" => "motion",
        w => w,
    }
}

/// Whether a thing's reading is a yes-or-no flag.
pub fn is_flag(what: &str) -> bool {
    sensor_range(what) == (0, 1)
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
    thing(d).is_some_and(|t| !t.sensor)
}

/// The SSID a Wi-Fi smart thing looks for when placed: the home gateway's factory network.
pub const HOME_SSID: &str = "HomeNetwork";

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
        assert!(thing_of("IoT-Thermostat").is_some_and(|t| t.sensor) && thing_of("IoT-Garage").is_some_and(|t| t.on_word == "open"));
    }

    #[test]
    fn the_second_wave_of_home_things_are_switched_or_read_as_they_say() {
        for id in ["IoT-Appliance", "IoT-BTSpeaker", "IoT-CeilingFan", "IoT-Furnace", "IoT-Humidifier", "IoT-Player"] {
            assert!(thing_of(id).is_some_and(|t| !t.sensor && !t.on_word.is_empty() && t.also.is_empty()), "{id} is switched on and off");
        }
        assert!(
            thing_of("IoT-SoundDet").is_some_and(|t| t.sensor && t.also == "volume" && !t.also_unit.is_empty()),
            "the detector reports two readings"
        );
        // The humiture monitor keeps its humidity in the second reading every humiture sensor has.
        assert!(thing_of("IoT-Humiture").is_some_and(|t| t.sensor && t.what == "humiture" && t.also.is_empty()));
        assert_eq!(room_word("humiture"), "temperature");
        assert_eq!(room_word("pitch"), "pitch");
    }

    #[test]
    fn a_smart_leds_settings_are_trimmed_and_kept_only_when_set() {
        let mut i = Iot { brightness: Some(250), frequency: 1 << 30, colour: Some([1, 2, 3]), ..Iot::default() };
        i.sanitize();
        assert_eq!((i.brightness, i.frequency), (Some(100), MAX_FREQUENCY));
        let json = serde_json::to_string(&Iot::default()).unwrap();
        assert_eq!(json, "{}", "nothing new is written for a thing that does not use it");
        let back: Iot = serde_json::from_str(&serde_json::to_string(&i).unwrap()).unwrap();
        assert_eq!(back, i);
    }

    #[test]
    fn levels_ranges_and_words_of_the_components() {
        assert_eq!(level_range("dimmable led"), Some((0, 100)));
        assert_eq!(level_range("servo"), Some((0, 180)));
        assert_eq!(level_range("rgb led"), Some((0, MAX_RGB)));
        assert_eq!(level_range("led"), None);
        // Off shows 0; on with no level set shows the top of the range; a servo always holds its angle.
        assert_eq!(shown_level("dimmable led", false, 40), 0);
        assert_eq!(shown_level("dimmable led", true, 40), 40);
        assert_eq!(shown_level("dimmable led", true, 0), 100);
        assert_eq!(shown_level("dimmable led", true, 900), 100);
        assert_eq!(shown_level("servo", false, 90), 90);
        assert_eq!(shown_level("rgb led", true, 0), MAX_RGB);
        assert_eq!(shown_level("led", true, 7), 1);
        assert_eq!(shown_level("led", false, 7), 0);
        assert_eq!(sensor_range("trip wire"), (0, 1));
        assert_eq!(sensor_range("flex"), (0, 100));
        assert_eq!(sensor_range("temperature"), (MIN_VALUE, MAX_VALUE));
        assert!(is_flag("metal") && is_flag("rocker switch") && !is_flag("flex"));
        assert!(is_pressed_toggle("push toggle") && is_pressed_toggle("latching switch") && !is_pressed_toggle("rocker switch"));
        assert_eq!(room_word("humiture"), "temperature");
        assert_eq!(room_word("trip wire"), "motion");
        assert_eq!(room_word("thermostat"), "temperature");
        assert_eq!(room_word("smoke"), "smoke");
        // A file naming a variable that does not exist, or a second reading out of range.
        let mut i = Iot { variable: "No such variable!".into(), second: 5_000, ..Iot::default() };
        i.sanitize();
        assert_eq!((i.variable.as_str(), i.second), ("", MAX_VALUE));
        let mut i = Iot { variable: "wind_speed".into(), ..Iot::default() };
        i.sanitize();
        assert_eq!(i.variable, "wind_speed");
        // Every new part is a smart thing of the right sort.
        for id in [
            "Part-AirCooler",
            "Part-Alarm",
            "Part-CeilingSprinkler",
            "Part-FloorSprinkler",
            "Part-DimLED",
            "Part-HeatingElement",
            "Part-RGBLED",
            "Part-Servo",
            "Part-Speaker",
        ] {
            assert!(thing_of(id).is_some_and(|t| !t.sensor), "{id}");
        }
        for id in [
            "Part-Flex",
            "Part-EnvSensor",
            "Part-Generic",
            "Part-Humidity",
            "Part-Humiture",
            "Part-Membrane",
            "Part-Metal",
            "Part-Motion",
            "Part-LatchSwitch",
            "Part-Rocker",
            "Part-PushToggle",
            "Part-Smoke",
            "Part-Temp",
            "Part-Trip",
            "Part-WaterDetector",
            "Part-WaterLevel",
            "Part-Wind",
        ] {
            assert!(thing_of(id).is_some_and(|t| t.sensor), "{id}");
        }
    }
}
