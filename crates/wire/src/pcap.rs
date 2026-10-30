//! Classic pcap capture files (the format every packet analyser reads): a 24-byte global header,
//! then a 16-byte record header before each frame. Link type 1 (Ethernet).

/// Writes a capture of `(time in microseconds, frame bytes)` records.
pub fn write<'a>(records: impl IntoIterator<Item = (u64, &'a [u8])>) -> Vec<u8> {
    write_link(LINKTYPE_ETHERNET, records)
}

/// Ethernet II frames.
pub const LINKTYPE_ETHERNET: u32 = 1;
/// IEEE 802.11 frames without a radio header or FCS.
pub const LINKTYPE_IEEE802_11: u32 = 105;

/// A capture whose records all have the link type `link` (a pcap file has exactly one).
pub fn write_link<'a>(link: u32, records: impl IntoIterator<Item = (u64, &'a [u8])>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xa1b2_c3d4u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes()); // thiszone
    out.extend_from_slice(&0u32.to_le_bytes()); // sigfigs
    out.extend_from_slice(&65_535u32.to_le_bytes()); // snaplen
    out.extend_from_slice(&link.to_le_bytes());
    for (micros, frame) in records {
        let len = u32::try_from(frame.len()).unwrap_or(u32::MAX);
        let secs = u32::try_from(micros / 1_000_000).unwrap_or(u32::MAX);
        let usecs = (micros % 1_000_000) as u32;
        out.extend_from_slice(&secs.to_le_bytes());
        out.extend_from_slice(&usecs.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(frame);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn header_and_records() {
        let f1 = [1u8; 60];
        let f2 = [2u8; 64];
        let b = super::write([(1_500_000, &f1[..]), (2_000_001, &f2[..])]);
        assert_eq!(b.len(), 24 + 16 + 60 + 16 + 64);
        assert_eq!(&b[..4], &[0xd4, 0xc3, 0xb2, 0xa1]);
        assert_eq!(u32::from_le_bytes([b[20], b[21], b[22], b[23]]), 1);
        assert_eq!(u32::from_le_bytes([b[24], b[25], b[26], b[27]]), 1);
        assert_eq!(u32::from_le_bytes([b[28], b[29], b[30], b[31]]), 500_000);
        assert_eq!(u32::from_le_bytes([b[32], b[33], b[34], b[35]]), 60);
    }
}
