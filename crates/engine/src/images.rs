//! Reading the pictures people give the lab (backgrounds and place icons).
//!
//! Only PNG and JPEG are accepted, and whatever arrives is hostile: the size is checked before
//! the picture is decoded (so a file that claims a huge picture is refused without allocating
//! it), the decoder runs under allocation limits, and what is stored is made by us. A picture
//! within the lab's limits ([`MAX_IMAGE_SIDE`] pixels on a side, [`MAX_IMAGE_BYTES`] bytes) keeps
//! its own JPEG bytes with the metadata segments cut out (camera, place and time in a photo);
//! anything else is decoded, scaled down to fit, and written again as a PNG, which carries no
//! metadata. The model enforces its limits again when it stores the result.

use std::io::Cursor;

use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader, Limits, RgbaImage};
use netlabcraft_model::backdrop::{ImageKind, MAX_IMAGE_BYTES, MAX_IMAGE_SIDE};

/// Largest file read at all, in bytes.
pub const MAX_SOURCE_BYTES: usize = 48 * 1024 * 1024;
/// Longest side of a picture that is opened (then scaled down to [`MAX_IMAGE_SIDE`]), in pixels.
pub const MAX_SOURCE_SIDE: u32 = 12_000;
/// Most pixels of a picture that is opened (a 40 megapixel photo is about 7 000 by 5 700).
pub const MAX_SOURCE_PIXELS: u64 = 40_000_000;
/// What the decoder may allocate at once, in bytes.
const MAX_ALLOC: u64 = 384 * 1024 * 1024;
/// How many times a picture that is still too large is made smaller before giving up.
const SHRINKS: usize = 12;

/// A picture ready to be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub kind: ImageKind,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
    /// The size it had before it was scaled down to fit the limits (none: not scaled).
    pub scaled_from: Option<(u32, u32)>,
}

fn damaged() -> String {
    "the picture is damaged and cannot be read".to_string()
}

fn format_of(kind: ImageKind) -> ImageFormat {
    match kind {
        ImageKind::Png => ImageFormat::Png,
        ImageKind::Jpeg => ImageFormat::Jpeg,
    }
}

#[allow(clippy::field_reassign_with_default)]
fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_alloc = Some(MAX_ALLOC);
    l
}

fn reader(bytes: &[u8], kind: ImageKind) -> ImageReader<Cursor<&[u8]>> {
    let mut r = ImageReader::with_format(Cursor::new(bytes), format_of(kind));
    r.limits(limits());
    r
}

/// The kind and size of a picture, read from its header only. Refuses what is not a PNG or a JPEG,
/// what is larger than the lab will open, and what is damaged at the start.
pub fn inspect(bytes: &[u8]) -> Result<(ImageKind, u32, u32), String> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(format!("that file is too large to open as a picture (the limit is {} MiB)", MAX_SOURCE_BYTES / (1024 * 1024)));
    }
    let kind = ImageKind::sniff(bytes).ok_or_else(|| "that is not a PNG or JPEG picture".to_string())?;
    let (w, h) = reader(bytes, kind).into_dimensions().map_err(|_| damaged())?;
    if w == 0 || h == 0 {
        return Err(damaged());
    }
    if w > MAX_SOURCE_SIDE || h > MAX_SOURCE_SIDE || u64::from(w) * u64::from(h) > MAX_SOURCE_PIXELS {
        return Err(format!("the picture is {w} by {h} pixels, too large to open (at most {MAX_SOURCE_SIDE} on a side and 40 megapixels)"));
    }
    Ok((kind, w, h))
}

/// The size a picture of `w` by `h` shrinks to so that no side passes `max`, keeping its proportions.
fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest <= max {
        return (w, h);
    }
    let scale = |v: u32| u32::try_from(u64::from(v) * u64::from(max) / u64::from(longest)).unwrap_or(max).max(1);
    (scale(w), scale(h))
}

/// A JPEG without its metadata segments (EXIF and XMP in APP1, the other APPn but the JFIF header,
/// the colour profile and Adobe's colour transform, and comments); the rest is kept byte for byte.
/// `None` when the segments do not make sense.
pub fn strip_jpeg(b: &[u8]) -> Option<Vec<u8>> {
    if b.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2usize;
    loop {
        if *b.get(i)? != 0xFF {
            return None;
        }
        let m = *b.get(i + 1)?;
        if m == 0xFF {
            // A fill byte before a marker.
            i += 1;
            continue;
        }
        // The start of the scan or the end of the image: the rest is the picture's data.
        if m == 0xDA || m == 0xD9 {
            out.extend_from_slice(b.get(i..)?);
            return Some(out);
        }
        // Markers without a length.
        if m == 0x01 || (0xD0..=0xD8).contains(&m) {
            out.extend_from_slice(b.get(i..i + 2)?);
            i += 2;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([*b.get(i + 2)?, *b.get(i + 3)?]));
        if len < 2 {
            return None;
        }
        let seg = b.get(i..i + 2 + len)?;
        // APP1 (EXIF, XMP), APP3 to APP13 (IPTC and others), APP15 and comments go.
        let metadata = m == 0xE1 || (0xE3..=0xED).contains(&m) || m == 0xEF || m == 0xFE;
        if !metadata {
            out.extend_from_slice(seg);
        }
        i += 2 + len;
    }
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let (w, h) = img.dimensions();
    let opaque = img.pixels().all(|p| p.0[3] == 255);
    let mut out = Vec::new();
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Best, PngFilter::Adaptive);
    let result = if opaque {
        let rgb: Vec<u8> = img.pixels().flat_map(|p| [p.0[0], p.0[1], p.0[2]]).collect();
        enc.write_image(&rgb, w, h, ExtendedColorType::Rgb8)
    } else {
        enc.write_image(img.as_raw(), w, h, ExtendedColorType::Rgba8)
    };
    result.map_err(|_| "the picture could not be stored".to_string())?;
    Ok(out)
}

/// Checks a picture and makes what the lab stores: see the module's description.
pub fn prepare(bytes: &[u8]) -> Result<Prepared, String> {
    let (kind, w, h) = inspect(bytes)?;
    // Decoding all of it is what proves it is a picture and not a header followed by rubbish.
    let img = reader(bytes, kind).decode().map_err(|_| damaged())?;
    if img.width() != w || img.height() != h {
        return Err(damaged());
    }
    if kind == ImageKind::Jpeg
        && w <= MAX_IMAGE_SIDE
        && h <= MAX_IMAGE_SIDE
        && let Some(clean) = strip_jpeg(bytes)
        && clean.len() <= MAX_IMAGE_BYTES
    {
        return Ok(Prepared { kind, w, h, data: clean, scaled_from: None });
    }
    let rgba = img.into_rgba8();
    let (mut tw, mut th) = fit_within(w, h, MAX_IMAGE_SIDE);
    for _ in 0..SHRINKS {
        let data = if (tw, th) == (w, h) { encode_png(&rgba)? } else { encode_png(&image::imageops::resize(&rgba, tw, th, FilterType::Triangle))? };
        if data.len() <= MAX_IMAGE_BYTES {
            return Ok(Prepared { kind: ImageKind::Png, w: tw, h: th, data, scaled_from: ((tw, th) != (w, h)).then_some((w, h)) });
        }
        tw = (tw * 3 / 4).max(1);
        th = (th * 3 / 4).max(1);
    }
    Err("the picture is too detailed to be stored, even scaled down".to_string())
}

/// The pixels of a stored picture: width, height and RGBA bytes.
pub fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    decode_display(bytes, u32::MAX)
}

/// The pixels of a stored picture for drawing, scaled down so that no side passes `max_side` (a
/// graphics card takes pictures only up to a side of its own): width, height and RGBA bytes.
pub fn decode_display(bytes: &[u8], max_side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let kind = ImageKind::sniff(bytes).ok_or_else(damaged)?;
    let img = reader(bytes, kind).decode().map_err(|_| damaged())?.into_rgba8();
    let (w, h) = fit_within(img.width(), img.height(), max_side.max(1));
    if (w, h) == (img.width(), img.height()) {
        return Ok((w, h, img.into_raw()));
    }
    let small = image::imageops::resize(&img, w, h, FilterType::Triangle);
    Ok((small.width(), small.height(), small.into_raw()))
}

/// A PNG of `w` by `h` pixels made in code: a colour gradient with a grid of light lines, so a
/// background can be told from the empty map. The tests, the snapshot sheets and the examples use
/// it, so no picture is ever downloaded. Empty when the size is 0 or has more pixels than [`MAX_SOURCE_PIXELS`].
pub fn pattern_png(w: u32, h: u32) -> Vec<u8> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_SOURCE_PIXELS {
        return Vec::new();
    }
    let mut img = RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let grid = x % 50 == 0 || y % 50 == 0 || x == y;
        let (r, g, b) = ((x * 255 / w) as u8, (y * 255 / h) as u8, 150u8);
        *p = if grid { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([r / 2 + 90, g / 2 + 90, b, 255]) };
    }
    encode_png(&img).unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A PNG of `w` by `h` pixels.
    pub fn png(w: u32, h: u32) -> Vec<u8> {
        pattern_png(w, h)
    }

    /// The smallest JPEG there is: one grey pixel, a baseline file with one component and the
    /// shortest Huffman tables (written by hand, no encoder needed).
    pub fn jpeg() -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        v.extend_from_slice(&[1u8; 64]);
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00]);
        for class in [0x00u8, 0x10] {
            v.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, class, 0x01]);
            v.extend_from_slice(&[0u8; 15]);
            v.push(0x00);
        }
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x3F, 0xFF, 0xD9]);
        v
    }

    /// The same JPEG with an EXIF segment and a comment after the start marker.
    pub fn jpeg_with_metadata() -> Vec<u8> {
        let plain = jpeg();
        let mut v = vec![0xFF, 0xD8];
        v.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x0A]);
        v.extend_from_slice(b"Exif\0\0\0\0");
        v.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x07]);
        v.extend_from_slice(b"hello");
        v.extend_from_slice(&plain[2..]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn a_png_is_read_and_stored_as_a_png_of_the_same_size() {
        let p = prepare(&png(64, 48)).unwrap();
        assert_eq!((p.kind, p.w, p.h, p.scaled_from), (ImageKind::Png, 64, 48, None));
        let (w, h, px) = decode_rgba(&p.data).unwrap();
        assert_eq!((w, h, px.len()), (64, 48, 64 * 48 * 4));
        assert_eq!(inspect(&p.data).unwrap(), (ImageKind::Png, 64, 48));
    }

    #[test]
    fn a_picture_with_transparency_keeps_it() {
        let mut img = RgbaImage::new(4, 4);
        for p in img.pixels_mut() {
            *p = image::Rgba([10, 20, 30, 128]);
        }
        let p = prepare(&encode_png(&img).unwrap()).unwrap();
        let (_, _, px) = decode_rgba(&p.data).unwrap();
        assert_eq!(px.get(3), Some(&128));
    }

    #[test]
    fn a_picture_wider_than_the_limit_is_scaled_down_and_says_so() {
        let p = prepare(&png(MAX_IMAGE_SIDE + 904, 40)).unwrap();
        assert_eq!((p.w, p.h), (MAX_IMAGE_SIDE, 32));
        assert_eq!(p.scaled_from, Some((MAX_IMAGE_SIDE + 904, 40)));
        assert_eq!(inspect(&p.data).unwrap().1, MAX_IMAGE_SIDE);
    }

    #[test]
    fn the_smallest_jpeg_is_read_and_stripped_of_its_metadata() {
        let plain = prepare(&jpeg()).unwrap();
        assert_eq!((plain.kind, plain.w, plain.h), (ImageKind::Jpeg, 1, 1));
        assert_eq!(plain.data, jpeg(), "nothing to strip: the bytes are kept");
        let with = jpeg_with_metadata();
        assert!(contains(&with, b"Exif"));
        let cleaned = prepare(&with).unwrap();
        assert_eq!(cleaned.kind, ImageKind::Jpeg);
        assert!(!contains(&cleaned.data, b"Exif") && !contains(&cleaned.data, b"hello"));
        assert_eq!(cleaned.data, jpeg());
        let (w, h, px) = decode_rgba(&cleaned.data).unwrap();
        assert_eq!((w, h, px.len()), (1, 1, 4));
    }

    #[test]
    fn strip_jpeg_refuses_nonsense() {
        assert_eq!(strip_jpeg(&[]), None);
        assert_eq!(strip_jpeg(&[0xFF, 0xD8]), None);
        assert_eq!(strip_jpeg(&[0xFF, 0xD8, 0x00, 0x00]), None);
        assert_eq!(strip_jpeg(&[0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01]), None, "a length under 2");
        assert_eq!(strip_jpeg(&[0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF, 1, 2]), None, "a segment past the end");
        assert!(strip_jpeg(&[0xFF, 0xD8, 0xFF, 0xD9]).is_some());
    }

    #[test]
    fn what_is_not_a_picture_is_refused_with_a_message() {
        assert!(prepare(&[]).unwrap_err().contains("PNG or JPEG"));
        assert!(prepare(b"GIF89a....").is_err());
        assert!(prepare(b"<svg></svg>").is_err());
        assert!(prepare(&[0xFF, 0xD8, 0xFF]).is_err());
    }

    #[test]
    fn a_picture_that_claims_to_be_enormous_is_refused_before_it_is_decoded() {
        // A header (with a good checksum) for a 100 000 by 100 000 picture and nothing else.
        fn crc(data: &[u8]) -> u32 {
            let mut c = 0xFFFF_FFFFu32;
            for b in data {
                c ^= u32::from(*b);
                for _ in 0..8 {
                    c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        v.extend_from_slice(&ihdr);
        v.extend_from_slice(&crc(&ihdr).to_be_bytes());
        let err = prepare(&v).unwrap_err();
        assert!(err.contains("too large") || err.contains("damaged"), "{err}");
        // Too many pixels in a picture whose sides are each allowed.
        let wide = png(MAX_SOURCE_SIDE + 1, 1);
        assert!(inspect(&wide).unwrap_err().contains("too large"));
    }

    #[test]
    fn a_file_over_the_source_limit_is_refused_unread() {
        let big = vec![0x89u8; MAX_SOURCE_BYTES + 1];
        assert!(inspect(&big).unwrap_err().contains("too large"));
    }

    #[test]
    fn truncated_and_corrupt_pictures_never_panic() {
        for original in [png(40, 30), jpeg()] {
            for n in 0..original.len() {
                let _ = prepare(original.get(..n).unwrap_or_default());
            }
            // Flip each byte in turn.
            for i in 0..original.len() {
                let mut bad = original.clone();
                if let Some(b) = bad.get_mut(i) {
                    *b ^= 0xA5;
                }
                let _ = prepare(&bad);
            }
        }
        // Corrupt data after a good header is refused.
        let mut cut = png(40, 30);
        let half = cut.len() / 2;
        cut.truncate(half);
        assert!(prepare(&cut).is_err());
    }

    #[test]
    fn a_picture_for_drawing_is_scaled_to_the_side_the_card_takes() {
        let p = png(300, 100);
        let (w, h, px) = decode_display(&p, 150).unwrap();
        assert_eq!((w, h, px.len()), (150, 50, 150 * 50 * 4));
        assert_eq!(decode_display(&p, 4096).unwrap().0, 300);
        assert!(decode_display(b"nope", 100).is_err());
    }

    #[test]
    fn fit_within_keeps_the_proportions() {
        assert_eq!(fit_within(100, 50, 4096), (100, 50));
        assert_eq!(fit_within(8192, 4096, 4096), (4096, 2048));
        assert_eq!(fit_within(1, 10_000, 4096), (1, 4096));
        assert_eq!(fit_within(10_000, 1, 4096), (4096, 1));
    }
}
