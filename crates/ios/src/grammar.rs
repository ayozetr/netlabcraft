//! The command grammar: every command is a path of tokens, matched with unambiguous prefixes
//! (`sh ip int br`). The same table answers `?` help and Tab completion.

/// One position in a command path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    /// A keyword, matched by unambiguous prefix.
    K(&'static str),
    /// One argument word; the text is its help placeholder (`A.B.C.D`).
    Arg(&'static str),
    /// The rest of the line (one or more words).
    Rest(&'static str),
}

/// Where a command is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes(pub u32);

impl Modes {
    pub const USER: Modes = Modes(1);
    pub const EXEC: Modes = Modes(2);
    pub const CONFIG: Modes = Modes(4);
    pub const IF: Modes = Modes(8);
    pub const SUBIF: Modes = Modes(16);
    pub const VLAN: Modes = Modes(32);
    pub const ROUTER: Modes = Modes(64);
    pub const SWITCH: Modes = Modes(128);
    pub const LINE: Modes = Modes(256);
    pub const DHCP: Modes = Modes(512);
    /// A switch's VLAN interface.
    pub const SVI: Modes = Modes(1024);
    /// `router ospf`.
    pub const OSPF: Modes = Modes(2048);
    /// `ip access-list`.
    pub const ACL: Modes = Modes(4096);
    /// `router rip`.
    pub const RIP: Modes = Modes(8192);
    /// `ipv6 access-list`.
    pub const ACL6: Modes = Modes(16384);
    /// `ipv6 dhcp pool`.
    pub const DHCP6: Modes = Modes(32768);
    /// `ipv6 router ospf`.
    pub const OSPF6: Modes = Modes(65536);
    /// `router eigrp`.
    pub const EIGRP: Modes = Modes(131072);
    /// `telephony-service` (bits 18 and 19 were free; `Modes` is a u32).
    pub const TELEPHONY: Modes = Modes(1 << 18);
    /// `ephone-dn N` and `ephone N` (one bit for both; the command checks which).
    pub const EPHONE: Modes = Modes(1 << 19);
    /// `interface Tunnel N`.
    pub const TUNNEL: Modes = Modes(1 << 20);
    /// `crypto isakmp policy N`.
    pub const ISAKMP: Modes = Modes(1 << 21);
    /// `crypto map NAME N ipsec-isakmp`.
    pub const CRYPTO_MAP: Modes = Modes(1 << 22);
    /// `crypto ipsec transform-set NAME …`.
    pub const TRANSFORM: Modes = Modes(1 << 23);
    /// `class-map [match-all|match-any] NAME`.
    pub const CLASS_MAP: Modes = Modes(1 << 26);
    /// `policy-map NAME`.
    pub const POLICY_MAP: Modes = Modes(1 << 27);
    /// `class NAME` inside a policy map.
    pub const POLICY_CLASS: Modes = Modes(1 << 28);
    /// The firewall appliance (a device flag, like `ROUTER` and `SWITCH`).
    pub const FIREWALL: Modes = Modes(1 << 29);
    /// `object network NAME` on the firewall.
    pub const OBJECT: Modes = Modes(1 << 30);
    /// Every command mode, either device.
    pub const ANY_MODE: Modes = Modes(
        63 | 256
            | 512
            | 1024
            | 2048
            | 4096
            | 8192
            | 16384
            | 32768
            | 65536
            | 131072
            | (1 << 18)
            | (1 << 19)
            | (1 << 20)
            | (1 << 21)
            | (1 << 22)
            | (1 << 23)
            | (1 << 26)
            | (1 << 27)
            | (1 << 28)
            | (1 << 30),
    );
    pub const BOTH: Modes = Modes(192);

    pub const fn or(self, o: Modes) -> Modes {
        Modes(self.0 | o.0)
    }

    pub fn has(self, o: Modes) -> bool {
        self.0 & o.0 != 0
    }
}

/// A command.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub id: Id,
    pub modes: Modes,
    pub devices: Modes,
    pub path: &'static [Tok],
    pub help: &'static str,
}

/// What a command does (dispatched in `exec.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Id {
    Enable,
    Disable,
    Exit,
    End,
    Logout,
    ConfigureTerminal,
    Ping,
    PingRepeat,
    Traceroute,
    ShowRun,
    ShowStartup,
    ShowIpIntBrief,
    ShowIpRoute,
    ShowInterfaces,
    ShowInterface,
    ShowIntTrunk,
    ShowVlan,
    ShowVlanBrief,
    ShowMac,
    ShowArp,
    ShowVersion,
    ShowHistory,
    CopyRunStart,
    WriteMemory,
    EraseStartup,
    Reload,
    ClearMac,
    ClearArp,
    Hostname,
    Interface,
    InterfaceRange,
    NoInterface,
    IpRoute,
    NoIpRoute,
    Vlan,
    NoVlan,
    Do,
    IpAddress,
    NoIpAddress,
    Shutdown,
    NoShutdown,
    Description,
    NoDescription,
    SwModeAccess,
    SwModeTrunk,
    SwAccessVlan,
    NoSwAccessVlan,
    SwNativeVlan,
    NoSwNativeVlan,
    SwAllowed,
    NoSwAllowed,
    Encapsulation,
    VlanName,
    NoVlanName,
    EnableSecret,
    EnablePassword,
    NoEnableSecret,
    NoEnablePassword,
    LineConsole,
    LineVty,
    LinePassword,
    NoLinePassword,
    Login,
    NoLogin,
    Accepted,
    BannerMotd,
    NoBannerMotd,
    ServicePasswordEncryption,
    NoServicePasswordEncryption,
    DomainLookup,
    NoDomainLookup,
    DhcpPool,
    NoDhcpPool,
    DhcpExcluded,
    NoDhcpExcluded,
    DhcpNetwork,
    DhcpDefaultRouter,
    DhcpDns,
    ShowDhcpBinding,
    ShowDhcpPool,
    ClearDhcpBinding,
    StpPriority,
    NoStpPriority,
    StpRoot,
    Portfast,
    NoPortfast,
    ShowStp,
    ShowStpVlan,
    NoInterfaceVlan,
    DefaultGateway,
    NoDefaultGateway,
    HelperAddress,
    NoHelperAddress,
    RouterOspf,
    NoRouterOspf,
    OspfNetwork,
    NoOspfNetwork,
    OspfRouterId,
    NoOspfRouterId,
    OspfPassive,
    NoOspfPassive,
    OspfDefaultOriginate,
    NoOspfDefaultOriginate,
    ShowOspfNeighbor,
    Ipv6RouterOspf,
    NoIpv6RouterOspf,
    Ipv6OspfArea,
    NoIpv6OspfArea,
    ShowIpv6OspfNeighbor,
    ShowIpv6Ospf,
    ShowIpProtocols,
    AccessList,
    NoAccessList,
    IpAccessListStandard,
    IpAccessListExtended,
    NoIpAccessList,
    AclEntryLine,
    NoAclEntryLine,
    AccessGroup,
    NoAccessGroup,
    ShowAccessLists,
    ClearAclCounters,
    NatSideCmd,
    NoNatSide,
    NatSource,
    NoNatSource,
    NatPool,
    NoNatPool,
    ShowNat,
    ClearNat,
    // ---- site-to-site VPN: GRE tunnels and IPsec ----
    TunnelSource,
    NoTunnelSource,
    TunnelDestination,
    NoTunnelDestination,
    TunnelMode,
    IsakmpPolicy,
    NoIsakmpPolicy,
    IsakmpEncryption,
    IsakmpHash,
    IsakmpAuthentication,
    IsakmpGroup,
    IsakmpLifetime,
    IsakmpKey,
    NoIsakmpKey,
    TransformSet,
    NoTransformSet,
    TransformMode,
    CryptoMap,
    NoCryptoMap,
    CryptoSetPeer,
    NoCryptoSetPeer,
    CryptoSetTransformSet,
    NoCryptoSetTransformSet,
    CryptoMatchAddress,
    NoCryptoMatchAddress,
    CryptoMapIf,
    NoCryptoMapIf,
    ShowCryptoIsakmpSa,
    ShowCryptoIpsecSa,
    ShowCryptoIsakmpPolicy,
    ShowCryptoMap,
    ClearCryptoSa,
    // ---- QoS: class maps, policy maps, service policies ----
    ClassMap,
    NoClassMap,
    PolicyMap,
    NoPolicyMap,
    QosMatchDscp,
    QosMatchPrecedence,
    QosMatchAccessGroup,
    QosMatchAccessGroupName,
    QosMatchProtocol,
    QosMatchAny,
    NoQosMatch,
    PolicyClass,
    NoPolicyClass,
    QosSetDscp,
    QosSetPrecedence,
    NoQosSet,
    QosPriority,
    QosPriorityPercent,
    NoQosPriority,
    QosBandwidth,
    QosBandwidthPercent,
    NoQosBandwidth,
    QosPolice,
    NoQosPolice,
    QosShape,
    NoQosShape,
    ServicePolicy,
    NoServicePolicy,
    ShowClassMap,
    ShowPolicyMap,
    ShowPolicyMapInterface,
    WlanAdd,
    NoWlan,
    CapwapController,
    NoCapwapController,
    ShowWlan,
    ShowCapwap,
    FwNameif,
    FwNoNameif,
    FwSecurityLevel,
    FwNoSecurityLevel,
    FwRoute,
    FwNoRoute,
    FwAccessList,
    FwNoAccessList,
    FwNoAccessListEntry,
    FwAccessGroup,
    FwNoAccessGroup,
    FwObjectNetwork,
    FwNoObjectNetwork,
    FwObjHost,
    FwObjSubnet,
    FwObjNat,
    FwObjNoNat,
    FwSameSecurity,
    FwNoSameSecurity,
    FwDhcpdAddress,
    FwNoDhcpdAddress,
    FwDhcpdEnable,
    FwNoDhcpdEnable,
    FwDhcpdDns,
    FwNoDhcpdDns,
    FwTimeout,
    FwShowNat,
    FwShowXlate,
    FwShowConn,
    FwShowAccessList,
    FwShowIntIpBrief,
    FwShowRoute,
    FwShowNameif,
    FwShowDhcpdBinding,
    FwShowObjects,
    FwClearConn,
    FwClearXlate,
    FwClearAclCounters,
    FwObjectGroup,
    FwNoObjectGroup,
    FwGrpMember,
    FwGrpNoMember,
    FwAclRemark,
    FwNoAclRemark,
    FwIcmp,
    FwNoIcmp,
    FwTwiceNat,
    FwNoTwiceNat,
    FwShowNatDetail,
    FwShowObjectGroup,
    FwClassMap,
    FwNoClassMap,
    FwClassMatch,
    FwNoClassMatch,
    FwPolicyMap,
    FwNoPolicyMap,
    FwPolicyClass,
    FwNoPolicyClass,
    FwPolicyAction,
    FwNoPolicyAction,
    FwServicePolicy,
    FwNoServicePolicy,
    FwShowServicePolicy,
    Username,
    NoUsername,
    DomainName,
    NoDomainName,
    CryptoKey,
    CryptoKeyZeroize,
    SshVersion,
    LoginLocal,
    AaaNewModel,
    NoAaaNewModel,
    AaaLogin,
    NoAaaLogin,
    RadiusHost,
    NoRadiusHost,
    TacacsHost,
    NoTacacsHost,
    SnmpCommunity,
    NoSnmpCommunity,
    SnmpLocation,
    SnmpContact,
    ShowSnmp,
    SnmpHost,
    NoSnmpHost,
    SnmpTraps,
    NoSnmpTraps,
    SnmpGroup,
    NoSnmpGroup,
    SnmpUser,
    NoSnmpUser,
    LoginAuthentication,
    TransportInput,
    Telnet,
    Ssh,
    ShowIpSsh,
    IpAddressDhcp,
    SecurityMinLength,
    NoSecurityMinLength,
    LoginBlockFor,
    NoLoginBlockFor,
    IfSpeed,
    IfDuplex,
    IfMdix,
    NoIfMdix,
    SshTimeout,
    SshRetries,
    ShowSsh,
    ShowProtocols,
    ShowIntDescription,
    ShowLogin,
    ShowPrivilege,
    ShowTechSupport,
    ShowBoot,
    ShowEnv,
    ShowInventory,
    DeleteVlanDat,
    ShowIpv6Interface,
    ClearCounters,
    PingSize,
    ShowSessions,
    ShowHosts,
    PortSecurity,
    NoPortSecurity,
    PortSecMax,
    PortSecViolation,
    PortSecSticky,
    PortSecMac,
    ShowPortSecurity,
    ChannelGroup,
    NoChannelGroup,
    ShowEtherchannel,
    VtpModeCmd,
    VtpDomain,
    VtpPassword,
    NoVtpPassword,
    VtpVersion,
    ShowVtpStatus,
    ShowVtpPassword,
    Ipv6UnicastRouting,
    NoIpv6UnicastRouting,
    Ipv6Route,
    NoIpv6Route,
    Ipv6Address,
    Ipv6AddressEui,
    Ipv6AddressLinkLocal,
    NoIpv6Address,
    Ipv6Enable,
    NoIpv6Enable,
    ShowIpv6IntBrief,
    ShowIpv6Route,
    ShowIpv6Neighbors,
    IpRouting,
    NoIpRouting,
    NoSwitchport,
    SwitchportOn,
    EncapSerial,
    ClockRate,
    NoClockRate,
    ShowControllers,
    RouterRip,
    NoRouterRip,
    RipVersion,
    RipNetwork,
    NoRipNetwork,
    RipPassive,
    NoRipPassive,
    RipDefaultOriginate,
    NoRipDefaultOriginate,
    RipAutoSummary,
    NoRipAutoSummary,
    ShowIpRipDatabase,
    RouterEigrp,
    NoRouterEigrp,
    EigrpNetwork,
    NoEigrpNetwork,
    EigrpPassive,
    NoEigrpPassive,
    EigrpRouterId,
    NoEigrpRouterId,
    EigrpAutoSummary,
    NoEigrpAutoSummary,
    EigrpMaximumPaths,
    NoEigrpMaximumPaths,
    EigrpVariance,
    NoEigrpVariance,
    OspfMaximumPaths,
    NoOspfMaximumPaths,
    IpHelloEigrp,
    NoIpHelloEigrp,
    IpHoldEigrp,
    NoIpHoldEigrp,
    ShowIpEigrpNeighbors,
    ShowIpEigrpTopology,
    ShowIpEigrpInterfaces,
    DebugEigrpPackets,
    NoDebugEigrpPackets,
    Bandwidth,
    NoBandwidth,
    Delay,
    NoDelay,
    ShowClock,
    ClockSet,
    ShowFlash,
    ShowUsers,
    TerminalLength,
    CdpRun,
    NoCdpRun,
    CdpEnable,
    NoCdpEnable,
    ShowCdp,
    ShowCdpNeighbors,
    ShowCdpNeighborsDetail,
    LldpRun,
    NoLldpRun,
    LldpTransmit,
    NoLldpTransmit,
    LldpReceive,
    NoLldpReceive,
    ShowLldp,
    ShowLldpNeighbors,
    ShowLldpNeighborsDetail,
    ShowProcesses,
    ShowMemory,
    ShowIpInterface,
    ShowIntStatus,
    ShowRunInterface,
    ShowLogging,
    ClearLogging,
    LoggingBuffered,
    DebugIpIcmp,
    DebugIpRip,
    DebugIpOspf,
    DebugIpOther,
    NoDebugIpOther,
    NoDebugIpRip,
    NoDebugIpOspf,
    UndebugAll,
    ShowDebugging,
    StandbyIp,
    StandbyIpGroup0,
    StandbyPriority,
    StandbyPreempt,
    NoStandby,
    ShowStandby,
    SwModeDynAuto,
    SwModeDynDesirable,
    SwNonegotiate,
    NoSwNonegotiate,
    PppAuthentication,
    NoPppAuthentication,
    PppPapSent,
    AccessClass,
    NoAccessClass,
    Ipv6AccessList,
    NoIpv6AccessList,
    Acl6EntryLine,
    NoAcl6EntryLine,
    TrafficFilter,
    NoTrafficFilter,
    Ipv6DhcpRelay,
    NoIpv6DhcpRelay,
    ShowIpv6AccessList,
    IpOspfCost,
    NoIpOspfCost,
    IpOspfPriority,
    NoIpOspfPriority,
    AutoCost,
    AreaStub,
    NoAreaStub,
    AreaRange,
    NoAreaRange,
    ShowIpOspf,
    ShowIpOspfInterface,
    Ipv6DhcpPool,
    NoIpv6DhcpPool,
    Dhcp6AddressPrefix,
    Dhcp6DnsServer,
    Dhcp6DomainName,
    Ipv6DhcpServer,
    NoIpv6DhcpServer,
    NdManaged,
    NoNdManaged,
    NdOther,
    NoNdOther,
    ShowIpv6DhcpPool,
    ShowIpv6DhcpBinding,
    NtpServer,
    NoNtpServer,
    ShowNtp,
    LoggingHost,
    NoLoggingHost,
    CopyToTftp,
    CopyFromTftp,
    CopyToFtp,
    CopyFromFtp,
    IpFtpUsername,
    IpFtpPassword,
    NoIpFtpLogin,
    StpMode,
    VoiceTelephonyService,
    VoiceNoTelephonyService,
    VoiceMaxEphones,
    VoiceMaxDn,
    VoiceSourceAddress,
    VoiceNoSourceAddress,
    VoiceAutoAssign,
    VoiceNoAutoAssign,
    VoiceEphoneDn,
    VoiceNoEphoneDn,
    VoiceNumber,
    VoiceNoNumber,
    VoiceEphone,
    VoiceNoEphone,
    VoiceMacAddress,
    VoiceButton,
    VoiceNoButton,
    VoiceDhcpOption150,
    VoiceNoDhcpOption150,
    SwVoiceVlan,
    NoSwVoiceVlan,
    ShowEphone,
    ShowEphoneDn,
    ShowTelephony,
}

use Tok::{Arg, K, Rest};

const EXECS: Modes = Modes::USER.or(Modes::EXEC);
const SUBMODES: Modes = Modes::IF
    .or(Modes::SUBIF)
    .or(Modes::VLAN)
    .or(Modes::LINE)
    .or(Modes::DHCP)
    .or(Modes::SVI)
    .or(Modes::OSPF)
    .or(Modes::ACL)
    .or(Modes::RIP)
    .or(Modes::ACL6)
    .or(Modes::DHCP6)
    .or(Modes::OSPF6)
    .or(Modes::EIGRP)
    .or(VPN_MODES)
    .or(QOS_MODES)
    .or(Modes::OBJECT)
    .or(Modes::TELEPHONY)
    .or(Modes::EPHONE);
/// Routers, switches and the firewall.
const BOTH_FW: Modes = Modes::BOTH.or(Modes::FIREWALL);
const FW: Modes = Modes::FIREWALL;
/// The site-to-site VPN sub-modes.
const VPN_MODES: Modes = Modes::TUNNEL.or(Modes::ISAKMP).or(Modes::CRYPTO_MAP).or(Modes::TRANSFORM);
/// The QoS sub-modes.
const QOS_MODES: Modes = Modes::CLASS_MAP.or(Modes::POLICY_MAP).or(Modes::POLICY_CLASS);
/// Where a service policy can be applied: routed interfaces, VLAN interfaces and tunnels.
const QOS_IFS: Modes = IFS.or(Modes::SVI).or(Modes::TUNNEL);
const IFS: Modes = Modes::IF.or(Modes::SUBIF);
const ANY_IF: Modes = IFS.or(Modes::SVI);

macro_rules! spec {
    ($id:ident, $modes:expr, $dev:expr, [$($t:expr),*], $help:literal) => {
        Spec { id: Id::$id, modes: $modes, devices: $dev, path: &[$($t),*], help: $help }
    };
}

/// Every command. Order matters only for help listings.
pub static SPECS: &[Spec] = &[
    spec!(Enable, Modes::USER, BOTH_FW, [K("enable")], "Turn on privileged commands"),
    spec!(Disable, Modes::EXEC, BOTH_FW, [K("disable")], "Turn off privileged commands"),
    spec!(Exit, Modes::ANY_MODE, BOTH_FW, [K("exit")], "Leave the current mode"),
    spec!(Logout, EXECS, BOTH_FW, [K("logout")], "End the session"),
    spec!(End, Modes::CONFIG.or(SUBMODES), BOTH_FW, [K("end")], "Return to privileged mode"),
    spec!(ConfigureTerminal, Modes::EXEC, BOTH_FW, [K("configure"), K("terminal")], "Configure from the terminal"),
    spec!(Ping, EXECS, BOTH_FW, [K("ping"), Arg("A.B.C.D|X:X::X")], "Send echo requests"),
    spec!(PingRepeat, EXECS, BOTH_FW, [K("ping"), Arg("A.B.C.D"), K("repeat"), Arg("<1-1000>")], "Send echo requests"),
    spec!(Traceroute, EXECS, BOTH_FW, [K("traceroute"), Arg("A.B.C.D|X:X::X")], "Trace the route to a destination"),
    spec!(ShowRun, Modes::EXEC, BOTH_FW, [K("show"), K("running-config")], "Current configuration"),
    spec!(ShowStartup, Modes::EXEC, BOTH_FW, [K("show"), K("startup-config")], "Saved configuration"),
    spec!(ShowIpIntBrief, EXECS, Modes::BOTH, [K("show"), K("ip"), K("interface"), K("brief")], "Interface addresses and status"),
    spec!(ShowIpRoute, EXECS, Modes::BOTH, [K("show"), K("ip"), K("route")], "Routing table"),
    spec!(
        ShowIpRoute,
        EXECS,
        Modes::BOTH,
        [K("show"), K("ip"), K("route"), Arg("{connected|static|eigrp|ospf|rip}")],
        "Only the routes one source gave"
    ),
    spec!(ShowArp, EXECS, Modes::BOTH, [K("show"), K("ip"), K("arp")], "ARP cache"),
    spec!(ShowArp, EXECS, BOTH_FW, [K("show"), K("arp")], "ARP cache"),
    spec!(ShowInterfaces, EXECS, BOTH_FW, [K("show"), K("interfaces")], "Interface details"),
    spec!(ShowIntTrunk, EXECS, Modes::SWITCH, [K("show"), K("interfaces"), K("trunk")], "Trunk ports"),
    spec!(ShowInterface, EXECS, BOTH_FW, [K("show"), K("interfaces"), Rest("<interface>")], "One interface's details"),
    spec!(ShowVlan, EXECS, Modes::SWITCH, [K("show"), K("vlan")], "VLAN database"),
    spec!(ShowVlanBrief, EXECS, Modes::SWITCH, [K("show"), K("vlan"), K("brief")], "VLAN database"),
    spec!(ShowMac, EXECS, Modes::SWITCH, [K("show"), K("mac"), K("address-table")], "Learned MAC addresses"),
    spec!(ShowVersion, EXECS, BOTH_FW, [K("show"), K("version")], "Software and hardware"),
    spec!(ShowHistory, EXECS, BOTH_FW, [K("show"), K("history")], "Commands typed in this session"),
    spec!(CopyRunStart, Modes::EXEC, BOTH_FW, [K("copy"), K("running-config"), K("startup-config")], "Save the configuration"),
    spec!(WriteMemory, Modes::EXEC, BOTH_FW, [K("write"), K("memory")], "Save the configuration"),
    spec!(WriteMemory, Modes::EXEC, BOTH_FW, [K("write")], "Save the configuration"),
    spec!(EraseStartup, Modes::EXEC, BOTH_FW, [K("erase"), K("startup-config")], "Delete the saved configuration"),
    spec!(Reload, Modes::EXEC, BOTH_FW, [K("reload")], "Restart with the saved configuration"),
    spec!(ClearMac, Modes::EXEC, Modes::SWITCH, [K("clear"), K("mac"), K("address-table")], "Forget learned MAC addresses"),
    spec!(ClearArp, Modes::EXEC, Modes::ROUTER, [K("clear"), K("arp-cache")], "Forget ARP entries"),
    spec!(Hostname, Modes::CONFIG, BOTH_FW, [K("hostname"), Arg("WORD")], "Set the device name"),
    spec!(InterfaceRange, Modes::CONFIG, Modes::BOTH, [K("interface"), K("range"), Rest("<interfaces>")], "Configure several interfaces"),
    spec!(Interface, Modes::CONFIG, BOTH_FW, [K("interface"), Rest("<interface>")], "Configure an interface"),
    spec!(NoInterface, Modes::CONFIG, Modes::ROUTER, [K("no"), K("interface"), Rest("<subinterface>")], "Delete a subinterface"),
    spec!(
        IpRoute,
        Modes::CONFIG,
        Modes::BOTH,
        [K("ip"), K("route"), Arg("A.B.C.D"), Arg("MASK"), Rest("<next hop or interface>")],
        "Add a static route"
    ),
    spec!(
        NoIpRoute,
        Modes::CONFIG,
        Modes::BOTH,
        [K("no"), K("ip"), K("route"), Arg("A.B.C.D"), Arg("MASK"), Rest("<next hop or interface>")],
        "Remove a static route"
    ),
    spec!(NoIpRoute, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("route"), Arg("A.B.C.D"), Arg("MASK")], "Remove static routes"),
    spec!(Vlan, Modes::CONFIG, Modes::SWITCH, [K("vlan"), Arg("<1-4094>[,-]")], "Create or edit VLANs"),
    spec!(NoVlan, Modes::CONFIG, Modes::SWITCH, [K("no"), K("vlan"), Arg("<1-4094>[,-]")], "Delete VLANs"),
    spec!(Do, Modes::CONFIG.or(SUBMODES), BOTH_FW, [K("do"), Rest("<command>")], "Run a privileged command"),
    spec!(IpAddress, IFS, Modes::ROUTER.or(Modes::FIREWALL), [K("ip"), K("address"), Arg("A.B.C.D"), Arg("MASK")], "Set the interface address"),
    spec!(NoIpAddress, IFS, Modes::ROUTER.or(Modes::FIREWALL), [K("no"), K("ip"), K("address")], "Remove the interface address"),
    spec!(IpAddressDhcp, Modes::IF, Modes::BOTH, [K("ip"), K("address"), K("dhcp")], "Take the address from a DHCP server"),
    spec!(Shutdown, ANY_IF, BOTH_FW, [K("shutdown")], "Disable the interface"),
    spec!(NoShutdown, ANY_IF, BOTH_FW, [K("no"), K("shutdown")], "Enable the interface"),
    spec!(Description, ANY_IF, BOTH_FW, [K("description"), Rest("LINE")], "Describe the interface"),
    spec!(NoDescription, ANY_IF, BOTH_FW, [K("no"), K("description")], "Remove the description"),
    spec!(IpAddress, Modes::SVI, Modes::SWITCH, [K("ip"), K("address"), Arg("A.B.C.D"), Arg("MASK")], "Set the management address"),
    spec!(NoIpAddress, Modes::SVI, Modes::SWITCH, [K("no"), K("ip"), K("address")], "Remove the management address"),
    spec!(
        AccessList,
        Modes::CONFIG,
        Modes::BOTH,
        [K("access-list"), Arg("<1-199>"), Rest("{permit|deny|remark} ...")],
        "Add a line to a numbered access list"
    ),
    spec!(NoAccessList, Modes::CONFIG, Modes::BOTH, [K("no"), K("access-list"), Arg("<1-199>")], "Delete a numbered access list"),
    spec!(
        IpAccessListStandard,
        Modes::CONFIG,
        Modes::BOTH,
        [K("ip"), K("access-list"), K("standard"), Arg("WORD")],
        "Create or edit a named standard list"
    ),
    spec!(
        IpAccessListExtended,
        Modes::CONFIG,
        Modes::BOTH,
        [K("ip"), K("access-list"), K("extended"), Arg("WORD")],
        "Create or edit a named extended list"
    ),
    spec!(
        NoIpAccessList,
        Modes::CONFIG,
        Modes::BOTH,
        [K("no"), K("ip"), K("access-list"), Arg("{standard|extended}"), Arg("WORD")],
        "Delete a named list"
    ),
    spec!(AclEntryLine, Modes::ACL, Modes::BOTH, [K("permit"), Rest("...")], "Allow matching packets"),
    spec!(AclEntryLine, Modes::ACL, Modes::BOTH, [K("deny"), Rest("...")], "Drop matching packets"),
    spec!(Accepted, Modes::ACL, Modes::BOTH, [K("remark"), Rest("LINE")], "A comment (accepted, not kept)"),
    spec!(NoAclEntryLine, Modes::ACL, Modes::BOTH, [K("no"), Rest("{permit|deny} ...")], "Remove a line"),
    spec!(AccessGroup, IFS, Modes::BOTH, [K("ip"), K("access-group"), Arg("WORD"), Arg("{in|out}")], "Filter this interface's traffic"),
    spec!(NoAccessGroup, IFS, Modes::BOTH, [K("no"), K("ip"), K("access-group"), Arg("WORD"), Arg("{in|out}")], "Stop filtering"),
    spec!(Accepted, Modes::LINE, Modes::BOTH, [K("access-class"), Rest("WORD {in|out}")], "Restrict remote logins (accepted, not simulated)"),
    spec!(ShowAccessLists, EXECS, Modes::BOTH, [K("show"), K("access-lists")], "Access lists and their matches"),
    spec!(ShowAccessLists, EXECS, Modes::BOTH, [K("show"), K("ip"), K("access-lists")], "Access lists and their matches"),
    spec!(ClearAclCounters, Modes::EXEC, Modes::BOTH, [K("clear"), K("access-list"), K("counters")], "Reset the match counts"),
    spec!(NatSideCmd, ANY_IF, Modes::BOTH, [K("ip"), K("nat"), Arg("{inside|outside}")], "Mark this interface inside or outside NAT"),
    spec!(NoNatSide, ANY_IF, Modes::BOTH, [K("no"), K("ip"), K("nat"), Arg("{inside|outside}")], "Take this interface out of NAT"),
    spec!(
        NatSource,
        Modes::CONFIG,
        Modes::BOTH,
        [K("ip"), K("nat"), K("inside"), K("source"), Rest("{static A B | list ACL {interface IF | pool NAME} [overload]}")],
        "Translate inside addresses"
    ),
    spec!(NoNatSource, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("nat"), K("inside"), K("source"), Rest("...")], "Remove a translation rule"),
    spec!(
        NatPool,
        Modes::CONFIG,
        Modes::BOTH,
        [K("ip"), K("nat"), K("pool"), Arg("WORD"), Rest("START END {netmask M | prefix-length N}")],
        "Addresses to translate to"
    ),
    spec!(NoNatPool, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("nat"), K("pool"), Arg("WORD")], "Delete a NAT pool"),
    spec!(ShowNat, EXECS, Modes::BOTH, [K("show"), K("ip"), K("nat"), K("translations")], "Translations in use"),
    spec!(ClearNat, Modes::EXEC, Modes::BOTH, [K("clear"), K("ip"), K("nat"), K("translation"), Rest("*")], "Forget dynamic translations"),
    // ---- site-to-site VPN: GRE tunnels (`interface Tunnel N`) and IPsec ----
    spec!(IpAddress, Modes::TUNNEL, Modes::ROUTER, [K("ip"), K("address"), Arg("A.B.C.D"), Arg("MASK")], "Set the tunnel's address"),
    spec!(NoIpAddress, Modes::TUNNEL, Modes::ROUTER, [K("no"), K("ip"), K("address")], "Remove the tunnel's address"),
    spec!(Shutdown, Modes::TUNNEL, Modes::ROUTER, [K("shutdown")], "Disable the tunnel"),
    spec!(NoShutdown, Modes::TUNNEL, Modes::ROUTER, [K("no"), K("shutdown")], "Enable the tunnel"),
    spec!(Description, Modes::TUNNEL, Modes::ROUTER, [K("description"), Rest("LINE")], "Describe the tunnel"),
    spec!(NoDescription, Modes::TUNNEL, Modes::ROUTER, [K("no"), K("description")], "Remove the description"),
    spec!(IpOspfCost, Modes::TUNNEL, Modes::ROUTER, [K("ip"), K("ospf"), K("cost"), Arg("<1-65535>")], "This tunnel's OSPF cost"),
    spec!(NoIpOspfCost, Modes::TUNNEL, Modes::ROUTER, [K("no"), K("ip"), K("ospf"), K("cost")], "Cost from the bandwidth again"),
    spec!(TunnelSource, Modes::TUNNEL, Modes::ROUTER, [K("tunnel"), K("source"), Rest("<interface>|A.B.C.D")], "Where the tunnel starts"),
    spec!(NoTunnelSource, Modes::TUNNEL, Modes::ROUTER, [K("no"), K("tunnel"), K("source")], "Clear the tunnel's source"),
    spec!(TunnelDestination, Modes::TUNNEL, Modes::ROUTER, [K("tunnel"), K("destination"), Arg("A.B.C.D")], "Where the tunnel ends"),
    spec!(NoTunnelDestination, Modes::TUNNEL, Modes::ROUTER, [K("no"), K("tunnel"), K("destination")], "Clear the tunnel's destination"),
    spec!(TunnelMode, Modes::TUNNEL, Modes::ROUTER, [K("tunnel"), K("mode"), Rest("gre ip")], "Encapsulation: GRE over IPv4 (the default)"),
    spec!(IsakmpPolicy, Modes::CONFIG, Modes::ROUTER, [K("crypto"), K("isakmp"), K("policy"), Arg("<1-10000>")], "Define an IKE phase-1 policy"),
    spec!(NoIsakmpPolicy, Modes::CONFIG, Modes::ROUTER, [K("no"), K("crypto"), K("isakmp"), K("policy"), Arg("<1-10000>")], "Delete an IKE policy"),
    spec!(IsakmpEncryption, Modes::ISAKMP, Modes::ROUTER, [K("encryption"), Rest("{des|3des|aes [128|192|256]}")], "Cipher the policy proposes"),
    spec!(IsakmpHash, Modes::ISAKMP, Modes::ROUTER, [K("hash"), Arg("{sha|sha256|md5}")], "Hash the policy proposes"),
    spec!(IsakmpAuthentication, Modes::ISAKMP, Modes::ROUTER, [K("authentication"), Arg("{pre-share|rsa-sig}")], "How peers prove who they are"),
    spec!(IsakmpGroup, Modes::ISAKMP, Modes::ROUTER, [K("group"), Arg("{1|2|5|14|15|16|19|20|24}")], "Diffie-Hellman group"),
    spec!(IsakmpLifetime, Modes::ISAKMP, Modes::ROUTER, [K("lifetime"), Arg("<60-86400>")], "Seconds before the keys are renewed"),
    spec!(
        IsakmpKey,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("crypto"), K("isakmp"), K("key"), Rest("[0] KEY address A.B.C.D")],
        "Pre-shared key for a peer"
    ),
    spec!(
        NoIsakmpKey,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("no"), K("crypto"), K("isakmp"), K("key"), Rest("[0] KEY address A.B.C.D")],
        "Delete a pre-shared key"
    ),
    spec!(
        TransformSet,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("crypto"), K("ipsec"), K("transform-set"), Arg("WORD"), Rest("TRANSFORM [TRANSFORM]")],
        "Define how protected traffic is wrapped"
    ),
    spec!(
        NoTransformSet,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("no"), K("crypto"), K("ipsec"), K("transform-set"), Arg("WORD")],
        "Delete a transform set"
    ),
    spec!(TransformMode, Modes::TRANSFORM, Modes::ROUTER, [K("mode"), Arg("{tunnel|transport}")], "Protect whole packets or only their payload"),
    spec!(
        CryptoMap,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("crypto"), K("map"), Arg("WORD"), Arg("<1-65535>"), K("ipsec-isakmp")],
        "Define or edit a crypto map entry"
    ),
    spec!(
        NoCryptoMap,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("no"), K("crypto"), K("map"), Arg("WORD"), Rest("[<1-65535>]")],
        "Delete one crypto map entry"
    ),
    spec!(NoCryptoMap, Modes::CONFIG, Modes::ROUTER, [K("no"), K("crypto"), K("map"), Arg("WORD")], "Delete a whole crypto map"),
    spec!(CryptoSetPeer, Modes::CRYPTO_MAP, Modes::ROUTER, [K("set"), K("peer"), Arg("A.B.C.D")], "The far end of the IPsec tunnel"),
    spec!(NoCryptoSetPeer, Modes::CRYPTO_MAP, Modes::ROUTER, [K("no"), K("set"), K("peer")], "Clear the peer"),
    spec!(NoCryptoSetPeer, Modes::CRYPTO_MAP, Modes::ROUTER, [K("no"), K("set"), K("peer"), Arg("A.B.C.D")], "Clear the peer"),
    spec!(CryptoSetTransformSet, Modes::CRYPTO_MAP, Modes::ROUTER, [K("set"), K("transform-set"), Arg("WORD")], "The transform set to use"),
    spec!(NoCryptoSetTransformSet, Modes::CRYPTO_MAP, Modes::ROUTER, [K("no"), K("set"), K("transform-set")], "Clear the transform set"),
    spec!(CryptoMatchAddress, Modes::CRYPTO_MAP, Modes::ROUTER, [K("match"), K("address"), Arg("ACL")], "The access list choosing what to protect"),
    spec!(NoCryptoMatchAddress, Modes::CRYPTO_MAP, Modes::ROUTER, [K("no"), K("match"), K("address")], "Protect nothing"),
    spec!(CryptoMapIf, Modes::IF.or(Modes::SUBIF), Modes::ROUTER, [K("crypto"), K("map"), Arg("WORD")], "Protect traffic leaving this interface"),
    spec!(NoCryptoMapIf, Modes::IF.or(Modes::SUBIF), Modes::ROUTER, [K("no"), K("crypto"), K("map")], "Stop protecting traffic here"),
    spec!(NoCryptoMapIf, Modes::IF.or(Modes::SUBIF), Modes::ROUTER, [K("no"), K("crypto"), K("map"), Arg("WORD")], "Stop protecting traffic here"),
    spec!(ShowCryptoIsakmpSa, EXECS, Modes::ROUTER, [K("show"), K("crypto"), K("isakmp"), K("sa")], "IKE peers and their state"),
    spec!(ShowCryptoIsakmpPolicy, EXECS, Modes::ROUTER, [K("show"), K("crypto"), K("isakmp"), K("policy")], "IKE policies"),
    spec!(ShowCryptoIpsecSa, EXECS, Modes::ROUTER, [K("show"), K("crypto"), K("ipsec"), K("sa")], "IPsec associations and packet counts"),
    // ---- QoS ----
    spec!(ClassMap, Modes::CONFIG, Modes::BOTH, [K("class-map"), Rest("[match-all|match-any] NAME")], "Define or edit a traffic class"),
    spec!(NoClassMap, Modes::CONFIG, Modes::BOTH, [K("no"), K("class-map"), Rest("[match-all|match-any] NAME")], "Delete a traffic class"),
    spec!(PolicyMap, Modes::CONFIG, Modes::BOTH, [K("policy-map"), Arg("NAME")], "Define or edit what happens to each class"),
    spec!(NoPolicyMap, Modes::CONFIG, Modes::BOTH, [K("no"), K("policy-map"), Arg("NAME")], "Delete a policy map"),
    spec!(QosMatchDscp, Modes::CLASS_MAP, Modes::BOTH, [K("match"), K("dscp"), Rest("DSCP [DSCP...]")], "Packets carrying one of these DSCPs"),
    spec!(
        QosMatchDscp,
        Modes::CLASS_MAP,
        Modes::BOTH,
        [K("match"), K("ip"), K("dscp"), Rest("DSCP [DSCP...]")],
        "Packets carrying one of these DSCPs"
    ),
    spec!(
        QosMatchPrecedence,
        Modes::CLASS_MAP,
        Modes::BOTH,
        [K("match"), K("ip"), K("precedence"), Rest("<0-7> [<0-7>...]")],
        "Packets with one of these IP precedences"
    ),
    spec!(
        QosMatchPrecedence,
        Modes::CLASS_MAP,
        Modes::BOTH,
        [K("match"), K("precedence"), Rest("<0-7> [<0-7>...]")],
        "Packets with one of these IP precedences"
    ),
    spec!(
        QosMatchAccessGroup,
        Modes::CLASS_MAP,
        Modes::BOTH,
        [K("match"), K("access-group"), Arg("<1-2699>")],
        "Packets a numbered access list permits"
    ),
    spec!(
        QosMatchAccessGroupName,
        Modes::CLASS_MAP,
        Modes::BOTH,
        [K("match"), K("access-group"), K("name"), Arg("NAME")],
        "Packets a named access list permits"
    ),
    spec!(
        QosMatchProtocol,
        Modes::CLASS_MAP,
        Modes::BOTH,
        [K("match"), K("protocol"), Arg("{icmp|tcp|udp|http|https|dns|telnet|ssh|ftp|smtp|pop3|snmp|tftp|ntp}")],
        "Packets of a protocol (applications by their well-known port)"
    ),
    spec!(QosMatchAny, Modes::CLASS_MAP, Modes::BOTH, [K("match"), K("any")], "Every packet"),
    spec!(NoQosMatch, Modes::CLASS_MAP, Modes::BOTH, [K("no"), K("match"), Rest("CRITERION")], "Remove a match line"),
    spec!(
        PolicyClass,
        Modes::POLICY_MAP.or(Modes::POLICY_CLASS),
        Modes::BOTH,
        [K("class"), Arg("{NAME|class-default}")],
        "Set what happens to a class"
    ),
    spec!(
        NoPolicyClass,
        Modes::POLICY_MAP.or(Modes::POLICY_CLASS),
        Modes::BOTH,
        [K("no"), K("class"), Arg("{NAME|class-default}")],
        "Remove a class from the policy"
    ),
    spec!(QosSetDscp, Modes::POLICY_CLASS, Modes::BOTH, [K("set"), K("dscp"), Arg("DSCP")], "Mark the class's packets with a DSCP"),
    spec!(QosSetDscp, Modes::POLICY_CLASS, Modes::BOTH, [K("set"), K("ip"), K("dscp"), Arg("DSCP")], "Mark the class's packets with a DSCP"),
    spec!(
        QosSetPrecedence,
        Modes::POLICY_CLASS,
        Modes::BOTH,
        [K("set"), K("ip"), K("precedence"), Arg("<0-7>")],
        "Mark the class's packets with an IP precedence"
    ),
    spec!(
        QosSetPrecedence,
        Modes::POLICY_CLASS,
        Modes::BOTH,
        [K("set"), K("precedence"), Arg("<0-7>")],
        "Mark the class's packets with an IP precedence"
    ),
    spec!(NoQosSet, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("set"), Rest("{dscp|ip precedence} ...")], "Stop marking"),
    spec!(QosPriority, Modes::POLICY_CLASS, Modes::BOTH, [K("priority"), Arg("<8-10000000> kbit/s")], "Strict-priority queue for the class"),
    spec!(
        QosPriorityPercent,
        Modes::POLICY_CLASS,
        Modes::BOTH,
        [K("priority"), K("percent"), Arg("<1-100>")],
        "Strict-priority queue, share of the link"
    ),
    spec!(NoQosPriority, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("priority")], "Remove the priority queue"),
    spec!(NoQosPriority, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("priority"), Rest("...")], "Remove the priority queue"),
    spec!(QosBandwidth, Modes::POLICY_CLASS, Modes::BOTH, [K("bandwidth"), Arg("<8-10000000> kbit/s")], "Guaranteed bandwidth for the class"),
    spec!(QosBandwidthPercent, Modes::POLICY_CLASS, Modes::BOTH, [K("bandwidth"), K("percent"), Arg("<1-100>")], "Guaranteed share of the link"),
    spec!(NoQosBandwidth, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("bandwidth")], "Remove the bandwidth guarantee"),
    spec!(NoQosBandwidth, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("bandwidth"), Rest("...")], "Remove the bandwidth guarantee"),
    spec!(
        QosPolice,
        Modes::POLICY_CLASS,
        Modes::BOTH,
        [K("police"), Rest("BPS [BURST] [conform-action ACTION] [exceed-action ACTION]")],
        "Limit the class to a rate (token bucket)"
    ),
    spec!(NoQosPolice, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("police")], "Stop policing"),
    spec!(NoQosPolice, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("police"), Rest("...")], "Stop policing"),
    spec!(QosShape, Modes::POLICY_CLASS, Modes::BOTH, [K("shape"), K("average"), Arg("BPS")], "Shape the class to an average rate"),
    spec!(NoQosShape, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("shape")], "Stop shaping"),
    spec!(NoQosShape, Modes::POLICY_CLASS, Modes::BOTH, [K("no"), K("shape"), Rest("...")], "Stop shaping"),
    spec!(ServicePolicy, QOS_IFS, Modes::BOTH, [K("service-policy"), Arg("{input|output}"), Arg("NAME")], "Apply a policy map to this interface"),
    spec!(NoServicePolicy, QOS_IFS, Modes::BOTH, [K("no"), K("service-policy"), Arg("{input|output}")], "Remove a service policy"),
    spec!(NoServicePolicy, QOS_IFS, Modes::BOTH, [K("no"), K("service-policy"), Arg("{input|output}"), Arg("NAME")], "Remove a service policy"),
    spec!(ShowClassMap, EXECS, Modes::BOTH, [K("show"), K("class-map")], "Traffic classes"),
    spec!(ShowClassMap, EXECS, Modes::BOTH, [K("show"), K("class-map"), Arg("NAME")], "One traffic class"),
    spec!(ShowPolicyMap, EXECS, Modes::BOTH, [K("show"), K("policy-map")], "Policy maps"),
    spec!(ShowPolicyMap, EXECS, Modes::BOTH, [K("show"), K("policy-map"), Arg("NAME")], "One policy map"),
    spec!(ShowPolicyMapInterface, EXECS, Modes::BOTH, [K("show"), K("policy-map"), K("interface")], "Service policies and their counters"),
    spec!(
        ShowPolicyMapInterface,
        EXECS,
        Modes::BOTH,
        [K("show"), K("policy-map"), K("interface"), Rest("<interface>")],
        "One interface's service policies and counters"
    ),
    spec!(ShowCryptoMap, EXECS, Modes::ROUTER, [K("show"), K("crypto"), K("map")], "Crypto maps and where they apply"),
    spec!(
        WlanAdd,
        Modes::CONFIG,
        Modes::SWITCH,
        [K("wlan"), Arg("<1-16>"), Arg("SSID"), Rest("{open | psk PASSPHRASE | enterprise}")],
        "A WLAN this controller offers through its access points"
    ),
    spec!(NoWlan, Modes::CONFIG, Modes::SWITCH, [K("no"), K("wlan"), Arg("<1-16>")], "Stop offering a WLAN"),
    spec!(
        CapwapController,
        Modes::CONFIG,
        Modes::SWITCH,
        [K("capwap"), K("controller"), Arg("A.B.C.D")],
        "The wireless LAN controller this access point joins"
    ),
    spec!(NoCapwapController, Modes::CONFIG, Modes::SWITCH, [K("no"), K("capwap"), K("controller")], "Join a controller in this subnet"),
    spec!(ShowWlan, EXECS, Modes::SWITCH, [K("show"), K("wlan")], "WLANs and the hosts on them"),
    spec!(ShowCapwap, EXECS, Modes::SWITCH, [K("show"), K("capwap")], "Access points joined to this controller, or this AP's controller"),
    spec!(ClearCryptoSa, Modes::EXEC, Modes::ROUTER, [K("clear"), K("crypto"), K("sa")], "Forget IPsec associations and counters"),
    spec!(EncapSerial, Modes::IF, Modes::ROUTER, [K("encapsulation"), Arg("{hdlc|ppp}")], "Framing on a serial link"),
    spec!(ClockRate, Modes::IF, Modes::ROUTER, [K("clock"), K("rate"), Arg("<1200-2000000>")], "Clock a serial link from its DCE end"),
    spec!(NoClockRate, Modes::IF, Modes::ROUTER, [K("no"), K("clock"), K("rate")], "Remove the clock rate"),
    spec!(ShowControllers, EXECS, Modes::BOTH, [K("show"), K("controllers"), Rest("<interface>")], "Cable type at an interface (DCE or DTE)"),
    spec!(ShowClock, EXECS, Modes::BOTH, [K("show"), K("clock")], "The device's date and time"),
    spec!(ClockSet, Modes::EXEC, Modes::BOTH, [K("clock"), K("set"), Rest("hh:mm:ss <day> <month> <year>")], "Set the date and time"),
    spec!(ShowFlash, EXECS, Modes::BOTH, [K("show"), K("flash:")], "Files in flash memory"),
    spec!(ShowFlash, EXECS, Modes::BOTH, [K("show"), K("flash")], "Files in flash memory"),
    spec!(ShowFlash, Modes::EXEC, Modes::BOTH, [K("dir"), K("flash:")], "Files in flash memory"),
    spec!(ShowUsers, EXECS, Modes::BOTH, [K("show"), K("users")], "Who is logged in, and where"),
    spec!(
        TerminalLength,
        EXECS,
        Modes::BOTH,
        [K("terminal"), Rest("length N | width N | monitor")],
        "Terminal settings (output here is never paged)"
    ),
    spec!(CdpRun, Modes::CONFIG, Modes::BOTH, [K("cdp"), K("run")], "Discover neighbouring routers and switches"),
    spec!(NoCdpRun, Modes::CONFIG, Modes::BOTH, [K("no"), K("cdp"), K("run")], "Stop neighbour discovery on this device"),
    spec!(CdpEnable, Modes::IF, Modes::BOTH, [K("cdp"), K("enable")], "Discover neighbours on this port"),
    spec!(NoCdpEnable, Modes::IF, Modes::BOTH, [K("no"), K("cdp"), K("enable")], "Stop neighbour discovery on this port"),
    spec!(ShowCdp, EXECS, Modes::BOTH, [K("show"), K("cdp")], "Neighbour discovery settings"),
    spec!(ShowCdpNeighbors, EXECS, Modes::BOTH, [K("show"), K("cdp"), K("neighbors")], "Neighbouring routers and switches"),
    spec!(ShowCdpNeighborsDetail, EXECS, Modes::BOTH, [K("show"), K("cdp"), K("neighbors"), K("detail")], "Neighbours with their addresses"),
    spec!(ShowCdpNeighborsDetail, EXECS, Modes::BOTH, [K("show"), K("cdp"), K("entry"), K("*")], "Neighbours with their addresses"),
    spec!(LldpRun, Modes::CONFIG, Modes::BOTH, [K("lldp"), K("run")], "Send and hear LLDP advertisements (IEEE 802.1AB)"),
    spec!(NoLldpRun, Modes::CONFIG, Modes::BOTH, [K("no"), K("lldp"), K("run")], "Stop LLDP on this device"),
    spec!(LldpTransmit, Modes::IF, Modes::BOTH, [K("lldp"), K("transmit")], "Send LLDP advertisements on this port"),
    spec!(NoLldpTransmit, Modes::IF, Modes::BOTH, [K("no"), K("lldp"), K("transmit")], "Stop sending LLDP on this port"),
    spec!(LldpReceive, Modes::IF, Modes::BOTH, [K("lldp"), K("receive")], "Hear LLDP advertisements on this port"),
    spec!(NoLldpReceive, Modes::IF, Modes::BOTH, [K("no"), K("lldp"), K("receive")], "Ignore LLDP advertisements on this port"),
    spec!(ShowLldp, EXECS, Modes::BOTH, [K("show"), K("lldp")], "LLDP settings"),
    spec!(ShowLldpNeighbors, EXECS, Modes::BOTH, [K("show"), K("lldp"), K("neighbors")], "Neighbours heard through LLDP"),
    spec!(
        ShowLldpNeighborsDetail,
        EXECS,
        Modes::BOTH,
        [K("show"), K("lldp"), K("neighbors"), K("detail")],
        "LLDP neighbours with everything they advertise"
    ),
    spec!(ShowLldpNeighborsDetail, EXECS, Modes::BOTH, [K("show"), K("lldp"), K("entry"), K("*")], "LLDP neighbours with everything they advertise"),
    spec!(ShowProcesses, EXECS, Modes::BOTH, [K("show"), K("processes")], "What the device is busy with"),
    spec!(ShowProcesses, EXECS, Modes::BOTH, [K("show"), K("processes"), K("cpu")], "What the device is busy with"),
    spec!(ShowMemory, EXECS, Modes::BOTH, [K("show"), K("memory")], "Memory in use"),
    spec!(ShowMemory, EXECS, Modes::BOTH, [K("show"), K("memory"), K("statistics")], "Memory in use"),
    spec!(ShowIpInterface, EXECS, Modes::BOTH, [K("show"), K("ip"), K("interface")], "Layer-3 settings of every interface"),
    spec!(ShowIpInterface, EXECS, Modes::BOTH, [K("show"), K("ip"), K("interface"), Rest("<interface>")], "Layer-3 settings of an interface"),
    spec!(ShowIntStatus, EXECS, Modes::SWITCH, [K("show"), K("interfaces"), K("status")], "Port state, VLAN and speed"),
    spec!(
        ShowRunInterface,
        Modes::EXEC,
        Modes::BOTH,
        [K("show"), K("running-config"), K("interface"), Rest("<interface>")],
        "One interface's configuration"
    ),
    spec!(ShowLogging, EXECS, BOTH_FW, [K("show"), K("logging")], "The logging buffer"),
    spec!(ClearLogging, Modes::EXEC, BOTH_FW, [K("clear"), K("logging")], "Empty the logging buffer"),
    spec!(
        LoggingBuffered,
        Modes::CONFIG,
        Modes::BOTH,
        [K("logging"), K("buffered"), Rest("[size]")],
        "Keep messages in the logging buffer (always on here)"
    ),
    spec!(DebugIpIcmp, Modes::EXEC, Modes::BOTH, [K("debug"), K("ip"), K("icmp")], "Report echo requests and replies this device receives"),
    spec!(UndebugAll, Modes::EXEC, Modes::BOTH, [K("undebug"), K("all")], "Turn every debug off"),
    spec!(UndebugAll, Modes::EXEC, Modes::BOTH, [K("no"), K("debug"), K("all")], "Turn every debug off"),
    spec!(UndebugAll, Modes::EXEC, Modes::BOTH, [K("no"), K("debug"), K("ip"), K("icmp")], "Stop reporting echo messages"),
    spec!(DebugIpRip, Modes::EXEC, Modes::BOTH, [K("debug"), K("ip"), K("rip")], "Report the RIP updates this router hears"),
    spec!(DebugIpOther, Modes::EXEC, Modes::BOTH, [K("debug"), K("ip"), K("packet")], "Report each packet this device routes"),
    spec!(DebugIpOther, Modes::EXEC, Modes::BOTH, [K("debug"), K("ip"), K("nat")], "Report new NAT translations"),
    spec!(
        DebugIpOther,
        Modes::EXEC,
        Modes::BOTH,
        [K("debug"), K("ip"), K("dhcp"), K("server"), K("events")],
        "Report the addresses this DHCP server hands out"
    ),
    spec!(NoDebugIpOther, Modes::EXEC, Modes::BOTH, [K("no"), K("debug"), K("ip"), K("packet")], "Stop reporting packets"),
    spec!(NoDebugIpOther, Modes::EXEC, Modes::BOTH, [K("no"), K("debug"), K("ip"), K("nat")], "Stop reporting NAT"),
    spec!(NoDebugIpOther, Modes::EXEC, Modes::BOTH, [K("no"), K("debug"), K("ip"), K("dhcp"), K("server"), K("events")], "Stop reporting DHCP"),
    spec!(NoDebugIpRip, Modes::EXEC, Modes::BOTH, [K("no"), K("debug"), K("ip"), K("rip")], "Stop reporting RIP updates"),
    spec!(
        DebugIpOspf,
        Modes::EXEC,
        Modes::ROUTER,
        [K("debug"), K("ip"), K("ospf"), Rest("adj|events|hello")],
        "Report the OSPF hellos this router hears"
    ),
    spec!(
        NoDebugIpOspf,
        Modes::EXEC,
        Modes::ROUTER,
        [K("no"), K("debug"), K("ip"), K("ospf"), Rest("adj|events|hello")],
        "Stop reporting OSPF hellos"
    ),
    spec!(NoDebugIpOspf, Modes::EXEC, Modes::ROUTER, [K("no"), K("debug"), K("ip"), K("ospf")], "Stop reporting OSPF hellos"),
    spec!(ShowDebugging, EXECS, Modes::BOTH, [K("show"), K("debugging")], "Which debugs are on"),
    spec!(StandbyIp, IFS, Modes::ROUTER, [K("standby"), Arg("<0-255>"), K("ip"), Arg("A.B.C.D")], "HSRP: the group's virtual gateway address"),
    spec!(StandbyIpGroup0, IFS, Modes::ROUTER, [K("standby"), K("ip"), Arg("A.B.C.D")], "HSRP group 0's virtual gateway address"),
    spec!(StandbyPriority, IFS, Modes::ROUTER, [K("standby"), Arg("<0-255>"), K("priority"), Arg("<0-255>")], "HSRP: higher priority becomes active"),
    spec!(
        StandbyPreempt,
        IFS,
        Modes::ROUTER,
        [K("standby"), Arg("<0-255>"), K("preempt")],
        "HSRP: take over when this router has the highest priority"
    ),
    spec!(NoStandby, IFS, Modes::ROUTER, [K("no"), K("standby"), Arg("<0-255>")], "Remove an HSRP group"),
    spec!(StandbyIp, Modes::SVI, Modes::SWITCH, [K("standby"), Arg("<0-255>"), K("ip"), Arg("A.B.C.D")], "HSRP: the group's virtual gateway address"),
    spec!(StandbyIpGroup0, Modes::SVI, Modes::SWITCH, [K("standby"), K("ip"), Arg("A.B.C.D")], "HSRP group 0's virtual gateway address"),
    spec!(
        StandbyPriority,
        Modes::SVI,
        Modes::SWITCH,
        [K("standby"), Arg("<0-255>"), K("priority"), Arg("<0-255>")],
        "HSRP: higher priority becomes active"
    ),
    spec!(
        StandbyPreempt,
        Modes::SVI,
        Modes::SWITCH,
        [K("standby"), Arg("<0-255>"), K("preempt")],
        "HSRP: take over when this switch has the highest priority"
    ),
    spec!(NoStandby, Modes::SVI, Modes::SWITCH, [K("no"), K("standby"), Arg("<0-255>")], "Remove an HSRP group"),
    spec!(ShowStandby, EXECS, Modes::BOTH, [K("show"), K("standby")], "HSRP groups and who is active"),
    spec!(ShowStandby, EXECS, Modes::BOTH, [K("show"), K("standby"), K("brief")], "HSRP groups and who is active"),
    spec!(
        PppAuthentication,
        Modes::IF,
        Modes::ROUTER,
        [K("ppp"), K("authentication"), Rest("{chap | pap | chap pap | pap chap}")],
        "Demand authentication from the other end"
    ),
    spec!(NoPppAuthentication, Modes::IF, Modes::ROUTER, [K("no"), K("ppp"), K("authentication")], "Demand no authentication"),
    spec!(
        PppPapSent,
        Modes::IF,
        Modes::ROUTER,
        [K("ppp"), K("pap"), K("sent-username"), Arg("WORD"), K("password"), Arg("WORD")],
        "The name and password this end answers PAP with"
    ),
    spec!(IpOspfCost, IFS, Modes::ROUTER, [K("ip"), K("ospf"), K("cost"), Arg("<1-65535>")], "This interface's OSPF cost"),
    spec!(NoIpOspfCost, IFS, Modes::ROUTER, [K("no"), K("ip"), K("ospf"), K("cost")], "Cost from the bandwidth again"),
    spec!(IpOspfPriority, IFS, Modes::ROUTER, [K("ip"), K("ospf"), K("priority"), Arg("<0-255>")], "DR election priority (0: never DR)"),
    spec!(NoIpOspfPriority, IFS, Modes::ROUTER, [K("no"), K("ip"), K("ospf"), K("priority")], "Priority 1 again"),
    spec!(IpOspfCost, Modes::SVI.or(Modes::IF), Modes::SWITCH, [K("ip"), K("ospf"), K("cost"), Arg("<1-65535>")], "This interface's OSPF cost"),
    spec!(NoIpOspfCost, Modes::SVI.or(Modes::IF), Modes::SWITCH, [K("no"), K("ip"), K("ospf"), K("cost")], "Cost from the bandwidth again"),
    spec!(
        IpOspfPriority,
        Modes::SVI.or(Modes::IF),
        Modes::SWITCH,
        [K("ip"), K("ospf"), K("priority"), Arg("<0-255>")],
        "DR election priority (0: never DR)"
    ),
    spec!(NoIpOspfPriority, Modes::SVI.or(Modes::IF), Modes::SWITCH, [K("no"), K("ip"), K("ospf"), K("priority")], "Priority 1 again"),
    spec!(AutoCost, Modes::OSPF, Modes::BOTH, [K("auto-cost"), K("reference-bandwidth"), Arg("<1-4294967>")], "Mbit/s that cost 1"),
    spec!(
        AreaStub,
        Modes::OSPF,
        Modes::BOTH,
        [K("area"), Arg("<0-4294967295>"), K("stub")],
        "No external routes in this area: a default route instead"
    ),
    spec!(
        AreaStub,
        Modes::OSPF,
        Modes::BOTH,
        [K("area"), Arg("<0-4294967295>"), K("stub"), K("no-summary")],
        "Border router: routes from other areas stay out of this stub area too"
    ),
    spec!(NoAreaStub, Modes::OSPF, Modes::BOTH, [K("no"), K("area"), Arg("<0-4294967295>"), K("stub")], "A normal area again"),
    spec!(
        NoAreaStub,
        Modes::OSPF,
        Modes::BOTH,
        [K("no"), K("area"), Arg("<0-4294967295>"), K("stub"), K("no-summary")],
        "Let routes from other areas in again"
    ),
    spec!(
        AreaRange,
        Modes::OSPF,
        Modes::BOTH,
        [K("area"), Arg("<0-4294967295>"), K("range"), Arg("A.B.C.D"), Arg("A.B.C.D")],
        "Border router: advertise one summary for the area's networks inside it"
    ),
    spec!(
        NoAreaRange,
        Modes::OSPF,
        Modes::BOTH,
        [K("no"), K("area"), Arg("<0-4294967295>"), K("range"), Arg("A.B.C.D"), Arg("A.B.C.D")],
        "Stop summarising"
    ),
    spec!(ShowIpOspf, EXECS, Modes::BOTH, [K("show"), K("ip"), K("ospf")], "OSPF process summary"),
    spec!(ShowIpOspfInterface, EXECS, Modes::BOTH, [K("show"), K("ip"), K("ospf"), K("interface")], "OSPF interfaces: area, cost, state"),
    spec!(ShowIpOspfInterface, EXECS, Modes::BOTH, [K("show"), K("ip"), K("ospf"), K("interface"), K("brief")], "OSPF interfaces: area, cost, state"),
    spec!(Ipv6DhcpPool, Modes::CONFIG, Modes::ROUTER, [K("ipv6"), K("dhcp"), K("pool"), Arg("WORD")], "Create or edit a DHCPv6 pool"),
    spec!(NoIpv6DhcpPool, Modes::CONFIG, Modes::ROUTER, [K("no"), K("ipv6"), K("dhcp"), K("pool"), Arg("WORD")], "Delete a DHCPv6 pool"),
    spec!(Dhcp6AddressPrefix, Modes::DHCP6, Modes::ROUTER, [K("address"), K("prefix"), Arg("X:X::/64")], "Addresses to hand out (stateful DHCPv6)"),
    spec!(Dhcp6DnsServer, Modes::DHCP6, Modes::ROUTER, [K("dns-server"), Arg("X:X::X")], "The DNS server clients get"),
    spec!(Dhcp6DomainName, Modes::DHCP6, Modes::ROUTER, [K("domain-name"), Arg("WORD")], "The domain clients get"),
    spec!(Ipv6DhcpServer, ANY_IF, Modes::BOTH, [K("ipv6"), K("dhcp"), K("server"), Arg("WORD")], "Answer DHCPv6 on this interface from a pool"),
    spec!(NoIpv6DhcpServer, ANY_IF, Modes::BOTH, [K("no"), K("ipv6"), K("dhcp"), K("server")], "Stop answering DHCPv6 here"),
    spec!(NdManaged, ANY_IF, Modes::BOTH, [K("ipv6"), K("nd"), K("managed-config-flag")], "Tell hosts to take addresses from DHCPv6"),
    spec!(NoNdManaged, ANY_IF, Modes::BOTH, [K("no"), K("ipv6"), K("nd"), K("managed-config-flag")], "Hosts build their own addresses"),
    spec!(NdOther, ANY_IF, Modes::BOTH, [K("ipv6"), K("nd"), K("other-config-flag")], "Tell hosts to take DNS and domain from DHCPv6"),
    spec!(NoNdOther, ANY_IF, Modes::BOTH, [K("no"), K("ipv6"), K("nd"), K("other-config-flag")], "No other settings from DHCPv6"),
    spec!(ShowIpv6DhcpPool, EXECS, Modes::ROUTER, [K("show"), K("ipv6"), K("dhcp"), K("pool")], "DHCPv6 pools"),
    spec!(ShowIpv6DhcpBinding, EXECS, Modes::ROUTER, [K("show"), K("ipv6"), K("dhcp"), K("binding")], "DHCPv6 addresses handed out"),
    spec!(NtpServer, Modes::CONFIG, Modes::BOTH, [K("ntp"), K("server"), Arg("A.B.C.D")], "Take the time from an NTP server"),
    spec!(NoNtpServer, Modes::CONFIG, Modes::BOTH, [K("no"), K("ntp"), K("server"), Arg("A.B.C.D")], "Stop using an NTP server"),
    spec!(ShowNtp, EXECS, Modes::BOTH, [K("show"), K("ntp"), K("status")], "Whether the clock is synchronised"),
    spec!(ShowNtp, EXECS, Modes::BOTH, [K("show"), K("ntp"), K("associations")], "NTP servers and which one answered"),
    spec!(LoggingHost, Modes::CONFIG, Modes::BOTH, [K("logging"), K("host"), Arg("A.B.C.D")], "Send log messages to a syslog server"),
    spec!(LoggingHost, Modes::CONFIG, Modes::BOTH, [K("logging"), Arg("A.B.C.D")], "Send log messages to a syslog server"),
    spec!(NoLoggingHost, Modes::CONFIG, Modes::BOTH, [K("no"), K("logging"), K("host"), Arg("A.B.C.D")], "Stop sending to a syslog server"),
    spec!(
        CopyToTftp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("running-config"), K("tftp:"), Rest("<address> [file]")],
        "Save the configuration on a TFTP server"
    ),
    spec!(
        CopyToTftp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("startup-config"), K("tftp:"), Rest("<address> [file]")],
        "Save the startup configuration on a TFTP server"
    ),
    spec!(
        CopyFromTftp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("tftp:"), K("running-config"), Rest("<address> <file>")],
        "Merge a configuration from a TFTP server"
    ),
    spec!(
        CopyFromTftp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("tftp:"), K("startup-config"), Rest("<address> <file>")],
        "Make a TFTP file the startup configuration"
    ),
    spec!(
        CopyToFtp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("running-config"), K("ftp:"), Rest("<address> [file]")],
        "Save the configuration on an FTP server"
    ),
    spec!(
        CopyToFtp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("startup-config"), K("ftp:"), Rest("<address> [file]")],
        "Save the startup configuration on an FTP server"
    ),
    spec!(
        CopyFromFtp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("ftp:"), K("running-config"), Rest("<address> <file>")],
        "Merge a configuration from an FTP server"
    ),
    spec!(
        CopyFromFtp,
        Modes::EXEC,
        Modes::BOTH,
        [K("copy"), K("ftp:"), K("startup-config"), Rest("<address> <file>")],
        "Make an FTP file the startup configuration"
    ),
    spec!(IpFtpUsername, Modes::CONFIG, Modes::BOTH, [K("ip"), K("ftp"), K("username"), Arg("WORD")], "The FTP account `copy … ftp:` uses"),
    spec!(IpFtpPassword, Modes::CONFIG, Modes::BOTH, [K("ip"), K("ftp"), K("password"), Arg("WORD")], "The FTP account's password"),
    spec!(NoIpFtpLogin, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("ftp"), K("username")], "Forget the FTP account"),
    spec!(RouterRip, Modes::CONFIG, Modes::BOTH, [K("router"), K("rip")], "Start or configure RIP"),
    spec!(NoRouterRip, Modes::CONFIG, Modes::BOTH, [K("no"), K("router"), K("rip")], "Stop RIP"),
    spec!(RipVersion, Modes::RIP, Modes::BOTH, [K("version"), Arg("{1|2}")], "RIP version"),
    spec!(RipNetwork, Modes::RIP, Modes::BOTH, [K("network"), Arg("A.B.C.D")], "Run RIP on this classful network's interfaces"),
    spec!(NoRipNetwork, Modes::RIP, Modes::BOTH, [K("no"), K("network"), Arg("A.B.C.D")], "Stop RIP on these interfaces"),
    spec!(RipPassive, Modes::RIP, Modes::BOTH, [K("passive-interface"), Rest("<interface>")], "Advertise but send no updates there"),
    spec!(NoRipPassive, Modes::RIP, Modes::BOTH, [K("no"), K("passive-interface"), Rest("<interface>")], "Send updates there again"),
    spec!(RipDefaultOriginate, Modes::RIP, Modes::BOTH, [K("default-information"), K("originate")], "Advertise this router's default route"),
    spec!(NoRipDefaultOriginate, Modes::RIP, Modes::BOTH, [K("no"), K("default-information"), K("originate")], "Stop advertising the default route"),
    spec!(RipAutoSummary, Modes::RIP, Modes::BOTH, [K("auto-summary")], "Summarise at classful boundaries (kept, not simulated)"),
    spec!(NoRipAutoSummary, Modes::RIP, Modes::BOTH, [K("no"), K("auto-summary")], "Advertise subnets as they are"),
    spec!(ShowIpRipDatabase, EXECS, Modes::BOTH, [K("show"), K("ip"), K("rip"), K("database")], "Routes RIP knows"),
    spec!(RouterEigrp, Modes::CONFIG, Modes::BOTH, [K("router"), K("eigrp"), Arg("<1-65535>")], "Start or configure EIGRP in this autonomous system"),
    spec!(NoRouterEigrp, Modes::CONFIG, Modes::BOTH, [K("no"), K("router"), K("eigrp"), Arg("<1-65535>")], "Stop EIGRP"),
    spec!(EigrpNetwork, Modes::EIGRP, Modes::BOTH, [K("network"), Arg("A.B.C.D")], "Run EIGRP on this classful network's interfaces"),
    spec!(EigrpNetwork, Modes::EIGRP, Modes::BOTH, [K("network"), Arg("A.B.C.D"), Arg("WILDCARD")], "Run EIGRP on matching interfaces"),
    spec!(NoEigrpNetwork, Modes::EIGRP, Modes::BOTH, [K("no"), K("network"), Arg("A.B.C.D")], "Stop EIGRP on these interfaces"),
    spec!(NoEigrpNetwork, Modes::EIGRP, Modes::BOTH, [K("no"), K("network"), Arg("A.B.C.D"), Arg("WILDCARD")], "Stop EIGRP on these interfaces"),
    spec!(EigrpPassive, Modes::EIGRP, Modes::BOTH, [K("passive-interface"), Rest("<interface>")], "Advertise but send no hellos there"),
    spec!(NoEigrpPassive, Modes::EIGRP, Modes::BOTH, [K("no"), K("passive-interface"), Rest("<interface>")], "Send hellos there again"),
    spec!(EigrpRouterId, Modes::EIGRP, Modes::BOTH, [K("eigrp"), K("router-id"), Arg("A.B.C.D")], "Set the router id"),
    spec!(NoEigrpRouterId, Modes::EIGRP, Modes::BOTH, [K("no"), K("eigrp"), K("router-id")], "Use the highest interface address"),
    spec!(EigrpAutoSummary, Modes::EIGRP, Modes::BOTH, [K("auto-summary")], "Summarise at classful boundaries (kept, not simulated)"),
    spec!(NoEigrpAutoSummary, Modes::EIGRP, Modes::BOTH, [K("no"), K("auto-summary")], "Advertise subnets as they are"),
    spec!(EigrpMaximumPaths, Modes::EIGRP, Modes::BOTH, [K("maximum-paths"), Arg("<1-16>")], "Paths installed per destination"),
    spec!(NoEigrpMaximumPaths, Modes::EIGRP, Modes::BOTH, [K("no"), K("maximum-paths")], "Four paths per destination again"),
    spec!(EigrpVariance, Modes::EIGRP, Modes::BOTH, [K("variance"), Arg("<1-128>")], "Also use feasible successors up to N times the best metric"),
    spec!(NoEigrpVariance, Modes::EIGRP, Modes::BOTH, [K("no"), K("variance")], "Equal-cost paths only"),
    spec!(OspfMaximumPaths, Modes::OSPF, Modes::BOTH, [K("maximum-paths"), Arg("<1-16>")], "Equal-cost paths installed per destination"),
    spec!(NoOspfMaximumPaths, Modes::OSPF, Modes::BOTH, [K("no"), K("maximum-paths")], "Four paths per destination again"),
    spec!(
        IpHelloEigrp,
        IFS.or(Modes::SVI).or(Modes::TUNNEL),
        Modes::BOTH,
        [K("ip"), K("hello-interval"), K("eigrp"), Arg("<1-65535>"), Arg("<1-65535> seconds")],
        "How often EIGRP says hello on this interface"
    ),
    spec!(
        NoIpHelloEigrp,
        IFS.or(Modes::SVI).or(Modes::TUNNEL),
        Modes::BOTH,
        [K("no"), K("ip"), K("hello-interval"), K("eigrp"), Arg("<1-65535>")],
        "Hellos every 5 seconds again"
    ),
    spec!(
        IpHoldEigrp,
        IFS.or(Modes::SVI).or(Modes::TUNNEL),
        Modes::BOTH,
        [K("ip"), K("hold-time"), K("eigrp"), Arg("<1-65535>"), Arg("<1-65535> seconds")],
        "How long neighbours wait for this router's hellos"
    ),
    spec!(
        NoIpHoldEigrp,
        IFS.or(Modes::SVI).or(Modes::TUNNEL),
        Modes::BOTH,
        [K("no"), K("ip"), K("hold-time"), K("eigrp"), Arg("<1-65535>")],
        "A 15-second hold time again"
    ),
    spec!(ShowIpEigrpNeighbors, EXECS, Modes::BOTH, [K("show"), K("ip"), K("eigrp"), K("neighbors")], "EIGRP neighbours"),
    spec!(ShowIpEigrpTopology, EXECS, Modes::BOTH, [K("show"), K("ip"), K("eigrp"), K("topology")], "Successors and feasible successors"),
    spec!(ShowIpEigrpInterfaces, EXECS, Modes::BOTH, [K("show"), K("ip"), K("eigrp"), K("interfaces")], "Interfaces running EIGRP"),
    spec!(DebugEigrpPackets, Modes::EXEC, Modes::BOTH, [K("debug"), K("eigrp"), K("packets")], "Report the EIGRP hellos this router hears"),
    spec!(NoDebugEigrpPackets, Modes::EXEC, Modes::BOTH, [K("no"), K("debug"), K("eigrp"), K("packets")], "Stop reporting EIGRP hellos"),
    spec!(Bandwidth, Modes::IF, Modes::BOTH, [K("bandwidth"), Arg("<1-10000000>")], "The bandwidth EIGRP's metric uses (kbit/s)"),
    spec!(NoBandwidth, Modes::IF, Modes::BOTH, [K("no"), K("bandwidth")], "Bandwidth from the port's speed again"),
    spec!(Delay, Modes::IF, Modes::BOTH, [K("delay"), Arg("<1-16777215>")], "The delay EIGRP's metric adds (tens of microseconds)"),
    spec!(NoDelay, Modes::IF, Modes::BOTH, [K("no"), K("delay")], "Delay from the port's type again"),
    spec!(RouterOspf, Modes::CONFIG, Modes::BOTH, [K("router"), K("ospf"), Arg("<1-65535>")], "Start or configure OSPF"),
    spec!(NoRouterOspf, Modes::CONFIG, Modes::BOTH, [K("no"), K("router"), K("ospf"), Arg("<1-65535>")], "Stop OSPF"),
    spec!(
        OspfNetwork,
        Modes::OSPF,
        Modes::BOTH,
        [K("network"), Arg("A.B.C.D"), Arg("WILDCARD"), K("area"), Arg("<0-4294967295>")],
        "Run OSPF on matching interfaces"
    ),
    spec!(
        NoOspfNetwork,
        Modes::OSPF,
        Modes::BOTH,
        [K("no"), K("network"), Arg("A.B.C.D"), Arg("WILDCARD"), K("area"), Arg("<0-4294967295>")],
        "Stop OSPF on matching interfaces"
    ),
    spec!(OspfRouterId, Modes::OSPF.or(Modes::OSPF6), Modes::BOTH, [K("router-id"), Arg("A.B.C.D")], "Set the router id"),
    spec!(NoOspfRouterId, Modes::OSPF.or(Modes::OSPF6), Modes::BOTH, [K("no"), K("router-id")], "Use the highest interface address"),
    spec!(OspfPassive, Modes::OSPF.or(Modes::OSPF6), Modes::BOTH, [K("passive-interface"), Rest("<interface>")], "Advertise but form no neighbours"),
    spec!(NoOspfPassive, Modes::OSPF.or(Modes::OSPF6), Modes::BOTH, [K("no"), K("passive-interface"), Rest("<interface>")], "Form neighbours again"),
    spec!(
        OspfDefaultOriginate,
        Modes::OSPF.or(Modes::OSPF6),
        Modes::BOTH,
        [K("default-information"), K("originate")],
        "Advertise this router's default route"
    ),
    spec!(
        NoOspfDefaultOriginate,
        Modes::OSPF.or(Modes::OSPF6),
        Modes::BOTH,
        [K("no"), K("default-information"), K("originate")],
        "Stop advertising the default route"
    ),
    spec!(ShowOspfNeighbor, EXECS, Modes::BOTH, [K("show"), K("ip"), K("ospf"), K("neighbor")], "OSPF neighbours"),
    spec!(Ipv6RouterOspf, Modes::CONFIG, Modes::BOTH, [K("ipv6"), K("router"), K("ospf"), Arg("<1-65535>")], "Start or configure OSPFv3"),
    spec!(NoIpv6RouterOspf, Modes::CONFIG, Modes::BOTH, [K("no"), K("ipv6"), K("router"), K("ospf"), Arg("<1-65535>")], "Stop OSPFv3"),
    spec!(
        Ipv6OspfArea,
        ANY_IF,
        Modes::BOTH,
        [K("ipv6"), K("ospf"), Arg("<1-65535>"), K("area"), Arg("<0-4294967295>")],
        "Run OSPFv3 on this interface"
    ),
    spec!(
        NoIpv6OspfArea,
        ANY_IF,
        Modes::BOTH,
        [K("no"), K("ipv6"), K("ospf"), Arg("<1-65535>"), K("area"), Arg("<0-4294967295>")],
        "Stop OSPFv3 on this interface"
    ),
    spec!(ShowIpv6OspfNeighbor, EXECS, Modes::BOTH, [K("show"), K("ipv6"), K("ospf"), K("neighbor")], "OSPFv3 neighbours"),
    spec!(ShowIpv6Ospf, EXECS, Modes::BOTH, [K("show"), K("ipv6"), K("ospf")], "OSPFv3 process"),
    spec!(ShowIpProtocols, EXECS, Modes::BOTH, [K("show"), K("ip"), K("protocols")], "Routing protocols running"),
    spec!(HelperAddress, IFS, Modes::ROUTER, [K("ip"), K("helper-address"), Arg("A.B.C.D")], "Relay DHCP requests to this server"),
    spec!(NoHelperAddress, IFS, Modes::ROUTER, [K("no"), K("ip"), K("helper-address"), Rest("[A.B.C.D]")], "Stop relaying (to one server)"),
    spec!(NoHelperAddress, IFS, Modes::ROUTER, [K("no"), K("ip"), K("helper-address")], "Stop relaying DHCP requests"),
    spec!(NoInterfaceVlan, Modes::CONFIG, Modes::SWITCH, [K("no"), K("interface"), K("vlan"), Arg("<1-4094>")], "Delete a VLAN interface"),
    spec!(DefaultGateway, Modes::CONFIG, Modes::SWITCH, [K("ip"), K("default-gateway"), Arg("A.B.C.D")], "Gateway for the switch's own traffic"),
    spec!(NoDefaultGateway, Modes::CONFIG, Modes::SWITCH, [K("no"), K("ip"), K("default-gateway")], "Remove the gateway"),
    spec!(NoSwitchport, Modes::IF, Modes::SWITCH, [K("no"), K("switchport")], "Make this a routed port (multilayer switches)"),
    spec!(SwitchportOn, Modes::IF, Modes::SWITCH, [K("switchport")], "Make this a switch port again"),
    spec!(IpAddress, Modes::IF, Modes::SWITCH, [K("ip"), K("address"), Arg("A.B.C.D"), Arg("MASK")], "Address a routed port"),
    spec!(NoIpAddress, Modes::IF, Modes::SWITCH, [K("no"), K("ip"), K("address")], "Remove a routed port's address"),
    spec!(SwModeAccess, Modes::IF, Modes::SWITCH, [K("switchport"), K("mode"), K("access")], "Carry one VLAN, untagged"),
    spec!(SwModeTrunk, Modes::IF, Modes::SWITCH, [K("switchport"), K("mode"), K("trunk")], "Carry many VLANs, 802.1Q tagged"),
    spec!(
        SwModeDynAuto,
        Modes::IF,
        Modes::SWITCH,
        [K("switchport"), K("mode"), K("dynamic"), K("auto")],
        "Become a trunk if the other end asks (DTP)"
    ),
    spec!(
        SwModeDynDesirable,
        Modes::IF,
        Modes::SWITCH,
        [K("switchport"), K("mode"), K("dynamic"), K("desirable")],
        "Ask the other end to trunk (DTP)"
    ),
    spec!(SwNonegotiate, Modes::IF, Modes::SWITCH, [K("switchport"), K("nonegotiate")], "Send no DTP messages"),
    spec!(NoSwNonegotiate, Modes::IF, Modes::SWITCH, [K("no"), K("switchport"), K("nonegotiate")], "Send DTP messages again"),
    spec!(SwAccessVlan, Modes::IF, Modes::SWITCH, [K("switchport"), K("access"), K("vlan"), Arg("<1-4094>")], "Set the access VLAN"),
    spec!(NoSwAccessVlan, Modes::IF, Modes::SWITCH, [K("no"), K("switchport"), K("access"), K("vlan")], "Back to VLAN 1"),
    spec!(
        SwNativeVlan,
        Modes::IF,
        Modes::SWITCH,
        [K("switchport"), K("trunk"), K("native"), K("vlan"), Arg("<1-4094>")],
        "Set the untagged trunk VLAN"
    ),
    spec!(NoSwNativeVlan, Modes::IF, Modes::SWITCH, [K("no"), K("switchport"), K("trunk"), K("native"), K("vlan")], "Back to native VLAN 1"),
    spec!(
        SwAllowed,
        Modes::IF,
        Modes::SWITCH,
        [K("switchport"), K("trunk"), K("allowed"), K("vlan"), Rest("{all|none|add|remove} <list>")],
        "Set the trunk's VLANs"
    ),
    spec!(NoSwAllowed, Modes::IF, Modes::SWITCH, [K("no"), K("switchport"), K("trunk"), K("allowed"), K("vlan")], "Allow every VLAN"),
    spec!(Encapsulation, Modes::SUBIF, Modes::ROUTER, [K("encapsulation"), K("dot1q"), Arg("<1-4094>")], "Bind the subinterface to a VLAN"),
    spec!(VlanName, Modes::VLAN, Modes::SWITCH, [K("name"), Rest("WORD")], "Name the VLAN"),
    spec!(NoVlanName, Modes::VLAN, Modes::SWITCH, [K("no"), K("name")], "Back to the default name"),
    spec!(
        EnableSecret,
        Modes::CONFIG,
        Modes::BOTH,
        [K("enable"), K("secret"), Rest("[0|5] PASSWORD")],
        "Protect privileged mode with a hashed password"
    ),
    spec!(EnablePassword, Modes::CONFIG, BOTH_FW, [K("enable"), K("password"), Rest("[0|7] PASSWORD")], "Protect privileged mode with a password"),
    spec!(NoEnableSecret, Modes::CONFIG, Modes::BOTH, [K("no"), K("enable"), K("secret")], "Remove the enable secret"),
    spec!(NoEnablePassword, Modes::CONFIG, BOTH_FW, [K("no"), K("enable"), K("password")], "Remove the enable password"),
    spec!(LineConsole, Modes::CONFIG, Modes::BOTH, [K("line"), K("console"), Arg("0")], "Configure the console line"),
    spec!(LineVty, Modes::CONFIG, Modes::BOTH, [K("line"), K("vty"), Rest("<first> [<last>]")], "Configure the remote-access lines"),
    spec!(LinePassword, Modes::LINE, Modes::BOTH, [K("password"), Rest("[0|7] PASSWORD")], "Set the line password"),
    spec!(NoLinePassword, Modes::LINE, Modes::BOTH, [K("no"), K("password")], "Remove the line password"),
    spec!(Login, Modes::LINE, Modes::BOTH, [K("login")], "Ask for the line password"),
    spec!(NoLogin, Modes::LINE, Modes::BOTH, [K("no"), K("login")], "Don't ask for a password"),
    spec!(Accepted, Modes::LINE, Modes::BOTH, [K("exec-timeout"), Rest("<minutes> [<seconds>]")], "Idle timeout (accepted, not simulated)"),
    spec!(Accepted, Modes::LINE, Modes::BOTH, [K("logging"), K("synchronous")], "Keep messages from breaking your typing (accepted)"),
    spec!(Ipv6AccessList, Modes::CONFIG, Modes::BOTH, [K("ipv6"), K("access-list"), Arg("WORD")], "Create or edit an IPv6 access list"),
    spec!(NoIpv6AccessList, Modes::CONFIG, Modes::BOTH, [K("no"), K("ipv6"), K("access-list"), Arg("WORD")], "Delete an IPv6 access list"),
    spec!(Acl6EntryLine, Modes::ACL6, Modes::ROUTER, [K("permit"), Rest("{ipv6|icmp|tcp|udp} <src> [port] <dst> [port]")], "Allow matching packets"),
    spec!(Acl6EntryLine, Modes::ACL6, Modes::ROUTER, [K("deny"), Rest("{ipv6|icmp|tcp|udp} <src> [port] <dst> [port]")], "Drop matching packets"),
    spec!(NoAcl6EntryLine, Modes::ACL6, Modes::ROUTER, [K("no"), Rest("{permit|deny} ...")], "Remove a line"),
    spec!(
        TrafficFilter,
        ANY_IF,
        Modes::BOTH,
        [K("ipv6"), K("traffic-filter"), Arg("WORD"), Arg("{in|out}")],
        "Filter IPv6 packets with an access list"
    ),
    spec!(NoTrafficFilter, ANY_IF, Modes::BOTH, [K("no"), K("ipv6"), K("traffic-filter"), Arg("WORD"), Arg("{in|out}")], "Stop filtering"),
    spec!(
        Ipv6DhcpRelay,
        ANY_IF,
        Modes::BOTH,
        [K("ipv6"), K("dhcp"), K("relay"), K("destination"), Arg("X:X::X")],
        "Pass this interface's DHCPv6 clients on to a server"
    ),
    spec!(
        NoIpv6DhcpRelay,
        ANY_IF,
        Modes::BOTH,
        [K("no"), K("ipv6"), K("dhcp"), K("relay"), K("destination"), Arg("X:X::X")],
        "Stop relaying to that server"
    ),
    spec!(ShowIpv6AccessList, EXECS, Modes::BOTH, [K("show"), K("ipv6"), K("access-list")], "IPv6 access lists and their matches"),
    spec!(AccessClass, Modes::LINE, Modes::BOTH, [K("access-class"), Arg("<1-99>|WORD"), K("in")], "Only logins this list permits (vty lines)"),
    spec!(NoAccessClass, Modes::LINE, Modes::BOTH, [K("no"), K("access-class"), Rest("[list in]")], "Remove the login filter"),
    spec!(TransportInput, Modes::LINE, Modes::BOTH, [K("transport"), K("input"), Rest("{ssh|telnet|all|none}")], "Protocols remote logins may use"),
    spec!(LoginLocal, Modes::LINE, Modes::BOTH, [K("login"), K("local")], "Ask for a username and password (username lines)"),
    spec!(AaaNewModel, Modes::CONFIG, Modes::BOTH, [K("aaa"), K("new-model")], "Check logins with AAA method lists"),
    spec!(NoAaaNewModel, Modes::CONFIG, Modes::BOTH, [K("no"), K("aaa"), K("new-model")], "Back to line passwords and login local"),
    spec!(
        AaaLogin,
        Modes::CONFIG,
        Modes::BOTH,
        [K("aaa"), K("authentication"), K("login"), K("default"), Rest("{group radius|local} ...")],
        "How logins are checked, in order"
    ),
    spec!(NoAaaLogin, Modes::CONFIG, Modes::BOTH, [K("no"), K("aaa"), K("authentication"), K("login"), K("default")], "Local users only"),
    spec!(
        RadiusHost,
        Modes::CONFIG,
        Modes::BOTH,
        [K("radius-server"), K("host"), Arg("A.B.C.D"), K("key"), Arg("WORD")],
        "A RADIUS server and the secret shared with it"
    ),
    spec!(NoRadiusHost, Modes::CONFIG, Modes::BOTH, [K("no"), K("radius-server"), K("host"), Arg("A.B.C.D")], "Forget a RADIUS server"),
    spec!(
        TacacsHost,
        Modes::CONFIG,
        Modes::BOTH,
        [K("tacacs-server"), K("host"), Arg("A.B.C.D"), K("key"), Arg("WORD")],
        "A TACACS+ server and the key shared with it"
    ),
    spec!(
        SnmpCommunity,
        Modes::CONFIG,
        Modes::BOTH,
        [K("snmp-server"), K("community"), Arg("WORD"), Arg("{ro|rw}")],
        "Let SNMP managers read (ro) or change (rw) this device"
    ),
    spec!(NoSnmpCommunity, Modes::CONFIG, Modes::BOTH, [K("no"), K("snmp-server"), K("community"), Arg("WORD")], "Forget an SNMP community"),
    spec!(SnmpLocation, Modes::CONFIG, Modes::BOTH, [K("snmp-server"), K("location"), Rest("TEXT")], "Where the device is (sysLocation)"),
    spec!(SnmpContact, Modes::CONFIG, Modes::BOTH, [K("snmp-server"), K("contact"), Rest("TEXT")], "Who looks after it (sysContact)"),
    spec!(ShowSnmp, EXECS, Modes::BOTH, [K("show"), K("snmp")], "SNMP agent settings"),
    spec!(
        SnmpHost,
        Modes::CONFIG,
        Modes::BOTH,
        [K("snmp-server"), K("host"), Arg("A.B.C.D"), Rest("[version 2c] COMMUNITY")],
        "Send SNMP traps to a manager"
    ),
    spec!(NoSnmpHost, Modes::CONFIG, Modes::BOTH, [K("no"), K("snmp-server"), K("host"), Arg("A.B.C.D")], "Stop sending traps to a manager"),
    spec!(
        NoSnmpHost,
        Modes::CONFIG,
        Modes::BOTH,
        [K("no"), K("snmp-server"), K("host"), Arg("A.B.C.D"), Rest("...")],
        "Stop sending traps to a manager"
    ),
    spec!(SnmpTraps, Modes::CONFIG, Modes::BOTH, [K("snmp-server"), K("enable"), K("traps")], "Send every supported trap"),
    spec!(
        SnmpTraps,
        Modes::CONFIG,
        Modes::BOTH,
        [K("snmp-server"), K("enable"), K("traps"), Rest("[snmp] [linkdown] [linkup] [coldstart]")],
        "Send these traps"
    ),
    spec!(NoSnmpTraps, Modes::CONFIG, Modes::BOTH, [K("no"), K("snmp-server"), K("enable"), K("traps")], "Send no traps"),
    spec!(
        NoSnmpTraps,
        Modes::CONFIG,
        Modes::BOTH,
        [K("no"), K("snmp-server"), K("enable"), K("traps"), Rest("[snmp] [linkdown] [linkup] [coldstart]")],
        "Stop sending these traps"
    ),
    spec!(
        SnmpGroup,
        Modes::CONFIG,
        Modes::BOTH,
        [K("snmp-server"), K("group"), Arg("WORD"), K("v3"), Rest("{noauth|auth} [read VIEW] [write VIEW]")],
        "An SNMPv3 group: the security level and whether its users may write"
    ),
    spec!(NoSnmpGroup, Modes::CONFIG, Modes::BOTH, [K("no"), K("snmp-server"), K("group"), Arg("WORD")], "Delete an SNMPv3 group"),
    spec!(NoSnmpGroup, Modes::CONFIG, Modes::BOTH, [K("no"), K("snmp-server"), K("group"), Arg("WORD"), Rest("...")], "Delete an SNMPv3 group"),
    spec!(
        SnmpUser,
        Modes::CONFIG,
        Modes::BOTH,
        [K("snmp-server"), K("user"), Arg("WORD"), Arg("GROUP"), K("v3")],
        "An SNMPv3 user without authentication"
    ),
    spec!(
        SnmpUser,
        Modes::CONFIG,
        Modes::BOTH,
        [K("snmp-server"), K("user"), Arg("WORD"), Arg("GROUP"), K("v3"), Rest("auth {md5|sha} PASSWORD")],
        "An SNMPv3 user who authenticates with a password"
    ),
    spec!(NoSnmpUser, Modes::CONFIG, Modes::BOTH, [K("no"), K("snmp-server"), K("user"), Arg("WORD")], "Delete an SNMPv3 user"),
    spec!(NoSnmpUser, Modes::CONFIG, Modes::BOTH, [K("no"), K("snmp-server"), K("user"), Arg("WORD"), Rest("...")], "Delete an SNMPv3 user"),
    spec!(NoTacacsHost, Modes::CONFIG, Modes::BOTH, [K("no"), K("tacacs-server"), K("host"), Arg("A.B.C.D")], "Forget a TACACS+ server"),
    spec!(LoginAuthentication, Modes::LINE, Modes::BOTH, [K("login"), K("authentication"), Arg("default")], "Use the default AAA method list"),
    spec!(Username, Modes::CONFIG, Modes::BOTH, [K("username"), Arg("WORD"), Rest("{secret|password} PASSWORD")], "Add a local user"),
    spec!(NoUsername, Modes::CONFIG, Modes::BOTH, [K("no"), K("username"), Arg("WORD")], "Delete a local user"),
    spec!(DomainName, Modes::CONFIG, Modes::BOTH, [K("ip"), K("domain-name"), Arg("WORD")], "The domain name (SSH keys need one)"),
    spec!(NoDomainName, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("domain-name")], "Remove the domain name"),
    spec!(CryptoKey, Modes::CONFIG, Modes::BOTH, [K("crypto"), K("key"), K("generate"), K("rsa")], "Generate the SSH keys (asks for the size)"),
    spec!(
        CryptoKey,
        Modes::CONFIG,
        Modes::BOTH,
        [K("crypto"), K("key"), K("generate"), K("rsa"), Rest("[general-keys] modulus <360-4096>")],
        "Generate the SSH keys"
    ),
    spec!(CryptoKeyZeroize, Modes::CONFIG, Modes::BOTH, [K("crypto"), K("key"), K("zeroize"), K("rsa")], "Delete the SSH keys"),
    spec!(SshVersion, Modes::CONFIG, Modes::BOTH, [K("ip"), K("ssh"), K("version"), Arg("{1|2}")], "SSH protocol version"),
    spec!(Ipv6UnicastRouting, Modes::CONFIG, Modes::BOTH, [K("ipv6"), K("unicast-routing")], "Forward IPv6 and advertise prefixes"),
    spec!(NoIpv6UnicastRouting, Modes::CONFIG, Modes::BOTH, [K("no"), K("ipv6"), K("unicast-routing")], "Stop forwarding IPv6"),
    spec!(
        Ipv6Route,
        Modes::CONFIG,
        Modes::BOTH,
        [K("ipv6"), K("route"), Arg("X:X::X/<0-128>"), Rest("<next hop and/or interface>")],
        "Add an IPv6 static route"
    ),
    spec!(NoIpv6Route, Modes::CONFIG, Modes::BOTH, [K("no"), K("ipv6"), K("route"), Arg("X:X::X/<0-128>")], "Remove IPv6 static routes"),
    spec!(
        NoIpv6Route,
        Modes::CONFIG,
        Modes::BOTH,
        [K("no"), K("ipv6"), K("route"), Arg("X:X::X/<0-128>"), Rest("<next hop and/or interface>")],
        "Remove an IPv6 static route"
    ),
    spec!(Ipv6Address, ANY_IF, Modes::BOTH, [K("ipv6"), K("address"), Arg("X:X::X/<0-128>")], "Add an IPv6 address"),
    spec!(Ipv6AddressEui, ANY_IF, Modes::BOTH, [K("ipv6"), K("address"), Arg("X:X::/64"), K("eui-64")], "Add an address built from the MAC address"),
    spec!(Ipv6AddressLinkLocal, ANY_IF, Modes::BOTH, [K("ipv6"), K("address"), Arg("FE80::X"), K("link-local")], "Set the link-local address"),
    spec!(NoIpv6Address, ANY_IF, Modes::BOTH, [K("no"), K("ipv6"), K("address")], "Remove every IPv6 address"),
    spec!(NoIpv6Address, ANY_IF, Modes::BOTH, [K("no"), K("ipv6"), K("address"), Rest("<address>")], "Remove one IPv6 address"),
    spec!(Ipv6Enable, ANY_IF, Modes::BOTH, [K("ipv6"), K("enable")], "IPv6 with a link-local address only"),
    spec!(NoIpv6Enable, ANY_IF, Modes::BOTH, [K("no"), K("ipv6"), K("enable")], "Turn link-local-only IPv6 off"),
    spec!(ShowIpv6IntBrief, EXECS, Modes::BOTH, [K("show"), K("ipv6"), K("interface"), K("brief")], "IPv6 addresses and status"),
    spec!(ShowIpv6Route, EXECS, Modes::BOTH, [K("show"), K("ipv6"), K("route")], "IPv6 routing table"),
    spec!(ShowIpv6Neighbors, EXECS, Modes::BOTH, [K("show"), K("ipv6"), K("neighbors")], "IPv6 neighbour cache"),
    spec!(IpRouting, Modes::CONFIG, Modes::SWITCH, [K("ip"), K("routing")], "Route between VLAN interfaces and routed ports"),
    spec!(NoIpRouting, Modes::CONFIG, Modes::SWITCH, [K("no"), K("ip"), K("routing")], "Stop routing"),
    spec!(VtpModeCmd, Modes::CONFIG, Modes::SWITCH, [K("vtp"), K("mode"), Arg("{server|client|transparent}")], "How this switch takes part in VTP"),
    spec!(VtpDomain, Modes::CONFIG, Modes::SWITCH, [K("vtp"), K("domain"), Arg("WORD")], "The VTP domain this switch belongs to"),
    spec!(VtpPassword, Modes::CONFIG, Modes::SWITCH, [K("vtp"), K("password"), Arg("WORD")], "Only switches with the same password share VLANs"),
    spec!(NoVtpPassword, Modes::CONFIG, Modes::SWITCH, [K("no"), K("vtp"), K("password")], "Remove the VTP password"),
    spec!(VtpVersion, Modes::CONFIG, Modes::SWITCH, [K("vtp"), K("version"), Arg("{1|2}")], "VTP version"),
    spec!(ShowVtpStatus, EXECS, Modes::SWITCH, [K("show"), K("vtp"), K("status")], "VTP mode, domain and revision"),
    spec!(ShowVtpPassword, EXECS, Modes::SWITCH, [K("show"), K("vtp"), K("password")], "The VTP password"),
    spec!(
        ChannelGroup,
        Modes::IF,
        Modes::SWITCH,
        [K("channel-group"), Arg("<1-48>"), K("mode"), Arg("{on|active|passive|desirable|auto}")],
        "Bundle this port into an EtherChannel"
    ),
    spec!(NoChannelGroup, Modes::IF, Modes::SWITCH, [K("no"), K("channel-group")], "Take the port out of its EtherChannel"),
    spec!(ShowEtherchannel, EXECS, Modes::SWITCH, [K("show"), K("etherchannel")], "EtherChannels and their ports"),
    spec!(ShowEtherchannel, EXECS, Modes::SWITCH, [K("show"), K("etherchannel"), K("summary")], "EtherChannels and their ports"),
    spec!(PortSecurity, Modes::IF, Modes::SWITCH, [K("switchport"), K("port-security")], "Limit the MAC addresses on this port"),
    spec!(NoPortSecurity, Modes::IF, Modes::SWITCH, [K("no"), K("switchport"), K("port-security")], "Turn port security off"),
    spec!(PortSecMax, Modes::IF, Modes::SWITCH, [K("switchport"), K("port-security"), K("maximum"), Arg("<1-132>")], "Most addresses allowed"),
    spec!(
        PortSecViolation,
        Modes::IF,
        Modes::SWITCH,
        [K("switchport"), K("port-security"), K("violation"), Arg("{shutdown|restrict|protect}")],
        "What to do with one address too many"
    ),
    spec!(
        PortSecSticky,
        Modes::IF,
        Modes::SWITCH,
        [K("switchport"), K("port-security"), K("mac-address"), K("sticky")],
        "Remember learned addresses in the configuration"
    ),
    spec!(PortSecMac, Modes::IF, Modes::SWITCH, [K("switchport"), K("port-security"), K("mac-address"), Arg("H.H.H")], "Allow this address"),
    spec!(ShowPortSecurity, EXECS, Modes::SWITCH, [K("show"), K("port-security")], "Secured ports"),
    spec!(ShowPortSecurity, EXECS, Modes::SWITCH, [K("show"), K("port-security"), Rest("{interface X | address}")], "Secured ports"),
    spec!(Telnet, EXECS, Modes::BOTH, [K("telnet"), Arg("A.B.C.D")], "Log in to another device with Telnet"),
    spec!(Ssh, EXECS, Modes::BOTH, [K("ssh"), Rest("-l USER A.B.C.D")], "Log in to another device with SSH"),
    spec!(ShowIpSsh, EXECS, Modes::BOTH, [K("show"), K("ip"), K("ssh")], "SSH status"),
    spec!(ShowSsh, EXECS, Modes::BOTH, [K("show"), K("ssh")], "SSH sessions open to this device"),
    spec!(SshTimeout, Modes::CONFIG, Modes::BOTH, [K("ip"), K("ssh"), K("time-out"), Arg("<1-120>")], "Seconds an SSH login may take"),
    spec!(SshRetries, Modes::CONFIG, Modes::BOTH, [K("ip"), K("ssh"), K("authentication-retries"), Arg("<0-5>")], "Password tries per SSH login"),
    spec!(
        SecurityMinLength,
        Modes::CONFIG,
        Modes::BOTH,
        [K("security"), K("passwords"), K("min-length"), Arg("<0-16>")],
        "Refuse shorter passwords from now on"
    ),
    spec!(NoSecurityMinLength, Modes::CONFIG, Modes::BOTH, [K("no"), K("security"), K("passwords"), K("min-length")], "Any password length"),
    spec!(
        LoginBlockFor,
        Modes::CONFIG,
        Modes::BOTH,
        [K("login"), K("block-for"), Arg("<1-65535>"), K("attempts"), Arg("<1-65535>"), K("within"), Arg("<1-65535>")],
        "After N failed logins in W seconds, refuse logins for S seconds"
    ),
    spec!(NoLoginBlockFor, Modes::CONFIG, Modes::BOTH, [K("no"), K("login"), K("block-for")], "Stop blocking logins"),
    spec!(IfSpeed, Modes::IF, Modes::BOTH, [K("speed"), Arg("{10|100|1000|auto}")], "Fix the port's speed (auto negotiates)"),
    spec!(IfSpeed, Modes::IF, Modes::BOTH, [K("no"), K("speed")], "Negotiate the speed"),
    spec!(IfDuplex, Modes::IF, Modes::BOTH, [K("duplex"), Arg("{full|half|auto}")], "Fix the port's duplex (auto negotiates)"),
    spec!(IfDuplex, Modes::IF, Modes::BOTH, [K("no"), K("duplex")], "Negotiate the duplex"),
    spec!(IfMdix, Modes::IF, Modes::BOTH, [K("mdix"), K("auto")], "Work with straight-through or crossover cables"),
    spec!(NoIfMdix, Modes::IF, Modes::BOTH, [K("no"), K("mdix"), K("auto")], "Expect the right cable type"),
    spec!(ShowProtocols, EXECS, Modes::BOTH, [K("show"), K("protocols")], "Interface status and addresses"),
    spec!(ShowIntDescription, EXECS, Modes::BOTH, [K("show"), K("interfaces"), K("description")], "Interface status and descriptions"),
    spec!(ShowLogin, EXECS, Modes::BOTH, [K("show"), K("login")], "Login blocking state"),
    spec!(ShowPrivilege, EXECS, Modes::BOTH, [K("show"), K("privilege")], "This session's privilege level"),
    spec!(ShowTechSupport, Modes::EXEC, Modes::BOTH, [K("show"), K("tech-support")], "Everything for a support request"),
    spec!(ShowBoot, EXECS, Modes::BOTH, [K("show"), K("boot")], "What the device starts from"),
    spec!(ShowEnv, EXECS, Modes::BOTH, [K("show"), K("environment")], "Power and temperature"),
    spec!(ShowInventory, EXECS, Modes::BOTH, [K("show"), K("inventory")], "Hardware"),
    spec!(DeleteVlanDat, Modes::EXEC, Modes::SWITCH, [K("delete"), Rest("flash:vlan.dat")], "Delete the VLAN database (takes effect at reload)"),
    spec!(ShowIpv6Interface, EXECS, Modes::BOTH, [K("show"), K("ipv6"), K("interface"), Rest("<interface>")], "One interface's IPv6 settings"),
    spec!(
        ShowIpv6Route,
        EXECS,
        Modes::BOTH,
        [K("show"), K("ipv6"), K("route"), Arg("{connected|static|ospf}")],
        "Only the IPv6 routes one source gave"
    ),
    spec!(ClearCounters, Modes::EXEC, Modes::BOTH, [K("clear"), K("counters")], "Reset interface counters"),
    spec!(PingSize, EXECS, Modes::BOTH, [K("ping"), Arg("A.B.C.D|X:X::X"), K("size"), Arg("<36-18024>")], "Send echo requests of a given size"),
    spec!(ShowSessions, EXECS, Modes::BOTH, [K("show"), K("sessions")], "Telnet and SSH sessions opened from here"),
    spec!(ShowHosts, EXECS, Modes::BOTH, [K("show"), K("hosts")], "Name lookup settings"),
    spec!(ShowMac, EXECS, Modes::SWITCH, [K("show"), K("mac"), K("address-table"), K("dynamic")], "Learned MAC addresses"),
    spec!(ClearMac, Modes::EXEC, Modes::SWITCH, [K("clear"), K("mac"), K("address-table"), K("dynamic")], "Forget learned MAC addresses"),
    spec!(BannerMotd, Modes::CONFIG, Modes::BOTH, [K("banner"), K("motd"), Rest("#message#")], "Message shown before login"),
    spec!(NoBannerMotd, Modes::CONFIG, Modes::BOTH, [K("no"), K("banner"), K("motd")], "Remove the message"),
    spec!(ServicePasswordEncryption, Modes::CONFIG, Modes::BOTH, [K("service"), K("password-encryption")], "Write passwords obscured"),
    spec!(NoServicePasswordEncryption, Modes::CONFIG, Modes::BOTH, [K("no"), K("service"), K("password-encryption")], "Write passwords in clear"),
    spec!(DomainLookup, Modes::CONFIG, Modes::BOTH, [K("ip"), K("domain-lookup")], "Resolve mistyped commands as host names"),
    spec!(NoDomainLookup, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("domain-lookup")], "Don't resolve mistyped commands"),
    spec!(DhcpPool, Modes::CONFIG, Modes::BOTH, [K("ip"), K("dhcp"), K("pool"), Arg("WORD")], "Create or edit a DHCP pool"),
    spec!(NoDhcpPool, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("dhcp"), K("pool"), Arg("WORD")], "Delete a DHCP pool"),
    spec!(
        DhcpExcluded,
        Modes::CONFIG,
        Modes::BOTH,
        [K("ip"), K("dhcp"), K("excluded-address"), Rest("A.B.C.D [A.B.C.D]")],
        "Addresses DHCP never hands out"
    ),
    spec!(
        NoDhcpExcluded,
        Modes::CONFIG,
        Modes::BOTH,
        [K("no"), K("ip"), K("dhcp"), K("excluded-address"), Rest("A.B.C.D [A.B.C.D]")],
        "Hand those addresses out again"
    ),
    spec!(DhcpNetwork, Modes::DHCP, Modes::BOTH, [K("network"), Arg("A.B.C.D"), Arg("MASK")], "The pool's network"),
    spec!(DhcpDefaultRouter, Modes::DHCP, Modes::BOTH, [K("default-router"), Arg("A.B.C.D")], "Gateway given to clients"),
    spec!(DhcpDns, Modes::DHCP, Modes::BOTH, [K("dns-server"), Arg("A.B.C.D")], "DNS server given to clients"),
    spec!(ShowDhcpBinding, EXECS, Modes::BOTH, [K("show"), K("ip"), K("dhcp"), K("binding")], "Addresses DHCP handed out"),
    spec!(ShowDhcpPool, EXECS, Modes::BOTH, [K("show"), K("ip"), K("dhcp"), K("pool")], "DHCP pools"),
    spec!(ClearDhcpBinding, Modes::EXEC, Modes::BOTH, [K("clear"), K("ip"), K("dhcp"), K("binding"), Rest("*")], "Forget DHCP leases"),
    spec!(
        StpPriority,
        Modes::CONFIG,
        Modes::SWITCH,
        [K("spanning-tree"), K("vlan"), Arg("<1-4094>[,-]"), K("priority"), Arg("<0-61440>")],
        "Bridge priority (multiples of 4096)"
    ),
    spec!(
        NoStpPriority,
        Modes::CONFIG,
        Modes::SWITCH,
        [K("no"), K("spanning-tree"), K("vlan"), Arg("<1-4094>[,-]"), K("priority")],
        "Back to priority 32768"
    ),
    spec!(
        StpRoot,
        Modes::CONFIG,
        Modes::SWITCH,
        [K("spanning-tree"), K("vlan"), Arg("<1-4094>[,-]"), K("root"), Arg("{primary|secondary}")],
        "Make this switch the root (or the backup root)"
    ),
    spec!(
        StpMode,
        Modes::CONFIG,
        Modes::SWITCH,
        [K("spanning-tree"), K("mode"), Arg("{pvst|rapid-pvst}")],
        "Classic (30 s to forward) or rapid spanning tree"
    ),
    spec!(Portfast, Modes::IF, Modes::SWITCH, [K("spanning-tree"), K("portfast")], "An edge port: forward at once"),
    spec!(NoPortfast, Modes::IF, Modes::SWITCH, [K("no"), K("spanning-tree"), K("portfast")], "Not an edge port"),
    spec!(ShowStp, EXECS, Modes::SWITCH, [K("show"), K("spanning-tree")], "Spanning trees of every VLAN"),
    spec!(ShowStpVlan, EXECS, Modes::SWITCH, [K("show"), K("spanning-tree"), K("vlan"), Arg("<1-4094>")], "One VLAN's spanning tree"),
    // ---- the firewall appliance ----
    spec!(FwNameif, IFS, FW, [K("nameif"), Arg("NAME")], "Name the interface (rules, routes and NAT refer to it by this name)"),
    spec!(FwNoNameif, IFS, FW, [K("no"), K("nameif")], "Take the name away: the interface stops passing traffic"),
    spec!(FwSecurityLevel, IFS, FW, [K("security-level"), Arg("<0-100>")], "How trusted the interface is (100 most, 0 least)"),
    spec!(FwNoSecurityLevel, IFS, FW, [K("no"), K("security-level")], "Back to level 0"),
    spec!(Encapsulation, Modes::SUBIF, FW, [K("vlan"), Arg("<1-4094>")], "The 802.1Q VLAN the subinterface sends and accepts"),
    spec!(NoInterface, Modes::CONFIG, FW, [K("no"), K("interface"), Rest("<subinterface>")], "Delete a subinterface"),
    spec!(
        FwRoute,
        Modes::CONFIG,
        FW,
        [K("route"), Arg("IFNAME"), Arg("A.B.C.D"), Arg("MASK"), Arg("GATEWAY")],
        "A static route out of a named interface"
    ),
    spec!(
        FwRoute,
        Modes::CONFIG,
        FW,
        [K("route"), Arg("IFNAME"), Arg("A.B.C.D"), Arg("MASK"), Arg("GATEWAY"), Arg("<1-255>")],
        "A static route out of a named interface, with a distance"
    ),
    spec!(FwNoRoute, Modes::CONFIG, FW, [K("no"), K("route"), Arg("IFNAME"), Arg("A.B.C.D"), Arg("MASK"), Arg("GATEWAY")], "Remove a static route"),
    spec!(
        FwNoRoute,
        Modes::CONFIG,
        FW,
        [K("no"), K("route"), Arg("IFNAME"), Arg("A.B.C.D"), Arg("MASK"), Arg("GATEWAY"), Arg("<1-255>")],
        "Remove a static route"
    ),
    spec!(
        FwAccessList,
        Modes::CONFIG,
        FW,
        [K("access-list"), Arg("NAME"), K("extended"), Rest("{permit|deny} PROTOCOL SOURCE [PORT] DESTINATION [PORT]")],
        "Add a rule to an access list (addresses: any, host A.B.C.D, A.B.C.D MASK, object NAME)"
    ),
    spec!(FwNoAccessList, Modes::CONFIG, FW, [K("no"), K("access-list"), Arg("NAME")], "Delete an access list"),
    spec!(
        FwNoAccessListEntry,
        Modes::CONFIG,
        FW,
        [K("no"), K("access-list"), Arg("NAME"), K("extended"), Rest("{permit|deny} RULE")],
        "Delete one rule of an access list"
    ),
    spec!(
        FwAccessGroup,
        Modes::CONFIG,
        FW,
        [K("access-group"), Arg("ACL"), K("in"), K("interface"), Arg("IFNAME")],
        "Check traffic arriving on an interface against an access list"
    ),
    spec!(
        FwNoAccessGroup,
        Modes::CONFIG,
        FW,
        [K("no"), K("access-group"), Arg("ACL"), K("in"), K("interface"), Arg("IFNAME")],
        "Stop checking an interface against the list"
    ),
    spec!(FwObjectNetwork, Modes::CONFIG, FW, [K("object"), K("network"), Arg("NAME")], "Create or edit a network object"),
    spec!(FwNoObjectNetwork, Modes::CONFIG, FW, [K("no"), K("object"), K("network"), Arg("NAME")], "Delete a network object"),
    spec!(FwObjHost, Modes::OBJECT, FW, [K("host"), Arg("A.B.C.D")], "The object is one address"),
    spec!(FwObjSubnet, Modes::OBJECT, FW, [K("subnet"), Arg("A.B.C.D"), Arg("MASK")], "The object is a subnet"),
    spec!(
        FwObjNat,
        Modes::OBJECT,
        FW,
        [K("nat"), Arg("(REAL,MAPPED)"), K("dynamic"), Arg("{interface|A.B.C.D}")],
        "Share one address among the object's hosts (port translation)"
    ),
    spec!(FwObjNat, Modes::OBJECT, FW, [K("nat"), Arg("(REAL,MAPPED)"), K("static"), Arg("A.B.C.D")], "Give the object a fixed outside address"),
    spec!(FwObjNoNat, Modes::OBJECT, FW, [K("no"), K("nat")], "Stop translating the object"),
    spec!(
        FwSameSecurity,
        Modes::CONFIG,
        FW,
        [K("same-security-traffic"), K("permit"), K("inter-interface")],
        "Let traffic pass between interfaces of equal security level"
    ),
    spec!(
        FwNoSameSecurity,
        Modes::CONFIG,
        FW,
        [K("no"), K("same-security-traffic"), K("permit"), K("inter-interface")],
        "Interfaces of equal level don't talk unless a rule says so"
    ),
    spec!(
        FwDhcpdAddress,
        Modes::CONFIG,
        FW,
        [K("dhcpd"), K("address"), Arg("A.B.C.D-A.B.C.D"), Arg("IFNAME")],
        "Addresses the DHCP server hands out on an interface"
    ),
    spec!(FwNoDhcpdAddress, Modes::CONFIG, FW, [K("no"), K("dhcpd"), K("address"), Arg("A.B.C.D-A.B.C.D"), Arg("IFNAME")], "Remove a DHCP range"),
    spec!(FwDhcpdEnable, Modes::CONFIG, FW, [K("dhcpd"), K("enable"), Arg("IFNAME")], "Run the DHCP server on an interface"),
    spec!(FwNoDhcpdEnable, Modes::CONFIG, FW, [K("no"), K("dhcpd"), K("enable"), Arg("IFNAME")], "Stop the DHCP server on an interface"),
    spec!(FwDhcpdDns, Modes::CONFIG, FW, [K("dhcpd"), K("dns"), Arg("A.B.C.D")], "The DNS server DHCP clients are told about"),
    spec!(FwNoDhcpdDns, Modes::CONFIG, FW, [K("no"), K("dhcpd"), K("dns")], "Tell DHCP clients no DNS server"),
    spec!(FwTimeout, Modes::CONFIG, FW, [K("timeout"), Arg("{conn|udp|icmp}"), Arg("H:MM:SS")], "How long an idle connection is kept"),
    spec!(FwShowNat, EXECS, FW, [K("show"), K("nat")], "NAT rules and how often each was used"),
    spec!(FwShowXlate, EXECS, FW, [K("show"), K("xlate")], "Address translations in use"),
    spec!(FwShowConn, EXECS, FW, [K("show"), K("conn")], "Connections the firewall is tracking"),
    spec!(FwShowAccessList, EXECS, FW, [K("show"), K("access-list")], "Access lists with hit counts"),
    spec!(FwShowAccessList, EXECS, FW, [K("show"), K("access-list"), Arg("NAME")], "One access list with hit counts"),
    spec!(FwShowIntIpBrief, EXECS, FW, [K("show"), K("interfaces"), K("ip"), K("brief")], "Interface addresses and status"),
    spec!(FwShowRoute, EXECS, FW, [K("show"), K("route")], "Routing table"),
    spec!(FwShowNameif, EXECS, FW, [K("show"), K("nameif")], "Interface names and security levels"),
    spec!(FwShowDhcpdBinding, EXECS, FW, [K("show"), K("dhcpd"), K("binding")], "Addresses the DHCP server handed out"),
    spec!(FwShowObjects, EXECS, FW, [K("show"), K("object")], "Network objects"),
    spec!(FwClearConn, Modes::EXEC, FW, [K("clear"), K("conn")], "Forget every tracked connection"),
    spec!(FwClearXlate, Modes::EXEC, FW, [K("clear"), K("xlate")], "Forget the dynamic translations"),
    spec!(FwClearAclCounters, Modes::EXEC, FW, [K("clear"), K("access-list"), K("counters")], "Reset access-list hit counts"),
    spec!(
        FwSameSecurity,
        Modes::CONFIG,
        FW,
        [K("same-security-traffic"), K("permit"), K("intra-interface")],
        "Let traffic leave by the interface it came in on (hairpin)"
    ),
    spec!(
        FwNoSameSecurity,
        Modes::CONFIG,
        FW,
        [K("no"), K("same-security-traffic"), K("permit"), K("intra-interface")],
        "Traffic never leaves by the interface it came in on"
    ),
    spec!(
        FwAccessGroup,
        Modes::CONFIG,
        FW,
        [K("access-group"), Arg("ACL"), K("out"), K("interface"), Arg("IFNAME")],
        "Check traffic leaving by an interface against an access list"
    ),
    spec!(
        FwNoAccessGroup,
        Modes::CONFIG,
        FW,
        [K("no"), K("access-group"), Arg("ACL"), K("out"), K("interface"), Arg("IFNAME")],
        "Stop checking traffic leaving by the interface"
    ),
    spec!(FwAclRemark, Modes::CONFIG, FW, [K("access-list"), Arg("NAME"), K("remark"), Rest("TEXT")], "A note shown before the list's next rule"),
    spec!(FwNoAclRemark, Modes::CONFIG, FW, [K("no"), K("access-list"), Arg("NAME"), K("remark"), Rest("TEXT")], "Delete a note from an access list"),
    spec!(FwObjectGroup, Modes::CONFIG, FW, [K("object-group"), K("network"), Arg("NAME")], "Create or edit a group of addresses"),
    spec!(FwObjectGroup, Modes::CONFIG, FW, [K("object-group"), K("service"), Arg("NAME")], "Create or edit a group of services"),
    spec!(
        FwObjectGroup,
        Modes::CONFIG,
        FW,
        [K("object-group"), K("service"), Arg("NAME"), Arg("{tcp|udp|tcp-udp}")],
        "Create or edit a group of ports for TCP, UDP or both"
    ),
    spec!(FwNoObjectGroup, Modes::CONFIG, FW, [K("no"), K("object-group"), K("network"), Arg("NAME")], "Delete a group of addresses"),
    spec!(FwNoObjectGroup, Modes::CONFIG, FW, [K("no"), K("object-group"), K("service"), Arg("NAME")], "Delete a group of services"),
    spec!(
        FwGrpMember,
        Modes::OBJECT,
        FW,
        [K("network-object"), Rest("{host A.B.C.D|A.B.C.D MASK|object NAME}")],
        "Add addresses to the network group"
    ),
    spec!(FwGrpMember, Modes::OBJECT, FW, [K("group-object"), Arg("NAME")], "Add another group of the same kind"),
    spec!(FwGrpMember, Modes::OBJECT, FW, [K("port-object"), Rest("{eq PORT|range PORT PORT}")], "Add ports to the service group"),
    spec!(
        FwGrpMember,
        Modes::OBJECT,
        FW,
        [K("service-object"), Rest("{ip|icmp|tcp|udp} [destination {eq PORT|range PORT PORT}]")],
        "Add a service to the service group"
    ),
    spec!(FwGrpNoMember, Modes::OBJECT, FW, [K("no"), K("network-object"), Rest("…")], "Remove addresses from the group"),
    spec!(FwGrpNoMember, Modes::OBJECT, FW, [K("no"), K("group-object"), Arg("NAME")], "Remove a nested group"),
    spec!(FwGrpNoMember, Modes::OBJECT, FW, [K("no"), K("port-object"), Rest("…")], "Remove ports from the group"),
    spec!(FwGrpNoMember, Modes::OBJECT, FW, [K("no"), K("service-object"), Rest("…")], "Remove a service from the group"),
    spec!(
        FwIcmp,
        Modes::CONFIG,
        FW,
        [K("icmp"), Arg("{permit|deny}"), Rest("{any|host A.B.C.D|A.B.C.D MASK} [TYPE] IFNAME")],
        "Which ICMP the firewall answers on an interface (to its own address)"
    ),
    spec!(FwNoIcmp, Modes::CONFIG, FW, [K("no"), K("icmp"), Arg("{permit|deny}"), Rest("SOURCE [TYPE] IFNAME")], "Remove an icmp rule"),
    spec!(
        FwTwiceNat,
        Modes::CONFIG,
        FW,
        [
            K("nat"),
            Arg("(REAL,MAPPED)"),
            K("source"),
            Rest("{static|dynamic} REAL-OBJ {MAPPED-OBJ|interface} [destination static MAPPED-OBJ REAL-OBJ]")
        ],
        "Twice NAT: translate the source, and the destination too, by rule"
    ),
    spec!(FwNoTwiceNat, Modes::CONFIG, FW, [K("no"), K("nat"), Arg("(REAL,MAPPED)"), K("source"), Rest("…")], "Remove a twice NAT rule"),
    spec!(FwShowNatDetail, EXECS, FW, [K("show"), K("nat"), K("detail")], "NAT rules in order, with what each translates and its counters"),
    spec!(FwShowObjectGroup, EXECS, FW, [K("show"), K("object-group")], "Network and service object groups"),
    spec!(FwClassMap, Modes::CONFIG, FW, [K("class-map"), Arg("NAME")], "Create or edit a traffic class for the global policy"),
    spec!(FwNoClassMap, Modes::CONFIG, FW, [K("no"), K("class-map"), Arg("NAME")], "Delete a traffic class"),
    spec!(FwClassMatch, Modes::CLASS_MAP, FW, [K("match"), K("default-inspection-traffic")], "The traffic the inspections usually look at"),
    spec!(FwClassMatch, Modes::CLASS_MAP, FW, [K("match"), K("any")], "Every packet"),
    spec!(FwNoClassMatch, Modes::CLASS_MAP, FW, [K("no"), K("match"), Rest("CRITERION")], "Remove the match line"),
    spec!(FwPolicyMap, Modes::CONFIG, FW, [K("policy-map"), Arg("NAME")], "Create or edit a policy (what to do with each class)"),
    spec!(FwNoPolicyMap, Modes::CONFIG, FW, [K("no"), K("policy-map"), Arg("NAME")], "Delete a policy"),
    spec!(FwPolicyClass, Modes::POLICY_MAP.or(Modes::POLICY_CLASS), FW, [K("class"), Arg("NAME")], "Say what to do with a class (or class-default)"),
    spec!(FwNoPolicyClass, Modes::POLICY_MAP.or(Modes::POLICY_CLASS), FW, [K("no"), K("class"), Arg("NAME")], "Remove a class from the policy"),
    spec!(
        FwPolicyAction,
        Modes::POLICY_CLASS,
        FW,
        [K("inspect"), Rest("icmp [error]")],
        "Track ICMP like a connection (echo replies come back by themselves)"
    ),
    spec!(
        FwPolicyAction,
        Modes::POLICY_CLASS,
        FW,
        [K("set"), K("connection"), K("decrement-ttl")],
        "Count the firewall as a hop (lower TTL, answer traceroute)"
    ),
    spec!(FwNoPolicyAction, Modes::POLICY_CLASS, FW, [K("no"), K("inspect"), Rest("icmp [error]")], "Stop the inspection"),
    spec!(FwNoPolicyAction, Modes::POLICY_CLASS, FW, [K("no"), K("set"), K("connection"), K("decrement-ttl")], "Leave TTL alone again"),
    spec!(FwServicePolicy, Modes::CONFIG, FW, [K("service-policy"), Arg("NAME"), K("global")], "Apply a policy to every interface"),
    spec!(FwNoServicePolicy, Modes::CONFIG, FW, [K("no"), K("service-policy"), Arg("NAME"), K("global")], "Stop applying the global policy"),
    spec!(FwShowServicePolicy, EXECS, FW, [K("show"), K("service-policy")], "The global policy and what its inspections saw"),
    // Voice: call control on routers, voice VLANs on switch ports.
    spec!(VoiceTelephonyService, Modes::CONFIG, Modes::ROUTER, [K("telephony-service")], "Set up call control for IP phones"),
    spec!(VoiceNoTelephonyService, Modes::CONFIG, Modes::ROUTER, [K("no"), K("telephony-service")], "Turn call control off"),
    spec!(VoiceMaxEphones, Modes::TELEPHONY, Modes::ROUTER, [K("max-ephones"), Arg("<1-256>")], "How many phones may register"),
    spec!(VoiceMaxDn, Modes::TELEPHONY, Modes::ROUTER, [K("max-dn"), Arg("<1-256>")], "How many directory numbers may be defined"),
    spec!(VoiceSourceAddress, Modes::TELEPHONY, Modes::ROUTER, [K("ip"), K("source-address"), Arg("A.B.C.D")], "The address phones register at"),
    spec!(
        VoiceSourceAddress,
        Modes::TELEPHONY,
        Modes::ROUTER,
        [K("ip"), K("source-address"), Arg("A.B.C.D"), K("port"), Arg("<1-65535>")],
        "The address and UDP port phones register at"
    ),
    spec!(VoiceNoSourceAddress, Modes::TELEPHONY, Modes::ROUTER, [K("no"), K("ip"), K("source-address")], "Phones register at any address"),
    spec!(
        VoiceAutoAssign,
        Modes::TELEPHONY,
        Modes::ROUTER,
        [K("auto"), K("assign"), Arg("<1-1000>"), K("to"), Arg("<1-1000>")],
        "Hand these directory numbers to phones with no ephone entry"
    ),
    spec!(VoiceNoAutoAssign, Modes::TELEPHONY, Modes::ROUTER, [K("no"), K("auto"), K("assign")], "Stop handing out numbers"),
    spec!(VoiceEphoneDn, Modes::CONFIG, Modes::ROUTER, [K("ephone-dn"), Arg("<1-1000>")], "Define or edit a directory number"),
    spec!(VoiceNoEphoneDn, Modes::CONFIG, Modes::ROUTER, [K("no"), K("ephone-dn"), Arg("<1-1000>")], "Delete a directory number"),
    spec!(VoiceNumber, Modes::EPHONE, Modes::ROUTER, [K("number"), Arg("DIGITS")], "The number (under ephone-dn)"),
    spec!(VoiceNoNumber, Modes::EPHONE, Modes::ROUTER, [K("no"), K("number")], "Remove the number (under ephone-dn)"),
    spec!(VoiceEphone, Modes::CONFIG, Modes::ROUTER, [K("ephone"), Arg("<1-1000>")], "Define or edit a phone"),
    spec!(VoiceNoEphone, Modes::CONFIG, Modes::ROUTER, [K("no"), K("ephone"), Arg("<1-1000>")], "Delete a phone"),
    spec!(VoiceMacAddress, Modes::EPHONE, Modes::ROUTER, [K("mac-address"), Arg("H.H.H")], "The phone's MAC address (under ephone)"),
    spec!(VoiceButton, Modes::EPHONE, Modes::ROUTER, [K("button"), Arg("BUTTON:DN")], "Put a directory number on a line button (under ephone)"),
    spec!(VoiceNoButton, Modes::EPHONE, Modes::ROUTER, [K("no"), K("button")], "Clear the line buttons (under ephone)"),
    spec!(
        VoiceDhcpOption150,
        Modes::DHCP,
        Modes::ROUTER,
        [K("option"), K("150"), K("ip"), Arg("A.B.C.D")],
        "Tell IP phones the TFTP server (where they register)"
    ),
    spec!(VoiceNoDhcpOption150, Modes::DHCP, Modes::ROUTER, [K("no"), K("option"), K("150")], "Stop telling phones a TFTP server"),
    spec!(
        SwVoiceVlan,
        Modes::IF,
        Modes::SWITCH,
        [K("switchport"), K("voice"), K("vlan"), Arg("<1-4094>")],
        "Phones on this access port send their traffic tagged in this VLAN"
    ),
    spec!(NoSwVoiceVlan, Modes::IF, Modes::SWITCH, [K("no"), K("switchport"), K("voice"), K("vlan")], "No voice VLAN on this port"),
    spec!(ShowEphone, EXECS, Modes::ROUTER, [K("show"), K("ephone")], "Phones: configured and registered"),
    spec!(ShowEphoneDn, EXECS, Modes::ROUTER, [K("show"), K("ephone-dn")], "Directory numbers and their state"),
    spec!(ShowTelephony, EXECS, Modes::ROUTER, [K("show"), K("telephony-service")], "Call control settings and counts"),
];

/// Why a line didn't parse. Positions are word indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Unknown(usize),
    Ambiguous(usize, Vec<&'static str>),
    Incomplete,
}

/// The commands available in a mode on a device.
pub fn available(mode: Modes, device: Modes) -> impl Iterator<Item = &'static Spec> {
    SPECS.iter().filter(move |s| s.modes.has(mode) && s.devices.has(device))
}

/// Matches words to a command; returns it with its argument words (a `Rest` joins the rest).
pub fn resolve(words: &[&str], mode: Modes, device: Modes) -> Result<(&'static Spec, Vec<String>), ParseError> {
    let mut cands: Vec<&'static Spec> = available(mode, device).collect();
    let mut finished: Vec<(&'static Spec, usize)> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let lw = w.to_ascii_lowercase();
        let kw_at = |s: &Spec| match s.path.get(i) {
            Some(K(k)) => Some(*k),
            _ => None,
        };
        let exact: Vec<&'static str> = cands.iter().filter_map(|s| kw_at(s)).filter(|k| k.eq_ignore_ascii_case(&lw)).collect();
        let mut prefix: Vec<&'static str> = cands.iter().filter_map(|s| kw_at(s)).filter(|k| k.to_ascii_lowercase().starts_with(&lw)).collect();
        prefix.sort_unstable();
        prefix.dedup();
        let chosen = if let Some(k) = exact.first() {
            Some(*k)
        } else if prefix.len() == 1 {
            prefix.first().copied()
        } else if prefix.len() > 1 {
            return Err(ParseError::Ambiguous(i, prefix));
        } else {
            None
        };
        // A keyword match beats free-form arguments at the same position.
        match chosen {
            Some(k) => cands.retain(|s| kw_at(s) == Some(k)),
            None => {
                for s in cands.iter().filter(|s| matches!(s.path.get(i), Some(Rest(_)))) {
                    finished.push((s, i));
                }
                cands.retain(|s| matches!(s.path.get(i), Some(Arg(_))));
            }
        }
        if cands.is_empty() {
            break;
        }
    }
    // A full keyword/argument match first, then a command whose free text took the tail.
    if let Some(s) = cands.iter().find(|s| s.path.len() == words.len()) {
        return Ok((s, args(s, words, None)));
    }
    if let Some((s, at)) = finished.into_iter().max_by_key(|(_, at)| *at) {
        return Ok((s, args(s, words, Some(at))));
    }
    if !cands.is_empty() {
        return Err(ParseError::Incomplete);
    }
    Err(ParseError::Unknown(last_known(words, mode, device)))
}

fn args(s: &Spec, words: &[&str], rest_at: Option<usize>) -> Vec<String> {
    let mut out = Vec::new();
    for (i, t) in s.path.iter().enumerate() {
        match t {
            Arg(_) => out.push(words.get(i).map(|w| w.to_string()).unwrap_or_default()),
            Rest(_) if rest_at == Some(i) => out.push(words.get(i..).map(|w| w.join(" ")).unwrap_or_default()),
            _ => {}
        }
    }
    out
}

/// The first word that stops matching anything (for the `^` marker).
fn last_known(words: &[&str], mode: Modes, device: Modes) -> usize {
    (1..=words.len()).find(|&n| words.get(..n).is_some_and(|w| matches!(resolve_prefix(w, mode, device), PrefixState::Dead))).map_or(0, |n| n - 1)
}

enum PrefixState {
    Alive(Vec<&'static Spec>),
    Dead,
}

fn resolve_prefix(words: &[&str], mode: Modes, device: Modes) -> PrefixState {
    let mut cands: Vec<&'static Spec> = available(mode, device).collect();
    for (i, w) in words.iter().enumerate() {
        let lw = w.to_ascii_lowercase();
        if cands.iter().any(|s| matches!(s.path.get(i), Some(Rest(_))))
            && !cands.iter().any(|s| matches!(s.path.get(i), Some(K(k)) if k.starts_with(&lw)))
        {
            return PrefixState::Alive(cands.into_iter().filter(|s| matches!(s.path.get(i), Some(Rest(_)))).collect());
        }
        cands.retain(|s| match s.path.get(i) {
            Some(K(k)) => k.to_ascii_lowercase().starts_with(&lw),
            Some(Arg(_)) | Some(Rest(_)) => true,
            None => false,
        });
        if cands.is_empty() {
            return PrefixState::Dead;
        }
    }
    PrefixState::Alive(cands)
}

/// `?` help: the possible next words after `words` (when `partial` is empty) or the keywords
/// that complete `partial`, with their help text.
pub fn help(words: &[&str], partial: &str, mode: Modes, device: Modes) -> Vec<(String, &'static str)> {
    let PrefixState::Alive(cands) = resolve_prefix(words, mode, device) else { return Vec::new() };
    let i = words.len();
    let lp = partial.to_ascii_lowercase();
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for s in cands {
        let item = match s.path.get(i) {
            Some(K(k)) if k.starts_with(&lp) => Some((k.to_string(), s.help)),
            Some(Arg(a)) | Some(Rest(a)) if partial.is_empty() => Some((a.to_string(), s.help)),
            None if partial.is_empty() && s.path.len() == i => Some(("<cr>".to_string(), "")),
            _ => None,
        };
        if let Some(item) = item
            && !out.iter().any(|(w, _)| *w == item.0)
        {
            out.push(item);
        }
    }
    out
}

/// Tab completion of the last word: the keyword it uniquely starts, if any.
pub fn complete(words: &[&str], partial: &str, mode: Modes, device: Modes) -> Option<&'static str> {
    if partial.is_empty() {
        return None;
    }
    let PrefixState::Alive(cands) = resolve_prefix(words, mode, device) else { return None };
    let lp = partial.to_ascii_lowercase();
    let mut kws: Vec<&'static str> = cands
        .iter()
        .filter_map(|s| match s.path.get(words.len()) {
            Some(K(k)) if k.starts_with(&lp) => Some(*k),
            _ => None,
        })
        .collect();
    kws.sort_unstable();
    kws.dedup();
    if kws.len() == 1 { kws.first().copied() } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Modes = Modes::ROUTER;
    const S: Modes = Modes::SWITCH;

    #[test]
    fn abbreviations_resolve() {
        let (s, _) = resolve(&["conf", "t"], Modes::EXEC, R).unwrap();
        assert_eq!(s.id, Id::ConfigureTerminal);
        let (s, _) = resolve(&["sh", "ip", "int", "br"], Modes::EXEC, R).unwrap();
        assert_eq!(s.id, Id::ShowIpIntBrief);
        let (s, a) = resolve(&["int", "g0/0.10"], Modes::CONFIG, R).unwrap();
        assert_eq!((s.id, a), (Id::Interface, vec!["g0/0.10".to_string()]));
        let (s, a) = resolve(&["int", "ra", "f0/1", "-", "4"], Modes::CONFIG, S).unwrap();
        assert_eq!((s.id, a), (Id::InterfaceRange, vec!["f0/1 - 4".to_string()]));
        let (s, a) = resolve(&["ip", "route", "0.0.0.0", "0.0.0.0", "10.0.0.1"], Modes::CONFIG, R).unwrap();
        assert_eq!((s.id, a.len()), (Id::IpRoute, 3));
        let (s, _) = resolve(&["sh", "int", "trunk"], Modes::EXEC, S).unwrap();
        assert_eq!(s.id, Id::ShowIntTrunk);
        let (s, a) = resolve(&["sh", "int", "fa0/1"], Modes::EXEC, S).unwrap();
        assert_eq!((s.id, a), (Id::ShowInterface, vec!["fa0/1".to_string()]));
        let (s, _) = resolve(&["wr"], Modes::EXEC, R).unwrap();
        assert_eq!(s.id, Id::WriteMemory);
        let (s, a) = resolve(&["ping", "1.2.3.4", "rep", "3"], Modes::USER, R).unwrap();
        assert_eq!((s.id, a), (Id::PingRepeat, vec!["1.2.3.4".to_string(), "3".to_string()]));
    }

    #[test]
    fn errors() {
        assert!(matches!(resolve(&["c"], Modes::EXEC, R), Err(ParseError::Ambiguous(0, _))));
        assert_eq!(resolve(&["configure"], Modes::EXEC, R).unwrap_err(), ParseError::Incomplete);
        assert_eq!(resolve(&["show", "bogus"], Modes::EXEC, R).unwrap_err(), ParseError::Unknown(1));
        assert_eq!(resolve(&["frobnicate"], Modes::EXEC, R).unwrap_err(), ParseError::Unknown(0));
        // Mode and device gate commands.
        assert!(resolve(&["configure", "terminal"], Modes::USER, R).is_err());
        assert!(resolve(&["vlan", "10"], Modes::CONFIG, R).is_err());
        assert!(resolve(&["no", "interface", "g0/0.1"], Modes::CONFIG, S).is_err());
    }

    #[test]
    fn help_and_completion() {
        let h = help(&["show"], "", Modes::EXEC, R);
        assert!(h.iter().any(|(w, _)| w == "running-config") && h.iter().any(|(w, _)| w == "ip"));
        assert!(!h.iter().any(|(w, _)| w == "vlan"));
        let h = help(&[], "con", Modes::EXEC, R);
        assert_eq!(h.iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(), ["configure"]);
        assert_eq!(complete(&["show"], "run", Modes::EXEC, R), Some("running-config"));
        assert_eq!(complete(&[], "c", Modes::EXEC, R), None);
        assert!(help(&["show", "ip", "route"], "", Modes::EXEC, R).iter().any(|(w, _)| w == "<cr>"));
    }

    #[test]
    fn every_spec_is_reachable_by_its_full_words() {
        for s in SPECS {
            let words: Vec<&str> = s
                .path
                .iter()
                .map(|t| match t {
                    K(k) => *k,
                    Arg(_) | Rest(_) => "1",
                })
                .collect();
            let mode = [
                Modes::USER,
                Modes::EXEC,
                Modes::CONFIG,
                Modes::IF,
                Modes::SUBIF,
                Modes::VLAN,
                Modes::LINE,
                Modes::DHCP,
                Modes::SVI,
                Modes::OSPF,
                Modes::ACL,
                Modes::RIP,
                Modes::ACL6,
                Modes::DHCP6,
                Modes::OSPF6,
                Modes::EIGRP,
                Modes::TUNNEL,
                Modes::ISAKMP,
                Modes::CRYPTO_MAP,
                Modes::TRANSFORM,
                Modes::CLASS_MAP,
                Modes::POLICY_MAP,
                Modes::POLICY_CLASS,
                Modes::OBJECT,
                Modes::TELEPHONY,
                Modes::EPHONE,
            ]
            .into_iter()
            .find(|m| s.modes.has(*m))
            .unwrap();
            let dev = if s.devices.has(R) {
                R
            } else if s.devices.has(S) {
                S
            } else {
                Modes::FIREWALL
            };
            let got = resolve(&words, mode, dev).unwrap_or_else(|e| panic!("{:?}: {e:?}", s.path));
            assert_eq!(got.0.id, s.id, "{:?}", s.path);
        }
    }
}
