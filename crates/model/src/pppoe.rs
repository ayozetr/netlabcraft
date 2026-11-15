//! PPPoE (RFC 2516) as the PC's PPPoE Dialer and a server's PPPoE service model it.
//!
//! A server with the service on is an access concentrator: it checks a user name and password
//! with PAP or CHAP (RFC 1334, RFC 1994) and hands the caller an address from its pool, with the
//! server's own address on that cable as the default gateway. A PC, laptop or server keeps the
//! account the dialer uses and, while connected, the session. The exchange is carried out by the
//! engine as one step over the cable (see `docs/gaps.md` for what is not on the wire).

use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
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
}

impl PppoeClient {
    pub fn is_default(&self) -> bool {
        *self == PppoeClient::default()
    }

    pub fn sanitize(&mut self) {
        self.username = self.username.chars().take(MAX_CREDENTIAL).collect();
        self.password = self.password.chars().take(MAX_CREDENTIAL).collect();
    }
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
