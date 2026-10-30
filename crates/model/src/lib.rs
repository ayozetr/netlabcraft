//! PacketCraft L2: the lab document.
//!
//! A [`Topology`] holds devices (each with its ports and configuration) and the cables between
//! ports. It is plain data: the simulator (`packetcraft-sim`) reads it, the command line
//! (`packetcraft-ios`) and the engine change it. Saved as JSON (`.pcraft`, see [`file`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use packetcraft_core::{DeviceId, Ipv4Net, Ipv6Net, MacAddr, VlanId, VlanSet};
use serde::{Deserialize, Serialize};

pub mod acl;
pub mod activity;
pub mod file;
pub mod ifname;
pub mod models;
pub mod services;

/// Most devices a lab may hold (hostile files and scripts).
pub const MAX_DEVICES: usize = 2_000;
/// Longest hostname or description accepted.
pub const MAX_NAME: usize = 63;
pub const MAX_DESCRIPTION: usize = 240;
/// Most static routes per device.
pub const MAX_ROUTES: usize = 1_000;
/// Most subinterfaces per port.
pub const MAX_SUBIFS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("no device {0}")]
    NoDevice(String),
    #[error("{0} has no port {1}")]
    NoPort(String, String),
    #[error("{0} {1} is already cabled")]
    PortInUse(String, String),
    #[error("a cable needs two different ports")]
    SamePort,
    #[error("{0} has no free port")]
    NoFreePort(String),
    #[error("the lab is full ({MAX_DEVICES} devices)")]
    TooManyDevices,
    #[error("a device named {0} already exists")]
    NameTaken(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, ModelError>;

/// What a device is. The kind fixes its ports and which commands it understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    /// Three gigabit routed ports.
    Router,
    /// 24 fast + 2 gigabit switched ports.
    Switch,
    /// A repeater: every frame goes out of every other port.
    Hub,
    /// One fast port, a host.
    Pc,
    /// One fast port, a host.
    Laptop,
    /// One gigabit port, a host.
    Server,
    /// One fast port, a host.
    Printer,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 7] =
        [DeviceKind::Router, DeviceKind::Switch, DeviceKind::Hub, DeviceKind::Pc, DeviceKind::Laptop, DeviceKind::Server, DeviceKind::Printer];

    pub fn id(self) -> &'static str {
        match self {
            DeviceKind::Router => "router",
            DeviceKind::Switch => "switch",
            DeviceKind::Hub => "hub",
            DeviceKind::Pc => "pc",
            DeviceKind::Laptop => "laptop",
            DeviceKind::Server => "server",
            DeviceKind::Printer => "printer",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Router => "Router",
            DeviceKind::Switch => "Switch",
            DeviceKind::Hub => "Hub",
            DeviceKind::Pc => "PC",
            DeviceKind::Laptop => "Laptop",
            DeviceKind::Server => "Server",
            DeviceKind::Printer => "Printer",
        }
    }

    pub fn from_id(s: &str) -> Option<DeviceKind> {
        DeviceKind::ALL.into_iter().find(|k| k.id().eq_ignore_ascii_case(s) || k.label().eq_ignore_ascii_case(s))
    }

    /// The name prefix of new devices (`R1`, `S1`, `PC1`, `SRV1`).
    pub fn name_prefix(self) -> &'static str {
        match self {
            DeviceKind::Router => "R",
            DeviceKind::Switch => "S",
            DeviceKind::Hub => "HUB",
            DeviceKind::Pc => "PC",
            DeviceKind::Laptop => "LAPTOP",
            DeviceKind::Server => "SRV",
            DeviceKind::Printer => "PRN",
        }
    }

    /// Hosts run a desktop-style shell, have one port and a default gateway; they don't forward.
    pub fn is_host(self) -> bool {
        matches!(self, DeviceKind::Pc | DeviceKind::Laptop | DeviceKind::Server | DeviceKind::Printer)
    }

    /// The port names of a new device of this kind (its default model).
    pub fn port_names(self) -> Vec<String> {
        models::default_for(self).port_names()
    }

    /// Router ports start administratively down, as on real routers; everything else starts up.
    pub fn ports_start_shut(self) -> bool {
        self == DeviceKind::Router
    }
}

/// A switch port's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SwitchportMode {
    #[default]
    Access,
    Trunk,
    /// DTP: becomes a trunk if the other end asks (trunk or desirable).
    DynamicAuto,
    /// DTP: asks the other end to trunk (trunk, desirable or auto agree).
    DynamicDesirable,
}

impl SwitchportMode {
    pub fn name(self) -> &'static str {
        match self {
            SwitchportMode::Access => "access",
            SwitchportMode::Trunk => "trunk",
            SwitchportMode::DynamicAuto => "dynamic auto",
            SwitchportMode::DynamicDesirable => "dynamic desirable",
        }
    }
}

/// Layer-2 settings of a switch port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Switchport {
    pub mode: SwitchportMode,
    pub access_vlan: VlanId,
    pub native_vlan: VlanId,
    pub allowed: VlanSet,
    /// `switchport nonegotiate`: send no DTP messages.
    pub nonegotiate: bool,
}

impl Default for Switchport {
    fn default() -> Self {
        Switchport {
            mode: SwitchportMode::Access,
            access_vlan: VlanId::DEFAULT,
            native_vlan: VlanId::DEFAULT,
            allowed: VlanSet::all(),
            nonegotiate: false,
        }
    }
}

/// Port security of a switch port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortSecurity {
    /// Most MAC addresses the port accepts.
    pub max: u16,
    pub violation: Violation,
    /// `mac-address sticky`: learned addresses are written into the configuration.
    #[serde(default)]
    pub sticky: bool,
    /// Configured and sticky addresses.
    #[serde(default)]
    pub macs: Vec<MacAddr>,
}

impl Default for PortSecurity {
    fn default() -> Self {
        PortSecurity { max: 1, violation: Violation::Shutdown, sticky: false, macs: Vec::new() }
    }
}

/// VLANs VTP shares (the normal range); 1006-4094 only exist on transparent switches.
pub const VTP_MAX_VLAN: u16 = 1005;

/// A switch's VTP settings and its VLAN database revision. Like the VLAN database itself they
/// survive a reload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vtp {
    pub mode: VtpMode,
    /// Empty until set, or learned from a neighbour's advertisement.
    #[serde(default)]
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub version: u8,
    /// Goes up by one with every VLAN change on a server; the highest revision wins.
    #[serde(default)]
    pub revision: u32,
}

impl Default for Vtp {
    fn default() -> Self {
        Vtp { mode: VtpMode::Server, domain: String::new(), password: None, version: 1, revision: 0 }
    }
}

impl Vtp {
    pub fn is_default(&self) -> bool {
        *self == Vtp::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VtpMode {
    /// Creates VLANs and advertises them.
    Server,
    /// Takes its VLANs from the domain's advertisements; can't create any.
    Client,
    /// Keeps its own VLANs; passes advertisements on.
    Transparent,
}

impl VtpMode {
    pub fn name(self) -> &'static str {
        match self {
            VtpMode::Server => "server",
            VtpMode::Client => "client",
            VtpMode::Transparent => "transparent",
        }
    }
}

/// Most EtherChannel groups on a switch.
pub const MAX_CHANNEL_GROUP: u16 = 48;

/// `channel-group N mode M`: the port belongs to EtherChannel `number`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelGroup {
    pub number: u16,
    pub mode: ChannelMode,
}

/// How a port joins its EtherChannel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelMode {
    /// Bundled without negotiation; the other end must say `on` too.
    On,
    /// LACP, starts the negotiation.
    Active,
    /// LACP, answers an active end.
    Passive,
    /// PAgP, starts the negotiation.
    Desirable,
    /// PAgP, answers a desirable end.
    Auto,
}

impl ChannelMode {
    pub const ALL: [ChannelMode; 5] = [ChannelMode::On, ChannelMode::Active, ChannelMode::Passive, ChannelMode::Desirable, ChannelMode::Auto];

    pub fn name(self) -> &'static str {
        match self {
            ChannelMode::On => "on",
            ChannelMode::Active => "active",
            ChannelMode::Passive => "passive",
            ChannelMode::Desirable => "desirable",
            ChannelMode::Auto => "auto",
        }
    }

    pub fn parse(s: &str) -> Option<ChannelMode> {
        ChannelMode::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(s))
    }

    /// The negotiation protocol: LACP (IEEE 802.1AX), PAgP, or none.
    pub fn protocol(self) -> &'static str {
        match self {
            ChannelMode::On => "none",
            ChannelMode::Active | ChannelMode::Passive => "LACP",
            ChannelMode::Desirable | ChannelMode::Auto => "PAgP",
        }
    }

    /// Whether two ends in these modes form a bundle.
    pub fn bundles_with(self, other: ChannelMode) -> bool {
        use ChannelMode::*;
        matches!(
            (self, other),
            (On, On) | (Active, Active) | (Active, Passive) | (Passive, Active) | (Desirable, Desirable) | (Desirable, Auto) | (Auto, Desirable)
        )
    }
}

/// An HSRP group on an interface (`standby N …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Standby {
    pub group: u8,
    /// The virtual gateway address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Ipv4Addr>,
    /// 0-255, 100 by default; the highest is active.
    pub priority: u8,
    #[serde(default)]
    pub preempt: bool,
}

impl Standby {
    pub fn new(group: u8) -> Standby {
        Standby { group, ip: None, priority: 100, preempt: false }
    }
}

/// Most HSRP groups on one interface.
pub const MAX_STANDBY: usize = 16;

/// Wi-Fi settings of a radio port: on an access point the network it offers, on a host the
/// network it joins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wifi {
    pub ssid: String,
    /// WPA2-PSK passphrase; `None` is an open network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// 2.4 GHz channel (access points).
    #[serde(default = "default_channel")]
    pub channel: u8,
}

fn default_channel() -> u8 {
    6
}

/// How far (workspace units) a radio reaches.
pub const WIFI_RANGE: f32 = 450.0;

/// Layer-2 framing on a serial link.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encapsulation {
    /// Cisco-style HDLC, the default.
    #[default]
    Hdlc,
    /// PPP (RFC 1661).
    Ppp,
}

impl Encapsulation {
    pub fn name(self) -> &'static str {
        match self {
            Encapsulation::Hdlc => "HDLC",
            Encapsulation::Ppp => "PPP",
        }
    }
}

/// A serial port's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SerialConf {
    #[serde(default)]
    pub encapsulation: Encapsulation,
    /// `clock rate N` (bit/s): needed on the DCE end of the cable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_rate: Option<u32>,
    /// `ppp authentication …`: what this end demands from the other, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ppp_auth: Vec<PppAuth>,
    /// `ppp pap sent-username U password P`: what this end answers to PAP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pap_sent: Option<(String, String)>,
}

/// A PPP authentication protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PppAuth {
    /// RFC 1994: a challenge answered with a hash of the shared secret.
    Chap,
    /// RFC 1334: the name and password sent as they are.
    Pap,
}

impl PppAuth {
    pub fn name(self) -> &'static str {
        match self {
            PppAuth::Chap => "chap",
            PppAuth::Pap => "pap",
        }
    }
}

impl SerialConf {
    pub fn is_default(&self) -> bool {
        *self == SerialConf::default()
    }
}

/// Clock rates a DCE accepts (bit/s).
pub const CLOCK_RATES: [u32; 13] = [1_200, 2_400, 4_800, 9_600, 19_200, 38_400, 56_000, 64_000, 72_000, 125_000, 128_000, 148_000, 2_000_000];

/// What a port does when one address too many shows up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Violation {
    /// Drop it and shut the port down (err-disabled).
    Shutdown,
    /// Drop it and count it.
    Restrict,
    /// Drop it silently.
    Protect,
}

impl Violation {
    pub fn name(self) -> &'static str {
        match self {
            Violation::Shutdown => "shutdown",
            Violation::Restrict => "restrict",
            Violation::Protect => "protect",
        }
    }
}

/// A routed subinterface (`GigabitEthernet0/0.10`): one 802.1Q VLAN on a router port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubIf {
    pub number: u32,
    /// Set by `encapsulation dot1Q`; a subinterface without it carries no traffic.
    #[serde(default)]
    pub vlan: Option<VlanId>,
    #[serde(default)]
    pub ip: Option<Ipv4Net>,
    #[serde(default)]
    pub shutdown: bool,
    #[serde(default)]
    pub description: String,
    /// `ip helper-address`: DHCP servers to relay requests to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub helpers: Vec<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "AccessGroups::is_empty")]
    pub access: AccessGroups,
    /// `ip nat inside` / `ip nat outside`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nat: Option<NatSide>,
    /// HSRP groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standby: Vec<Standby>,
    #[serde(default, skip_serializing_if = "Ipv6Conf::is_default")]
    pub ipv6: Ipv6Conf,
}

/// One physical port and its configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub mac: MacAddr,
    #[serde(default)]
    pub shutdown: bool,
    #[serde(default)]
    pub description: String,
    /// The port's own address (routers and hosts).
    #[serde(default)]
    pub ip: Option<Ipv4Net>,
    /// Layer-2 settings (switches).
    #[serde(default)]
    pub switchport: Switchport,
    /// Routed subinterfaces (routers), sorted by number.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subifs: Vec<SubIf>,
    /// Switches: `spanning-tree portfast` (an edge port: forwards at once).
    #[serde(default)]
    pub portfast: bool,
    /// Switches: `switchport port-security`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_security: Option<PortSecurity>,
    #[serde(default, skip_serializing_if = "Ipv6Conf::is_default")]
    pub ipv6: Ipv6Conf,
    /// HSRP groups (routers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standby: Vec<Standby>,
    /// Radio ports: the Wi-Fi network offered (access points) or joined (hosts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Wifi>,
    /// Serial ports: encapsulation and clock rate.
    #[serde(default, skip_serializing_if = "SerialConf::is_default")]
    pub serial: SerialConf,
    /// `no cdp enable`.
    #[serde(default)]
    pub cdp_off: bool,
    /// `no lldp transmit`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lldp_tx_off: bool,
    /// `no lldp receive`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lldp_rx_off: bool,
    /// Multilayer switches: `no switchport` made this a routed port (it takes `ip address`).
    #[serde(default)]
    pub routed: bool,
    /// Switches: `channel-group N mode M`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelGroup>,
    /// Shut down by a port-security violation, until `shutdown` / `no shutdown`.
    #[serde(default)]
    pub err_disabled: bool,
    /// `ip helper-address`: DHCP servers to relay requests to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub helpers: Vec<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "AccessGroups::is_empty")]
    pub access: AccessGroups,
    /// `ip nat inside` / `ip nat outside`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nat: Option<NatSide>,
}

impl Port {
    pub fn subif(&self, number: u32) -> Option<&SubIf> {
        self.subifs.iter().find(|s| s.number == number)
    }

    /// The subinterface, created (unconfigured) if missing. `None` past [`MAX_SUBIFS`].
    pub fn subif_mut(&mut self, number: u32) -> Option<&mut SubIf> {
        if let Some(i) = self.subifs.iter().position(|s| s.number == number) {
            return self.subifs.get_mut(i);
        }
        if self.subifs.len() >= MAX_SUBIFS {
            return None;
        }
        let at = self.subifs.iter().position(|s| s.number > number).unwrap_or(self.subifs.len());
        self.subifs.insert(
            at,
            SubIf {
                number,
                vlan: None,
                ip: None,
                shutdown: false,
                description: String::new(),
                helpers: Vec::new(),
                access: AccessGroups::default(),
                nat: None,
                standby: Vec::new(),
                ipv6: Ipv6Conf::default(),
            },
        );
        self.subifs.get_mut(at)
    }
}

/// Where a static route sends traffic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NextHop {
    Address(Ipv4Addr),
    /// Out of an interface, by name (`GigabitEthernet0/1`, `GigabitEthernet0/0.10`).
    Interface(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StaticRoute {
    /// Network (host bits clear).
    pub net: Ipv4Net,
    pub via: NextHop,
}

/// An IPv6 static route: out of an interface, towards a next hop, or both (a link-local next
/// hop needs its interface).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StaticRoute6 {
    /// Network (host bits clear).
    pub net: Ipv6Net,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_hop: Option<Ipv6Addr>,
}

/// Most IPv6 addresses on one interface.
pub const MAX_IPV6_ADDRESSES: usize = 8;

/// A DHCPv6 pool (`ipv6 dhcp pool NAME`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dhcp6Pool {
    pub name: String,
    /// `address prefix X::/64`: where stateful addresses come from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<Ipv6Net>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dns: Vec<Ipv6Addr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// A port's IPv6 settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ipv6Conf {
    /// `ipv6 enable`: IPv6 with only a link-local address. Any address enables it too.
    #[serde(default)]
    pub enabled: bool,
    /// Global (and unique-local) addresses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addresses: Vec<Ipv6Net>,
    /// `ipv6 address X link-local`; otherwise the EUI-64 one from the MAC address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_local: Option<Ipv6Addr>,
    /// Hosts: addresses come from router advertisements (stateless autoconfiguration).
    #[serde(default)]
    pub autoconfig: bool,
    /// Routers: `ipv6 dhcp server POOL` on this interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcp_server: Option<String>,
    /// Routers: `ipv6 dhcp relay destination X`: DHCPv6 servers this interface's clients are
    /// relayed to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dhcp_relay: Vec<Ipv6Addr>,
    /// `ipv6 ospf PROCESS area N`: (process, area).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ospf: Option<(u16, u32)>,
    /// Routers: `ipv6 nd managed-config-flag` (hosts take addresses from DHCPv6).
    #[serde(default)]
    pub nd_managed: bool,
    /// Routers: `ipv6 nd other-config-flag` (hosts take DNS and domain from DHCPv6).
    #[serde(default)]
    pub nd_other: bool,
    /// `ipv6 traffic-filter NAME in` / `out`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_out: Option<String>,
}

impl Ipv6Conf {
    pub fn is_default(&self) -> bool {
        *self == Ipv6Conf::default()
    }

    pub fn active(&self) -> bool {
        self.enabled || self.autoconfig || !self.addresses.is_empty() || self.link_local.is_some()
    }
}

/// A device: identity, position on the canvas, ports, and its configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub kind: DeviceKind,
    /// The catalogue model (`models::MODELS`); empty in old files means the kind's default.
    #[serde(default)]
    pub model: String,
    /// The hostname; unique in the lab (case-insensitive).
    pub name: String,
    /// The workspace label ("Display Name"); the hostname when not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Canvas position (logical points).
    pub x: f32,
    pub y: f32,
    pub ports: Vec<Port>,
    /// Hosts: the default gateway.
    #[serde(default)]
    pub gateway: Option<Ipv4Addr>,
    /// Hosts: the IPv6 default gateway (often a router's link-local address).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway6: Option<Ipv6Addr>,
    /// Multilayer switches: `ip routing`.
    #[serde(default)]
    pub ip_routing: bool,
    /// Routers and switches: `ntp server A.B.C.D`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ntp_servers: Vec<Ipv4Addr>,
    /// Routers and switches: `logging host A.B.C.D`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log_hosts: Vec<Ipv4Addr>,
    /// Routers and switches: `no cdp run`.
    #[serde(default)]
    pub cdp_off: bool,
    /// Routers and switches: `lldp run` (off until turned on, as on most equipment).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lldp_on: bool,
    /// Routers: `ipv6 unicast-routing` (forward IPv6 and send router advertisements).
    #[serde(default)]
    pub ipv6_routing: bool,
    /// `ipv6 router ospf N`: OSPFv3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ospf6: Option<Ospf6>,
    /// Routers: IPv6 static routes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes6: Vec<StaticRoute6>,
    /// Routers: static routes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<StaticRoute>,
    /// Switches: the VLAN database (id → name). VLAN 1 always exists.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vlans: BTreeMap<VlanId, String>,
    /// Switches: VTP.
    #[serde(default, skip_serializing_if = "Vtp::is_default")]
    pub vtp: Vtp,
    /// Switches: `spanning-tree mode rapid-pvst` (ports forward at once instead of listening
    /// and learning for 30 s).
    #[serde(default)]
    pub rapid_stp: bool,
    /// The saved configuration (`copy running-config startup-config`), as command lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<String>,
    /// Routers: DHCP pools (`ip dhcp pool`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dhcp_pools: Vec<DhcpPool>,
    /// Routers: addresses DHCP never hands out (`ip dhcp excluded-address`), inclusive ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dhcp_excluded: Vec<(Ipv4Addr, Ipv4Addr)>,
    /// Hosts: the address came from DHCP.
    #[serde(default)]
    pub dhcp_client: bool,
    /// Hosts: the DNS server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<Ipv4Addr>,
    /// Routers and switches: access and management settings.
    #[serde(default)]
    pub access: Access,
    /// Switches: spanning-tree bridge priority per VLAN (`spanning-tree vlan N priority P`);
    /// VLANs not listed use 32768.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stp_priority: BTreeMap<VlanId, u16>,
    /// Switches: VLAN interfaces (`interface vlan N`), for management.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub svis: Vec<Svi>,
    /// Routers: IPv6 access lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acls6: Vec<acl::Acl6>,
    /// Routers: OSPF (`router ospf N`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ospf: Option<Ospf>,
    /// Routers: DHCPv6 pools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dhcp6_pools: Vec<Dhcp6Pool>,
    /// Hosts: the IPv6 DNS server learned from DHCPv6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns6: Option<Ipv6Addr>,
    /// Per-interface OSPF settings, by interface name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ospf_if: BTreeMap<String, OspfIf>,
    /// Routers: RIP (`router rip`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rip: Option<Rip>,
    /// Routers: access lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acls: Vec<acl::Acl>,
    /// Routers: network address translation.
    #[serde(default, skip_serializing_if = "Nat::is_empty")]
    pub nat: Nat,
    /// Servers: HTTP, DNS and DHCP services.
    #[serde(default, skip_serializing_if = "services::Services::is_default")]
    pub services: services::Services,
}

/// Which side of NAT an interface is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NatSide {
    Inside,
    Outside,
}

/// A router's NAT configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Nat {
    /// `ip nat inside source static <local> <global>`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statics: Vec<(Ipv4Addr, Ipv4Addr)>,
    /// `ip nat inside source list …`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<NatRule>,
    /// `ip nat pool …`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pools: Vec<NatPool>,
}

impl Nat {
    pub fn is_empty(&self) -> bool {
        self.statics.is_empty() && self.rules.is_empty() && self.pools.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NatRule {
    /// The access list choosing which inside addresses are translated.
    pub acl: String,
    pub target: NatTarget,
    /// Port address translation: many inside hosts share one address.
    #[serde(default)]
    pub overload: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NatTarget {
    Pool(String),
    /// An interface's own address.
    Interface(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NatPool {
    pub name: String,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
    pub prefix: u8,
}

/// The access lists applied to an interface (`ip access-group`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AccessGroups {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbound: Option<String>,
}

impl AccessGroups {
    pub fn is_empty(&self) -> bool {
        self.inbound.is_none() && self.outbound.is_none()
    }
}

/// A router's OSPF process.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ospf {
    pub process: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_id: Option<Ipv4Addr>,
    /// `network <address> <wildcard> area <n>`.
    #[serde(default)]
    pub networks: Vec<OspfNetwork>,
    /// Interfaces that advertise their network but form no neighbours.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passive: Vec<String>,
    /// `default-information originate`: advertise this router's default route.
    #[serde(default)]
    pub default_originate: bool,
    /// `auto-cost reference-bandwidth N` (Mbit/s; 100 when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_bw: Option<u32>,
    /// Area settings (`area N stub`, `area N range`), by area number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub areas: BTreeMap<u32, OspfArea>,
}

/// A router's OSPFv3 process (RFC 5340): interfaces join it with `ipv6 ospf N area A`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ospf6 {
    pub process: u16,
    /// A 32-bit router ID, as in OSPFv2; without one, the highest IPv4 interface address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_id: Option<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passive: Vec<String>,
    /// `default-information originate`: advertise this router's IPv6 default route.
    #[serde(default)]
    pub default_originate: bool,
}

/// Most `area range` lines per area.
pub const MAX_AREA_RANGES: usize = 64;

/// One area's settings on a router.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OspfArea {
    /// `area N stub`: no external routes come in; the border router sends a default instead.
    #[serde(default)]
    pub stub: bool,
    /// `area N stub no-summary` (on the border router): routes from other areas stay out too.
    #[serde(default)]
    pub no_summary: bool,
    /// `area N range`: the border router advertises these instead of the area's networks
    /// inside them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<Ipv4Net>,
}

impl OspfArea {
    pub fn is_default(&self) -> bool {
        *self == OspfArea::default()
    }
}

impl Ospf {
    /// An area's settings (defaults when none were typed).
    pub fn area(&self, n: u32) -> OspfArea {
        self.areas.get(&n).cloned().unwrap_or_default()
    }
}

/// OSPF settings of one interface (`ip ospf cost`, `ip ospf priority`), kept apart from the
/// process as on real routers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OspfIf {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
}

/// `router rip`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rip {
    /// 1 or 2 (both behave classless here; see the simulator).
    pub version: u8,
    /// `network <address>`: classful networks whose interfaces run RIP.
    #[serde(default)]
    pub networks: Vec<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passive: Vec<String>,
    #[serde(default)]
    pub default_originate: bool,
    #[serde(default)]
    pub auto_summary: bool,
}

impl Default for Rip {
    fn default() -> Self {
        Rip { version: 1, networks: Vec::new(), passive: Vec::new(), default_originate: false, auto_summary: false }
    }
}

impl Rip {
    /// The classful network of an address (class A /8, B /16, C /24).
    pub fn classful(a: Ipv4Addr) -> Ipv4Net {
        let first = a.octets()[0];
        let prefix = if first < 128 {
            8
        } else if first < 192 {
            16
        } else {
            24
        };
        Ipv4Net::new(a, prefix).network()
    }

    /// Whether an interface address falls under a `network` statement.
    pub fn covers(&self, ip: Ipv4Addr) -> bool {
        self.networks.iter().any(|n| Rip::classful(*n).contains(ip))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OspfNetwork {
    pub address: Ipv4Addr,
    pub wildcard: Ipv4Addr,
    pub area: u32,
}

impl OspfNetwork {
    /// Whether an interface address falls under this statement.
    pub fn matches(&self, ip: Ipv4Addr) -> bool {
        let keep = !u32::from(self.wildcard);
        u32::from(ip) & keep == u32::from(self.address) & keep
    }
}

/// A switch's VLAN interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Svi {
    pub vlan: VlanId,
    #[serde(default)]
    pub ip: Option<Ipv4Net>,
    /// HSRP groups (multilayer switches).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standby: Vec<Standby>,
    /// New VLAN interfaces start shut down, as on real switches.
    #[serde(default)]
    pub shutdown: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Ipv6Conf::is_default")]
    pub ipv6: Ipv6Conf,
}

impl Device {
    /// The VLAN interface, created (shut down, no address) if missing.
    pub fn svi_mut(&mut self, vlan: VlanId) -> Option<&mut Svi> {
        if !self.svis.iter().any(|s| s.vlan == vlan) {
            if self.svis.len() >= MAX_SUBIFS {
                return None;
            }
            self.svis.push(Svi { vlan, ip: None, standby: Vec::new(), shutdown: true, description: String::new(), ipv6: Ipv6Conf::default() });
            self.svis.sort_by_key(|s| s.vlan);
        }
        self.svis.iter_mut().find(|s| s.vlan == vlan)
    }
}

/// The default spanning-tree bridge priority.
pub const STP_DEFAULT_PRIORITY: u16 = 32_768;

/// A DHCP pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DhcpPool {
    pub name: String,
    #[serde(default)]
    pub network: Option<Ipv4Net>,
    #[serde(default)]
    pub default_router: Option<Ipv4Addr>,
    #[serde(default)]
    pub dns: Option<Ipv4Addr>,
}

/// Most DHCP pools and excluded ranges per device.
pub const MAX_DHCP_POOLS: usize = 64;

/// A console or vty line.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Line {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub login: bool,
    /// `login local`: ask for a username and check it against `username` lines.
    #[serde(default)]
    pub login_local: bool,
    /// `transport input`: the protocols remote logins may use (`None`: all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Vec<RemoteProto>>,
    /// `access-class N in`: who may log in (vty lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_class: Option<String>,
}

/// A remote login protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteProto {
    Telnet,
    Ssh,
}

impl RemoteProto {
    pub fn port(self) -> u16 {
        match self {
            RemoteProto::Telnet => 23,
            RemoteProto::Ssh => 22,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RemoteProto::Telnet => "telnet",
            RemoteProto::Ssh => "ssh",
        }
    }
}

/// A local user (`username … secret|password …`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    /// A [`secret_hash`] (`secret`), or the password itself (`password`).
    pub credential: String,
    #[serde(default)]
    pub hashed: bool,
}

impl User {
    pub fn matches(&self, typed: &str) -> bool {
        if self.hashed { self.credential == secret_hash(typed) } else { self.credential == typed }
    }

    /// Whether two users hold the same secret (CHAP's shared secret), however each is stored.
    pub fn same_secret(&self, other: &User) -> bool {
        match (self.hashed, other.hashed) {
            (false, _) => other.matches(&self.credential),
            (true, false) => self.matches(&other.credential),
            (true, true) => self.credential == other.credential,
        }
    }
}

/// Passwords, lines and banner of a router or switch.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Access {
    /// `enable secret`: a hash (see [`secret_hash`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_secret: Option<String>,
    /// `enable password`: plain text (written obscured with password encryption on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_password: Option<String>,
    #[serde(default)]
    pub console: Line,
    #[serde(default)]
    pub vty: Line,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub banner_motd: String,
    #[serde(default)]
    pub password_encryption: bool,
    /// `no ip domain-lookup`.
    #[serde(default)]
    pub no_domain_lookup: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<User>,
    /// `ip domain-name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_name: Option<String>,
    /// The RSA key size, once `crypto key generate rsa` ran (SSH needs it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rsa_bits: Option<u16>,
    /// `ip ssh version 2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_version: Option<u8>,
}

impl Access {
    /// Whether the vty lines accept a login over this protocol right now.
    pub fn accepts(&self, p: RemoteProto) -> bool {
        let v = &self.vty;
        let allowed = v.transport.as_ref().is_none_or(|t| t.contains(&p));
        let auth = (v.login_local && !self.users.is_empty()) || (v.login && v.password.is_some());
        let ssh_ready =
            p == RemoteProto::Telnet || (self.domain_name.is_some() && self.rsa_bits.is_some() && v.login_local && !self.users.is_empty());
        allowed && auth && ssh_ready
    }
}

/// PacketCraft's one-way hash for `enable secret` (64-bit FNV-1a, salted, iterated). Labs only:
/// it keeps the secret out of the configuration text; it is not meant to resist attack.
pub fn secret_hash(plain: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for _ in 0..1000 {
        for b in "packetcraft:".bytes().chain(plain.bytes()) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("$pc${h:016x}")
}

/// `service password-encryption`'s reversible obscuring of line passwords (our own scheme).
pub fn obscure(plain: &str) -> String {
    plain.bytes().enumerate().map(|(i, b)| format!("{:02X}", b ^ 0x5a_u8.wrapping_add((i as u8).wrapping_mul(7)))).collect()
}

/// Reverses [`obscure`]; `None` for text that isn't obscured output.
pub fn unobscure(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) || !text.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..text.len() / 2)
        .map(|i| u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok().map(|b| b ^ 0x5a_u8.wrapping_add((i as u8).wrapping_mul(7))))
        .collect();
    String::from_utf8(bytes?).ok()
}

impl Device {
    /// A factory-fresh device of the kind's default model.
    pub fn new(id: DeviceId, kind: DeviceKind, name: String, x: f32, y: f32) -> Device {
        Device::of_model(id, models::default_for(kind), name, x, y)
    }

    /// A factory-fresh device of a catalogue model.
    pub fn of_model(id: DeviceId, model: &models::Model, name: String, x: f32, y: f32) -> Device {
        let kind = model.kind;
        let ports = model
            .port_names()
            .into_iter()
            .enumerate()
            .map(|(i, name)| Port {
                name,
                mac: MacAddr::local(id, u16::try_from(i).unwrap_or(u16::MAX)),
                shutdown: kind.ports_start_shut(),
                description: String::new(),
                ip: None,
                switchport: Switchport::default(),
                subifs: Vec::new(),
                portfast: false,
                port_security: None,
                ipv6: Ipv6Conf::default(),
                channel: None,
                serial: SerialConf::default(),
                standby: Vec::new(),
                wifi: None,
                cdp_off: false,
                lldp_tx_off: false,
                lldp_rx_off: false,
                routed: false,
                err_disabled: false,
                helpers: Vec::new(),
                access: AccessGroups::default(),
                nat: None,
            })
            .collect();
        let mut d = Device {
            id,
            kind,
            model: model.id.to_string(),
            name,
            label: None,
            x,
            y,
            ports,
            gateway: None,
            gateway6: None,
            ip_routing: false,
            cdp_off: false,
            lldp_on: false,
            ntp_servers: Vec::new(),
            log_hosts: Vec::new(),
            ipv6_routing: false,
            ospf6: None,
            routes6: Vec::new(),
            routes: Vec::new(),
            vlans: BTreeMap::new(),
            vtp: Vtp::default(),
            rapid_stp: false,
            startup: None,
            dhcp_pools: Vec::new(),
            dhcp_excluded: Vec::new(),
            dhcp_client: false,
            dns: None,
            access: Access::default(),
            stp_priority: BTreeMap::new(),
            svis: Vec::new(),
            ospf: None,
            acls6: Vec::new(),
            ospf_if: BTreeMap::new(),
            dhcp6_pools: Vec::new(),
            dns6: None,
            rip: None,
            acls: Vec::new(),
            nat: Nat::default(),
            services: if kind == DeviceKind::Server { services::Services::server_defaults() } else { services::Services::default() },
        };
        d.ensure_default_vlan();
        d
    }

    /// Back to factory configuration, keeping identity, position and MACs.
    pub fn reset_config(&mut self) {
        let fresh = Device::of_model(self.id, self.catalogue_model(), self.name.clone(), self.x, self.y);
        for (p, f) in self.ports.iter_mut().zip(fresh.ports) {
            *p = Port { mac: p.mac, ..f };
        }
        self.gateway = None;
        self.gateway6 = None;
        self.ip_routing = false;
        self.cdp_off = false;
        self.lldp_on = false;
        self.ntp_servers.clear();
        self.log_hosts.clear();
        self.ipv6_routing = false;
        self.ospf6 = None;
        self.routes6.clear();
        self.routes.clear();
        // The VLAN database and VTP settings live apart from the configuration (as in a
        // switch's flash) and survive a reload.
        self.dhcp_pools.clear();
        self.dhcp_excluded.clear();
        self.dhcp_client = false;
        self.dns = None;
        self.access = Access::default();
        self.stp_priority.clear();
        self.svis.clear();
        self.ospf = None;
        self.rip = None;
        self.acls6.clear();
        self.ospf_if.clear();
        self.dhcp6_pools.clear();
        self.dns6 = None;
        self.acls.clear();
        self.nat = Nat::default();
        self.services = fresh.services;
    }

    /// What the workspace shows under the icon.
    pub fn display_name(&self) -> &str {
        self.label.as_deref().filter(|l| !l.trim().is_empty()).unwrap_or(&self.name)
    }

    /// Whether this device forwards IPv4 between its interfaces: a router, or a multilayer
    /// switch with `ip routing`.
    pub fn routes_ip(&self) -> bool {
        self.kind == DeviceKind::Router || (self.kind == DeviceKind::Switch && self.ip_routing)
    }

    /// The catalogue entry of this device (the kind's default if the id is unknown).
    pub fn catalogue_model(&self) -> &'static models::Model {
        models::find(&self.model).filter(|m| m.kind == self.kind).unwrap_or_else(|| models::default_for(self.kind))
    }

    pub fn ensure_default_vlan(&mut self) {
        if self.kind == DeviceKind::Switch {
            self.vlans.entry(VlanId::DEFAULT).or_insert_with(|| "default".into());
        }
    }

    /// The port named `name`, accepting the usual abbreviations (`g0/1`, `fa 0/3`).
    pub fn port_index(&self, name: &str) -> Option<usize> {
        let target = ifname::parse(name)?;
        if target.sub.is_some() {
            return None;
        }
        self.ports.iter().position(|p| ifname::parse(&p.name).is_some_and(|n| n.same_port(&target)))
    }

    /// The default name a created VLAN gets (`VLAN0010`).
    pub fn default_vlan_name(v: VlanId) -> String {
        format!("VLAN{:04}", v.get())
    }
}

/// One end of a cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct End {
    pub device: DeviceId,
    pub port: usize,
}

/// A cable between two ports. The simulator decides straight vs crossover itself (every port
/// is auto-MDIX), so a cable is just a pair of ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub a: End,
    pub b: End,
}

/// The lab.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Topology {
    pub devices: Vec<Device>,
    pub links: Vec<Link>,
    /// The next device id (ids are never reused).
    pub next_id: u32,
    /// Free text shown with the lab (instructions, notes).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Notes placed on the workspace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    /// A graded activity: instructions and the results to reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<activity::Activity>,
}

/// A note or drawing placed on the workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub shape: Shape,
    /// Size (rectangle, ellipse) or the offset to the far end (line).
    #[serde(default)]
    pub w: f32,
    #[serde(default)]
    pub h: f32,
    /// Fill or stroke colour, `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// What an annotation draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    #[default]
    Text,
    Rectangle,
    Ellipse,
    Line,
}

impl Shape {
    pub fn from_id(s: &str) -> Option<Shape> {
        Some(match s {
            "text" | "note" => Shape::Text,
            "rectangle" | "rect" => Shape::Rectangle,
            "ellipse" => Shape::Ellipse,
            "line" => Shape::Line,
            _ => return None,
        })
    }
}

/// Parses `#rrggbb`.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([v(0)?, v(2)?, v(4)?])
}

/// Most workspace notes, and the longest note.
pub const MAX_ANNOTATIONS: usize = 1_000;
pub const MAX_ANNOTATION_TEXT: usize = 4_000;

impl Topology {
    pub fn new() -> Topology {
        Topology { devices: Vec::new(), links: Vec::new(), next_id: 1, notes: String::new(), annotations: Vec::new(), activity: None }
    }

    pub fn device(&self, id: DeviceId) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn device_mut(&mut self, id: DeviceId) -> Option<&mut Device> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    /// By hostname (case-insensitive) or `#id`.
    pub fn find(&self, name_or_id: &str) -> Option<&Device> {
        let s = name_or_id.trim();
        if let Some(n) = s.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()) {
            return self.device(DeviceId(n));
        }
        self.devices.iter().find(|d| d.name.eq_ignore_ascii_case(s))
    }

    pub fn resolve(&self, name_or_id: &str) -> Result<DeviceId> {
        self.find(name_or_id).map(|d| d.id).ok_or_else(|| ModelError::NoDevice(name_or_id.to_string()))
    }

    /// Whether `name` is free for `except` (a device keeping its own name is fine).
    pub fn name_free(&self, name: &str, except: Option<DeviceId>) -> bool {
        !self.devices.iter().any(|d| Some(d.id) != except && d.name.eq_ignore_ascii_case(name))
    }

    fn next_name(&self, kind: DeviceKind) -> String {
        (1..).map(|n| format!("{}{n}", kind.name_prefix())).find(|n| self.name_free(n, None)).unwrap_or_default()
    }

    /// Adds a factory-fresh device; `name` defaults to the next free `R1`/`S1`/`PC1`.
    pub fn add_device(&mut self, kind: DeviceKind, name: Option<&str>, x: f32, y: f32) -> Result<DeviceId> {
        self.add_model(models::default_for(kind), name, x, y)
    }

    /// Adds a factory-fresh device of a catalogue model.
    pub fn add_model(&mut self, model: &models::Model, name: Option<&str>, x: f32, y: f32) -> Result<DeviceId> {
        let kind = model.kind;
        if self.devices.len() >= MAX_DEVICES {
            return Err(ModelError::TooManyDevices);
        }
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => {
                validate_hostname(n)?;
                if !self.name_free(n, None) {
                    return Err(ModelError::NameTaken(n.to_string()));
                }
                n.to_string()
            }
            None => self.next_name(kind),
        };
        let id = DeviceId(self.next_id.max(1));
        self.next_id = id.0.saturating_add(1);
        let (x, y) = (finite(x), finite(y));
        self.devices.push(Device::of_model(id, model, name, x, y));
        Ok(id)
    }

    /// Removes a device and its cables.
    pub fn remove_device(&mut self, id: DeviceId) -> Result<Device> {
        let i = self.devices.iter().position(|d| d.id == id).ok_or_else(|| ModelError::NoDevice(id.to_string()))?;
        self.links.retain(|l| l.a.device != id && l.b.device != id);
        Ok(self.devices.remove(i))
    }

    pub fn rename(&mut self, id: DeviceId, name: &str) -> Result<()> {
        let name = name.trim();
        validate_hostname(name)?;
        if !self.name_free(name, Some(id)) {
            return Err(ModelError::NameTaken(name.to_string()));
        }
        self.device_mut(id).ok_or_else(|| ModelError::NoDevice(id.to_string()))?.name = name.to_string();
        Ok(())
    }

    /// Adds a workspace note; returns its id.
    pub fn add_annotation(&mut self, x: f32, y: f32, text: &str) -> Result<u32> {
        if self.annotations.len() >= MAX_ANNOTATIONS {
            return Err(ModelError::Invalid(format!("at most {MAX_ANNOTATIONS} notes")));
        }
        let id = self.annotations.iter().map(|a| a.id).max().unwrap_or(0).saturating_add(1);
        self.annotations.push(Annotation {
            id,
            x: finite(x),
            y: finite(y),
            text: text.chars().take(MAX_ANNOTATION_TEXT).collect(),
            shape: Shape::Text,
            w: 0.0,
            h: 0.0,
            color: None,
        });
        Ok(id)
    }

    /// Adds a drawing (rectangle, ellipse or line); returns its id.
    pub fn add_drawing(&mut self, shape: Shape, x: f32, y: f32, w: f32, h: f32, color: Option<&str>) -> Result<u32> {
        let id = self.add_annotation(x, y, "")?;
        let color = color.filter(|c| parse_color(c).is_some()).map(str::to_string);
        if let Ok(a) = self.annotation_mut(id) {
            a.shape = shape;
            a.w = finite(w);
            a.h = finite(h);
            a.color = color;
        }
        Ok(id)
    }

    pub fn annotation_mut(&mut self, id: u32) -> Result<&mut Annotation> {
        self.annotations.iter_mut().find(|a| a.id == id).ok_or_else(|| ModelError::Invalid(format!("no note {id}")))
    }

    /// The cable plugged into a port.
    pub fn link_at(&self, end: End) -> Option<&Link> {
        self.links.iter().find(|l| l.a == end || l.b == end)
    }

    /// The far end of the cable plugged into a port.
    /// The port at the other end: of the cable, or for a Wi-Fi host the access point's radio.
    pub fn peer(&self, end: End) -> Option<End> {
        self.link_at(end).map(|l| if l.a == end { l.b } else { l.a }).or_else(|| self.wifi_ap_of(end))
    }

    fn radio(&self, end: End) -> Option<(&Device, &Port, &Wifi)> {
        let d = self.device(end.device)?;
        let p = d.ports.get(end.port).filter(|p| ifname::is_wireless(&p.name) && !p.shutdown)?;
        Some((d, p, p.wifi.as_ref()?))
    }

    /// The access point a Wi-Fi host is associated with: the nearest one in range offering
    /// the same SSID with the same key.
    pub fn wifi_ap_of(&self, client: End) -> Option<End> {
        let (cd, _, want) = self.radio(client)?;
        if !cd.kind.is_host() {
            return None;
        }
        self.devices
            .iter()
            .filter(|d| d.kind == DeviceKind::Switch)
            .flat_map(|d| (0..d.ports.len()).map(move |p| End { device: d.id, port: p }))
            .filter_map(|e| {
                let (ad, _, offer) = self.radio(e)?;
                let dist = ((ad.x - cd.x).powi(2) + (ad.y - cd.y).powi(2)).sqrt();
                (offer.ssid == want.ssid && offer.key == want.key && !offer.ssid.is_empty() && dist <= WIFI_RANGE).then_some((dist, e))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, e)| e)
    }

    /// The hosts associated with an access point's radio.
    pub fn wifi_clients(&self, ap: End) -> Vec<End> {
        if self.radio(ap).is_none_or(|(d, ..)| d.kind != DeviceKind::Switch) {
            return Vec::new();
        }
        self.devices
            .iter()
            .filter(|d| d.kind.is_host())
            .flat_map(|d| (0..d.ports.len()).map(move |p| End { device: d.id, port: p }))
            .filter(|e| self.wifi_ap_of(*e) == Some(ap))
            .collect()
    }

    /// The first port of a device with no cable.
    pub fn free_port(&self, id: DeviceId) -> Option<usize> {
        let d = self.device(id)?;
        (0..d.ports.len()).find(|&p| self.link_at(End { device: id, port: p }).is_none())
    }

    /// The first free port of a device that a cable from a serial (or Ethernet) port fits.
    pub fn free_port_for(&self, id: DeviceId, serial: bool) -> Option<usize> {
        let d = self.device(id)?;
        (0..d.ports.len())
            .find(|&p| self.link_at(End { device: id, port: p }).is_none() && d.ports.get(p).is_some_and(|x| ifname::is_serial(&x.name) == serial))
    }

    /// Whether a switch port runs as a trunk: configured so, or negotiated with DTP (desirable
    /// agrees with trunk, desirable or auto; auto only with trunk or desirable; nothing
    /// negotiates with a port that sends no DTP).
    pub fn is_trunk(&self, end: End) -> bool {
        let port = |e: End| self.device(e.device).filter(|d| d.kind == DeviceKind::Switch).and_then(|d| d.ports.get(e.port)).filter(|p| !p.routed);
        let Some(p) = port(end) else { return false };
        let mine = p.switchport.mode;
        match mine {
            SwitchportMode::Trunk => true,
            SwitchportMode::Access => false,
            SwitchportMode::DynamicAuto | SwitchportMode::DynamicDesirable => {
                let Some(q) = self.peer(end).and_then(port) else { return false };
                if q.switchport.nonegotiate {
                    return false;
                }
                matches!(
                    (mine, q.switchport.mode),
                    (SwitchportMode::DynamicDesirable, SwitchportMode::Trunk | SwitchportMode::DynamicDesirable | SwitchportMode::DynamicAuto)
                        | (SwitchportMode::DynamicAuto, SwitchportMode::Trunk | SwitchportMode::DynamicDesirable)
                )
            }
        }
    }

    /// Whether a port is serial.
    pub fn is_serial(&self, end: End) -> bool {
        self.device(end.device).and_then(|d| d.ports.get(end.port)).is_some_and(|p| ifname::is_serial(&p.name))
    }

    /// On a serial cable, the DCE end (the one cabled first, which provides the clock).
    pub fn dce_of(&self, end: End) -> Option<End> {
        let l = self.link_at(end)?;
        self.is_serial(l.a).then_some(l.a)
    }

    fn check_end(&self, end: End) -> Result<()> {
        let d = self.device(end.device).ok_or_else(|| ModelError::NoDevice(end.device.to_string()))?;
        let p = d.ports.get(end.port).ok_or_else(|| ModelError::NoPort(d.name.clone(), format!("#{}", end.port)))?;
        if self.link_at(end).is_some() {
            return Err(ModelError::PortInUse(d.name.clone(), p.name.clone()));
        }
        Ok(())
    }

    /// Cables two free ports.
    pub fn connect(&mut self, a: End, b: End) -> Result<()> {
        if a == b {
            return Err(ModelError::SamePort);
        }
        self.check_end(a)?;
        self.check_end(b)?;
        if self.is_serial(a) != self.is_serial(b) {
            return Err(ModelError::Invalid(format!(
                "{} and {} don't fit one cable: serial ports take a serial cable, Ethernet ports a copper one",
                self.end_label(a),
                self.end_label(b)
            )));
        }
        // On a serial cable the first end is the DCE.
        self.links.push(Link { a, b });
        Ok(())
    }

    /// Unplugs the cable at a port; returns it.
    pub fn disconnect(&mut self, end: End) -> Option<Link> {
        let i = self.links.iter().position(|l| l.a == end || l.b == end)?;
        Some(self.links.remove(i))
    }

    /// `Device Port` for messages.
    pub fn end_label(&self, end: End) -> String {
        match self.device(end.device) {
            Some(d) => format!("{} {}", d.name, d.ports.get(end.port).map_or("?", |p| p.name.as_str())),
            None => format!("{} ?", end.device),
        }
    }

    /// Repairs what a hand-edited or hostile file could break: dangling or duplicate cables,
    /// duplicate names and ids, non-finite positions, missing VLAN 1, oversized lists.
    pub fn sanitize(&mut self) {
        if let Some(a) = &mut self.activity {
            a.sanitize();
        }
        self.devices.truncate(MAX_DEVICES);
        let mut seen_ids = std::collections::BTreeSet::new();
        self.devices.retain(|d| seen_ids.insert(d.id));
        let max_id = self.devices.iter().map(|d| d.id.0).max().unwrap_or(0);
        self.next_id = self.next_id.max(max_id.saturating_add(1)).max(1);
        let mut names = std::collections::BTreeSet::new();
        for i in 0..self.devices.len() {
            let Some(d) = self.devices.get(i) else { continue };
            let mut name: String = d.name.trim().chars().take(MAX_NAME).collect();
            if name.is_empty() || validate_hostname(&name).is_err() || names.contains(&name.to_ascii_lowercase()) {
                let kind = d.kind;
                name = (1..)
                    .map(|n| format!("{}{n}", kind.name_prefix()))
                    .find(|n| !names.contains(&n.to_ascii_lowercase()) && self.name_free(n, Some(d.id)))
                    .unwrap_or_default();
            }
            names.insert(name.to_ascii_lowercase());
            if let Some(d) = self.devices.get_mut(i) {
                d.name = name;
                d.model = d.catalogue_model().id.to_string();
                d.x = finite(d.x);
                d.y = finite(d.y);
                d.routes.truncate(MAX_ROUTES);
                d.dhcp_pools.truncate(MAX_DHCP_POOLS);
                d.dhcp_excluded.truncate(MAX_DHCP_POOLS);
                for p in &mut d.ports {
                    p.subifs.truncate(MAX_SUBIFS);
                    p.subifs.sort_by_key(|s| s.number);
                    p.subifs.dedup_by_key(|s| s.number);
                }
                d.ensure_default_vlan();
            }
        }
        self.annotations.truncate(MAX_ANNOTATIONS);
        let mut note_ids = std::collections::BTreeSet::new();
        self.annotations.retain(|a| note_ids.insert(a.id));
        for a in &mut self.annotations {
            a.x = finite(a.x);
            a.y = finite(a.y);
            a.w = finite(a.w);
            a.h = finite(a.h);
            if a.color.as_deref().is_some_and(|c| parse_color(c).is_none()) {
                a.color = None;
            }
            a.text = a.text.chars().take(MAX_ANNOTATION_TEXT).collect();
        }
        let mut used = std::collections::BTreeSet::new();
        let devices = &self.devices;
        let valid = |e: End| devices.iter().any(|d| d.id == e.device && e.port < d.ports.len());
        self.links.retain(|l| l.a != l.b && valid(l.a) && valid(l.b) && used.insert((l.a.device, l.a.port)) && used.insert((l.b.device, l.b.port)));
    }
}

fn finite(v: f32) -> f32 {
    if v.is_finite() { v.clamp(-1.0e6, 1.0e6) } else { 0.0 }
}

/// A hostname: 1–63 characters, letters, digits, `-` and `_`, starting with a letter or digit.
pub fn validate_hostname(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok { Ok(()) } else { Err(ModelError::Invalid(format!("`{name}` is not a valid hostname (letters, digits, - and _, up to {MAX_NAME})"))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_get_names_ports_and_stable_macs() {
        let mut t = Topology::new();
        let r = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let r2 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let s = t.add_device(DeviceKind::Switch, Some("Core"), 0.0, 0.0).unwrap();
        assert_eq!(t.device(r).unwrap().name, "R1");
        assert_eq!(t.device(r2).unwrap().name, "R2");
        assert_eq!(t.find("core").unwrap().id, s);
        assert_eq!(t.find("#1").unwrap().id, r);
        let sw = t.device(s).unwrap();
        assert_eq!(sw.ports.len(), 26);
        assert!(!sw.ports[0].shutdown && t.device(r).unwrap().ports[0].shutdown);
        assert_eq!(sw.vlans.get(&VlanId::DEFAULT).map(String::as_str), Some("default"));
        assert_eq!(sw.port_index("g0/2"), Some(25));
        assert_eq!(sw.port_index("fa 0/3"), Some(2));
        assert_eq!(sw.port_index("Fa0/30"), None);
        assert!(t.add_device(DeviceKind::Pc, Some("core"), 0.0, 0.0).is_err());
        assert!(t.add_device(DeviceKind::Pc, Some("bad name"), 0.0, 0.0).is_err());
    }

    #[test]
    fn cables_take_free_ports_only() {
        let mut t = Topology::new();
        let a = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let b = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let c = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        t.connect(End { device: a, port: 0 }, End { device: b, port: 0 }).unwrap();
        assert!(matches!(t.connect(End { device: c, port: 0 }, End { device: b, port: 0 }), Err(ModelError::PortInUse(..))));
        assert!(t.connect(End { device: c, port: 0 }, End { device: c, port: 0 }).is_err());
        assert!(t.connect(End { device: c, port: 5 }, End { device: b, port: 1 }).is_err());
        assert_eq!(t.free_port(b), Some(1));
        assert_eq!(t.free_port(a), None);
        assert_eq!(t.peer(End { device: b, port: 0 }), Some(End { device: a, port: 0 }));
        t.remove_device(a).unwrap();
        assert!(t.links.is_empty());
    }

    #[test]
    fn secrets_hash_and_obscure() {
        assert_eq!(secret_hash("class"), secret_hash("class"));
        assert_ne!(secret_hash("class"), secret_hash("cisco"));
        assert!(secret_hash("x").starts_with("$pc$"));
        let o = obscure("cisco123");
        assert_ne!(o, "cisco123");
        assert_eq!(unobscure(&o).as_deref(), Some("cisco123"));
        assert_eq!(unobscure("zz"), None);
        assert_eq!(unobscure("ABC"), None);
    }

    #[test]
    fn reset_keeps_identity() {
        let mut t = Topology::new();
        let r = t.add_device(DeviceKind::Router, None, 5.0, 6.0).unwrap();
        let d = t.device_mut(r).unwrap();
        let mac = d.ports[1].mac;
        d.ports[1].shutdown = false;
        d.ports[1].ip = Some("10.0.0.1/24".parse().unwrap());
        d.ports[1].subif_mut(10).unwrap().vlan = VlanId::new(10);
        d.reset_config();
        assert!(d.ports[1].shutdown && d.ports[1].ip.is_none() && d.ports[1].subifs.is_empty());
        assert_eq!(d.ports[1].mac, mac);
        assert_eq!((d.x, d.y, d.name.as_str()), (5.0, 6.0, "R1"));
    }

    #[test]
    fn sanitize_repairs_hostile_documents() {
        let mut t = Topology::new();
        let a = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let b = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        t.devices[1].name = "pc1".into();
        t.devices[0].x = f32::NAN;
        t.devices.push(t.devices[0].clone());
        t.links.push(Link { a: End { device: a, port: 0 }, b: End { device: b, port: 0 } });
        t.links.push(Link { a: End { device: a, port: 0 }, b: End { device: b, port: 0 } });
        t.links.push(Link { a: End { device: a, port: 9 }, b: End { device: DeviceId(77), port: 0 } });
        t.next_id = 0;
        t.sanitize();
        assert_eq!(t.devices.len(), 2);
        assert_eq!(t.links.len(), 1);
        assert_ne!(t.devices[0].name.to_lowercase(), t.devices[1].name.to_lowercase());
        assert_eq!(t.devices[0].x, 0.0);
        assert_eq!(t.next_id, 3);
    }
}
