//! Keys, trustpoints and certificates: `crypto key generate|zeroize rsa`, `crypto pki
//! trustpoint|enroll|authenticate|server`, and the `show` commands for them. The cryptography
//! lives in `netlabcraft_wire::{rsa, x509}`; what a device does with it (seeding, its clock, the
//! CA's HTTP answers) in `netlabcraft_sim::pki`.

use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr};

use netlabcraft_core::DeviceId;
use netlabcraft_model::pki::{CaServer, Enrollment, MAX_DAYS, MAX_KEYS, MAX_NAME, MAX_TRUSTPOINTS, Trustpoint};
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_sim::pki as simpki;
use netlabcraft_wire::http;
use netlabcraft_wire::rsa::{self, RsaKey, SigHash};
use netlabcraft_wire::sha256::sha256;
use netlabcraft_wire::x509::{self, Certificate, Request};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// The key size an unqualified `crypto key generate rsa` and a new CA use.
pub const DEFAULT_BITS: usize = 1024;
/// Days a self-signed certificate lasts.
const SELF_DAYS: u32 = 1095;

/// The configuration lines of this feature, for `show running-config`: trustpoints and the
/// certificate authority. (Key pairs and certificates are kept apart from the configuration,
/// as a router keeps them in its private storage.)
pub fn running_config(s: &mut String, d: &Device) {
    for t in &d.pki.trustpoints {
        let _ = writeln!(s, "crypto pki trustpoint {}", t.name);
        match &t.enrollment {
            Enrollment::None => {}
            Enrollment::SelfSigned => {
                let _ = writeln!(s, " enrollment selfsigned");
            }
            Enrollment::Url(u) => {
                let _ = writeln!(s, " enrollment url {u}");
            }
        }
        if !t.subject_name.is_empty() {
            let _ = writeln!(s, " subject-name {}", t.subject_name);
        }
        if !t.rsakeypair.is_empty() {
            let _ = writeln!(s, " rsakeypair {}", t.rsakeypair);
        }
        if t.revocation_none {
            let _ = writeln!(s, " revocation-check none");
        }
        let _ = writeln!(s, "!");
    }
    if let Some(c) = &d.pki.server {
        let _ = writeln!(s, "crypto pki server {}", c.name);
        if !c.issuer_name.is_empty() {
            let _ = writeln!(s, " issuer-name {}", c.issuer_name);
        }
        if c.grant_auto {
            let _ = writeln!(s, " grant auto");
        }
        let _ = writeln!(s, " lifetime certificate {}", c.lifetime_days);
        let _ = writeln!(s, " {}shutdown", if c.enabled { "no " } else { "" });
        let _ = writeln!(s, "!");
    }
}

fn name_ok(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME || !t.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% Give a name of 1-{MAX_NAME} printable characters without spaces"));
    }
    Ok(t.to_string())
}

fn when(t: u64) -> String {
    let (y, mo, d, h, mi, s) = x509::civil(t);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

/// The label the SSH host key has: `NAME.DOMAIN`.
fn default_label(d: &Device) -> Option<String> {
    d.access.domain_name.as_ref().map(|dom| format!("{}.{dom}", d.name))
}

/// Generates a key pair (replacing one with the same label). `label` is the default
/// `NAME.DOMAIN` when `None`. Returns the message to print.
pub fn generate(t: &mut Topology, dev: DeviceId, label: Option<&str>, bits: usize) -> R {
    let d = t.device(dev).ok_or("% That device no longer exists")?;
    let default = default_label(d);
    let label = match (label, &default) {
        (Some(l), _) => name_ok(l)?,
        (None, Some(l)) => l.clone(),
        (None, None) => return Err("% Set `ip domain-name` first: the keys are named after it".into()),
    };
    if !rsa::bits_ok(bits) {
        return Err(format!(
            "% The key size is a multiple of 64 between {} and {} bits (the simulator generates real keys; no keys generated)",
            rsa::MIN_BITS,
            rsa::MAX_BITS
        ));
    }
    if d.pki.key(&label).is_none() && d.pki.keys.len() >= MAX_KEYS {
        return Err(format!("% At most {MAX_KEYS} key pairs: zeroize one first"));
    }
    let pair = simpki::generate_key(d, &label, bits).map_err(|e| format!("% {e}"))?;
    let is_default = default.as_deref() == Some(label.as_str());
    let dm = t.device_mut(dev).ok_or("% That device no longer exists")?;
    dm.pki.keys.retain(|k| k.label != label);
    dm.pki.keys.push(pair);
    dm.pki.generated = dm.pki.generated.saturating_add(1);
    if is_default {
        dm.access.rsa_bits = Some(bits as u16);
        Ok(format!("RSA keys generated ({bits} bits). SSH is available."))
    } else {
        Ok(format!("RSA key pair {label} generated ({bits} bits)."))
    }
}

fn key_params(args: &str) -> Result<(Option<String>, Option<usize>), String> {
    let w: Vec<&str> = args.split_whitespace().collect();
    let (mut label, mut bits) = (None, None);
    let mut i = 0;
    while let Some(word) = w.get(i) {
        match word.to_ascii_lowercase().as_str() {
            "general-keys" | "exportable" => i += 1,
            "modulus" => {
                bits = Some(w.get(i + 1).and_then(|n| n.parse::<usize>().ok()).ok_or("% `modulus` takes a number of bits")?);
                i += 2;
            }
            "label" => {
                label = Some(w.get(i + 1).ok_or("% `label` takes a name")?.to_string());
                i += 2;
            }
            _ => return Err("% Expected: [general-keys] [modulus BITS] [label NAME]".into()),
        }
    }
    Ok((label, bits))
}

fn key_of(d: &Device, label: &str) -> Result<RsaKey, String> {
    simpki::load_key(d, label)
        .ok_or_else(|| format!("% The key pair {label} is missing or damaged: generate it with `crypto key generate rsa label {label}`"))
}

fn trustpoint<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut Trustpoint, String> {
    let Mode::PkiTrustpoint(n) = c.cli.mode.clone() else { return Err("% Not in a trustpoint".into()) };
    c.dm()?.pki.trustpoints.iter_mut().find(|t| t.name == n).ok_or_else(|| "% The trustpoint was deleted".to_string())
}

fn server<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut CaServer, String> {
    let Mode::PkiServer(n) = c.cli.mode.clone() else { return Err("% Not in the certificate authority".into()) };
    c.dm()?.pki.server.as_mut().filter(|s| s.name == n).ok_or_else(|| "% The certificate authority was deleted".to_string())
}

/// `http://A.B.C.D[:PORT][/…]` as an address and a port.
fn parse_url(url: &str) -> Result<(Ipv4Addr, u16), String> {
    let rest = url.trim().strip_prefix("http://").ok_or("% The URL starts with http:// and names the CA by IPv4 address")?;
    let host = rest.split('/').next().unwrap_or("");
    let (addr, port) = match host.split_once(':') {
        Some((a, p)) => (a, p.parse::<u16>().ok().filter(|p| *p > 0).ok_or("% The port is 1-65535")?),
        None => (host, http::PORT),
    };
    Ok((addr.parse().map_err(|_| "% The URL names the CA by IPv4 address, such as http://10.0.0.1")?, port))
}

/// One request to a CA over the simulated network; the body of a 200 answer.
fn ask(c: &mut Ctx<'_>, url: &str, method: &str, path: &str, body: &str) -> Result<String, String> {
    let (ip, port) = parse_url(url)?;
    let headers = vec![
        ("Host".to_string(), ip.to_string()),
        ("User-Agent".to_string(), "NetLabCraft-PKI".to_string()),
        ("Content-Type".to_string(), "text/plain".to_string()),
        ("Connection".to_string(), "close".to_string()),
    ];
    let req = http::Request { method: method.into(), target: path.into(), headers, body: body.as_bytes().to_vec() };
    let got = c.sim.tcp_session(&*c.t, c.dev, IpAddr::V4(ip), port, &req.encode()).map_err(|e| format!("% The CA at {ip} did not answer: {e}"))?;
    let resp = match http::Response::parse(&got) {
        Ok(Some(r)) => r,
        _ => return Err(format!("% The CA at {ip} sent no usable answer")),
    };
    let text = String::from_utf8_lossy(&resp.body).trim().to_string();
    if resp.status != 200 {
        return Err(format!("% The CA at {ip} answered {} {}: {}", resp.status, resp.reason, text.chars().take(120).collect::<String>()));
    }
    Ok(text)
}

fn status(cert: &Certificate, now: i64) -> &'static str {
    let now = u64::try_from(now).unwrap_or(0);
    if now < cert.not_before {
        "not valid yet"
    } else if now > cert.not_after {
        "expired"
    } else {
        "valid"
    }
}

fn cert_block(s: &mut String, what: &str, cert: &Certificate, now: i64) {
    let _ = writeln!(s, "{what}");
    let _ = writeln!(s, "  Serial number : {}", hex(&cert.serial));
    let _ = writeln!(s, "  Subject       : {}", cert.subject.text());
    let _ = writeln!(s, "  Issuer        : {}", cert.issuer.text());
    let _ = writeln!(s, "  Valid         : {} to {} ({})", when(cert.not_before), when(cert.not_after), status(cert, now));
    let _ = writeln!(s, "  Public key    : RSA {} bits{}", cert.key.bits(), if cert.ca { ", certificate authority" } else { "" });
    let _ = writeln!(s, "  Fingerprint   : {}", simpki::fingerprint(cert));
}

pub(super) fn command(c: &mut Ctx<'_>, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim().to_string();
    let now = c.sim.clock(c.dev).0;
    let is_router = c.d()?.kind == DeviceKind::Router;
    match id {
        Id::CryptoKey => {
            let (label, bits) = key_params(&arg(0))?;
            let Some(bits) = bits.or_else(|| if arg(0).is_empty() { None } else { Some(DEFAULT_BITS) }) else {
                if label.is_none() && default_label(c.d()?).is_none() {
                    return Err("% Set `ip domain-name` first: the keys are named after it".into());
                }
                c.cli.modulus_prompt = true;
                return Ok(format!("The key name will be {}", label.or_else(|| default_label(c.d().ok()?)).unwrap_or_default()));
            };
            generate(c.t, c.dev, label.as_deref(), bits)
        }
        Id::CryptoKeyZeroize => {
            let label = arg(0);
            let default = default_label(c.d()?);
            let dm = c.dm()?;
            if label.is_empty() {
                dm.pki.keys.clear();
                dm.access.rsa_bits = None;
                return Ok("RSA keys deleted; SSH is off.".into());
            }
            let before = dm.pki.keys.len();
            dm.pki.keys.retain(|k| k.label != label);
            if dm.pki.keys.len() == before {
                return Err(format!("% No key pair named {label}"));
            }
            if default.as_deref() == Some(label.as_str()) {
                dm.access.rsa_bits = None;
                return Ok(format!("RSA key pair {label} deleted; SSH is off."));
            }
            Ok(format!("RSA key pair {label} deleted."))
        }
        Id::ShowCryptoKeyMypubkey => {
            let d = c.d()?;
            if d.pki.keys.is_empty() {
                return Ok(match d.access.rsa_bits {
                    Some(b) => {
                        format!("An RSA key of {b} bits was configured before keys were kept as real key pairs: generate it again to see it here.")
                    }
                    None => "No RSA key pairs.".into(),
                });
            }
            let mut s = String::new();
            for k in &d.pki.keys {
                match simpki::load_key(d, &k.label) {
                    Some(key) => {
                        let _ = writeln!(
                            s,
                            "Key pair {}{}",
                            k.label,
                            if default_label(d).as_deref() == Some(k.label.as_str()) { " (SSH host key)" } else { "" }
                        );
                        let _ = writeln!(s, "  Modulus size : {} bits, public exponent {}", key.bits(), key.public().exponent());
                        let _ = writeln!(s, "  Fingerprint  : {}", hex(&sha256(&key.public().modulus())));
                        let _ = writeln!(
                            s,
                            "  SSH          : {}",
                            netlabcraft_wire::ssh::fingerprint(&netlabcraft_wire::ssh::host_key_blob(key.public()))
                        );
                        let m = hex(&key.public().modulus());
                        for line in m.as_bytes().chunks(64) {
                            let _ = writeln!(s, "    {}", String::from_utf8_lossy(line));
                        }
                    }
                    None => {
                        let _ = writeln!(s, "Key pair {}: damaged (its stored values are not a valid RSA key)", k.label);
                    }
                }
            }
            Ok(s.trim_end().to_string())
        }
        _ if !is_router => Err("% Certificates are a router feature".into()),
        Id::CryptoPkiTrustpoint => {
            let name = name_ok(&arg(0))?;
            let p = &mut c.dm()?.pki;
            if p.trustpoint(&name).is_none() {
                if p.trustpoints.len() >= MAX_TRUSTPOINTS {
                    return Err(format!("% At most {MAX_TRUSTPOINTS} trustpoints"));
                }
                p.trustpoints.push(Trustpoint { name: name.clone(), ..Trustpoint::default() });
            }
            c.cli.mode = Mode::PkiTrustpoint(name);
            Ok(String::new())
        }
        Id::NoCryptoPkiTrustpoint => {
            let name = arg(0);
            let p = &mut c.dm()?.pki;
            let before = p.trustpoints.len();
            p.trustpoints.retain(|t| t.name != name);
            if p.trustpoints.len() == before {
                return Err(format!("% No trustpoint named {name}"));
            }
            Ok(String::new())
        }
        Id::PkiEnrollment => {
            let a = arg(0);
            let w: Vec<&str> = a.split_whitespace().collect();
            let e = match w.as_slice() {
                [s] if s.eq_ignore_ascii_case("selfsigned") => Enrollment::SelfSigned,
                [u, url] if u.eq_ignore_ascii_case("url") => {
                    parse_url(url)?;
                    Enrollment::Url(url.trim_end_matches('/').chars().take(MAX_NAME * 2).collect())
                }
                _ => return Err("% Expected `selfsigned` or `url http://A.B.C.D`".into()),
            };
            trustpoint(c)?.enrollment = e;
            Ok(String::new())
        }
        Id::PkiSubjectName => {
            let a = arg(0);
            simpki::parse_name(&a, "x").map_err(|e| format!("% {e}"))?;
            trustpoint(c)?.subject_name = a.chars().take(MAX_NAME * 4).collect();
            Ok(String::new())
        }
        Id::PkiRsakeypair => {
            trustpoint(c)?.rsakeypair = name_ok(&arg(0))?;
            Ok(String::new())
        }
        Id::PkiRevocationCheck => {
            if !arg(0).eq_ignore_ascii_case("none") {
                return Err("% Only `none` is supported: revocation is never checked".into());
            }
            trustpoint(c)?.revocation_none = true;
            Ok(String::new())
        }
        Id::CryptoPkiEnroll => enroll(c, &arg(0), now),
        Id::CryptoPkiAuthenticate => authenticate(c, &arg(0)),
        Id::CryptoPkiServer => {
            let name = name_ok(&arg(0))?;
            let p = &mut c.dm()?.pki;
            match &p.server {
                Some(s) if s.name != name => return Err(format!("% The certificate authority is {}; delete it first", s.name)),
                Some(_) => {}
                None => p.server = Some(CaServer { name: name.clone(), ..CaServer::default() }),
            }
            c.cli.mode = Mode::PkiServer(name);
            Ok(String::new())
        }
        Id::NoCryptoPkiServer => {
            let name = arg(0);
            let p = &mut c.dm()?.pki;
            if p.server.as_ref().is_none_or(|s| s.name != name) {
                return Err(format!("% No certificate authority named {name}"));
            }
            p.server = None;
            Ok(String::new())
        }
        Id::PkiServerIssuer => {
            let a = arg(0);
            simpki::parse_name(&a, "x").map_err(|e| format!("% {e}"))?;
            server(c)?.issuer_name = a.chars().take(MAX_NAME * 4).collect();
            Ok(String::new())
        }
        Id::PkiServerGrant => {
            if !arg(0).eq_ignore_ascii_case("auto") {
                return Err("% Only `grant auto` is supported".into());
            }
            server(c)?.grant_auto = true;
            Ok(String::new())
        }
        Id::PkiServerLifetime => {
            let n: u32 = arg(0).parse().ok().filter(|n| (1..=MAX_DAYS).contains(n)).ok_or_else(|| format!("% 1-{MAX_DAYS} days"))?;
            server(c)?.lifetime_days = n;
            Ok(String::new())
        }
        Id::PkiServerShutdown => {
            server(c)?.enabled = false;
            Ok("The certificate authority stopped answering.".into())
        }
        Id::NoPkiServerShutdown => start_server(c, now),
        Id::ShowCryptoPkiTrustpoints => {
            let d = c.d()?;
            if d.pki.trustpoints.is_empty() {
                return Ok("No trustpoints.".into());
            }
            let mut s = String::new();
            for t in &d.pki.trustpoints {
                let _ = writeln!(s, "Trustpoint {}", t.name);
                let how = match &t.enrollment {
                    Enrollment::None => "not set".to_string(),
                    Enrollment::SelfSigned => "self-signed".to_string(),
                    Enrollment::Url(u) => format!("CA at {u}"),
                };
                let _ = writeln!(s, "  Enrollment : {how}");
                let _ = writeln!(s, "  Subject    : {}", if t.subject_name.is_empty() { d.name.as_str() } else { t.subject_name.as_str() });
                let _ = writeln!(s, "  Key pair   : {}", if t.rsakeypair.is_empty() { "not set" } else { t.rsakeypair.as_str() });
                let _ = writeln!(s, "  Revocation : {}", if t.revocation_none { "not checked" } else { "not set (never checked)" });
                let mut state = |what: &str, pem: &str| match Certificate::from_pem(pem) {
                    Ok(cert) => {
                        let _ = writeln!(s, "  {what:<11}: {} ({})", cert.subject.text(), status(&cert, now));
                    }
                    Err(_) => {
                        let _ = writeln!(s, "  {what:<11}: {}", if pem.is_empty() { "none" } else { "damaged" });
                    }
                };
                state("Certificate", &t.certificate);
                state("CA trusted", &t.ca_certificate);
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowCryptoPkiCertificates => {
            let d = c.d()?;
            let mut s = String::new();
            for t in &d.pki.trustpoints {
                if let Ok(cert) = Certificate::from_pem(&t.certificate) {
                    cert_block(&mut s, &format!("Certificate of trustpoint {}", t.name), &cert, now);
                }
                if t.ca_certificate != t.certificate
                    && let Ok(cert) = Certificate::from_pem(&t.ca_certificate)
                {
                    cert_block(&mut s, &format!("CA certificate of trustpoint {}", t.name), &cert, now);
                }
            }
            if let Some(cert) = d.pki.server.as_ref().and_then(|sv| Certificate::from_pem(&sv.certificate).ok()) {
                cert_block(&mut s, "Certificate of this router's own certificate authority", &cert, now);
            }
            Ok(if s.is_empty() { "No certificates.".into() } else { s.trim_end().to_string() })
        }
        Id::ShowCryptoPkiServer => {
            let d = c.d()?;
            let Some(sv) = &d.pki.server else { return Ok("No certificate authority is configured.".into()) };
            let mut s = String::new();
            let _ = writeln!(s, "Certificate authority {}: {}", sv.name, if sv.enabled { "running" } else { "stopped" });
            let name = simpki::ca_name(d, sv).map(|n| n.text()).unwrap_or_default();
            let _ = writeln!(s, "  Issuer name : {name}");
            let _ = writeln!(
                s,
                "  Grants      : {}",
                if sv.grant_auto { "requests are signed as they arrive" } else { "no requests (set `grant auto`)" }
            );
            let _ = writeln!(s, "  Lifetime    : certificates last {} day(s)", sv.lifetime_days);
            let _ = writeln!(
                s,
                "  Answers on  : HTTP port 80 ({}), paths {} and {}",
                if d.automation.http { "`ip http server` is on" } else { "`ip http server` is OFF" },
                simpki::CA_PATH,
                simpki::ENROLL_PATH
            );
            if let Ok(cert) = Certificate::from_pem(&sv.certificate) {
                cert_block(&mut s, "CA certificate", &cert, now);
            }
            Ok(s.trim_end().to_string())
        }
        _ => Err("% Not a certificate command".into()),
    }
}

/// `no shutdown` in the certificate authority: makes its key and certificate the first time.
fn start_server(c: &mut Ctx<'_>, now: i64) -> R {
    let (name, issuer_text) = {
        let s = server(c)?;
        (s.name.clone(), s.issuer_name.clone())
    };
    let d = c.d()?;
    let issuer = simpki::parse_name(&issuer_text, &d.name).map_err(|e| format!("% {e}"))?;
    let mut note = String::new();
    if d.pki.key(&name).is_none() {
        if d.pki.keys.len() >= MAX_KEYS {
            return Err(format!("% At most {MAX_KEYS} key pairs: zeroize one first"));
        }
        let pair = simpki::generate_key(d, &name, DEFAULT_BITS).map_err(|e| format!("% {e}"))?;
        let dm = c.dm()?;
        dm.pki.keys.push(pair);
        dm.pki.generated = dm.pki.generated.saturating_add(1);
        let _ = writeln!(note, "Generated the CA's {DEFAULT_BITS}-bit key pair {name}.");
    }
    let d = c.d()?;
    let key = key_of(d, &name)?;
    let have = d.pki.server.as_ref().is_some_and(|s| Certificate::from_pem(&s.certificate).is_ok());
    if !have {
        let cert = simpki::issue(&issuer, &key, &issuer, &key, &[1], now, simpki::CA_DAYS, true)?;
        let pem = cert.to_pem();
        server(c)?.certificate = pem;
        let _ = writeln!(note, "Made the CA certificate for {}.", issuer.text());
    }
    server(c)?.enabled = true;
    if !c.d()?.automation.http {
        note.push_str("The CA answers on HTTP port 80: turn on `ip http server`.\n");
    }
    if !server(c)?.grant_auto {
        note.push_str("No requests are signed until `grant auto` is set.\n");
    }
    note.push_str("The certificate authority is running.");
    Ok(note)
}

fn enroll(c: &mut Ctx<'_>, name: &str, now: i64) -> R {
    let d = c.d()?;
    let tp = d.pki.trustpoint(name).cloned().ok_or_else(|| format!("% No trustpoint named {name}"))?;
    let mut note = String::new();
    let label = if tp.rsakeypair.is_empty() { tp.name.clone() } else { tp.rsakeypair.clone() };
    if d.pki.key(&label).is_none() {
        if tp.rsakeypair.is_empty() || d.pki.keys.len() < MAX_KEYS {
            generate(c.t, c.dev, Some(&label), DEFAULT_BITS)?;
            let _ = writeln!(note, "Generated a {DEFAULT_BITS}-bit key pair named {label}.");
        } else {
            return Err(format!("% The key pair {label} does not exist: `crypto key generate rsa label {label}`"));
        }
    }
    let d = c.d()?;
    let key = key_of(d, &label)?;
    let subject = simpki::subject_of(d, &tp.subject_name).map_err(|e| format!("% {e}"))?;
    let cert = match &tp.enrollment {
        Enrollment::None => return Err("% Set `enrollment selfsigned` or `enrollment url` in the trustpoint first".into()),
        Enrollment::SelfSigned => {
            let mut serial = sha256(&[subject.text().as_bytes(), &key.public().modulus()].concat()).get(..8).unwrap_or_default().to_vec();
            if let Some(f) = serial.first_mut() {
                *f = (*f & 0x3f) | 0x40;
            }
            simpki::issue(&subject, &key, &subject, &key, &serial, now, SELF_DAYS, false)?
        }
        Enrollment::Url(url) => {
            let ca = Certificate::from_pem(&tp.ca_certificate)
                .map_err(|_| format!("% Authenticate the CA first: `crypto pki authenticate {}`", tp.name))?;
            let req = Request::create(&subject, &key, SigHash::Sha256).map_err(|e| format!("% {e}"))?;
            let body = x509::pem_encode("CERTIFICATE REQUEST", req.der());
            let answer = ask(c, url, "POST", simpki::ENROLL_PATH, &body)?;
            let cert = Certificate::from_pem(&answer).map_err(|e| format!("% The CA's answer is not a certificate: {e}"))?;
            if &cert.key != key.public() {
                return Err("% The certificate the CA sent is for another key: refused".into());
            }
            x509::verify_chain(std::slice::from_ref(&cert), std::slice::from_ref(&ca), u64::try_from(now).unwrap_or(0))
                .map_err(|e| format!("% The CA's certificate does not check out against the trusted CA: {e}"))?;
            cert
        }
    };
    let pem = cert.to_pem();
    let self_signed = tp.enrollment == Enrollment::SelfSigned;
    let tpm = c.dm()?.pki.trustpoints.iter_mut().find(|t| t.name == tp.name).ok_or("% The trustpoint was deleted")?;
    tpm.certificate = pem.clone();
    if self_signed {
        tpm.ca_certificate = pem;
    }
    if tpm.rsakeypair.is_empty() {
        tpm.rsakeypair = label;
    }
    let _ = writeln!(
        note,
        "Certificate {} for {} received; valid {} to {}.",
        hex(&cert.serial),
        cert.subject.text(),
        when(cert.not_before),
        when(cert.not_after)
    );
    Ok(note.trim_end().to_string())
}

fn authenticate(c: &mut Ctx<'_>, name: &str) -> R {
    let tp = c.d()?.pki.trustpoint(name).cloned().ok_or_else(|| format!("% No trustpoint named {name}"))?;
    match &tp.enrollment {
        Enrollment::Url(url) => {
            let answer = ask(c, url, "GET", simpki::CA_PATH, "")?;
            let ca = Certificate::from_pem(&answer).map_err(|e| format!("% The CA's answer is not a certificate: {e}"))?;
            if !ca.ca {
                return Err("% The certificate the CA sent is not a certificate authority's: refused".into());
            }
            let tpm = c.dm()?.pki.trustpoints.iter_mut().find(|t| t.name == name).ok_or("% The trustpoint was deleted")?;
            tpm.ca_certificate = ca.to_pem();
            Ok(format!("Trusting the CA certificate of {} (fingerprint {}).", ca.subject.text(), simpki::fingerprint(&ca)))
        }
        Enrollment::SelfSigned => Ok("A self-signed trustpoint trusts its own certificate: `crypto pki enroll` it.".into()),
        Enrollment::None => Err("% Set `enrollment url http://A.B.C.D` in the trustpoint first".into()),
    }
}
