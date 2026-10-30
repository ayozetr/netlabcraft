//! Network-management settings of routers and switches: log timestamps and severity filters,
//! NTP master and authentication, the boot image and the configuration register; and what a
//! host learned from DHCP beyond its address.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// Most NTP keys, boot lines and flash files kept.
pub const MAX_NTP_KEYS: usize = 16;
pub const MAX_BOOT: usize = 4;
pub const MAX_FLASH_FILES: usize = 16;
/// The configuration register's usual value, and the one that skips the startup-config.
pub const CONFREG_NORMAL: u16 = 0x2102;
pub const CONFREG_SKIP_STARTUP: u16 = 0x2142;

/// Syslog severity names, 0 (emergencies) to 7 (debugging).
pub const SEVERITIES: [&str; 8] = ["emergencies", "alerts", "critical", "errors", "warnings", "notifications", "informational", "debugging"];

/// A severity by name (`warnings`) or number (`4`).
pub fn severity_from(w: &str) -> Option<u8> {
    let lw = w.to_ascii_lowercase();
    if let Ok(n) = lw.parse::<u8>() {
        return (n <= 7).then_some(n);
    }
    let hits: Vec<usize> = SEVERITIES.iter().enumerate().filter(|(_, s)| s.starts_with(&lw)).map(|(i, _)| i).collect();
    match hits.as_slice() {
        [i] if !lw.is_empty() => u8::try_from(*i).ok(),
        _ => None,
    }
}

/// The name of a severity.
pub fn severity_name(n: u8) -> &'static str {
    SEVERITIES.get(usize::from(n)).copied().unwrap_or("debugging")
}

/// `service timestamps log|debug datetime [msec]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timestamps {
    #[serde(default)]
    pub msec: bool,
}

/// An NTP authentication key (`ntp authentication-key N md5 KEY`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NtpKey {
    pub id: u32,
    pub key: String,
}

/// The management configuration (part of the running configuration).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Mgmt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamps_log: Option<Timestamps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamps_debug: Option<Timestamps>,
    /// `logging trap LEVEL`: the most detailed severity sent to syslog servers (default 6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging_trap: Option<u8>,
    /// `logging console LEVEL` (default 7: everything).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging_console: Option<u8>,
    /// `logging buffered [SIZE] [LEVEL]` (default 7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging_buffered: Option<u8>,
    /// `ntp master [STRATUM]`: this device answers NTP from its own clock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ntp_master: Option<u8>,
    /// `ntp authenticate`: only take time from servers that sign with a trusted key.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ntp_authenticate: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ntp_keys: Vec<NtpKey>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ntp_trusted: Vec<u32>,
    /// `ntp server A key N`: the key a server's answers must carry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ntp_server_keys: Vec<(Ipv4Addr, u32)>,
    /// `ntp update-calendar`: NTP also sets the hardware calendar.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ntp_update_calendar: bool,
    /// `boot system flash:NAME` lines, tried in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub boot_system: Vec<String>,
    /// `history size N` on the console and vty lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_size: Option<u16>,
    /// Interfaces with `standby version 2` (by name: `GigabitEthernet0/0`, `…0/0.10`,
    /// `Vlan10`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hsrp_v2: Vec<String>,
}

impl Mgmt {
    /// The key configured for an NTP server, with its secret.
    pub fn ntp_key_for(&self, server: Ipv4Addr) -> Option<&NtpKey> {
        let id = self.ntp_server_keys.iter().find(|(a, _)| *a == server)?.1;
        self.ntp_keys.iter().find(|k| k.id == id)
    }
}

/// An image file in flash beyond the one the device ships with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlashFile {
    pub name: String,
    pub size: u64,
}

/// What lives outside the configuration and survives a reload: the configuration register
/// and files copied into flash.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Nvram {
    /// `config-register 0xNNNN` (`None`: 0x2102).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_register: Option<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flash: Vec<FlashFile>,
}

impl Nvram {
    pub fn register(&self) -> u16 {
        self.config_register.unwrap_or(CONFREG_NORMAL)
    }
}

/// What a host learned from DHCP besides address, gateway and DNS server.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HostLease {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Lease time in seconds (`u32::MAX`: infinite).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_secs: Option<u32>,
    /// The server that gave the lease.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Ipv4Addr>,
}

/// A lease time as people read it: `1 day 2 hours`, `infinite`.
pub fn lease_text(secs: u32) -> String {
    if secs == u32::MAX {
        return "infinite".into();
    }
    let (d, h, m) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60);
    let mut parts = Vec::new();
    for (n, unit) in [(d, "day"), (h, "hour"), (m, "minute")] {
        if n > 0 {
            parts.push(format!("{n} {unit}{}", if n == 1 { "" } else { "s" }));
        }
    }
    if parts.is_empty() { format!("{secs} seconds") } else { parts.join(" ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severities_and_lease_text() {
        assert_eq!(severity_from("warn"), Some(4));
        assert_eq!(severity_from("7"), Some(7));
        assert_eq!(severity_from("8"), None);
        assert_eq!(severity_from("e"), None, "emergencies or errors");
        assert_eq!(severity_from(""), None);
        assert_eq!(severity_name(3), "errors");
        assert_eq!(lease_text(86_400 + 7_200), "1 day 2 hours");
        assert_eq!(lease_text(u32::MAX), "infinite");
        assert_eq!(lease_text(30), "30 seconds");
    }
}
