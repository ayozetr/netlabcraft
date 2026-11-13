//! Certificates on a device: loading its keys, issuing its certificates, and the certificate
//! authority a router can run (`crypto pki server`), which signs requests it receives over HTTP
//! on port 80 (`GET /pki/ca` returns the CA certificate, `POST /pki/enroll` takes a PEM
//! PKCS #10 request and returns the signed certificate). Every value that varies comes from the
//! device's configuration and its clock, so a lab replays the same bytes: key generation is
//! seeded by the device, the label and a counter, and serial numbers are a hash of the request.

use netlabcraft_model::Device;
use netlabcraft_model::pki::{CaServer, KeyPair};
use netlabcraft_wire::rsa::{RsaKey, SigHash};
use netlabcraft_wire::sha256::sha256;
use netlabcraft_wire::tls::{Drbg, Identity};
use netlabcraft_wire::x509::{self, Certificate, Issue, MAX_TIME, Name, Request};
use netlabcraft_wire::{http, rsa};

/// Path of the CA's certificate, and of the enrollment endpoint.
pub const CA_PATH: &str = "/pki/ca";
pub const ENROLL_PATH: &str = "/pki/enroll";
/// Validity of a CA's own certificate, in days.
pub const CA_DAYS: u32 = 1095;

/// The key pair stored under a label, when its text is a real RSA key.
pub fn load_key(d: &Device, label: &str) -> Option<RsaKey> {
    let kp = d.pki.key(label)?;
    let unhex = |h: &str| -> Option<Vec<u8>> {
        let h = if h.len() % 2 == 1 { format!("0{h}") } else { h.to_string() };
        h.as_bytes().chunks(2).map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect()
    };
    RsaKey::from_primes(&unhex(&kp.p)?, &unhex(&kp.q)?).ok()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A new key pair for this device, from a seed made of its identity, the label and how many
/// keys it generated before.
pub fn generate_key(d: &Device, label: &str, bits: usize) -> Result<KeyPair, String> {
    if !rsa::bits_ok(bits) {
        return Err(format!("the modulus is a multiple of 64 between {} and {} bits", rsa::MIN_BITS, rsa::MAX_BITS));
    }
    let seed = format!("netlabcraft rsa {} {} {} {} {}", d.id.0, d.name, label, bits, d.pki.generated);
    let key = RsaKey::generate(bits, &mut Drbg::new(seed.as_bytes())).map_err(|e| e.to_string())?;
    let (p, q) = key.primes();
    Ok(KeyPair { label: label.to_string(), p: hex(&p), q: hex(&q) })
}

/// `CN=name, O=org, OU=unit, C=XX` (any order, commas) as a name; `None` text means the fallback
/// common name.
pub fn parse_name(text: &str, fallback_cn: &str) -> Result<Name, String> {
    let mut n = Name::default();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once('=').ok_or_else(|| format!("`{part}` is not ATTRIBUTE=value"))?;
        if v.trim().is_empty() {
            return Err(format!("`{part}` has no value"));
        }
        let slot = match k.trim().to_ascii_uppercase().as_str() {
            "CN" => &mut n.common_name,
            "O" => &mut n.organization,
            "OU" => &mut n.unit,
            "C" => &mut n.country,
            other => return Err(format!("`{other}` is not CN, O, OU or C")),
        };
        *slot = v.trim().to_string();
    }
    if n.is_empty() {
        n.common_name = fallback_cn.to_string();
    }
    n.encode().map_err(|_| "the name has an attribute that is empty, too long (64), or a country that is not two letters".to_string())?;
    Ok(n)
}

/// The subject a trustpoint's certificate gets: its `subject-name`, or the device's name.
pub fn subject_of(d: &Device, subject_name: &str) -> Result<Name, String> {
    parse_name(subject_name, &d.name)
}

/// A certificate for `subject_key`, signed by `issuer_key`.
#[allow(clippy::too_many_arguments)]
pub fn issue(
    subject: &Name,
    subject_key: &RsaKey,
    issuer: &Name,
    issuer_key: &RsaKey,
    serial: &[u8],
    now: i64,
    days: u32,
    ca: bool,
) -> Result<Certificate, String> {
    let not_before = u64::try_from(now).unwrap_or(0);
    let not_after = not_before.saturating_add(u64::from(days) * 86_400).min(MAX_TIME);
    Certificate::issue(&Issue {
        subject,
        subject_key: subject_key.public(),
        issuer,
        issuer_key,
        serial,
        not_before,
        not_after,
        ca,
        path_len: None,
        hash: SigHash::Sha256,
    })
    .map_err(|e| e.to_string())
}

/// The CA's name: its `issuer-name`, or the router's.
pub fn ca_name(d: &Device, s: &CaServer) -> Result<Name, String> {
    parse_name(&s.issuer_name, &d.name)
}

/// The CA's certificate, parsed, when the server is enabled.
pub fn ca_certificate(d: &Device) -> Option<Certificate> {
    let s = d.pki.server.as_ref().filter(|s| s.enabled)?;
    Certificate::from_pem(&s.certificate).ok()
}

/// A serial number for a request: the first bytes of a hash of the CA's name and the request.
fn serial_for(ca: &Certificate, req: &Request) -> Vec<u8> {
    let h = sha256(&[ca.der(), req.der()].concat());
    let mut s = h.get(..8).unwrap_or_default().to_vec();
    if let Some(first) = s.first_mut() {
        *first = (*first & 0x3f) | 0x40;
    }
    s
}

fn text(status: u16, body: &str) -> http::Response {
    let mut r = http::Response::new(status);
    r.headers.push(("Server".into(), "NetLabCraft".into()));
    r.headers.push(("Content-Type".into(), "text/plain".into()));
    r.headers.push(("Connection".into(), "close".into()));
    r.body = body.as_bytes().to_vec();
    r
}

/// Whether a request is for the certificate authority.
pub fn is_pki_request(req: &http::Request) -> bool {
    req.path() == CA_PATH || req.path() == ENROLL_PATH
}

/// The CA's answer to a request at its two paths.
pub fn serve(d: &Device, now: i64, req: &http::Request) -> http::Response {
    let Some(server) = d.pki.server.as_ref().filter(|s| s.enabled) else {
        return text(503, "This router is not running a certificate authority.\n");
    };
    let Some(ca) = Certificate::from_pem(&server.certificate).ok() else {
        return text(503, "The certificate authority has no certificate.\n");
    };
    match (req.method.as_str(), req.path()) {
        ("GET", CA_PATH) => text(200, &ca.to_pem()),
        ("POST", ENROLL_PATH) => {
            if !server.grant_auto {
                return text(403, "Requests are not granted automatically here, and manual approval is not supported.\n");
            }
            let body = String::from_utf8_lossy(&req.body);
            let parsed = x509::pem_decode("CERTIFICATE REQUEST", &body).and_then(|der| Request::parse(&der));
            let Ok(request) = parsed else { return text(400, "The body is not a valid signed certificate request.\n") };
            let Some(ca_key) = load_key(d, &server.name) else { return text(503, "The certificate authority's key is missing.\n") };
            let Ok(issuer) = ca_name(d, server) else { return text(500, "The issuer name is not usable.\n") };
            let not_before = u64::try_from(now).unwrap_or(0).max(ca.not_before);
            let not_after = not_before.saturating_add(u64::from(server.lifetime_days) * 86_400).min(ca.not_after);
            let cert = Certificate::issue(&Issue {
                subject: &request.subject,
                subject_key: &request.key,
                issuer: &issuer,
                issuer_key: &ca_key,
                serial: &serial_for(&ca, &request),
                not_before,
                not_after,
                ca: false,
                path_len: None,
                hash: SigHash::Sha256,
            });
            match cert {
                Ok(c) => text(200, &c.to_pem()),
                Err(e) => text(500, &format!("The certificate could not be made: {e}\n")),
            }
        }
        _ => text(404, "No such path.\n"),
    }
}

/// A fingerprint to show for a certificate: SHA-256 of its DER, as hex pairs.
pub fn fingerprint(c: &Certificate) -> String {
    sha256(c.der()).chunks(2).map(|p| p.iter().map(|b| format!("{b:02X}")).collect::<String>()).collect::<Vec<_>>().join(" ")
}

/// What a router's HTTPS server shows: the certificate of the trustpoint `ip http
/// secure-trustpoint` names (and the CA's, when different), with the key that goes with it.
/// `None` when nothing is configured or the pieces are missing, damaged or do not match: the
/// server then falls back to the anonymous suite, as before.
pub fn tls_identity(d: &Device) -> Option<Identity> {
    let tp = d.pki.trustpoint(d.automation.https_trustpoint.as_deref()?)?;
    let leaf = Certificate::from_pem(&tp.certificate).ok()?;
    let key = load_key(d, &tp.rsakeypair)?;
    if &leaf.key != key.public() {
        return None;
    }
    let mut chain = vec![leaf.der().to_vec()];
    if let Ok(ca) = Certificate::from_pem(&tp.ca_certificate)
        && ca.der() != leaf.der()
    {
        chain.push(ca.der().to_vec());
    }
    Some(Identity { chain, key })
}

/// The CA certificates a device holds in its trustpoints, and its own CA's certificate: what its
/// clients trust.
pub fn trust_anchors(d: &Device) -> Vec<Certificate> {
    let mut out: Vec<Certificate> = d.pki.trustpoints.iter().filter_map(|t| Certificate::from_pem(&t.ca_certificate).ok()).collect();
    if let Some(c) = d.pki.server.as_ref().and_then(|s| Certificate::from_pem(&s.certificate).ok()) {
        out.push(c);
    }
    out
}

/// The key an SSH server signs with: the key pair named `NAME.DOMAIN` (what `crypto key
/// generate rsa` makes by default), when the device has one. Without it the server keeps the
/// stand-in host key.
pub fn ssh_host_key(d: &Device) -> Option<RsaKey> {
    d.access.rsa_bits?;
    load_key(d, &format!("{}.{}", d.name, d.access.domain_name.as_ref()?))
}
