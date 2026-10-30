//! IoT: smart things register with a server's IoT service over HTTP, the server keeps their
//! state and applies its rules.
//!
//! Requests are plain HTTP GETs to the server's web service (port 80), our own small
//! interface:
//!
//! - `GET /iot/register?name=N&kind=sensor&value=V` (a sensor reports its reading) or
//!   `…&kind=device` (a smart device registers); the answer is `registered`.
//! - `GET /iot/state?name=N`: a smart device asks whether it should be on; the answer is `on`
//!   or `off`.
//!
//! When a sensor reports, every rule naming it sets its device: on above the threshold, off
//! otherwise. A device can also be switched by hand from the server's IoT page.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use packetcraft_core::DeviceId;
use packetcraft_model::iot::{is_actuator, is_sensor};
use packetcraft_model::{Device, DeviceKind, Topology};

use crate::{Sim, SimError};

/// Most things one server lists.
const MAX_THINGS: usize = 256;

/// A thing the server knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IotEntry {
    pub name: String,
    /// `sensor` or `device`.
    pub kind: &'static str,
    pub address: Ipv4Addr,
    /// The last reading (sensors).
    pub value: Option<i32>,
    /// Switched on (devices).
    pub on: bool,
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
        let (what, q) = path.split_once('?').unwrap_or((path, ""));
        let name: String = query(q, "name").unwrap_or("").chars().take(packetcraft_model::MAX_NAME).collect();
        if name.is_empty() {
            return Some(("400 Bad Request", "no name".into()));
        }
        let list = self.iot.servers.entry(d.id).or_default();
        match what {
            "register" => {
                let kind = if query(q, "kind") == Some("sensor") { "sensor" } else { "device" };
                let value = query(q, "value").and_then(|v| v.parse::<i32>().ok());
                let (at, new) = match list.iter().position(|e| e.name == name) {
                    Some(i) => (i, false),
                    None if list.len() >= MAX_THINGS => return Some(("503 Service Unavailable", "too many things".into())),
                    None => {
                        list.push(IotEntry { name: name.clone(), kind, address: src, value: None, on: false });
                        (list.len() - 1, true)
                    }
                };
                // A device new to the server takes what the rules say of the current readings.
                if new && kind == "device" {
                    let on = d
                        .iot
                        .rules
                        .iter()
                        .filter(|r| r.actuator.eq_ignore_ascii_case(&name))
                        .find_map(|r| list.iter().find(|e| e.name.eq_ignore_ascii_case(&r.sensor)).and_then(|e| e.value).map(|v| v > r.above));
                    if let (Some(on), Some(e)) = (on, list.get_mut(at)) {
                        e.on = on;
                    }
                }
                let mut changed = false;
                if let Some(e) = list.get_mut(at) {
                    changed = value.is_some() && value != e.value;
                    e.kind = kind;
                    e.address = src;
                    e.value = value.or(e.value);
                }
                // A new reading: the rules naming this sensor set their devices (a device
                // switched by hand stays so until the reading changes).
                if let (Some(v), "sensor", true) = (value, kind, changed) {
                    for r in d.iot.rules.iter().filter(|r| r.sensor.eq_ignore_ascii_case(&name)) {
                        if let Some(e) = list.iter_mut().find(|e| e.name.eq_ignore_ascii_case(&r.actuator)) {
                            e.on = v > r.above;
                        }
                    }
                }
                Some(("200 OK", "registered".into()))
            }
            "state" => match list.iter().find(|e| e.name == name) {
                Some(e) => Some(("200 OK", if e.on { "on".into() } else { "off".into() })),
                None => Some(("404 Not Found", "not registered".into())),
            },
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
            return Err(SimError::NoSource(format!("{} is not a smart sensor or device", d.name)));
        }
        let result = (|| {
            if d.powered_off {
                return Err(SimError::NoSource("switched off".into()));
            }
            let server = d.iot.server.ok_or_else(|| SimError::NoSource("no IoT server set".into()))?;
            let path = if is_sensor(d) {
                format!("/iot/register?name={}&kind=sensor&value={}", d.name, d.iot.value)
            } else {
                format!("/iot/register?name={}&kind=device", d.name)
            };
            let (status, _) = self.http_get(t, dev, server, &server.to_string(), &path)?;
            if !status.starts_with("200") {
                return Err(SimError::NoSource(format!("the server answered {status}")));
            }
            let mut on = false;
            if is_actuator(d) {
                let (status, body) = self.http_get(t, dev, server, &server.to_string(), &format!("/iot/state?name={}", d.name))?;
                if !status.starts_with("200") {
                    return Err(SimError::NoSource(format!("the server answered {status}")));
                }
                on = body.trim() == "on";
            }
            Ok((server, on))
        })();
        let st = self.iot.things.entry(dev).or_default();
        match result {
            Ok((server, on)) => {
                *st = ThingState { registered: Some(server), on, message: None };
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
    }
}
