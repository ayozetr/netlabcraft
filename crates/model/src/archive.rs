//! The configuration archive (`archive` mode, `archive config`, `configure replace`): numbered
//! copies of the running configuration kept in flash, the oldest dropped past `maximum`.

use serde::{Deserialize, Serialize};

/// Most copies kept (the console accepts 1 to this).
pub const MAX_COPIES: u16 = 14;
/// How many it keeps unless `maximum` says.
pub const DEFAULT_COPIES: u16 = 10;
/// Biggest configuration text kept per copy.
pub const MAX_TEXT: usize = 262_144;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub number: u32,
    /// The device clock (seconds since 1970) when the copy was made.
    pub at: i64,
    pub config: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Archive {
    /// `path URL`: where copies go (`flash:archive`); empty until set.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// `maximum N`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<u16>,
    /// `time-period MINUTES`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_period: Option<u32>,
    /// `write-memory`: saving the startup configuration also archives it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub write_memory: bool,
    /// The copies, oldest first (kept across reloads, as flash is).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<Entry>,
    /// The number the next copy gets.
    pub next: u32,
}

impl Archive {
    /// Whether anything is set (the `archive` block of a running configuration).
    pub fn configured(&self) -> bool {
        !self.path.is_empty() || self.maximum.is_some() || self.time_period.is_some() || self.write_memory
    }

    /// The most copies it keeps.
    pub fn capacity(&self) -> usize {
        usize::from(self.maximum.unwrap_or(DEFAULT_COPIES).clamp(1, MAX_COPIES))
    }

    /// The name of copy `n`: `PATH-N`.
    pub fn name_of(&self, n: u32) -> String {
        format!("{}-{n}", self.path)
    }

    /// Keeps a copy of a configuration; the oldest is dropped past the capacity. Returns its
    /// number, or `None` when there is no path.
    pub fn push(&mut self, config: &str, at: i64) -> Option<u32> {
        if self.path.is_empty() {
            return None;
        }
        let number = self.next.saturating_add(1);
        self.next = number;
        self.entries.push(Entry { number, at, config: config.chars().take(MAX_TEXT).collect() });
        while self.entries.len() > self.capacity() {
            self.entries.remove(0);
        }
        Some(number)
    }

    /// A copy by number, or by a name that ends in `-N`.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let digits: String = name.rsplit(|c: char| !c.is_ascii_digit()).next().unwrap_or("").to_string();
        let n: u32 = digits.parse().ok()?;
        self.entries.iter().find(|e| e.number == n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_numbered_and_the_oldest_goes() {
        let mut a = Archive::default();
        assert_eq!(a.push("x", 0), None);
        a.path = "flash:cfg".into();
        a.maximum = Some(2);
        for i in 0..3 {
            assert_eq!(a.push(&format!("c{i}"), i64::from(i)), Some(u32::try_from(i).unwrap_or(0) + 1));
        }
        assert_eq!(a.entries.iter().map(|e| e.number).collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(a.find("flash:cfg-3").map(|e| e.config.as_str()), Some("c2"));
        assert_eq!(a.find("3").map(|e| e.number), Some(3));
        assert!(a.find("flash:cfg-1").is_none() && a.find("nothing").is_none());
        assert_eq!(a.name_of(3), "flash:cfg-3");
    }
}
