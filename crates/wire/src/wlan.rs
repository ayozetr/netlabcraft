//! IEEE 802.11 frames as they cross the air between a Wi-Fi host and its access point, and the
//! IEEE 802.1X EAPOL frames (EAP, and the RSN 4-way handshake's EAPOL-Key) they carry.
//!
//! Formats follow IEEE 802.11-2020 §9 (frame control, duration, three addresses, sequence
//! control; management bodies with their information elements; data frames carrying an
//! LLC/SNAP header then the EtherType and payload), IEEE 802.1X-2020 §11 (EAPOL), RFC 3748
//! (EAP) and IEEE 802.11 §12.7.2 (EAPOL-Key). The frame check sequence is not modelled, as for
//! Ethernet. On an RSN network, data frames after the 4-way handshake are protected with CCMP
//! (IEEE 802.11 §12.5.3: AES-CCM with an 8-byte MIC, keyed by the pairwise temporal key).

use netlabcraft_core::MacAddr;

use crate::{Frame, Result, WireError, be16, mac_at};

/// The EtherType of EAPOL (IEEE 802.1X).
pub const ETHERTYPE_EAPOL: u16 = 0x888e;

const TYPE_MGMT: u8 = 0;
const TYPE_DATA: u8 = 2;
const ST_ASSOC_REQ: u8 = 0;
const ST_ASSOC_RESP: u8 = 1;
const ST_REASSOC_REQ: u8 = 2;
const ST_REASSOC_RESP: u8 = 3;
const ST_PROBE_REQ: u8 = 4;
const ST_PROBE_RESP: u8 = 5;
const ST_BEACON: u8 = 8;
const ST_DISASSOC: u8 = 10;
const ST_AUTH: u8 = 11;
const ST_DEAUTH: u8 = 12;

const FLAG_TO_DS: u8 = 0x01;
const FLAG_FROM_DS: u8 = 0x02;
/// The Protected Frame bit: the body is encrypted (CCMP here).
pub const FLAG_PROTECTED: u8 = 0x40;
const FLAG_RETRY: u8 = 0x08;
const FLAG_PWR_MGT: u8 = 0x10;
const FLAG_MORE_DATA: u8 = 0x20;
/// The CCMP header: PN0, PN1, reserved, key id with Ext IV, PN2-PN5.
pub const CCMP_HEADER_LEN: usize = 8;
/// The CCMP message integrity code.
pub const CCMP_MIC_LEN: usize = 8;
const EXT_IV: u8 = 0x20;
/// The largest packet number (48 bits).
pub const MAX_PN: u64 = (1 << 48) - 1;

/// Capability information bits.
const CAP_ESS: u16 = 0x0001;
const CAP_PRIVACY: u16 = 0x0010;
const CAP_SHORT_PREAMBLE: u16 = 0x0020;

const IE_SSID: u8 = 0;
const IE_RATES: u8 = 1;
const IE_DS: u8 = 3;
const IE_RSN: u8 = 48;
/// 1, 2, 5.5 and 11 Mbit/s (basic) and 6, 9, 12, 18 Mbit/s, in 500 kbit/s units.
const RATES: [u8; 8] = [0x82, 0x84, 0x8b, 0x96, 0x0c, 0x12, 0x18, 0x24];
/// The LLC/SNAP header that precedes the EtherType in a data frame (RFC 1042).
const SNAP: [u8; 6] = [0xaa, 0xaa, 0x03, 0, 0, 0];
/// The IEEE 802.11 cipher and AKM suite OUI.
const OUI: [u8; 3] = [0x00, 0x0f, 0xac];
const CIPHER_CCMP: u8 = 4;

/// Status codes (IEEE 802.11 Table 9-50) and reason codes (Table 9-49) the simulator uses.
pub const STATUS_SUCCESS: u16 = 0;
pub const STATUS_REFUSED: u16 = 1;
pub const REASON_LEAVING: u16 = 3;
pub const REASON_HANDSHAKE_TIMEOUT: u16 = 15;
pub const REASON_8021X_FAILED: u16 = 23;

/// How a network authenticates its hosts (the RSN AKM suite).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Akm {
    /// WPA2-Enterprise: IEEE 802.1X with a RADIUS server (suite 1).
    Dot1x,
    /// WPA2-Personal: a pre-shared key (suite 2).
    Psk,
}

impl Akm {
    fn suite(self) -> u8 {
        match self {
            Akm::Dot1x => 1,
            Akm::Psk => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Akm::Dot1x => "WPA2-Enterprise",
            Akm::Psk => "WPA2-PSK",
        }
    }
}

/// The RSN information element for a CCMP network with one AKM suite.
pub fn rsn_ie(akm: Akm) -> Vec<u8> {
    let mut b = vec![IE_RSN, 20, 1, 0];
    b.extend_from_slice(&OUI);
    b.push(CIPHER_CCMP);
    b.extend_from_slice(&[1, 0]);
    b.extend_from_slice(&OUI);
    b.push(CIPHER_CCMP);
    b.extend_from_slice(&[1, 0]);
    b.extend_from_slice(&OUI);
    b.push(akm.suite());
    b.extend_from_slice(&[0, 0]);
    b
}

/// The AKM an RSN element (its body, without type and length) names.
pub(crate) fn rsn_akm(body: &[u8]) -> Option<Akm> {
    // version 2, group suite 4, pairwise count 2 + 4 each, AKM count 2, AKM suite 4.
    let pairwise = usize::from(*body.get(6)?) | usize::from(*body.get(7)?) << 8;
    let at = 8usize.checked_add(pairwise.checked_mul(4)?)?.checked_add(2)?;
    match body.get(at..at + 4)? {
        [0x00, 0x0f, 0xac, 1] => Some(Akm::Dot1x),
        [0x00, 0x0f, 0xac, 2] => Some(Akm::Psk),
        _ => None,
    }
}

/// What a beacon or probe response says about a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bss {
    pub ssid: String,
    pub channel: u8,
    pub rsn: Option<Akm>,
    /// Beacon interval, in time units of 1024 µs.
    pub interval: u16,
    pub timestamp: u64,
}

/// The body of an 802.11 frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Beacon(Bss),
    ProbeRequest {
        ssid: String,
    },
    ProbeResponse(Bss),
    /// Open System authentication (algorithm 0): transaction 1 asks, 2 answers.
    Auth {
        algorithm: u16,
        transaction: u16,
        status: u16,
    },
    AssocRequest {
        ssid: String,
        rsn: Option<Akm>,
    },
    AssocResponse {
        status: u16,
        aid: u16,
    },
    /// A host moving to another access point of the same network: `current_ap` is the one it
    /// leaves.
    ReassocRequest {
        ssid: String,
        rsn: Option<Akm>,
        current_ap: MacAddr,
    },
    ReassocResponse {
        status: u16,
        aid: u16,
    },
    Disassoc {
        reason: u16,
    },
    Deauth {
        reason: u16,
    },
    /// A data frame: LLC/SNAP, then this EtherType and payload.
    Data {
        ethertype: u16,
        payload: Vec<u8>,
    },
    /// A CCMP-protected data frame: its packet number and key id (from the CCMP header), then
    /// the encrypted LLC/SNAP, EtherType and payload, and the MIC. [`ccmp_decrypt`] opens it.
    Protected {
        key_id: u8,
        pn: u64,
        data: Vec<u8>,
    },
}

/// An IEEE 802.11 frame (no FCS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dot11 {
    /// The frame-control flags byte (To DS, From DS, retry…).
    pub flags: u8,
    pub duration: u16,
    pub addr1: MacAddr,
    pub addr2: MacAddr,
    pub addr3: MacAddr,
    /// Sequence number (12 bits); fragments are not modelled.
    pub seq: u16,
    /// The fourth address, in frames between two access points (To DS and From DS both set).
    pub addr4: Option<MacAddr>,
    pub body: Body,
}

fn ie(out: &mut Vec<u8>, id: u8, data: &[u8]) {
    let data = data.get(..data.len().min(255)).unwrap_or_default();
    out.push(id);
    out.push(data.len() as u8);
    out.extend_from_slice(data);
}

/// Information elements as (id, body), until the bytes run out or stop making sense.
fn elements(mut b: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    while let (Some(&id), Some(&len)) = (b.first(), b.get(1)) {
        let Some(body) = b.get(2..2 + usize::from(len)) else { break };
        out.push((id, body));
        b = b.get(2 + usize::from(len)..).unwrap_or_default();
    }
    out
}

fn ssid_of(ies: &[(u8, &[u8])]) -> String {
    ies.iter().find(|(id, _)| *id == IE_SSID).map(|(_, b)| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn bss_body(out: &mut Vec<u8>, b: &Bss) {
    out.extend_from_slice(&b.timestamp.to_le_bytes());
    out.extend_from_slice(&b.interval.to_le_bytes());
    let cap = CAP_ESS | CAP_SHORT_PREAMBLE | if b.rsn.is_some() { CAP_PRIVACY } else { 0 };
    out.extend_from_slice(&cap.to_le_bytes());
    ie(out, IE_SSID, b.ssid.as_bytes());
    ie(out, IE_RATES, &RATES);
    ie(out, IE_DS, &[b.channel]);
    if let Some(a) = b.rsn {
        out.extend_from_slice(&rsn_ie(a));
    }
}

fn bss_decode(b: &[u8]) -> Result<Bss> {
    let t = || WireError::Truncated("802.11 beacon");
    let timestamp = u64::from_le_bytes(b.get(0..8).ok_or_else(t)?.try_into().map_err(|_| t())?);
    let interval = le16(b, 8).ok_or_else(t)?;
    le16(b, 10).ok_or_else(t)?;
    let ies = elements(b.get(12..).unwrap_or_default());
    Ok(Bss {
        ssid: ssid_of(&ies),
        channel: ies.iter().find(|(id, _)| *id == IE_DS).and_then(|(_, d)| d.first().copied()).unwrap_or(0),
        rsn: ies.iter().find(|(id, _)| *id == IE_RSN).and_then(|(_, d)| rsn_akm(d)),
        interval,
        timestamp,
    })
}

impl Dot11 {
    /// A management frame from `sa` to `da` in the network `bssid`.
    pub fn mgmt(da: MacAddr, sa: MacAddr, bssid: MacAddr, seq: u16, body: Body) -> Dot11 {
        Dot11 { flags: 0, duration: 0, addr1: da, addr2: sa, addr3: bssid, seq, addr4: None, body }
    }

    /// A data frame a host sends its access point (To DS): for `da` beyond the AP.
    pub fn to_ap(bssid: MacAddr, sa: MacAddr, da: MacAddr, seq: u16, ethertype: u16, payload: Vec<u8>) -> Dot11 {
        Dot11 { flags: FLAG_TO_DS, duration: 0, addr1: bssid, addr2: sa, addr3: da, seq, addr4: None, body: Body::Data { ethertype, payload } }
    }

    /// A data frame an access point sends a host (From DS), on behalf of `sa`.
    pub fn from_ap(da: MacAddr, bssid: MacAddr, sa: MacAddr, seq: u16, ethertype: u16, payload: Vec<u8>) -> Dot11 {
        Dot11 { flags: FLAG_FROM_DS, duration: 0, addr1: da, addr2: bssid, addr3: sa, seq, addr4: None, body: Body::Data { ethertype, payload } }
    }

    pub fn to_ds(&self) -> bool {
        self.flags & FLAG_TO_DS != 0
    }

    pub fn from_ds(&self) -> bool {
        self.flags & FLAG_FROM_DS != 0
    }

    /// The access point's address (the BSSID) of a frame between a host and its AP.
    pub fn bssid(&self) -> MacAddr {
        match (self.to_ds(), self.from_ds()) {
            (true, false) => self.addr1,
            (false, true) => self.addr2,
            _ => self.addr3,
        }
    }

    /// For a data frame: the Ethernet II frame it stands for (destination and source are the
    /// end stations, not the radio hop).
    pub fn ethernet(&self) -> Option<Frame> {
        let Body::Data { ethertype, payload } = &self.body else { return None };
        let (dst, src) = match (self.to_ds(), self.from_ds()) {
            (false, false) => (self.addr1, self.addr2),
            (true, false) => (self.addr3, self.addr2),
            (false, true) => (self.addr1, self.addr3),
            (true, true) => (self.addr3, self.addr4?),
        };
        Some(Frame { dst, src, vlan: None, ethertype: *ethertype, payload: payload.clone() })
    }

    pub fn is_data(&self) -> bool {
        matches!(self.body, Body::Data { .. } | Body::Protected { .. })
    }

    pub fn is_protected(&self) -> bool {
        matches!(self.body, Body::Protected { .. })
    }

    fn type_subtype(&self) -> (u8, u8) {
        match &self.body {
            Body::AssocRequest { .. } => (TYPE_MGMT, ST_ASSOC_REQ),
            Body::AssocResponse { .. } => (TYPE_MGMT, ST_ASSOC_RESP),
            Body::ReassocRequest { .. } => (TYPE_MGMT, ST_REASSOC_REQ),
            Body::ReassocResponse { .. } => (TYPE_MGMT, ST_REASSOC_RESP),
            Body::ProbeRequest { .. } => (TYPE_MGMT, ST_PROBE_REQ),
            Body::ProbeResponse(_) => (TYPE_MGMT, ST_PROBE_RESP),
            Body::Beacon(_) => (TYPE_MGMT, ST_BEACON),
            Body::Disassoc { .. } => (TYPE_MGMT, ST_DISASSOC),
            Body::Auth { .. } => (TYPE_MGMT, ST_AUTH),
            Body::Deauth { .. } => (TYPE_MGMT, ST_DEAUTH),
            Body::Data { .. } | Body::Protected { .. } => (TYPE_DATA, 0),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let (ty, st) = self.type_subtype();
        let mut out = Vec::with_capacity(64);
        out.push((st << 4) | (ty << 2));
        out.push(if self.is_protected() { self.flags | FLAG_PROTECTED } else { self.flags & !FLAG_PROTECTED });
        out.extend_from_slice(&self.duration.to_le_bytes());
        out.extend_from_slice(&self.addr1.0);
        out.extend_from_slice(&self.addr2.0);
        out.extend_from_slice(&self.addr3.0);
        out.extend_from_slice(&((self.seq & 0x0fff) << 4).to_le_bytes());
        if self.to_ds() && self.from_ds() {
            out.extend_from_slice(&self.addr4.unwrap_or(MacAddr::ZERO).0);
        }
        match &self.body {
            Body::Beacon(b) | Body::ProbeResponse(b) => bss_body(&mut out, b),
            Body::ProbeRequest { ssid } => {
                ie(&mut out, IE_SSID, ssid.as_bytes());
                ie(&mut out, IE_RATES, &RATES);
            }
            Body::Auth { algorithm, transaction, status } => {
                for v in [algorithm, transaction, status] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            Body::AssocRequest { ssid, rsn } | Body::ReassocRequest { ssid, rsn, .. } => {
                let cap = CAP_ESS | CAP_SHORT_PREAMBLE | if rsn.is_some() { CAP_PRIVACY } else { 0 };
                out.extend_from_slice(&cap.to_le_bytes());
                out.extend_from_slice(&10u16.to_le_bytes()); // listen interval
                if let Body::ReassocRequest { current_ap, .. } = &self.body {
                    out.extend_from_slice(&current_ap.0);
                }
                ie(&mut out, IE_SSID, ssid.as_bytes());
                ie(&mut out, IE_RATES, &RATES);
                if let Some(a) = rsn {
                    out.extend_from_slice(&rsn_ie(*a));
                }
            }
            Body::AssocResponse { status, aid } | Body::ReassocResponse { status, aid } => {
                out.extend_from_slice(&(CAP_ESS | CAP_SHORT_PREAMBLE).to_le_bytes());
                out.extend_from_slice(&status.to_le_bytes());
                out.extend_from_slice(&(0xc000 | (aid & 0x3fff)).to_le_bytes());
                ie(&mut out, IE_RATES, &RATES);
            }
            Body::Disassoc { reason } | Body::Deauth { reason } => out.extend_from_slice(&reason.to_le_bytes()),
            Body::Data { ethertype, payload } => {
                out.extend_from_slice(&SNAP);
                out.extend_from_slice(&ethertype.to_be_bytes());
                out.extend_from_slice(payload);
            }
            Body::Protected { key_id, pn, data } => {
                out.extend_from_slice(&ccmp_header(*pn, *key_id));
                out.extend_from_slice(data);
            }
        }
        out
    }

    pub fn decode(b: &[u8]) -> Result<Dot11> {
        let t = || WireError::Truncated("802.11");
        let fc = *b.first().ok_or_else(t)?;
        let flags = *b.get(1).ok_or_else(t)?;
        if fc & 0x03 != 0 {
            return Err(WireError::Unsupported("802.11", format!("protocol version {}", fc & 3)));
        }
        let (ty, st) = ((fc >> 2) & 0x03, fc >> 4);
        let duration = le16(b, 2).ok_or_else(t)?;
        let addr1 = mac_at(b, 4).ok_or_else(t)?;
        let addr2 = mac_at(b, 10).ok_or_else(t)?;
        let addr3 = mac_at(b, 16).ok_or_else(t)?;
        let seq = le16(b, 22).ok_or_else(t)? >> 4;
        let mut at = 24;
        let mut addr4 = None;
        if ty == TYPE_DATA && flags & (FLAG_TO_DS | FLAG_FROM_DS) == FLAG_TO_DS | FLAG_FROM_DS {
            addr4 = Some(mac_at(b, 24).ok_or_else(t)?);
            at = 30;
        }
        let body = b.get(at..).ok_or_else(t)?;
        let body = match (ty, st) {
            (TYPE_MGMT, ST_BEACON) => Body::Beacon(bss_decode(body)?),
            (TYPE_MGMT, ST_PROBE_RESP) => Body::ProbeResponse(bss_decode(body)?),
            (TYPE_MGMT, ST_PROBE_REQ) => Body::ProbeRequest { ssid: ssid_of(&elements(body)) },
            (TYPE_MGMT, ST_AUTH) => Body::Auth {
                algorithm: le16(body, 0).ok_or_else(t)?,
                transaction: le16(body, 2).ok_or_else(t)?,
                status: le16(body, 4).ok_or_else(t)?,
            },
            (TYPE_MGMT, ST_ASSOC_REQ) => {
                le16(body, 2).ok_or_else(t)?;
                let ies = elements(body.get(4..).unwrap_or_default());
                Body::AssocRequest { ssid: ssid_of(&ies), rsn: ies.iter().find(|(id, _)| *id == IE_RSN).and_then(|(_, d)| rsn_akm(d)) }
            }
            (TYPE_MGMT, ST_ASSOC_RESP) => Body::AssocResponse { status: le16(body, 2).ok_or_else(t)?, aid: le16(body, 4).ok_or_else(t)? & 0x3fff },
            (TYPE_MGMT, ST_REASSOC_REQ) => {
                le16(body, 2).ok_or_else(t)?;
                let current_ap = mac_at(body, 4).ok_or_else(t)?;
                let ies = elements(body.get(10..).unwrap_or_default());
                Body::ReassocRequest { ssid: ssid_of(&ies), rsn: ies.iter().find(|(id, _)| *id == IE_RSN).and_then(|(_, d)| rsn_akm(d)), current_ap }
            }
            (TYPE_MGMT, ST_REASSOC_RESP) => {
                Body::ReassocResponse { status: le16(body, 2).ok_or_else(t)?, aid: le16(body, 4).ok_or_else(t)? & 0x3fff }
            }
            (TYPE_MGMT, ST_DISASSOC) => Body::Disassoc { reason: le16(body, 0).ok_or_else(t)? },
            (TYPE_MGMT, ST_DEAUTH) => Body::Deauth { reason: le16(body, 0).ok_or_else(t)? },
            (TYPE_DATA, 0) if flags & FLAG_PROTECTED != 0 => {
                let (pn, key_id) = ccmp_header_decode(body).ok_or_else(t)?;
                let data = body.get(CCMP_HEADER_LEN..).unwrap_or_default();
                if data.len() < CCMP_MIC_LEN {
                    return Err(t());
                }
                Body::Protected { key_id, pn, data: data.to_vec() }
            }
            (TYPE_DATA, 0) => {
                if body.get(..6).ok_or_else(t)? != SNAP {
                    return Err(WireError::Unsupported("802.11", "data without an LLC/SNAP header".into()));
                }
                Body::Data { ethertype: be16(body, 6).ok_or_else(t)?, payload: body.get(8..).unwrap_or_default().to_vec() }
            }
            (ty, st) => return Err(WireError::Unsupported("802.11", format!("type {ty} subtype {st}"))),
        };
        Ok(Dot11 { flags, duration, addr1, addr2, addr3, seq, addr4, body })
    }

    /// The frame's name and what it says, for the event list.
    pub fn describe(&self) -> String {
        let auth = |s: u16| if s == STATUS_SUCCESS { "success".to_string() } else { format!("refused (status {s})") };
        match &self.body {
            Body::Beacon(b) => {
                format!("802.11 beacon: \"{}\" on channel {}{}", b.ssid, b.channel, b.rsn.map(|a| format!(", {}", a.name())).unwrap_or_default())
            }
            Body::ProbeRequest { ssid } if ssid.is_empty() => "802.11 probe request: any network?".to_string(),
            Body::ProbeRequest { ssid } => format!("802.11 probe request: \"{ssid}\"?"),
            Body::ProbeResponse(b) => {
                format!(
                    "802.11 probe response: \"{}\" on channel {}{}",
                    b.ssid,
                    b.channel,
                    b.rsn.map_or(", open".to_string(), |a| format!(", {}", a.name()))
                )
            }
            Body::Auth { transaction, .. } if *transaction % 2 == 1 => format!("802.11 authentication request (open system, from {})", self.addr2),
            Body::Auth { status, .. } => format!("802.11 authentication response: {}", auth(*status)),
            Body::AssocRequest { ssid, rsn } => {
                format!("802.11 association request: \"{ssid}\"{}", rsn.map(|a| format!(" with {}", a.name())).unwrap_or_default())
            }
            Body::AssocResponse { status, aid } if *status == STATUS_SUCCESS => format!("802.11 association response: success, association id {aid}"),
            Body::AssocResponse { status, .. } => format!("802.11 association response: {}", auth(*status)),
            Body::ReassocRequest { ssid, current_ap, .. } => format!("802.11 reassociation request: \"{ssid}\", moving from {current_ap}"),
            Body::ReassocResponse { status, aid } if *status == STATUS_SUCCESS => {
                format!("802.11 reassociation response: success, association id {aid}")
            }
            Body::ReassocResponse { status, .. } => format!("802.11 reassociation response: {}", auth(*status)),
            Body::Disassoc { reason } => format!("802.11 disassociation (reason {reason})"),
            Body::Deauth { reason } => format!("802.11 deauthentication (reason {reason}: {})", reason_text(*reason)),
            Body::Data { ethertype, payload } if *ethertype == ETHERTYPE_EAPOL => match Eapol::decode(payload) {
                Ok(e) => e.describe(),
                Err(e) => format!("EAPOL ({e})"),
            },
            Body::Data { ethertype, .. } => format!("802.11 data, EtherType 0x{ethertype:04x}"),
            Body::Protected { pn, .. } => format!("802.11 protected data (CCMP, packet number {pn})"),
        }
    }

    /// The decoded header fields, as (name, value), for the PDU window.
    pub fn fields(&self) -> Vec<(String, String)> {
        let (ty, st) = self.type_subtype();
        let kind = match &self.body {
            Body::Beacon(_) => "beacon",
            Body::ProbeRequest { .. } => "probe request",
            Body::ProbeResponse(_) => "probe response",
            Body::Auth { .. } => "authentication",
            Body::AssocRequest { .. } => "association request",
            Body::AssocResponse { .. } => "association response",
            Body::ReassocRequest { .. } => "reassociation request",
            Body::ReassocResponse { .. } => "reassociation response",
            Body::Disassoc { .. } => "disassociation",
            Body::Deauth { .. } => "deauthentication",
            Body::Data { .. } => "data",
            Body::Protected { .. } => "protected data",
        };
        let mut out = vec![
            ("Type".to_string(), format!("{kind} (type {ty}, subtype {st})")),
            ("To DS / From DS".into(), format!("{} / {}", u8::from(self.to_ds()), u8::from(self.from_ds()))),
            ("Duration".into(), format!("{} µs", self.duration)),
            ("Address 1 (receiver)".into(), self.addr1.to_string()),
            ("Address 2 (transmitter)".into(), self.addr2.to_string()),
            ("Address 3".into(), self.addr3.to_string()),
            ("BSSID".into(), self.bssid().to_string()),
            ("Sequence".into(), self.seq.to_string()),
        ];
        match &self.body {
            Body::Beacon(b) | Body::ProbeResponse(b) => {
                out.push(("SSID".into(), b.ssid.clone()));
                out.push(("Channel".into(), b.channel.to_string()));
                out.push(("Security".into(), b.rsn.map_or("open", Akm::name).into()));
            }
            Body::ProbeRequest { ssid } => out.push(("SSID".into(), ssid.clone())),
            Body::Auth { algorithm, transaction, status } => {
                out.push(("Algorithm".into(), if *algorithm == 0 { "open system (0)".into() } else { algorithm.to_string() }));
                out.push(("Transaction".into(), transaction.to_string()));
                out.push(("Status".into(), status.to_string()));
            }
            Body::AssocRequest { ssid, rsn } => {
                out.push(("SSID".into(), ssid.clone()));
                out.push(("Security".into(), rsn.map_or("open", Akm::name).into()));
            }
            Body::ReassocRequest { ssid, rsn, current_ap } => {
                out.push(("SSID".into(), ssid.clone()));
                out.push(("Security".into(), rsn.map_or("open", Akm::name).into()));
                out.push(("Current AP".into(), current_ap.to_string()));
            }
            Body::AssocResponse { status, aid } | Body::ReassocResponse { status, aid } => {
                out.push(("Status".into(), status.to_string()));
                out.push(("Association id".into(), aid.to_string()));
            }
            Body::Disassoc { reason } | Body::Deauth { reason } => out.push(("Reason".into(), format!("{reason}: {}", reason_text(*reason)))),
            Body::Data { ethertype, payload } => {
                out.push(("LLC/SNAP".into(), "aa aa 03 00 00 00".into()));
                out.push(("EtherType".into(), format!("0x{ethertype:04x}")));
                out.push(("Payload".into(), format!("{} bytes", payload.len())));
            }
            Body::Protected { key_id, pn, data } => {
                out.push(("Protected".into(), "yes (CCMP)".into()));
                out.push(("Key id".into(), key_id.to_string()));
                out.push(("Packet number".into(), pn.to_string()));
                out.push(("Encrypted".into(), format!("{} bytes + {CCMP_MIC_LEN}-byte MIC", data.len().saturating_sub(CCMP_MIC_LEN))));
            }
        }
        out
    }
}

// ---- CCMP (IEEE 802.11 §12.5.3) ----

fn ccmp_header(pn: u64, key_id: u8) -> [u8; CCMP_HEADER_LEN] {
    let p = (pn & MAX_PN).to_le_bytes();
    [p[0], p[1], 0, EXT_IV | ((key_id & 3) << 6), p[2], p[3], p[4], p[5]]
}

fn ccmp_header_decode(b: &[u8]) -> Option<(u64, u8)> {
    let h = b.get(..CCMP_HEADER_LEN)?;
    let (h0, h1, h3, h4, h5, h6, h7) = (*h.first()?, *h.get(1)?, *h.get(3)?, *h.get(4)?, *h.get(5)?, *h.get(6)?, *h.get(7)?);
    if h3 & EXT_IV == 0 {
        return None;
    }
    Some((u64::from_le_bytes([h0, h1, h4, h5, h6, h7, 0, 0]), h3 >> 6))
}

/// The MAC header's length: three addresses and sequence control, a fourth address between
/// two access points, and QoS control on QoS data subtypes.
fn mac_header_len(b: &[u8]) -> Option<usize> {
    let (fc, flags) = (*b.first()?, *b.get(1)?);
    let mut n = 24;
    if (fc >> 2) & 3 == TYPE_DATA {
        if flags & (FLAG_TO_DS | FLAG_FROM_DS) == FLAG_TO_DS | FLAG_FROM_DS {
            n += 6;
        }
        if fc & 0x80 != 0 {
            n += 2;
        }
    }
    (b.len() >= n).then_some(n)
}

/// The CCM nonce and additional authenticated data of a frame (§12.5.3.3.3-4).
fn ccmp_nonce_aad(hdr: &[u8], pn: u64) -> Option<([u8; 13], Vec<u8>)> {
    let fc = *hdr.first()?;
    let flags = *hdr.get(1)?;
    let data = (fc >> 2) & 3 == TYPE_DATA;
    let qos = data && fc & 0x80 != 0;
    let mut aad = Vec::with_capacity(30);
    // Subtype bits 4-6 masked on data frames; retry, power management and more data masked;
    // protected set.
    aad.push(if data { fc & 0x8f } else { fc });
    aad.push((flags & !(FLAG_RETRY | FLAG_PWR_MGT | FLAG_MORE_DATA)) | FLAG_PROTECTED);
    aad.extend_from_slice(hdr.get(4..22)?);
    // Sequence control: the fragment number only.
    aad.push(*hdr.get(22)? & 0x0f);
    aad.push(0);
    let mut at = 24;
    if data && flags & (FLAG_TO_DS | FLAG_FROM_DS) == FLAG_TO_DS | FLAG_FROM_DS {
        aad.extend_from_slice(hdr.get(24..30)?);
        at = 30;
    }
    let mut priority = 0;
    if qos {
        priority = *hdr.get(at)? & 0x0f;
        aad.push(priority);
        aad.push(0);
    }
    let mut nonce = [0u8; 13];
    nonce[0] = priority;
    nonce[1..7].copy_from_slice(hdr.get(10..16)?);
    nonce[7..].copy_from_slice(&(pn & MAX_PN).to_be_bytes()[2..]);
    Some((nonce, aad))
}

/// Protects a data frame with CCMP under a temporal key: same MAC header (with the Protected
/// bit), the CCMP header carrying `pn`, the encrypted body and the MIC.
pub fn ccmp_encrypt(tk: &[u8; 16], mpdu: &[u8], pn: u64, key_id: u8) -> Result<Vec<u8>> {
    let t = || WireError::Truncated("802.11");
    let n = mac_header_len(mpdu).ok_or_else(t)?;
    if (*mpdu.first().ok_or_else(t)? >> 2) & 3 != TYPE_DATA {
        return Err(WireError::Unsupported("CCMP", "only data frames are protected here".into()));
    }
    let mut hdr = mpdu.get(..n).ok_or_else(t)?.to_vec();
    if let Some(f) = hdr.get_mut(1) {
        *f |= FLAG_PROTECTED;
    }
    let (nonce, aad) = ccmp_nonce_aad(&hdr, pn).ok_or_else(t)?;
    let sealed = crate::aes::Ccm::new(tk, CCMP_MIC_LEN)
        .seal(&nonce, &aad, mpdu.get(n..).unwrap_or_default())
        .map_err(|_| WireError::Unsupported("CCMP", "frame too long".into()))?;
    hdr.extend_from_slice(&ccmp_header(pn, key_id));
    hdr.extend_from_slice(&sealed);
    Ok(hdr)
}

/// Opens a CCMP-protected frame: the plaintext frame (Protected bit cleared) and its packet
/// number. Fails on a wrong key or changed bytes (the MIC doesn't check out).
pub fn ccmp_decrypt(tk: &[u8; 16], mpdu: &[u8]) -> Result<(Vec<u8>, u64)> {
    let t = || WireError::Truncated("802.11");
    let n = mac_header_len(mpdu).ok_or_else(t)?;
    if mpdu.get(1).is_none_or(|f| f & FLAG_PROTECTED == 0) {
        return Err(WireError::Unsupported("CCMP", "frame is not protected".into()));
    }
    let hdr = mpdu.get(..n).ok_or_else(t)?;
    let (pn, _) = ccmp_header_decode(mpdu.get(n..).unwrap_or_default()).ok_or_else(t)?;
    let (nonce, aad) = ccmp_nonce_aad(hdr, pn).ok_or_else(t)?;
    let body = crate::aes::Ccm::new(tk, CCMP_MIC_LEN)
        .open(&nonce, &aad, mpdu.get(n + CCMP_HEADER_LEN..).unwrap_or_default())
        .map_err(|_| WireError::Unsupported("CCMP", "MIC check failed (wrong key or changed bytes)".into()))?;
    let mut out = hdr.to_vec();
    if let Some(f) = out.get_mut(1) {
        *f &= !FLAG_PROTECTED;
    }
    out.extend_from_slice(&body);
    Ok((out, pn))
}

fn reason_text(r: u16) -> &'static str {
    match r {
        1 => "unspecified",
        2 => "previous authentication no longer valid",
        REASON_LEAVING => "leaving the network",
        14 => "message integrity check failure",
        REASON_HANDSHAKE_TIMEOUT => "4-way handshake failed",
        REASON_8021X_FAILED => "IEEE 802.1X authentication failed",
        _ => "other",
    }
}

// ---- EAPOL (IEEE 802.1X) ----

const EAPOL_VERSION: u8 = 2;
const EAPOL_EAP: u8 = 0;
const EAPOL_START: u8 = 1;
const EAPOL_LOGOFF: u8 = 2;
const EAPOL_KEY: u8 = 3;

pub const EAP_REQUEST: u8 = 1;
pub const EAP_RESPONSE: u8 = 2;
pub const EAP_SUCCESS: u8 = 3;
pub const EAP_FAILURE: u8 = 4;
pub const EAP_IDENTITY: u8 = 1;
/// EAP method 25, PEAP: a TLS tunnel protecting an inner login. Its first data byte holds the
/// flags (length included, more fragments, start) and version.
pub const EAP_PEAP: u8 = 25;
/// The PEAP Start flag (version 0).
pub const PEAP_START: u8 = 0x20;

/// An EAP packet (RFC 3748 §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eap {
    pub code: u8,
    pub id: u8,
    /// The method type of requests and responses (1 = identity).
    pub kind: Option<u8>,
    pub data: Vec<u8>,
}

impl Eap {
    pub fn encode(&self) -> Vec<u8> {
        let data = self.data.get(..self.data.len().min(1000)).unwrap_or_default();
        let len = 4 + usize::from(self.kind.is_some()) + data.len();
        let mut out = vec![self.code, self.id];
        out.extend_from_slice(&(len as u16).to_be_bytes());
        if let Some(k) = self.kind {
            out.push(k);
        }
        out.extend_from_slice(data);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Eap> {
        let t = || WireError::Truncated("eap");
        let (code, id) = (*b.first().ok_or_else(t)?, *b.get(1).ok_or_else(t)?);
        let len = usize::from(be16(b, 2).ok_or_else(t)?);
        if len < 4 {
            return Err(WireError::Unsupported("eap", format!("length {len}")));
        }
        let b = b.get(..len).ok_or_else(t)?;
        let (kind, data) = match code {
            EAP_REQUEST | EAP_RESPONSE => (Some(*b.get(4).ok_or_else(t)?), b.get(5..).unwrap_or_default().to_vec()),
            EAP_SUCCESS | EAP_FAILURE => (None, Vec::new()),
            other => return Err(WireError::Unsupported("eap", format!("code {other}"))),
        };
        Ok(Eap { code, id, kind, data })
    }
}

/// An EAPOL-Key frame: the RSN key descriptor (IEEE 802.11 §12.7.2), descriptor version 2
/// (HMAC-SHA1 MIC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EapolKey {
    pub info: u16,
    pub key_len: u16,
    pub replay: u64,
    pub nonce: [u8; 32],
    pub mic: [u8; 16],
    pub data: Vec<u8>,
}

/// Key information bits.
pub const KEY_VERSION_2: u16 = 0x0002;
pub const KEY_PAIRWISE: u16 = 0x0008;
pub const KEY_INSTALL: u16 = 0x0040;
pub const KEY_ACK: u16 = 0x0080;
pub const KEY_MIC: u16 = 0x0100;
pub const KEY_SECURE: u16 = 0x0200;

impl EapolKey {
    /// Which message of the 4-way handshake this is (1-4), from its flags.
    pub fn message(&self) -> u8 {
        match (self.info & KEY_ACK != 0, self.info & KEY_SECURE != 0) {
            (true, false) => 1,
            (true, true) => 3,
            (false, _) if self.nonce != [0; 32] => 2,
            _ => 4,
        }
    }
}

/// An EAPOL frame (IEEE 802.1X-2020 §11.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eapol {
    Eap(Eap),
    Start,
    Logoff,
    Key(EapolKey),
}

impl Eapol {
    pub fn encode(&self) -> Vec<u8> {
        let (kind, body) = match self {
            Eapol::Eap(e) => (EAPOL_EAP, e.encode()),
            Eapol::Start => (EAPOL_START, Vec::new()),
            Eapol::Logoff => (EAPOL_LOGOFF, Vec::new()),
            Eapol::Key(k) => {
                let mut b = vec![2]; // RSN key descriptor
                b.extend_from_slice(&k.info.to_be_bytes());
                b.extend_from_slice(&k.key_len.to_be_bytes());
                b.extend_from_slice(&k.replay.to_be_bytes());
                b.extend_from_slice(&k.nonce);
                b.extend_from_slice(&[0; 16]); // key IV
                b.extend_from_slice(&[0; 8]); // key RSC
                b.extend_from_slice(&[0; 8]); // reserved
                b.extend_from_slice(&k.mic);
                let data = k.data.get(..k.data.len().min(1000)).unwrap_or_default();
                b.extend_from_slice(&(data.len() as u16).to_be_bytes());
                b.extend_from_slice(data);
                (EAPOL_KEY, b)
            }
        };
        let mut out = vec![EAPOL_VERSION, kind];
        out.extend_from_slice(&(body.len() as u16).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Eapol> {
        let t = || WireError::Truncated("eapol");
        let kind = *b.get(1).ok_or_else(t)?;
        let len = usize::from(be16(b, 2).ok_or_else(t)?);
        let body = b.get(4..4 + len).ok_or_else(t)?;
        Ok(match kind {
            EAPOL_EAP => Eapol::Eap(Eap::decode(body)?),
            EAPOL_START => Eapol::Start,
            EAPOL_LOGOFF => Eapol::Logoff,
            EAPOL_KEY => {
                if body.first() != Some(&2) {
                    return Err(WireError::Unsupported("eapol", "not an RSN key descriptor".into()));
                }
                let mut nonce = [0u8; 32];
                nonce.copy_from_slice(body.get(13..45).ok_or_else(t)?);
                let mut mic = [0u8; 16];
                mic.copy_from_slice(body.get(77..93).ok_or_else(t)?);
                let dlen = usize::from(be16(body, 93).ok_or_else(t)?);
                Eapol::Key(EapolKey {
                    info: be16(body, 1).ok_or_else(t)?,
                    key_len: be16(body, 3).ok_or_else(t)?,
                    replay: u64::from_be_bytes(body.get(5..13).ok_or_else(t)?.try_into().map_err(|_| t())?),
                    nonce,
                    mic,
                    data: body.get(95..95 + dlen).ok_or_else(t)?.to_vec(),
                })
            }
            other => return Err(WireError::Unsupported("eapol", format!("packet type {other}"))),
        })
    }

    pub fn describe(&self) -> String {
        match self {
            Eapol::Start => "EAPOL start".into(),
            Eapol::Logoff => "EAPOL logoff".into(),
            Eapol::Eap(e) => match (e.code, e.kind) {
                (EAP_REQUEST, Some(EAP_IDENTITY)) => "EAP request: identity?".into(),
                (EAP_RESPONSE, Some(EAP_IDENTITY)) => format!("EAP response: identity \"{}\"", String::from_utf8_lossy(&e.data)),
                (EAP_REQUEST, Some(EAP_PEAP)) if e.data.first().is_some_and(|f| f & PEAP_START != 0) => "EAP request: PEAP start".into(),
                (EAP_REQUEST | EAP_RESPONSE, Some(EAP_PEAP)) => format!(
                    "EAP {}: PEAP, {} byte(s) of TLS tunnel (contents not modelled)",
                    if e.code == EAP_REQUEST { "request" } else { "response" },
                    e.data.len().saturating_sub(1)
                ),
                (EAP_SUCCESS, _) => "EAP success".into(),
                (EAP_FAILURE, _) => "EAP failure".into(),
                (c, k) => format!("EAP code {c} type {}", k.unwrap_or(0)),
            },
            Eapol::Key(k) => format!("EAPOL-Key, 4-way handshake message {} of 4", k.message()),
        }
    }
}

/// The MIC of an EAPOL-Key frame: HMAC-SHA1 with the KCK over the frame with its MIC zeroed,
/// first 16 bytes.
pub fn key_mic(kck: &[u8], key: &EapolKey) -> [u8; 16] {
    let zeroed = Eapol::Key(EapolKey { mic: [0; 16], ..key.clone() }).encode();
    let mac = crate::sha1::hmac_sha1(kck, &zeroed);
    let mut out = [0u8; 16];
    out.copy_from_slice(&mac[..16]);
    out
}

/// A one-line description of an over-the-air frame, and whether it is a data frame.
pub fn summarize(b: &[u8]) -> String {
    match Dot11::decode(b) {
        Ok(f) => f.describe(),
        Err(e) => format!("802.11 ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(n: u8) -> MacAddr {
        MacAddr([2, 0, 0, 0, 0, n])
    }

    fn samples() -> Vec<Dot11> {
        let bss = Bss { ssid: "Aula".into(), channel: 6, rsn: Some(Akm::Psk), interval: 100, timestamp: 1234 };
        let key =
            EapolKey { info: KEY_VERSION_2 | KEY_PAIRWISE | KEY_ACK, key_len: 16, replay: 1, nonce: [7; 32], mic: [0; 16], data: rsn_ie(Akm::Psk) };
        vec![
            Dot11::mgmt(MacAddr::BROADCAST, mac(1), mac(2), 1, Body::Beacon(bss.clone())),
            Dot11::mgmt(MacAddr::BROADCAST, mac(1), MacAddr::BROADCAST, 2, Body::ProbeRequest { ssid: "Aula".into() }),
            Dot11::mgmt(mac(1), mac(2), mac(2), 3, Body::ProbeResponse(Bss { rsn: Some(Akm::Dot1x), ..bss })),
            Dot11::mgmt(mac(2), mac(1), mac(2), 4, Body::Auth { algorithm: 0, transaction: 1, status: 0 }),
            Dot11::mgmt(mac(2), mac(1), mac(2), 5, Body::AssocRequest { ssid: "Aula".into(), rsn: Some(Akm::Psk) }),
            Dot11::mgmt(mac(1), mac(2), mac(2), 6, Body::AssocResponse { status: 0, aid: 3 }),
            Dot11::mgmt(mac(3), mac(1), mac(3), 12, Body::ReassocRequest { ssid: "Aula".into(), rsn: Some(Akm::Psk), current_ap: mac(2) }),
            Dot11::mgmt(mac(1), mac(3), mac(3), 13, Body::ReassocResponse { status: 0, aid: 1 }),
            Dot11 {
                flags: FLAG_TO_DS | FLAG_PROTECTED,
                body: Body::Protected { key_id: 0, pn: 0x0102_0304_0506, data: vec![0x5a; 40] },
                ..Dot11::to_ap(mac(2), mac(1), mac(9), 14, 0, vec![])
            },
            Dot11::mgmt(mac(1), mac(2), mac(2), 7, Body::Deauth { reason: REASON_HANDSHAKE_TIMEOUT }),
            Dot11::mgmt(mac(1), mac(2), mac(2), 8, Body::Disassoc { reason: REASON_LEAVING }),
            Dot11::to_ap(mac(2), mac(1), mac(9), 9, crate::ETHERTYPE_IPV4, vec![0x45; 30]),
            Dot11::from_ap(mac(1), mac(2), mac(9), 10, ETHERTYPE_EAPOL, Eapol::Key(key).encode()),
            Dot11::from_ap(
                mac(1),
                mac(2),
                mac(2),
                11,
                ETHERTYPE_EAPOL,
                Eapol::Eap(Eap { code: EAP_REQUEST, id: 1, kind: Some(EAP_IDENTITY), data: vec![] }).encode(),
            ),
        ]
    }

    #[test]
    fn frames_round_trip() {
        for f in samples() {
            let b = f.encode();
            assert_eq!(Dot11::decode(&b).as_ref(), Ok(&f), "{}", f.describe());
            assert!(!summarize(&b).contains("truncated"));
        }
    }

    #[test]
    fn header_is_ieee_802_11() {
        let f = Dot11::to_ap(mac(2), mac(1), mac(9), 0x123, crate::ETHERTYPE_ARP, vec![1, 2]);
        let b = f.encode();
        // Data (type 2), To DS, addresses BSSID / SA / DA, sequence in the top 12 bits.
        assert_eq!(&b[..2], &[0x08, 0x01]);
        assert_eq!(&b[4..10], &mac(2).0);
        assert_eq!(&b[22..24], &(0x123u16 << 4).to_le_bytes());
        assert_eq!(&b[24..32], &[0xaa, 0xaa, 0x03, 0, 0, 0, 0x08, 0x06]);
        let eth = f.ethernet().unwrap();
        assert_eq!((eth.dst, eth.src, eth.ethertype), (mac(9), mac(1), crate::ETHERTYPE_ARP));
        let down = Dot11::from_ap(mac(1), mac(2), mac(9), 1, crate::ETHERTYPE_ARP, vec![]);
        assert_eq!(down.ethernet().map(|e| (e.dst, e.src)), Some((mac(1), mac(9))));
        assert_eq!(down.bssid(), mac(2));
        let probe = Dot11::mgmt(MacAddr::BROADCAST, mac(1), MacAddr::BROADCAST, 0, Body::ProbeRequest { ssid: String::new() }).encode();
        assert_eq!(probe[0], 0x40);
        assert_eq!(rsn_ie(Akm::Dot1x).len(), 22);
    }

    #[test]
    fn eapol_round_trips_and_signs() {
        let key =
            EapolKey { info: KEY_VERSION_2 | KEY_PAIRWISE | KEY_MIC, key_len: 16, replay: 1, nonce: [9; 32], mic: [0; 16], data: rsn_ie(Akm::Psk) };
        let mic = key_mic(&[1; 16], &key);
        let signed = EapolKey { mic, ..key };
        let b = Eapol::Key(signed.clone()).encode();
        assert_eq!(Eapol::decode(&b), Ok(Eapol::Key(signed.clone())));
        assert_eq!(signed.message(), 2);
        assert_eq!(key_mic(&[1; 16], &signed), mic);
        assert_ne!(key_mic(&[2; 16], &signed), mic);
        for e in [
            Eapol::Start,
            Eapol::Logoff,
            Eapol::Eap(Eap { code: EAP_RESPONSE, id: 4, kind: Some(EAP_IDENTITY), data: b"ana".to_vec() }),
            Eapol::Eap(Eap { code: EAP_REQUEST, id: 5, kind: Some(EAP_PEAP), data: vec![PEAP_START] }),
            Eapol::Eap(Eap { code: EAP_RESPONSE, id: 5, kind: Some(EAP_PEAP), data: vec![0] }),
        ] {
            assert_eq!(Eapol::decode(&e.encode()), Ok(e.clone()));
        }
        assert_eq!(
            Eapol::decode(&Eapol::Eap(Eap { code: EAP_SUCCESS, id: 4, kind: None, data: vec![] }).encode()).map(|e| e.describe()),
            Ok("EAP success".into())
        );
    }

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// The CCMP test vector of IEEE 802.11 Annex J (non-QoS data frame, retry set).
    #[test]
    fn ccmp_matches_the_ieee_802_11_test_vector() {
        let tk: [u8; 16] = hex("c97c1f67ce371185514a8a19f2bdd52f").try_into().unwrap();
        let hdr = hex("08 48 c3 2c 0f d2 e1 28 a5 7c 50 30 f1 84 44 08 ab ae a5 b8 fc ba 80 33");
        let plain = hex("f8 ba 1a 55 d0 2f 85 ae 96 7b b6 2f b6 cd a8 eb 7e 78 a0 50");
        let mut mpdu = hdr.clone();
        mpdu[1] &= !FLAG_PROTECTED;
        mpdu.extend_from_slice(&plain);
        let enc = ccmp_encrypt(&tk, &mpdu, 0xb503_9776_e70c, 0).unwrap();
        let want = hex("08 48 c3 2c 0f d2 e1 28 a5 7c 50 30 f1 84 44 08 ab ae a5 b8 fc ba 80 33 0c e7 00 20 76 97 03 b5
             f3 d0 a2 fe 9a 3d bf 23 42 a6 43 e4 32 46 e8 0c 3c 04 d0 19 78 45 ce 0b 16 f9 76 23");
        assert_eq!(enc, want);
        let (back, pn) = ccmp_decrypt(&tk, &enc).unwrap();
        assert_eq!((back, pn), (mpdu, 0xb503_9776_e70c));
        let f = Dot11::decode(&enc).unwrap();
        assert!(f.is_protected() && f.ethernet().is_none());
        assert_eq!(f.encode(), enc);
        assert!(summarize(&enc).contains("protected"));
    }

    #[test]
    fn ccmp_round_trips_and_refuses_a_wrong_key_or_changed_bytes() {
        let tk = [7u8; 16];
        let f = Dot11::to_ap(mac(2), mac(1), mac(9), 5, crate::ETHERTYPE_IPV4, vec![0x45; 60]).encode();
        let enc = ccmp_encrypt(&tk, &f, 42, 0).unwrap();
        assert_eq!(enc.len(), f.len() + CCMP_HEADER_LEN + CCMP_MIC_LEN);
        assert_ne!(&enc[24 + 8..24 + 8 + 20], &f[24..44]);
        assert_eq!(ccmp_decrypt(&tk, &enc).unwrap(), (f.clone(), 42));
        assert!(ccmp_decrypt(&[8; 16], &enc).is_err());
        let mut bad = enc.clone();
        bad[40] ^= 0x80;
        assert!(ccmp_decrypt(&tk, &bad).is_err());
        // The header is authenticated too: changing the destination breaks the MIC.
        let mut moved = enc.clone();
        moved[16] ^= 1;
        assert!(ccmp_decrypt(&tk, &moved).is_err());
        assert!(ccmp_decrypt(&tk, &f).is_err());
        for n in 0..enc.len() {
            assert!(ccmp_decrypt(&tk, &enc[..n]).is_err());
            let _ = ccmp_encrypt(&tk, &f[..n.min(f.len())], 1, 0);
        }
        let probe = Dot11::mgmt(MacAddr::BROADCAST, mac(1), MacAddr::BROADCAST, 0, Body::ProbeRequest { ssid: String::new() }).encode();
        assert!(ccmp_encrypt(&tk, &probe, 1, 0).is_err());
    }

    #[test]
    fn decoders_survive_every_truncation_and_garbage() {
        for f in samples() {
            let b = f.encode();
            for n in 0..b.len() {
                let _ = Dot11::decode(&b[..n]);
                let _ = summarize(&b[..n]);
                let _ = Eapol::decode(b.get(32..n).unwrap_or_default());
                let _ = Eap::decode(b.get(36..n).unwrap_or_default());
            }
        }
        for seed in 0u32..300 {
            let junk: Vec<u8> = (0..(seed % 120)).map(|i| (seed.wrapping_mul(2_654_435_761).wrapping_add(i * 97) >> 7) as u8).collect();
            let _ = Dot11::decode(&junk);
            let _ = Eapol::decode(&junk);
            let _ = rsn_akm(&junk);
        }
    }
}
