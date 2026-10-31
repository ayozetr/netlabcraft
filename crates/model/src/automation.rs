//! Network automation: a router's web server and its RESTCONF (RFC 8040) and NETCONF
//! (RFC 6241) interfaces; on PCs, the requests the Programming app keeps.

use serde::{Deserialize, Serialize};

/// Most saved requests per host.
pub const MAX_SAVED: usize = 32;
/// Most bytes of a saved request's body.
pub const MAX_SAVED_BODY: usize = 16 * 1024;
/// Most routers one controller manages.
pub const MAX_MANAGED: usize = 32;

/// A router's automation settings (part of its running configuration).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Automation {
    /// `ip http server`: HTTP on port 80.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub http: bool,
    /// `ip http secure-server`: HTTP over TLS on port 443.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub https: bool,
    /// `ip http authentication local`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub http_auth_local: bool,
    /// `restconf`: the web server answers under `/restconf`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub restconf: bool,
    /// `netconf-yang`: NETCONF on port 830.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub netconf: bool,
    /// Hosts: the Programming app's saved requests.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub saved: Vec<SavedRequest>,
    /// Hosts acting as a controller: the routers they manage over RESTCONF.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub managed: Vec<Managed>,
}

impl Automation {
    pub fn is_default(&self) -> bool {
        *self == Automation::default()
    }
}

/// A request saved in a PC's Programming app.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedRequest {
    pub name: String,
    pub method: String,
    pub url: String,
    /// `Name: value` lines.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub body: String,
}

/// A router a controller manages: where it answers RESTCONF and the login to use.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Managed {
    pub address: String,
    pub user: String,
    pub password: String,
}
