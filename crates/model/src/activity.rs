//! Activities: a lab that carries instructions and the results a student should reach, each
//! worth some points.
//!
//! An instructor builds the finished network, captures its configuration as checks, then
//! saves the lab in its starting state. With `hidden`, configuration answers are stored as
//! salted hashes so students can't read them in the file. (A determined student could still
//! guess lines and hash them; this keeps honest people honest, it isn't encryption.)

use packetcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

/// Most checks an activity holds.
pub const MAX_CHECKS: usize = 2_000;
/// Longest text a check or the instructions hold.
pub const MAX_TEXT: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub title: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub checks: Vec<Check>,
    /// Configuration answers are stored as hashes.
    #[serde(default)]
    pub hidden: bool,
    /// Salt for the hashes, fixed when the activity is created.
    #[serde(default)]
    pub salt: String,
    /// Minutes the student has (0: no limit). The time counts from when the lab is opened.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub time_limit: u32,
    /// The hash of the instructor's password; while set, the activity can't be changed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lock: String,
    /// While locked, Check Results shows the score only, not which checks passed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub score_only: bool,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Longest time limit, in minutes.
pub const MAX_TIME_LIMIT: u32 = 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// What the student sees.
    pub label: String,
    #[serde(default = "one")]
    pub points: u32,
    /// What the student reads when the check fails (a hint, not the answer).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub feedback: String,
    #[serde(flatten)]
    pub kind: CheckKind,
}

fn one() -> u32 {
    1
}

/// What a check looks at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CheckKind {
    /// A line of the device's running configuration, with its block (`interface X | ip address …`).
    Config { device: DeviceId, line: String },
    /// The same, stored as a salted hash.
    ConfigHashed { device: DeviceId, hash: String },
    /// A ping from a device to an address must succeed (or must fail).
    Ping { from: DeviceId, to: String, succeed: bool },
    /// A host's IPv4 address (`10.0.0.2/24`) and, optionally, its gateway.
    HostAddress {
        device: DeviceId,
        address: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gateway: Option<String>,
    },
    /// A command's output must contain a text (case-insensitive).
    Output { device: DeviceId, command: String, contains: String },
}

impl CheckKind {
    /// The device the check is about (the source, for a ping).
    pub fn device(&self) -> DeviceId {
        match self {
            CheckKind::Config { device, .. }
            | CheckKind::ConfigHashed { device, .. }
            | CheckKind::HostAddress { device, .. }
            | CheckKind::Output { device, .. } => *device,
            CheckKind::Ping { from, .. } => *from,
        }
    }
}

/// A configuration line as compared: trimmed, single spaces, lower case.
pub fn normalize(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The running configuration as comparable lines: each line inside a block carries its block
/// header (`interface gigabitethernet0/0 | ip address 10.0.0.1 255.255.255.0`); comments, `end`
/// and blank lines are left out.
pub fn flatten(config: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut block: Option<String> = None;
    for raw in config.lines() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('!') || raw.trim() == "end" {
            if raw.trim_start().starts_with('!') {
                block = None;
            }
            continue;
        }
        if raw.starts_with(' ') {
            match &block {
                Some(b) => out.push(format!("{b} | {}", normalize(raw))),
                None => out.push(normalize(raw)),
            }
        } else {
            let line = normalize(raw);
            out.push(line.clone());
            block = Some(line);
        }
    }
    out
}

/// The salted hash of a flattened configuration line on a device (FNV-1a, 64 bits).
pub fn line_hash(salt: &str, device: DeviceId, line: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in salt.bytes().chain(device.0.to_string().bytes()).chain([0u8]).chain(line.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

impl Activity {
    /// Caps sizes from hostile files.
    pub fn sanitize(&mut self) {
        self.checks.truncate(MAX_CHECKS);
        self.time_limit = self.time_limit.min(MAX_TIME_LIMIT);
        let texts = [&mut self.title, &mut self.instructions, &mut self.salt, &mut self.lock];
        for s in texts.into_iter().chain(self.checks.iter_mut().map(|c| &mut c.feedback)) {
            if s.len() > MAX_TEXT {
                let cut = (0..=MAX_TEXT).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
                s.truncate(cut);
            }
        }
        for c in &mut self.checks {
            c.points = c.points.min(1_000);
        }
    }

    pub fn locked(&self) -> bool {
        !self.lock.is_empty()
    }

    /// The hash a password is stored as.
    pub fn password_hash(&self, password: &str) -> String {
        line_hash(&self.salt, DeviceId(0), &format!("lock:{password}"))
    }

    pub fn total_points(&self) -> u32 {
        self.checks.iter().map(|c| c.points).fold(0, u32::saturating_add)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_keeps_blocks() {
        let cfg = "! comment\nhostname R1\n!\ninterface GigabitEthernet0/0\n ip address 10.0.0.1  255.255.255.0\n no shutdown\n!\nip route 0.0.0.0 0.0.0.0 10.0.0.2\nend";
        assert_eq!(
            flatten(cfg),
            [
                "hostname r1",
                "interface gigabitethernet0/0",
                "interface gigabitethernet0/0 | ip address 10.0.0.1 255.255.255.0",
                "interface gigabitethernet0/0 | no shutdown",
                "ip route 0.0.0.0 0.0.0.0 10.0.0.2"
            ]
        );
        assert_ne!(line_hash("a", DeviceId(1), "x"), line_hash("b", DeviceId(1), "x"));
        assert_ne!(line_hash("a", DeviceId(1), "x"), line_hash("a", DeviceId(2), "x"));
    }

    #[test]
    fn checks_round_trip_as_json() {
        let a = Activity {
            title: "t".into(),
            instructions: "i".into(),
            checks: vec![
                Check {
                    label: "l".into(),
                    points: 2,
                    feedback: "look at R1".into(),
                    kind: CheckKind::Ping { from: DeviceId(1), to: "10.0.0.1".into(), succeed: true },
                },
                Check {
                    label: "m".into(),
                    points: 1,
                    feedback: String::new(),
                    kind: CheckKind::ConfigHashed { device: DeviceId(2), hash: "ab".into() },
                },
            ],
            hidden: true,
            salt: "s".into(),
            time_limit: 30,
            lock: String::new(),
            score_only: true,
        };
        let j = serde_json::to_string(&a).unwrap();
        assert!(j.contains(r#""type":"ping""#));
        assert_eq!(serde_json::from_str::<Activity>(&j).unwrap(), a);
        assert_eq!(a.total_points(), 3);
    }
}
