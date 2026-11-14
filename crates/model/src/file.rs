//! The `.nlcraft` lab file: UTF-8 JSON, `{"format":"netlabcraft","version":4,"topology":{…}}`.
//! Readers accept older versions and refuse newer ones; loading always sanitizes.

use serde::{Deserialize, Serialize};

use crate::{ModelError, Result, Topology};

pub const FORMAT: &str = "netlabcraft";
/// 2: the lab-level `environment` (the smart things' shared room).
/// 3: smartphones and tablets have a `Cellular0` port, and devices may carry `cell` settings.
/// 4: drawings have separate outline and fill settings (`outline`, `outline_color`, `outline_width`, `no_fill`).
/// Older files still open (their phones get the port; a drawing with only `color` draws as it always did).
pub const VERSION: u32 = 4;
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
    fn legacy_drawing_keeps_its_single_colour_and_styles_round_trip() {
        let old = r##"{"format":"netlabcraft","version":3,"topology":{"devices":[],"links":[],"next_id":1,
            "annotations":[{"id":1,"x":0,"y":0,"shape":"rectangle","w":10,"h":10,"color":"#aabbcc"}]}}"##;
        let t = from_json(old).unwrap();
        let a = &t.annotations[0];
        assert_eq!(a.color.as_deref(), Some("#aabbcc"));
        assert!(a.has_outline() && !a.no_fill && a.outline_color.is_none() && a.outline_px() == 1);
        // A legacy drawing is saved without the new keys.
        assert!(!to_json(&t).unwrap().contains("outline"));

        let mut t = Topology::new();
        let id = t.add_drawing(crate::Shape::Ellipse, 0.0, 0.0, 30.0, 20.0, None).unwrap();
        let patch = crate::StylePatch { outline: Some(true), outline_color: Some("#112233".into()), outline_width: Some(5), fill: Some(None) };
        t.annotation_mut(id).unwrap().apply_style(&patch).unwrap();
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert!(back.annotations[0].no_fill);
        assert_eq!(back.annotations[0].outline_px(), 5);
    }

    #[test]
    fn hostile_style_values_are_cleaned_on_load() {
        let bad = r##"{"format":"netlabcraft","version":4,"topology":{"devices":[],"links":[],"next_id":1,
            "annotations":[{"id":1,"x":0,"y":0,"shape":"line","w":10,"h":10,"outline_color":"red","outline_width":200}]}}"##;
        let t = from_json(bad).unwrap();
        assert_eq!(t.annotations[0].outline_color, None);
        assert_eq!(t.annotations[0].outline_px(), crate::MAX_OUTLINE_WIDTH);
    }

    #[test]
    fn refuses_bad_styles_without_changing_anything() {
        let mut t = Topology::new();
        let r = t.add_drawing(crate::Shape::Rectangle, 0.0, 0.0, 30.0, 20.0, Some("#aabbcc")).unwrap();
        let l = t.add_drawing(crate::Shape::Line, 0.0, 0.0, 30.0, 20.0, None).unwrap();
        let n = t.add_annotation(0.0, 0.0, "hi").unwrap();
        let before = t.clone();
        let with = |f: fn(&mut crate::StylePatch)| {
            let mut p = crate::StylePatch::default();
            f(&mut p);
            p
        };
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.outline_width = Some(0))).is_err());
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.outline_width = Some(9))).is_err());
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.outline_color = Some("red".into()))).is_err());
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.fill = Some(Some("#12".into())))).is_err());
        assert!(t.annotation_mut(l).unwrap().apply_style(&with(|p| p.fill = Some(None))).is_err());
        assert!(t.annotation_mut(l).unwrap().apply_style(&with(|p| p.outline = Some(false))).is_err());
        assert!(t.annotation_mut(n).unwrap().apply_style(&with(|p| p.outline = Some(true))).is_err());
        assert_eq!(t, before);
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
        assert!(from_json(r#"{"format":"netlabcraft","version":4,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":5,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_err());
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
