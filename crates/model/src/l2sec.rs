//! Switch edge protection and tuning: DHCP snooping, dynamic ARP inspection, spanning-tree edge
//! features (BPDU guard, root guard), errdisable recovery, per-port spanning-tree cost and
//! priority, EtherChannel load balancing and the SDM template.
//!
//! The switch-wide settings live in [`SwitchL2`] (`Device::l2`), the per-port ones in
//! [`PortL2`] (`Port::l2`).

use std::collections::{BTreeMap, BTreeSet};

use netlabcraft_core::{VlanId, VlanSet};
use serde::{Deserialize, Serialize};

/// Why a port went err-disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrCause {
    /// Port security saw one address too many (violation mode shutdown).
    PsecureViolation,
    /// A BPDU arrived on a port with BPDU guard.
    Bpduguard,
    /// More DHCP messages per second than `ip dhcp snooping limit rate` allows.
    DhcpRateLimit,
    /// More ARP messages per second than `ip arp inspection limit rate` allows.
    ArpInspection,
    /// Broadcast, multicast or unknown-unicast traffic above its `storm-control` level, with
    /// `storm-control action shutdown`.
    StormControl,
    /// UDLD found a link that works in one direction only.
    Udld,
}

impl ErrCause {
    pub const ALL: [ErrCause; 6] =
        [ErrCause::Bpduguard, ErrCause::DhcpRateLimit, ErrCause::ArpInspection, ErrCause::PsecureViolation, ErrCause::StormControl, ErrCause::Udld];

    /// The keyword `errdisable recovery cause` takes.
    pub fn name(self) -> &'static str {
        match self {
            ErrCause::PsecureViolation => "psecure-violation",
            ErrCause::Bpduguard => "bpduguard",
            ErrCause::DhcpRateLimit => "dhcp-rate-limit",
            ErrCause::ArpInspection => "arp-inspection",
            ErrCause::StormControl => "storm-control",
            ErrCause::Udld => "udld",
        }
    }

    pub fn parse(s: &str) -> Option<ErrCause> {
        let l = s.trim().to_ascii_lowercase();
        ErrCause::ALL.into_iter().find(|c| c.name() == l)
    }
}

/// How an EtherChannel picks the member a frame leaves by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadBalance {
    #[default]
    SrcMac,
    DstMac,
    SrcDstMac,
    SrcIp,
    DstIp,
    SrcDstIp,
}

impl LoadBalance {
    pub const ALL: [LoadBalance; 6] =
        [LoadBalance::SrcMac, LoadBalance::DstMac, LoadBalance::SrcDstMac, LoadBalance::SrcIp, LoadBalance::DstIp, LoadBalance::SrcDstIp];

    pub fn name(self) -> &'static str {
        match self {
            LoadBalance::SrcMac => "src-mac",
            LoadBalance::DstMac => "dst-mac",
            LoadBalance::SrcDstMac => "src-dst-mac",
            LoadBalance::SrcIp => "src-ip",
            LoadBalance::DstIp => "dst-ip",
            LoadBalance::SrcDstIp => "src-dst-ip",
        }
    }

    pub fn parse(s: &str) -> Option<LoadBalance> {
        let l = s.trim().to_ascii_lowercase();
        LoadBalance::ALL.into_iter().find(|m| m.name() == l)
    }
}

/// `ip arp inspection validate …`: extra checks on ARP messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArpValidate {
    /// The Ethernet source must be the ARP sender's MAC address.
    #[serde(default)]
    pub src_mac: bool,
    /// In replies, the Ethernet destination must be the ARP target's MAC address.
    #[serde(default)]
    pub dst_mac: bool,
    /// No broadcast, multicast or all-zero addresses where a host address belongs.
    #[serde(default)]
    pub ip: bool,
}

impl ArpValidate {
    pub fn any(&self) -> bool {
        self.src_mac || self.dst_mac || self.ip
    }
}

/// The SDM templates a switch can be told to prefer.
pub const SDM_TEMPLATES: [&str; 5] = ["default", "dual-ipv4-and-ipv6", "lanbase-routing", "vlan", "advanced"];

/// Default errdisable recovery interval (seconds).
pub const ERRDISABLE_INTERVAL: u32 = 300;

/// Switch-wide settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwitchL2 {
    /// `ip dhcp snooping`.
    pub snooping: bool,
    /// `ip dhcp snooping vlan …`.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub snooping_vlans: BTreeSet<VlanId>,
    /// `no ip dhcp snooping information option` turns this off (on by default).
    pub option82: bool,
    /// `ip arp inspection vlan …`.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub dai_vlans: BTreeSet<VlanId>,
    pub dai_validate: ArpValidate,
    /// `spanning-tree portfast default`: every access port is an edge port.
    pub portfast_default: bool,
    /// `spanning-tree portfast bpduguard default`: edge ports have BPDU guard.
    pub bpduguard_default: bool,
    /// `errdisable recovery cause …`.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub recovery: BTreeSet<ErrCause>,
    /// `errdisable recovery interval N` (seconds).
    pub recovery_interval: u32,
    /// `port-channel load-balance …`.
    pub load_balance: LoadBalance,
    /// `sdm prefer …`: the template the next boot uses (kept apart from the configuration).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdm_next: Option<String>,
    /// The template running now (`None`: default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdm_active: Option<String>,
    /// `mls qos`: the switch classifies frames on arrival; ports not trusted have their
    /// markings rewritten to 0.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mls_qos: bool,
    /// `spanning-tree loopguard default`: loop guard on every switch-to-switch port.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub loopguard_default: bool,
    /// `ip dhcp relay information trust-all` (routers): accept requests that carry relay
    /// information (option 82) with no relay address, from any interface.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub relay_trust_all: bool,
    /// `mls qos map cos-dscp D0 … D7`: the DSCP each CoS becomes on a port that trusts CoS
    /// (`None`: CoS × 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cos_dscp: Option<[u8; 8]>,
    /// `monitor session N …`: port mirroring.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub span: Vec<crate::span::SpanSession>,
    /// `ip source binding …`: bindings typed by hand, which IP source guard accepts like the
    /// ones DHCP snooping learned.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub static_bindings: Vec<StaticBinding>,
    /// `no errdisable detect cause …`: causes that no longer shut a port (the offending frame is
    /// still dropped).
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub detect_off: BTreeSet<ErrCause>,
    /// `udld enable` (normal) or `udld aggressive`: UDLD on every port that has no setting of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udld: Option<UdldMode>,
    /// `udld message time N`: seconds between UDLD probes.
    pub udld_interval: u32,
}

/// Seconds between UDLD probes unless `udld message time` says otherwise.
pub const UDLD_INTERVAL: u32 = 15;

impl Default for SwitchL2 {
    fn default() -> Self {
        SwitchL2 {
            snooping: false,
            snooping_vlans: BTreeSet::new(),
            option82: true,
            dai_vlans: BTreeSet::new(),
            dai_validate: ArpValidate::default(),
            portfast_default: false,
            bpduguard_default: false,
            recovery: BTreeSet::new(),
            recovery_interval: ERRDISABLE_INTERVAL,
            load_balance: LoadBalance::default(),
            sdm_next: None,
            sdm_active: None,
            mls_qos: false,
            loopguard_default: false,
            relay_trust_all: false,
            cos_dscp: None,
            span: Vec::new(),
            static_bindings: Vec::new(),
            detect_off: BTreeSet::new(),
            udld: None,
            udld_interval: UDLD_INTERVAL,
        }
    }
}

impl SwitchL2 {
    pub fn is_default(&self) -> bool {
        *self == SwitchL2::default()
    }

    /// Whether DHCP snooping acts on a VLAN.
    pub fn snoops(&self, vlan: VlanId) -> bool {
        self.snooping && self.snooping_vlans.contains(&vlan)
    }

    /// Whether ARP inspection acts on a VLAN.
    pub fn inspects(&self, vlan: VlanId) -> bool {
        self.dai_vlans.contains(&vlan)
    }

    /// The DSCP a CoS value maps to on a port that trusts CoS.
    pub fn cos_to_dscp(&self, cos: u8) -> u8 {
        let map = self.cos_dscp.unwrap_or(DEFAULT_COS_DSCP);
        map.get(usize::from(cos & 7)).copied().unwrap_or(0)
    }

    /// A monitor session by number, created (and kept in order) when it is not there yet.
    pub fn span_entry(&mut self, id: u8) -> Option<&mut crate::span::SpanSession> {
        if !self.span.iter().any(|s| s.id == id) {
            self.span.push(crate::span::SpanSession::new(id));
            self.span.sort_by_key(|s| s.id);
        }
        self.span.iter_mut().find(|s| s.id == id)
    }

    /// The session whose destination is this port.
    pub fn span_destination_of(&self, port: &str) -> Option<u8> {
        self.span.iter().find(|s| s.destination.as_deref() == Some(port)).map(|s| s.id)
    }

    /// A reload: the configuration goes back to factory; the SDM template chosen takes effect.
    pub fn reload(&mut self) {
        let next = self.sdm_next.clone();
        *self = SwitchL2 { sdm_active: next.clone(), sdm_next: next, ..SwitchL2::default() };
    }
}

/// A VLAN list in its compact form (`10,20-30`).
pub fn vlan_list(set: &BTreeSet<VlanId>) -> String {
    let mut s = VlanSet::none();
    for v in set {
        s.insert(*v);
    }
    s.to_string()
}

/// What IP source guard checks in a frame from an untrusted port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceGuard {
    /// The source address has a binding on this port (`ip verify source`).
    Ip,
    /// The source address and the source MAC address both match one binding
    /// (`ip verify source port-security`).
    IpMac,
}

impl SourceGuard {
    pub fn name(self) -> &'static str {
        match self {
            SourceGuard::Ip => "ip",
            SourceGuard::IpMac => "ip-mac",
        }
    }
}

/// `ip source binding MAC vlan N A.B.C.D interface X`: a binding typed by hand, for a host that
/// does not use DHCP.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StaticBinding {
    pub mac: netlabcraft_core::MacAddr,
    pub vlan: u16,
    pub ip: std::net::Ipv4Addr,
    /// The port, by index.
    pub port: usize,
}

/// Most static bindings one switch keeps.
pub const MAX_STATIC_BINDINGS: usize = 1_024;

/// `spanning-tree link-type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkType {
    PointToPoint,
    Shared,
}

impl LinkType {
    pub fn name(self) -> &'static str {
        match self {
            LinkType::PointToPoint => "point-to-point",
            LinkType::Shared => "shared",
        }
    }
}

/// `mls qos trust …`: which marking a port keeps when `mls qos` is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QosTrust {
    /// The IPv4 DS field is kept.
    Dscp,
    /// The 802.1p priority of a tagged frame is kept, and the DS field follows it (CoS × 8);
    /// untagged frames count as CoS 0.
    Cos,
}

impl QosTrust {
    pub fn name(self) -> &'static str {
        match self {
            QosTrust::Dscp => "dscp",
            QosTrust::Cos => "cos",
        }
    }
}

/// The usual CoS-to-DSCP map: each CoS to its class selector (CoS × 8).
pub const DEFAULT_COS_DSCP: [u8; 8] = [0, 8, 16, 24, 32, 40, 48, 56];

/// Spanning-tree port settings that may be per VLAN: key 0 is "every VLAN".
pub type PerVlan = BTreeMap<u16, u32>;

/// Per-port settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct PortL2 {
    /// `ip dhcp snooping trust`.
    pub snoop_trust: bool,
    /// `ip dhcp snooping limit rate N` (messages per second).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoop_rate: Option<u32>,
    /// `ip arp inspection trust`.
    pub dai_trust: bool,
    /// `ip arp inspection limit rate N` (messages per second).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dai_rate: Option<u32>,
    /// `spanning-tree bpduguard enable` (true) / `disable` (false); `None` follows the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpduguard: Option<bool>,
    /// `spanning-tree guard root`.
    pub root_guard: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_type: Option<LinkType>,
    /// `spanning-tree [vlan N] cost C`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stp_cost: PerVlan,
    /// `spanning-tree [vlan N] port-priority P`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stp_priority: PerVlan,
    /// `ip verify source [port-security]`: IP source guard on this port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_guard: Option<SourceGuard>,
    /// `spanning-tree mst N cost C`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub mst_cost: BTreeMap<u16, u32>,
    /// `spanning-tree mst N port-priority P`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub mst_priority: BTreeMap<u16, u32>,
    /// `lacp port-priority N`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lacp_priority: Option<u16>,
    /// Multilayer switches: `switchport trunk encapsulation dot1q`.
    pub encap_dot1q: bool,
    /// `mls qos trust dscp|cos`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qos_trust: Option<QosTrust>,
    /// `spanning-tree guard loop`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub loop_guard: bool,
    /// `spanning-tree bpdufilter enable` (true) / `disable` (false): the port neither sends
    /// nor takes BPDUs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpdufilter: Option<bool>,
    /// `ip dhcp relay information trusted` (router interfaces): requests with relay
    /// information and no relay address are taken here.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub relay_trusted: bool,
    /// Why the port is err-disabled (`None` with `err_disabled` set: port security).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub err_cause: Option<ErrCause>,
    /// `mls qos cos N`: the CoS untagged frames arriving here count as (0 when not set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qos_cos: Option<u8>,
    /// `switchport priority extend cos N`: the IP phone on this port gives the frames of the PC
    /// behind it this CoS (when not set, the PC's frames count as the port's own CoS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority_extend: Option<u8>,
    /// `switchport trunk pruning vlan …`: the VLANs VTP pruning may stop on this trunk (`None`:
    /// 2-1001).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prune_eligible: Option<netlabcraft_core::VlanSet>,
    /// `storm-control …`: levels per kind of traffic and what happens above them.
    #[serde(skip_serializing_if = "StormCtl::is_default")]
    pub storm: StormCtl,
    /// `udld port [aggressive]` (true: aggressive), `no udld port` (false); `None` follows the
    /// switch-wide setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udld: Option<UdldMode>,
}

/// The kinds of traffic `storm-control` measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StormKind {
    Broadcast,
    Multicast,
    /// Unicast frames to a destination the switch has not learned.
    Unicast,
}

impl StormKind {
    pub const ALL: [StormKind; 3] = [StormKind::Broadcast, StormKind::Multicast, StormKind::Unicast];

    pub fn name(self) -> &'static str {
        match self {
            StormKind::Broadcast => "broadcast",
            StormKind::Multicast => "multicast",
            StormKind::Unicast => "unicast",
        }
    }

    pub fn parse(s: &str) -> Option<StormKind> {
        let l = s.trim().to_ascii_lowercase();
        StormKind::ALL.into_iter().find(|k| k.name() == l)
    }
}

/// What a storm-control level is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StormUnit {
    /// Hundredths of a percent of the link's speed (2050 is 20.5 %).
    Percent,
    /// Bits per second.
    Bps,
    /// Packets per second.
    Pps,
}

/// One kind's `storm-control … level`: the rising threshold that starts the response and the
/// falling one that ends it (the rising one when none is given), in one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StormLevel {
    pub unit: StormUnit,
    pub rising: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falling: Option<u64>,
}

/// What happens above the level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StormAction {
    /// The excess is dropped until the traffic falls (the default).
    #[default]
    Filter,
    /// The port is err-disabled (cause `storm-control`).
    Shutdown,
    /// The excess is dropped and a message goes to the log.
    Trap,
}

impl StormAction {
    pub fn name(self) -> &'static str {
        match self {
            StormAction::Filter => "filter",
            StormAction::Shutdown => "shutdown",
            StormAction::Trap => "trap",
        }
    }
}

/// A port's `storm-control` settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct StormCtl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast: Option<StormLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multicast: Option<StormLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unicast: Option<StormLevel>,
    pub action: StormAction,
}

impl StormCtl {
    pub fn is_default(&self) -> bool {
        *self == StormCtl::default()
    }

    pub fn level(&self, kind: StormKind) -> Option<StormLevel> {
        match kind {
            StormKind::Broadcast => self.broadcast,
            StormKind::Multicast => self.multicast,
            StormKind::Unicast => self.unicast,
        }
    }

    pub fn set(&mut self, kind: StormKind, level: Option<StormLevel>) {
        match kind {
            StormKind::Broadcast => self.broadcast = level,
            StormKind::Multicast => self.multicast = level,
            StormKind::Unicast => self.unicast = level,
        }
    }
}

/// UDLD on a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UdldMode {
    /// Off on this port, whatever the switch says.
    Off,
    /// Probes are sent and heard; a link that is one-way is shut down.
    Normal,
    /// As normal, and a neighbour that was heard and goes silent also shuts the port down.
    Aggressive,
}

/// Writes a percent in hundredths the way it is typed (`40`, `20.5`, `0.25`).
pub fn percent_text(hundredths: u64) -> String {
    let whole = hundredths / 100;
    let frac = hundredths % 100;
    match (frac / 10, frac % 10) {
        (0, 0) => whole.to_string(),
        (t, 0) => format!("{whole}.{t}"),
        (t, u) => format!("{whole}.{t}{u}"),
    }
}

/// Reads a percent (`0` to `100`, up to two decimals) as hundredths.
pub fn parse_percent(text: &str) -> Option<u64> {
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || whole.len() > 3 || frac.len() > 2 || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let w: u64 = whole.parse().ok()?;
    let f: u64 = if frac.is_empty() { 0 } else { format!("{frac:0<2}").parse().ok()? };
    let v = w * 100 + f;
    (v <= 10_000).then_some(v)
}

impl StormLevel {
    /// The words after `level`, as typed: `40`, `20.5 10`, `bps 1000000 500000`, `pps 100`.
    pub fn text(&self) -> String {
        let one = |v: u64| match self.unit {
            StormUnit::Percent => percent_text(v),
            _ => v.to_string(),
        };
        let head = match self.unit {
            StormUnit::Percent => String::new(),
            StormUnit::Bps => "bps ".to_string(),
            StormUnit::Pps => "pps ".to_string(),
        };
        match self.falling {
            Some(f) => format!("{head}{} {}", one(self.rising), one(f)),
            None => format!("{head}{}", one(self.rising)),
        }
    }

    /// Reads the words after `level`.
    pub fn parse(words: &[&str]) -> Result<StormLevel, String> {
        let (unit, nums) = match words.first().map(|w| w.to_ascii_lowercase()).as_deref() {
            Some("bps") => (StormUnit::Bps, words.get(1..).unwrap_or(&[])),
            Some("pps") => (StormUnit::Pps, words.get(1..).unwrap_or(&[])),
            _ => (StormUnit::Percent, words),
        };
        let read = |w: &str| -> Result<u64, String> {
            match unit {
                StormUnit::Percent => parse_percent(w).ok_or_else(|| "The level is a percent from 0.00 to 100.00".to_string()),
                _ => w
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (1..=100_000_000_000).contains(n))
                    .ok_or_else(|| "The level is a number of bits or packets per second, from 1".to_string()),
            }
        };
        match nums {
            [r] => Ok(StormLevel { unit, rising: read(r)?, falling: None }),
            [r, f] => {
                let (rising, falling) = (read(r)?, read(f)?);
                if falling > rising {
                    return Err("The falling level cannot be above the rising one".into());
                }
                Ok(StormLevel { unit, rising, falling: Some(falling) })
            }
            _ => Err("Give one level, or a rising and a falling one".into()),
        }
    }
}

impl PortL2 {
    pub fn is_default(&self) -> bool {
        *self == PortL2::default()
    }

    /// The configured cost for a VLAN, if any (a per-VLAN value beats the general one).
    pub fn cost(&self, vlan: VlanId) -> Option<u32> {
        self.stp_cost.get(&vlan.get()).or_else(|| self.stp_cost.get(&0)).copied()
    }

    /// The port priority for a VLAN (128 unless set).
    pub fn priority(&self, vlan: VlanId) -> u32 {
        self.stp_priority.get(&vlan.get()).or_else(|| self.stp_priority.get(&0)).copied().unwrap_or(128)
    }
}

/// How port security forgets learned addresses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgingType {
    /// Forgotten a fixed time after being learned.
    #[default]
    Absolute,
    /// Forgotten after that long without traffic from them.
    Inactivity,
}

impl AgingType {
    pub fn name(self) -> &'static str {
        match self {
            AgingType::Absolute => "absolute",
            AgingType::Inactivity => "inactivity",
        }
    }
}
