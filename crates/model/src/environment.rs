//! The lab's environment: one small room that the smart things share. Its temperature,
//! humidity, soil moisture, smoke, carbon monoxide and light move over simulated time as the
//! smart devices work (a heater warms, an air conditioner cools, a window lets the outside in,
//! a sprinkler wets), and the smart sensors read them.
//!
//! The model is deliberately simple and exact: every quantity is a whole number of thousandths
//! (milli-units), one step is one second of simulated time, and each step adds integers and
//! clamps, so a result is the same on every platform and a long run cannot drift. The pieces
//! that stand outside the room (the outdoor temperature, humidity and daylight) are settings.
//! The limits are in `docs/decisions.md`: one room per lab, no heat capacity per wall, no
//! day/night cycle.

use serde::{Deserialize, Serialize};

/// Temperatures the room can reach, in degrees.
pub const MIN_TEMP: i32 = -50;
pub const MAX_TEMP: i32 = 100;
/// A heater stops adding heat above this (degrees); an air conditioner stops cooling below that.
pub const HEATER_CEILING: i32 = 40;
pub const AC_FLOOR: i32 = 10;
/// Carbon monoxide, in ppm.
pub const MAX_CO: i32 = 1_000;
/// Carbon dioxide, in ppm; the air outside holds [`OUTSIDE_CO2`] and the room leaks toward it.
pub const MAX_CO2: i32 = 10_000;
pub const OUTSIDE_CO2: i32 = 420;
/// Atmospheric pressure the room starts at (kPa, sea level) and the highest it reads.
pub const SEA_LEVEL_KPA: i32 = 101;
pub const MAX_KPA: i32 = 200;
/// The fastest wind the room reads (kph).
pub const MAX_WIND: i32 = 500;
/// What a smart device that makes sound adds to the room: a music player alone, a speaker
/// (or a player paired with one), and the one pitch they all play (A above middle C, Hz).
pub const PLAYER_DB: u32 = 55;
pub const SPEAKER_DB: u32 = 75;
pub const TONE_HZ: u32 = 440;
/// The most smart devices of one kind the physics counts (more add no more).
pub const MAX_EFFECTS: u32 = 16;
/// The longest stretch one call steps (seconds of simulated time); a longer one is cut.
pub const MAX_STEP_SECONDS: u64 = 86_400;

/// The environment settings and its starting values, saved with the lab. Whole units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    /// The physics is on: sensors read the room instead of their own number.
    pub enabled: bool,
    /// The room's values when it starts (degrees, percent, ppm); motion is 0 or 1.
    pub temperature: i32,
    pub humidity: i32,
    pub soil: i32,
    pub smoke: i32,
    pub co: i32,
    pub motion: i32,
    /// Flames (percent) and carbon dioxide (ppm) when the room starts.
    pub fire: i32,
    pub co2: i32,
    /// What lies outside: degrees, percent humidity, percent daylight.
    pub outside_temperature: i32,
    pub outside_humidity: i32,
    pub outside_light: i32,
}

impl Default for Environment {
    fn default() -> Self {
        Environment {
            enabled: false,
            temperature: 20,
            humidity: 50,
            soil: 40,
            smoke: 0,
            co: 0,
            motion: 0,
            fire: 0,
            co2: OUTSIDE_CO2,
            outside_temperature: 15,
            outside_humidity: 60,
            outside_light: 60,
        }
    }
}

impl Environment {
    pub fn is_default(&self) -> bool {
        *self == Environment::default()
    }

    /// Clamps every value into its range (a hostile file cannot hold more).
    pub fn sanitize(&mut self) {
        self.temperature = self.temperature.clamp(MIN_TEMP, MAX_TEMP);
        self.outside_temperature = self.outside_temperature.clamp(MIN_TEMP, MAX_TEMP);
        for v in [&mut self.humidity, &mut self.soil, &mut self.smoke, &mut self.outside_humidity, &mut self.outside_light] {
            *v = (*v).clamp(0, 100);
        }
        self.co = self.co.clamp(0, MAX_CO);
        self.motion = self.motion.clamp(0, 1);
        self.fire = self.fire.clamp(0, 100);
        self.co2 = self.co2.clamp(0, MAX_CO2);
    }
}

/// What the running smart devices do to the room (counts of devices that are on).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    pub heaters: u32,
    pub coolers: u32,
    pub fans: u32,
    pub sprinklers: u32,
    pub windows_open: u32,
    pub lamps: u32,
    /// Ceiling fans stir the air: they pull the temperature toward the outside's.
    pub breezes: u32,
    pub furnaces: u32,
    pub humidifiers: u32,
    /// Fire sprinklers that are spraying (switched on, or seeing flames).
    pub fire_sprinklers: u32,
    /// Cars whose engine runs: they add carbon monoxide and carbon dioxide.
    pub cars: u32,
    /// The loudest thing playing (dB) and its pitch (Hz); 0: silence.
    pub sound_db: u32,
    pub sound_hz: u32,
    /// The light of the brightest lamp, in percent of the room's light (a smart LED's brightness
    /// scales its 70); `None`: the lamps shine at their usual 70.
    pub lamp_percent: Option<u32>,
    /// The wind outside (kph); it speeds up what an open window lets in or out. 0: still air.
    pub wind_kph: u32,
}

impl Effects {
    fn capped(n: u32) -> i64 {
        i64::from(n.min(MAX_EFFECTS))
    }
}

/// The room's state at run time, in thousandths. Light and motion are whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Climate {
    pub temperature_m: i64,
    pub humidity_m: i64,
    pub soil_m: i64,
    pub smoke_m: i64,
    pub co_m: i64,
    pub light: i64,
    pub motion: i64,
    pub co2_m: i64,
    pub fire_m: i64,
    /// Whole values: what is playing now, the pressure and the wind (not simulated further).
    pub sound_db: i64,
    pub sound_hz: i64,
    pub pressure: i64,
    pub wind: i64,
}

/// Moves `from` toward `to` by a share of the gap (`1/div`), at least a thousandth so it always
/// arrives, never past it.
fn leak(from: i64, to: i64, div: i64) -> i64 {
    let gap = to.saturating_sub(from);
    let step = gap.abs().saturating_add(div.saturating_sub(1)) / div.max(1);
    gap.signum().saturating_mul(step.min(gap.abs()))
}

fn milli(v: i32) -> i64 {
    i64::from(v).saturating_mul(1_000)
}

/// Thousandths to the nearest whole unit.
fn whole(m: i64) -> i32 {
    i32::try_from((m.saturating_add(500)).div_euclid(1_000)).unwrap_or(0)
}

impl Climate {
    /// The room as the settings say it starts.
    pub fn from_settings(e: &Environment) -> Climate {
        let mut c = Climate {
            temperature_m: milli(e.temperature),
            humidity_m: milli(e.humidity),
            soil_m: milli(e.soil),
            smoke_m: milli(e.smoke),
            co_m: milli(e.co),
            light: 0,
            motion: i64::from(e.motion),
            co2_m: milli(e.co2),
            fire_m: milli(e.fire),
            sound_db: 0,
            sound_hz: 0,
            pressure: i64::from(SEA_LEVEL_KPA),
            wind: 0,
        };
        c.light = c.light_level(e, &Effects::default());
        c
    }

    fn light_level(&self, e: &Environment, fx: &Effects) -> i64 {
        let day = i64::from(e.outside_light);
        let through = if fx.windows_open > 0 { day * 3 / 4 } else { 0 };
        let lamp = if fx.lamps > 0 { i64::from(fx.lamp_percent.unwrap_or(70).min(100)) } else { 0 };
        (day / 4 + through + lamp).clamp(0, 100)
    }

    /// One second of simulated time.
    pub fn step_second(&mut self, e: &Environment, fx: &Effects) {
        let open = fx.windows_open > 0;
        // Wind through an open window shortens the time the room takes to match the outside.
        let draught = |base: i64| base.saturating_mul(20).checked_div(20 + i64::from(fx.wind_kph.min(500))).unwrap_or(base).max(10);
        let (heat, cool, fans, sprinklers) =
            (Effects::capped(fx.heaters), Effects::capped(fx.coolers), Effects::capped(fx.fans), Effects::capped(fx.sprinklers));
        let (breezes, furnaces, humidifiers, hoses, cars) = (
            Effects::capped(fx.breezes),
            Effects::capped(fx.furnaces),
            Effects::capped(fx.humidifiers),
            Effects::capped(fx.fire_sprinklers),
            Effects::capped(fx.cars),
        );
        // Temperature: leaks toward the outside, a heater adds 1.2 degrees a minute (up to a
        // ceiling), an air conditioner removes 1.5 a minute (down to a floor).
        let mut t =
            self.temperature_m.saturating_add(leak(self.temperature_m, milli(e.outside_temperature), if open { draught(300) } else { 1_800 }));
        if (heat > 0 || furnaces > 0) && t < milli(HEATER_CEILING) {
            t = t.saturating_add(heat.saturating_mul(20)).saturating_add(furnaces.saturating_mul(30));
        }
        // A ceiling fan only mixes the air: it brings the room toward the outside, never past it.
        if breezes > 0 && t > milli(e.outside_temperature) {
            t = t.saturating_sub(breezes.saturating_mul(8).min(t.saturating_sub(milli(e.outside_temperature))));
        }
        // Flames warm the room a little.
        t = t.saturating_add(self.fire_m / 5_000);
        if cool > 0 && t > milli(AC_FLOOR) {
            t = t.saturating_sub(cool.saturating_mul(25));
        }
        self.temperature_m = t.clamp(milli(MIN_TEMP), milli(MAX_TEMP));
        // Humidity: leaks toward the outside; a sprinkler wets, a cooler and a fan dry.
        let h = self
            .humidity_m
            .saturating_add(leak(self.humidity_m, milli(e.outside_humidity), if open { draught(300) } else { 1_200 }))
            .saturating_add(sprinklers.saturating_mul(10))
            .saturating_add(humidifiers.saturating_mul(15))
            .saturating_sub(cool.saturating_mul(5))
            .saturating_sub(fans.saturating_mul(3));
        self.humidity_m = h.clamp(0, 100_000);
        // Soil: dries a little every second, a sprinkler soaks it.
        self.soil_m = self.soil_m.saturating_sub(1).saturating_add(sprinklers.saturating_mul(40)).clamp(0, 100_000);
        // Smoke and carbon monoxide: only fresh air clears them (a fan, an open window).
        let clear = fans.saturating_mul(50).saturating_add(if open { 30 } else { 0 }).saturating_add(2);
        // Flames burn on until a fire sprinkler puts them out, and make smoke and carbon monoxide.
        self.fire_m = self.fire_m.saturating_sub(hoses.saturating_mul(3_000)).clamp(0, 100_000);
        self.smoke_m = self.smoke_m.saturating_add(self.fire_m / 200).saturating_sub(clear).clamp(0, 100_000);
        self.co_m = self
            .co_m
            .saturating_add(self.fire_m / 100)
            .saturating_add(cars.saturating_mul(500))
            .saturating_sub(clear.saturating_mul(10))
            .clamp(0, milli(MAX_CO));
        // Carbon dioxide: a running car adds 5 ppm a second; the room leaks toward the outside air.
        let co2_leak = leak(self.co2_m, milli(OUTSIDE_CO2), if open { draught(150) } else { 600 });
        self.co2_m = self.co2_m.saturating_add(co2_leak).saturating_add(cars.saturating_mul(5_000)).clamp(0, milli(MAX_CO2));
        self.sound_db = i64::from(fx.sound_db.min(200));
        self.sound_hz = if fx.sound_db > 0 { i64::from(fx.sound_hz) } else { 0 };
        self.light = self.light_level(e, fx);
    }

    /// What a smart sensor of this kind (`temperature`, `thermostat`, `humidity`, `soil`,
    /// `smoke`, `co`, `motion`) reads; `None`: the room has no such quantity.
    pub fn reading(&self, what: &str) -> Option<i32> {
        Some(match what {
            "temperature" | "thermostat" | "humiture" => whole(self.temperature_m),
            "humidity" => whole(self.humidity_m),
            "soil" => whole(self.soil_m),
            "smoke" => whole(self.smoke_m),
            "co" => whole(self.co_m),
            "motion" => i32::try_from(self.motion).unwrap_or(0),
            "co2" => whole(self.co2_m),
            "fire" => whole(self.fire_m),
            "volume" => i32::try_from(self.sound_db).unwrap_or(0),
            "pitch" => i32::try_from(self.sound_hz).unwrap_or(0),
            "pressure" => i32::try_from(self.pressure).unwrap_or(0),
            "wind" => i32::try_from(self.wind).unwrap_or(0),
            _ => return None,
        })
    }

    /// Sets one quantity to a whole value (clamped); `false`: unknown name.
    pub fn set(&mut self, what: &str, v: i32) -> bool {
        match what {
            "temperature" => self.temperature_m = milli(v.clamp(MIN_TEMP, MAX_TEMP)),
            "humidity" => self.humidity_m = milli(v.clamp(0, 100)),
            "soil" => self.soil_m = milli(v.clamp(0, 100)),
            "smoke" => self.smoke_m = milli(v.clamp(0, 100)),
            "co" => self.co_m = milli(v.clamp(0, MAX_CO)),
            "motion" => self.motion = i64::from(v.clamp(0, 1)),
            "co2" => self.co2_m = milli(v.clamp(0, MAX_CO2)),
            "fire" => self.fire_m = milli(v.clamp(0, 100)),
            "pressure" => self.pressure = i64::from(v.clamp(0, MAX_KPA)),
            "wind" => self.wind = i64::from(v.clamp(0, MAX_WIND)),
            _ => return false,
        }
        true
    }
}

/// The names `Climate::set`, `Climate::reading` and the `iot.environment` command use.
pub const QUANTITIES: [&str; 12] = ["temperature", "humidity", "soil", "smoke", "co", "motion", "co2", "fire", "pressure", "wind", "volume", "pitch"];

#[cfg(test)]
mod tests {
    use super::*;

    fn run(c: &mut Climate, e: &Environment, fx: &Effects, seconds: u32) {
        for _ in 0..seconds {
            c.step_second(e, fx);
        }
    }

    #[test]
    fn a_closed_room_drifts_to_the_outside_and_arrives() {
        let e = Environment { enabled: true, temperature: 30, outside_temperature: 10, ..Environment::default() };
        let mut c = Climate::from_settings(&e);
        run(&mut c, &e, &Effects::default(), 20_000);
        assert_eq!(c.reading("temperature"), Some(10));
        assert_eq!(c.temperature_m, 10_000, "it arrives exactly, no stall and no overshoot");
    }

    #[test]
    fn a_dimmed_lamp_lights_the_room_less() {
        let e = Environment { enabled: true, outside_light: 0, ..Environment::default() };
        let light = |fx: Effects| {
            let mut c = Climate::from_settings(&e);
            c.step_second(&e, &fx);
            c.light
        };
        assert_eq!(light(Effects { lamps: 1, ..Effects::default() }), 70, "a lamp as before");
        assert_eq!(light(Effects { lamps: 1, lamp_percent: Some(70), ..Effects::default() }), 70);
        assert_eq!(light(Effects { lamps: 1, lamp_percent: Some(21), ..Effects::default() }), 21, "a smart LED at 30 %");
        assert_eq!(light(Effects { lamps: 1, lamp_percent: Some(0), ..Effects::default() }), 0, "switched on at 0 %");
        assert_eq!(light(Effects { lamp_percent: Some(50), ..Effects::default() }), 0, "no lamp, no light");
    }

    #[test]
    fn wind_speeds_up_an_open_window_and_still_air_changes_nothing() {
        let e = Environment { enabled: true, outside_temperature: 0, ..Environment::default() };
        let open = Effects { windows_open: 1, ..Effects::default() };
        let windy = Effects { windows_open: 1, wind_kph: 100, ..Effects::default() };
        let (mut calm, mut gale) = (Climate::from_settings(&e), Climate::from_settings(&e));
        run(&mut calm, &e, &open, 300);
        run(&mut gale, &e, &windy, 300);
        assert!(gale.temperature_m < calm.temperature_m, "the wind brings the outside in sooner");
        // Wind with the window shut does nothing.
        let (mut a, mut b) = (Climate::from_settings(&e), Climate::from_settings(&e));
        run(&mut a, &e, &Effects::default(), 300);
        run(&mut b, &e, &Effects { wind_kph: 400, ..Effects::default() }, 300);
        assert_eq!(a, b);
    }

    #[test]
    fn heater_and_cooler_move_the_temperature_within_bounds() {
        let e = Environment { enabled: true, ..Environment::default() };
        let mut c = Climate::from_settings(&e);
        run(&mut c, &e, &Effects { heaters: 1, ..Effects::default() }, 600);
        let warm = c.reading("thermostat").unwrap_or(0);
        assert!(warm > 20, "ten minutes of heating warms the room: {warm}");
        run(&mut c, &e, &Effects { heaters: 16, ..Effects::default() }, 86_400);
        assert!(c.reading("temperature").unwrap_or(0) <= HEATER_CEILING + 1, "the heater has a ceiling");
        run(&mut c, &e, &Effects { coolers: 1, ..Effects::default() }, 86_400);
        assert!(c.reading("temperature").unwrap_or(0) >= AC_FLOOR - 1, "the cooler has a floor");
    }

    #[test]
    fn open_window_couples_the_room_to_the_outside_and_lets_light_in() {
        let e = Environment { enabled: true, outside_temperature: 0, outside_light: 80, ..Environment::default() };
        let (mut shut, mut open) = (Climate::from_settings(&e), Climate::from_settings(&e));
        run(&mut shut, &e, &Effects::default(), 600);
        run(&mut open, &e, &Effects { windows_open: 1, ..Effects::default() }, 600);
        assert!(open.temperature_m < shut.temperature_m);
        assert!(open.light > shut.light);
        let lit = {
            let mut c = Climate::from_settings(&e);
            c.step_second(&e, &Effects { lamps: 1, ..Effects::default() });
            c.light
        };
        assert_eq!(lit, 80 / 4 + 70);
    }

    #[test]
    fn sprinkler_wets_fan_clears_smoke_and_values_stay_in_range() {
        let e = Environment { enabled: true, smoke: 50, co: 400, ..Environment::default() };
        let mut c = Climate::from_settings(&e);
        run(&mut c, &e, &Effects { sprinklers: 1, fans: 1, ..Effects::default() }, 3_600);
        assert_eq!(c.reading("soil"), Some(100));
        assert_eq!(c.reading("smoke"), Some(0));
        assert_eq!(c.reading("co"), Some(0));
        assert!((0..=100).contains(&c.reading("humidity").unwrap_or(-1)));
        assert_eq!(c.reading("siren"), None);
        assert!(c.set("co", 5_000) && c.reading("co") == Some(MAX_CO) && !c.set("nope", 1));
    }

    #[test]
    fn hostile_settings_are_clamped() {
        let mut e = Environment { temperature: 9_999, humidity: -5, co: 1 << 30, motion: 7, outside_light: 1_000, ..Environment::default() };
        e.sanitize();
        assert_eq!((e.temperature, e.humidity, e.co, e.motion, e.outside_light), (MAX_TEMP, 0, MAX_CO, 1, 100));
        assert!(Environment::default().is_default());
    }

    #[test]
    fn furnace_heats_faster_than_a_heater_and_a_ceiling_fan_only_goes_as_far_as_the_outside() {
        let e = Environment { enabled: true, temperature: 15, outside_temperature: 15, ..Environment::default() };
        let (mut heater, mut furnace) = (Climate::from_settings(&e), Climate::from_settings(&e));
        run(&mut heater, &e, &Effects { heaters: 1, ..Effects::default() }, 300);
        run(&mut furnace, &e, &Effects { furnaces: 1, ..Effects::default() }, 300);
        assert!(furnace.temperature_m > heater.temperature_m, "{} against {}", furnace.temperature_m, heater.temperature_m);
        let hot = Environment { enabled: true, temperature: 30, outside_temperature: 20, ..Environment::default() };
        let mut c = Climate::from_settings(&hot);
        run(&mut c, &hot, &Effects { breezes: 16, ..Effects::default() }, 86_400);
        assert_eq!(c.reading("temperature"), Some(20), "a fan brings the room to the outside's temperature and stops there");
        let (mut shut, mut stirred) = (Climate::from_settings(&hot), Climate::from_settings(&hot));
        run(&mut shut, &hot, &Effects::default(), 600);
        run(&mut stirred, &hot, &Effects { breezes: 1, ..Effects::default() }, 600);
        assert!(stirred.temperature_m < shut.temperature_m);
    }

    #[test]
    fn a_humidifier_raises_the_humidity_and_it_stays_in_range() {
        let e = Environment { enabled: true, humidity: 40, outside_humidity: 40, ..Environment::default() };
        let (mut plain, mut misty) = (Climate::from_settings(&e), Climate::from_settings(&e));
        run(&mut plain, &e, &Effects::default(), 600);
        run(&mut misty, &e, &Effects { humidifiers: 1, ..Effects::default() }, 600);
        assert!(misty.humidity_m > plain.humidity_m + 5_000, "{} against {}", misty.humidity_m, plain.humidity_m);
        run(&mut misty, &e, &Effects { humidifiers: 16, ..Effects::default() }, 86_400);
        assert_eq!(misty.reading("humidity"), Some(100));
    }

    #[test]
    fn a_running_car_makes_carbon_monoxide_and_dioxide_that_fresh_air_clears() {
        let e = Environment { enabled: true, ..Environment::default() };
        let mut c = Climate::from_settings(&e);
        assert_eq!(c.reading("co2"), Some(OUTSIDE_CO2));
        run(&mut c, &e, &Effects { cars: 1, ..Effects::default() }, 300);
        assert!(c.reading("co").unwrap_or(0) > 100 && c.reading("co2").unwrap_or(0) > 1_000, "{:?} {:?}", c.reading("co"), c.reading("co2"));
        run(&mut c, &e, &Effects { cars: 16, ..Effects::default() }, 86_400);
        assert!(c.reading("co").unwrap_or(0) <= MAX_CO && c.reading("co2").unwrap_or(0) <= MAX_CO2);
        run(&mut c, &e, &Effects { fans: 1, windows_open: 1, ..Effects::default() }, 86_400);
        assert_eq!(c.reading("co"), Some(0));
        assert_eq!(c.reading("co2"), Some(OUTSIDE_CO2), "the room leaks back to the outside air");
    }

    #[test]
    fn flames_burn_until_a_fire_sprinkler_puts_them_out_and_they_make_smoke() {
        let e = Environment { enabled: true, fire: 40, ..Environment::default() };
        let mut c = Climate::from_settings(&e);
        run(&mut c, &e, &Effects::default(), 60);
        assert_eq!(c.reading("fire"), Some(40), "nothing puts the flames out");
        assert!(c.reading("smoke").unwrap_or(0) > 0 && c.reading("co").unwrap_or(0) > 0);
        run(&mut c, &e, &Effects { fire_sprinklers: 1, ..Effects::default() }, 30);
        assert_eq!(c.reading("fire"), Some(0));
        assert!(c.set("fire", 500) && c.reading("fire") == Some(100) && c.set("co2", 99_999) && c.reading("co2") == Some(MAX_CO2));
    }

    #[test]
    fn sound_is_what_is_playing_now() {
        let e = Environment { enabled: true, ..Environment::default() };
        let mut c = Climate::from_settings(&e);
        c.step_second(&e, &Effects { sound_db: SPEAKER_DB, sound_hz: TONE_HZ, ..Effects::default() });
        assert_eq!((c.reading("volume"), c.reading("pitch")), (Some(75), Some(440)));
        c.step_second(&e, &Effects::default());
        assert_eq!((c.reading("volume"), c.reading("pitch")), (Some(0), Some(0)), "silence has no pitch");
        assert_eq!(c.reading("pressure"), Some(SEA_LEVEL_KPA));
        assert!(c.set("wind", 9_999) && c.reading("wind") == Some(MAX_WIND) && !c.set("volume", 3));
    }
}
