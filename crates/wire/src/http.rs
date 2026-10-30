//! HTTP/1.1 messages (RFC 9112): requests and responses with headers and a `Content-Length`
//! body, as RESTCONF (RFC 8040) carries them; Basic authentication's base64 (RFC 7617,
//! RFC 4648) and percent-encoding (RFC 3986 §2.1).
//!
//! Parsing is incremental: [`Request::parse`] answers `Ok(None)` while the bytes so far are a
//! complete-looking prefix, and an error once they can never become a valid message. Sizes are
//! capped ([`MAX_HEAD`], [`MAX_BODY`]); chunked transfer coding is not accepted.

use crate::{Result, WireError};

/// The well-known ports: plain HTTP and HTTP over TLS (the simulator carries no TLS: see
/// `docs/architecture.md`, port 443 carries the same plain messages).
pub const PORT: u16 = 80;
pub const TLS_PORT: u16 = 443;
/// Most bytes of a request or status line plus headers.
pub const MAX_HEAD: usize = 8 * 1024;
/// Most bytes of a body.
pub const MAX_BODY: usize = 64 * 1024;
/// Most header fields.
pub const MAX_HEADERS: usize = 64;

const WHAT: &str = "HTTP";

fn bad(why: impl Into<String>) -> WireError {
    WireError::Unsupported(WHAT, why.into())
}

/// A request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The request target as sent (`/restconf/data/…?depth=1`).
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// A response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Splits head and body; `None` until the blank line arrived.
fn split_head(buf: &[u8]) -> Result<Option<(&str, &[u8])>> {
    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        if buf.len() > MAX_HEAD {
            return Err(bad("headers too long"));
        }
        return Ok(None);
    };
    if end > MAX_HEAD {
        return Err(bad("headers too long"));
    }
    let head = std::str::from_utf8(buf.get(..end).unwrap_or_default()).map_err(|_| bad("headers are not text"))?;
    Ok(Some((head, buf.get(end + 4..).unwrap_or_default())))
}

fn parse_headers<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for l in lines {
        let (n, v) = l.split_once(':').ok_or_else(|| bad("a header line without a colon"))?;
        if n.is_empty() || n.contains(char::is_whitespace) {
            return Err(bad("a malformed header name"));
        }
        if out.len() >= MAX_HEADERS {
            return Err(bad("too many headers"));
        }
        out.push((n.to_string(), v.trim().to_string()));
    }
    Ok(out)
}

/// The body length the headers announce; chunked coding is refused.
fn body_len(headers: &[(String, String)]) -> Result<usize> {
    if header(headers, "transfer-encoding").is_some() {
        return Err(bad("transfer codings are not supported; send Content-Length"));
    }
    match header(headers, "content-length") {
        None => Ok(0),
        Some(v) => {
            let n: usize = v.parse().map_err(|_| bad("Content-Length is not a number"))?;
            if n > MAX_BODY {
                return Err(WireError::Unsupported(WHAT, format!("body larger than {MAX_BODY} bytes")));
            }
            Ok(n)
        }
    }
}

/// Whether an error is about size (a server answers 413 instead of 400).
pub fn too_large(e: &WireError) -> bool {
    matches!(e, WireError::Unsupported(_, why) if why.contains("larger than") || why.contains("too long"))
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    /// The path without its query.
    pub fn path(&self) -> &str {
        self.target.split_once('?').map_or(self.target.as_str(), |(p, _)| p)
    }

    /// The query's `name=value` pairs, percent-decoded.
    pub fn query(&self) -> Vec<(String, String)> {
        let Some((_, q)) = self.target.split_once('?') else { return Vec::new() };
        q.split('&')
            .filter(|p| !p.is_empty())
            .take(32)
            .map(|p| {
                let (n, v) = p.split_once('=').unwrap_or((p, ""));
                (percent_decode(n).unwrap_or_default(), percent_decode(v).unwrap_or_default())
            })
            .collect()
    }

    /// Parses a request from the bytes received so far: `Ok(None)` if more are needed.
    pub fn parse(buf: &[u8]) -> Result<Option<(Request, usize)>> {
        let Some((head, rest)) = split_head(buf)? else { return Ok(None) };
        let mut lines = head.split("\r\n");
        let first = lines.next().unwrap_or("");
        let mut words = first.split(' ');
        let (method, target, version) = (words.next().unwrap_or(""), words.next().unwrap_or(""), words.next().unwrap_or(""));
        if words.next().is_some() || method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) || !target.starts_with('/') {
            return Err(bad("a malformed request line"));
        }
        if !version.starts_with("HTTP/1.") {
            return Err(bad(format!("version `{}`", version.chars().take(16).collect::<String>())));
        }
        let headers = parse_headers(lines)?;
        let n = body_len(&headers)?;
        let Some(body) = rest.get(..n) else { return Ok(None) };
        let used = buf.len() - rest.len() + n;
        Ok(Some((Request { method: method.to_string(), target: target.to_string(), headers, body: body.to_vec() }, used)))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = format!("{} {} HTTP/1.1\r\n", self.method, self.target);
        for (n, v) in &self.headers {
            if !n.eq_ignore_ascii_case("content-length") {
                out.push_str(&format!("{n}: {v}\r\n"));
            }
        }
        if !self.body.is_empty() || matches!(self.method.as_str(), "POST" | "PUT" | "PATCH") {
            out.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        }
        out.push_str("\r\n");
        let mut b = out.into_bytes();
        b.extend_from_slice(&self.body);
        b
    }
}

/// The usual reason phrase of a status code.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "",
    }
}

impl Response {
    /// A response with this status, the usual reason and no headers.
    pub fn new(status: u16) -> Response {
        Response { status, reason: reason(status).to_string(), headers: Vec::new(), body: Vec::new() }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    /// Parses a response; `Ok(None)` if more bytes are needed. Without `Content-Length` the
    /// body runs to the end of what arrived (the connection closes after it).
    pub fn parse(buf: &[u8]) -> Result<Option<Response>> {
        let Some((head, rest)) = split_head(buf)? else { return Ok(None) };
        let mut lines = head.split("\r\n");
        let first = lines.next().unwrap_or("");
        let mut words = first.splitn(3, ' ');
        let version = words.next().unwrap_or("");
        let status: u16 =
            words.next().and_then(|s| s.parse().ok()).filter(|s| (100..600).contains(s)).ok_or_else(|| bad("a malformed status line"))?;
        if !version.starts_with("HTTP/1.") {
            return Err(bad("a malformed status line"));
        }
        let reason = words.next().unwrap_or("").to_string();
        let headers = parse_headers(lines)?;
        let body = match header(&headers, "content-length") {
            Some(_) => {
                let n = body_len(&headers)?;
                match rest.get(..n) {
                    Some(b) => b.to_vec(),
                    None => return Ok(None),
                }
            }
            None => rest.get(..rest.len().min(MAX_BODY)).unwrap_or_default().to_vec(),
        };
        Ok(Some(Response { status, reason, headers, body }))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {} {}\r\n", self.status, self.reason);
        for (n, v) in &self.headers {
            if !n.eq_ignore_ascii_case("content-length") {
                out.push_str(&format!("{n}: {v}\r\n"));
            }
        }
        out.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));
        let mut b = out.into_bytes();
        b.extend_from_slice(&self.body);
        b
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 (RFC 4648 §4), with padding.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= c.len() {
                out.push(char::from(B64.get(((n >> (18 - 6 * i)) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes base64; `None` for anything that isn't.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let t = text.trim().trim_end_matches('=');
    if t.len() > 4 * MAX_HEAD {
        return None;
    }
    let mut out = Vec::with_capacity(t.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in t.bytes() {
        let v = B64.iter().position(|x| *x == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// The user and password of an `Authorization: Basic …` header.
pub fn basic_credentials(value: &str) -> Option<(String, String)> {
    let (scheme, token) = value.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let text = String::from_utf8(base64_decode(token)?).ok()?;
    let (u, p) = text.split_once(':')?;
    Some((u.to_string(), p.to_string()))
}

/// Decodes `%XX` escapes; `None` for a broken escape or bytes that aren't UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Escapes everything but unreserved characters (RFC 3986 §2.3).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_and_arrive_in_pieces() {
        let r = Request {
            method: "PUT".into(),
            target: "/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0".into(),
            headers: vec![("Content-Type".into(), "application/yang-data+json".into())],
            body: br#"{"a":1}"#.to_vec(),
        };
        let bytes = r.encode();
        for cut in 0..bytes.len() {
            assert_eq!(Request::parse(&bytes[..cut]).unwrap(), None, "cut at {cut}");
        }
        let (mut back, used) = Request::parse(&bytes).unwrap().unwrap();
        assert_eq!(back.header("content-length"), Some("7"));
        back.headers.retain(|(n, _)| n != "Content-Length");
        assert_eq!((back, used), (r, bytes.len()));
    }

    #[test]
    fn responses_round_trip() {
        let mut r = Response::new(201);
        r.headers.push(("Location".into(), "/x".into()));
        r.body = b"hello".to_vec();
        let bytes = r.encode();
        assert!(String::from_utf8_lossy(&bytes).starts_with("HTTP/1.1 201 Created\r\n"));
        let mut back = Response::parse(&bytes).unwrap().unwrap();
        back.headers.retain(|(n, _)| n != "Content-Length");
        assert_eq!(back, r);
        for cut in 0..bytes.len() {
            let _ = Response::parse(&bytes[..cut]);
        }
    }

    #[test]
    fn hostile_requests_are_refused_not_trusted() {
        for junk in [
            &b"GET\r\n\r\n"[..],
            b"get / HTTP/1.1\r\n\r\n",
            b"GET / SPDY/3\r\n\r\n",
            b"GET / HTTP/1.1\r\nno colon\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"\xff\xfe / HTTP/1.1\r\n\r\n",
        ] {
            assert!(Request::parse(junk).is_err(), "{}", String::from_utf8_lossy(junk));
        }
        let big = vec![b'a'; MAX_HEAD + 10];
        assert!(Request::parse(&big).is_err_and(|e| too_large(&e)));
        let huge = format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        assert!(Request::parse(huge.as_bytes()).is_err_and(|e| too_large(&e)));
        let many: String = (0..MAX_HEADERS + 1).map(|i| format!("X-{i}: y\r\n")).collect();
        assert!(Request::parse(format!("GET / HTTP/1.1\r\n{many}\r\n").as_bytes()).is_err());
    }

    #[test]
    fn base64_and_basic_auth() {
        // RFC 4648 §10 test vectors.
        for (plain, enc) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64_encode(plain.as_bytes()), enc);
            assert_eq!(base64_decode(enc).unwrap(), plain.as_bytes());
        }
        assert_eq!(basic_credentials(&format!("Basic {}", base64_encode(b"admin:cisco:123"))), Some(("admin".into(), "cisco:123".into())));
        assert_eq!(basic_credentials("Bearer abc"), None);
        assert_eq!(basic_credentials("Basic !!!"), None);
        assert_eq!(base64_decode("@@"), None);
    }

    #[test]
    fn percent_coding() {
        assert_eq!(percent_decode("GigabitEthernet0%2F0").unwrap(), "GigabitEthernet0/0");
        assert_eq!(percent_encode("10.0.0.0/8"), "10.0.0.0%2F8");
        assert_eq!(percent_decode("%"), None);
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%ff"), None);
        let r = Request { method: "GET".into(), target: "/a?content=config&depth=2".into(), headers: vec![], body: vec![] };
        assert_eq!(r.path(), "/a");
        assert_eq!(r.query(), vec![("content".into(), "config".into()), ("depth".into(), "2".into())]);
    }
}
