//! CAPWAP (RFC 5415) with its IEEE 802.11 binding (RFC 5416): how a lightweight access point
//! finds, joins and is configured by its wireless LAN controller.
//!
//! Control messages go to UDP 5246 and data to UDP 5247, each after the 8-byte CAPWAP header
//! (preamble, HLEN, RID, WBID = 1 for IEEE 802.11, the T/F/L/W/M/K flags, fragment fields).
//! A control message is a header (message type, sequence number, element length, flags) and
//! type-length-value message elements. The control channel's DTLS protection is not
//! modelled: messages are sent in the clear.

use crate::wlan::{Akm, rsn_ie};
use crate::{Result, WireError, be16};

pub const CONTROL_PORT: u16 = 5246;
pub const DATA_PORT: u16 = 5247;

/// Message types (RFC 5415 §4.5.1.1, RFC 5416 §3).
pub const DISCOVERY_REQUEST: u32 = 1;
pub const DISCOVERY_RESPONSE: u32 = 2;
pub const JOIN_REQUEST: u32 = 3;
pub const JOIN_RESPONSE: u32 = 4;
pub const CONFIG_STATUS_REQUEST: u32 = 5;
pub const CONFIG_STATUS_RESPONSE: u32 = 6;
pub const ECHO_REQUEST: u32 = 13;
pub const ECHO_RESPONSE: u32 = 14;
pub const WLAN_CONFIG_REQUEST: u32 = 3_398_913;
pub const WLAN_CONFIG_RESPONSE: u32 = 3_398_914;

/// Message element types (RFC 5415 §4.6, RFC 5416 §6).
pub const AC_NAME: u16 = 4;
pub const RESULT_CODE: u16 = 33;
pub const WTP_NAME: u16 = 45;
pub const ADD_WLAN: u16 = 1024;
pub const DELETE_WLAN: u16 = 1027;
pub const IE_80211: u16 = 1029;

/// Result codes (RFC 5415 §4.6.35).
pub const RESULT_SUCCESS: u32 = 0;
pub const RESULT_JOIN_REFUSED: u32 = 6;

/// The most message elements one message may carry here.
const MAX_ELEMENTS: usize = 64;
const WBID_80211: u32 = 1;

fn header(native_80211: bool) -> [u8; 8] {
    // Preamble 0 (version 0, type 0), HLEN 2 words, RID 0, WBID 1, T flag, no fragments.
    let w = (2u32 << 19) | (WBID_80211 << 9) | (u32::from(native_80211) << 8);
    let b = w.to_be_bytes();
    [b[0], b[1], b[2], b[3], 0, 0, 0, 0]
}

/// The payload after the CAPWAP header, and whether the T flag (native 802.11 frame) is set.
fn strip(b: &[u8]) -> Result<(&[u8], bool)> {
    let t = || WireError::Truncated("capwap");
    let w = u32::from_be_bytes(b.get(..4).ok_or_else(t)?.try_into().map_err(|_| t())?);
    if w >> 28 != 0 {
        return Err(WireError::Unsupported("capwap", format!("version {}", w >> 28)));
    }
    if (w >> 24) & 0x0f != 0 {
        return Err(WireError::Unsupported("capwap", "DTLS-protected".into()));
    }
    let hlen = ((w >> 19) & 0x1f) as usize * 4;
    if hlen < 8 {
        return Err(WireError::Unsupported("capwap", format!("header length {hlen}")));
    }
    Ok((b.get(hlen..).ok_or_else(t)?, (w >> 8) & 1 == 1))
}

/// A control message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub kind: u32,
    pub seq: u8,
    pub elements: Vec<(u16, Vec<u8>)>,
}

impl Control {
    pub fn new(kind: u32, seq: u8) -> Control {
        Control { kind, seq, elements: Vec::new() }
    }

    pub fn with(mut self, kind: u16, value: Vec<u8>) -> Control {
        if self.elements.len() < MAX_ELEMENTS {
            self.elements.push((kind, value));
        }
        self
    }

    pub fn element(&self, kind: u16) -> Option<&[u8]> {
        self.elements.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.as_slice())
    }

    pub fn text(&self, kind: u16) -> Option<String> {
        self.element(kind).map(|v| String::from_utf8_lossy(v).into_owned())
    }

    pub fn result(&self) -> Option<u32> {
        let v = self.element(RESULT_CODE)?;
        Some(u32::from_be_bytes(v.get(..4)?.try_into().ok()?))
    }

    /// The WLANs an IEEE 802.11 WLAN Configuration Request adds.
    pub fn wlans(&self) -> Vec<AddWlan> {
        self.elements.iter().filter(|(k, _)| *k == ADD_WLAN).filter_map(|(_, v)| AddWlan::decode(v).ok()).collect()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut els = Vec::new();
        for (k, v) in &self.elements {
            let v = v.get(..v.len().min(1024)).unwrap_or_default();
            els.extend_from_slice(&k.to_be_bytes());
            els.extend_from_slice(&(v.len() as u16).to_be_bytes());
            els.extend_from_slice(v);
        }
        let mut out = header(false).to_vec();
        out.extend_from_slice(&self.kind.to_be_bytes());
        out.push(self.seq);
        // The length counts the flags byte and the elements.
        out.extend_from_slice(&u16::try_from(els.len() + 1).unwrap_or(u16::MAX).to_be_bytes());
        out.push(0);
        out.extend_from_slice(&els);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Control> {
        let t = || WireError::Truncated("capwap");
        let (b, _) = strip(b)?;
        let kind = u32::from_be_bytes(b.get(..4).ok_or_else(t)?.try_into().map_err(|_| t())?);
        let seq = *b.get(4).ok_or_else(t)?;
        let len = usize::from(be16(b, 5).ok_or_else(t)?);
        let mut rest = b.get(8..(7 + len).max(8)).ok_or_else(t)?;
        let mut elements = Vec::new();
        while !rest.is_empty() && elements.len() < MAX_ELEMENTS {
            let k = be16(rest, 0).ok_or_else(t)?;
            let n = usize::from(be16(rest, 2).ok_or_else(t)?);
            elements.push((k, rest.get(4..4 + n).ok_or_else(t)?.to_vec()));
            rest = rest.get(4 + n..).unwrap_or_default();
        }
        Ok(Control { kind, seq, elements })
    }

    pub fn name(&self) -> String {
        match self.kind {
            DISCOVERY_REQUEST => "discovery request".into(),
            DISCOVERY_RESPONSE => "discovery response".into(),
            JOIN_REQUEST => "join request".into(),
            JOIN_RESPONSE => "join response".into(),
            CONFIG_STATUS_REQUEST => "configuration status request".into(),
            CONFIG_STATUS_RESPONSE => "configuration status response".into(),
            ECHO_REQUEST => "echo request".into(),
            ECHO_RESPONSE => "echo response".into(),
            WLAN_CONFIG_REQUEST => "802.11 WLAN configuration request".into(),
            WLAN_CONFIG_RESPONSE => "802.11 WLAN configuration response".into(),
            k => format!("message type {k}"),
        }
    }

    /// One line for the event list.
    pub fn describe(&self) -> String {
        let mut s = format!("CAPWAP {}", self.name());
        if let Some(n) = self.text(WTP_NAME) {
            s.push_str(&format!(" from AP \"{n}\""));
        }
        if let Some(n) = self.text(AC_NAME) {
            s.push_str(&format!(" from controller \"{n}\""));
        }
        match self.result() {
            Some(RESULT_SUCCESS) => s.push_str(": success"),
            Some(r) => s.push_str(&format!(": refused (result {r})")),
            None => {}
        }
        let wlans = self.wlans();
        if !wlans.is_empty() {
            let list: Vec<String> = wlans
                .iter()
                .map(|w| format!("{} \"{}\" ({}{})", w.wlan_id, w.ssid, w.security_name(), if w.central { ", central switching" } else { "" }))
                .collect();
            s.push_str(&format!(": add WLAN {}", list.join(", ")));
        }
        let deleted = self.elements.iter().filter(|(k, _)| *k == DELETE_WLAN).count();
        if deleted > 0 {
            s.push_str(&format!(": delete {deleted} WLAN(s)"));
        }
        s
    }
}

/// The IEEE 802.11 Add WLAN message element (RFC 5416 §6.1), plus the RSN element the network
/// uses (carried in the same element's key fields here: the AKM in `akm`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddWlan {
    pub radio: u8,
    pub wlan_id: u8,
    /// The pairwise master key of a WPA2-Personal network (empty otherwise).
    pub key: Vec<u8>,
    pub akm: Option<Akm>,
    pub ssid: String,
    /// Tunnel mode 802.3 (central switching) rather than local bridging.
    pub central: bool,
}

/// Capability bits of Add WLAN, most significant first: E(SS), I(BSS), C(F-pollable),
/// F(CF-poll request), P(rivacy)…
const WLAN_CAP_ESS: u16 = 0x8000;
const WLAN_CAP_PRIVACY: u16 = 0x0800;

impl AddWlan {
    pub fn security_name(&self) -> &'static str {
        self.akm.map_or("open", Akm::name)
    }

    pub fn encode(&self) -> Vec<u8> {
        let key = self.key.get(..self.key.len().min(64)).unwrap_or_default();
        let ssid = self.ssid.as_bytes().get(..self.ssid.len().min(32)).unwrap_or_default();
        let mut b = vec![self.radio, self.wlan_id];
        let cap = WLAN_CAP_ESS | if self.akm.is_some() { WLAN_CAP_PRIVACY } else { 0 };
        b.extend_from_slice(&cap.to_be_bytes());
        b.push(0); // key index
        b.push(u8::from(!key.is_empty())); // key status: 1 = per-station keys from this key
        b.extend_from_slice(&(key.len() as u16).to_be_bytes());
        b.extend_from_slice(key);
        b.extend_from_slice(&[0; 6]); // group TSC
        b.push(0); // QoS: best effort
        b.push(0); // auth type: open system
        b.push(1); // MAC mode: split MAC
        b.push(u8::from(self.central)); // tunnel mode: 0 local bridging, 1 802.3 tunnel
        b.push(0); // SSID broadcast in beacons
        b.extend_from_slice(ssid);
        // The RSN element follows the SSID, after a zero separator (an extension of ours, so one
        // element says everything the AP needs).
        if let Some(a) = self.akm {
            b.push(0);
            b.extend_from_slice(&rsn_ie(a));
        }
        b
    }

    pub fn decode(b: &[u8]) -> Result<AddWlan> {
        let t = || WireError::Truncated("capwap add wlan");
        let (radio, wlan_id) = (*b.first().ok_or_else(t)?, *b.get(1).ok_or_else(t)?);
        let klen = usize::from(be16(b, 6).ok_or_else(t)?);
        let key = b.get(8..8 + klen).ok_or_else(t)?.to_vec();
        let central = *b.get(8 + klen + 9).ok_or_else(t)? == 1;
        let rest = b.get(8 + klen + 11..).ok_or_else(t)?;
        let (ssid, rsn) = match rest.iter().position(|&x| x == 0) {
            Some(i) => (rest.get(..i).unwrap_or_default(), rest.get(i + 1..).unwrap_or_default()),
            None => (rest, &[][..]),
        };
        let akm = if rsn.first() == Some(&48) { crate::wlan::rsn_akm(rsn.get(2..).unwrap_or_default()) } else { None };
        Ok(AddWlan { radio, wlan_id, key, akm, ssid: String::from_utf8_lossy(ssid).into_owned(), central })
    }
}

/// A CAPWAP data message carrying one native IEEE 802.11 frame (T flag set).
pub fn data_encode(frame: &[u8]) -> Vec<u8> {
    let mut out = header(true).to_vec();
    out.extend_from_slice(frame);
    out
}

/// The 802.11 frame a CAPWAP data message carries.
/// CAPWAP data carrying an IEEE 802.3 frame (T bit clear): a host's traffic tunnelled between
/// an access point and its controller (central switching).
pub fn data_encode_8023(frame: &[u8]) -> Vec<u8> {
    let mut out = header(false).to_vec();
    out.extend_from_slice(frame);
    out
}

/// The frame CAPWAP data carries, and whether it is a native IEEE 802.11 frame (else 802.3).
pub fn data_frame(b: &[u8]) -> Result<(Vec<u8>, bool)> {
    let (rest, native) = strip(b)?;
    Ok((rest.to_vec(), native))
}

pub fn data_decode(b: &[u8]) -> Result<Vec<u8>> {
    let (rest, native) = strip(b)?;
    if !native {
        return Err(WireError::Unsupported("capwap", "data that is not a native 802.11 frame".into()));
    }
    Ok(rest.to_vec())
}

/// One line for the event list: control or data.
pub fn describe(port: u16, b: &[u8]) -> String {
    if port == DATA_PORT {
        return match data_frame(b) {
            Ok((f, true)) => format!("CAPWAP data carrying {}", crate::wlan::summarize(&f)),
            Ok((f, false)) => format!("CAPWAP data (central switching) carrying {}", crate::summarize(&f).1),
            Err(e) => format!("CAPWAP data ({e})"),
        };
    }
    match Control::decode(b) {
        Ok(c) => c.describe(),
        Err(e) => format!("CAPWAP ({e})"),
    }
}

/// A 4-byte result code element.
pub fn result_code(code: u32) -> Vec<u8> {
    code.to_be_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Control> {
        let w = AddWlan { radio: 1, wlan_id: 2, key: vec![5; 32], akm: Some(Akm::Psk), ssid: "Staff".into(), central: true };
        vec![
            Control::new(DISCOVERY_REQUEST, 1).with(WTP_NAME, b"LAP1".to_vec()),
            Control::new(JOIN_RESPONSE, 2).with(AC_NAME, b"WLC1".to_vec()).with(RESULT_CODE, result_code(RESULT_SUCCESS)),
            Control::new(WLAN_CONFIG_REQUEST, 3)
                .with(ADD_WLAN, w.encode())
                .with(ADD_WLAN, AddWlan { radio: 1, wlan_id: 3, key: vec![], akm: Some(Akm::Dot1x), ssid: "Corp".into(), central: false }.encode())
                .with(ADD_WLAN, AddWlan { radio: 1, wlan_id: 4, key: vec![], akm: None, ssid: "Guest".into(), central: false }.encode()),
        ]
    }

    #[test]
    fn control_round_trips() {
        for c in samples() {
            let b = c.encode();
            assert_eq!(&b[..4], &[0x00, 0x10, 0x02, 0x00], "HLEN 2, WBID 1");
            assert_eq!(Control::decode(&b), Ok(c.clone()));
        }
        let wl = samples()[2].wlans();
        assert_eq!(
            wl.iter().map(|w| (w.ssid.as_str(), w.akm)).collect::<Vec<_>>(),
            vec![("Staff", Some(Akm::Psk)), ("Corp", Some(Akm::Dot1x)), ("Guest", None)]
        );
        assert_eq!(wl[0].key, vec![5; 32]);
        assert!(
            samples()[2].describe().contains("2 \"Staff\" (WPA2-PSK, central switching), 3 \"Corp\" (WPA2-Enterprise)"),
            "{}",
            samples()[2].describe()
        );
        assert_eq!(wl.iter().map(|w| w.central).collect::<Vec<_>>(), vec![true, false, false]);
        assert_eq!(samples()[1].describe(), "CAPWAP join response from controller \"WLC1\": success");
        let d = data_encode(&[0x08, 0x01, 0, 0]);
        assert_eq!(data_decode(&d), Ok(vec![0x08, 0x01, 0, 0]));
        assert!(data_decode(&samples()[0].encode()).is_err());
        // Central switching: an 802.3 frame, the T bit clear.
        let eth = crate::Frame {
            dst: crate::MacAddr::BROADCAST,
            src: crate::MacAddr([2, 0, 0, 0, 0, 9]),
            vlan: Some(10),
            ethertype: 0x0806,
            payload: vec![0; 28],
        }
        .encode();
        let d = data_encode_8023(&eth);
        assert_eq!(d[2] & 1, 0, "T bit clear");
        assert_eq!(data_frame(&d), Ok((eth.clone(), false)));
        assert!(data_decode(&d).is_err());
        assert!(describe(DATA_PORT, &d).starts_with("CAPWAP data (central switching) carrying ARP"), "{}", describe(DATA_PORT, &d));
        for n in 0..d.len() {
            let _ = data_frame(&d[..n]);
            let _ = describe(DATA_PORT, &d[..n]);
        }
    }

    #[test]
    fn decoders_survive_every_truncation() {
        for c in samples() {
            let b = c.encode();
            for n in 0..b.len() {
                let _ = Control::decode(&b[..n]);
                let _ = describe(CONTROL_PORT, &b[..n]);
                let _ = describe(DATA_PORT, &b[..n]);
                let _ = AddWlan::decode(b.get(12..n).unwrap_or_default());
            }
        }
    }
}
