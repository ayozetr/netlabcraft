//! The `.nlcraft` lab file: UTF-8 JSON, `{"format":"netlabcraft","version":1,"topology":{…}}`.
//! Readers accept older versions and refuse newer ones; loading always sanitizes.

use serde::{Deserialize, Serialize};

use crate::{ModelError, Result, Topology};

pub const FORMAT: &str = "netlabcraft";
/// The format name labs carried before the project was renamed (still read).
pub const LEGACY_FORMAT: &str = "packetcraft";
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "nlcraft";
/// Extensions the open dialogs accept: today's and the one from before the rename.
pub const OPEN_EXTENSIONS: [&str; 2] = ["nlcraft", "pcraft"];
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
    if env.format != FORMAT && env.format != LEGACY_FORMAT {
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

    #[test]
    fn labs_saved_before_the_rename_still_open() {
        let t = Topology { notes: "old".into(), ..Default::default() };
        let old = to_json(&t).unwrap().replace("\"netlabcraft\"", "\"packetcraft\"");
        assert!(old.contains("\"packetcraft\""));
        assert_eq!(from_json(&old).unwrap().notes, "old");
        assert!(from_json(&to_json(&t).unwrap().replace("\"netlabcraft\"", "\"other\"")).is_err());
    }
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
    }
}
