//! IoT: smart sensors and smart devices register with a server's IoT service over HTTP; the
//! server lists them with their state and applies simple rules (a sensor reading above a
//! threshold switches a device on). The exchange is in `packetcraft-sim`'s `iot` module.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, MAX_NAME};

/// Most rules a server keeps.
pub const MAX_RULES: usize = 32;
/// The readings a sensor may report.
pub const MIN_VALUE: i32 = -1_000;
pub const MAX_VALUE: i32 = 1_000;

/// `if SENSOR > ABOVE then ACTUATOR on (else off)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IotRule {
    pub sensor: String,
    pub above: i32,
    pub actuator: String,
}

/// A device's IoT settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Iot {
    /// Smart sensors and devices: the IoT server they register with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Ipv4Addr>,
    /// Smart sensors: the reading (degrees, percent…).
    #[serde(skip_serializing_if = "is_zero")]
    pub value: i32,
    /// Servers: the IoT service is on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub service: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<IotRule>,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

impl Iot {
    pub fn is_default(&self) -> bool {
        *self == Iot::default()
    }

    /// Caps what a hostile file could hold.
    pub fn sanitize(&mut self) {
        self.value = self.value.clamp(MIN_VALUE, MAX_VALUE);
        self.rules.truncate(MAX_RULES);
        for r in &mut self.rules {
            r.sensor = r.sensor.chars().take(MAX_NAME).collect();
            r.actuator = r.actuator.chars().take(MAX_NAME).collect();
        }
        self.rules.retain(|r| !r.sensor.is_empty() && !r.actuator.is_empty());
    }
}

/// A smart sensor (reports a reading).
pub fn is_sensor(d: &Device) -> bool {
    d.kind == DeviceKind::Iot && d.catalogue_model().id == "IoT-Sensor"
}

/// A smart device (switched on and off by the server).
pub fn is_actuator(d: &Device) -> bool {
    d.kind == DeviceKind::Iot && !is_sensor(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_settings_are_trimmed() {
        let mut i = Iot { value: 99_999, rules: vec![IotRule { sensor: String::new(), above: 1, actuator: "Fan".into() }], ..Iot::default() };
        i.rules.extend((0..40).map(|n| IotRule { sensor: format!("S{n}"), above: n, actuator: "x".repeat(200) }));
        i.sanitize();
        assert_eq!(i.value, MAX_VALUE);
        assert!(i.rules.len() < MAX_RULES && i.rules.iter().all(|r| r.actuator.len() == MAX_NAME));
    }
}
