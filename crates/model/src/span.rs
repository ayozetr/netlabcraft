//! Port mirroring (SPAN) on switches: `monitor session N source ...` and `... destination ...`.
//!
//! A session names what to watch (ports, or VLANs) and the one port that receives a copy of
//! the frames. The sessions live in [`crate::l2sec::SwitchL2::span`]; the simulator reads them
//! where frames enter and leave a switch port (`netlabcraft-sim`).

use std::collections::{BTreeMap, BTreeSet};

use netlabcraft_core::VlanId;
use serde::{Deserialize, Serialize};

/// The highest session number `monitor session` takes.
pub const MAX_SESSION_ID: u8 = 66;
/// Sessions a switch holds at once.
pub const MAX_SESSIONS: usize = 2;
/// Source ports one session holds (a hostile file is cut to this).
pub const MAX_SOURCE_PORTS: usize = 128;
/// The longest port name kept from a file.
const MAX_NAME: usize = 64;

/// Which frames of a source are copied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpanDir {
    /// Frames the switch receives on the source and frames it sends out of it.
    #[default]
    Both,
    /// Only the frames the switch receives.
    Rx,
    /// Only the frames the switch sends.
    Tx,
}

impl SpanDir {
    pub const ALL: [SpanDir; 3] = [SpanDir::Both, SpanDir::Rx, SpanDir::Tx];

    /// The keyword the console takes.
    pub fn name(self) -> &'static str {
        match self {
            SpanDir::Both => "both",
            SpanDir::Rx => "rx",
            SpanDir::Tx => "tx",
        }
    }

    pub fn parse(s: &str) -> Option<SpanDir> {
        let l = s.trim().to_ascii_lowercase();
        SpanDir::ALL.into_iter().find(|d| d.name() == l)
    }

    /// Whether frames the switch receives are copied.
    pub fn rx(self) -> bool {
        self != SpanDir::Tx
    }

    /// Whether frames the switch sends are copied.
    pub fn tx(self) -> bool {
        self != SpanDir::Rx
    }
}

/// One monitor session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpanSession {
    /// The session number, 1 to [`MAX_SESSION_ID`].
    pub id: u8,
    /// Source ports by their full name (`FastEthernet0/1`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub ports: BTreeMap<String, SpanDir>,
    /// Source VLANs: every frame of the VLAN, on any port.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub vlans: BTreeMap<VlanId, SpanDir>,
    /// The port that receives the copies, by its full name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

impl SpanSession {
    pub fn new(id: u8) -> SpanSession {
        SpanSession { id, ..SpanSession::default() }
    }

    /// Nothing configured: the session is gone.
    pub fn is_empty(&self) -> bool {
        self.ports.is_empty() && self.vlans.is_empty() && self.destination.is_none()
    }

    /// Whether a frame received (`rx`) or sent on `port`, in `vlan` (when known), is copied.
    pub fn watches(&self, port: &str, vlan: Option<VlanId>, rx: bool) -> bool {
        let hit = |d: &SpanDir| if rx { d.rx() } else { d.tx() };
        self.ports.get(port).is_some_and(hit) || vlan.and_then(|v| self.vlans.get(&v)).is_some_and(hit)
    }
}

/// Drops what a hostile file cannot mean: bad or repeated session numbers, more sessions than
/// a switch holds, too many sources, odd names.
pub fn sanitize(sessions: &mut Vec<SpanSession>) {
    sessions.retain(|s| (1..=MAX_SESSION_ID).contains(&s.id));
    let mut seen = BTreeSet::new();
    sessions.retain(|s| seen.insert(s.id));
    sessions.sort_by_key(|s| s.id);
    sessions.truncate(MAX_SESSIONS);
    let name_ok = |n: &String| !n.is_empty() && n.chars().count() <= MAX_NAME && !n.chars().any(char::is_control);
    for s in sessions.iter_mut() {
        s.ports.retain(|n, _| name_ok(n));
        while s.ports.len() > MAX_SOURCE_PORTS {
            s.ports.pop_last();
        }
        if s.destination.as_ref().is_some_and(|n| !name_ok(n)) {
            s.destination = None;
        }
    }
    sessions.retain(|s| !s.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_copies_by_direction_and_by_vlan() {
        let mut s = SpanSession::new(1);
        s.ports.insert("FastEthernet0/1".into(), SpanDir::Rx);
        s.ports.insert("FastEthernet0/2".into(), SpanDir::Both);
        s.vlans.insert(VlanId::new(10).unwrap(), SpanDir::Tx);
        assert!(s.watches("FastEthernet0/1", None, true) && !s.watches("FastEthernet0/1", None, false));
        assert!(s.watches("FastEthernet0/2", None, true) && s.watches("FastEthernet0/2", None, false));
        assert!(!s.watches("FastEthernet0/3", None, true));
        assert!(s.watches("FastEthernet0/3", VlanId::new(10), false) && !s.watches("FastEthernet0/3", VlanId::new(10), true));
        assert!(!s.watches("FastEthernet0/3", VlanId::new(20), false));
    }

    #[test]
    fn a_hostile_list_of_sessions_is_cut_down() {
        let mut list: Vec<SpanSession> = (0..=70)
            .map(|i| {
                let mut s = SpanSession::new(i);
                s.destination = Some("FastEthernet0/24".into());
                s
            })
            .collect();
        list.push(SpanSession::new(5));
        list.push(SpanSession::new(7));
        sanitize(&mut list);
        assert_eq!(list.len(), MAX_SESSIONS);
        assert_eq!(list.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2]);
        let mut odd = vec![SpanSession::new(1)];
        odd[0].ports.insert("x\ny".into(), SpanDir::Both);
        sanitize(&mut odd);
        assert!(odd.is_empty(), "a session with only a bad name is empty and goes");
    }
}
