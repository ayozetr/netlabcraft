//! The `.nlcraft` lab file: UTF-8 JSON, `{"format":"netlabcraft","version":3,"topology":{…}}`.
//! Readers accept older versions and refuse newer ones; loading always sanitizes.

use serde::{Deserialize, Serialize};

use crate::{ModelError, Result, Topology};

pub const FORMAT: &str = "netlabcraft";
/// 2: the lab-level `environment` (the smart things' shared room).
/// 3: smartphones and tablets have a `Cellular0` port, and devices may carry `cell` settings.
/// Older files still open (their phones get the port).
pub const VERSION: u32 = 3;
pub const EXTENSION: &str = "nlcraft";

/// Largest lab file accepted (hostile input).
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u32,
    topology: Topology,
}

pub fn to_json(t: &Topology) -> Result<String> {
    let env = Envelope { format: FORMAT.into(), version: VERSION, topology: t.clone() };
    serde_json::to_string_pretty(&env).map_err(|e| ModelError::Invalid(format!("couldn't write the lab: {e}")))
}

pub fn from_json(s: &str) -> Result<Topology> {
    if s.len() > MAX_BYTES {
        return Err(ModelError::Invalid("the file is too large to be a lab".into()));
    }
    let env: Envelope = serde_json::from_str(s).map_err(|e| ModelError::Invalid(format!("not a NetLabCraft lab: {e}")))?;
    if env.format != FORMAT {
        return Err(ModelError::Invalid(format!("not a NetLabCraft lab (format `{}`)", env.format)));
    }
    if env.version > VERSION {
        return Err(ModelError::Invalid(format!(
            "this lab was saved by a newer NetLabCraft (file version {}, this build reads up to {VERSION})",
            env.version
        )));
    }
    let mut t = env.topology;
    t.sanitize();
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, End};

    #[test]
    fn round_trip() {
        let mut t = Topology::new();
        let a = t.add_device(DeviceKind::Router, None, 1.0, 2.0).unwrap();
        let b = t.add_device(DeviceKind::Switch, None, 3.0, 4.0).unwrap();
        t.connect(End { device: a, port: 0 }, End { device: b, port: 25 }).unwrap();
        t.device_mut(a).unwrap().ports[0].ip = Some("10.1.1.1/24".parse().unwrap());
        let s = to_json(&t).unwrap();
        assert_eq!(from_json(&s).unwrap(), t);
    }

    #[test]
    fn refuses_foreign_and_future_files() {
        assert!(from_json("{}").is_err());
        assert!(from_json("not json").is_err());
        assert!(from_json(r#"{"format":"other","version":1,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_err());
        assert!(from_json(r#"{"format":"netlabcraft","version":99,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_err());
        assert!(from_json(r#"{"format":"netlabcraft","version":1,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":2,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":3,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":4,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_err());
    }

    #[test]
    fn a_version_1_phone_opens_with_a_cellular_port_and_cell_settings_round_trip() {
        let mut t = Topology::new();
        let ph = t.add_model(crate::models::find("Smartphone").unwrap(), None, 0.0, 0.0).unwrap();
        // As saved before the cellular radio: version 1, one port.
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();
        v["version"] = 1.into();
        v["topology"]["devices"][0]["ports"].as_array_mut().unwrap().truncate(1);
        let old = v.to_string();
        assert!(!old.contains("Cellular0"), "{old}");
        let back = from_json(&old).unwrap();
        assert_eq!(back.device(ph).unwrap().ports.len(), 2);
        assert_eq!(back.device(ph).unwrap().ports[1].name, "Cellular0");
        let mut t2 = back;
        t2.device_mut(ph).unwrap().cell.apn = "internet".into();
        t2.device_mut(ph).unwrap().cell.data_off = true;
        let again = from_json(&to_json(&t2).unwrap()).unwrap();
        assert_eq!(again.device(ph).unwrap().cell, t2.device(ph).unwrap().cell);
    }
}
