//! X.509 v3 certificates (RFC 5280) and PKCS #10 certificate requests (RFC 2986), small and
//! strict: RSA keys, `sha256WithRSAEncryption` (and `sha1WithRSAEncryption` accepted), names of
//! country, organisation, organisational unit and common name, validity in seconds since the
//! Unix epoch (the simulator's clock decides what that means), and the basicConstraints
//! extension. Chain verification checks signatures, validity windows, the CA flag, the path
//! length and the trust anchors.
//!
//! Hostile input is the norm: a certificate may be at most [`MAX_CERT`] bytes, a name has at
//! most four attributes of at most [`MAX_NAME_LEN`] bytes, a chain at most [`MAX_CHAIN`]
//! certificates, and anything this module does not model (other name attributes, unique ids,
//! unknown critical extensions, v1 and v2 certificates) is refused rather than guessed at.
//! Key usage, key identifiers and alternative names are understood but not enforced.

use crate::der::{self, Tlv};
use crate::http::{base64_decode, base64_encode};
use crate::rsa::{RsaKey, RsaPublicKey, SigHash};
use crate::{Result, WireError};

/// Largest certificate or request accepted (bytes).
pub const MAX_CERT: usize = 8192;
pub const MAX_NAME_LEN: usize = 64;
pub const MAX_CHAIN: usize = 8;
const MAX_SERIAL: usize = 20;
/// Latest time that can be encoded (9999-12-31T23:59:59Z).
pub const MAX_TIME: u64 = 253_402_300_799;

const OID_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_SHA256_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
const OID_SHA1_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x05];
const OID_CN: &[u8] = &[0x55, 0x04, 0x03];
const OID_C: &[u8] = &[0x55, 0x04, 0x06];
const OID_O: &[u8] = &[0x55, 0x04, 0x0a];
const OID_OU: &[u8] = &[0x55, 0x04, 0x0b];
const OID_BASIC: &[u8] = &[0x55, 0x1d, 0x13];
const OID_SAN: &[u8] = &[0x55, 0x1d, 0x11];
const OID_CRL_NUMBER: &[u8] = &[0x55, 0x1d, 0x14];
/// PKCS #9 extensionRequest: the attribute a request carries its extensions in.
const OID_EXT_REQ: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x0e];
/// Most alternative names of each kind in a certificate or request.
pub const MAX_ALT: usize = 4;
/// Most revoked certificates in a CRL, and its largest size in bytes.
pub const MAX_REVOKED: usize = 128;
pub const MAX_CRL: usize = 16_384;
/// Extensions that are understood and ignored (key usage, key identifiers, alternative name).
const OID_IGNORED: [&[u8]; 4] = [&[0x55, 0x1d, 0x0f], &[0x55, 0x1d, 0x0e], &[0x55, 0x1d, 0x23], &[0x55, 0x1d, 0x11]];

fn bad(why: &str) -> WireError {
    WireError::Unsupported("x509", why.to_string())
}

fn oid(content: &[u8]) -> Vec<u8> {
    der::tlv(der::OID, content)
}

fn sig_oid(hash: SigHash) -> &'static [u8] {
    match hash {
        SigHash::Sha256 => OID_SHA256_RSA,
        SigHash::Sha1 => OID_SHA1_RSA,
    }
}

/// `AlgorithmIdentifier { sigAlg, NULL }`.
fn sig_alg(hash: SigHash) -> Vec<u8> {
    der::sequence(&[&oid(sig_oid(hash)), &der::null()])
}

fn parse_sig_alg(t: &Tlv<'_>) -> Result<SigHash> {
    let items = t.items(der::SEQUENCE)?;
    let (Some(o), Some(p), 2) = (items.first(), items.get(1), items.len()) else {
        return Err(bad("signature algorithm"));
    };
    if p.raw != der::null().as_slice() || o.tag != der::OID {
        return Err(bad("signature algorithm parameters"));
    }
    match o.content {
        c if c == OID_SHA256_RSA => Ok(SigHash::Sha256),
        c if c == OID_SHA1_RSA => Ok(SigHash::Sha1),
        _ => Err(bad("unsupported signature algorithm")),
    }
}

// ---- names ----------------------------------------------------------------------------------

/// A distinguished name of up to four attributes; an empty string means the attribute is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Name {
    pub common_name: String,
    pub organization: String,
    pub unit: String,
    pub country: String,
}

fn name_text_ok(s: &str) -> bool {
    s.len() <= MAX_NAME_LEN && s.chars().all(|c| !c.is_control())
}

impl Name {
    pub fn new(common_name: &str, organization: &str) -> Name {
        Name { common_name: common_name.into(), organization: organization.into(), ..Name::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.common_name.is_empty() && self.organization.is_empty() && self.unit.is_empty() && self.country.is_empty()
    }

    /// `CN=…, O=…` style text, most specific attribute first.
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        for (k, v) in [("CN", &self.common_name), ("OU", &self.unit), ("O", &self.organization), ("C", &self.country)] {
            if !v.is_empty() {
                parts.push(format!("{k}={v}"));
            }
        }
        parts.join(", ")
    }

    /// DER `Name`: one single-attribute set per attribute, country first, common name last.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.is_empty() {
            return Err(bad("empty name"));
        }
        let mut rdns = Vec::new();
        for (o, v, printable) in
            [(OID_C, &self.country, true), (OID_O, &self.organization, false), (OID_OU, &self.unit, false), (OID_CN, &self.common_name, false)]
        {
            if v.is_empty() {
                continue;
            }
            if !name_text_ok(v) || (printable && v.len() != 2) {
                return Err(bad("name attribute"));
            }
            let tag = if printable { der::PRINTABLE_STRING } else { der::UTF8_STRING };
            let atv = der::sequence(&[&oid(o), &der::tlv(tag, v.as_bytes())]);
            rdns.push(der::set(&[&atv]));
        }
        let refs: Vec<&[u8]> = rdns.iter().map(Vec::as_slice).collect();
        Ok(der::sequence(&refs))
    }

    fn parse(t: &Tlv<'_>) -> Result<Name> {
        let mut name = Name::default();
        let rdns = t.items(der::SEQUENCE)?;
        if rdns.is_empty() || rdns.len() > 4 {
            return Err(bad("name attributes"));
        }
        for rdn in rdns {
            let atvs = rdn.items(der::SET)?;
            let (Some(atv), 1) = (atvs.first(), atvs.len()) else { return Err(bad("multi-valued name part")) };
            let pair = atv.items(der::SEQUENCE)?;
            let (Some(o), Some(v), 2) = (pair.first(), pair.get(1), pair.len()) else { return Err(bad("name attribute")) };
            if o.tag != der::OID || !matches!(v.tag, der::UTF8_STRING | der::PRINTABLE_STRING | der::IA5_STRING) {
                return Err(bad("name attribute type"));
            }
            let text = std::str::from_utf8(v.content).map_err(|_| bad("name text"))?;
            if text.is_empty() || !name_text_ok(text) {
                return Err(bad("name text"));
            }
            let slot = match o.content {
                c if c == OID_CN => &mut name.common_name,
                c if c == OID_O => &mut name.organization,
                c if c == OID_OU => &mut name.unit,
                c if c == OID_C => &mut name.country,
                _ => return Err(bad("unsupported name attribute")),
            };
            if !slot.is_empty() {
                return Err(bad("repeated name attribute"));
            }
            *slot = text.to_string();
        }
        Ok(name)
    }
}

// ---- time -----------------------------------------------------------------------------------

/// Days since 1970-01-01 of a civil date (the proleptic Gregorian calendar).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Seconds since the Unix epoch as `(year, month, day, hour, minute, second)`.
pub fn civil(t: u64) -> (u32, u32, u32, u32, u32, u32) {
    let t = t.min(MAX_TIME);
    let (y, m, d) = civil_from_days((t / 86_400) as i64);
    let s = t % 86_400;
    (y as u32, m as u32, d as u32, (s / 3600) as u32, (s % 3600 / 60) as u32, (s % 60) as u32)
}

/// Seconds since the Unix epoch of a calendar time; `None` when the fields are not a real time
/// between 1970 and 9999.
pub fn from_civil(y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<u64> {
    let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let dim = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + u32::from(leap),
        _ => return None,
    };
    if !(1970..=9999).contains(&y) || d == 0 || d > dim || h > 23 || mi > 59 || s > 59 {
        return None;
    }
    let days = days_from_civil(i64::from(y), i64::from(mo), i64::from(d));
    u64::try_from(days).ok()?.checked_mul(86_400)?.checked_add(u64::from(h * 3600 + mi * 60 + s))
}

fn encode_time(t: u64) -> Vec<u8> {
    let (y, mo, d, h, mi, s) = civil(t);
    if y < 2050 {
        der::tlv(der::UTC_TIME, format!("{:02}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z", y % 100).as_bytes())
    } else {
        der::tlv(der::GENERALIZED_TIME, format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z").as_bytes())
    }
}

fn parse_time(t: &Tlv<'_>) -> Result<u64> {
    let text = std::str::from_utf8(t.content).map_err(|_| bad("time"))?;
    let digits = text.strip_suffix('Z').ok_or_else(|| bad("time without Z"))?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad("time digits"));
    }
    let (year_len, base) = match (t.tag, digits.len()) {
        (der::UTC_TIME, 12) => (2, 0),
        (der::GENERALIZED_TIME, 14) => (4, 0),
        _ => return Err(bad("time form")),
    };
    let num = |from: usize, len: usize| digits.get(from..from + len).and_then(|s| s.parse::<u32>().ok()).ok_or_else(|| bad("time"));
    let mut y = num(0, year_len)?;
    if year_len == 2 {
        y += if y >= 50 { 1900 } else { 2000 };
    }
    let o = year_len;
    let _ = base;
    from_civil(y, num(o, 2)?, num(o + 2, 2)?, num(o + 4, 2)?, num(o + 6, 2)?, num(o + 8, 2)?).ok_or_else(|| bad("time value"))
}

// ---- keys -----------------------------------------------------------------------------------

fn encode_spki(key: &RsaPublicKey) -> Vec<u8> {
    let alg = der::sequence(&[&oid(OID_RSA), &der::null()]);
    let inner = der::sequence(&[&der::uint(&key.modulus()), &der::uint(&key.exponent().to_be_bytes())]);
    der::sequence(&[&alg, &der::bit_string(&inner)])
}

fn parse_spki(t: &Tlv<'_>) -> Result<RsaPublicKey> {
    let items = t.items(der::SEQUENCE)?;
    let (Some(alg), Some(bits), 2) = (items.first(), items.get(1), items.len()) else { return Err(bad("public key info")) };
    let a = alg.items(der::SEQUENCE)?;
    let (Some(o), Some(p), 2) = (a.first(), a.get(1), a.len()) else { return Err(bad("public key algorithm")) };
    if o.content != OID_RSA || o.tag != der::OID || p.raw != der::null().as_slice() {
        return Err(bad("only RSA keys"));
    }
    let body = der::bit_string_bytes(bits)?;
    let pair = der::read_exact(body, der::SEQUENCE)?.items(der::SEQUENCE)?;
    let (Some(n), Some(e), 2) = (pair.first(), pair.get(1), pair.len()) else { return Err(bad("RSA public key")) };
    let e = e.uint()?;
    if e.len() > 8 {
        return Err(bad("exponent too large"));
    }
    let e = e.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b));
    RsaPublicKey::new(n.uint()?, e)
}

// ---- alternative names ----------------------------------------------------------------------

/// The subjectAltName entries this module models: DNS names and IPv4 addresses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AltNames {
    pub dns: Vec<String>,
    pub ips: Vec<[u8; 4]>,
}

fn dns_text_ok(t: &str) -> bool {
    !t.is_empty() && t.len() <= MAX_NAME_LEN && t.bytes().all(|b| b.is_ascii_graphic())
}

/// Whether a certificate name (maybe `*.example`, one label only) names `host` (lower case).
fn dns_match(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(tail) => host.split_once('.').is_some_and(|(label, rest)| !label.is_empty() && rest == tail),
        None => pattern == host,
    }
}

impl AltNames {
    pub fn is_empty(&self) -> bool {
        self.dns.is_empty() && self.ips.is_empty()
    }

    /// The GeneralNames: a SEQUENCE of `[2]` DNS names and `[7]` addresses.
    fn encode_names(&self) -> Result<Vec<u8>> {
        if self.dns.len() > MAX_ALT || self.ips.len() > MAX_ALT {
            return Err(bad("too many alternative names"));
        }
        let mut parts = Vec::new();
        for n in &self.dns {
            if !dns_text_ok(n) {
                return Err(bad("alternative name"));
            }
            parts.push(der::tlv(0x82, n.as_bytes()));
        }
        for ip in &self.ips {
            parts.push(der::tlv(0x87, ip));
        }
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        Ok(der::sequence(&refs))
    }

    /// The extension: `{ subjectAltName, OCTET STRING { names } }`.
    fn encode_extension(&self) -> Result<Vec<u8>> {
        Ok(der::sequence(&[&oid(OID_SAN), &der::tlv(der::OCTET_STRING, &self.encode_names()?)]))
    }

    /// Reads the names out of an extension's value; kinds this module does not model are skipped.
    fn parse_names(value: &[u8]) -> Result<AltNames> {
        let mut alt = AltNames::default();
        for it in der::read_exact(value, der::SEQUENCE)?.items(der::SEQUENCE)? {
            match it.tag {
                0x82 => {
                    let t = std::str::from_utf8(it.content).map_err(|_| bad("alternative name"))?;
                    if !dns_text_ok(t) || alt.dns.len() >= MAX_ALT {
                        return Err(bad("alternative name"));
                    }
                    alt.dns.push(t.to_string());
                }
                0x87 => {
                    if let Ok(ip) = <[u8; 4]>::try_from(it.content) {
                        if alt.ips.len() >= MAX_ALT {
                            return Err(bad("too many alternative names"));
                        }
                        alt.ips.push(ip);
                    }
                }
                _ => {}
            }
        }
        Ok(alt)
    }
}

// ---- certificates ---------------------------------------------------------------------------

/// A parsed, well-formed certificate. Parsing does not check the signature or the dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate {
    der: Vec<u8>,
    tbs: Vec<u8>,
    sig: Vec<u8>,
    hash: SigHash,
    pub serial: Vec<u8>,
    pub issuer: Name,
    pub subject: Name,
    issuer_raw: Vec<u8>,
    subject_raw: Vec<u8>,
    pub not_before: u64,
    pub not_after: u64,
    pub key: RsaPublicKey,
    /// The basicConstraints CA flag.
    pub ca: bool,
    /// The basicConstraints path length limit, when present.
    pub path_len: Option<u32>,
    /// The subjectAltName entries.
    pub alt: AltNames,
}

/// What to put in a certificate.
#[derive(Debug, Clone)]
pub struct Issue<'a> {
    pub subject: &'a Name,
    pub subject_key: &'a RsaPublicKey,
    pub issuer: &'a Name,
    pub issuer_key: &'a RsaKey,
    pub serial: &'a [u8],
    pub not_before: u64,
    pub not_after: u64,
    pub ca: bool,
    pub path_len: Option<u8>,
    pub hash: SigHash,
}

impl Certificate {
    /// Builds and signs a certificate. Self-signed when the issuer key is the subject key.
    pub fn issue(p: &Issue<'_>) -> Result<Certificate> {
        Certificate::issue_alt(p, &AltNames::default())
    }

    /// [`Certificate::issue`] with alternative names (DNS names and addresses the certificate
    /// is also for).
    pub fn issue_alt(p: &Issue<'_>, alt: &AltNames) -> Result<Certificate> {
        let serial = p.serial.get(p.serial.iter().take_while(|b| **b == 0).count()..).unwrap_or_default();
        if serial.is_empty() || serial.len() > MAX_SERIAL {
            return Err(bad("serial number"));
        }
        if p.not_before > p.not_after || p.not_after > MAX_TIME {
            return Err(bad("validity"));
        }
        let validity = der::sequence(&[&encode_time(p.not_before), &encode_time(p.not_after)]);
        let version = der::tlv(0xa0, &der::uint(&[2]));
        let mut bc_parts = Vec::new();
        if p.ca {
            bc_parts.push(der::boolean(true));
            if let Some(n) = p.path_len {
                bc_parts.push(der::uint(&[n]));
            }
        }
        let bc_refs: Vec<&[u8]> = bc_parts.iter().map(Vec::as_slice).collect();
        let bc = der::sequence(&[&oid(OID_BASIC), &der::boolean(true), &der::tlv(der::OCTET_STRING, &der::sequence(&bc_refs))]);
        let san = if alt.is_empty() { Vec::new() } else { alt.encode_extension()? };
        let exts = if san.is_empty() { der::tlv(0xa3, &der::sequence(&[&bc])) } else { der::tlv(0xa3, &der::sequence(&[&bc, &san])) };
        let tbs = der::sequence(&[
            &version,
            &der::uint(serial),
            &sig_alg(p.hash),
            &p.issuer.encode()?,
            &validity,
            &p.subject.encode()?,
            &encode_spki(p.subject_key),
            &exts,
        ]);
        let sig = p.issuer_key.sign(p.hash, &tbs).ok_or_else(|| bad("key too small to sign"))?;
        let cert = der::sequence(&[&tbs, &sig_alg(p.hash), &der::bit_string(&sig)]);
        Certificate::parse(&cert)
    }

    /// Parses one DER certificate (the whole input).
    pub fn parse(input: &[u8]) -> Result<Certificate> {
        if input.len() > MAX_CERT {
            return Err(bad("certificate too large"));
        }
        let top = der::read_exact(input, der::SEQUENCE)?;
        let parts = top.items(der::SEQUENCE)?;
        let (Some(tbs), Some(alg), Some(sigv), 3) = (parts.first(), parts.get(1), parts.get(2), parts.len()) else {
            return Err(bad("certificate layout"));
        };
        let hash = parse_sig_alg(alg)?;
        let sig = der::bit_string_bytes(sigv)?.to_vec();
        let f = tbs.items(der::SEQUENCE)?;
        let mut it = f.iter();
        let version = it.next().ok_or_else(|| bad("tbs layout"))?;
        let v = der::read_exact(version.content, der::INTEGER)?;
        if version.tag != 0xa0 || v.uint()? != [2] {
            return Err(bad("only version 3"));
        }
        let (Some(serial), Some(inner_alg), Some(issuer), Some(validity), Some(subject), Some(spki)) =
            (it.next(), it.next(), it.next(), it.next(), it.next(), it.next())
        else {
            return Err(bad("tbs layout"));
        };
        let serial = serial.uint()?.to_vec();
        if serial.is_empty() || serial.len() > MAX_SERIAL {
            return Err(bad("serial number"));
        }
        if inner_alg.raw != alg.raw {
            return Err(bad("signature algorithms differ"));
        }
        let times = validity.items(der::SEQUENCE)?;
        let (Some(nb), Some(na), 2) = (times.first(), times.get(1), times.len()) else { return Err(bad("validity")) };
        let (not_before, not_after) = (parse_time(nb)?, parse_time(na)?);
        let key = parse_spki(spki)?;
        let (mut ca, mut path_len) = (false, None);
        let mut alt = AltNames::default();
        if let Some(ext) = it.next() {
            if ext.tag != 0xa3 || it.next().is_some() {
                return Err(bad("unsupported tbs field"));
            }
            let list = der::read_exact(ext.content, der::SEQUENCE)?.items(der::SEQUENCE)?;
            let mut seen_basic = false;
            let mut seen_san = false;
            for e in list {
                let fields = e.items(der::SEQUENCE)?;
                let (Some(id), Some(rest)) = (fields.first(), fields.get(1..)) else { return Err(bad("extension")) };
                let (critical, value) = match rest {
                    [v] => (false, v),
                    [c, v] => (c.boolean()?, v),
                    _ => return Err(bad("extension")),
                };
                if id.tag != der::OID || value.tag != der::OCTET_STRING {
                    return Err(bad("extension"));
                }
                if id.content == OID_BASIC {
                    if seen_basic {
                        return Err(bad("repeated basicConstraints"));
                    }
                    seen_basic = true;
                    let bc = der::read_exact(value.content, der::SEQUENCE)?.items(der::SEQUENCE)?;
                    let mut b = bc.iter().peekable();
                    if let Some(flag) = b.next_if(|t| t.tag == der::BOOLEAN) {
                        ca = flag.boolean()?;
                    }
                    if let Some(n) = b.next_if(|t| t.tag == der::INTEGER) {
                        let n = n.uint()?;
                        if n.len() > 4 {
                            return Err(bad("path length"));
                        }
                        path_len = Some(n.iter().fold(0u32, |a, x| (a << 8) | u32::from(*x)));
                    }
                    if b.next().is_some() {
                        return Err(bad("basicConstraints"));
                    }
                } else if id.content == OID_SAN {
                    if seen_san {
                        return Err(bad("repeated alternative names"));
                    }
                    seen_san = true;
                    alt = AltNames::parse_names(value.content)?;
                } else if critical && !OID_IGNORED.contains(&id.content) {
                    return Err(bad("unknown critical extension"));
                }
            }
        }
        Ok(Certificate {
            der: input.to_vec(),
            tbs: tbs.raw.to_vec(),
            sig,
            hash,
            serial,
            issuer: Name::parse(issuer)?,
            subject: Name::parse(subject)?,
            issuer_raw: issuer.raw.to_vec(),
            subject_raw: subject.raw.to_vec(),
            not_before,
            not_after,
            key,
            ca,
            path_len,
            alt,
        })
    }

    /// Whether the certificate is for `host`, a DNS name or an IPv4 address in text (RFC 6125
    /// in short): an address matches the certificate's address entries, a name its DNS entries
    /// (`*.example.net` covers one label); a certificate with no entry of the kind falls back to
    /// its common name.
    pub fn matches_host(&self, host: &str) -> bool {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
            if !self.alt.ips.is_empty() {
                return self.alt.ips.contains(&ip.octets());
            }
            return self.subject.common_name.eq_ignore_ascii_case(&host);
        }
        if !self.alt.dns.is_empty() {
            return self.alt.dns.iter().any(|n| dns_match(n, &host));
        }
        dns_match(&self.subject.common_name, &host)
    }

    /// The DER encoding.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// The hash of its signature algorithm.
    pub fn hash(&self) -> SigHash {
        self.hash
    }

    pub fn self_issued(&self) -> bool {
        self.issuer_raw == self.subject_raw
    }

    /// Whether the signature was made by this key.
    pub fn signed_by(&self, issuer_key: &RsaPublicKey) -> bool {
        issuer_key.verify(self.hash, &self.tbs, &self.sig)
    }

    /// Whether the validity window holds at `now`.
    pub fn valid_at(&self, now: u64) -> bool {
        self.not_before <= now && now <= self.not_after
    }

    /// PEM text (`-----BEGIN CERTIFICATE-----`), 64 columns.
    pub fn to_pem(&self) -> String {
        pem_encode("CERTIFICATE", &self.der)
    }

    pub fn from_pem(text: &str) -> Result<Certificate> {
        Certificate::parse(&pem_decode("CERTIFICATE", text)?)
    }
}

pub fn pem_encode(label: &str, der: &[u8]) -> String {
    let b64 = base64_encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for line in b64.as_bytes().chunks(64) {
        out.push_str(&String::from_utf8_lossy(line));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// The DER inside a PEM block with this label: one block, strict markers, base64 lines.
pub fn pem_decode(label: &str, text: &str) -> Result<Vec<u8>> {
    if text.len() > MAX_CERT * 2 {
        return Err(bad("pem too large"));
    }
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some(begin.as_str()) {
        return Err(bad("pem begin line"));
    }
    let mut b64 = String::new();
    let mut closed = false;
    for l in lines.by_ref() {
        if l == end {
            closed = true;
            break;
        }
        b64.push_str(l);
    }
    if !closed || lines.next().is_some() {
        return Err(bad("pem end line"));
    }
    base64_decode(&b64).ok_or_else(|| bad("pem base64"))
}

// ---- chain verification ---------------------------------------------------------------------

/// Why a chain was not accepted; the number is the position in the chain (0 is the leaf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    Empty,
    TooLong,
    NotYetValid(usize),
    Expired(usize),
    /// The issuer's name does not match, or its signature does not verify.
    BadSignature(usize),
    /// The certificate that signed this one is not a CA.
    NotCa(usize),
    /// Too many CAs below a certificate whose path length forbids it.
    PathLength(usize),
    /// The chain ends at a certificate no trust anchor vouches for.
    Untrusted,
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainError::Empty => write!(f, "no certificate was offered"),
            ChainError::TooLong => write!(f, "the chain is longer than {MAX_CHAIN} certificates"),
            ChainError::NotYetValid(i) => write!(f, "certificate {i} is not valid yet"),
            ChainError::Expired(i) => write!(f, "certificate {i} has expired"),
            ChainError::BadSignature(i) => write!(f, "certificate {i} was not signed by the next one up"),
            ChainError::NotCa(i) => write!(f, "the signer of certificate {i} is not a certificate authority"),
            ChainError::PathLength(i) => write!(f, "the signer of certificate {i} allows fewer CAs below it"),
            ChainError::Untrusted => write!(f, "the chain does not end at a trusted certificate"),
        }
    }
}

/// Checks a chain (leaf first, each certificate signed by the next) against trust anchors at
/// time `now`: every certificate inside its validity window, every signature good, every signer
/// a CA with a large enough path length, and the chain ending at, or issued by, an anchor.
pub fn verify_chain(chain: &[Certificate], anchors: &[Certificate], now: u64) -> std::result::Result<(), ChainError> {
    if chain.is_empty() {
        return Err(ChainError::Empty);
    }
    if chain.len() > MAX_CHAIN {
        return Err(ChainError::TooLong);
    }
    for (i, cert) in chain.iter().enumerate() {
        if now < cert.not_before {
            return Err(ChainError::NotYetValid(i));
        }
        if now > cert.not_after {
            return Err(ChainError::Expired(i));
        }
        // The certificate is itself an anchor: the chain ends here.
        if anchors.iter().any(|a| a.der == cert.der) {
            return Ok(());
        }
        // The signer: the next certificate of the chain, or an anchor.
        let (signer, last) = match chain.get(i + 1) {
            Some(next) => (Some(next), false),
            None => (anchors.iter().find(|a| a.subject_raw == cert.issuer_raw && cert.signed_by(&a.key)), true),
        };
        let Some(signer) = signer else { return Err(if last { ChainError::Untrusted } else { ChainError::BadSignature(i) }) };
        if signer.subject_raw != cert.issuer_raw || !cert.signed_by(&signer.key) {
            return Err(ChainError::BadSignature(i));
        }
        if !signer.ca {
            return Err(ChainError::NotCa(i));
        }
        if signer.path_len.is_some_and(|max| i as u64 > u64::from(max)) {
            return Err(ChainError::PathLength(i));
        }
        if last {
            if now < signer.not_before {
                return Err(ChainError::NotYetValid(i + 1));
            }
            if now > signer.not_after {
                return Err(ChainError::Expired(i + 1));
            }
            return Ok(());
        }
    }
    Err(ChainError::Untrusted)
}

// ---- revocation lists -----------------------------------------------------------------------

fn trim_zeros(s: &[u8]) -> &[u8] {
    s.get(s.iter().take_while(|b| **b == 0).count()..).unwrap_or_default()
}

/// A certificate revocation list (RFC 5280 §5): the serial numbers an authority has revoked,
/// the time of the list and of the next one, and the authority's signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crl {
    der: Vec<u8>,
    tbs: Vec<u8>,
    sig: Vec<u8>,
    hash: SigHash,
    pub issuer: Name,
    issuer_raw: Vec<u8>,
    pub this_update: u64,
    pub next_update: u64,
    /// The cRLNumber extension (0 when absent).
    pub number: u64,
    revoked: Vec<Vec<u8>>,
}

impl Crl {
    /// Builds and signs a list of `revoked` serial numbers (big-endian, leading zeros ignored).
    pub fn issue(
        issuer: &Name,
        issuer_key: &RsaKey,
        number: u64,
        this_update: u64,
        next_update: u64,
        revoked: &[Vec<u8>],
        hash: SigHash,
    ) -> Result<Crl> {
        if this_update > next_update || next_update > MAX_TIME {
            return Err(bad("crl validity"));
        }
        if revoked.len() > MAX_REVOKED {
            return Err(bad("too many revoked certificates"));
        }
        let mut entries = Vec::new();
        for serial in revoked {
            let serial = trim_zeros(serial);
            if serial.is_empty() || serial.len() > MAX_SERIAL {
                return Err(bad("serial number"));
            }
            entries.push(der::sequence(&[&der::uint(serial), &encode_time(this_update)]));
        }
        let entry_refs: Vec<&[u8]> = entries.iter().map(Vec::as_slice).collect();
        let n = number.to_be_bytes();
        let number_ext = der::sequence(&[&oid(OID_CRL_NUMBER), &der::tlv(der::OCTET_STRING, &der::uint(trim_zeros(&n)))]);
        let exts = der::tlv(0xa0, &der::sequence(&[&number_ext]));
        let mut parts: Vec<Vec<u8>> = vec![der::uint(&[1]), sig_alg(hash), issuer.encode()?, encode_time(this_update), encode_time(next_update)];
        if !entries.is_empty() {
            parts.push(der::sequence(&entry_refs));
        }
        parts.push(exts);
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let tbs = der::sequence(&refs);
        let sig = issuer_key.sign(hash, &tbs).ok_or_else(|| bad("key too small to sign"))?;
        let raw = der::sequence(&[&tbs, &sig_alg(hash), &der::bit_string(&sig)]);
        Crl::parse(&raw)
    }

    /// Parses one DER list (the whole input). The signature is not checked here.
    pub fn parse(input: &[u8]) -> Result<Crl> {
        if input.len() > MAX_CRL {
            return Err(bad("crl too large"));
        }
        let top = der::read_exact(input, der::SEQUENCE)?;
        let parts = top.items(der::SEQUENCE)?;
        let (Some(tbs), Some(alg), Some(sigv), 3) = (parts.first(), parts.get(1), parts.get(2), parts.len()) else {
            return Err(bad("crl layout"));
        };
        let hash = parse_sig_alg(alg)?;
        let sig = der::bit_string_bytes(sigv)?.to_vec();
        let fields = tbs.items(der::SEQUENCE)?;
        let mut it = fields.iter().peekable();
        let (Some(version), Some(inner_alg), Some(issuer), Some(this), Some(next)) = (it.next(), it.next(), it.next(), it.next(), it.next()) else {
            return Err(bad("crl layout"));
        };
        if version.tag != der::INTEGER || version.uint()? != [1] {
            return Err(bad("only version 2"));
        }
        if inner_alg.raw != alg.raw {
            return Err(bad("signature algorithms differ"));
        }
        let (this_update, next_update) = (parse_time(this)?, parse_time(next)?);
        let mut revoked = Vec::new();
        if let Some(list) = it.next_if(|t| t.tag == der::SEQUENCE) {
            for e in list.items(der::SEQUENCE)? {
                if revoked.len() >= MAX_REVOKED {
                    return Err(bad("too many revoked certificates"));
                }
                let entry = e.items(der::SEQUENCE)?;
                let (Some(serial), Some(date)) = (entry.first(), entry.get(1)) else { return Err(bad("revoked certificate")) };
                parse_time(date)?;
                let serial = serial.uint()?;
                if serial.is_empty() || serial.len() > MAX_SERIAL {
                    return Err(bad("serial number"));
                }
                revoked.push(serial.to_vec());
            }
        }
        let mut number = 0u64;
        if let Some(ext) = it.next() {
            if ext.tag != 0xa0 || it.next().is_some() {
                return Err(bad("unsupported crl field"));
            }
            for e in der::read_exact(ext.content, der::SEQUENCE)?.items(der::SEQUENCE)? {
                let f = e.items(der::SEQUENCE)?;
                let (Some(id), Some(rest)) = (f.first(), f.get(1..)) else { return Err(bad("crl extension")) };
                let (critical, value) = match rest {
                    [v] => (false, v),
                    [c, v] => (c.boolean()?, v),
                    _ => return Err(bad("crl extension")),
                };
                if id.tag != der::OID || value.tag != der::OCTET_STRING {
                    return Err(bad("crl extension"));
                }
                if id.content == OID_CRL_NUMBER {
                    let n = der::read_exact(value.content, der::INTEGER)?.uint()?;
                    if n.len() > 8 {
                        return Err(bad("crl number"));
                    }
                    number = n.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b));
                } else if critical {
                    return Err(bad("unknown critical crl extension"));
                }
            }
        }
        Ok(Crl {
            der: input.to_vec(),
            tbs: tbs.raw.to_vec(),
            sig,
            hash,
            issuer: Name::parse(issuer)?,
            issuer_raw: issuer.raw.to_vec(),
            this_update,
            next_update,
            number,
            revoked,
        })
    }

    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// Whether the signature was made by this key.
    pub fn signed_by(&self, issuer_key: &RsaPublicKey) -> bool {
        issuer_key.verify(self.hash, &self.tbs, &self.sig)
    }

    /// Whether `signer` (a CA certificate) issued this list: its name, and its key's signature.
    pub fn issued_by(&self, signer: &Certificate) -> bool {
        self.issuer_raw == signer.subject_raw && self.signed_by(&signer.key)
    }

    /// Whether the list is current at `now`.
    pub fn current_at(&self, now: u64) -> bool {
        self.this_update <= now && now <= self.next_update
    }

    /// Whether a serial number (big-endian, leading zeros ignored) is on the list.
    pub fn is_revoked(&self, serial: &[u8]) -> bool {
        let serial = trim_zeros(serial);
        self.revoked.iter().any(|s| s.as_slice() == serial)
    }

    /// The revoked serial numbers, in the order of the list.
    pub fn revoked(&self) -> &[Vec<u8>] {
        &self.revoked
    }

    pub fn to_pem(&self) -> String {
        pem_encode("X509 CRL", &self.der)
    }

    pub fn from_pem(text: &str) -> Result<Crl> {
        Crl::parse(&pem_decode("X509 CRL", text)?)
    }
}

/// The position in `chain` (0 is the leaf) of the first certificate a list in `crls` revokes: a
/// list counts when it was issued by the certificate that signed that one, which is the next in
/// the chain or one of the `anchors`.
pub fn revoked_index(chain: &[Certificate], anchors: &[Certificate], crls: &[Crl]) -> Option<usize> {
    for (i, cert) in chain.iter().enumerate() {
        let signer = chain.get(i + 1).into_iter().chain(anchors.iter()).find(|s| s.subject_raw == cert.issuer_raw && cert.signed_by(&s.key));
        let Some(signer) = signer else { continue };
        if crls.iter().any(|c| c.issued_by(signer) && c.is_revoked(&cert.serial)) {
            return Some(i);
        }
    }
    None
}

// ---- certificate requests -------------------------------------------------------------------

/// A PKCS #10 request (RFC 2986): a name and a public key, signed with the matching private key
/// as proof of possession.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    der: Vec<u8>,
    pub subject: Name,
    pub key: RsaPublicKey,
    /// The alternative names it asks for (an extensionRequest attribute).
    pub alt: AltNames,
}

impl Request {
    pub fn create(subject: &Name, key: &RsaKey, hash: SigHash) -> Result<Request> {
        Request::create_alt(subject, key, hash, &AltNames::default())
    }

    /// A request that also asks for alternative names.
    pub fn create_alt(subject: &Name, key: &RsaKey, hash: SigHash, alt: &AltNames) -> Result<Request> {
        let attrs = if alt.is_empty() {
            der::tlv(0xa0, &[])
        } else {
            let ext_list = der::sequence(&[&alt.encode_extension()?]);
            der::tlv(0xa0, &der::sequence(&[&oid(OID_EXT_REQ), &der::set(&[&ext_list])]))
        };
        let info = der::sequence(&[&der::uint(&[]), &subject.encode()?, &encode_spki(key.public()), &attrs]);
        let sig = key.sign(hash, &info).ok_or_else(|| bad("key too small to sign"))?;
        let req = der::sequence(&[&info, &sig_alg(hash), &der::bit_string(&sig)]);
        Request::parse(&req)
    }

    /// Parses a request and checks its signature against the key it carries.
    pub fn parse(input: &[u8]) -> Result<Request> {
        if input.len() > MAX_CERT {
            return Err(bad("request too large"));
        }
        let top = der::read_exact(input, der::SEQUENCE)?.items(der::SEQUENCE)?;
        let (Some(info), Some(alg), Some(sigv), 3) = (top.first(), top.get(1), top.get(2), top.len()) else {
            return Err(bad("request layout"));
        };
        let hash = parse_sig_alg(alg)?;
        let f = info.items(der::SEQUENCE)?;
        let (Some(ver), Some(subject), Some(spki), Some(attrs), 4) = (f.first(), f.get(1), f.get(2), f.get(3), f.len()) else {
            return Err(bad("request info"));
        };
        if !ver.uint()?.is_empty() || attrs.tag != 0xa0 {
            return Err(bad("request version or attributes"));
        }
        let mut alt = AltNames::default();
        if !attrs.content.is_empty() {
            let attr = der::read_exact(attrs.content, der::SEQUENCE)?.items(der::SEQUENCE)?;
            let (Some(kind), Some(values), 2) = (attr.first(), attr.get(1), attr.len()) else { return Err(bad("request attribute")) };
            if kind.tag != der::OID || kind.content != OID_EXT_REQ {
                return Err(bad("unsupported request attribute"));
            }
            let sets = values.items(der::SET)?;
            let (Some(list), 1) = (sets.first(), sets.len()) else { return Err(bad("request attribute values")) };
            for e in list.items(der::SEQUENCE)? {
                let fields = e.items(der::SEQUENCE)?;
                let (Some(id), Some(value)) = (fields.first(), fields.last()) else { return Err(bad("request extension")) };
                if id.tag != der::OID || value.tag != der::OCTET_STRING || fields.len() > 3 {
                    return Err(bad("request extension"));
                }
                if id.content == OID_SAN {
                    alt = AltNames::parse_names(value.content)?;
                }
            }
        }
        let key = parse_spki(spki)?;
        if !key.verify(hash, info.raw, der::bit_string_bytes(sigv)?) {
            return Err(bad("request signature"));
        }
        Ok(Request { der: input.to_vec(), subject: Name::parse(subject)?, key, alt })
    }

    pub fn der(&self) -> &[u8] {
        &self.der
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::Drbg;

    fn key(seed: &str) -> RsaKey {
        RsaKey::generate(512, &mut Drbg::new(seed.as_bytes())).unwrap()
    }

    fn selfsigned(name: &Name, k: &RsaKey, ca: bool) -> Certificate {
        Certificate::issue(&Issue {
            subject: name,
            subject_key: k.public(),
            issuer: name,
            issuer_key: k,
            serial: &[1],
            not_before: 1000,
            not_after: 100_000,
            ca,
            path_len: None,
            hash: SigHash::Sha256,
        })
        .unwrap()
    }

    #[test]
    fn calendar_conversions() {
        assert_eq!(from_civil(1970, 1, 1, 0, 0, 0), Some(0));
        assert_eq!(from_civil(2000, 2, 29, 12, 0, 0), Some(951_825_600));
        assert_eq!(from_civil(2026, 11, 13, 0, 0, 0), Some(1_794_528_000));
        assert_eq!(civil(1_794_528_000), (2026, 11, 13, 0, 0, 0));
        assert_eq!(civil(MAX_TIME), (9999, 12, 31, 23, 59, 59));
        assert_eq!(from_civil(9999, 12, 31, 23, 59, 59), Some(MAX_TIME));
        for bad_date in [(1900, 1, 1), (2023, 2, 29), (2024, 13, 1), (2024, 4, 31), (2024, 0, 1), (10_000, 1, 1)] {
            assert_eq!(from_civil(bad_date.0, bad_date.1, bad_date.2, 0, 0, 0), None, "{bad_date:?}");
        }
        assert_eq!(from_civil(2024, 1, 1, 24, 0, 0), None);
        // Every day of a few years survives the round trip.
        for t in (0..4_000_000_000u64).step_by(86_400 * 7 + 3601) {
            let (y, mo, d, h, mi, s) = civil(t);
            assert_eq!(from_civil(y, mo, d, h, mi, s), Some(t));
        }
        // UTCTime before 2050, GeneralizedTime from 2050.
        assert_eq!(encode_time(1_794_528_000), der::tlv(der::UTC_TIME, b"261113000000Z"));
        let t2050 = from_civil(2050, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(encode_time(t2050)[0], der::GENERALIZED_TIME);
        for t in [0, 1_794_528_000, t2050, MAX_TIME] {
            let enc = encode_time(t);
            assert_eq!(parse_time(&der::read_exact(&enc, enc[0]).unwrap()).unwrap(), t);
        }
    }

    #[test]
    fn a_self_signed_certificate_round_trips() {
        let k = key("ca key");
        let name = Name { common_name: "Lab Root".into(), organization: "NetLab".into(), unit: "Training".into(), country: "ES".into() };
        let cert = selfsigned(&name, &k, true);
        assert!(cert.ca && cert.self_issued() && cert.signed_by(k.public()));
        assert_eq!((cert.not_before, cert.not_after, cert.subject.clone()), (1000, 100_000, name.clone()));
        assert_eq!(cert.text_check(), "CN=Lab Root, OU=Training, O=NetLab, C=ES");
        assert_eq!(&cert.key, k.public());
        // Parsing its own encoding gives the same certificate, and so does PEM.
        assert_eq!(Certificate::parse(cert.der()).unwrap(), cert);
        let pem = cert.to_pem();
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n") && pem.ends_with("-----END CERTIFICATE-----\n"));
        assert_eq!(Certificate::from_pem(&pem).unwrap(), cert);
        // Another key does not verify it.
        assert!(!cert.signed_by(key("other").public()));
        // Deterministic: the same inputs give the same bytes.
        assert_eq!(selfsigned(&name, &k, true).der(), cert.der());
    }

    impl Certificate {
        fn text_check(&self) -> String {
            self.subject.text()
        }
    }

    #[test]
    fn chains_verify_and_fail_for_the_right_reasons() {
        let (root_k, mid_k, leaf_k) = (key("root"), key("mid"), key("leaf"));
        let (root_n, mid_n, leaf_n) = (Name::new("Root", "Lab"), Name::new("Mid", "Lab"), Name::new("router", "Lab"));
        let root = selfsigned(&root_n, &root_k, true);
        let mk = |subject: &Name, skey: &RsaKey, issuer: &Name, ikey: &RsaKey, ca: bool, path_len: Option<u8>| {
            Certificate::issue(&Issue {
                subject,
                subject_key: skey.public(),
                issuer,
                issuer_key: ikey,
                serial: &[7],
                not_before: 1000,
                not_after: 50_000,
                ca,
                path_len,
                hash: SigHash::Sha256,
            })
            .unwrap()
        };
        let mid = mk(&mid_n, &mid_k, &root_n, &root_k, true, Some(0));
        let leaf = mk(&leaf_n, &leaf_k, &mid_n, &mid_k, false, None);
        let anchors = [root.clone()];
        assert_eq!(verify_chain(&[leaf.clone(), mid.clone()], &anchors, 2000), Ok(()));
        // A chain that already includes the root, and a root that is the only certificate.
        assert_eq!(verify_chain(&[leaf.clone(), mid.clone(), root.clone()], &anchors, 2000), Ok(()));
        assert_eq!(verify_chain(std::slice::from_ref(&root), &anchors, 2000), Ok(()));
        // No anchors, a missing intermediate, a leaf alone.
        assert_eq!(verify_chain(&[leaf.clone(), mid.clone()], &[], 2000), Err(ChainError::Untrusted));
        assert_eq!(verify_chain(std::slice::from_ref(&leaf), &anchors, 2000), Err(ChainError::Untrusted));
        // Time.
        assert_eq!(verify_chain(&[leaf.clone(), mid.clone()], &anchors, 999), Err(ChainError::NotYetValid(0)));
        assert_eq!(verify_chain(&[leaf.clone(), mid.clone()], &anchors, 50_001), Err(ChainError::Expired(0)));
        // A leaf cannot sign: a certificate "issued" by the leaf is refused as NotCa.
        let sub = mk(&Name::new("sub", "Lab"), &key("sub"), &leaf_n, &leaf_k, false, None);
        assert_eq!(verify_chain(&[sub, leaf.clone(), mid.clone()], &anchors, 2000), Err(ChainError::NotCa(0)));
        // A path length of 0 on the middle CA forbids a second CA under it.
        let mid2_k = key("mid2");
        let mid2 = mk(&Name::new("Mid2", "Lab"), &mid2_k, &mid_n, &mid_k, true, None);
        let leaf2 = mk(&leaf_n, &leaf_k, &Name::new("Mid2", "Lab"), &mid2_k, false, None);
        assert_eq!(verify_chain(&[leaf2, mid2, mid.clone()], &anchors, 2000), Err(ChainError::PathLength(1)));
        // Another root's anchor does not vouch for it; a stranger certificate in the middle breaks.
        let other = selfsigned(&Name::new("Other", "Lab"), &key("other root"), true);
        assert!(verify_chain(&[leaf.clone(), mid.clone()], &[other], 2000).is_err());
        assert_eq!(verify_chain(&[leaf.clone(), root.clone()], &anchors, 2000), Err(ChainError::BadSignature(0)));
        // Too long and empty.
        assert_eq!(verify_chain(&[], &anchors, 0), Err(ChainError::Empty));
        let long = vec![leaf; MAX_CHAIN + 1];
        assert_eq!(verify_chain(&long, &anchors, 2000), Err(ChainError::TooLong));
        // A signer that has itself expired fails the chain.
        let short_root = Certificate::issue(&Issue {
            subject: &root_n,
            subject_key: root_k.public(),
            issuer: &root_n,
            issuer_key: &root_k,
            serial: &[9],
            not_before: 1000,
            not_after: 60_000,
            ca: true,
            path_len: None,
            hash: SigHash::Sha256,
        })
        .unwrap();
        let late = mk(&Name::new("late", "Lab"), &key("late"), &root_n, &root_k, false, None);
        assert_eq!(verify_chain(&[late], &[short_root], 70_000), Err(ChainError::Expired(0)));
    }

    #[test]
    fn a_self_signed_certificate_is_not_trusted_unless_it_is_an_anchor() {
        let k = key("lonely");
        let c = selfsigned(&Name::new("lonely", "Lab"), &k, true);
        assert_eq!(verify_chain(std::slice::from_ref(&c), &[], 2000), Err(ChainError::Untrusted));
        assert_eq!(verify_chain(std::slice::from_ref(&c), std::slice::from_ref(&c), 2000), Ok(()));
    }

    #[test]
    fn requests_round_trip_and_check_the_signature() {
        let k = key("csr");
        let name = Name::new("r1.example", "Lab");
        let req = Request::create(&name, &k, SigHash::Sha256).unwrap();
        assert_eq!(Request::parse(req.der()).unwrap(), req);
        assert_eq!((&req.subject, &req.key), (&name, k.public()));
        // Flip a byte inside the signed part: the signature no longer holds.
        let mut tampered = req.der().to_vec();
        let at = tampered.len() / 3;
        tampered[at] ^= 0x01;
        assert!(Request::parse(&tampered).is_err());
    }

    #[test]
    fn hostile_certificates_never_panic() {
        let k = key("fuzz");
        let cert = selfsigned(&Name::new("fuzz", "Lab"), &k, true);
        let enc = cert.der().to_vec();
        // Every truncation is an error.
        for cut in 0..enc.len() {
            assert!(Certificate::parse(&enc[..cut]).is_err(), "cut {cut}");
        }
        // Every single-byte change either parses or errors; the signature then fails to hold.
        for at in 0..enc.len() {
            let mut m = enc.clone();
            m[at] ^= 0xff;
            if let Ok(c) = Certificate::parse(&m) {
                assert!(!c.signed_by(k.public()) || m == enc, "byte {at} changed unnoticed");
            }
        }
        assert!(Certificate::parse(&[]).is_err());
        assert!(Certificate::parse(&vec![0x30; 100_000]).is_err());
        assert!(Certificate::parse(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(Certificate::from_pem("").is_err());
        assert!(Certificate::from_pem("-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----\n").is_err());
        assert!(Certificate::from_pem(&format!("{}{}", cert.to_pem(), cert.to_pem())).is_err());
        assert!(Certificate::from_pem(&cert.to_pem().replace("END", "FIN")).is_err());
        // Bad names and serials are refused when issuing.
        let bad_name = Name::new(&"x".repeat(65), "Lab");
        assert!(bad_name.encode().is_err() && Name::default().encode().is_err());
        assert!(Name { country: "ESP".into(), common_name: "a".into(), ..Name::default() }.encode().is_err());
        assert!(Name::new("a\nb", "").encode().is_err());
        let n = Name::new("a", "");
        let base = Issue {
            subject: &n,
            subject_key: k.public(),
            issuer: &n,
            issuer_key: &k,
            serial: &[1],
            not_before: 5,
            not_after: 4,
            ca: true,
            path_len: None,
            hash: SigHash::Sha256,
        };
        assert!(Certificate::issue(&base).is_err());
        assert!(Certificate::issue(&Issue { not_before: 1, not_after: MAX_TIME + 1, ..base.clone() }).is_err());
        assert!(Certificate::issue(&Issue { not_before: 1, not_after: 2, serial: &[], ..base.clone() }).is_err());
        assert!(Certificate::issue(&Issue { not_before: 1, not_after: 2, serial: &[1; 21], ..base.clone() }).is_err());
        assert!(Certificate::issue(&Issue { not_before: 1, not_after: 2, ..base }).is_ok());
    }

    #[test]
    fn a_certificate_made_by_another_implementation_parses_and_verifies() {
        // A 1024-bit RSA self-signed certificate (CN=interop test, O=NetLab Test) made with a
        // command-line tool for this test; only the public certificate is kept here.
        let pem = concat!(
            "-----BEGIN CERTIFICATE-----\n",
            "MIICFTCCAX6gAwIBAgIUBawzqGiCsJZv8biuZ7CoXjdRvW8wDQYJKoZIhvcNAQEL\n",
            "BQAwLTEVMBMGA1UEAwwMaW50ZXJvcCB0ZXN0MRQwEgYDVQQKDAtOZXRMYWIgVGVz\n",
            "dDAeFw0yNjExMTMxNTU2NDFaFw0zNjExMTAxNTU2NDFaMC0xFTATBgNVBAMMDGlu\n",
            "dGVyb3AgdGVzdDEUMBIGA1UECgwLTmV0TGFiIFRlc3QwgZ8wDQYJKoZIhvcNAQEB\n",
            "BQADgY0AMIGJAoGBAL2R3r0AXcGcJwi/shXQnLDbd4tLwlolsdQGybdRjc9hJqmz\n",
            "tHqWBfD3wuXGOS6BL0Zl7D42EGB1lYCKa6i/a0jbaU1fV4ha5cqGfhSw7pUPRMRD\n",
            "s0K/PcTHfgzqPkA8a3FTODQyX2oBVuyq8y/yqewqabPwwNQQJ0XKeyivmKidAgMB\n",
            "AAGjMjAwMB0GA1UdDgQWBBR3aMdZdHKFnZaPPg/PuqM8UacdHzAPBgNVHRMBAf8E\n",
            "BTADAQH/MA0GCSqGSIb3DQEBCwUAA4GBALs40b1dfNtc+d8NQhzqorSvSbglekUZ\n",
            "FIsKQBFbE1izDU9qerKyATW5/kx4pQvcbNiFKn+7F/zNEafH9w4RXDz0DjyHOcnZ\n",
            "oELirw0monaTJD7KlaWlKgnP2Coa/cwegdXAZKsRy5zPU4m7YjsNJYQXr9ckkOWj\n",
            "rYyXp3b3frDr\n",
            "-----END CERTIFICATE-----\n",
        );
        let c = Certificate::from_pem(pem).unwrap();
        assert_eq!(c.subject.text(), "CN=interop test, O=NetLab Test");
        assert!(c.self_issued() && c.ca && c.signed_by(&c.key));
        assert_eq!(c.key.bits(), 1024);
        assert_eq!(verify_chain(std::slice::from_ref(&c), std::slice::from_ref(&c), c.not_before + 1), Ok(()));
        assert_eq!(verify_chain(std::slice::from_ref(&c), std::slice::from_ref(&c), c.not_after + 1), Err(ChainError::Expired(0)));
    }

    fn alt_names() -> AltNames {
        AltNames { dns: vec!["r1.lab.test".into(), "*.edge.lab.test".into()], ips: vec![[10, 0, 0, 1]] }
    }

    #[test]
    fn alternative_names_round_trip_and_match_hosts() {
        let ca_key = key("alt ca");
        let ca_name = Name::new("Lab Root", "NetLab");
        let leaf_key = key("alt leaf");
        let subject = Name::new("r1", "NetLab");
        let cert = Certificate::issue_alt(
            &Issue {
                subject: &subject,
                subject_key: leaf_key.public(),
                issuer: &ca_name,
                issuer_key: &ca_key,
                serial: &[7],
                not_before: 1000,
                not_after: 100_000,
                ca: false,
                path_len: None,
                hash: SigHash::Sha256,
            },
            &alt_names(),
        )
        .unwrap();
        assert_eq!(cert.alt, alt_names());
        assert_eq!(Certificate::parse(cert.der()).unwrap().alt, alt_names());
        assert_eq!(Certificate::from_pem(&cert.to_pem()).unwrap(), cert);
        // Addresses and names match their own kind; the common name is not consulted when the
        // certificate has entries of the kind.
        assert!(cert.matches_host("10.0.0.1") && !cert.matches_host("10.0.0.2"));
        assert!(cert.matches_host("R1.lab.test") && cert.matches_host("r1.lab.test.") && !cert.matches_host("r2.lab.test"));
        assert!(cert.matches_host("a.edge.lab.test") && !cert.matches_host("a.b.edge.lab.test") && !cert.matches_host("edge.lab.test"));
        assert!(!cert.matches_host("r1"));
        // Without entries the common name stands in.
        let plain = selfsigned(&Name::new("host.lab.test", ""), &key("plain"), false);
        assert!(plain.alt.is_empty() && plain.matches_host("HOST.lab.test") && !plain.matches_host("other.lab.test"));
        let by_ip = selfsigned(&Name::new("192.0.2.9", ""), &key("by ip"), false);
        assert!(by_ip.matches_host("192.0.2.9") && !by_ip.matches_host("192.0.2.10"));
        // Too many or malformed names are refused when issuing.
        let many = AltNames { dns: (0..=MAX_ALT).map(|i| format!("h{i}.test")).collect(), ips: Vec::new() };
        let bad_dns = AltNames { dns: vec!["has space".into()], ips: Vec::new() };
        for alt in [&many, &bad_dns] {
            let e = Certificate::issue_alt(
                &Issue {
                    subject: &subject,
                    subject_key: leaf_key.public(),
                    issuer: &ca_name,
                    issuer_key: &ca_key,
                    serial: &[7],
                    not_before: 1,
                    not_after: 2,
                    ca: false,
                    path_len: None,
                    hash: SigHash::Sha256,
                },
                alt,
            );
            assert!(e.is_err());
        }
        // Every truncation of the certificate is refused without a panic.
        for n in 0..cert.der().len() {
            assert!(Certificate::parse(&cert.der()[..n]).is_err(), "{n}");
        }
    }

    #[test]
    fn a_request_can_ask_for_alternative_names() {
        let k = key("req alt");
        let subject = Name::new("r1", "NetLab");
        let req = Request::create_alt(&subject, &k, SigHash::Sha256, &alt_names()).unwrap();
        assert_eq!(req.alt, alt_names());
        let back = Request::parse(req.der()).unwrap();
        assert_eq!((back.subject.clone(), back.alt.clone(), back.key.clone()), (subject.clone(), alt_names(), k.public().clone()));
        // A request with none still parses as before.
        assert!(Request::create(&subject, &k, SigHash::Sha256).unwrap().alt.is_empty());
        // A flipped byte anywhere is refused or changes nothing it should not (signature).
        for n in 0..req.der().len() {
            let mut v = req.der().to_vec();
            v[n] ^= 0x01;
            if let Ok(r) = Request::parse(&v) {
                assert_ne!(r.der(), req.der());
            }
        }
        for n in 0..req.der().len() {
            assert!(Request::parse(&req.der()[..n]).is_err(), "{n}");
        }
    }

    #[test]
    fn revocation_lists_sign_parse_and_revoke() {
        let ca_key = key("crl ca");
        let ca_name = Name::new("Lab Root", "NetLab");
        let ca = selfsigned(&ca_name, &ca_key, true);
        let crl = Crl::issue(&ca_name, &ca_key, 3, 5000, 9000, &[vec![0, 9], vec![0x01, 0x02, 0x03]], SigHash::Sha256).unwrap();
        assert_eq!((crl.number, crl.this_update, crl.next_update), (3, 5000, 9000));
        assert_eq!(crl.revoked(), &[vec![9], vec![1, 2, 3]]);
        assert!(crl.is_revoked(&[9]) && crl.is_revoked(&[0, 0, 9]) && crl.is_revoked(&[1, 2, 3]) && !crl.is_revoked(&[1, 2]));
        assert!(crl.signed_by(&ca.key) && crl.issued_by(&ca));
        assert!(!crl.signed_by(key("other").public()));
        assert!(crl.current_at(5000) && crl.current_at(9000) && !crl.current_at(4999) && !crl.current_at(9001));
        assert_eq!(Crl::from_pem(&crl.to_pem()).unwrap(), crl);
        assert!(crl.to_pem().starts_with("-----BEGIN X509 CRL-----"));
        // An empty list has no revokedCertificates field at all.
        let empty = Crl::issue(&ca_name, &ca_key, 0, 1, 2, &[], SigHash::Sha256).unwrap();
        assert!(empty.revoked().is_empty() && !empty.is_revoked(&[1]));
        assert_eq!(Crl::parse(empty.der()).unwrap(), empty);
        // Bad input when issuing.
        assert!(Crl::issue(&ca_name, &ca_key, 1, 9, 8, &[], SigHash::Sha256).is_err());
        assert!(Crl::issue(&ca_name, &ca_key, 1, 1, 2, &[vec![]], SigHash::Sha256).is_err());
        assert!(Crl::issue(&ca_name, &ca_key, 1, 1, 2, &[vec![1; 21]], SigHash::Sha256).is_err());
        let too_many: Vec<Vec<u8>> = (0..=MAX_REVOKED).map(|_| vec![1]).collect();
        assert!(Crl::issue(&ca_name, &ca_key, 1, 1, 2, &too_many, SigHash::Sha256).is_err());
        // Every truncation is refused, and a flipped byte never leaves a list that the CA's key
        // still signed.
        for n in 0..crl.der().len() {
            assert!(Crl::parse(&crl.der()[..n]).is_err(), "{n}");
        }
        for n in 0..crl.der().len() {
            let mut v = crl.der().to_vec();
            v[n] ^= 0x01;
            if let Ok(c) = Crl::parse(&v) {
                assert!(!c.issued_by(&ca), "{n}");
            }
        }
    }

    #[test]
    fn chains_are_checked_against_the_revocation_lists_of_their_signers() {
        let (ca_key, other_key, leaf_key) = (key("rv ca"), key("rv other"), key("rv leaf"));
        let ca_name = Name::new("Lab Root", "NetLab");
        let ca = selfsigned(&ca_name, &ca_key, true);
        let leaf_name = Name::new("leaf", "NetLab");
        let leaf = Certificate::issue(&Issue {
            subject: &leaf_name,
            subject_key: leaf_key.public(),
            issuer: &ca_name,
            issuer_key: &ca_key,
            serial: &[0x42],
            not_before: 1000,
            not_after: 100_000,
            ca: false,
            path_len: None,
            hash: SigHash::Sha256,
        })
        .unwrap();
        let chain = std::slice::from_ref(&leaf);
        let anchors = std::slice::from_ref(&ca);
        let revoking = Crl::issue(&ca_name, &ca_key, 1, 1000, 2000, &[vec![0x42]], SigHash::Sha256).unwrap();
        let other_serial = Crl::issue(&ca_name, &ca_key, 1, 1000, 2000, &[vec![0x43]], SigHash::Sha256).unwrap();
        // The same name signed by another key is not the issuer's list.
        let forged = Crl::issue(&ca_name, &other_key, 1, 1000, 2000, &[vec![0x42]], SigHash::Sha256).unwrap();
        assert_eq!(revoked_index(chain, anchors, std::slice::from_ref(&revoking)), Some(0));
        assert_eq!(revoked_index(chain, anchors, &[other_serial, forged]), None);
        assert_eq!(revoked_index(chain, anchors, &[]), None);
        // A chain whose first certificate is the leaf and whose second is the CA: only the leaf
        // is on the list.
        let both = [leaf.clone(), ca.clone()];
        assert_eq!(revoked_index(&both, anchors, std::slice::from_ref(&revoking)), Some(0));
    }
}
