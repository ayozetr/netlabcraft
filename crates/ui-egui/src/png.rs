//! A small PNG writer (PNG 1.2: 8-bit RGBA, no filtering, zlib "stored" blocks), for exporting
//! the workspace as an image without an image library.

fn crc32(chunks: &[&[u8]]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for bytes in chunks {
        for b in *bytes {
            c ^= u32::from(*b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
    }
    !c
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for x in bytes {
        a = (a + u32::from(*x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
}

/// Encodes `rgba` (`width × height` pixels, 4 bytes each, rows top to bottom) as a PNG file.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row = width as usize * 4;
    // Each row starts with its filter type: 0 (none).
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0);
        raw.extend_from_slice(rgba.get(y * row..(y + 1) * row).unwrap_or(&vec![0; row]));
    }
    // zlib: header, stored deflate blocks of at most 65535 bytes, Adler-32.
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if raw.is_empty() { vec![&[][..]] } else { raw.chunks(65_535).collect() };
    let last = blocks.len().saturating_sub(1);
    for (i, b) in blocks.into_iter().enumerate() {
        z.push(u8::from(i == last));
        let len = u16::try_from(b.len()).unwrap_or(u16::MAX);
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_and_layout() {
        // Known values: CRC-32 of "123456789" and Adler-32 of "Wikipedia".
        assert_eq!(crc32(&[b"123456789"]), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        let png = encode(2, 1, &[255, 0, 0, 255, 0, 0, 255, 128]);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert!(png.ends_with(&[0xae, 0x42, 0x60, 0x82]), "IEND's CRC");
        // Bigger than one stored block.
        let big = encode(200, 100, &vec![7u8; 200 * 100 * 4]);
        assert!(big.len() > 80_000);
    }
}
