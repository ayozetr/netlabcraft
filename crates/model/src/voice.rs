//! Voice: IP phones and a router's basic call control.
//!
//! A router with `telephony-service` registers IP phones and connects calls between their
//! directory numbers (`ephone-dn N` with `number X`, given to the phone `ephone N` with that
//! MAC address through `button 1:N`; or handed out with `auto assign`). Phones learn where to
//! register from the DHCP option 150 (`option 150 ip A.B.C.D` in the router's pool). The
//! signalling and the voice packets are in `netlabcraft-sim`'s `voice` module.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::MacAddr;
use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind};

/// Most directory numbers and phones a router keeps.
pub const MAX_DNS: usize = 256;
pub const MAX_EPHONES: usize = 256;
/// The longest directory number.
pub const MAX_NUMBER: usize = 16;
/// The UDP port the call agent listens on (our own signalling, see `netlabcraft-sim`).
pub const SIGNAL_PORT: u16 = 2000;

/// `telephony-service`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelephonyService {
    /// `max-ephones N`: phones that may register (0: none until set, as on real equipment).
    pub max_ephones: u16,
    /// `max-dn N`: directory numbers that may be defined.
    pub max_dn: u16,
    /// `ip source-address A.B.C.D port P`: where phones register (none: any of the router's).
    pub source: Option<Ipv4Addr>,
    pub port: u16,
    /// `auto assign A to B`: directory numbers handed to phones with no `ephone` entry.
    pub auto_assign: Option<(u16, u16)>,
}

impl Default for TelephonyService {
    fn default() -> Self {
        TelephonyService { max_ephones: 0, max_dn: 0, source: None, port: SIGNAL_PORT, auto_assign: None }
    }
}

/// `ephone-dn N`: a directory number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EphoneDn {
    pub tag: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
}

/// `ephone N`: a phone, known by its MAC address, and its line buttons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ephone {
    pub tag: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<MacAddr>,
    /// `button 1:5`: (button, directory number tag).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<(u8, u16)>,
}

/// `voice register global`: SIP call control (RFC 3261) for SIP phones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceRegister {
    /// `mode cme`.
    pub cme: bool,
    /// `source-address A.B.C.D port P`: where phones register (none: any of the router's).
    pub source: Option<Ipv4Addr>,
    pub port: u16,
    /// `max-dn N` and `max-pool N` (0: none until set).
    pub max_dn: u16,
    pub max_pool: u16,
}

impl Default for VoiceRegister {
    fn default() -> Self {
        VoiceRegister { cme: false, source: None, port: SIP_PORT, max_dn: 0, max_pool: 0 }
    }
}

/// `voice register pool N`: a SIP phone, known by its MAC address (`id mac`), and the
/// directory number on its first line (`number 1 dn N`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterPool {
    pub tag: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<MacAddr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dn: Option<u16>,
}

/// The SIP port.
pub const SIP_PORT: u16 = 5060;

/// A device's voice settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Voice {
    /// Routers: `telephony-service`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<TelephonyService>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dns: Vec<EphoneDn>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ephones: Vec<Ephone>,
    /// Routers: `option 150 ip A.B.C.D` per DHCP pool (pool name → TFTP server).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub option150: BTreeMap<String, Ipv4Addr>,
    /// Phones: the TFTP server DHCP gave (where the phone registers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tftp: Option<Ipv4Addr>,
    /// Routers: `voice register global` (SIP call control).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub register: Option<VoiceRegister>,
    /// Routers: `voice register dn N` with its `number`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reg_dns: Vec<EphoneDn>,
    /// Routers: `voice register pool N`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pools: Vec<RegisterPool>,
    /// Phones: signal with SIP instead of NetLabCraft's own call-control protocol.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sip: bool,
    /// Phones: the SIP registrar (none: the TFTP server DHCP option 150 gave).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sip_server: Option<Ipv4Addr>,
}

impl Voice {
    pub fn is_default(&self) -> bool {
        *self == Voice::default()
    }

    pub fn dn(&self, tag: u16) -> Option<&EphoneDn> {
        self.dns.iter().find(|d| d.tag == tag)
    }

    /// The directory number on button 1 of the phone with this MAC address, if configured.
    pub fn number_for(&self, mac: MacAddr) -> Option<(u16, String)> {
        let e = self.ephones.iter().find(|e| e.mac == Some(mac))?;
        let (_, dn) = e.buttons.iter().min_by_key(|(b, _)| *b)?;
        self.dn(*dn).and_then(|d| d.number.clone()).map(|n| (e.tag, n))
    }

    /// The SIP phone with this MAC address: its pool and the number on its first line.
    pub fn sip_number_for(&self, mac: MacAddr) -> Option<(u16, Option<String>)> {
        let pool = self.pools.iter().find(|p| p.mac == Some(mac))?;
        let number = pool.dn.and_then(|t| self.reg_dns.iter().find(|d| d.tag == t)).and_then(|d| d.number.clone());
        Some((pool.tag, number))
    }

    /// Caps lists and drops repeats (hostile files).
    pub fn sanitize(&mut self) {
        self.reg_dns.truncate(MAX_DNS);
        self.reg_dns.sort_by_key(|d| d.tag);
        self.reg_dns.dedup_by_key(|d| d.tag);
        for d in &mut self.reg_dns {
            if d.number.as_ref().is_some_and(|n| !valid_number(n)) {
                d.number = None;
            }
        }
        self.pools.truncate(MAX_EPHONES);
        self.pools.sort_by_key(|p| p.tag);
        self.pools.dedup_by_key(|p| p.tag);
        self.dns.truncate(MAX_DNS);
        self.dns.sort_by_key(|d| d.tag);
        self.dns.dedup_by_key(|d| d.tag);
        for d in &mut self.dns {
            if d.number.as_ref().is_some_and(|n| !valid_number(n)) {
                d.number = None;
            }
        }
        self.ephones.truncate(MAX_EPHONES);
        self.ephones.sort_by_key(|e| e.tag);
        self.ephones.dedup_by_key(|e| e.tag);
        for e in &mut self.ephones {
            e.buttons.truncate(8);
        }
        while self.option150.len() > crate::MAX_DHCP_POOLS {
            self.option150.pop_last();
        }
    }
}

/// A directory number: 1 to 16 digits.
pub fn valid_number(n: &str) -> bool {
    !n.is_empty() && n.len() <= MAX_NUMBER && n.bytes().all(|b| b.is_ascii_digit())
}

/// Whether a device is an IP phone.
pub fn is_phone(d: &Device) -> bool {
    d.kind == DeviceKind::Phone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_the_first_button_and_hostile_lists_are_trimmed() {
        let mac = MacAddr([2, 0, 0, 0, 0, 9]);
        let mut v = Voice {
            dns: vec![EphoneDn { tag: 1, number: Some("1001".into()) }, EphoneDn { tag: 2, number: Some("12x".into()) }],
            ephones: vec![Ephone { tag: 1, mac: Some(mac), buttons: vec![(2, 2), (1, 1)] }],
            ..Voice::default()
        };
        assert_eq!(v.number_for(mac), Some((1, "1001".into())));
        assert_eq!(v.number_for(MacAddr([2, 0, 0, 0, 0, 1])), None);
        v.ephones.push(v.ephones[0].clone());
        v.sanitize();
        assert_eq!(v.ephones.len(), 1);
        assert_eq!(v.dn(2).unwrap().number, None);
        assert!(valid_number("2001") && !valid_number("") && !valid_number("12345678901234567"));
    }
}
