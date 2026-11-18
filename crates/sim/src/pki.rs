//! Certificates on a device: loading its keys, issuing its certificates, and the certificate
//! authority a router can run (`crypto pki server`), which signs requests it receives over HTTP
//! on port 80 (`GET /pki/ca` returns the CA certificate, `POST /pki/enroll` takes a PEM
//! PKCS #10 request and returns the signed certificate). Every value that varies comes from the
//! device's configuration and its clock, so a lab replays the same bytes: key generation is
//! seeded by the device, the label and a counter, and serial numbers are a hash of the request.

use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::Device;
use netlabcraft_model::pki::{CaServer, KeyPair, Trustpoint};
use netlabcraft_wire::rsa::{RsaKey, SigHash};
use netlabcraft_wire::sha256::sha256;
use netlabcraft_wire::tls::{Drbg, Identity};
use netlabcraft_wire::x509::{self, AltNames, Certificate, Crl, Issue, MAX_TIME, Name, Request};
use netlabcraft_wire::{http, rsa};

/// Path of the CA's certificate, and of the enrollment endpoint.
pub const CA_PATH: &str = "/pki/ca";
pub const ENROLL_PATH: &str = "/pki/enroll";
/// Path of the CA's revocation list.
pub const CRL_PATH: &str = "/pki/crl";
/// Most requests a CA keeps (granted ones count until they are revoked or forgotten).
pub const MAX_REQUESTS: usize = 32;

/// Where a certificate request stands at the CA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqState {
    /// Waiting for `crypto pki server NAME grant`.
    Pending,
    Granted,
    Rejected,
}

/// A certificate request a CA has received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertRequest {
    pub id: u32,
    /// The subject it asks a certificate for, as text.
    pub subject: String,
    /// The PKCS #10 request.
    pub der: Vec<u8>,
    pub state: ReqState,
}

/// The requests a CA router holds. They live in the running simulation, not in the lab file:
/// a request is repeated by the router that made it, so a reloaded lab asks again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaBook {
    pub requests: Vec<CertRequest>,
    next_id: u32,
}

impl CaBook {
    /// The place of the entry for `request`, adding one (granted at once when `auto`) when it is
    /// new. `None` when the book is full.
    pub fn register(&mut self, request: &Request, auto: bool) -> Option<usize> {
        if let Some(i) = self.requests.iter().position(|r| r.der == request.der()) {
            return Some(i);
        }
        if self.requests.len() >= MAX_REQUESTS {
            return None;
        }
        self.next_id = self.next_id.saturating_add(1);
        let state = if auto { ReqState::Granted } else { ReqState::Pending };
        self.requests.push(CertRequest { id: self.next_id, subject: request.subject.text(), der: request.der().to_vec(), state });
        Some(self.requests.len() - 1)
    }
}
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
    issue_alt(subject, subject_key, issuer, issuer_key, serial, now, days, ca, &AltNames::default())
}

/// [`issue`] for a certificate that is also for some DNS names and addresses.
#[allow(clippy::too_many_arguments)]
pub fn issue_alt(
    subject: &Name,
    subject_key: &RsaKey,
    issuer: &Name,
    issuer_key: &RsaKey,
    serial: &[u8],
    now: i64,
    days: u32,
    ca: bool,
    alt: &AltNames,
) -> Result<Certificate, String> {
    let not_before = u64::try_from(now).unwrap_or(0);
    let not_after = not_before.saturating_add(u64::from(days) * 86_400).min(MAX_TIME);
    Certificate::issue_alt(
        &Issue {
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
        },
        alt,
    )
    .map_err(|e| e.to_string())
}

/// The names a trustpoint's `fqdn` and `ip-address` ask its certificate to be for.
pub fn alt_of(t: &Trustpoint) -> AltNames {
    AltNames {
        dns: if t.fqdn.is_empty() { Vec::new() } else { vec![t.fqdn.clone()] },
        ips: t.ip_address.parse::<Ipv4Addr>().map(|a| vec![a.octets()]).unwrap_or_default(),
    }
}

/// A serial number as upper-case hex (no leading zeros).
pub fn serial_hex(serial: &[u8]) -> String {
    let h: String = serial.iter().map(|b| format!("{b:02X}")).collect();
    let t = h.trim_start_matches('0');
    if t.is_empty() { "0".to_string() } else { t.to_string() }
}

/// The bytes of a serial number typed in hex (any case, an odd count of digits allowed).
pub fn parse_serial(text: &str) -> Option<Vec<u8>> {
    let t = text.trim().trim_start_matches("0x");
    if t.is_empty() || t.len() > 40 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let t = if t.len() % 2 == 1 { format!("0{t}") } else { t.to_string() };
    let bytes: Option<Vec<u8>> = t.as_bytes().chunks(2).map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect();
    let bytes = bytes?;
    let start = bytes.iter().take_while(|b| **b == 0).count();
    let trimmed = bytes.get(start..)?.to_vec();
    if trimmed.is_empty() { None } else { Some(trimmed) }
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

/// A serial number for a request: the first bytes of a hash of the CA's certificate, the
/// request and the number the CA gave the request, so the same request granted again after a
/// revocation gets another serial.
fn serial_for(ca: &Certificate, req: &Request, id: u32) -> Vec<u8> {
    let h = sha256(&[ca.der(), req.der(), &id.to_be_bytes()].concat());
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
    [CA_PATH, ENROLL_PATH, CRL_PATH].contains(&req.path())
}

/// The certificate the CA signs for a request it has granted: valid for the CA's lifetime
/// setting (but not past the CA's own certificate), with the names the request asked for.
pub fn sign_request(d: &Device, now: i64, request_der: &[u8], id: u32) -> Result<Certificate, String> {
    let server = d.pki.server.as_ref().filter(|s| s.enabled).ok_or("this router is not running a certificate authority")?;
    let ca = Certificate::from_pem(&server.certificate).map_err(|_| "the certificate authority has no certificate".to_string())?;
    let request = Request::parse(request_der).map_err(|e| e.to_string())?;
    let ca_key = load_key(d, &server.name).ok_or("the certificate authority's key is missing")?;
    let issuer = ca_name(d, server)?;
    let not_before = u64::try_from(now).unwrap_or(0).max(ca.not_before);
    let not_after = not_before.saturating_add(u64::from(server.lifetime_days) * 86_400).min(ca.not_after);
    Certificate::issue_alt(
        &Issue {
            subject: &request.subject,
            subject_key: &request.key,
            issuer: &issuer,
            issuer_key: &ca_key,
            serial: &serial_for(&ca, &request, id),
            not_before,
            not_after,
            ca: false,
            path_len: None,
            hash: SigHash::Sha256,
        },
        &request.alt,
    )
    .map_err(|e| e.to_string())
}

/// The serial number the CA gave (or would give) the request with this number.
pub fn request_serial(d: &Device, request_der: &[u8], id: u32) -> Option<Vec<u8>> {
    let ca = Certificate::from_pem(&d.pki.server.as_ref()?.certificate).ok()?;
    Some(serial_for(&ca, &Request::parse(request_der).ok()?, id))
}

/// The revocation list the CA publishes now: every serial revoked so far, signed with its key,
/// good for its `lifetime crl` hours.
pub fn ca_crl(d: &Device, now: i64) -> Option<Crl> {
    let s = d.pki.server.as_ref().filter(|s| s.enabled)?;
    let ca = Certificate::from_pem(&s.certificate).ok()?;
    let key = load_key(d, &s.name)?;
    let serials: Vec<Vec<u8>> = s.revoked.iter().filter_map(|h| parse_serial(h)).collect();
    let this = u64::try_from(now).unwrap_or(0).max(ca.not_before);
    let next = this.saturating_add(u64::from(s.crl_hours) * 3600).min(MAX_TIME);
    Crl::issue(&ca.subject, &key, u64::from(s.crl_number), this, next, &serials, SigHash::Sha256).ok()
}

/// The CA's answer to a request at its three paths.
pub fn serve(d: &Device, now: i64, book: &mut CaBook, req: &http::Request) -> http::Response {
    let Some(server) = d.pki.server.as_ref().filter(|s| s.enabled) else {
        return text(503, "This router is not running a certificate authority.\n");
    };
    let Some(ca) = Certificate::from_pem(&server.certificate).ok() else {
        return text(503, "The certificate authority has no certificate.\n");
    };
    match (req.method.as_str(), req.path()) {
        ("GET", CA_PATH) => text(200, &ca.to_pem()),
        ("GET", CRL_PATH) => match ca_crl(d, now) {
            Some(crl) => text(200, &crl.to_pem()),
            None => text(500, "The revocation list could not be made.\n"),
        },
        ("POST", ENROLL_PATH) => {
            let body = String::from_utf8_lossy(&req.body);
            let parsed = x509::pem_decode("CERTIFICATE REQUEST", &body).and_then(|der| Request::parse(&der));
            let Ok(request) = parsed else { return text(400, "The body is not a valid signed certificate request.\n") };
            let Some(at) = book.register(&request, server.grant_auto) else {
                return text(503, "The CA holds too many requests: grant, reject or revoke some first.\n");
            };
            let Some(entry) = book.requests.get(at) else { return text(500, "The request book is damaged.\n") };
            match entry.state {
                ReqState::Pending => text(202, &format!("Request {} is waiting for the administrator to grant it.\n", entry.id)),
                ReqState::Rejected => text(403, &format!("Request {} was rejected by the administrator.\n", entry.id)),
                ReqState::Granted => match sign_request(d, now, request.der(), entry.id) {
                    Ok(c) => text(200, &c.to_pem()),
                    Err(e) => text(500, &format!("The certificate could not be made: {e}\n")),
                },
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

/// A certificate and the key that goes with it: what IKE signs with.
pub struct IkeIdentity {
    /// The trustpoint the certificate comes from.
    pub trustpoint: String,
    pub cert: Certificate,
    /// What IKE sends: the certificate, then the CA certificate the trustpoint authenticated
    /// (when it is another one), so that a peer that trusts only the root can verify it.
    pub chain: Vec<Certificate>,
    key: RsaKey,
}

/// The authorities a peer's certificate request named: IKEv1 by the DER of their names, IKEv2 by
/// the SHA-1 of their public keys. Empty when it named none (any certificate will do).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    pub names: Vec<Vec<u8>>,
    pub hashes: Vec<[u8; 20]>,
}

/// Most authorities taken from one peer's certificate request.
const MAX_WANTED: usize = 32;

impl Wanted {
    /// From the CERTREQ payloads of an IKEv1 message.
    pub fn from_v1(list: &[netlabcraft_wire::ike::Payload]) -> Wanted {
        Wanted {
            names: netlabcraft_wire::ike::cert_request_authorities(list).into_iter().take(MAX_WANTED).map(<[u8]>::to_vec).collect(),
            hashes: Vec::new(),
        }
    }

    /// From the CERTREQ payloads of an IKEv2 message.
    pub fn from_v2(list: &[netlabcraft_wire::ikev2::Payload]) -> Wanted {
        Wanted { names: Vec::new(), hashes: netlabcraft_wire::ikev2::cert_request_hashes(list) }
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.hashes.is_empty()
    }

    /// Whether a chain is acceptable to the peer: it asked for nothing, or some certificate of
    /// the chain is for, or was issued by, an authority it named.
    pub fn accepts(&self, chain: &[Certificate]) -> bool {
        self.is_empty()
            || chain.iter().any(|c| {
                self.names.iter().any(|n| n.as_slice() == c.issuer_der() || n.as_slice() == c.subject_der()) || self.hashes.contains(&c.key_hash())
            })
    }
}

/// The authorities this router trusts, as an IKEv1 request names them: the DER of each name.
pub fn request_names(d: &Device) -> Vec<Vec<u8>> {
    trust_anchors(d).iter().take(MAX_WANTED).map(|c| c.subject_der().to_vec()).collect()
}

/// The same as an IKEv2 request names them: the SHA-1 of each public key.
pub fn request_hashes(d: &Device) -> Vec<[u8; 20]> {
    trust_anchors(d).iter().take(MAX_WANTED).map(Certificate::key_hash).collect()
}

/// The reason an identity is refused when the peer asked for other authorities than the one
/// that issued it.
pub fn unwanted(me: &IkeIdentity) -> String {
    format!(
        "the peer asked (CERTREQ) for a certificate from other authorities than {}, which issued this router's certificate",
        me.cert.issuer.text()
    )
}

impl IkeIdentity {
    /// An RSA signature (PKCS #1 v1.5 over SHA-256) of `data`; `None` when the key is too small.
    pub fn sign(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.key.sign(SigHash::Sha256, data)
    }
}

/// The certificate IKE authenticates this router with (`authentication rsa-sig`): the one of the
/// named trustpoint, or of the first trustpoint whose certificate and key pair are both there
/// and match.
pub fn ike_identity(d: &Device, trustpoint: Option<&str>) -> Option<IkeIdentity> {
    ike_identity_for(d, trustpoint, &Wanted::default())
}

/// [`ike_identity`] for a peer that asked for certificates from some authorities: with no
/// trustpoint named, the first usable one that has such a certificate is chosen, and the first
/// usable one when none has (the caller then finds the answer unacceptable).
pub fn ike_identity_for(d: &Device, trustpoint: Option<&str>, wanted: &Wanted) -> Option<IkeIdentity> {
    let usable = |t: &Trustpoint| -> Option<IkeIdentity> {
        let cert = Certificate::from_pem(&t.certificate).ok()?;
        let key = load_key(d, &t.rsakeypair)?;
        if &cert.key != key.public() {
            return None;
        }
        let mut chain = vec![cert.clone()];
        if let Ok(ca) = Certificate::from_pem(&t.ca_certificate)
            && ca.der() != cert.der()
        {
            chain.push(ca);
        }
        Some(IkeIdentity { trustpoint: t.name.clone(), cert, chain, key })
    };
    match trustpoint {
        Some(n) => d.pki.trustpoint(n).and_then(usable),
        None => {
            let all: Vec<IkeIdentity> = d.pki.trustpoints.iter().filter_map(usable).collect();
            let at = all.iter().position(|i| wanted.accepts(&i.chain)).unwrap_or(0);
            all.into_iter().nth(at)
        }
    }
}

/// Checks what a peer proved itself with: its certificate (and any CA certificates it sent after
/// it) must chain to a certificate this device trusts, inside its validity window at this
/// device's clock `now`, and `sig` must be the peer's signature of `data` under the certificate's
/// key. The peer's certificate on success.
pub fn verify_peer(d: &Device, now: i64, certs: &[&[u8]], data: &[u8], sig: &[u8], id: Option<Ipv4Addr>) -> Result<Certificate, String> {
    if certs.is_empty() {
        return Err("the peer sent no certificate".into());
    }
    let mut chain = Vec::new();
    for c in certs.iter().take(x509::MAX_CHAIN) {
        chain.push(Certificate::parse(c).map_err(|e| format!("the peer's certificate is not usable ({e})"))?);
    }
    let anchors = trust_anchors(d);
    if anchors.is_empty() {
        return Err("this router trusts no certificate authority (`crypto pki authenticate`)".into());
    }
    let at = u64::try_from(now).unwrap_or(0);
    x509::verify_chain(&chain, &anchors, at).map_err(|e| format!("the peer's certificate is refused: {e}"))?;
    revocation_verdict(d, at, &chain).map_err(|e| format!("the peer's certificate is refused: {e}"))?;
    let leaf = chain.first().ok_or("the peer sent no certificate")?;
    // A certificate that names addresses is for those only: the peer's identity must be one.
    if let Some(ip) = id
        && !leaf.alt.ips.is_empty()
        && !leaf.alt.ips.contains(&ip.octets())
    {
        let named: Vec<String> = leaf.alt.ips.iter().map(|a| Ipv4Addr::from(*a).to_string()).collect();
        return Err(format!("the peer's certificate is for {}, not for the address {ip} it identified itself with", named.join(", ")));
    }
    if !leaf.key.verify(SigHash::Sha256, data, sig) {
        return Err("the peer's signature does not verify with the key in its certificate".into());
    }
    Ok(leaf.clone())
}

/// Whether a certificate is, or was signed by, one of the authorities a device trusts (names and
/// signatures only: dates are checked when a peer presents it).
pub fn trusts(d: &Device, cert: &Certificate) -> bool {
    trust_anchors(d).iter().any(|a| a.der() == cert.der() || (a.ca && cert.issuer == a.subject && cert.signed_by(&a.key)))
}

/// The certificate [`ike_identity`] would sign with towards `peer`, found without loading the
/// key (for checks of the configuration made on every packet): of the certificates this device
/// holds a key pair for, one the peer trusts when there is one (the peer's request names its
/// authorities), else the first.
pub fn ike_certificate_for(d: &Device, peer: &Device) -> Option<Certificate> {
    let all: Vec<Certificate> =
        d.pki.trustpoints.iter().filter(|t| d.pki.key(&t.rsakeypair).is_some()).filter_map(|t| Certificate::from_pem(&t.certificate).ok()).collect();
    all.iter().find(|c| trusts(peer, c)).or_else(|| all.first()).cloned()
}

/// The address an IKE identification payload body (type, protocol, port, data) names, when it is
/// an IPv4 address.
pub fn ip_of_id_body(body: &[u8]) -> Option<Ipv4Addr> {
    match body {
        [1, _, _, _, a, b, c, d] => Some(Ipv4Addr::new(*a, *b, *c, *d)),
        _ => None,
    }
}

/// Whether a certificate was revoked, by this router's own CA or by a CRL a trustpoint with
/// `revocation-check crl` holds. Without a usable CRL the check fails unless the trustpoint's
/// `revocation-check` ends with `none`.
fn revocation_verdict(d: &Device, now: u64, chain: &[Certificate]) -> Result<(), String> {
    let own = d.pki.server.as_ref().filter(|s| s.enabled).and_then(|s| Certificate::from_pem(&s.certificate).ok().map(|c| (s, c)));
    for (i, cert) in chain.iter().enumerate() {
        if let Some((s, ca)) = &own
            && cert.issuer == ca.subject
            && cert.signed_by(&ca.key)
            && s.revoked.iter().any(|r| parse_serial(r).is_some_and(|b| b == cert.serial))
        {
            return Err(format!("certificate {i} (serial {}) was revoked by this router's authority", serial_hex(&cert.serial)));
        }
        for t in d.pki.trustpoints.iter().filter(|t| t.revocation_crl) {
            let Ok(ca) = Certificate::from_pem(&t.ca_certificate) else { continue };
            if cert.issuer != ca.subject || !cert.signed_by(&ca.key) {
                continue;
            }
            match Crl::from_pem(&t.crl) {
                Ok(crl) if crl.issued_by(&ca) => {
                    if !crl.current_at(now) {
                        if !t.revocation_none {
                            return Err(format!("the CRL of {} is out of date (`crypto pki crl request {}`)", ca.subject.text(), t.name));
                        }
                    } else if crl.is_revoked(&cert.serial) {
                        return Err(format!("certificate {i} (serial {}) was revoked by {}", serial_hex(&cert.serial), ca.subject.text()));
                    }
                }
                _ => {
                    if !t.revocation_none {
                        return Err(format!(
                            "this router holds no CRL of {} and `revocation-check crl` goes no further without one (`crypto pki crl request {}`)",
                            ca.subject.text(),
                            t.name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The CRLs a device holds: the ones of its trustpoints, and its own CA's.
pub fn trust_crls(d: &Device, now: i64) -> Vec<Crl> {
    let mut out: Vec<Crl> = d.pki.trustpoints.iter().filter_map(|t| Crl::from_pem(&t.crl).ok()).collect();
    out.extend(ca_crl(d, now));
    out
}

impl crate::Sim {
    /// The requests a device's CA holds, oldest first.
    pub fn pki_requests(&self, d: DeviceId) -> Vec<CertRequest> {
        self.state.get(&d).map(|s| s.pki_book.requests.clone()).unwrap_or_default()
    }

    /// Adds a request to a CA's book (granted at once when `auto`): its number and state.
    pub fn pki_submit(&mut self, d: DeviceId, request: &Request, auto: bool) -> Option<(u32, ReqState)> {
        let book = &mut self.st(d).pki_book;
        let at = book.register(request, auto)?;
        book.requests.get(at).map(|r| (r.id, r.state))
    }

    /// Sets the state of one request, or of every pending one (`id` `None`): the numbers it
    /// changed.
    pub fn pki_decide(&mut self, d: DeviceId, id: Option<u32>, state: ReqState) -> Vec<u32> {
        let mut changed = Vec::new();
        for r in &mut self.st(d).pki_book.requests {
            let hit = match id {
                Some(n) => r.id == n,
                None => r.state == ReqState::Pending,
            };
            if hit && r.state != state {
                r.state = state;
                changed.push(r.id);
            }
        }
        changed
    }

    /// Forgets the requests with these numbers (so a later request for the same key is new).
    pub fn pki_forget(&mut self, d: DeviceId, ids: &[u32]) {
        self.st(d).pki_book.requests.retain(|r| !ids.contains(&r.id));
    }
}
