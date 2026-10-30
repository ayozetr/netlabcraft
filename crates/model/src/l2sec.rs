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
}

impl ErrCause {
    pub const ALL: [ErrCause; 4] = [ErrCause::Bpduguard, ErrCause::DhcpRateLimit, ErrCause::ArpInspection, ErrCause::PsecureViolation];

    /// The keyword `errdisable recovery cause` takes.
    pub fn name(self) -> &'static str {
        match self {
            ErrCause::PsecureViolation => "psecure-violation",
            ErrCause::Bpduguard => "bpduguard",
            ErrCause::DhcpRateLimit => "dhcp-rate-limit",
            ErrCause::ArpInspection => "arp-inspection",
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
}

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
    /// `lacp port-priority N`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lacp_priority: Option<u16>,
    /// Multilayer switches: `switchport trunk encapsulation dot1q`.
    pub encap_dot1q: bool,
    /// Why the port is err-disabled (`None` with `err_disabled` set: port security).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub err_cause: Option<ErrCause>,
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
