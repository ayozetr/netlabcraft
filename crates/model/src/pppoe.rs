//! PPPoE (RFC 2516) as the PC's PPPoE Dialer and a server's PPPoE service model it.
//!
//! A server with the service on is an access concentrator: it checks a user name and password
//! with PAP or CHAP (RFC 1334, RFC 1994) and hands the caller an address from its pool, with the
//! server's own address on that cable as the default gateway. A PC, laptop or server keeps the
//! account the dialer uses and, while connected, the session. The exchange is carried out by the
//! simulator as real PPPoE discovery and PPP session frames (see `docs/gaps.md` for what is left).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net, MacAddr};
use serde::{Deserialize, Serialize};

use crate::PppAuth;

/// Most accounts a server keeps.
pub const MAX_USERS: usize = 32;
/// Most addresses in a server's pool.
pub const MAX_POOL: u32 = 250;
/// Longest user name or password.
pub const MAX_CREDENTIAL: usize = 64;

/// An account a server accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PppoeUser {
    pub name: String,
    pub password: String,
}

/// A server's PPPoE service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PppoeServer {
    #[serde(default)]
    pub on: bool,
    /// What the server demands from a caller.
    #[serde(default = "default_auth")]
    pub auth: PppAuth,
    /// The first address of the pool (it must be on the subnet of the cable the caller uses).
    #[serde(default = "unspecified")]
    pub pool_start: Ipv4Addr,
    /// How many addresses the pool holds, from `pool_start` up.
    #[serde(default)]
    pub pool_size: u32,
    #[serde(default)]
    pub users: Vec<PppoeUser>,
}

fn unspecified() -> Ipv4Addr {
    Ipv4Addr::UNSPECIFIED
}

fn default_auth() -> PppAuth {
    PppAuth::Chap
}

impl Default for PppoeServer {
    fn default() -> Self {
        PppoeServer { on: false, auth: default_auth(), pool_start: Ipv4Addr::UNSPECIFIED, pool_size: 0, users: Vec::new() }
    }
}

impl PppoeServer {
    pub fn is_default(&self) -> bool {
        *self == PppoeServer::default()
    }

    /// Keeps a hostile file's service within bounds: a bounded pool, accounts with a name,
    /// unique names, short credentials.
    pub fn sanitize(&mut self) {
        self.pool_size = self.pool_size.min(MAX_POOL);
        let mut seen = std::collections::BTreeSet::new();
        self.users.retain(|u| !u.name.trim().is_empty() && seen.insert(u.name.clone()));
        self.users.truncate(MAX_USERS);
        for u in &mut self.users {
            u.name = u.name.chars().take(MAX_CREDENTIAL).collect();
            u.password = u.password.chars().take(MAX_CREDENTIAL).collect();
        }
    }

    /// The addresses of the pool, in order.
    pub fn pool(&self) -> impl Iterator<Item = Ipv4Addr> + '_ {
        let first = u32::from(self.pool_start);
        (0..self.pool_size.min(MAX_POOL)).filter_map(move |i| first.checked_add(i)).map(Ipv4Addr::from)
    }

    /// The account called `name`.
    pub fn user(&self, name: &str) -> Option<&PppoeUser> {
        self.users.iter().find(|u| u.name == name)
    }
}

/// A connected dialer's session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PppoeSession {
    /// The server that answered.
    pub server: DeviceId,
    /// The address it gave.
    pub address: Ipv4Addr,
    /// Its own address on the cable: the default gateway.
    pub gateway: Ipv4Addr,
    /// How the caller proved who it is.
    pub auth: PppAuth,
    /// The PPPoE session id the concentrator granted. 0 is a session saved before sessions
    /// were carried on frames: it has nothing on the wire and shows as lost.
    #[serde(default)]
    pub id: u16,
    /// The MAC address of the concentrator's port, where every session frame goes.
    #[serde(default)]
    pub server_mac: MacAddr,
}

/// An end device's PPPoE Dialer: the account it uses and, while connected, the session.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PppoeClient {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub session: Option<PppoeSession>,
    /// The Dial-up app's telephone number (digits): the same account and session are used, over
    /// a phone line instead of an Ethernet cable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub number: String,
}

impl PppoeClient {
    pub fn is_default(&self) -> bool {
        *self == PppoeClient::default()
    }

    pub fn sanitize(&mut self) {
        self.username = self.username.chars().take(MAX_CREDENTIAL).collect();
        self.password = self.password.chars().take(MAX_CREDENTIAL).collect();
        if crate::cloud::normalize_number(&self.number).as_deref() != Some(self.number.as_str()) {
            self.number.clear();
        }
    }
}

// ---- routers: the access concentrator and the client ----

/// Most `bba-group pppoe` groups, virtual templates and dialer interfaces on one router.
pub const MAX_GROUPS: usize = 8;
/// Most sessions a router concentrator holds at once.
pub const MAX_SESSIONS: usize = 64;
/// Highest virtual-template or dialer number.
pub const MAX_NUMBER: u32 = 255;

/// `bba-group pppoe NAME`: the settings a port that enables PPPoE with this group uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BbaGroup {
    pub name: String,
    /// `virtual-template N`: the template every session of the group is built from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<u32>,
}

/// `interface Virtual-Template N`: what a session's virtual-access interface is set up with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualTemplate {
    pub number: u32,
    /// `ip address A.B.C.D MASK`: the concentrator's own address on every session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Ipv4Net>,
    /// `ip unnumbered INTERFACE`: the address of that interface is used instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unnumbered: Option<String>,
    /// `peer default ip address pool NAME`: the `ip local pool` the callers' addresses come from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    /// `ppp authentication pap|chap`: what a caller must prove (none: nothing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<PppAuth>,
    /// `mtu N`: the largest packet of a session (at most 1,492).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl VirtualTemplate {
    pub fn new(number: u32) -> VirtualTemplate {
        VirtualTemplate { number, ip: None, unnumbered: None, pool: None, auth: None, mtu: None, description: String::new() }
    }

    pub fn name(&self) -> String {
        format!("Virtual-Template{}", self.number)
    }
}

/// A dialer's session, with the Ethernet port it runs over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialerSession {
    /// The full name of the Ethernet port (`GigabitEthernet0/1`).
    pub port: String,
    pub session: PppoeSession,
}

/// `interface Dialer N`: the PPPoE client's interface. It takes its address from the
/// concentrator (`ip address negotiated`) and is the exit of the default route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dialer {
    pub number: u32,
    /// `encapsulation ppp`.
    #[serde(default)]
    pub ppp: bool,
    /// `ip address negotiated`.
    #[serde(default)]
    pub negotiated: bool,
    /// `ppp chap hostname NAME` (the router's own name when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chap_hostname: Option<String>,
    /// `ppp chap password SECRET`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chap_password: Option<String>,
    /// `ppp pap sent-username NAME password SECRET`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pap: Option<PppoeUser>,
    /// `dialer pool N`: the pool of ports (`pppoe-client dial-pool-number N`) this interface dials from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<u32>,
    /// `mtu N` (at most 1,492).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u16>,
    #[serde(default)]
    pub shutdown: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The session while connected (kept in the lab, like a PC's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<DialerSession>,
}

impl Dialer {
    pub fn new(number: u32) -> Dialer {
        Dialer {
            number,
            ppp: false,
            negotiated: false,
            chap_hostname: None,
            chap_password: None,
            pap: None,
            pool: None,
            mtu: None,
            shutdown: false,
            description: String::new(),
            session: None,
        }
    }

    pub fn name(&self) -> String {
        format!("Dialer{}", self.number)
    }
}

/// A router's PPPoE settings: as an access concentrator (groups, templates, the ports that
/// answer) and as a client (dialers and the ports that dial).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RouterPppoe {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<BbaGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<VirtualTemplate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dialers: Vec<Dialer>,
    /// `pppoe enable [group NAME]`: port name to group name (`global` when no group was named).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub enabled: BTreeMap<String, String>,
    /// `pppoe-client dial-pool-number N`: port name to pool number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dial_ports: BTreeMap<String, u32>,
}

/// The group a bare `pppoe enable` uses.
pub const GLOBAL_GROUP: &str = "global";

impl RouterPppoe {
    pub fn is_default(&self) -> bool {
        *self == RouterPppoe::default()
    }

    pub fn group(&self, name: &str) -> Option<&BbaGroup> {
        self.groups.iter().find(|g| g.name == name)
    }

    pub fn template(&self, number: u32) -> Option<&VirtualTemplate> {
        self.templates.iter().find(|t| t.number == number)
    }

    /// The template a port answers with: the one of the group it enabled PPPoE with.
    pub fn template_of_port(&self, port: &str) -> Option<&VirtualTemplate> {
        let group = self.group(self.enabled.get(port)?)?;
        self.template(group.template?)
    }

    /// The template, created (empty) if missing. `None` past [`MAX_GROUPS`].
    pub fn template_mut(&mut self, number: u32) -> Option<&mut VirtualTemplate> {
        if !self.templates.iter().any(|t| t.number == number) {
            if self.templates.len() >= MAX_GROUPS {
                return None;
            }
            self.templates.push(VirtualTemplate::new(number));
            self.templates.sort_by_key(|t| t.number);
        }
        self.templates.iter_mut().find(|t| t.number == number)
    }

    /// The dialer interface, created (empty) if missing. `None` past [`MAX_GROUPS`].
    pub fn dialer_mut(&mut self, number: u32) -> Option<&mut Dialer> {
        if !self.dialers.iter().any(|d| d.number == number) {
            if self.dialers.len() >= MAX_GROUPS {
                return None;
            }
            self.dialers.push(Dialer::new(number));
            self.dialers.sort_by_key(|d| d.number);
        }
        self.dialers.iter_mut().find(|d| d.number == number)
    }

    /// The dialer a port dials for: the one whose `dialer pool` is the port's pool number.
    pub fn dialer_of_port(&self, port: &str) -> Option<&Dialer> {
        let pool = *self.dial_ports.get(port)?;
        self.dialers.iter().find(|d| d.pool == Some(pool))
    }

    /// Whether there is anything configured (a router with none does not speak PPPoE at all).
    pub fn active(&self) -> bool {
        !self.enabled.is_empty() || !self.dial_ports.is_empty() || self.dialers.iter().any(|d| d.session.is_some())
    }

    /// Keeps a hostile file's settings within bounds.
    pub fn sanitize(&mut self) {
        self.groups.truncate(MAX_GROUPS);
        let mut seen = std::collections::BTreeSet::new();
        self.groups.retain(|g| seen.insert(g.name.clone()));
        for g in &mut self.groups {
            g.name = g.name.chars().take(MAX_CREDENTIAL).collect();
        }
        self.templates.truncate(MAX_GROUPS);
        self.templates.retain(|t| t.number <= MAX_NUMBER);
        self.templates.sort_by_key(|t| t.number);
        self.templates.dedup_by_key(|t| t.number);
        for t in &mut self.templates {
            t.mtu = t.mtu.map(|m| m.clamp(MIN_MTU, MAX_MTU));
            t.description = t.description.chars().take(crate::MAX_DESCRIPTION).collect();
        }
        self.dialers.truncate(MAX_GROUPS);
        self.dialers.retain(|d| d.number <= MAX_NUMBER);
        self.dialers.sort_by_key(|d| d.number);
        self.dialers.dedup_by_key(|d| d.number);
        for d in &mut self.dialers {
            d.mtu = d.mtu.map(|m| m.clamp(MIN_MTU, MAX_MTU));
            d.description = d.description.chars().take(crate::MAX_DESCRIPTION).collect();
            d.chap_hostname = d.chap_hostname.take().map(|s| s.chars().take(MAX_CREDENTIAL).collect());
            d.chap_password = d.chap_password.take().map(|s| s.chars().take(MAX_CREDENTIAL).collect());
            if let Some(p) = d.pap.as_mut() {
                p.name = p.name.chars().take(MAX_CREDENTIAL).collect();
                p.password = p.password.chars().take(MAX_CREDENTIAL).collect();
            }
        }
        while self.enabled.len() > crate::MAX_SUBIFS {
            self.enabled.pop_last();
        }
        while self.dial_ports.len() > crate::MAX_SUBIFS {
            self.dial_ports.pop_last();
        }
    }
}

/// Smallest and largest `mtu` of a template or dialer.
pub const MIN_MTU: u16 = 576;
pub const MAX_MTU: u16 = 1_492;

/// The number in a `Dialer` interface name (`Dialer1`, `dialer 1`, `di1`), if it names one.
pub fn dialer_number(name: &str) -> Option<u32> {
    number_after(name, "dialer")
}

/// The number in a `Virtual-Template` interface name (`Virtual-Template1`, `virtual-template 1`,
/// `vi1`), if it names one.
pub fn template_number(name: &str) -> Option<u32> {
    number_after(name, "virtual-template")
}

fn number_after(name: &str, full: &str) -> Option<u32> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let split = compact.find(|c: char| c.is_ascii_digit())?;
    let (word, num) = compact.split_at(split);
    let word = word.to_ascii_lowercase();
    if word.len() < 2 || !full.starts_with(&word) {
        return None;
    }
    num.parse::<u32>().ok().filter(|n| *n <= MAX_NUMBER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_is_bounded_and_in_order() {
        let mut s = PppoeServer { on: true, pool_start: Ipv4Addr::new(10, 0, 0, 250), pool_size: 1_000_000, ..PppoeServer::default() };
        s.sanitize();
        assert_eq!(s.pool_size, MAX_POOL);
        let all: Vec<Ipv4Addr> = s.pool().collect();
        assert_eq!(all.len(), MAX_POOL as usize);
        assert_eq!(all.first(), Some(&Ipv4Addr::new(10, 0, 0, 250)));
        assert_eq!(all.get(6), Some(&Ipv4Addr::new(10, 0, 1, 0)));
        // A pool at the very end of the address space stops instead of wrapping.
        let end = PppoeServer { pool_start: Ipv4Addr::new(255, 255, 255, 254), pool_size: 10, ..PppoeServer::default() };
        assert_eq!(end.pool().count(), 2);
    }

    #[test]
    fn sanitize_drops_nameless_and_duplicate_accounts() {
        let user = |n: &str, p: &str| PppoeUser { name: n.into(), password: p.into() };
        let mut s = PppoeServer {
            users: vec![user("", "x"), user("ann", "one"), user("ann", "two"), user("bob", &"p".repeat(500))],
            ..PppoeServer::default()
        };
        s.sanitize();
        assert_eq!(s.users.len(), 2);
        assert_eq!(s.user("ann").map(|u| u.password.as_str()), Some("one"));
        assert_eq!(s.user("bob").map(|u| u.password.len()), Some(MAX_CREDENTIAL));
        assert!(s.user("nobody").is_none());
    }

    #[test]
    fn a_default_service_and_account_are_left_out_of_a_file() {
        assert!(PppoeServer::default().is_default());
        assert!(PppoeClient::default().is_default());
        assert!(!PppoeClient { username: "ann".into(), ..PppoeClient::default() }.is_default());
    }
}
