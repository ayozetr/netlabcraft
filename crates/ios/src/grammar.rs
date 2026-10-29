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
pub struct Modes(pub u16);

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
    /// Every command mode, either device.
    pub const ANY_MODE: Modes = Modes(63 | 256 | 512 | 1024 | 2048 | 4096 | 8192);
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
    Username,
    NoUsername,
    DomainName,
    NoDomainName,
    CryptoKey,
    CryptoKeyZeroize,
    SshVersion,
    LoginLocal,
    TransportInput,
    Telnet,
    Ssh,
    ShowIpSsh,
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
}

use Tok::{Arg, K, Rest};

const EXECS: Modes = Modes::USER.or(Modes::EXEC);
const SUBMODES: Modes =
    Modes::IF.or(Modes::SUBIF).or(Modes::VLAN).or(Modes::LINE).or(Modes::DHCP).or(Modes::SVI).or(Modes::OSPF).or(Modes::ACL).or(Modes::RIP);
const IFS: Modes = Modes::IF.or(Modes::SUBIF);
const ANY_IF: Modes = IFS.or(Modes::SVI);

macro_rules! spec {
    ($id:ident, $modes:expr, $dev:expr, [$($t:expr),*], $help:literal) => {
        Spec { id: Id::$id, modes: $modes, devices: $dev, path: &[$($t),*], help: $help }
    };
}

/// Every command. Order matters only for help listings.
pub static SPECS: &[Spec] = &[
    spec!(Enable, Modes::USER, Modes::BOTH, [K("enable")], "Turn on privileged commands"),
    spec!(Disable, Modes::EXEC, Modes::BOTH, [K("disable")], "Turn off privileged commands"),
    spec!(Exit, Modes::ANY_MODE, Modes::BOTH, [K("exit")], "Leave the current mode"),
    spec!(Logout, EXECS, Modes::BOTH, [K("logout")], "End the session"),
    spec!(End, Modes::CONFIG.or(SUBMODES), Modes::BOTH, [K("end")], "Return to privileged mode"),
    spec!(ConfigureTerminal, Modes::EXEC, Modes::BOTH, [K("configure"), K("terminal")], "Configure from the terminal"),
    spec!(Ping, EXECS, Modes::BOTH, [K("ping"), Arg("A.B.C.D|X:X::X")], "Send echo requests"),
    spec!(PingRepeat, EXECS, Modes::BOTH, [K("ping"), Arg("A.B.C.D"), K("repeat"), Arg("<1-1000>")], "Send echo requests"),
    spec!(Traceroute, EXECS, Modes::BOTH, [K("traceroute"), Arg("A.B.C.D|X:X::X")], "Trace the route to a destination"),
    spec!(ShowRun, Modes::EXEC, Modes::BOTH, [K("show"), K("running-config")], "Current configuration"),
    spec!(ShowStartup, Modes::EXEC, Modes::BOTH, [K("show"), K("startup-config")], "Saved configuration"),
    spec!(ShowIpIntBrief, EXECS, Modes::BOTH, [K("show"), K("ip"), K("interface"), K("brief")], "Interface addresses and status"),
    spec!(ShowIpRoute, EXECS, Modes::BOTH, [K("show"), K("ip"), K("route")], "Routing table"),
    spec!(ShowArp, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("arp")], "ARP cache"),
    spec!(ShowArp, EXECS, Modes::ROUTER, [K("show"), K("arp")], "ARP cache"),
    spec!(ShowInterfaces, EXECS, Modes::BOTH, [K("show"), K("interfaces")], "Interface details"),
    spec!(ShowIntTrunk, EXECS, Modes::SWITCH, [K("show"), K("interfaces"), K("trunk")], "Trunk ports"),
    spec!(ShowInterface, EXECS, Modes::BOTH, [K("show"), K("interfaces"), Rest("<interface>")], "One interface's details"),
    spec!(ShowVlan, EXECS, Modes::SWITCH, [K("show"), K("vlan")], "VLAN database"),
    spec!(ShowVlanBrief, EXECS, Modes::SWITCH, [K("show"), K("vlan"), K("brief")], "VLAN database"),
    spec!(ShowMac, EXECS, Modes::SWITCH, [K("show"), K("mac"), K("address-table")], "Learned MAC addresses"),
    spec!(ShowVersion, EXECS, Modes::BOTH, [K("show"), K("version")], "Software and hardware"),
    spec!(ShowHistory, EXECS, Modes::BOTH, [K("show"), K("history")], "Commands typed in this session"),
    spec!(CopyRunStart, Modes::EXEC, Modes::BOTH, [K("copy"), K("running-config"), K("startup-config")], "Save the configuration"),
    spec!(WriteMemory, Modes::EXEC, Modes::BOTH, [K("write"), K("memory")], "Save the configuration"),
    spec!(WriteMemory, Modes::EXEC, Modes::BOTH, [K("write")], "Save the configuration"),
    spec!(EraseStartup, Modes::EXEC, Modes::BOTH, [K("erase"), K("startup-config")], "Delete the saved configuration"),
    spec!(Reload, Modes::EXEC, Modes::BOTH, [K("reload")], "Restart with the saved configuration"),
    spec!(ClearMac, Modes::EXEC, Modes::SWITCH, [K("clear"), K("mac"), K("address-table")], "Forget learned MAC addresses"),
    spec!(ClearArp, Modes::EXEC, Modes::ROUTER, [K("clear"), K("arp-cache")], "Forget ARP entries"),
    spec!(Hostname, Modes::CONFIG, Modes::BOTH, [K("hostname"), Arg("WORD")], "Set the device name"),
    spec!(InterfaceRange, Modes::CONFIG, Modes::BOTH, [K("interface"), K("range"), Rest("<interfaces>")], "Configure several interfaces"),
    spec!(Interface, Modes::CONFIG, Modes::BOTH, [K("interface"), Rest("<interface>")], "Configure an interface"),
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
    spec!(Do, Modes::CONFIG.or(SUBMODES), Modes::BOTH, [K("do"), Rest("<command>")], "Run a privileged command"),
    spec!(IpAddress, IFS, Modes::ROUTER, [K("ip"), K("address"), Arg("A.B.C.D"), Arg("MASK")], "Set the interface address"),
    spec!(NoIpAddress, IFS, Modes::ROUTER, [K("no"), K("ip"), K("address")], "Remove the interface address"),
    spec!(Shutdown, ANY_IF, Modes::BOTH, [K("shutdown")], "Disable the interface"),
    spec!(NoShutdown, ANY_IF, Modes::BOTH, [K("no"), K("shutdown")], "Enable the interface"),
    spec!(Description, ANY_IF, Modes::BOTH, [K("description"), Rest("LINE")], "Describe the interface"),
    spec!(NoDescription, ANY_IF, Modes::BOTH, [K("no"), K("description")], "Remove the description"),
    spec!(IpAddress, Modes::SVI, Modes::SWITCH, [K("ip"), K("address"), Arg("A.B.C.D"), Arg("MASK")], "Set the management address"),
    spec!(NoIpAddress, Modes::SVI, Modes::SWITCH, [K("no"), K("ip"), K("address")], "Remove the management address"),
    spec!(
        AccessList,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("access-list"), Arg("<1-199>"), Rest("{permit|deny|remark} ...")],
        "Add a line to a numbered access list"
    ),
    spec!(NoAccessList, Modes::CONFIG, Modes::ROUTER, [K("no"), K("access-list"), Arg("<1-199>")], "Delete a numbered access list"),
    spec!(
        IpAccessListStandard,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("ip"), K("access-list"), K("standard"), Arg("WORD")],
        "Create or edit a named standard list"
    ),
    spec!(
        IpAccessListExtended,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("ip"), K("access-list"), K("extended"), Arg("WORD")],
        "Create or edit a named extended list"
    ),
    spec!(
        NoIpAccessList,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("no"), K("ip"), K("access-list"), Arg("{standard|extended}"), Arg("WORD")],
        "Delete a named list"
    ),
    spec!(AclEntryLine, Modes::ACL, Modes::ROUTER, [K("permit"), Rest("...")], "Allow matching packets"),
    spec!(AclEntryLine, Modes::ACL, Modes::ROUTER, [K("deny"), Rest("...")], "Drop matching packets"),
    spec!(Accepted, Modes::ACL, Modes::ROUTER, [K("remark"), Rest("LINE")], "A comment (accepted, not kept)"),
    spec!(NoAclEntryLine, Modes::ACL, Modes::ROUTER, [K("no"), Rest("{permit|deny} ...")], "Remove a line"),
    spec!(AccessGroup, IFS, Modes::ROUTER, [K("ip"), K("access-group"), Arg("WORD"), Arg("{in|out}")], "Filter this interface's traffic"),
    spec!(NoAccessGroup, IFS, Modes::ROUTER, [K("no"), K("ip"), K("access-group"), Arg("WORD"), Arg("{in|out}")], "Stop filtering"),
    spec!(Accepted, Modes::LINE, Modes::BOTH, [K("access-class"), Rest("WORD {in|out}")], "Restrict remote logins (accepted, not simulated)"),
    spec!(ShowAccessLists, EXECS, Modes::ROUTER, [K("show"), K("access-lists")], "Access lists and their matches"),
    spec!(ShowAccessLists, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("access-lists")], "Access lists and their matches"),
    spec!(ClearAclCounters, Modes::EXEC, Modes::ROUTER, [K("clear"), K("access-list"), K("counters")], "Reset the match counts"),
    spec!(NatSideCmd, IFS, Modes::ROUTER, [K("ip"), K("nat"), Arg("{inside|outside}")], "Mark this interface inside or outside NAT"),
    spec!(NoNatSide, IFS, Modes::ROUTER, [K("no"), K("ip"), K("nat"), Arg("{inside|outside}")], "Take this interface out of NAT"),
    spec!(
        NatSource,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("ip"), K("nat"), K("inside"), K("source"), Rest("{static A B | list ACL {interface IF | pool NAME} [overload]}")],
        "Translate inside addresses"
    ),
    spec!(
        NoNatSource,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("no"), K("ip"), K("nat"), K("inside"), K("source"), Rest("...")],
        "Remove a translation rule"
    ),
    spec!(
        NatPool,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("ip"), K("nat"), K("pool"), Arg("WORD"), Rest("START END {netmask M | prefix-length N}")],
        "Addresses to translate to"
    ),
    spec!(NoNatPool, Modes::CONFIG, Modes::ROUTER, [K("no"), K("ip"), K("nat"), K("pool"), Arg("WORD")], "Delete a NAT pool"),
    spec!(ShowNat, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("nat"), K("translations")], "Translations in use"),
    spec!(ClearNat, Modes::EXEC, Modes::ROUTER, [K("clear"), K("ip"), K("nat"), K("translation"), Rest("*")], "Forget dynamic translations"),
    spec!(RouterRip, Modes::CONFIG, Modes::ROUTER, [K("router"), K("rip")], "Start or configure RIP"),
    spec!(NoRouterRip, Modes::CONFIG, Modes::ROUTER, [K("no"), K("router"), K("rip")], "Stop RIP"),
    spec!(RipVersion, Modes::RIP, Modes::ROUTER, [K("version"), Arg("{1|2}")], "RIP version"),
    spec!(RipNetwork, Modes::RIP, Modes::ROUTER, [K("network"), Arg("A.B.C.D")], "Run RIP on this classful network's interfaces"),
    spec!(NoRipNetwork, Modes::RIP, Modes::ROUTER, [K("no"), K("network"), Arg("A.B.C.D")], "Stop RIP on these interfaces"),
    spec!(RipPassive, Modes::RIP, Modes::ROUTER, [K("passive-interface"), Rest("<interface>")], "Advertise but send no updates there"),
    spec!(NoRipPassive, Modes::RIP, Modes::ROUTER, [K("no"), K("passive-interface"), Rest("<interface>")], "Send updates there again"),
    spec!(RipDefaultOriginate, Modes::RIP, Modes::ROUTER, [K("default-information"), K("originate")], "Advertise this router's default route"),
    spec!(
        NoRipDefaultOriginate,
        Modes::RIP,
        Modes::ROUTER,
        [K("no"), K("default-information"), K("originate")],
        "Stop advertising the default route"
    ),
    spec!(RipAutoSummary, Modes::RIP, Modes::ROUTER, [K("auto-summary")], "Summarise at classful boundaries (kept, not simulated)"),
    spec!(NoRipAutoSummary, Modes::RIP, Modes::ROUTER, [K("no"), K("auto-summary")], "Advertise subnets as they are"),
    spec!(ShowIpRipDatabase, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("rip"), K("database")], "Routes RIP knows"),
    spec!(RouterOspf, Modes::CONFIG, Modes::ROUTER, [K("router"), K("ospf"), Arg("<1-65535>")], "Start or configure OSPF"),
    spec!(NoRouterOspf, Modes::CONFIG, Modes::ROUTER, [K("no"), K("router"), K("ospf"), Arg("<1-65535>")], "Stop OSPF"),
    spec!(
        OspfNetwork,
        Modes::OSPF,
        Modes::ROUTER,
        [K("network"), Arg("A.B.C.D"), Arg("WILDCARD"), K("area"), Arg("<0-4294967295>")],
        "Run OSPF on matching interfaces"
    ),
    spec!(
        NoOspfNetwork,
        Modes::OSPF,
        Modes::ROUTER,
        [K("no"), K("network"), Arg("A.B.C.D"), Arg("WILDCARD"), K("area"), Arg("<0-4294967295>")],
        "Stop OSPF on matching interfaces"
    ),
    spec!(OspfRouterId, Modes::OSPF, Modes::ROUTER, [K("router-id"), Arg("A.B.C.D")], "Set the router id"),
    spec!(NoOspfRouterId, Modes::OSPF, Modes::ROUTER, [K("no"), K("router-id")], "Use the highest interface address"),
    spec!(OspfPassive, Modes::OSPF, Modes::ROUTER, [K("passive-interface"), Rest("<interface>")], "Advertise but form no neighbours"),
    spec!(NoOspfPassive, Modes::OSPF, Modes::ROUTER, [K("no"), K("passive-interface"), Rest("<interface>")], "Form neighbours again"),
    spec!(OspfDefaultOriginate, Modes::OSPF, Modes::ROUTER, [K("default-information"), K("originate")], "Advertise this router's default route"),
    spec!(
        NoOspfDefaultOriginate,
        Modes::OSPF,
        Modes::ROUTER,
        [K("no"), K("default-information"), K("originate")],
        "Stop advertising the default route"
    ),
    spec!(
        Accepted,
        Modes::OSPF,
        Modes::ROUTER,
        [K("auto-cost"), K("reference-bandwidth"), Arg("<1-4294967>")],
        "Reference bandwidth (accepted; costs use 100 Mbit/s)"
    ),
    spec!(ShowOspfNeighbor, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("ospf"), K("neighbor")], "OSPF neighbours"),
    spec!(ShowIpProtocols, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("protocols")], "Routing protocols running"),
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
    spec!(
        EnablePassword,
        Modes::CONFIG,
        Modes::BOTH,
        [K("enable"), K("password"), Rest("[0|7] PASSWORD")],
        "Protect privileged mode with a password"
    ),
    spec!(NoEnableSecret, Modes::CONFIG, Modes::BOTH, [K("no"), K("enable"), K("secret")], "Remove the enable secret"),
    spec!(NoEnablePassword, Modes::CONFIG, Modes::BOTH, [K("no"), K("enable"), K("password")], "Remove the enable password"),
    spec!(LineConsole, Modes::CONFIG, Modes::BOTH, [K("line"), K("console"), Arg("0")], "Configure the console line"),
    spec!(LineVty, Modes::CONFIG, Modes::BOTH, [K("line"), K("vty"), Rest("<first> [<last>]")], "Configure the remote-access lines"),
    spec!(LinePassword, Modes::LINE, Modes::BOTH, [K("password"), Rest("[0|7] PASSWORD")], "Set the line password"),
    spec!(NoLinePassword, Modes::LINE, Modes::BOTH, [K("no"), K("password")], "Remove the line password"),
    spec!(Login, Modes::LINE, Modes::BOTH, [K("login")], "Ask for the line password"),
    spec!(NoLogin, Modes::LINE, Modes::BOTH, [K("no"), K("login")], "Don't ask for a password"),
    spec!(Accepted, Modes::LINE, Modes::BOTH, [K("exec-timeout"), Rest("<minutes> [<seconds>]")], "Idle timeout (accepted, not simulated)"),
    spec!(Accepted, Modes::LINE, Modes::BOTH, [K("logging"), K("synchronous")], "Keep messages from breaking your typing (accepted)"),
    spec!(TransportInput, Modes::LINE, Modes::BOTH, [K("transport"), K("input"), Rest("{ssh|telnet|all|none}")], "Protocols remote logins may use"),
    spec!(LoginLocal, Modes::LINE, Modes::BOTH, [K("login"), K("local")], "Ask for a username and password (username lines)"),
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
    spec!(Ipv6UnicastRouting, Modes::CONFIG, Modes::ROUTER, [K("ipv6"), K("unicast-routing")], "Forward IPv6 and advertise prefixes"),
    spec!(NoIpv6UnicastRouting, Modes::CONFIG, Modes::ROUTER, [K("no"), K("ipv6"), K("unicast-routing")], "Stop forwarding IPv6"),
    spec!(
        Ipv6Route,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("ipv6"), K("route"), Arg("X:X::X/<0-128>"), Rest("<next hop and/or interface>")],
        "Add an IPv6 static route"
    ),
    spec!(NoIpv6Route, Modes::CONFIG, Modes::ROUTER, [K("no"), K("ipv6"), K("route"), Arg("X:X::X/<0-128>")], "Remove IPv6 static routes"),
    spec!(
        NoIpv6Route,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("no"), K("ipv6"), K("route"), Arg("X:X::X/<0-128>"), Rest("<next hop and/or interface>")],
        "Remove an IPv6 static route"
    ),
    spec!(Ipv6Address, Modes::IF, Modes::ROUTER, [K("ipv6"), K("address"), Arg("X:X::X/<0-128>")], "Add an IPv6 address"),
    spec!(
        Ipv6AddressEui,
        Modes::IF,
        Modes::ROUTER,
        [K("ipv6"), K("address"), Arg("X:X::/64"), K("eui-64")],
        "Add an address built from the MAC address"
    ),
    spec!(Ipv6AddressLinkLocal, Modes::IF, Modes::ROUTER, [K("ipv6"), K("address"), Arg("FE80::X"), K("link-local")], "Set the link-local address"),
    spec!(NoIpv6Address, Modes::IF, Modes::ROUTER, [K("no"), K("ipv6"), K("address")], "Remove every IPv6 address"),
    spec!(NoIpv6Address, Modes::IF, Modes::ROUTER, [K("no"), K("ipv6"), K("address"), Rest("<address>")], "Remove one IPv6 address"),
    spec!(Ipv6Enable, Modes::IF, Modes::ROUTER, [K("ipv6"), K("enable")], "IPv6 with a link-local address only"),
    spec!(NoIpv6Enable, Modes::IF, Modes::ROUTER, [K("no"), K("ipv6"), K("enable")], "Turn link-local-only IPv6 off"),
    spec!(ShowIpv6IntBrief, EXECS, Modes::ROUTER, [K("show"), K("ipv6"), K("interface"), K("brief")], "IPv6 addresses and status"),
    spec!(ShowIpv6Route, EXECS, Modes::ROUTER, [K("show"), K("ipv6"), K("route")], "IPv6 routing table"),
    spec!(ShowIpv6Neighbors, EXECS, Modes::ROUTER, [K("show"), K("ipv6"), K("neighbors")], "IPv6 neighbour cache"),
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
    spec!(BannerMotd, Modes::CONFIG, Modes::BOTH, [K("banner"), K("motd"), Rest("#message#")], "Message shown before login"),
    spec!(NoBannerMotd, Modes::CONFIG, Modes::BOTH, [K("no"), K("banner"), K("motd")], "Remove the message"),
    spec!(ServicePasswordEncryption, Modes::CONFIG, Modes::BOTH, [K("service"), K("password-encryption")], "Write passwords obscured"),
    spec!(NoServicePasswordEncryption, Modes::CONFIG, Modes::BOTH, [K("no"), K("service"), K("password-encryption")], "Write passwords in clear"),
    spec!(DomainLookup, Modes::CONFIG, Modes::BOTH, [K("ip"), K("domain-lookup")], "Resolve mistyped commands as host names"),
    spec!(NoDomainLookup, Modes::CONFIG, Modes::BOTH, [K("no"), K("ip"), K("domain-lookup")], "Don't resolve mistyped commands"),
    spec!(DhcpPool, Modes::CONFIG, Modes::ROUTER, [K("ip"), K("dhcp"), K("pool"), Arg("WORD")], "Create or edit a DHCP pool"),
    spec!(NoDhcpPool, Modes::CONFIG, Modes::ROUTER, [K("no"), K("ip"), K("dhcp"), K("pool"), Arg("WORD")], "Delete a DHCP pool"),
    spec!(
        DhcpExcluded,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("ip"), K("dhcp"), K("excluded-address"), Rest("A.B.C.D [A.B.C.D]")],
        "Addresses DHCP never hands out"
    ),
    spec!(
        NoDhcpExcluded,
        Modes::CONFIG,
        Modes::ROUTER,
        [K("no"), K("ip"), K("dhcp"), K("excluded-address"), Rest("A.B.C.D [A.B.C.D]")],
        "Hand those addresses out again"
    ),
    spec!(DhcpNetwork, Modes::DHCP, Modes::ROUTER, [K("network"), Arg("A.B.C.D"), Arg("MASK")], "The pool's network"),
    spec!(DhcpDefaultRouter, Modes::DHCP, Modes::ROUTER, [K("default-router"), Arg("A.B.C.D")], "Gateway given to clients"),
    spec!(DhcpDns, Modes::DHCP, Modes::ROUTER, [K("dns-server"), Arg("A.B.C.D")], "DNS server given to clients"),
    spec!(ShowDhcpBinding, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("dhcp"), K("binding")], "Addresses DHCP handed out"),
    spec!(ShowDhcpPool, EXECS, Modes::ROUTER, [K("show"), K("ip"), K("dhcp"), K("pool")], "DHCP pools"),
    spec!(ClearDhcpBinding, Modes::EXEC, Modes::ROUTER, [K("clear"), K("ip"), K("dhcp"), K("binding"), Rest("*")], "Forget DHCP leases"),
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
        Accepted,
        Modes::CONFIG,
        Modes::SWITCH,
        [K("spanning-tree"), K("mode"), Arg("{pvst|rapid-pvst}")],
        "Spanning-tree flavour (accepted; the tree is computed the same way)"
    ),
    spec!(Portfast, Modes::IF, Modes::SWITCH, [K("spanning-tree"), K("portfast")], "An edge port: forward at once"),
    spec!(NoPortfast, Modes::IF, Modes::SWITCH, [K("no"), K("spanning-tree"), K("portfast")], "Not an edge port"),
    spec!(ShowStp, EXECS, Modes::SWITCH, [K("show"), K("spanning-tree")], "Spanning trees of every VLAN"),
    spec!(ShowStpVlan, EXECS, Modes::SWITCH, [K("show"), K("spanning-tree"), K("vlan"), Arg("<1-4094>")], "One VLAN's spanning tree"),
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
        assert!(resolve(&["router", "ospf", "1"], Modes::CONFIG, S).is_err());
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
            ]
            .into_iter()
            .find(|m| s.modes.has(*m))
            .unwrap();
            let dev = if s.devices.has(R) { R } else { S };
            let got = resolve(&words, mode, dev).unwrap_or_else(|e| panic!("{:?}: {e:?}", s.path));
            assert_eq!(got.0.id, s.id, "{:?}", s.path);
        }
    }
}
