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
use netlabcraft_wire::x509::{self, Certificate, Crl, Request};

use super::{Ctx, Mode, Outcome, Paste, PasteKind, R};
use crate::grammar::Id;
use netlabcraft_sim::Sim;
use netlabcraft_sim::pki::ReqState;

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
            Enrollment::Terminal => {
                let _ = writeln!(s, " enrollment terminal");
            }
        }
        if !t.subject_name.is_empty() {
            let _ = writeln!(s, " subject-name {}", t.subject_name);
        }
        if !t.rsakeypair.is_empty() {
            let _ = writeln!(s, " rsakeypair {}", t.rsakeypair);
        }
        if !t.fqdn.is_empty() {
            let _ = writeln!(s, " fqdn {}", t.fqdn);
        }
        if !t.ip_address.is_empty() {
            let _ = writeln!(s, " ip-address {}", t.ip_address);
        }
        match (t.revocation_crl, t.revocation_none) {
            (true, true) => {
                let _ = writeln!(s, " revocation-check crl none");
            }
            (true, false) => {
                let _ = writeln!(s, " revocation-check crl");
            }
            (false, true) => {
                let _ = writeln!(s, " revocation-check none");
            }
            (false, false) => {}
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
        if c.crl_hours != netlabcraft_model::pki::DEFAULT_CRL_HOURS {
            let _ = writeln!(s, " lifetime crl {}", c.crl_hours);
        }
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
    let (ip, _) = parse_url(url)?;
    let (status, reason, text) = ask_raw(c, url, method, path, body)?;
    if status != 200 {
        return Err(ca_error(ip, status, &reason, &text));
    }
    Ok(text)
}

fn ca_error(ip: Ipv4Addr, status: u16, reason: &str, text: &str) -> String {
    format!("% The CA at {ip} answered {status} {reason}: {}", text.chars().take(120).collect::<String>())
}

/// One request to a CA: the status, its reason phrase and the body of the answer.
fn ask_raw(c: &mut Ctx<'_>, url: &str, method: &str, path: &str, body: &str) -> Result<(u16, String, String), String> {
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
    Ok((resp.status, resp.reason.clone(), String::from_utf8_lossy(&resp.body).trim().to_string()))
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
    if !cert.alt.is_empty() {
        let mut names: Vec<String> = cert.alt.dns.clone();
        names.extend(cert.alt.ips.iter().map(|a| Ipv4Addr::from(*a).to_string()));
        let _ = writeln!(s, "  Also for      : {}", names.join(", "));
    }
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
                [s] if s.eq_ignore_ascii_case("terminal") => Enrollment::Terminal,
                [u, url] if u.eq_ignore_ascii_case("url") => {
                    parse_url(url)?;
                    Enrollment::Url(url.trim_end_matches('/').chars().take(MAX_NAME * 2).collect())
                }
                _ => return Err("% Expected `selfsigned`, `terminal` or `url http://A.B.C.D`".into()),
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
            let a = arg(0).to_ascii_lowercase();
            let (crl, none) = match a.split_whitespace().collect::<Vec<_>>().as_slice() {
                ["none"] => (false, true),
                ["crl"] => (true, false),
                ["crl", "none"] => (true, true),
                _ => return Err("% Expected `none`, `crl` or `crl none`".into()),
            };
            let t = trustpoint(c)?;
            (t.revocation_crl, t.revocation_none) = (crl, none);
            Ok(String::new())
        }
        Id::PkiFqdn => {
            let n = arg(0);
            if n.is_empty() || n.len() > MAX_NAME || !n.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '*')) {
                return Err(format!("% A DNS name of at most {MAX_NAME} letters, digits, `-`, `.` or `*`"));
            }
            trustpoint(c)?.fqdn = n;
            Ok(String::new())
        }
        Id::PkiIpAddress => {
            let a: Ipv4Addr = arg(0).parse().map_err(|_| "% An IPv4 address, such as 10.0.0.1".to_string())?;
            trustpoint(c)?.ip_address = a.to_string();
            Ok(String::new())
        }
        Id::PkiServerNoGrant => {
            server(c)?.grant_auto = false;
            Ok(String::new())
        }
        Id::PkiServerLifetimeCrl => {
            let n: u32 = arg(0)
                .parse()
                .ok()
                .filter(|n| (1..=netlabcraft_model::pki::MAX_CRL_HOURS).contains(n))
                .ok_or_else(|| format!("% 1-{} hours", netlabcraft_model::pki::MAX_CRL_HOURS))?;
            server(c)?.crl_hours = n;
            Ok(String::new())
        }
        Id::CryptoPkiImportCertificate | Id::CryptoPkiImportCrl => {
            let name = arg(0);
            if c.d()?.pki.trustpoint(&name).is_none() {
                return Err(format!("% No trustpoint named {name}"));
            }
            let (kind, what) = if id == Id::CryptoPkiImportCertificate {
                (PasteKind::Certificate, "the certificate")
            } else {
                (PasteKind::Crl, "the revocation list")
            };
            c.cli.paste = Some(Paste { kind, name, text: String::new() });
            Ok(format!("Paste {what} in PEM form, then `quit` on a line by itself."))
        }
        Id::CryptoPkiServerRequest => {
            let name = arg(0);
            running_server(c, &name)?;
            c.cli.paste = Some(Paste { kind: PasteKind::Request, name, text: String::new() });
            Ok("Paste the certificate request in PEM form, then `quit` on a line by itself.".into())
        }
        Id::CryptoPkiCrlRequest => crl_request(c, &arg(0), now),
        Id::CryptoPkiServerGrantRequest | Id::CryptoPkiServerReject => {
            running_server(c, &arg(0))?;
            let which = if arg(1).eq_ignore_ascii_case("all") {
                None
            } else {
                Some(arg(1).parse::<u32>().map_err(|_| "% A request number, or `all`".to_string())?)
            };
            if let Some(n) = which
                && !c.sim.pki_requests(c.dev).iter().any(|r| r.id == n)
            {
                return Err(format!("% The certificate authority holds no request number {n} (`show crypto pki server`)"));
            }
            let grant = id == Id::CryptoPkiServerGrantRequest;
            let changed = c.sim.pki_decide(c.dev, which, if grant { ReqState::Granted } else { ReqState::Rejected });
            if changed.is_empty() {
                return Ok("No request changed: none is waiting.".into());
            }
            let mut out = String::new();
            for n in changed {
                let Some(r) = c.sim.pki_requests(c.dev).into_iter().find(|r| r.id == n) else { continue };
                if grant {
                    let cert = simpki::sign_request(c.d()?, now, &r.der, n)?;
                    let _ =
                        writeln!(out, "Request {n} for {} granted; its certificate (serial {}):\n{}", r.subject, hex(&cert.serial), cert.to_pem());
                } else {
                    let _ = writeln!(out, "Request {n} for {} rejected.", r.subject);
                }
            }
            Ok(out.trim_end().to_string())
        }
        Id::CryptoPkiServerRevoke => {
            running_server(c, &arg(0))?;
            let serial = simpki::parse_serial(&arg(1)).ok_or("% The serial number is hexadecimal, as `show crypto pki certificates` prints it")?;
            let hexed = simpki::serial_hex(&serial);
            // The requests whose certificate this was are forgotten: a new request for the same key is a new one.
            let d = c.d()?;
            let gone: Vec<u32> = c
                .sim
                .pki_requests(c.dev)
                .iter()
                .filter(|r| r.state == ReqState::Granted && simpki::request_serial(d, &r.der, r.id).is_some_and(|s| s == serial))
                .map(|r| r.id)
                .collect();
            let sv = c.dm()?.pki.server.as_mut().ok_or("% The certificate authority was deleted")?;
            if sv.revoked.iter().any(|r| *r == hexed) {
                return Err(format!("% Serial {hexed} is already revoked"));
            }
            if sv.revoked.len() >= netlabcraft_model::pki::MAX_REVOKED {
                return Err(format!("% At most {} certificates can be revoked", netlabcraft_model::pki::MAX_REVOKED));
            }
            sv.revoked.push(hexed.clone());
            sv.crl_number = sv.crl_number.saturating_add(1);
            let number = sv.crl_number;
            c.sim.pki_forget(c.dev, &gone);
            Ok(format!(
                "Certificate {hexed} revoked; revocation list number {number} is published at {} (`crypto pki crl request`).",
                simpki::CRL_PATH
            ))
        }
        Id::ShowCryptoPkiCrls => show_crls(c, now),
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
                    Enrollment::Terminal => "terminal (copied by hand)".to_string(),
                };
                let _ = writeln!(s, "  Enrollment : {how}");
                let _ = writeln!(s, "  Subject    : {}", if t.subject_name.is_empty() { d.name.as_str() } else { t.subject_name.as_str() });
                let _ = writeln!(s, "  Key pair   : {}", if t.rsakeypair.is_empty() { "not set" } else { t.rsakeypair.as_str() });
                let rev = match (t.revocation_crl, t.revocation_none) {
                    (true, true) => "by CRL; goes on when none is held",
                    (true, false) => "by CRL, required",
                    (false, true) => "not checked",
                    (false, false) => "not set (never checked)",
                };
                let _ = writeln!(s, "  Revocation : {rev}");
                if !t.fqdn.is_empty() || !t.ip_address.is_empty() {
                    let _ = writeln!(s, "  Also for   : {} {}", t.fqdn, t.ip_address);
                }
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
                if sv.grant_auto { "requests are signed as they arrive" } else { "requests wait for `crypto pki server NAME grant`" }
            );
            let _ = writeln!(
                s,
                "  Lifetime    : certificates last {} day(s); the revocation list is good for {} hour(s)",
                sv.lifetime_days, sv.crl_hours
            );
            let _ = writeln!(
                s,
                "  Answers on  : HTTP port 80 ({}), paths {}, {} and {}",
                if d.automation.http { "`ip http server` is on" } else { "`ip http server` is OFF" },
                simpki::CA_PATH,
                simpki::ENROLL_PATH,
                simpki::CRL_PATH
            );
            let requests = c.sim.pki_requests(c.dev);
            if requests.is_empty() {
                let _ = writeln!(s, "  Requests    : none held");
            } else {
                let _ = writeln!(s, "  Requests    :");
                for r in &requests {
                    let state = match r.state {
                        ReqState::Pending => "waiting",
                        ReqState::Granted => "granted",
                        ReqState::Rejected => "rejected",
                    };
                    let _ = writeln!(s, "    {}: {} ({state})", r.id, r.subject);
                }
            }
            let _ = writeln!(s, "  Revoked     : {} certificate(s); revocation list number {}", sv.revoked.len(), sv.crl_number);
            for r in &sv.revoked {
                let _ = writeln!(s, "    serial {r}");
            }
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
        note.push_str("Requests wait until an administrator grants them (`crypto pki server NAME grant`); `grant auto` signs them as they arrive.\n");
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
        Enrollment::None => return Err("% Set `enrollment selfsigned`, `terminal` or `url` in the trustpoint first".into()),
        Enrollment::Terminal => {
            let req = Request::create_alt(&subject, &key, SigHash::Sha256, &simpki::alt_of(&tp)).map_err(|e| format!("% {e}"))?;
            let _ = writeln!(
                note,
                "Copy this request to the certificate authority; the certificate it signs goes in with `crypto pki import {name} certificate`."
            );
            note.push_str(&x509::pem_encode("CERTIFICATE REQUEST", req.der()));
            return Ok(note.trim_end().to_string());
        }
        Enrollment::SelfSigned => {
            let mut serial = sha256(&[subject.text().as_bytes(), &key.public().modulus()].concat()).get(..8).unwrap_or_default().to_vec();
            if let Some(f) = serial.first_mut() {
                *f = (*f & 0x3f) | 0x40;
            }
            simpki::issue_alt(&subject, &key, &subject, &key, &serial, now, SELF_DAYS, false, &simpki::alt_of(&tp))?
        }
        Enrollment::Url(url) => {
            let ca = Certificate::from_pem(&tp.ca_certificate)
                .map_err(|_| format!("% Authenticate the CA first: `crypto pki authenticate {}`", tp.name))?;
            let req = Request::create_alt(&subject, &key, SigHash::Sha256, &simpki::alt_of(&tp)).map_err(|e| format!("% {e}"))?;
            let body = x509::pem_encode("CERTIFICATE REQUEST", req.der());
            let (status, reason, answer) = ask_raw(c, url, "POST", simpki::ENROLL_PATH, &body)?;
            if status == 202 {
                let _ = writeln!(note, "{answer}");
                note.push_str("Run `crypto pki enroll` again once the administrator has granted the request.");
                return Ok(note);
            }
            if status != 200 {
                return Err(ca_error(parse_url(url)?.0, status, &reason, &answer));
            }
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
        Enrollment::Terminal => {
            c.cli.paste = Some(Paste { kind: PasteKind::CaCertificate, name: tp.name.clone(), text: String::new() });
            Ok("Paste the CA's certificate in PEM form, then `quit` on a line by itself.".into())
        }
        Enrollment::SelfSigned => Ok("A self-signed trustpoint trusts its own certificate: `crypto pki enroll` it.".into()),
        Enrollment::None => Err("% Set `enrollment url http://A.B.C.D` or `enrollment terminal` in the trustpoint first".into()),
    }
}

/// The certificate authority named `name`, which has to be running.
fn running_server(c: &Ctx<'_>, name: &str) -> Result<(), String> {
    match &c.d()?.pki.server {
        Some(sv) if sv.name == name && sv.enabled => Ok(()),
        Some(sv) if sv.name == name => Err(format!("% The certificate authority {name} is stopped: `no shutdown` in its mode starts it")),
        _ => Err(format!("% No certificate authority named {name} runs here")),
    }
}

/// `crypto pki crl request NAME`: the revocation list of a trustpoint's CA, fetched over HTTP,
/// checked against the CA certificate the trustpoint holds.
fn crl_request(c: &mut Ctx<'_>, name: &str, now: i64) -> R {
    let tp = c.d()?.pki.trustpoint(name).cloned().ok_or_else(|| format!("% No trustpoint named {name}"))?;
    let Enrollment::Url(url) = &tp.enrollment else {
        return Err("% Only a trustpoint with `enrollment url` can fetch a list: for the others `crypto pki import NAME crl`".into());
    };
    let ca = Certificate::from_pem(&tp.ca_certificate).map_err(|_| format!("% Authenticate the CA first: `crypto pki authenticate {name}`"))?;
    let answer = ask(c, url, "GET", simpki::CRL_PATH, "")?;
    let crl = Crl::from_pem(&answer).map_err(|e| format!("% The CA's answer is not a revocation list: {e}"))?;
    store_crl(c.t, c.dev, name, &ca, &crl, now)
}

/// Keeps a CRL in a trustpoint when its CA issued it.
fn store_crl(t: &mut Topology, dev: DeviceId, name: &str, ca: &Certificate, crl: &Crl, now: i64) -> R {
    if !crl.issued_by(ca) {
        return Err(format!("% The list was not issued by the CA of trustpoint {name} ({})", ca.subject.text()));
    }
    let tp = t.device_mut(dev).and_then(|d| d.pki.trustpoints.iter_mut().find(|x| x.name == name)).ok_or("% The trustpoint was deleted")?;
    tp.crl = crl.to_pem();
    let current = if crl.current_at(u64::try_from(now).unwrap_or(0)) { "current" } else { "not current at this router's clock" };
    Ok(format!(
        "Revocation list number {} of {} with {} revoked certificate(s), valid {} to {} ({current}).",
        crl.number,
        crl.issuer.text(),
        crl.revoked().len(),
        when(crl.this_update),
        when(crl.next_update)
    ))
}

/// `show crypto pki crls`.
fn show_crls(c: &mut Ctx<'_>, now: i64) -> R {
    let d = c.d()?;
    let mut s = String::new();
    let at = u64::try_from(now).unwrap_or(0);
    let mut block = |what: &str, crl: &Crl, signed_by: Option<&Certificate>| {
        let _ = writeln!(s, "{what}");
        let _ = writeln!(s, "  Issuer        : {}", crl.issuer.text());
        let _ = writeln!(s, "  Number        : {}", crl.number);
        let fresh = if crl.current_at(at) { "current" } else { "not current" };
        let _ = writeln!(s, "  Valid         : {} to {} ({fresh})", when(crl.this_update), when(crl.next_update));
        if let Some(ca) = signed_by {
            let _ = writeln!(s, "  Signature     : {}", if crl.issued_by(ca) { "made by the CA's key" } else { "NOT made by the CA's key" });
        }
        let _ = writeln!(s, "  Revoked       : {}", crl.revoked().len());
        for r in crl.revoked() {
            let _ = writeln!(s, "    serial {}", simpki::serial_hex(r));
        }
    };
    for t in &d.pki.trustpoints {
        if let Ok(crl) = Crl::from_pem(&t.crl) {
            let ca = Certificate::from_pem(&t.ca_certificate).ok();
            block(&format!("Revocation list held for trustpoint {}", t.name), &crl, ca.as_ref());
        }
    }
    if let Some(crl) = simpki::ca_crl(d, now) {
        block("Revocation list this router's own CA publishes", &crl, None);
    }
    Ok(if s.is_empty() { "No revocation lists.".into() } else { s.trim_end().to_string() })
}

/// The end of a pasted block (`quit`): what the text was for is done with it.
pub(crate) fn paste_done(t: &mut Topology, sim: &mut Sim, dev: DeviceId, p: Paste) -> Outcome {
    let now = sim.clock(dev).0;
    match paste_apply(t, sim, dev, &p, now) {
        Ok(output) => {
            sim.topology_changed(t);
            Outcome { output, changed: true, clear: false }
        }
        Err(output) => Outcome { output, ..Outcome::default() },
    }
}

fn paste_apply(t: &mut Topology, sim: &mut Sim, dev: DeviceId, p: &Paste, now: i64) -> R {
    let at = u64::try_from(now).unwrap_or(0);
    let d = t.device(dev).ok_or("% That device no longer exists")?;
    match p.kind {
        PasteKind::CaCertificate => {
            let cert = Certificate::from_pem(&p.text).map_err(|e| format!("% The pasted text is not a certificate: {e}"))?;
            if !cert.ca {
                return Err("% The pasted certificate is not a certificate authority's: refused".into());
            }
            let tp = t.device_mut(dev).and_then(|d| d.pki.trustpoints.iter_mut().find(|x| x.name == p.name)).ok_or("% The trustpoint was deleted")?;
            tp.ca_certificate = cert.to_pem();
            Ok(format!("Trusting the CA certificate of {} (fingerprint {}).", cert.subject.text(), simpki::fingerprint(&cert)))
        }
        PasteKind::Certificate => {
            let cert = Certificate::from_pem(&p.text).map_err(|e| format!("% The pasted text is not a certificate: {e}"))?;
            let tp = d.pki.trustpoint(&p.name).cloned().ok_or("% The trustpoint was deleted")?;
            let label = if tp.rsakeypair.is_empty() { tp.name.clone() } else { tp.rsakeypair.clone() };
            let key = key_of(d, &label)?;
            if &cert.key != key.public() {
                return Err(format!("% The pasted certificate is for another key than the pair {label}: refused"));
            }
            let ca = Certificate::from_pem(&tp.ca_certificate)
                .map_err(|_| format!("% Authenticate the CA first: `crypto pki authenticate {}`", tp.name))?;
            x509::verify_chain(std::slice::from_ref(&cert), std::slice::from_ref(&ca), at)
                .map_err(|e| format!("% The pasted certificate does not check out against the trusted CA: {e}"))?;
            let tpm =
                t.device_mut(dev).and_then(|d| d.pki.trustpoints.iter_mut().find(|x| x.name == p.name)).ok_or("% The trustpoint was deleted")?;
            tpm.certificate = cert.to_pem();
            if tpm.rsakeypair.is_empty() {
                tpm.rsakeypair = label;
            }
            Ok(format!(
                "Certificate {} for {} imported; valid {} to {}.",
                hex(&cert.serial),
                cert.subject.text(),
                when(cert.not_before),
                when(cert.not_after)
            ))
        }
        PasteKind::Crl => {
            let crl = Crl::from_pem(&p.text).map_err(|e| format!("% The pasted text is not a revocation list: {e}"))?;
            let tp = d.pki.trustpoint(&p.name).ok_or("% The trustpoint was deleted")?;
            let ca = Certificate::from_pem(&tp.ca_certificate)
                .map_err(|_| format!("% Authenticate the CA first: `crypto pki authenticate {}`", tp.name))?;
            store_crl(t, dev, &p.name, &ca, &crl, now)
        }
        PasteKind::Request => {
            let sv = d.pki.server.as_ref().filter(|s| s.name == p.name && s.enabled).ok_or("% The certificate authority is not running")?;
            let der = x509::pem_decode("CERTIFICATE REQUEST", &p.text).map_err(|e| format!("% The pasted text is not a certificate request: {e}"))?;
            let request = Request::parse(&der).map_err(|e| format!("% The pasted request is not valid: {e}"))?;
            let auto = sv.grant_auto;
            let (id, state) =
                sim.pki_submit(dev, &request, auto).ok_or("% The certificate authority holds too many requests: grant, reject or revoke some")?;
            let d = t.device(dev).ok_or("% That device no longer exists")?;
            match state {
                ReqState::Granted => {
                    let cert = simpki::sign_request(d, now, &der, id)?;
                    Ok(format!(
                        "Request {id} for {} granted; its certificate (serial {}):\n{}",
                        request.subject.text(),
                        hex(&cert.serial),
                        cert.to_pem().trim_end()
                    ))
                }
                ReqState::Pending => {
                    Ok(format!("Request {id} for {} is waiting: `crypto pki server {} grant {id}` signs it.", request.subject.text(), p.name))
                }
                ReqState::Rejected => Err(format!("% Request {id} was rejected earlier")),
            }
        }
    }
}
