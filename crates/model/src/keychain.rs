//! Key chains (`key chain NAME`, `key N`, `key-string`, `accept-lifetime`, `send-lifetime`):
//! the keys a routing protocol signs with and accepts, each valid between two times of the
//! device's clock. OSPF reads a chain through `ip ospf authentication key-chain NAME`.

use serde::{Deserialize, Serialize};

/// Most key chains a device keeps.
pub const MAX_CHAINS: usize = 16;
/// Most keys one chain keeps.
pub const MAX_KEYS: usize = 32;
/// Longest chain name or key string kept.
pub const MAX_TEXT: usize = 80;

/// When a key may be used: from `start` (Unix seconds) to `end`, or for ever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lifetime {
    pub start: i64,
    /// `None` is `infinite`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
}

impl Lifetime {
    /// Whether `now` (Unix seconds) falls inside the lifetime.
    pub fn covers(&self, now: i64) -> bool {
        now >= self.start && self.end.is_none_or(|e| now < e)
    }
}

/// One `key N` of a chain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    /// `key-string TEXT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,
    /// `accept-lifetime`: when packets signed with the key are accepted (always when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<Lifetime>,
    /// `send-lifetime`: when the key signs packets (always when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send: Option<Lifetime>,
}

impl Key {
    /// Whether packets signed with this key are accepted at `now`.
    pub fn accepts(&self, now: i64) -> bool {
        self.string.is_some() && self.accept.is_none_or(|l| l.covers(now))
    }

    /// Whether this key signs packets at `now`.
    pub fn sends(&self, now: i64) -> bool {
        self.string.is_some() && self.send.is_none_or(|l| l.covers(now))
    }
}

/// A named chain of keys, by key number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chain {
    pub name: String,
    #[serde(default)]
    pub keys: std::collections::BTreeMap<u32, Key>,
}

/// A device's key chains.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyChains {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chains: Vec<Chain>,
}

impl KeyChains {
    pub fn is_default(&self) -> bool {
        self.chains.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&Chain> {
        self.chains.iter().find(|c| c.name == name)
    }

    /// The chain called `name`, created when there is room.
    pub fn get_or_add(&mut self, name: &str) -> Option<&mut Chain> {
        if !self.chains.iter().any(|c| c.name == name) {
            if self.chains.len() >= MAX_CHAINS {
                return None;
            }
            self.chains.push(Chain { name: name.to_string(), keys: std::collections::BTreeMap::new() });
        }
        self.chains.iter_mut().find(|c| c.name == name)
    }

    /// Bounds what a lab file holds.
    pub fn sanitize(&mut self) {
        self.chains.truncate(MAX_CHAINS);
        for c in &mut self.chains {
            c.name.truncate(MAX_TEXT);
            while c.keys.len() > MAX_KEYS {
                c.keys.pop_last();
            }
            for k in c.keys.values_mut() {
                if let Some(s) = &mut k.string {
                    s.truncate(MAX_TEXT);
                }
            }
        }
    }
}

impl Chain {
    /// The keys that accept packets at `now`, as (key number, string); only numbers 1-255 can
    /// be an OSPF key id.
    pub fn accepting(&self, now: i64) -> Vec<(u8, &str)> {
        self.keys
            .iter()
            .filter(|(_, k)| k.accepts(now))
            .filter_map(|(n, k)| Some((u8::try_from(*n).ok().filter(|n| *n >= 1)?, k.string.as_deref()?)))
            .collect()
    }

    /// The key that signs at `now`: of the keys whose send lifetime covers it, the one that
    /// became valid last (the highest number among equals).
    pub fn signing(&self, now: i64) -> Option<(u8, &str)> {
        self.keys
            .iter()
            .filter(|(_, k)| k.sends(now))
            .filter_map(|(n, k)| Some((u8::try_from(*n).ok().filter(|n| *n >= 1)?, k.string.as_deref()?, k.send.map_or(i64::MIN, |l| l.start))))
            .max_by_key(|(n, _, start)| (*start, *n))
            .map(|(n, s, _)| (n, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> Chain {
        let mut c = Chain { name: "K".into(), keys: Default::default() };
        c.keys.insert(1, Key { string: Some("sk-test-a".into()), accept: None, send: Some(Lifetime { start: 100, end: Some(200) }) });
        c.keys.insert(2, Key { string: Some("sk-test-b".into()), accept: None, send: Some(Lifetime { start: 150, end: None }) });
        c.keys.insert(300, Key { string: Some("sk-test-c".into()), accept: None, send: None });
        c.keys.insert(3, Key::default());
        c
    }

    #[test]
    fn the_newest_valid_key_signs() {
        let c = chain();
        assert_eq!(c.signing(120), Some((1, "sk-test-a")));
        assert_eq!(c.signing(160), Some((2, "sk-test-b")));
        assert_eq!(c.signing(250), Some((2, "sk-test-b")));
    }

    #[test]
    fn keys_without_a_string_or_an_ospf_id_are_not_used() {
        let c = chain();
        assert_eq!(c.accepting(10).iter().map(|k| k.0).collect::<Vec<_>>(), vec![1, 2]);
    }
}
