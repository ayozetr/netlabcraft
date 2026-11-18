//! Console settings the simulator keeps and shows but does not act on: the long tail of
//! `service …`, `ip classless`, line and interface options that a lab file or a pasted
//! configuration contains. Each is stored under the words that name it (its key), so typing
//! the command again replaces the value and the `no` form removes it, and the running
//! configuration writes them back. What is on by default is written only when it is turned off.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Most settings kept per scope (the device, one interface, one line group).
pub const MAX_PER_SCOPE: usize = 96;
/// Most interfaces (or line groups) that hold settings.
pub const MAX_SCOPES: usize = 512;
/// Longest value kept after the words of a setting.
pub const MAX_VALUE: usize = 80;

/// One scope's settings: key (the command's keywords) to the full line as it is written.
pub type Lines = BTreeMap<String, String>;

/// The settings of a device.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Knobs {
    /// Global configuration.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub global: Lines,
    /// Per interface, by its name (`GigabitEthernet0/0`, `Vlan10`, `Loopback0`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub iface: BTreeMap<String, Lines>,
    /// Per line group: `con` or `vty`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub line: BTreeMap<String, Lines>,
}

/// Where a setting lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope<'a> {
    Global,
    Iface(&'a str),
    Line(&'a str),
}

impl Knobs {
    fn lines(&self, scope: Scope) -> Option<&Lines> {
        match scope {
            Scope::Global => Some(&self.global),
            Scope::Iface(n) => self.iface.get(n),
            Scope::Line(n) => self.line.get(n),
        }
    }

    fn lines_mut(&mut self, scope: Scope) -> Result<&mut Lines, String> {
        let (map, name) = match scope {
            Scope::Global => return Ok(&mut self.global),
            Scope::Iface(n) => (&mut self.iface, n),
            Scope::Line(n) => (&mut self.line, n),
        };
        if !map.contains_key(name) && map.len() >= MAX_SCOPES {
            return Err(format!("% At most {MAX_SCOPES} interfaces or lines can hold settings"));
        }
        Ok(map.entry(name.to_string()).or_default())
    }

    /// Keeps `line` under `key`, replacing an earlier value.
    pub fn set(&mut self, scope: Scope, key: &str, line: String) -> Result<(), String> {
        let lines = self.lines_mut(scope)?;
        if !lines.contains_key(key) && lines.len() >= MAX_PER_SCOPE {
            return Err(format!("% At most {MAX_PER_SCOPE} settings of this kind"));
        }
        lines.insert(key.to_string(), line);
        Ok(())
    }

    /// Forgets the setting under `key`.
    pub fn clear(&mut self, scope: Scope, key: &str) {
        let (map, name) = match scope {
            Scope::Global => {
                self.global.remove(key);
                return;
            }
            Scope::Iface(n) => (&mut self.iface, n),
            Scope::Line(n) => (&mut self.line, n),
        };
        if let Some(l) = map.get_mut(name) {
            l.remove(key);
            if l.is_empty() {
                map.remove(name);
            }
        }
    }

    /// The setting under `key`, as written.
    pub fn get(&self, scope: Scope, key: &str) -> Option<&str> {
        self.lines(scope)?.get(key).map(String::as_str)
    }

    /// Every line of a scope, in key order.
    pub fn written(&self, scope: Scope) -> Vec<&str> {
        self.lines(scope).map(|l| l.values().map(String::as_str).collect()).unwrap_or_default()
    }

    /// Whether the device writes `key` as turned off (`no …`).
    pub fn is_off(&self, key: &str) -> bool {
        self.global.get(key).is_some_and(|l| l.starts_with("no "))
    }
}

/// Cleans a value typed after a setting's words: no control characters, bounded.
pub fn clean_value(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_VALUE).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_is_replaced_removed_and_bounded() {
        let mut k = Knobs::default();
        k.set(Scope::Global, "ip classless", "no ip classless".into()).unwrap();
        assert!(k.is_off("ip classless"));
        k.set(Scope::Iface("Gi0/0"), "load-interval", "load-interval 30".into()).unwrap();
        k.set(Scope::Iface("Gi0/0"), "load-interval", "load-interval 60".into()).unwrap();
        assert_eq!(k.written(Scope::Iface("Gi0/0")), ["load-interval 60"]);
        k.clear(Scope::Iface("Gi0/0"), "load-interval");
        assert!(k.iface.is_empty(), "an empty scope is dropped");
        for i in 0..200 {
            let _ = k.set(Scope::Global, &format!("k{i}"), format!("k{i}"));
        }
        assert_eq!(k.global.len(), MAX_PER_SCOPE);
        for i in 0..600 {
            let _ = k.set(Scope::Iface(&format!("I{i}")), "x", "x".into());
        }
        assert_eq!(k.iface.len(), MAX_SCOPES);
        assert_eq!(clean_value("a\u{7}b"), "ab");
        let json = serde_json::to_string(&k).unwrap();
        let back: Knobs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, k);
    }
}
