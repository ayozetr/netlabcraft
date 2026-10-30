//! Simulation scenarios: each holds a list of user PDUs (simple ones, a single echo request,
//! and complex ones built field by field), saved with the lab so a network can be re-tested.

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

/// Most scenarios a lab keeps.
pub const MAX_SCENARIOS: usize = 32;
/// Most PDUs in one scenario.
pub const MAX_PDUS: usize = 200;
/// Longest text a PDU field holds (a name to look up, a file to fetch).
pub const MAX_PDU_TEXT: usize = 120;
/// Largest payload a PDU asks for (bytes).
pub const MAX_PDU_SIZE: u16 = 1_400;
/// Shortest and longest period of a periodic PDU (seconds).
pub const MIN_PERIOD_S: f64 = 0.1;
pub const MAX_PERIOD_S: f64 = 86_400.0;

/// What a PDU carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PduApp {
    #[default]
    Ping,
    Dns,
    Http,
    Telnet,
    Ssh,
    Ftp,
    Smtp,
    Pop3,
    Dhcp,
    Tftp,
    Snmp,
    Arp,
    Udp,
    Tcp,
}

impl PduApp {
    pub const ALL: [PduApp; 14] = [
        PduApp::Ping,
        PduApp::Dns,
        PduApp::Http,
        PduApp::Telnet,
        PduApp::Ssh,
        PduApp::Ftp,
        PduApp::Smtp,
        PduApp::Pop3,
        PduApp::Dhcp,
        PduApp::Tftp,
        PduApp::Snmp,
        PduApp::Arp,
        PduApp::Udp,
        PduApp::Tcp,
    ];

    /// The id used in commands and files.
    pub fn id(self) -> &'static str {
        match self {
            PduApp::Ping => "ping",
            PduApp::Dns => "dns",
            PduApp::Http => "http",
            PduApp::Telnet => "telnet",
            PduApp::Ssh => "ssh",
            PduApp::Ftp => "ftp",
            PduApp::Smtp => "smtp",
            PduApp::Pop3 => "pop3",
            PduApp::Dhcp => "dhcp",
            PduApp::Tftp => "tftp",
            PduApp::Snmp => "snmp",
            PduApp::Arp => "arp",
            PduApp::Udp => "udp",
            PduApp::Tcp => "tcp",
        }
    }

    pub fn parse(s: &str) -> Option<PduApp> {
        let s = s.trim().to_ascii_lowercase();
        let s = match s.as_str() {
            "icmp" | "echo" => "ping",
            "dns query" | "dns-query" => "dns",
            "http get" | "http-get" | "web" => "http",
            "snmp get" | "snmp-get" => "snmp",
            "pop" => "pop3",
            other => other,
        };
        PduApp::ALL.into_iter().find(|a| a.id() == s)
    }

    /// The protocol name the PDU list shows.
    pub fn type_name(self) -> &'static str {
        match self {
            PduApp::Ping => "ICMP",
            PduApp::Dns => "DNS",
            PduApp::Http => "HTTP",
            PduApp::Telnet => "Telnet",
            PduApp::Ssh => "SSH",
            PduApp::Ftp => "FTP",
            PduApp::Smtp => "SMTP",
            PduApp::Pop3 => "POP3",
            PduApp::Dhcp => "DHCP",
            PduApp::Tftp => "TFTP",
            PduApp::Snmp => "SNMP",
            PduApp::Arp => "ARP",
            PduApp::Udp => "UDP",
            PduApp::Tcp => "TCP",
        }
    }

    /// Its usual destination port (none: not carried over UDP or TCP).
    pub fn default_port(self) -> Option<u16> {
        match self {
            PduApp::Dns => Some(53),
            PduApp::Http => Some(80),
            PduApp::Telnet => Some(23),
            PduApp::Ssh => Some(22),
            PduApp::Ftp => Some(21),
            PduApp::Smtp => Some(25),
            PduApp::Pop3 => Some(110),
            PduApp::Dhcp => Some(67),
            PduApp::Tftp => Some(69),
            PduApp::Snmp => Some(161),
            PduApp::Udp | PduApp::Tcp => Some(1_024),
            PduApp::Ping | PduApp::Arp => None,
        }
    }

    /// Carried over TCP.
    pub fn is_tcp(self) -> bool {
        matches!(self, PduApp::Http | PduApp::Telnet | PduApp::Ssh | PduApp::Ftp | PduApp::Smtp | PduApp::Pop3 | PduApp::Tcp)
    }

    /// Needs no destination address (sent to everyone on the link).
    pub fn broadcast(self) -> bool {
        self == PduApp::Dhcp
    }
}

/// How a PDU did the last time it was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PduStatus {
    /// Not sent yet (scheduled for later).
    #[default]
    Waiting,
    Successful,
    Failed,
}

impl PduStatus {
    pub fn id(self) -> &'static str {
        match self {
            PduStatus::Waiting => "waiting",
            PduStatus::Successful => "successful",
            PduStatus::Failed => "failed",
        }
    }
}

/// One PDU of a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPdu {
    /// Its number in the list (unique in the scenario).
    pub num: u32,
    pub source: DeviceId,
    /// The source interface (none: the one the route picks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// The destination address (empty for a broadcast, DHCP).
    #[serde(default)]
    pub destination: String,
    /// The device that owns the destination address, when it is one of the lab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<DeviceId>,
    #[serde(default)]
    pub app: PduApp,
    /// A simple PDU (the envelope tool): one echo request with the defaults.
    #[serde(default)]
    pub simple: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_port: Option<u16>,
    /// None: the device's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u8>,
    /// The IPv4 type-of-service byte (DSCP in the top six bits).
    #[serde(default)]
    pub tos: u8,
    /// The ICMP sequence number or TCP initial sequence number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u32>,
    /// Payload bytes.
    #[serde(default)]
    pub size: u16,
    /// The name a DNS query asks for, the file TFTP reads, the page HTTP gets.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Sent again every so many seconds (none: once).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_s: Option<f64>,
    /// Seconds after it is created (or the scenario's PDUs are rescheduled) that it is sent first.
    #[serde(default)]
    pub start_s: f64,
    #[serde(default)]
    pub status: PduStatus,
    /// Simulated time it was last sent (seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_s: Option<f64>,
    /// Times it was sent.
    #[serde(default)]
    pub fired: u32,
    /// What happened the last time (for people: "reply from 10.0.2.10").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// When it is due next (simulated µs). Not saved: a reopened lab starts its clock again.
    #[serde(skip)]
    pub next_us: Option<u64>,
}

impl UserPdu {
    /// The list's Type column.
    pub fn type_name(&self) -> &'static str {
        self.app.type_name()
    }

    fn sanitize(&mut self) {
        self.text = self.text.chars().take(MAX_PDU_TEXT).collect();
        self.destination = self.destination.chars().take(64).collect();
        self.port = self.port.take().map(|p| p.chars().take(64).collect());
        self.note = self.note.chars().take(MAX_PDU_TEXT).collect();
        self.size = self.size.min(MAX_PDU_SIZE);
        self.ttl = self.ttl.map(|t| t.max(1));
        self.period_s = self.period_s.filter(|p| p.is_finite()).map(|p| p.clamp(MIN_PERIOD_S, MAX_PERIOD_S));
        self.start_s = if self.start_s.is_finite() { self.start_s.clamp(0.0, MAX_PERIOD_S) } else { 0.0 };
        self.last_s = self.last_s.filter(|v| v.is_finite());
    }
}

/// One scenario: a named list of PDUs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scenario {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub pdus: Vec<UserPdu>,
}

/// The lab's scenarios and the one in use. An empty list stands for one empty scenario.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scenarios {
    #[serde(default)]
    pub list: Vec<Scenario>,
    #[serde(default)]
    pub current: usize,
}

impl Scenarios {
    pub fn is_default(&self) -> bool {
        self.current == 0 && self.list.iter().all(|s| s.pdus.is_empty() && s.description.is_empty()) && self.list.len() <= 1
    }

    /// How many scenarios there are (at least one).
    pub fn count(&self) -> usize {
        self.list.len().max(1)
    }

    /// The scenario in use (made on first use).
    pub fn current_mut(&mut self) -> &mut Scenario {
        if self.list.is_empty() {
            self.list.push(Scenario::default());
        }
        if self.current >= self.list.len() {
            self.current = 0;
        }
        let i = self.current;
        // In range: the list is never empty here and `current` was just checked.
        &mut self.list[i]
    }

    /// The PDUs of the scenario in use.
    pub fn pdus(&self) -> &[UserPdu] {
        self.list.get(self.current).map_or(&[], |s| s.pdus.as_slice())
    }

    /// The same, to change in place (nothing is made when there is no scenario yet).
    pub fn pdus_mut(&mut self) -> &mut [UserPdu] {
        self.list.get_mut(self.current).map_or(&mut [], |s| s.pdus.as_mut_slice())
    }

    pub fn sanitize(&mut self, devices: &[DeviceId]) {
        self.list.truncate(MAX_SCENARIOS);
        if self.current >= self.count() {
            self.current = 0;
        }
        for s in &mut self.list {
            s.description = s.description.chars().take(MAX_PDU_TEXT).collect();
            s.pdus.truncate(MAX_PDUS);
            s.pdus.retain(|p| devices.contains(&p.source));
            let mut seen = std::collections::BTreeSet::new();
            s.pdus.retain(|p| seen.insert(p.num));
            for p in &mut s.pdus {
                p.sanitize();
                if p.target.is_some_and(|t| !devices.contains(&t)) {
                    p.target = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_round_trip_by_id() {
        for a in PduApp::ALL {
            assert_eq!(PduApp::parse(a.id()), Some(a));
        }
        assert_eq!(PduApp::parse("HTTP GET"), Some(PduApp::Http));
        assert_eq!(PduApp::parse("bogus"), None);
    }

    #[test]
    fn sanitize_repairs_hostile_scenarios() {
        let pdu = UserPdu {
            num: 1,
            source: DeviceId(1),
            port: None,
            destination: "x".repeat(500),
            target: Some(DeviceId(99)),
            app: PduApp::Ping,
            simple: false,
            src_port: None,
            dst_port: None,
            ttl: Some(0),
            tos: 0,
            seq: None,
            size: u16::MAX,
            text: String::new(),
            period_s: Some(f64::NAN),
            start_s: f64::INFINITY,
            status: PduStatus::Waiting,
            last_s: None,
            fired: 0,
            note: String::new(),
            next_us: None,
        };
        let mut s = Scenarios { list: vec![Scenario { description: String::new(), pdus: vec![pdu.clone(), pdu] }], current: 7 };
        s.sanitize(&[DeviceId(1)]);
        assert_eq!(s.current, 0);
        let p = &s.list[0].pdus;
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].ttl, Some(1));
        assert_eq!(p[0].size, MAX_PDU_SIZE);
        assert_eq!(p[0].period_s, None);
        assert_eq!(p[0].start_s, 0.0);
        assert_eq!(p[0].target, None);
        assert_eq!(p[0].destination.len(), 64);
        let mut empty = Scenarios::default();
        assert!(empty.is_default());
        empty.current_mut().pdus.clear();
        assert_eq!(empty.count(), 1);
    }
}
