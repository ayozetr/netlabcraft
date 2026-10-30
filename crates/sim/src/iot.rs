//! IoT: smart things register with an IoT service (a server's, or a home gateway's built-in
//! one) over HTTP, the service keeps their state and applies its rules. Things report on a
//! timer in simulated time (`Iot::interval`), and at once when their reading changes.
//!
//! Requests are plain HTTP GETs to the service's web port (80), our own small interface:
//!
//! - `GET /iot/register?name=N&kind=sensor&type=T&value=V` (a sensor reports its reading) or
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
use netlabcraft_model::iot::{is_actuator, is_sensor, thing};
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
    /// Why the last report failed.
    pub message: Option<String>,
    /// Webcams: the still image the server returned.
    pub image: Option<String>,
    /// When it last reported (simulated microseconds), for the timer.
    pub last_report_us: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct IotState {
    servers: BTreeMap<DeviceId, Vec<IotEntry>>,
    things: BTreeMap<DeviceId, ThingState>,
}

impl IotState {
    pub(crate) fn forget(&mut self, d: DeviceId) {
        self.servers.remove(&d);
        self.things.remove(&d);
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
                let thing_type: String = query(q, "type").unwrap_or(if kind == "sensor" { "sensor" } else { "device" }).chars().take(16).collect();
                let value = query(q, "value").and_then(|v| v.parse::<i32>().ok());
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
        let now = self.now;
        let result = (|| {
            if t.is_dark(dev) {
                return Err(SimError::NoSource("switched off".into()));
            }
            let server = d.iot.server.ok_or_else(|| SimError::NoSource("no IoT server set".into()))?;
            let what = thing(d).map_or("thing", |x| x.what);
            let path = if is_sensor(d) {
                format!("/iot/register?name={}&kind=sensor&type={what}&value={}", d.name, d.iot.value)
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
                *st = ThingState { registered: Some(server), on, message: None, image, last_report_us: Some(now) };
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
            .filter(|d| d.kind == DeviceKind::Iot && d.iot.server.is_some() && d.iot.interval > 0)
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
            match self.iot_next_due(t).filter(|at| *at < deadline) {
                Some(at) => {
                    if at > self.now {
                        self.advance(t, at, |_| false);
                    }
                    // Each report stamps its time, so the next due time moves on.
                    self.iot_timed(t);
                }
                None => break,
            }
        }
        if deadline > self.now {
            self.advance(t, deadline, |_| false);
        }
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
