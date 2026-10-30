//! File ▸ Print: the workspace as a one-page PDF (A4 landscape, a title line and the picture
//! scaled to fit), for the system's PDF viewer to print. Written by hand: an image XObject with
//! FlateDecode (stored zlib blocks, `png::zlib_stored`) and the standard Helvetica font.

use std::fmt::Write as _;

/// A4 landscape, in points.
const PAGE: (f32, f32) = (842.0, 595.0);
const MARGIN: f32 = 36.0;
/// Room for the title line above the picture.
const TITLE: f32 = 24.0;

/// Text for a PDF string in Helvetica: printable ASCII only (anything else becomes `?`), with
/// `\`, `(` and `)` escaped.
fn pdf_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars().take(200) {
        match ch {
            '\\' | '(' | ')' => {
                out.push('\\');
                out.push(ch);
            }
            ' '..='~' => out.push(ch),
            _ => out.push('?'),
        }
    }
    out
}

/// A one-page PDF: `title` on top, then `rgb` (`width × height` pixels, 3 bytes each, rows top
/// to bottom) scaled to fit the page, keeping its proportions.
pub fn page(title: &str, width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let (w, h) = (width.max(1), height.max(1));
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(3);
    let mut pixels = rgb.get(..need.min(rgb.len())).unwrap_or_default().to_vec();
    pixels.resize(need, 255);
    let image = crate::png::zlib_stored(&pixels);
    let (room_w, room_h) = (PAGE.0 - 2.0 * MARGIN, PAGE.1 - 2.0 * MARGIN - TITLE);
    let scale = (room_w / w as f32).min(room_h / h as f32);
    let (dw, dh) = (w as f32 * scale, h as f32 * scale);
    let (x, y) = (MARGIN + (room_w - dw) / 2.0, MARGIN + (room_h - dh));
    let content = format!(
        "BT /F1 14 Tf {MARGIN} {:.1} Td ({}) Tj ET\nq {dw:.2} 0 0 {dh:.2} {x:.2} {y:.2} cm /Im1 Do Q\n",
        PAGE.1 - MARGIN - 14.0,
        pdf_text(title)
    );
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, head: String, stream: Option<&[u8]>| {
        offsets.push(out.len());
        out.extend_from_slice(head.as_bytes());
        if let Some(s) = stream {
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(s);
            out.extend_from_slice(b"\nendstream");
        }
        out.extend_from_slice(b"\nendobj\n");
    };
    let n = |i: usize| format!("{i} 0 obj\n");
    object(&mut out, n(1) + "<< /Type /Catalog /Pages 2 0 R >>", None);
    object(&mut out, n(2) + "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", None);
    object(
        &mut out,
        n(3) + &format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 4 0 R >> /XObject << /Im1 5 0 R >> >> /Contents 6 0 R >>",
            PAGE.0, PAGE.1
        ),
        None,
    );
    object(&mut out, n(4) + "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>", None);
    object(
        &mut out,
        n(5) + &format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
            image.len()
        ),
        Some(&image),
    );
    object(&mut out, n(6) + &format!("<< /Length {} >>", content.len()), Some(content.as_bytes()));
    let xref = out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(tail, "{o:010} 00000 n ");
    }
    let _ = write!(tail, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    out.extend_from_slice(tail.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_with_its_picture_and_a_cross_reference_table() {
        let pdf = page("Lab (draft) \\ niño", 2, 1, &[255, 0, 0, 0, 0, 255]);
        let has = |needle: &str| pdf.windows(needle.len()).any(|w| w == needle.as_bytes());
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        assert!(has("/Width 2 /Height 1") && has("(Lab \\(draft\\) \\\\ ni?o) Tj"));
        // Every xref offset points at its object.
        let xref = pdf.windows(5).rposition(|w| w == b"xref\n").unwrap();
        let table = String::from_utf8(pdf[xref..].to_vec()).unwrap();
        for (i, line) in table.lines().skip(3).take(6).enumerate() {
            let at: usize = line[..10].parse().unwrap();
            assert!(pdf[at..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
        // Hostile sizes don't panic: short pixel data is padded white, nothing is huge.
        assert!(!page("", 0, 0, &[]).is_empty());
        assert!(!page("", 3, 3, &[1]).is_empty());
    }
}
