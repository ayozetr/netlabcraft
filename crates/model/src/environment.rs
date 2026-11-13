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
        };
        c.light = c.light_level(e, &Effects::default());
        c
    }

    fn light_level(&self, e: &Environment, fx: &Effects) -> i64 {
        let day = i64::from(e.outside_light);
        let through = if fx.windows_open > 0 { day * 3 / 4 } else { 0 };
        (day / 4 + through + if fx.lamps > 0 { 70 } else { 0 }).clamp(0, 100)
    }

    /// One second of simulated time.
    pub fn step_second(&mut self, e: &Environment, fx: &Effects) {
        let open = fx.windows_open > 0;
        let (heat, cool, fans, sprinklers) =
            (Effects::capped(fx.heaters), Effects::capped(fx.coolers), Effects::capped(fx.fans), Effects::capped(fx.sprinklers));
        // Temperature: leaks toward the outside, a heater adds 1.2 degrees a minute (up to a
        // ceiling), an air conditioner removes 1.5 a minute (down to a floor).
        let mut t = self.temperature_m.saturating_add(leak(self.temperature_m, milli(e.outside_temperature), if open { 300 } else { 1_800 }));
        if heat > 0 && t < milli(HEATER_CEILING) {
            t = t.saturating_add(heat.saturating_mul(20));
        }
        if cool > 0 && t > milli(AC_FLOOR) {
            t = t.saturating_sub(cool.saturating_mul(25));
        }
        self.temperature_m = t.clamp(milli(MIN_TEMP), milli(MAX_TEMP));
        // Humidity: leaks toward the outside; a sprinkler wets, a cooler and a fan dry.
        let h = self
            .humidity_m
            .saturating_add(leak(self.humidity_m, milli(e.outside_humidity), if open { 300 } else { 1_200 }))
            .saturating_add(sprinklers.saturating_mul(10))
            .saturating_sub(cool.saturating_mul(5))
            .saturating_sub(fans.saturating_mul(3));
        self.humidity_m = h.clamp(0, 100_000);
        // Soil: dries a little every second, a sprinkler soaks it.
        self.soil_m = self.soil_m.saturating_sub(1).saturating_add(sprinklers.saturating_mul(40)).clamp(0, 100_000);
        // Smoke and carbon monoxide: only fresh air clears them (a fan, an open window).
        let clear = fans.saturating_mul(50).saturating_add(if open { 30 } else { 0 }).saturating_add(2);
        self.smoke_m = self.smoke_m.saturating_sub(clear).clamp(0, 100_000);
        self.co_m = self.co_m.saturating_sub(clear.saturating_mul(10)).clamp(0, milli(MAX_CO));
        self.light = self.light_level(e, fx);
    }

    /// What a smart sensor of this kind (`temperature`, `thermostat`, `humidity`, `soil`,
    /// `smoke`, `co`, `motion`) reads; `None`: the room has no such quantity.
    pub fn reading(&self, what: &str) -> Option<i32> {
        Some(match what {
            "temperature" | "thermostat" => whole(self.temperature_m),
            "humidity" => whole(self.humidity_m),
            "soil" => whole(self.soil_m),
            "smoke" => whole(self.smoke_m),
            "co" => whole(self.co_m),
            "motion" => i32::try_from(self.motion).unwrap_or(0),
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
            _ => return false,
        }
        true
    }
}

/// The names `Climate::set`, `Climate::reading` and the `iot.environment` command use.
pub const QUANTITIES: [&str; 6] = ["temperature", "humidity", "soil", "smoke", "co", "motion"];

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
}
