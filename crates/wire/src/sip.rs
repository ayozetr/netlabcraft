//! SIP (RFC 3261) messages as IP phones and a router's call control exchange them: the subset
//! of methods (REGISTER, INVITE, ACK, BYE) and responses (100, 180, 200, 4xx) a basic call
//! needs, with an SDP body (RFC 4566) for the voice address. Text on the wire, UDP port 5060.

/// The SIP port.
pub const PORT: u16 = 5060;
/// The longest message read; the rest is ignored.
const MAX_LEN: usize = 4_096;
/// Most header lines kept.
const MAX_HEADERS: usize = 32;

/// A request or a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// `REGISTER sip:10.0.0.1 SIP/2.0` or `SIP/2.0 200 OK`.
    pub start: Start,
    /// Header lines in order (name as written, value).
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Request { method: String, uri: String },
    Response { code: u16, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SipError {
    #[error("not text")]
    NotText,
    #[error("no start line")]
    NoStart,
    #[error("bad start line")]
    BadStart,
}

impl Message {
    pub fn request(method: &str, uri: &str) -> Message {
        Message { start: Start::Request { method: method.into(), uri: uri.into() }, headers: Vec::new(), body: String::new() }
    }

    pub fn response(code: u16, reason: &str) -> Message {
        Message { start: Start::Response { code, reason: reason.into() }, headers: Vec::new(), body: String::new() }
    }

    /// Adds a header line.
    pub fn with(mut self, name: &str, value: impl Into<String>) -> Message {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The first header of this name (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn method(&self) -> Option<&str> {
        match &self.start {
            Start::Request { method, .. } => Some(method),
            Start::Response { .. } => None,
        }
    }

    pub fn code(&self) -> Option<u16> {
        match &self.start {
            Start::Response { code, .. } => Some(*code),
            Start::Request { .. } => None,
        }
    }

    /// `CSeq: 1 INVITE` → `INVITE`.
    pub fn cseq_method(&self) -> Option<&str> {
        self.header("CSeq").and_then(|v| v.split_whitespace().nth(1))
    }

    /// The user part of a header's SIP URI: `<sip:2001@10.0.0.1>;tag=x` → `2001`.
    pub fn user_of(&self, header: &str) -> Option<String> {
        uri_user(self.header(header)?)
    }

    /// The SDP body's connection address (`c=IN IP4 a.b.c.d`).
    pub fn sdp_address(&self) -> Option<std::net::Ipv4Addr> {
        self.body.lines().find_map(|l| l.trim().strip_prefix("c=IN IP4 ")).and_then(|a| a.trim().parse().ok())
    }

    /// The message as it goes on the wire (CRLF line ends, Content-Length filled in).
    pub fn encode(&self) -> Vec<u8> {
        let mut s = match &self.start {
            Start::Request { method, uri } => format!("{method} {uri} SIP/2.0\r\n"),
            Start::Response { code, reason } => format!("SIP/2.0 {code} {reason}\r\n"),
        };
        for (n, v) in self.headers.iter().filter(|(n, _)| !n.eq_ignore_ascii_case("Content-Length")) {
            s.push_str(&format!("{n}: {v}\r\n"));
        }
        s.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));
        s.push_str(&self.body);
        s.into_bytes()
    }

    pub fn decode(b: &[u8]) -> Result<Message, SipError> {
        let b = b.get(..b.len().min(MAX_LEN)).unwrap_or_default();
        let text = std::str::from_utf8(b).map_err(|_| SipError::NotText)?;
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text, ""));
        let mut lines = head.split("\r\n");
        let first = lines.next().filter(|l| !l.trim().is_empty()).ok_or(SipError::NoStart)?;
        let start = if let Some(rest) = first.strip_prefix("SIP/2.0 ") {
            let (code, reason) = rest.split_once(' ').unwrap_or((rest, ""));
            let code: u16 = code.parse().map_err(|_| SipError::BadStart)?;
            if !(100..700).contains(&code) {
                return Err(SipError::BadStart);
            }
            Start::Response { code, reason: reason.chars().take(64).collect() }
        } else {
            let mut w = first.split(' ');
            let (Some(method), Some(uri), Some("SIP/2.0"), None) = (w.next(), w.next(), w.next(), w.next()) else { return Err(SipError::BadStart) };
            if method.is_empty() || !method.bytes().all(|c| c.is_ascii_uppercase()) {
                return Err(SipError::BadStart);
            }
            Start::Request { method: method.into(), uri: uri.chars().take(128).collect() }
        };
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .take(MAX_HEADERS)
            .map(|(n, v)| (n.trim().chars().take(32).collect(), v.trim().chars().take(256).collect()))
            .collect();
        Ok(Message { start, headers, body: body.chars().take(1_024).collect() })
    }

    /// One line for the event list: `SIP INVITE sip:2002@10.0.0.1` or `SIP 180 Ringing`.
    pub fn summary(&self) -> String {
        match &self.start {
            Start::Request { method, uri } => format!("SIP {method} {uri}"),
            Start::Response { code, reason } => format!("SIP {code} {reason} ({})", self.cseq_method().unwrap_or("?")),
        }
    }
}

/// `<sip:2001@10.0.0.1>;tag=x`, `sip:2001@host` → `2001`.
pub fn uri_user(v: &str) -> Option<String> {
    let at = v.find("sip:")?;
    let rest = v.get(at + 4..)?;
    let user = rest.split(['@', '>', ';']).next()?;
    (!user.is_empty() && rest.contains('@')).then(|| user.chars().take(32).collect())
}

/// An SDP offer or answer: voice (G.711 µ-law) to `addr` port `port`.
pub fn sdp(addr: std::net::Ipv4Addr, port: u16, session: u32) -> String {
    format!("v=0\r\no=- {session} 1 IN IP4 {addr}\r\ns=call\r\nc=IN IP4 {addr}\r\nt=0 0\r\nm=audio {port} RTP/AVP 0\r\na=rtpmap:0 PCMU/8000\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_and_every_truncation_is_safe() {
        let m = Message::request("INVITE", "sip:2002@10.0.0.1")
            .with("Via", "SIP/2.0/UDP 10.0.0.5:5060;branch=z9hG4bK1")
            .with("From", "<sip:2001@10.0.0.1>;tag=a1")
            .with("To", "<sip:2002@10.0.0.1>")
            .with("Call-ID", "7@10.0.0.5")
            .with("CSeq", "1 INVITE")
            .with("Content-Type", "application/sdp");
        let m = Message { body: sdp("10.0.0.5".parse().unwrap(), 16384, 7), ..m };
        let wire = m.encode();
        let text = String::from_utf8(wire.clone()).unwrap();
        assert!(text.starts_with("INVITE sip:2002@10.0.0.1 SIP/2.0\r\nVia: SIP/2.0/UDP"), "{text}");
        assert!(text.contains(&format!("Content-Length: {}\r\n\r\nv=0", m.body.len())));
        let back = Message::decode(&wire).unwrap();
        assert_eq!(back.method(), Some("INVITE"));
        assert_eq!(back.user_of("from").as_deref(), Some("2001"));
        assert_eq!(back.sdp_address(), Some("10.0.0.5".parse().unwrap()));
        assert_eq!(back.cseq_method(), Some("INVITE"));
        let ok = Message::response(180, "Ringing").with("CSeq", "1 INVITE");
        let back = Message::decode(&ok.encode()).unwrap();
        assert_eq!((back.code(), back.summary().as_str()), (Some(180), "SIP 180 Ringing (INVITE)"));
        for n in 0..wire.len() {
            let _ = Message::decode(&wire[..n]);
        }
        for bad in [&b""[..], b"\xff\xfe", b"HELLO", b"SIP/2.0 99 x", b"invite sip:x SIP/2.0", b"INVITE sip:x HTTP/1.1"] {
            assert!(Message::decode(bad).is_err(), "{bad:?}");
        }
        assert_eq!(uri_user("sip:host"), None);
        assert_eq!(uri_user("<sip:@h>"), None);
    }
}
