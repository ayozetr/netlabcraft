//! Services a Server device runs: HTTP (a small web site), DNS (A records) and DHCP pools.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// Most files, records and pools per service, and the largest file.
pub const MAX_ITEMS: usize = 256;
pub const MAX_FILE: usize = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Services {
    #[serde(default)]
    pub http: Http,
    #[serde(default)]
    pub dns: Dns,
    #[serde(default)]
    pub dhcp: ServerDhcp,
}

impl Services {
    pub fn is_default(&self) -> bool {
        *self == Services::default()
    }

    /// What a new server starts with: the web service on with a sample site, DNS and DHCP off.
    pub fn server_defaults() -> Services {
        Services {
            http: Http {
                on: true,
                files: vec![
                    WebFile {
                        name: "index.html".into(),
                        content: "<html>\n<body>\n<h1>PacketCraft web server</h1>\n<p>This page comes from a simulated server, over simulated TCP.</p>\n<p><a href=\"about.html\">About this server</a></p>\n</body>\n</html>\n".into(),
                    },
                    WebFile {
                        name: "about.html".into(),
                        content: "<html>\n<body>\n<h2>About</h2>\n<p>Edit these pages in the server's Services tab, under HTTP.</p>\n<p><a href=\"index.html\">Back to the start page</a></p>\n</body>\n</html>\n".into(),
                    },
                ],
            },
            dns: Dns::default(),
            dhcp: ServerDhcp { on: false, pools: vec![ServerPool { name: "defaultPool".into(), gateway: Ipv4Addr::UNSPECIFIED, dns: Ipv4Addr::UNSPECIFIED, start: Ipv4Addr::new(192, 168, 0, 10), mask: Ipv4Addr::new(255, 255, 255, 0), max_users: 50 }] },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Http {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub files: Vec<WebFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Dns {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub records: Vec<DnsRecord>,
}

impl Dns {
    pub fn lookup(&self, name: &str) -> Option<Ipv4Addr> {
        let n = name.trim_end_matches('.').to_ascii_lowercase();
        self.records.iter().find(|r| r.name.trim_end_matches('.').eq_ignore_ascii_case(&n)).map(|r| r.address)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub name: String,
    pub address: Ipv4Addr,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ServerDhcp {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub pools: Vec<ServerPool>,
}

/// A server DHCP pool: `max_users` addresses from `start`, within `start`/`mask`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerPool {
    pub name: String,
    pub gateway: Ipv4Addr,
    pub dns: Ipv4Addr,
    pub start: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub max_users: u32,
}

impl ServerPool {
    /// The addresses this pool hands out, in order.
    pub fn addresses(&self) -> impl Iterator<Item = Ipv4Addr> + '_ {
        let m = u32::from(self.mask);
        let net = u32::from(self.start) & m;
        let bcast = net | !m;
        let first = u32::from(self.start);
        (0..self.max_users.min(65_536)).filter_map(move |i| first.checked_add(i)).filter(move |a| *a > net && *a < bcast).map(Ipv4Addr::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_and_lookups() {
        let p = ServerPool {
            name: "p".into(),
            gateway: Ipv4Addr::UNSPECIFIED,
            dns: Ipv4Addr::UNSPECIFIED,
            start: Ipv4Addr::new(10, 0, 0, 250),
            mask: Ipv4Addr::new(255, 255, 255, 0),
            max_users: 10,
        };
        let all: Vec<Ipv4Addr> = p.addresses().collect();
        assert_eq!(all.len(), 5);
        assert_eq!(all.last(), Some(&Ipv4Addr::new(10, 0, 0, 254)));
        let d = Dns { on: true, records: vec![DnsRecord { name: "www.lab.".into(), address: Ipv4Addr::new(1, 2, 3, 4) }] };
        assert_eq!(d.lookup("WWW.lab"), Some(Ipv4Addr::new(1, 2, 3, 4)));
        assert_eq!(d.lookup("ftp.lab"), None);
        assert!(Services::server_defaults().http.on);
    }
}
