//! NETCONF message framing (RFC 6242 §4): the end-of-message marker `]]>]]>` of base:1.0, used
//! for the `<hello>` exchange, and the chunked framing of base:1.1 (`\n#LEN\n…\n##\n`).
//!
//! NETCONF runs as the `netconf` subsystem of an SSH session on port 830 (RFC 6242 §3); the
//! framed messages are that channel's data (see [`crate::ssh`]).

use crate::{Result, WireError};

pub const PORT: u16 = 830;
/// The base:1.0 end-of-message marker.
pub const EOM: &str = "]]>]]>";
/// Most bytes in one message (and in one chunk).
pub const MAX_MESSAGE: usize = 256 * 1024;
/// RFC 6242 §4.2: a chunk size is at most 4294967295; here the message cap bounds it.
const MAX_CHUNK_DIGITS: usize = 10;

pub const BASE_10: &str = "urn:ietf:params:netconf:base:1.0";
pub const BASE_11: &str = "urn:ietf:params:netconf:base:1.1";
pub const NS: &str = "urn:ietf:params:xml:ns:netconf:base:1.0";

const WHAT: &str = "NETCONF framing";

fn bad(why: impl Into<String>) -> WireError {
    WireError::Unsupported(WHAT, why.into())
}

/// Which framing a message uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// base:1.0, `]]>]]>` after each message.
    EndOfMessage,
    /// base:1.1, chunked.
    Chunked,
}

/// Frames one message.
pub fn encode(msg: &str, framing: Framing) -> Vec<u8> {
    match framing {
        Framing::EndOfMessage => format!("{msg}{EOM}").into_bytes(),
        Framing::Chunked => {
            let mut out = Vec::with_capacity(msg.len() + 16);
            if !msg.is_empty() {
                out.extend_from_slice(format!("\n#{}\n", msg.len()).as_bytes());
                out.extend_from_slice(msg.as_bytes());
            }
            out.extend_from_slice(b"\n##\n");
            out
        }
    }
}

/// Takes the first complete message off the front of `buf`: `Ok(None)` if it hasn't all
/// arrived, else the message and how many bytes it used. A framing error ends the session
/// (RFC 6242 §4.2).
pub fn decode(buf: &[u8], framing: Framing) -> Result<Option<(String, usize)>> {
    match framing {
        Framing::EndOfMessage => {
            let Some(at) = buf.windows(EOM.len()).position(|w| w == EOM.as_bytes()) else {
                if buf.len() > MAX_MESSAGE {
                    return Err(bad(format!("a message longer than {MAX_MESSAGE} bytes")));
                }
                return Ok(None);
            };
            let msg = std::str::from_utf8(buf.get(..at).unwrap_or_default()).map_err(|_| bad("a message that is not UTF-8"))?;
            Ok(Some((msg.to_string(), at + EOM.len())))
        }
        Framing::Chunked => {
            let mut msg: Vec<u8> = Vec::new();
            let mut i = 0usize;
            loop {
                // "\n#" then either "#\n" (end of chunks) or digits and "\n".
                let Some(head) = buf.get(i..i + 2) else { return Ok(None) };
                if head != b"\n#" {
                    return Err(bad("a chunk does not start with LF #"));
                }
                i += 2;
                let Some(&next) = buf.get(i) else { return Ok(None) };
                if next == b'#' {
                    let Some(&lf) = buf.get(i + 1) else { return Ok(None) };
                    if lf != b'\n' {
                        return Err(bad("the end-of-chunks marker is not LF ## LF"));
                    }
                    if msg.is_empty() {
                        return Err(bad("a message with no chunks"));
                    }
                    let text = String::from_utf8(msg).map_err(|_| bad("a message that is not UTF-8"))?;
                    return Ok(Some((text, i + 2)));
                }
                let digits: Vec<u8> =
                    buf.get(i..).unwrap_or_default().iter().take_while(|b| b.is_ascii_digit()).take(MAX_CHUNK_DIGITS + 1).copied().collect();
                if digits.len() > MAX_CHUNK_DIGITS || digits.first() == Some(&b'0') {
                    return Err(bad("a bad chunk size"));
                }
                let Some(&lf) = buf.get(i + digits.len()) else { return Ok(None) };
                if digits.is_empty() || lf != b'\n' {
                    return Err(bad("a bad chunk size"));
                }
                let n: usize = std::str::from_utf8(&digits).ok().and_then(|d| d.parse().ok()).ok_or_else(|| bad("a bad chunk size"))?;
                if n == 0 || msg.len().saturating_add(n) > MAX_MESSAGE {
                    return Err(bad(format!("a message longer than {MAX_MESSAGE} bytes")));
                }
                i += digits.len() + 1;
                let Some(data) = buf.get(i..i + n) else { return Ok(None) };
                msg.extend_from_slice(data);
                i += n;
            }
        }
    }
}

/// A short description of a segment's NETCONF content, for the event list (passwords hidden).
pub fn describe(payload: &[u8]) -> String {
    let text = String::from_utf8_lossy(payload);
    let start = if text.starts_with("SSH-2.0-") { "session start (SSH identification, user authentication), then " } else { "" };
    let tags = ["hello", "rpc-reply", "rpc", "get-config", "edit-config", "close-session", "rpc-error", "ok"];
    let found: Vec<&str> = tags
        .iter()
        .filter(|t| text.contains(&format!("<{t}")) || text.contains(&format!(":{t}")))
        .copied()
        .filter(|t| *t != "rpc" || !text.contains("<rpc-reply"))
        .take(3)
        .collect();
    if found.is_empty() {
        format!("{start}data, {} bytes", payload.len()).replace(", then data", "")
    } else {
        format!("{start}<{}>, {} bytes", found.join("> <"), payload.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_framings_round_trip_and_wait_for_more() {
        for f in [Framing::EndOfMessage, Framing::Chunked] {
            let msg = "<rpc message-id=\"1\"><get-config><source><running/></source></get-config></rpc>";
            let mut bytes = encode(msg, f);
            let used = bytes.len();
            bytes.extend_from_slice(&encode("<rpc/>", f));
            for cut in 0..used {
                assert_eq!(decode(&bytes[..cut], f).unwrap(), None, "{f:?} cut {cut}");
            }
            assert_eq!(decode(&bytes, f).unwrap(), Some((msg.to_string(), used)));
            assert_eq!(decode(&bytes[used..], f).unwrap().unwrap().0, "<rpc/>");
        }
    }

    #[test]
    fn several_chunks_make_one_message() {
        // RFC 6242 §4.2's example splits a message into chunks of 4 and 18 bytes.
        let bytes = b"\n#4\n<rpc\n#18\n message-id=\"102\"\n\n##\n";
        assert_eq!(decode(bytes, Framing::Chunked).unwrap().unwrap().0, "<rpc message-id=\"102\"\n");
    }

    #[test]
    fn bad_framing_is_an_error() {
        for junk in [
            &b"x#4\nabcd\n##\n"[..],
            b"\n#0\n\n##\n",
            b"\n#04\nabcd\n##\n",
            b"\n#a\n",
            b"\n##\n",
            b"\n#99999999999\n",
            b"\n#4xabcd",
            b"\n#4\nabcd\n#x",
        ] {
            assert!(decode(junk, Framing::Chunked).is_err(), "{}", String::from_utf8_lossy(junk));
        }
        let huge = format!("\n#{}\n", MAX_MESSAGE + 1);
        assert!(decode(huge.as_bytes(), Framing::Chunked).is_err());
        assert!(decode(&vec![b'a'; MAX_MESSAGE + 1], Framing::EndOfMessage).is_err());
        assert!(decode(b"\xff\xfe]]>]]>", Framing::EndOfMessage).is_err());
        assert!(describe(b"<rpc-reply><ok/></rpc-reply>").contains("rpc-reply"));
    }
}
