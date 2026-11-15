//! Services a Server device runs: HTTP (a small web site), DNS (A records) and DHCP pools.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// Most files, records and pools per service, and the largest file.
pub const MAX_ITEMS: usize = 256;
pub const MAX_FILE: usize = 60_000;
/// Most files a host keeps (the Text Editor's and `ftp`'s), and the most bytes one holds.
pub const MAX_HOST_FILES: usize = 32;
pub const MAX_HOST_FILE_BYTES: usize = 16 * 1024;
/// Longest file name on a host.
pub const MAX_HOST_FILE_NAME: usize = 64;

/// Whether a host accepts `name` for a file: 1 to 64 characters, no spaces, no slashes, no control
/// characters.
pub fn valid_host_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_HOST_FILE_NAME
        && name.chars().all(|c| !c.is_whitespace() && !c.is_control() && c != '/' && c != '\\')
}

/// Keeps a hostile file's host files within bounds: valid unique names, at most
/// [`MAX_HOST_FILES`] of them, each at most [`MAX_HOST_FILE_BYTES`] long (cut on a character).
pub fn sanitize_host_files(files: &mut Vec<WebFile>) {
    let mut seen = std::collections::BTreeSet::new();
    files.retain(|f| valid_host_file_name(&f.name) && seen.insert(f.name.clone()));
    files.truncate(MAX_HOST_FILES);
    for f in files.iter_mut() {
        if f.content.len() > MAX_HOST_FILE_BYTES {
            let mut end = MAX_HOST_FILE_BYTES;
            while end > 0 && !f.content.is_char_boundary(end) {
                end -= 1;
            }
            f.content.truncate(end);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Services {
    #[serde(default)]
    pub http: Http,
    #[serde(default)]
    pub dns: Dns,
    #[serde(default)]
    pub dhcp: ServerDhcp,
    /// NTP: the server gives its clock to whoever asks.
    #[serde(default)]
    pub ntp: Switch,
    /// Syslog: the server keeps messages devices send it.
    #[serde(default)]
    pub syslog: Switch,
    /// TFTP: files routers and switches copy configurations to and from.
    #[serde(default)]
    pub tftp: Tftp,
    /// AAA: a RADIUS server that checks the logins of its client routers and switches.
    #[serde(default)]
    pub aaa: Aaa,
    /// FTP: files that users with an account send and fetch.
    #[serde(default)]
    pub ftp: Ftp,
    /// Email: SMTP takes messages for the domain's users, POP3 hands them out.
    #[serde(default)]
    pub email: Email,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Email {
    #[serde(default)]
    pub on: bool,
    /// The domain whose mail this server keeps (`lab.local` in `ana@lab.local`).
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub users: Vec<AaaUser>,
    /// Messages waiting to be fetched, with the user they are for.
    #[serde(default)]
    pub mailboxes: Vec<(String, Mail)>,
}

/// An email message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mail {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
}

/// Most messages a server keeps, and the longest message.
pub const MAX_MAILS: usize = 500;
pub const MAX_MAIL: usize = 20_000;

/// A host's email account (the Email app's settings).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MailAccount {
    pub name: String,
    /// The address (`ana@lab.local`).
    pub address: String,
    /// Incoming (POP3) and outgoing (SMTP) servers: addresses or names.
    pub incoming: String,
    pub outgoing: String,
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ftp {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub users: Vec<FtpUser>,
    #[serde(default)]
    pub files: Vec<WebFile>,
}

/// An FTP account and what it may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FtpUser {
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub write: bool,
    #[serde(default)]
    pub delete: bool,
    #[serde(default)]
    pub list: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Aaa {
    #[serde(default)]
    pub on: bool,
    /// Devices allowed to ask, with the secret each shares with the server.
    #[serde(default)]
    pub clients: Vec<AaaClient>,
    #[serde(default)]
    pub users: Vec<AaaUser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AaaClient {
    pub name: String,
    pub address: Ipv4Addr,
    pub secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AaaUser {
    pub name: String,
    pub password: String,
}

/// A service that is just on or off.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Switch {
    #[serde(default)]
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Tftp {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub files: Vec<WebFile>,
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
                        content: "<html>\n<body>\n<h1>NetLabCraft web server</h1>\n<p>This page comes from a simulated server, over simulated TCP.</p>\n<p><a href=\"about.html\">About this server</a></p>\n</body>\n</html>\n".into(),
                    },
                    WebFile {
                        name: "about.html".into(),
                        content: "<html>\n<body>\n<h2>About</h2>\n<p>Edit these pages in the server's Services tab, under HTTP.</p>\n<p><a href=\"index.html\">Back to the start page</a></p>\n</body>\n</html>\n".into(),
                    },
                ],
            },
            dns: Dns::default(),
                        ntp: Switch { on: true },
            syslog: Switch { on: true },
                        tftp: Tftp { on: true, files: Vec::new() },
                        aaa: Aaa::default(),
            ftp: Ftp { on: true, users: Vec::new(), files: Vec::new() },
            email: Email { on: true, ..Email::default() },
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

    #[test]
    fn host_files_are_kept_within_bounds() {
        assert!(valid_host_file_name("notes.txt"));
        assert!(!valid_host_file_name(""));
        assert!(!valid_host_file_name("my notes"));
        assert!(!valid_host_file_name("a/b"));
        assert!(!valid_host_file_name(&"x".repeat(MAX_HOST_FILE_NAME + 1)));
        let file = |n: &str, c: String| WebFile { name: n.into(), content: c };
        let mut files = vec![file("a.txt", "é".repeat(MAX_HOST_FILE_BYTES)), file("a.txt", "dup".into()), file("bad name", "x".into())];
        files.extend((0..MAX_HOST_FILES + 5).map(|i| file(&format!("f{i}"), String::new())));
        sanitize_host_files(&mut files);
        assert_eq!(files.len(), MAX_HOST_FILES);
        assert_eq!(files[0].name, "a.txt");
        assert!(files[0].content.len() <= MAX_HOST_FILE_BYTES);
        assert!(files[0].content.chars().all(|c| c == 'é'), "cut on a character boundary");
        assert!(files.iter().all(|f| f.name != "bad name"));
    }
}
