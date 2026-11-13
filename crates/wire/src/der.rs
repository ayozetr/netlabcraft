//! A strict, bounded subset of ASN.1 DER (ITU-T X.690): the reader and writer that
//! [`crate::x509`] builds certificates and certificate requests from.
//!
//! The reader accepts only what DER allows: single-byte tags, definite and minimal lengths,
//! nothing left over. Every length is checked against the bytes that are there, the number of
//! items in a constructed value is capped ([`MAX_ITEMS`]) and the nesting a caller walks is
//! fixed by the structures it parses, so hostile input costs bounded time and memory.

use crate::{Result, WireError};

pub const INTEGER: u8 = 0x02;
pub const BIT_STRING: u8 = 0x03;
pub const OCTET_STRING: u8 = 0x04;
pub const NULL: u8 = 0x05;
pub const OID: u8 = 0x06;
pub const UTF8_STRING: u8 = 0x0c;
pub const PRINTABLE_STRING: u8 = 0x13;
pub const IA5_STRING: u8 = 0x16;
pub const UTC_TIME: u8 = 0x17;
pub const GENERALIZED_TIME: u8 = 0x18;
pub const BOOLEAN: u8 = 0x01;
pub const SEQUENCE: u8 = 0x30;
pub const SET: u8 = 0x31;
/// Items allowed in one constructed value.
pub const MAX_ITEMS: usize = 64;

fn bad(why: &str) -> WireError {
    WireError::Unsupported("der", why.to_string())
}

/// One tag-length-value: the tag, the content and the whole encoding (header included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tlv<'a> {
    pub tag: u8,
    pub content: &'a [u8],
    pub raw: &'a [u8],
}

/// Reads the first value of `input`; returns it and what follows.
pub fn read(input: &[u8]) -> Result<(Tlv<'_>, &[u8])> {
    let tag = *input.first().ok_or(WireError::Truncated("der tag"))?;
    if tag & 0x1f == 0x1f {
        return Err(bad("multi-byte tag"));
    }
    let l0 = *input.get(1).ok_or(WireError::Truncated("der length"))?;
    let (len, hdr) = if l0 < 0x80 {
        (usize::from(l0), 2usize)
    } else {
        let n = usize::from(l0 & 0x7f);
        if n == 0 || n > 3 {
            return Err(bad("length form"));
        }
        let bytes = input.get(2..2 + n).ok_or(WireError::Truncated("der length"))?;
        if bytes.first() == Some(&0) {
            return Err(bad("length not minimal"));
        }
        let len = bytes.iter().fold(0usize, |a, b| (a << 8) | usize::from(*b));
        if len < 0x80 {
            return Err(bad("length not minimal"));
        }
        (len, 2 + n)
    };
    let end = hdr.checked_add(len).ok_or(WireError::Truncated("der value"))?;
    let content = input.get(hdr..end).ok_or(WireError::Truncated("der value"))?;
    let raw = input.get(..end).ok_or(WireError::Truncated("der value"))?;
    let rest = input.get(end..).ok_or(WireError::Truncated("der value"))?;
    Ok((Tlv { tag, content, raw }, rest))
}

/// The whole of `input` as one value of this tag.
pub fn read_exact(input: &[u8], tag: u8) -> Result<Tlv<'_>> {
    let (t, rest) = read(input)?;
    if !rest.is_empty() {
        return Err(bad("trailing bytes"));
    }
    if t.tag != tag {
        return Err(bad("unexpected tag"));
    }
    Ok(t)
}

/// The values inside a constructed value, at most [`MAX_ITEMS`].
pub fn children(content: &[u8]) -> Result<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    let mut rest = content;
    while !rest.is_empty() {
        if out.len() >= MAX_ITEMS {
            return Err(bad("too many items"));
        }
        let (t, r) = read(rest)?;
        out.push(t);
        rest = r;
    }
    Ok(out)
}

impl<'a> Tlv<'a> {
    /// The items of a value with this tag.
    pub fn items(&self, tag: u8) -> Result<Vec<Tlv<'a>>> {
        if self.tag != tag {
            return Err(bad("unexpected tag"));
        }
        children(self.content)
    }

    /// A non-negative INTEGER's magnitude, big-endian with no leading zero byte (empty for 0).
    /// Negative numbers and redundant leading bytes are refused.
    pub fn uint(&self) -> Result<&'a [u8]> {
        if self.tag != INTEGER || self.content.is_empty() {
            return Err(bad("not an integer"));
        }
        let (first, rest) = self.content.split_at(1);
        let first = first.first().copied().unwrap_or(0);
        if first & 0x80 != 0 {
            return Err(bad("negative integer"));
        }
        if first == 0 {
            return match rest.first() {
                None => Ok(&[]),
                Some(b) if b & 0x80 != 0 => Ok(rest),
                Some(_) => Err(bad("integer not minimal")),
            };
        }
        Ok(self.content)
    }

    /// A BOOLEAN (DER: 0x00 or 0xFF).
    pub fn boolean(&self) -> Result<bool> {
        match (self.tag, self.content) {
            (BOOLEAN, [0x00]) => Ok(false),
            (BOOLEAN, [0xff]) => Ok(true),
            _ => Err(bad("not a DER boolean")),
        }
    }
}

/// The encoding of a length.
fn length(len: usize) -> Vec<u8> {
    if len < 0x80 {
        return vec![len as u8];
    }
    let bytes: Vec<u8> = len.to_be_bytes().iter().copied().skip_while(|b| *b == 0).collect();
    let mut out = vec![0x80 | bytes.len() as u8];
    out.extend(bytes);
    out
}

/// A value: tag, length, content.
pub fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend(length(content.len()));
    out.extend_from_slice(content);
    out
}

pub fn sequence(parts: &[&[u8]]) -> Vec<u8> {
    tlv(SEQUENCE, &parts.concat())
}

pub fn set(parts: &[&[u8]]) -> Vec<u8> {
    tlv(SET, &parts.concat())
}

/// A non-negative INTEGER from big-endian bytes (leading zeros dropped, a zero byte added when
/// the top bit is set).
pub fn uint(be: &[u8]) -> Vec<u8> {
    let trimmed: &[u8] = be.get(be.iter().take_while(|b| **b == 0).count()..).unwrap_or_default();
    let mut content = Vec::with_capacity(trimmed.len() + 1);
    if trimmed.first().is_none_or(|b| b & 0x80 != 0) {
        content.push(0);
    }
    content.extend_from_slice(trimmed);
    tlv(INTEGER, &content)
}

pub fn boolean(v: bool) -> Vec<u8> {
    tlv(BOOLEAN, &[if v { 0xff } else { 0x00 }])
}

pub fn null() -> Vec<u8> {
    vec![NULL, 0]
}

/// A BIT STRING with no unused bits.
pub fn bit_string(bytes: &[u8]) -> Vec<u8> {
    let mut content = vec![0];
    content.extend_from_slice(bytes);
    tlv(BIT_STRING, &content)
}

/// The bytes of a BIT STRING that has no unused bits.
pub fn bit_string_bytes<'a>(t: &Tlv<'a>) -> Result<&'a [u8]> {
    match (t.tag, t.content.split_first()) {
        (BIT_STRING, Some((0, rest))) => Ok(rest),
        _ => Err(bad("not a whole-byte bit string")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_encodings() {
        assert_eq!(uint(&[0x01, 0x00, 0x01]), vec![2, 3, 1, 0, 1]);
        assert_eq!(uint(&[0x80]), vec![2, 2, 0, 0x80]);
        assert_eq!(uint(&[0, 0, 0x7f]), vec![2, 1, 0x7f]);
        assert_eq!(uint(&[]), vec![2, 1, 0]);
        assert_eq!(null(), vec![5, 0]);
        assert_eq!(tlv(OCTET_STRING, &[0u8; 127])[..2], [4, 127]);
        assert_eq!(tlv(OCTET_STRING, &[0u8; 128])[..3], [4, 0x81, 128]);
        assert_eq!(tlv(OCTET_STRING, &[0u8; 300])[..4], [4, 0x82, 1, 44]);
        assert_eq!(bit_string(&[0xab]), vec![3, 2, 0, 0xab]);
    }

    #[test]
    fn writer_and_reader_agree() {
        for len in [0usize, 1, 127, 128, 255, 256, 65_535, 65_536, 70_000] {
            let enc = tlv(OCTET_STRING, &vec![7u8; len]);
            let t = read_exact(&enc, OCTET_STRING).unwrap();
            assert_eq!(t.content.len(), len);
            assert_eq!(t.raw, &enc[..]);
        }
        let seq = sequence(&[&uint(&[5]), &boolean(true), &null()]);
        let items = read_exact(&seq, SEQUENCE).unwrap().items(SEQUENCE).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].uint().unwrap(), &[5]);
        assert!(items[1].boolean().unwrap());
        assert_eq!(uint(&[0xff, 0x01]), vec![2, 3, 0, 0xff, 1]);
        assert_eq!(read_exact(&uint(&[0xff, 0x01]), INTEGER).unwrap().uint().unwrap(), &[0xff, 1]);
    }

    #[test]
    fn non_der_forms_are_refused() {
        // Multi-byte tag, indefinite length, over-long and non-minimal lengths.
        assert!(read(&[0x1f, 0x81, 0]).is_err());
        assert!(read(&[0x30, 0x80, 0, 0]).is_err());
        assert!(read(&[0x04, 0x84, 0, 0, 0, 1, 0]).is_err());
        assert!(read(&[0x04, 0x81, 0x05, 1, 2, 3, 4, 5]).is_err());
        assert!(read(&[0x04, 0x82, 0x00, 0x90]).is_err());
        // Trailing bytes, wrong tag, non-minimal or negative integers, bad booleans.
        assert!(read_exact(&[5, 0, 0], NULL).is_err());
        assert!(read_exact(&[5, 0], INTEGER).is_err());
        assert!(read_exact(&[2, 2, 0, 1], INTEGER).unwrap().uint().is_err());
        assert!(read_exact(&[2, 1, 0x80], INTEGER).unwrap().uint().is_err());
        assert!(read_exact(&[2, 0], INTEGER).unwrap().uint().is_err());
        assert!(read_exact(&[1, 1, 1], BOOLEAN).unwrap().boolean().is_err());
        assert!(read_exact(&[3, 2, 1, 0], BIT_STRING).and_then(|t| bit_string_bytes(&t)).is_err());
    }

    #[test]
    fn every_truncation_and_huge_lengths_are_errors() {
        let seq = sequence(&[&uint(&[1, 2, 3]), &tlv(OCTET_STRING, &[9; 200]), &null()]);
        for cut in 0..seq.len() {
            let part = &seq[..cut];
            assert!(read_exact(part, SEQUENCE).is_err(), "cut {cut}");
        }
        // A length that claims far more than is there, and one that would overflow.
        assert!(read(&[4, 0x83, 0xff, 0xff, 0xff, 1]).is_err());
        assert!(read(&[4, 0x88, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err());
        // Too many items in one constructed value.
        let many = tlv(SEQUENCE, &[5u8, 0].repeat(MAX_ITEMS + 1));
        assert!(read_exact(&many, SEQUENCE).unwrap().items(SEQUENCE).is_err());
        let ok = tlv(SEQUENCE, &[5u8, 0].repeat(MAX_ITEMS));
        assert_eq!(read_exact(&ok, SEQUENCE).unwrap().items(SEQUENCE).unwrap().len(), MAX_ITEMS);
    }
}
