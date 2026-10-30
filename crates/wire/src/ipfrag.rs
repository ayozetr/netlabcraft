//! IPv4 fragmentation and reassembly (RFC 791), "fragmentation needed" errors carrying the
//! next-hop MTU (RFC 1191), and TCP MSS clamping (the MSS option of RFC 9293 §3.7.1 rewritten
//! in passing SYNs). Everything works on encoded datagrams, so headers the simulator does not
//! model (flags, fragment offset, options) survive.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use crate::{PROTO_TCP, Result, WireError, checksum, pseudo_header};

/// The "don't fragment" flag in the flags/offset word.
const DF: u16 = 0x4000;
/// The "more fragments" flag.
const MF: u16 = 0x2000;
const OFFSET_MASK: u16 = 0x1fff;
/// Largest datagram reassembly builds (the IPv4 total length field's limit).
pub const MAX_DATAGRAM: usize = 65_535;
/// Most datagrams being reassembled at once by one device (the oldest is given up).
pub const MAX_REASSEMBLIES: usize = 64;
/// Most fragments one datagram may arrive in.
const MAX_PIECES: usize = 256;
/// How long fragments wait for the rest (µs): RFC 791's suggested 15 s.
pub const REASSEMBLY_US: u64 = 15_000_000;
/// The ICMP destination-unreachable code for "fragmentation needed and DF set".
pub const UNREACH_FRAG_NEEDED: u8 = 4;
/// The TCP option kind for the maximum segment size.
const OPT_MSS: u8 = 2;

fn word(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn header_len(d: &[u8]) -> Option<usize> {
    let ihl = usize::from(d.first()? & 0x0f) * 4;
    (ihl >= 20 && d.len() >= ihl).then_some(ihl)
}

/// Recomputes the IPv4 header checksum in place.
fn fix_header(d: &mut [u8]) {
    let Some(ihl) = header_len(d) else { return };
    if let Some(c) = d.get_mut(10..12) {
        c.copy_from_slice(&[0, 0]);
    }
    let sum = d.get(..ihl).map_or(0, |h| checksum(&[h]));
    if let Some(c) = d.get_mut(10..12) {
        c.copy_from_slice(&sum.to_be_bytes());
    }
}

/// Whether the datagram says "don't fragment".
pub fn dont_fragment(d: &[u8]) -> bool {
    word(d, 6).is_some_and(|f| f & DF != 0)
}

/// Sets or clears "don't fragment" (header checksum fixed).
pub fn set_dont_fragment(d: &mut [u8], on: bool) {
    let Some(f) = word(d, 6) else { return };
    let f = if on { f | DF } else { f & !DF };
    if let Some(b) = d.get_mut(6..8) {
        b.copy_from_slice(&f.to_be_bytes());
    }
    fix_header(d);
}

/// Whether the datagram is a fragment (more follow, or it is not the first).
pub fn is_fragment(d: &[u8]) -> bool {
    word(d, 6).is_some_and(|f| f & MF != 0 || f & OFFSET_MASK != 0)
}

/// Copies the flags and fragment offset of `from` into `into` (both encoded datagrams), so a
/// re-encoded header keeps them; the header checksum is fixed.
pub fn keep_fragment_bits(from: &[u8], mut into: Vec<u8>) -> Vec<u8> {
    if let (Some(f), Some(slot)) = (from.get(6..8), into.get_mut(6..8)) {
        slot.copy_from_slice(f);
        fix_header(&mut into);
    }
    into
}

/// The options a fragment after the first carries: those with the "copied" bit (RFC 791).
fn copied_options(opts: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&kind) = opts.get(i) {
        match kind {
            0 => break,
            1 => i += 1,
            _ => {
                let len = usize::from(*opts.get(i + 1).unwrap_or(&0)).max(2);
                if kind & 0x80 != 0
                    && let Some(o) = opts.get(i..i + len)
                {
                    out.extend_from_slice(o);
                }
                i += len;
            }
        }
    }
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// Splits a datagram into fragments no larger than `mtu` (RFC 791 §3.2). A datagram that
/// already fits comes back whole; one that says "don't fragment", or an MTU too small to carry
/// 8 data bytes, is an error.
pub fn fragment(d: &[u8], mtu: usize) -> Result<Vec<Vec<u8>>> {
    let t = || WireError::Truncated("ipv4 fragment");
    let ihl = header_len(d).ok_or_else(t)?;
    let total = usize::from(word(d, 2).ok_or_else(t)?).min(d.len());
    if total <= mtu {
        return Ok(vec![d.get(..total).ok_or_else(t)?.to_vec()]);
    }
    if dont_fragment(d) {
        return Err(WireError::Unsupported("ipv4", "fragmentation needed but DF is set".into()));
    }
    let flags = word(d, 6).ok_or_else(t)?;
    let base = flags & OFFSET_MASK;
    let last_mf = flags & MF != 0;
    let header = d.get(..ihl).ok_or_else(t)?;
    let later_opts = copied_options(header.get(20..).unwrap_or_default());
    let data = d.get(ihl..total).ok_or_else(t)?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < data.len() {
        let hlen = if at == 0 { ihl } else { 20 + later_opts.len() };
        let room = mtu.checked_sub(hlen).ok_or_else(|| WireError::Unsupported("ipv4", format!("MTU {mtu} too small")))? / 8 * 8;
        if room == 0 {
            return Err(WireError::Unsupported("ipv4", format!("MTU {mtu} too small")));
        }
        let take = room.min(data.len() - at);
        let more = at + take < data.len() || last_mf;
        let mut f = if at == 0 {
            header.to_vec()
        } else {
            let mut h = header.get(..20).ok_or_else(t)?.to_vec();
            h.extend_from_slice(&later_opts);
            if let Some(v) = h.first_mut() {
                *v = 0x40 | u8::try_from(hlen / 4).unwrap_or(5);
            }
            h
        };
        let off = base.saturating_add(u16::try_from(at / 8).map_err(|_| t())?);
        let fw = (flags & !(MF | OFFSET_MASK)) | if more { MF } else { 0 } | (off & OFFSET_MASK);
        let len = u16::try_from(hlen + take).map_err(|_| t())?;
        f.get_mut(2..4).ok_or_else(t)?.copy_from_slice(&len.to_be_bytes());
        f.get_mut(6..8).ok_or_else(t)?.copy_from_slice(&fw.to_be_bytes());
        f.extend_from_slice(data.get(at..at + take).ok_or_else(t)?);
        fix_header(&mut f);
        out.push(f);
        at += take;
    }
    Ok(out)
}

/// Which datagram a fragment belongs to: source, destination, protocol and identification.
pub type FragKey = (Ipv4Addr, Ipv4Addr, u8, u16);

#[derive(Debug, Clone)]
struct Partial {
    first_header: Option<Vec<u8>>,
    /// Data by byte offset.
    pieces: BTreeMap<usize, Vec<u8>>,
    /// The data length, once the last fragment came.
    total: Option<usize>,
    since_us: u64,
}

/// A device's reassembly buffers.
#[derive(Debug, Clone, Default)]
pub struct Reassembly {
    partial: BTreeMap<FragKey, Partial>,
}

impl Reassembly {
    /// Takes one fragment; returns the whole datagram once every piece is in. Malformed,
    /// overlapping or oversized fragments are an error (and the datagram is given up).
    pub fn add(&mut self, d: &[u8], now_us: u64) -> Result<Option<Vec<u8>>> {
        let t = || WireError::Truncated("ipv4 fragment");
        self.partial.retain(|_, p| now_us.saturating_sub(p.since_us) < REASSEMBLY_US);
        let ihl = header_len(d).ok_or_else(t)?;
        let total = usize::from(word(d, 2).ok_or_else(t)?);
        if total < ihl || d.len() < total {
            return Err(t());
        }
        let flags = word(d, 6).ok_or_else(t)?;
        let off = usize::from(flags & OFFSET_MASK) * 8;
        let more = flags & MF != 0;
        let data = d.get(ihl..total).ok_or_else(t)?;
        let addr = |at: usize| -> Option<Ipv4Addr> { Some(Ipv4Addr::new(*d.get(at)?, *d.get(at + 1)?, *d.get(at + 2)?, *d.get(at + 3)?)) };
        let key = (addr(12).ok_or_else(t)?, addr(16).ok_or_else(t)?, *d.get(9).ok_or_else(t)?, word(d, 4).ok_or_else(t)?);
        if off + data.len() > MAX_DATAGRAM - ihl || (more && data.len() % 8 != 0) {
            self.partial.remove(&key);
            return Err(WireError::Unsupported("ipv4", "a fragment that cannot fit a datagram".into()));
        }
        if !self.partial.contains_key(&key) && self.partial.len() >= MAX_REASSEMBLIES {
            let oldest = self.partial.iter().min_by_key(|(_, p)| p.since_us).map(|(k, _)| *k);
            if let Some(k) = oldest {
                self.partial.remove(&k);
            }
        }
        let p = self.partial.entry(key).or_insert(Partial { first_header: None, pieces: BTreeMap::new(), total: None, since_us: now_us });
        if p.pieces.len() >= MAX_PIECES {
            self.partial.remove(&key);
            return Err(WireError::Unsupported("ipv4", "too many fragments".into()));
        }
        // Overlaps are refused (RFC 5722's advice for IPv6, applied here too).
        let overlaps = p.pieces.iter().any(|(o, v)| *o < off + data.len() && off < o + v.len() && !(*o == off && v.as_slice() == data));
        if overlaps {
            self.partial.remove(&key);
            return Err(WireError::Unsupported("ipv4", "overlapping fragments".into()));
        }
        if off == 0 {
            p.first_header = Some(d.get(..ihl).ok_or_else(t)?.to_vec());
        }
        if !more {
            p.total = Some(off + data.len());
        }
        p.pieces.insert(off, data.to_vec());
        let Some(len) = p.total else { return Ok(None) };
        let Some(header) = p.first_header.clone() else { return Ok(None) };
        let mut covered = 0usize;
        for (o, v) in &p.pieces {
            if *o > covered {
                return Ok(None);
            }
            covered = covered.max(o + v.len());
        }
        if covered < len {
            return Ok(None);
        }
        let mut whole = header;
        let hlen = whole.len();
        whole.resize(hlen + len, 0);
        for (o, v) in &p.pieces {
            if let Some(slot) = whole.get_mut(hlen + o..hlen + o + v.len()) {
                slot.copy_from_slice(v);
            }
        }
        self.partial.remove(&key);
        let tl = u16::try_from(whole.len()).map_err(|_| t())?;
        whole.get_mut(2..4).ok_or_else(t)?.copy_from_slice(&tl.to_be_bytes());
        let f = word(&whole, 6).ok_or_else(t)? & DF;
        whole.get_mut(6..8).ok_or_else(t)?.copy_from_slice(&f.to_be_bytes());
        fix_header(&mut whole);
        Ok(Some(whole))
    }

    /// Datagrams waiting for more fragments.
    pub fn waiting(&self) -> usize {
        self.partial.len()
    }
}

/// An ICMP "fragmentation needed and DF set" message (type 3, code 4) with the next-hop MTU
/// in its second word (RFC 1191 §4).
pub fn frag_needed(mtu: u16, original: &[u8]) -> Vec<u8> {
    let mut v = vec![3, UNREACH_FRAG_NEEDED, 0, 0, 0, 0];
    v.extend_from_slice(&mtu.to_be_bytes());
    v.extend_from_slice(original);
    let c = checksum(&[&v]);
    if let Some(s) = v.get_mut(2..4) {
        s.copy_from_slice(&c.to_be_bytes());
    }
    v
}

/// The MSS a TCP segment's options announce, if any.
pub fn tcp_mss(seg: &[u8]) -> Option<u16> {
    let off = usize::from(seg.get(12)? >> 4) * 4;
    let opts = seg.get(20..off)?;
    let mut i = 0;
    while let Some(&kind) = opts.get(i) {
        match kind {
            0 => return None,
            1 => i += 1,
            _ => {
                let len = usize::from(*opts.get(i + 1)?);
                if len < 2 {
                    return None;
                }
                if kind == OPT_MSS && len == 4 {
                    return word(opts, i + 2);
                }
                i += len;
            }
        }
    }
    None
}

/// A TCP segment with an MSS option added after its 20-byte header (for SYNs), checksum
/// recomputed. A segment that already has options comes back unchanged.
pub fn tcp_with_mss(seg: &[u8], mss: u16, src: Ipv4Addr, dst: Ipv4Addr) -> Vec<u8> {
    if seg.len() < 20 || seg.get(12).is_none_or(|b| b >> 4 != 5) {
        return seg.to_vec();
    }
    let mut v = seg.get(..20).unwrap_or_default().to_vec();
    v.extend_from_slice(&[OPT_MSS, 4]);
    v.extend_from_slice(&mss.to_be_bytes());
    v.extend_from_slice(seg.get(20..).unwrap_or_default());
    if let Some(b) = v.get_mut(12) {
        *b = (6 << 4) | (*b & 0x0f);
    }
    tcp_checksum(&mut v, src, dst);
    v
}

fn tcp_checksum(seg: &mut [u8], src: Ipv4Addr, dst: Ipv4Addr) {
    if let Some(c) = seg.get_mut(16..18) {
        c.copy_from_slice(&[0, 0]);
    }
    let len = u16::try_from(seg.len()).unwrap_or(u16::MAX);
    let sum = checksum(&[&pseudo_header(src, dst, PROTO_TCP, len), seg]);
    if let Some(c) = seg.get_mut(16..18) {
        c.copy_from_slice(&sum.to_be_bytes());
    }
}

/// `ip tcp adjust-mss`: a TCP SYN in this datagram announcing a larger MSS than `max` gets
/// `max` instead, with the TCP checksum made right. `None` when nothing changed (not TCP, not
/// a SYN, a fragment, no MSS option, or one already small enough).
pub fn clamp_mss(d: &[u8], max: u16) -> Option<Vec<u8>> {
    let ihl = header_len(d)?;
    if *d.get(9)? != PROTO_TCP || is_fragment(d) {
        return None;
    }
    let total = usize::from(word(d, 2)?).min(d.len());
    let seg = d.get(ihl..total)?;
    if seg.get(13)? & 0x02 == 0 {
        return None;
    }
    let off = usize::from(seg.get(12)? >> 4) * 4;
    let opts = seg.get(20..off)?;
    let mut i = 0;
    let mut at = None;
    while let Some(&kind) = opts.get(i) {
        match kind {
            0 => break,
            1 => i += 1,
            _ => {
                let len = usize::from(*opts.get(i + 1)?);
                if len < 2 {
                    return None;
                }
                if kind == OPT_MSS && len == 4 {
                    at = Some(i + 2);
                    break;
                }
                i += len;
            }
        }
    }
    let at = at?;
    if word(opts, at)? <= max {
        return None;
    }
    let src = Ipv4Addr::new(*d.get(12)?, *d.get(13)?, *d.get(14)?, *d.get(15)?);
    let dst = Ipv4Addr::new(*d.get(16)?, *d.get(17)?, *d.get(18)?, *d.get(19)?);
    let mut out = d.get(..total)?.to_vec();
    let seg = out.get_mut(ihl..)?;
    seg.get_mut(20 + at..22 + at)?.copy_from_slice(&max.to_be_bytes());
    tcp_checksum(seg, src, dst);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::{SYN, Tcp};
    use crate::{Ipv4, PROTO_ICMP};

    fn datagram(len: usize, df: bool) -> Vec<u8> {
        let ip = Ipv4 {
            id: 77,
            ttl: 64,
            protocol: PROTO_ICMP,
            src: Ipv4Addr::new(10, 0, 0, 1),
            dst: Ipv4Addr::new(10, 0, 0, 2),
            payload: (0..len).map(|i| (i % 251) as u8).collect(),
        };
        let mut d = ip.encode();
        set_dont_fragment(&mut d, df);
        d
    }

    #[test]
    fn fragments_and_reassembles_per_rfc_791() {
        let d = datagram(1_400, false);
        assert!(!dont_fragment(&d) && !is_fragment(&d));
        let frags = fragment(&d, 576).unwrap();
        assert_eq!(frags.len(), 3);
        // Every fragment fits, data lengths are multiples of 8 except the last, offsets add up.
        let mut expect_off = 0u16;
        for (i, f) in frags.iter().enumerate() {
            assert!(f.len() <= 576);
            assert!(is_fragment(f));
            let ip = Ipv4::decode(f).unwrap();
            let flags = word(f, 6).unwrap();
            assert_eq!(flags & OFFSET_MASK, expect_off);
            assert_eq!(flags & MF != 0, i + 1 < frags.len());
            if i + 1 < frags.len() {
                assert_eq!(ip.payload.len() % 8, 0);
            }
            expect_off += (ip.payload.len() / 8) as u16;
        }
        // In any order, reassembly gives back the original.
        let mut r = Reassembly::default();
        assert_eq!(r.add(&frags[2], 0).unwrap(), None);
        assert_eq!(r.add(&frags[0], 1).unwrap(), None);
        assert_eq!(r.waiting(), 1);
        assert_eq!(r.add(&frags[1], 2).unwrap(), Some(d.clone()));
        assert_eq!(r.waiting(), 0);
        // A datagram that fits is left alone; DF refuses.
        assert_eq!(fragment(&d, 1_500).unwrap(), vec![d.clone()]);
        assert!(fragment(&datagram(1_400, true), 576).is_err());
        assert!(fragment(&d, 24).is_err());
        // Fragments of fragments keep their place.
        let again: Vec<Vec<u8>> = frags.iter().flat_map(|f| fragment(f, 300).unwrap()).collect();
        let mut r = Reassembly::default();
        let whole: Vec<Option<Vec<u8>>> = again.iter().map(|f| r.add(f, 0).unwrap()).collect();
        assert_eq!(whole.last().cloned().flatten(), Some(d));
    }

    #[test]
    fn reassembly_times_out_refuses_overlaps_and_survives_truncation() {
        let d = datagram(1_000, false);
        let frags = fragment(&d, 400).unwrap();
        let mut r = Reassembly::default();
        r.add(&frags[0], 0).unwrap();
        // Too late: the first piece was forgotten.
        assert_eq!(r.add(&frags[1], REASSEMBLY_US + 1).unwrap(), None);
        assert_eq!(r.add(&frags[2], REASSEMBLY_US + 2).unwrap(), None);
        // Overlap.
        let mut bad = frags[1].clone();
        bad[6..8].copy_from_slice(&(MF | 1).to_be_bytes());
        fix_header(&mut bad);
        let mut r = Reassembly::default();
        r.add(&frags[0], 0).unwrap();
        assert!(r.add(&bad, 0).is_err());
        for f in &frags {
            for n in 0..f.len() {
                let _ = r.add(&f[..n], 0);
                let _ = fragment(&f[..n], 100);
                let _ = clamp_mss(&f[..n], 536);
                let _ = tcp_mss(&f[..n]);
            }
        }
        // Many datagrams at once: the oldest is given up.
        let mut r = Reassembly::default();
        for id in 0..(MAX_REASSEMBLIES as u16 + 5) {
            let mut f = frags[0].clone();
            f[4..6].copy_from_slice(&id.to_be_bytes());
            fix_header(&mut f);
            r.add(&f, u64::from(id)).unwrap();
        }
        assert_eq!(r.waiting(), MAX_REASSEMBLIES);
    }

    #[test]
    fn frag_needed_carries_the_mtu() {
        let d = datagram(100, true);
        let m = frag_needed(1_400, &d[..28]);
        assert_eq!(&m[..2], &[3, 4]);
        assert_eq!(word(&m, 6), Some(1_400));
        assert_eq!(checksum(&[&m]), 0);
    }

    #[test]
    fn mss_option_is_added_read_and_clamped() {
        let (s, d) = (Ipv4Addr::new(192, 168, 1, 10), Ipv4Addr::new(203, 0, 113, 10));
        let syn = Tcp { src_port: 49_152, dst_port: 80, seq: 1, ack: 0, flags: SYN, window: 65_535, payload: Vec::new() };
        let seg = tcp_with_mss(&syn.encode(s, d), 1_460, s, d);
        assert_eq!(tcp_mss(&seg), Some(1_460));
        assert_eq!(Tcp::decode(&seg, s, d).unwrap(), syn);
        let ip = Ipv4 { id: 1, ttl: 64, protocol: PROTO_TCP, src: s, dst: d, payload: seg.clone() }.encode();
        let clamped = clamp_mss(&ip, 1_360).unwrap();
        let back = Ipv4::decode(&clamped).unwrap();
        assert_eq!(tcp_mss(&back.payload), Some(1_360));
        assert_eq!(Tcp::decode(&back.payload, s, d).unwrap(), syn);
        assert_eq!(clamp_mss(&clamped, 1_400), None);
        // Not a SYN: left alone.
        let ack = Tcp { flags: crate::tcp::ACK, ..syn.clone() };
        let ip = Ipv4 { id: 1, ttl: 64, protocol: PROTO_TCP, src: s, dst: d, payload: tcp_with_mss(&ack.encode(s, d), 1_460, s, d) }.encode();
        assert_eq!(clamp_mss(&ip, 536), None);
        // A segment that already has options is not given a second MSS.
        assert_eq!(tcp_with_mss(&seg, 500, s, d), seg);
    }
}
