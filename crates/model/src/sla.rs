//! IP SLA (`ip sla N`): operations that probe a destination on a schedule and keep what they
//! learned: whether the last probe got through, how long it took, how many were sent and
//! answered. A tracked object (`track N ip sla M …`) follows the last outcome, which is how a
//! failed probe takes a static route out of the table. The simulator runs the probes
//! (`netlabcraft_sim::sla`); this module holds the configuration and the results.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::track::SlaOutcome;

/// Most operations on one device.
pub const MAX_OPS: usize = 64;
/// Most reaction lines on one operation.
pub const MAX_REACTIONS: usize = 8;
/// Longest text kept for a tag or a reaction line.
pub const MAX_TEXT: usize = 120;
/// Highest operation number.
pub const MAX_OP_ID: u32 = 2_147_483_647;
/// Most probes one step of the scheduler sends per operation (a long jump in simulated time
/// does not send a probe for every missed interval).
pub const MAX_CATCH_UP: u32 = 5;

/// What an operation sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Probe {
    /// `icmp-echo A.B.C.D [source-ip A.B.C.D]`: an ICMP echo request.
    IcmpEcho { dest: Ipv4Addr, source: Option<Ipv4Addr> },
    /// `tcp-connect A.B.C.D PORT`: a TCP handshake.
    TcpConnect { dest: Ipv4Addr, port: u16 },
    /// `udp-echo A.B.C.D PORT`: answered only by a device with `ip sla responder`.
    UdpEcho { dest: Ipv4Addr, port: u16 },
    /// `udp-jitter A.B.C.D PORT`: like `udp-echo` (one datagram per probe).
    UdpJitter { dest: Ipv4Addr, port: u16 },
    /// `http get http://A.B.C.D/PATH`.
    HttpGet { dest: Ipv4Addr, path: String },
}

impl Probe {
    /// The command word that creates it.
    pub fn name(&self) -> &'static str {
        match self {
            Probe::IcmpEcho { .. } => "icmp-echo",
            Probe::TcpConnect { .. } => "tcp-connect",
            Probe::UdpEcho { .. } => "udp-echo",
            Probe::UdpJitter { .. } => "udp-jitter",
            Probe::HttpGet { .. } => "http get",
        }
    }

    pub fn dest(&self) -> Ipv4Addr {
        match self {
            Probe::IcmpEcho { dest, .. }
            | Probe::TcpConnect { dest, .. }
            | Probe::UdpEcho { dest, .. }
            | Probe::UdpJitter { dest, .. }
            | Probe::HttpGet { dest, .. } => *dest,
        }
    }

    /// The configuration line that makes it.
    pub fn line(&self) -> String {
        match self {
            Probe::IcmpEcho { dest, source: None } => format!("icmp-echo {dest}"),
            Probe::IcmpEcho { dest, source: Some(s) } => format!("icmp-echo {dest} source-ip {s}"),
            Probe::TcpConnect { dest, port } => format!("tcp-connect {dest} {port}"),
            Probe::UdpEcho { dest, port } => format!("udp-echo {dest} {port}"),
            Probe::UdpJitter { dest, port } => format!("udp-jitter {dest} {port}"),
            Probe::HttpGet { dest, path } => format!("http get http://{dest}{path}"),
        }
    }
}

/// When an operation starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Start {
    Now,
    /// Scheduled, but waiting for a start time that never comes.
    Pending,
    /// This many seconds after the schedule command.
    After(u32),
}

/// `ip sla schedule N life … start-time …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// Seconds the operation runs; `None`: for ever.
    pub life_s: Option<u32>,
    pub start: Start,
}

/// One operation's configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<Probe>,
    /// Seconds between probes (60 when not set).
    #[serde(default = "default_frequency")]
    pub frequency_s: u32,
    /// Milliseconds to wait for an answer (5000 when not set).
    #[serde(default = "default_timeout")]
    pub timeout_ms: u32,
    /// Milliseconds above which an answer counts as over threshold (5000 when not set).
    #[serde(default = "default_threshold")]
    pub threshold_ms: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    /// `ip sla reaction-configuration N …`: the lines kept as typed, one per condition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<String>,
}

fn default_frequency() -> u32 {
    60
}
fn default_timeout() -> u32 {
    5_000
}
fn default_threshold() -> u32 {
    5_000
}

impl Default for Op {
    fn default() -> Op {
        Op { probe: None, frequency_s: 60, timeout_ms: 5_000, threshold_ms: 5_000, tag: String::new(), schedule: None, reactions: Vec::new() }
    }
}

/// What an operation has measured since it started.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// Simulated time the schedule started (µs); `None` before it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_us: Option<u64>,
    /// When the last probe was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_us: Option<u64>,
    #[serde(default)]
    pub sent: u32,
    #[serde(default)]
    pub answered: u32,
    #[serde(default)]
    pub over_threshold: u32,
    #[serde(default)]
    pub failed: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<SlaOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rtt_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rtt_us: Option<u64>,
    #[serde(default)]
    pub max_rtt_us: u64,
    #[serde(default)]
    pub total_rtt_us: u64,
}

impl Stats {
    /// Records one probe's result.
    pub fn record(&mut self, outcome: SlaOutcome, rtt_us: Option<u64>, at_us: u64) {
        self.sent = self.sent.saturating_add(1);
        self.last_run_us = Some(at_us);
        self.last = Some(outcome);
        self.last_rtt_us = rtt_us;
        match outcome {
            SlaOutcome::Failed => self.failed = self.failed.saturating_add(1),
            SlaOutcome::OverThreshold | SlaOutcome::Ok => {
                self.answered = self.answered.saturating_add(1);
                if outcome == SlaOutcome::OverThreshold {
                    self.over_threshold = self.over_threshold.saturating_add(1);
                }
                if let Some(r) = rtt_us {
                    self.min_rtt_us = Some(self.min_rtt_us.map_or(r, |m| m.min(r)));
                    self.max_rtt_us = self.max_rtt_us.max(r);
                    self.total_rtt_us = self.total_rtt_us.saturating_add(r);
                }
            }
        }
    }

    /// Average round trip, µs.
    pub fn average_rtt_us(&self) -> Option<u64> {
        (self.answered > 0).then(|| self.total_rtt_us / u64::from(self.answered))
    }
}

/// Cuts a text to [`MAX_TEXT`] bytes at a character boundary.
fn clip(text: &mut String) {
    let cut = (0..=MAX_TEXT.min(text.len())).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
    text.truncate(cut);
}

/// A device's IP SLA operations and results.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sla {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub ops: BTreeMap<u32, Op>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stats: BTreeMap<u32, Stats>,
    /// `ip sla responder`: this device answers `udp-echo` and `udp-jitter` probes.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub responder: bool,
}

impl Sla {
    pub fn is_empty(&self) -> bool {
        *self == Sla::default()
    }

    /// Removes an operation with its results and the outcome tracked objects read.
    pub fn remove(&mut self, id: u32) -> bool {
        self.stats.remove(&id);
        self.ops.remove(&id).is_some()
    }

    /// Caps every table (a hostile file).
    pub fn sanitize(&mut self) {
        self.ops.retain(|id, _| (1..=MAX_OP_ID).contains(id));
        while self.ops.len() > MAX_OPS {
            self.ops.pop_last();
        }
        for o in self.ops.values_mut() {
            o.frequency_s = o.frequency_s.clamp(1, 604_800);
            o.timeout_ms = o.timeout_ms.min(604_800_000);
            o.threshold_ms = o.threshold_ms.min(604_800_000);
            clip(&mut o.tag);
            o.reactions.truncate(MAX_REACTIONS);
            for r in &mut o.reactions {
                clip(r);
            }
            if let Some(Probe::HttpGet { path, .. }) = &mut o.probe {
                clip(path);
            }
        }
        let ids: Vec<u32> = self.ops.keys().copied().collect();
        self.stats.retain(|id, _| ids.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_keep_the_extremes_and_the_average() {
        let mut s = Stats::default();
        s.record(SlaOutcome::Ok, Some(2_000), 10);
        s.record(SlaOutcome::OverThreshold, Some(8_000), 20);
        s.record(SlaOutcome::Failed, None, 30);
        assert_eq!((s.sent, s.answered, s.over_threshold, s.failed), (3, 2, 1, 1));
        assert_eq!((s.min_rtt_us, s.max_rtt_us, s.average_rtt_us()), (Some(2_000), 8_000, Some(5_000)));
        assert_eq!((s.last, s.last_rtt_us, s.last_run_us), (Some(SlaOutcome::Failed), None, Some(30)));
    }

    #[test]
    fn sanitize_caps_hostile_tables() {
        let mut sla = Sla::default();
        for id in 0..500u32 {
            sla.ops.insert(id, Op { frequency_s: 0, tag: "x".repeat(10_000), ..Op::default() });
        }
        sla.stats.insert(9_999, Stats::default());
        sla.sanitize();
        assert!(sla.ops.len() <= MAX_OPS && !sla.ops.contains_key(&0) && sla.stats.is_empty());
        assert!(sla.ops.values().all(|o| o.frequency_s == 1 && o.tag.len() == MAX_TEXT));
    }

    #[test]
    fn a_probe_prints_the_line_that_makes_it() {
        let p = Probe::IcmpEcho { dest: Ipv4Addr::new(10, 0, 0, 2), source: Some(Ipv4Addr::new(10, 0, 0, 1)) };
        assert_eq!(p.line(), "icmp-echo 10.0.0.2 source-ip 10.0.0.1");
        assert_eq!(Probe::HttpGet { dest: Ipv4Addr::new(10, 0, 0, 2), path: "/index".into() }.line(), "http get http://10.0.0.2/index");
    }
}
