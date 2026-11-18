//! IoT: smart things register with an IoT service (a server's, or a home gateway's built-in
//! one) over HTTP, the service keeps their state and applies its rules. Things report on a
//! timer in simulated time (`Iot::interval`), and at once when their reading changes.
//!
//! Requests are plain HTTP GETs to the service's web port (80), our own small interface:
//!
//! - `GET /iot/register?name=N&kind=sensor&type=T&value=V[&value2=W]` (a sensor reports its
//!   reading, and a second one when it has two) or
//!   `…&kind=device&type=T` (a smart device registers); the answer is `registered`.
//! - `GET /iot/state?name=N`: a smart device asks whether it should be on; the answer is `on`
//!   or `off` (a webcam that is on also gets its still image's text on a second line).
//!
//! When a sensor reports a new reading, every rule naming it is evaluated (its conditions
//! joined with AND or OR): a rule that holds switches its device as it says, one that doesn't
//! the opposite way. A device can also be switched by hand from the service's IoT page.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::environment::{Climate, Effects, MAX_STEP_SECONDS, PLAYER_DB, SPEAKER_DB, TONE_HZ};
use netlabcraft_model::iot::{is_actuator, is_sensor, room_word, sensor_range, thing};
use netlabcraft_model::proximity::announced_id;
use netlabcraft_model::{Device, DeviceKind, Topology};

use crate::{Sim, SimError};

/// Most things one server lists.
const MAX_THINGS: usize = 256;
/// Most timed reports one run makes (a guard against a runaway timer).
const MAX_TIMED_ROUNDS: usize = 10_000;

/// A thing the server knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IotEntry {
    pub name: String,
    /// `sensor` or `device`.
    pub kind: &'static str,
    /// What it is (`temperature`, `motion`, `light`, `webcam`…).
    pub what: String,
    pub address: Ipv4Addr,
    /// The last reading (sensors).
    pub value: Option<i32>,
    /// The second reading of a sensor that has one (a humiture monitor's humidity).
    pub value2: Option<i32>,
    /// Switched on (devices).
    pub on: bool,
    /// Webcams: the text of the last still image.
    pub image: Option<String>,
    /// When it last reported (simulated microseconds).
    pub seen_us: u64,
}

/// What a thing knows at run time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThingState {
    /// The server that took its registration.
    pub registered: Option<Ipv4Addr>,
    /// Smart devices: switched on by the server.
    pub on: bool,
    /// A dimmable LED's brightness, a servo's angle or an RGB LED's colour, as last set (by a
    /// board's `write` or by hand); 0 until then.
    pub level: i32,
    /// Why the last report failed.
    pub message: Option<String>,
    /// Webcams: the still image the server returned.
    pub image: Option<String>,
    /// When it last reported (simulated microseconds), for the timer.
    pub last_report_us: Option<u64>,
}

/// The room at run time and the simulated time it was last stepped to.
#[derive(Debug, Clone, Copy)]
pub struct EnvRun {
    pub climate: Climate,
    pub last_us: u64,
}

#[derive(Debug, Clone, Default)]
pub struct IotState {
    /// The shared room, once the physics has started.
    pub(crate) env: Option<EnvRun>,
    /// The places' own physics, once it has started (see `parts`).
    pub(crate) places: BTreeMap<u32, crate::parts::PlaceRun>,
    servers: BTreeMap<DeviceId, Vec<IotEntry>>,
    pub(crate) things: BTreeMap<DeviceId, ThingState>,
    /// Programmable boards' runs (see the `board` module).
    pub(crate) boards: BTreeMap<DeviceId, crate::board::BoardRun>,
}

impl IotState {
    pub(crate) fn forget(&mut self, d: DeviceId) {
        self.servers.remove(&d);
        self.things.remove(&d);
        self.boards.remove(&d);
    }
}

/// `a=1&b=2` → its value for `key`.
fn query<'a>(q: &'a str, key: &str) -> Option<&'a str> {
    q.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

/// `mm:ss` of simulated time.
fn clock(us: u64) -> String {
    let s = us / 1_000_000;
    format!("{:02}:{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60, s % 60)
}

/// Applies the rules that name `sensor` (or, with `None`, every rule) to the devices listed.
fn apply_rules(d: &Device, list: &mut [IotEntry], sensor: Option<&str>) {
    for r in d.iot.rules.iter().filter(|r| sensor.is_none_or(|s| r.names(s))) {
        let reading = |name: &str| list.iter().find(|e| e.kind == "sensor" && e.name.eq_ignore_ascii_case(name)).and_then(|e| e.value);
        let Some(holds) = r.holds(reading) else { continue };
        if let Some(e) = list.iter_mut().find(|e| e.kind == "device" && e.name.eq_ignore_ascii_case(&r.actuator)) {
            e.on = if holds { r.on } else { !r.on };
        }
    }
}

impl Sim {
    /// Whether a smart device is doing its work now: switched on by the server, or (a fire
    /// sprinkler) seeing flames in the room, which it puts out whether or not the server asked.
    pub fn iot_active(&self, t: &Topology, d: &Device) -> bool {
        if !is_actuator(d) || t.is_dark(d.id) {
            return false;
        }
        let burning = self.iot.env.as_ref().is_some_and(|r| r.climate.fire_m > 0) || self.place_burning(t, d);
        self.iot.things.get(&d.id).is_some_and(|s| s.on) || (burning && thing(d).is_some_and(|x| x.what == "fire sprinkler"))
    }

    /// What the running smart devices do to the room right now.
    fn env_effects(&self, t: &Topology) -> Effects {
        let mut fx = Effects::default();
        for d in t.devices.iter().filter(|d| is_actuator(d) && self.iot_active(t, d)) {
            let what = thing(d).map(|x| x.what);
            let n = match what {
                Some("heater") | Some("heating element") => &mut fx.heaters,
                Some("furnace") => &mut fx.furnaces,
                Some("ac") | Some("air cooler") => &mut fx.coolers,
                Some("fan") => &mut fx.fans,
                Some("ceiling fan") => &mut fx.breezes,
                Some("humidifier") => &mut fx.humidifiers,
                Some("sprinkler") | Some("ceiling sprinkler") | Some("floor sprinkler") => &mut fx.sprinklers,
                Some("fire sprinkler") => &mut fx.fire_sprinklers,
                Some("car") => &mut fx.cars,
                Some("window") | Some("garage") => &mut fx.windows_open,
                Some("light") | Some("smart led") | Some("dimmable led") | Some("rgb led") => &mut fx.lamps,
                Some("bluetooth speaker") => {
                    fx.sound_db = fx.sound_db.max(SPEAKER_DB);
                    continue;
                }
                // A player alone is quiet; paired with a speaker in range it plays loud.
                Some("music player") => {
                    fx.sound_db = fx.sound_db.max(if t.paired_speaker(d).is_some() { SPEAKER_DB } else { PLAYER_DB });
                    continue;
                }
                _ => continue,
            };
            *n = n.saturating_add(1);
            // A ceiling or floor sprinkler puts out flames as a fire sprinkler does.
            if matches!(what, Some("ceiling sprinkler" | "floor sprinkler")) {
                fx.fire_sprinklers = fx.fire_sprinklers.saturating_add(1);
            }
        }
        if fx.sound_db > 0 {
            fx.sound_hz = TONE_HZ;
        }
        fx
    }

    /// Steps the room up to the present, a second at a time, with the devices as they are.
    /// Does nothing while the physics is off.
    pub fn env_step(&mut self, t: &Topology) {
        self.place_step(t);
        if !t.environment.enabled {
            self.iot.env = None;
            return;
        }
        let now = self.now;
        let mut fx = self.env_effects(t);
        let run = self.iot.env.get_or_insert_with(|| EnvRun { climate: Climate::from_settings(&t.environment), last_us: now });
        // With the places' own physics on, the wind outside also blows through an open window.
        if t.environments.physics {
            fx.wind_kph = u32::try_from(run.climate.wind).unwrap_or(0);
        }
        let seconds = (now.saturating_sub(run.last_us) / 1_000_000).min(MAX_STEP_SECONDS);
        for _ in 0..seconds {
            run.climate.step_second(&t.environment, &fx);
        }
        // Keep the sub-second remainder for the next step.
        run.last_us = if now.saturating_sub(run.last_us) / 1_000_000 > MAX_STEP_SECONDS {
            now
        } else {
            run.last_us.saturating_add(seconds.saturating_mul(1_000_000))
        };
    }

    /// The room as it is (`None`: the physics is off or hasn't started).
    pub fn environment(&self) -> Option<&Climate> {
        self.iot.env.as_ref().map(|r| &r.climate)
    }

    /// Forgets the room's state: it starts again from the settings.
    pub fn env_reset(&mut self) {
        self.iot.env = None;
        self.iot.places.clear();
    }

    /// Sets one quantity of the running room (starting it first), by name.
    pub fn env_set(&mut self, t: &Topology, what: &str, value: i32) -> bool {
        let now = self.now;
        let run = self.iot.env.get_or_insert_with(|| EnvRun { climate: Climate::from_settings(&t.environment), last_us: now });
        run.climate.set(what, value)
    }

    /// What a smart sensor reads: the room's quantity while the physics is on, else its own
    /// number.
    pub fn iot_reading(&self, t: &Topology, d: &Device) -> i32 {
        let Some(x) = thing(d) else { return d.iot.value };
        // Things that read who is near them answer from where things are, not from a variable.
        match x.what {
            "rfid reader" => return t.rfid_reading(d),
            "rfid card" | "beacon" => return announced_id(d),
            "trip" => return t.trip_reading(d),
            _ => {}
        }
        // The generic environment sensor and the water detector read what only they read.
        if let Some(v) = self.part_reading(t, d, x.what) {
            return v;
        }
        // A place (or one above it) that sets the variable, or a part that changes it, decides
        // what the sensor reads.
        let word = room_word(x.what);
        if let Some(v) = self.mapped_reading(t, d, word) {
            return v;
        }
        if t.environment.enabled
            && let Some(c) = self.environment()
            && let Some(v) = c.reading(word)
        {
            return v;
        }
        let (lo, hi) = sensor_range(x.what);
        d.iot.value.clamp(lo, hi)
    }

    /// A sensor's second reading, when it has one (a humiture sensor's humidity, a sound
    /// detector's volume, how many things hear a beacon, a signal generator's frequency).
    pub fn iot_reading2(&self, t: &Topology, d: &Device) -> Option<i32> {
        let x = thing(d)?;
        if x.what == "humiture" {
            return Some(self.iot_second(t, d));
        }
        match x.also {
            "" => None,
            "audience" => Some(t.beacon_audience(d)),
            "frequency" => Some(i32::try_from(d.iot.frequency).unwrap_or(0)),
            w => {
                if let Some(v) = self.mapped_reading(t, d, w) {
                    return Some(v);
                }
                if t.environment.enabled
                    && let Some(v) = self.environment().and_then(|c| c.reading(w))
                {
                    return Some(v);
                }
                // Nothing sets it: the variable's ordinary value.
                netlabcraft_model::environments::sensor_var(w).map(|v| v.default.round() as i32)
            }
        }
    }

    /// The time of day of the environment clock now, in seconds from midnight.
    pub fn env_time(&self, t: &Topology) -> u32 {
        t.environments.clock.time_at(self.now)
    }

    /// The things a server's IoT service lists.
    pub fn iot_registry(&self, server: DeviceId) -> &[IotEntry] {
        self.iot.servers.get(&server).map_or(&[], Vec::as_slice)
    }

    /// A smart thing's run-time state.
    pub fn iot_thing(&self, d: DeviceId) -> Option<&ThingState> {
        self.iot.things.get(&d)
    }

    /// The server side: `path` is what follows `/iot/`. `None`: not an IoT request here.
    pub(crate) fn iot_http(&mut self, d: &Device, src: Ipv4Addr, path: &str) -> Option<(&'static str, String)> {
        if !d.iot.service {
            return None;
        }
        let now = self.now;
        let (what, q) = path.split_once('?').unwrap_or((path, ""));
        let name: String = query(q, "name").unwrap_or("").chars().take(netlabcraft_model::MAX_NAME).collect();
        if name.is_empty() {
            return Some(("400 Bad Request", "no name".into()));
        }
        let list = self.iot.servers.entry(d.id).or_default();
        match what {
            "register" => {
                let kind = if query(q, "kind") == Some("sensor") { "sensor" } else { "device" };
                let thing_type: String =
                    query(q, "type").unwrap_or(if kind == "sensor" { "sensor" } else { "device" }).replace('+', " ").chars().take(24).collect();
                let value = query(q, "value").and_then(|v| v.parse::<i32>().ok());
                let value2 = query(q, "value2").and_then(|v| v.parse::<i32>().ok());
                let (at, new) = match list.iter().position(|e| e.name == name) {
                    Some(i) => (i, false),
                    None if list.len() >= MAX_THINGS => return Some(("503 Service Unavailable", "too many things".into())),
                    None => {
                        list.push(IotEntry {
                            name: name.clone(),
                            kind,
                            what: thing_type.clone(),
                            address: src,
                            value: None,
                            value2: None,
                            on: false,
                            image: None,
                            seen_us: now,
                        });
                        (list.len() - 1, true)
                    }
                };
                let mut changed = false;
                if let Some(e) = list.get_mut(at) {
                    changed = value.is_some() && value != e.value;
                    e.kind = kind;
                    e.what = thing_type;
                    e.address = src;
                    e.value = value.or(e.value);
                    e.value2 = value2.or(e.value2);
                    e.seen_us = now;
                }
                // A device new to the server takes what the rules say of the current readings;
                // a new reading sets the devices of the rules naming the sensor (a device
                // switched by hand stays so until a reading changes).
                if new && kind == "device" {
                    apply_rules(d, list, None);
                } else if kind == "sensor" && changed {
                    apply_rules(d, list, Some(&name));
                }
                Some(("200 OK", "registered".into()))
            }
            "state" => {
                let motion = list.iter().any(|e| e.what == "motion" && e.value.is_some_and(|v| v > 0));
                match list.iter_mut().find(|e| e.name == name) {
                    Some(e) => {
                        let mut body = if e.on { "on".to_string() } else { "off".to_string() };
                        if e.what == "webcam" {
                            let at = clock(now);
                            let shot =
                                if motion { format!("still image {at}: someone in view") } else { format!("still image {at}: nobody in view") };
                            e.image = e.on.then_some(shot);
                            if let Some(img) = &e.image {
                                body.push('\n');
                                body.push_str(img);
                            }
                        }
                        Some(("200 OK", body))
                    }
                    None => Some(("404 Not Found", "not registered".into())),
                }
            }
            _ => Some(("404 Not Found", "unknown IoT request".into())),
        }
    }

    /// Switches a registered device by hand, from the server's IoT page.
    pub fn iot_switch(&mut self, server: DeviceId, name: &str, on: bool) -> Result<(), SimError> {
        let e = self
            .iot
            .servers
            .get_mut(&server)
            .and_then(|l| l.iter_mut().find(|e| e.name == name && e.kind == "device"))
            .ok_or_else(|| SimError::NoSource(format!("{name} is not a registered smart device")))?;
        e.on = on;
        Ok(())
    }

    /// One smart thing reports to its server (a sensor its reading; a device registers and
    /// asks whether to be on).
    pub fn iot_report(&mut self, t: &Topology, dev: DeviceId) -> Result<(), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        if d.kind != DeviceKind::Iot {
            return Err(SimError::NoSource(format!("{} is not a smart thing", d.name)));
        }
        self.env_step(t);
        let now = self.now;
        let reading = self.iot_reading(t, d);
        // A device wired to a board follows the board's program, not the server.
        let wired = t.board_of(dev).is_some();
        let before = self.iot.things.get(&dev).is_some_and(|s| s.on);
        let level = self.iot.things.get(&dev).map_or(0, |s| s.level);
        let result = (|| {
            if t.is_dark(dev) {
                return Err(SimError::NoSource("switched off".into()));
            }
            let server = d.iot.server.ok_or_else(|| SimError::NoSource("no IoT server set".into()))?;
            // A space would end the request's path (the server reads the second word of the request line).
            let what = thing(d).map_or("thing", |x| x.what).replace(' ', "+");
            let path = if is_sensor(d) {
                let second = self.iot_reading2(t, d).map(|v| format!("&value2={v}")).unwrap_or_default();
                format!("/iot/register?name={}&kind=sensor&type={what}&value={reading}{second}", d.name)
            } else {
                format!("/iot/register?name={}&kind=device&type={what}", d.name)
            };
            let (status, _) = self.http_get(t, dev, server, &server.to_string(), &path)?;
            if !status.starts_with("200") {
                return Err(SimError::NoSource(format!("the server answered {status}")));
            }
            let mut on = false;
            let mut image = None;
            if is_actuator(d) {
                let (status, body) = self.http_get(t, dev, server, &server.to_string(), &format!("/iot/state?name={}", d.name))?;
                if !status.starts_with("200") {
                    return Err(SimError::NoSource(format!("the server answered {status}")));
                }
                let mut lines = body.lines();
                on = lines.next().map(str::trim) == Some("on");
                image = lines.next().map(|l| l.chars().take(120).collect());
            }
            Ok((server, on, image))
        })();
        let st = self.iot.things.entry(dev).or_default();
        st.last_report_us = Some(now);
        match result {
            Ok((server, on, image)) => {
                let on = if wired { before } else { on };
                *st = ThingState { registered: Some(server), on, level, message: None, image, last_report_us: Some(now) };
                Ok(())
            }
            Err(e) => {
                st.registered = None;
                st.message = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// Every smart thing reports: sensors first, so the devices hear what the rules decided.
    pub fn iot_sync(&mut self, t: &Topology) {
        let sensors: Vec<DeviceId> = t.devices.iter().filter(|d| is_sensor(d) && d.iot.server.is_some()).map(|d| d.id).collect();
        let devices: Vec<DeviceId> = t.devices.iter().filter(|d| is_actuator(d) && d.iot.server.is_some()).map(|d| d.id).collect();
        for d in sensors.into_iter().chain(devices) {
            let _ = self.iot_report(t, d);
        }
    }

    /// When the next timed report is due (things with a server and an interval).
    fn iot_next_due(&self, t: &Topology) -> Option<u64> {
        t.devices
            .iter()
            .filter(|d| thing(d).is_some() && d.iot.server.is_some() && d.iot.interval > 0)
            .map(|d| {
                let last = self.iot.things.get(&d.id).and_then(|s| s.last_report_us);
                last.map_or(self.now, |l| l.saturating_add(u64::from(d.iot.interval).saturating_mul(1_000_000)))
            })
            .min()
    }

    /// The things whose timer has run out report (sensors first).
    fn iot_timed(&mut self, t: &Topology) {
        let now = self.now;
        let due = |d: &Device, s: &Sim| {
            d.kind == DeviceKind::Iot
                && d.iot.server.is_some()
                && d.iot.interval > 0
                && s.iot
                    .things
                    .get(&d.id)
                    .and_then(|x| x.last_report_us)
                    .is_none_or(|l| now.saturating_sub(l) >= u64::from(d.iot.interval).saturating_mul(1_000_000))
        };
        let sensors: Vec<DeviceId> = t.devices.iter().filter(|d| is_sensor(d) && due(d, self)).map(|d| d.id).collect();
        let devices: Vec<DeviceId> = t.devices.iter().filter(|d| is_actuator(d) && due(d, self)).map(|d| d.id).collect();
        for d in sensors.into_iter().chain(devices) {
            let _ = self.iot_report(t, d);
        }
    }

    /// Runs the simulation for `us` microseconds, the smart things reporting as their timers
    /// run out.
    pub(crate) fn run_with_iot(&mut self, t: &Topology, deadline: u64) {
        for _ in 0..MAX_TIMED_ROUNDS {
            let due = [self.iot_next_due(t), self.board_next_due(t)].into_iter().flatten().min();
            match due.filter(|at| *at < deadline) {
                Some(at) => {
                    if at > self.now {
                        self.advance(t, at, |_| false);
                    }
                    // Each report stamps its time and each program slice sets its next wake,
                    // so the next due time moves on.
                    self.env_step(t);
                    self.iot_timed(t);
                    self.board_timed(t);
                }
                None => break,
            }
        }
        if deadline > self.now {
            self.advance(t, deadline, |_| false);
        }
        self.env_step(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_are_read_safely() {
        assert_eq!(query("name=Fan1&kind=device", "kind"), Some("device"));
        assert_eq!(query("name", "name"), None);
        assert_eq!(query("", "name"), None);
        assert_eq!(query("a=&name=x=y", "name"), Some("x=y"));
        assert_eq!(clock(3_725_000_000), "01:02:05");
    }
}
