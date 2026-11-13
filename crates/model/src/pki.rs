//! A device's public-key infrastructure state: its RSA key pairs (`crypto key generate rsa`),
//! its trustpoints (`crypto pki trustpoint`) with the certificates they hold, and a small
//! certificate authority (`crypto pki server`).
//!
//! The model only keeps text and checks its shape: keys are the two primes in hex, certificates
//! are PEM. Whether a prime is prime or a certificate parses is decided where they are used
//! (`netlabcraft_sim::pki`), so a hostile or damaged file costs nothing here and a bad entry is
//! simply unusable there. Keys belong to the lab file the way a router's NVRAM keeps its
//! keys, not to the configuration text.

use serde::{Deserialize, Serialize};

/// Most key pairs, trustpoints, and characters in names and subject names.
pub const MAX_KEYS: usize = 8;
pub const MAX_TRUSTPOINTS: usize = 8;
pub const MAX_NAME: usize = 64;
/// Longest prime in hex (1024 bits is 256 digits; some slack).
pub const MAX_HEX: usize = 300;
/// Longest certificate in PEM text.
pub const MAX_PEM: usize = 16 * 1024;
/// Certificate lifetime limits of the CA, in days.
pub const MAX_DAYS: u32 = 36_500;
pub const DEFAULT_DAYS: u32 = 365;

/// An RSA key pair: its label and its two primes (big-endian hex).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyPair {
    pub label: String,
    pub p: String,
    pub q: String,
}

/// How a trustpoint obtains its certificate.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Enrollment {
    #[default]
    None,
    /// `enrollment selfsigned`.
    SelfSigned,
    /// `enrollment url http://ADDRESS`: the address of a CA, as written.
    Url(String),
}

/// `crypto pki trustpoint NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Trustpoint {
    pub name: String,
    pub enrollment: Enrollment,
    /// `subject-name CN=…,O=…`, as typed.
    pub subject_name: String,
    /// `rsakeypair LABEL`.
    pub rsakeypair: String,
    /// `revocation-check none` (revocation is never checked; the line records the choice).
    pub revocation_none: bool,
    /// The device's own certificate (PEM), once enrolled.
    pub certificate: String,
    /// The CA's certificate (PEM), once authenticated, or the certificate itself when
    /// self-signed.
    pub ca_certificate: String,
}

/// `crypto pki server NAME`: a certificate authority that signs requests it receives over HTTP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaServer {
    pub name: String,
    /// `issuer-name CN=…`; the router's name when empty.
    pub issuer_name: String,
    /// `lifetime certificate DAYS`.
    pub lifetime_days: u32,
    /// `grant auto`: requests are signed as they arrive (the only mode there is).
    pub grant_auto: bool,
    /// Not `shutdown`: the CA exists (it has a key and a certificate) and answers.
    pub enabled: bool,
    /// The CA's own certificate (PEM), made when it was first enabled.
    pub certificate: String,
}

impl Default for CaServer {
    fn default() -> CaServer {
        CaServer {
            name: String::new(),
            issuer_name: String::new(),
            lifetime_days: DEFAULT_DAYS,
            grant_auto: false,
            enabled: false,
            certificate: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pki {
    pub keys: Vec<KeyPair>,
    pub trustpoints: Vec<Trustpoint>,
    pub server: Option<CaServer>,
    /// Keys generated so far: part of the seed, so generating again gives a new key.
    pub generated: u32,
}

impl Pki {
    pub fn is_default(&self) -> bool {
        *self == Pki::default()
    }

    pub fn key(&self, label: &str) -> Option<&KeyPair> {
        self.keys.iter().find(|k| k.label == label)
    }

    pub fn trustpoint(&self, name: &str) -> Option<&Trustpoint> {
        self.trustpoints.iter().find(|t| t.name == name)
    }

    /// Keeps whatever a lab file says within bounds: printable short names, hex primes, PEM
    /// blocks of the right kind, no duplicates, no more than the caps.
    pub fn sanitize(&mut self) {
        fn name_ok(s: &str) -> bool {
            !s.is_empty() && s.chars().count() <= MAX_NAME && s.chars().all(|c| c.is_ascii_graphic())
        }
        fn text(s: &mut String) {
            *s = s.chars().filter(|c| !c.is_control()).take(MAX_NAME * 4).collect();
        }
        fn hex_ok(s: &str) -> bool {
            !s.is_empty() && s.len() <= MAX_HEX && s.bytes().all(|b| b.is_ascii_hexdigit())
        }
        fn pem(s: &mut String) {
            if s.len() > MAX_PEM || (!s.is_empty() && !s.starts_with("-----BEGIN CERTIFICATE-----")) {
                s.clear();
            }
        }
        let mut seen = Vec::new();
        self.keys.retain(|k| {
            let keep = name_ok(&k.label) && hex_ok(&k.p) && hex_ok(&k.q) && !seen.contains(&k.label);
            if keep {
                seen.push(k.label.clone());
            }
            keep
        });
        self.keys.truncate(MAX_KEYS);
        for k in &mut self.keys {
            k.p.make_ascii_lowercase();
            k.q.make_ascii_lowercase();
        }
        let mut seen = Vec::new();
        self.trustpoints.retain(|t| {
            let keep = name_ok(&t.name) && !seen.contains(&t.name);
            if keep {
                seen.push(t.name.clone());
            }
            keep
        });
        self.trustpoints.truncate(MAX_TRUSTPOINTS);
        for t in &mut self.trustpoints {
            text(&mut t.subject_name);
            if !t.rsakeypair.is_empty() && !name_ok(&t.rsakeypair) {
                t.rsakeypair.clear();
            }
            if let Enrollment::Url(u) = &mut t.enrollment {
                text(u);
                if u.is_empty() {
                    t.enrollment = Enrollment::None;
                }
            }
            pem(&mut t.certificate);
            pem(&mut t.ca_certificate);
        }
        if let Some(s) = &mut self.server {
            if !name_ok(&s.name) {
                self.server = None;
            } else {
                text(&mut s.issuer_name);
                s.lifetime_days = s.lifetime_days.clamp(1, MAX_DAYS);
                pem(&mut s.certificate);
                if s.certificate.is_empty() {
                    s.enabled = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_good_entries_and_drops_bad_ones() {
        let key = |l: &str, p: &str| KeyPair { label: l.into(), p: p.into(), q: "ABCDEF01".into() };
        let mut p = Pki {
            keys: vec![
                key("R1.lab", "ff01"),
                key("R1.lab", "ff02"),
                key("bad label", "ff"),
                key("x", "zz"),
                key("", "ff"),
                key("long", &"f".repeat(500)),
            ],
            trustpoints: vec![
                Trustpoint {
                    name: "T".into(),
                    certificate: "garbage".into(),
                    ca_certificate: "-----BEGIN CERTIFICATE-----\nAA\n".into(),
                    ..Trustpoint::default()
                },
                Trustpoint { name: "T".into(), ..Trustpoint::default() },
                Trustpoint { name: "has space".into(), ..Trustpoint::default() },
                Trustpoint { name: "U".into(), enrollment: Enrollment::Url(String::new()), subject_name: "CN=a\nb".into(), ..Trustpoint::default() },
            ],
            server: Some(CaServer { name: "CA".into(), lifetime_days: 0, enabled: true, ..CaServer::default() }),
            generated: 3,
        };
        p.sanitize();
        assert_eq!(p.keys.len(), 1);
        assert_eq!(p.keys[0].label, "R1.lab");
        assert_eq!(p.keys[0].q, "abcdef01");
        assert_eq!(p.trustpoints.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["T", "U"]);
        assert!(p.trustpoints[0].certificate.is_empty() && !p.trustpoints[0].ca_certificate.is_empty());
        assert_eq!((p.trustpoints[1].enrollment.clone(), p.trustpoints[1].subject_name.as_str()), (Enrollment::None, "CN=ab"));
        // A CA with no certificate is not enabled; its lifetime is brought into range.
        let s = p.server.as_ref().unwrap();
        assert!(!s.enabled && s.lifetime_days == 1);
        let mut many = Pki { keys: (0..20).map(|i| key(&format!("k{i}"), "ff")).collect(), ..Pki::default() };
        many.trustpoints = (0..20).map(|i| Trustpoint { name: format!("t{i}"), ..Trustpoint::default() }).collect();
        many.sanitize();
        assert_eq!((many.keys.len(), many.trustpoints.len()), (MAX_KEYS, MAX_TRUSTPOINTS));
    }

    #[test]
    fn default_is_not_written_and_round_trips() {
        assert!(Pki::default().is_default());
        let p = Pki {
            keys: vec![KeyPair { label: "a".into(), p: "ff".into(), q: "ee".into() }],
            trustpoints: vec![Trustpoint { name: "T".into(), enrollment: Enrollment::Url("http://10.0.0.1".into()), ..Trustpoint::default() }],
            ..Pki::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Pki>(&json).unwrap(), p);
        assert_eq!(serde_json::from_str::<Pki>("{}").unwrap(), Pki::default());
    }
}
