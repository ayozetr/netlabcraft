//! File ▸ Print: the lab as a PDF for the system's viewer to print. A4 landscape, written by hand.
//!
//! Page 1 is the workspace picture under the lab's name (an image XObject, Deflate-compressed with
//! `miniz_oxide`). The pages after it are a report read straight from the model: every device
//! with its model and its interfaces (IPv4 address and prefix, IPv6 addresses, VLAN or switchport
//! mode, status, MAC address), then the list of cables. The text is real PDF text in the standard
//! Helvetica font (WinAnsi: Latin-1 letters print, anything else becomes `?`), the tables are
//! drawn with lines, and every page has a footer with its number. The file carries an Info
//! dictionary and an XMP packet (title, creator, producer, subject, keywords, creation date).
//!
//! The report is capped (devices, rows, cables, pages) so a huge lab cannot make a huge file, and
//! nothing here panics on odd input.

use std::fmt::Write as _;

use netlabcraft_model::{DeviceKind, End, Port, Switchport, SwitchportMode, Topology, models};

/// A4 landscape, in points.
const PAGE: (f32, f32) = (842.0, 595.0);
const MARGIN: f32 = 36.0;
/// Room for the title line above the picture.
const TITLE: f32 = 24.0;
/// The lowest line a table row may use (the footer sits below it).
const BOTTOM: f32 = 46.0;
/// Where the footer's text sits.
const FOOTER_Y: f32 = 20.0;

/// Report caps: past these the report says it was cut instead of growing.
const MAX_DEVICES: usize = 400;
const MAX_ROWS: usize = 2_400;
const MAX_LINKS: usize = 1_000;
const MAX_REPORT_PAGES: usize = 80;
/// Longest text taken from the lab (a name, a description) before it is clipped.
const MAX_TEXT: usize = 200;

const FONT: f32 = 8.0;
const ROW: f32 = 12.0;

/// Helvetica advance widths (1/1000 em) for ASCII 32..=126, from the standard font metrics.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584,
    584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667,
    611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500,
    722, 500, 500, 500, 334, 260, 334, 584,
];

/// One character of text as the font can print it: ASCII and Latin-1 as they are, a few common
/// typographic marks as their plain forms, and anything else (or a control character) as `?`.
fn latin1(ch: char) -> char {
    match ch {
        ' '..='~' | '\u{a0}'..='\u{ff}' => ch,
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
        '\u{2026}' => '.',
        '\t' | '\n' | '\r' => ' ',
        _ => '?',
    }
}

/// `s` as printable Latin-1 text, at most `MAX_TEXT` characters.
fn clean(s: &str) -> String {
    s.chars().take(MAX_TEXT).map(latin1).collect()
}

fn char_width(ch: char) -> f32 {
    let w = (ch as usize).checked_sub(32).and_then(|i| HELVETICA.get(i)).copied().unwrap_or(556);
    f32::from(w) / 1000.0
}

/// The width of `s` at `size` points.
fn text_width(s: &str, size: f32, bold: bool) -> f32 {
    let w: f32 = s.chars().map(|c| char_width(latin1(c))).sum::<f32>() * size;
    if bold { w * 1.08 } else { w }
}

/// `s` cleaned and cut with `...` so it fits `max` points.
fn fit(s: &str, max: f32, size: f32, bold: bool) -> String {
    let s = clean(s);
    if text_width(&s, size, bold) <= max {
        return s;
    }
    let mut out = String::new();
    for ch in s.chars() {
        if text_width(&format!("{out}{ch}..."), size, bold) > max {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

/// A PDF string for page text: `(…)` with `\`, `(` and `)` escaped and Latin-1 letters as octal
/// escapes, so the file stays 7-bit.
fn pdf_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('(');
    for ch in s.chars().map(latin1) {
        match ch {
            '\\' | '(' | ')' => {
                out.push('\\');
                out.push(ch);
            }
            ' '..='~' => out.push(ch),
            _ => {
                let _ = write!(out, "\\{:03o}", u32::from(ch) & 0xff);
            }
        }
    }
    out.push(')');
    out
}

/// A PDF text string for the Info dictionary: a literal string when it is plain ASCII, otherwise
/// UTF-16BE with a byte-order mark as a hex string (so any title survives).
fn info_string(s: &str) -> String {
    let s: String = s.chars().take(MAX_TEXT).map(|c| if c.is_control() { ' ' } else { c }).collect();
    if s.chars().all(|c| matches!(c, ' '..='~')) {
        let mut out = String::from("(");
        for ch in s.chars() {
            if matches!(ch, '\\' | '(' | ')') {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        for unit in s.encode_utf16() {
            let _ = write!(out, "{unit:04X}");
        }
        out.push('>');
        out
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars().take(MAX_TEXT).filter(|c| !c.is_control()) {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `2026-11-14T09:30:00Z` as its fourteen digits (`20261114093000`), or `None` when it is not
/// that shape (the clock is missing on wasm).
fn date_digits(iso: &str) -> Option<String> {
    let digits: String = iso.chars().filter(char::is_ascii_digit).collect();
    let shaped = iso.len() == 20 && iso.ends_with('Z') && digits.len() == 14;
    shaped.then_some(digits)
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

// ---------------------------------------------------------------------------------------------
// The report's rows, read from the model.

/// One line of the report.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    /// A section title (`Devices`, `Links`).
    Section(String),
    /// A group of devices (`Routers`).
    Group(String),
    /// The column titles and widths of the rows that follow.
    Header(Vec<(String, f32)>),
    /// A device: its name and model, and a detail on the right.
    Device(String, String),
    /// One row of the current table.
    Cells(Vec<String>),
    /// A line of plain text.
    Note(String),
}

/// Column titles and widths of the interface table (770 points in all).
const IFACE_COLS: [(&str, f32); 6] =
    [("Interface", 92.0), ("IPv4 / mask", 108.0), ("IPv6", 214.0), ("VLAN / mode", 134.0), ("Status", 94.0), ("MAC address", 128.0)];

fn iface_header() -> Row {
    Row::Header(IFACE_COLS.iter().map(|(t, w)| ((*t).to_string(), *w)).collect())
}

fn or_dash(s: String) -> String {
    if s.is_empty() { "-".to_string() } else { s }
}

fn ipv6_cell(port: &Port) -> String {
    let v6 = &port.ipv6;
    let mut cell = match v6.addresses.first() {
        Some(a) => a.to_string(),
        None if v6.autoconfig => "autoconfig (SLAAC)".to_string(),
        None => v6.link_local.map(|a| format!("{a} (link-local)")).unwrap_or_default(),
    };
    if v6.addresses.len() > 1 {
        let _ = write!(cell, " (+{})", v6.addresses.len() - 1);
    }
    or_dash(cell)
}

fn mode_cell(kind: DeviceKind, port: &Port) -> String {
    if let Some(w) = &port.wifi {
        return format!("Wi-Fi {}", w.ssid);
    }
    if kind != DeviceKind::Switch || port.routed {
        return if port.routed { "routed".to_string() } else { "-".to_string() };
    }
    let sp = &port.switchport;
    let mut cell = match sp.mode {
        SwitchportMode::Access => format!("access VLAN {}", sp.access_vlan.get()),
        SwitchportMode::Trunk => format!("trunk native {} allowed {}", sp.native_vlan.get(), sp.allowed),
        other => other.name().to_string(),
    };
    if let Some(v) = sp.voice_vlan {
        let _ = write!(cell, " voice {}", v.get());
    }
    if let Some(c) = &port.channel {
        let _ = write!(cell, " Po{}", c.number);
    }
    cell
}

fn status_cell(port: &Port, linked: bool) -> &'static str {
    if port.err_disabled {
        "err-disabled"
    } else if port.shutdown {
        "admin down"
    } else if linked || port.wifi.is_some() {
        "connected"
    } else {
        "not connected"
    }
}

/// Whether a port belongs in the report: cabled, or set to anything.
fn is_listed(kind: DeviceKind, port: &Port, linked: bool) -> bool {
    linked
        || port.ip.is_some()
        || port.dhcp
        || port.shutdown
        || port.err_disabled
        || port.wifi.is_some()
        || port.ipv6.enabled
        || !port.ipv6.addresses.is_empty()
        || !port.subifs.is_empty()
        || !port.description.is_empty()
        || port.channel.is_some()
        || (kind == DeviceKind::Switch && port.switchport != Switchport::default())
}

fn group_of(kind: DeviceKind) -> usize {
    match kind {
        DeviceKind::Router => 0,
        DeviceKind::Switch | DeviceKind::Hub => 1,
        _ => 2,
    }
}

const GROUPS: [&str; 3] = ["Routers", "Switches and hubs", "End devices"];

/// The report's rows: the devices grouped by role, each with its interfaces, then the cables.
/// Capped at `MAX_DEVICES` devices, `MAX_ROWS` rows and `MAX_LINKS` cables; returns whether
/// anything was left out.
fn report_rows(t: &Topology) -> (Vec<Row>, bool) {
    let mut rows = vec![Row::Section("Devices".to_string())];
    let mut cut = false;
    let mut listed = 0usize;
    'groups: for (g, title) in GROUPS.iter().enumerate() {
        let members: Vec<_> = t.devices.iter().filter(|d| group_of(d.kind) == g).collect();
        if members.is_empty() {
            continue;
        }
        rows.push(Row::Group(format!("{title} ({})", members.len())));
        rows.push(iface_header());
        for d in members {
            if listed >= MAX_DEVICES || rows.len() >= MAX_ROWS {
                cut = true;
                break 'groups;
            }
            listed += 1;
            let model =
                if d.model.is_empty() { models::default_for(d.kind) } else { models::find(&d.model).unwrap_or_else(|| models::default_for(d.kind)) };
            let mut detail = String::new();
            if let Some(gw) = d.gateway {
                let _ = write!(detail, "default gateway {gw}");
            }
            if let Some(gw) = d.gateway6 {
                let _ = write!(detail, "{}default gateway {gw}", if detail.is_empty() { "" } else { ", " });
            }
            rows.push(Row::Device(format!("{}   {} ({})", clean(&d.name), model.label, d.kind.label()), detail));
            if d.kind == DeviceKind::Switch && d.vlans.len() > 1 {
                let vlans: Vec<String> = d.vlans.iter().take(16).map(|(id, name)| format!("{} {}", id.get(), clean(name))).collect();
                let more = if d.vlans.len() > 16 { ", ..." } else { "" };
                rows.push(Row::Note(format!("VLANs: {}{more}", vlans.join(", "))));
            }
            let mut unused = 0usize;
            let mut any = false;
            for (i, port) in d.ports.iter().enumerate() {
                if rows.len() >= MAX_ROWS {
                    cut = true;
                    break 'groups;
                }
                let linked = t.peer(End { device: d.id, port: i }).is_some();
                if !is_listed(d.kind, port, linked) {
                    unused += 1;
                    continue;
                }
                any = true;
                let ip = match (&port.ip, port.dhcp) {
                    (Some(ip), true) => format!("{ip} (DHCP)"),
                    (Some(ip), false) => ip.to_string(),
                    (None, true) => "DHCP (no lease)".to_string(),
                    (None, false) => String::new(),
                };
                rows.push(Row::Cells(vec![
                    clean(&port.name),
                    or_dash(ip),
                    ipv6_cell(port),
                    mode_cell(d.kind, port),
                    status_cell(port, linked).to_string(),
                    port.mac.to_string(),
                ]));
                for sub in port.subifs.iter().take(64) {
                    let status = if sub.shutdown { "admin down" } else { "connected" };
                    rows.push(Row::Cells(vec![
                        format!("{}.{}", clean(&port.name), sub.number),
                        or_dash(sub.ip.map(|i| i.to_string()).unwrap_or_default()),
                        "-".to_string(),
                        sub.vlan.map_or_else(|| "-".to_string(), |v| format!("dot1Q {}", v.get())),
                        status.to_string(),
                        "-".to_string(),
                    ]));
                }
            }
            if !any {
                rows.push(Row::Note("No cabled or configured interfaces.".to_string()));
            } else if unused > 0 {
                rows.push(Row::Note(format!("{unused} other interface(s) not cabled or configured.")));
            }
        }
    }
    if cut {
        rows.push(Row::Note(format!("Report cut: the lab has {} devices; the list stops here.", t.devices.len())));
    }

    // The cables.
    let mut links_cut = false;
    if !t.links.is_empty() {
        rows.push(Row::Section("Links".to_string()));
        rows.push(Row::Header(vec![("Device / port".to_string(), 250.0), ("Device / port".to_string(), 250.0), ("Cable".to_string(), 150.0)]));
        let end = |e: End| match t.device(e.device) {
            Some(d) => format!("{} / {}", clean(&d.name), d.ports.get(e.port).map_or("?", |p| p.name.as_str())),
            None => "?".to_string(),
        };
        for l in t.links.iter().take(MAX_LINKS) {
            rows.push(Row::Cells(vec![end(l.a), end(l.b), t.resolved_cable(l).label().to_string()]));
        }
        links_cut = t.links.len() > MAX_LINKS;
        if links_cut {
            rows.push(Row::Note(format!("{} more cable(s) not listed.", t.links.len() - MAX_LINKS)));
        }
    }
    (rows, cut || links_cut)
}

// ---------------------------------------------------------------------------------------------
// Pages.

/// Builds the report pages' content streams.
struct Pager {
    pages: Vec<String>,
    cur: String,
    y: f32,
    title: String,
    cols: Vec<f32>,
    header: Option<Vec<(String, f32)>>,
    full: bool,
}

impl Pager {
    fn new(title: &str) -> Pager {
        let mut p = Pager { pages: Vec::new(), cur: String::new(), y: 0.0, title: title.to_string(), cols: Vec::new(), header: None, full: false };
        p.start(true);
        p
    }

    fn text(&mut self, x: f32, y: f32, size: f32, bold: bool, s: &str) {
        let font = if bold { "F2" } else { "F1" };
        let _ = writeln!(self.cur, "BT /{font} {size} Tf {x:.1} {y:.1} Td {} Tj ET", pdf_text(s));
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, gray: f32) {
        let _ = writeln!(self.cur, "{gray} G 0.5 w {x1:.1} {y1:.1} m {x2:.1} {y2:.1} l S");
    }

    fn fill(&mut self, y: f32, h: f32, w: f32, gray: f32) {
        let _ = writeln!(self.cur, "{gray} g {MARGIN} {y:.1} {w:.1} {h:.1} re f 0 g");
    }

    /// A new page; the first one carries the report's title.
    fn start(&mut self, first: bool) {
        if !self.cur.is_empty() {
            self.pages.push(std::mem::take(&mut self.cur));
        }
        if self.pages.len() >= MAX_REPORT_PAGES {
            self.full = true;
            return;
        }
        let top = PAGE.1 - MARGIN;
        if first {
            let s = fit(&format!("Network report: {}", self.title), PAGE.0 - 2.0 * MARGIN, 14.0, true);
            self.text(MARGIN, top - 14.0, 14.0, true, &s);
            self.y = top - 30.0;
        } else {
            let s = fit(&format!("Network report: {} (continued)", self.title), PAGE.0 - 2.0 * MARGIN, 9.0, false);
            self.text(MARGIN, top - 9.0, 9.0, false, &s);
            self.y = top - 22.0;
        }
    }

    /// Room for `need` points: a new page (with the table header again) when there is not.
    fn ensure(&mut self, need: f32) {
        if self.full || self.y - need >= BOTTOM {
            return;
        }
        self.start(false);
        if let Some(h) = self.header.clone() {
            self.header_row(&h);
        }
    }

    fn header_row(&mut self, cols: &[(String, f32)]) {
        if self.full {
            return;
        }
        self.cols = cols.iter().map(|(_, w)| *w).collect();
        let total: f32 = self.cols.iter().sum();
        let y = self.y - 14.0;
        self.fill(y, 14.0, total, 0.85);
        let mut x = MARGIN;
        for (title, w) in cols {
            let s = fit(title, w - 6.0, FONT, true);
            self.text(x + 3.0, y + 4.0, FONT, true, &s);
            x += w;
        }
        self.y = y;
    }

    fn cells(&mut self, cells: &[String]) {
        if self.full {
            return;
        }
        let y = self.y - ROW;
        let mut x = MARGIN;
        let cols = self.cols.clone();
        let total: f32 = cols.iter().sum();
        self.line(MARGIN, y, MARGIN + total, y, 0.7);
        self.line(x, y, x, y + ROW, 0.7);
        for (cell, w) in cells.iter().zip(&cols) {
            let s = fit(cell, w - 6.0, FONT, false);
            self.text(x + 3.0, y + 3.5, FONT, false, &s);
            x += w;
            self.line(x, y, x, y + ROW, 0.7);
        }
        self.y = y;
    }

    fn row(&mut self, row: &Row) {
        if self.full {
            return;
        }
        match row {
            Row::Section(s) => {
                self.y -= 6.0;
                let y = self.y - 16.0;
                let s = fit(s, 600.0, 12.0, true);
                self.text(MARGIN, y + 4.0, 12.0, true, &s);
                self.y = y;
                self.header = None;
            }
            Row::Group(s) => {
                self.y -= 4.0;
                let y = self.y - 14.0;
                let s = fit(s, 600.0, 10.0, true);
                self.text(MARGIN, y + 3.0, 10.0, true, &s);
                self.y = y;
            }
            Row::Header(h) => {
                self.header = Some(h.clone());
                self.header_row(h);
            }
            Row::Device(name, detail) => {
                let total: f32 = self.cols.iter().sum();
                let y = self.y - 15.0;
                self.fill(y, 15.0, total, 0.94);
                let left = fit(name, total * 0.6, 9.0, true);
                self.text(MARGIN + 3.0, y + 4.5, 9.0, true, &left);
                let right = fit(detail, total * 0.4 - 8.0, FONT, false);
                let w = text_width(&right, FONT, false);
                self.text(MARGIN + total - w - 4.0, y + 4.5, FONT, false, &right);
                self.line(MARGIN, y, MARGIN + total, y, 0.7);
                self.y = y;
            }
            Row::Cells(c) => self.cells(c),
            Row::Note(s) => {
                let y = self.y - ROW;
                let s = fit(s, PAGE.0 - 2.0 * MARGIN - 8.0, FONT, false);
                self.text(MARGIN + 3.0, y + 3.5, FONT, false, &s);
                self.y = y;
            }
        }
    }

    fn height(row: &Row) -> f32 {
        match row {
            Row::Section(_) => 22.0,
            Row::Group(_) => 18.0,
            Row::Header(_) => 14.0,
            Row::Device(..) => 15.0,
            Row::Cells(_) | Row::Note(_) => ROW,
        }
    }

    /// Lays out `rows`, keeping a title with the rows after it.
    fn lay_out(&mut self, rows: &[Row]) {
        let is_title = |r: &Row| matches!(r, Row::Section(_) | Row::Group(_) | Row::Header(_) | Row::Device(..));
        for (i, row) in rows.iter().enumerate() {
            if self.full {
                break;
            }
            let mut need = Self::height(row);
            if is_title(row) {
                // The row, the titles that follow it directly, and the first two lines under them.
                let mut body = 0;
                for next in rows.iter().skip(i + 1) {
                    need += Self::height(next);
                    if !is_title(next) {
                        body += 1;
                        if body == 2 {
                            break;
                        }
                    }
                }
            }
            self.ensure(need);
            self.row(row);
        }
    }

    /// The finished content streams, one per page.
    fn finish(mut self) -> Vec<String> {
        if self.full {
            let _ = writeln!(self.cur, "BT /F1 {FONT} Tf {MARGIN} {} Td {} Tj ET", BOTTOM - 2.0, pdf_text("Report cut: page limit reached."));
        }
        if !self.cur.is_empty() {
            self.pages.push(std::mem::take(&mut self.cur));
        }
        self.pages
    }
}

/// The footer of every page: a line, `Generated with NetLabCraft` and `Page N of M`.
fn footer(n: usize, of: usize) -> String {
    let page = format!("Page {n} of {of}");
    let w = text_width(&page, FONT, false);
    let mut s = format!("q 0.6 G 0.5 w {MARGIN} {} m {} {} l S 0.35 g\n", FOOTER_Y + 12.0, PAGE.0 - MARGIN, FOOTER_Y + 12.0);
    let _ = writeln!(s, "BT /F1 {FONT} Tf {MARGIN} {FOOTER_Y} Td {} Tj ET", pdf_text("Generated with NetLabCraft"));
    let _ = writeln!(s, "BT /F1 {FONT} Tf {:.1} {FOOTER_Y} Td {} Tj ET Q", PAGE.0 - MARGIN - w, pdf_text(&page));
    s
}

// ---------------------------------------------------------------------------------------------
// The file.

/// Collects the objects of a PDF file and writes the cross-reference table.
struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    fn new() -> Writer {
        Writer { out: b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec(), offsets: Vec::new() }
    }

    /// The next object (numbered from 1): `body` is its dictionary, `stream` its data.
    fn object(&mut self, body: &str, stream: Option<&[u8]>) {
        self.offsets.push(self.out.len());
        self.out.extend_from_slice(format!("{} 0 obj\n{body}", self.offsets.len()).as_bytes());
        if let Some(s) = stream {
            self.out.extend_from_slice(b"\nstream\n");
            self.out.extend_from_slice(s);
            self.out.extend_from_slice(b"\nendstream");
        }
        self.out.extend_from_slice(b"\nendobj\n");
    }

    /// A stream object holding `data` Deflate-compressed.
    fn compressed(&mut self, dict: &str, data: &[u8]) {
        let z = deflate(data);
        self.object(&format!("<< {dict} /Filter /FlateDecode /Length {} >>", z.len()), Some(&z));
    }

    fn finish(mut self, trailer: &str) -> Vec<u8> {
        let xref = self.out.len();
        let size = self.offsets.len() + 1;
        let mut tail = format!("xref\n0 {size}\n0000000000 65535 f \n");
        for o in &self.offsets {
            let _ = writeln!(tail, "{o:010} 00000 n ");
        }
        let _ = write!(tail, "trailer\n<< /Size {size} {trailer} >>\nstartxref\n{xref}\n%%EOF\n");
        self.out.extend_from_slice(tail.as_bytes());
        self.out
    }
}

/// What the file says about itself.
struct Meta {
    title: String,
    /// `2026-11-14T09:30:00Z`, or empty when there is no clock.
    created: String,
}

const CREATOR: &str = concat!("NetLabCraft ", env!("CARGO_PKG_VERSION"));
const SUBJECT: &str = "Network lab report generated with NetLabCraft";
const KEYWORDS: &str = "network, lab, topology, report, NetLabCraft";

impl Meta {
    /// The Info dictionary.
    fn info(&self) -> String {
        let mut d = format!(
            "<< /Title {} /Creator {} /Producer {} /Subject {} /Keywords {}",
            info_string(&self.title),
            info_string(CREATOR),
            info_string(CREATOR),
            info_string(SUBJECT),
            info_string(KEYWORDS),
        );
        if let Some(digits) = date_digits(&self.created) {
            let date = format!("(D:{digits}+00'00')");
            let _ = write!(d, " /CreationDate {date} /ModDate {date}");
        }
        d.push_str(" >>");
        d
    }

    /// The XMP packet.
    fn xmp(&self) -> String {
        let mut s = String::from("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
        s.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
        s.push_str("<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n");
        let _ = writeln!(s, "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>", xml_escape(&self.title));
        let _ = writeln!(s, "<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{SUBJECT}</rdf:li></rdf:Alt></dc:description>");
        let _ = writeln!(s, "<pdf:Producer>{CREATOR}</pdf:Producer>\n<pdf:Keywords>{KEYWORDS}</pdf:Keywords>");
        let _ = writeln!(s, "<xmp:CreatorTool>{CREATOR}</xmp:CreatorTool>");
        if date_digits(&self.created).is_some() {
            let _ = writeln!(s, "<xmp:CreateDate>{0}</xmp:CreateDate>\n<xmp:ModifyDate>{0}</xmp:ModifyDate>", self.created);
        }
        s.push_str("</rdf:Description></rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>");
        s
    }
}

/// The print-out: page 1 is `title` over `rgb` (`width × height` pixels, 3 bytes each, rows top
/// to bottom) scaled to fit the page, keeping its proportions; the pages after it are the
/// report on `lab`. `created` is the time now as `2026-11-14T09:30:00Z` (empty: no date).
pub fn document(title: &str, created: &str, lab: &Topology, width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let meta = Meta { title: title.chars().take(MAX_TEXT).collect(), created: created.to_string() };
    let (w, h) = (width.max(1), height.max(1));
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(3);
    let mut pixels = rgb.get(..need.min(rgb.len())).unwrap_or_default().to_vec();
    pixels.resize(need, 255);

    let (room_w, room_h) = (PAGE.0 - 2.0 * MARGIN, PAGE.1 - 2.0 * MARGIN - TITLE);
    let scale = (room_w / w as f32).min(room_h / h as f32);
    let (dw, dh) = (w as f32 * scale, h as f32 * scale);
    let (x, y) = (MARGIN + (room_w - dw) / 2.0, MARGIN + (room_h - dh));
    let first = format!(
        "BT /F2 14 Tf {MARGIN} {:.1} Td {} Tj ET\nq {dw:.2} 0 0 {dh:.2} {x:.2} {y:.2} cm /Im1 Do Q\n",
        PAGE.1 - MARGIN - 14.0,
        pdf_text(&fit(&meta.title, room_w, 14.0, true))
    );

    let (rows, _cut) = report_rows(lab);
    let mut pager = Pager::new(&meta.title);
    pager.row(&Row::Note(format!(
        "{} device(s), {} cable(s). Interfaces that are neither cabled nor configured are counted, not listed.",
        lab.devices.len(),
        lab.links.len()
    )));
    pager.lay_out(&rows);
    let mut streams = vec![first];
    streams.extend(pager.finish());
    let total = streams.len();

    // Object numbers: 1 catalog, 2 page tree, 3 and 4 fonts, 5 info, 6 XMP, 7 image, then a
    // page and its content stream per page.
    let kids: Vec<String> = (0..total).map(|i| format!("{} 0 R", 8 + 2 * i)).collect();
    let mut f = Writer::new();
    f.object("<< /Type /Catalog /Pages 2 0 R /Metadata 6 0 R >>", None);
    f.object(&format!("<< /Type /Pages /Kids [{}] /Count {total} >>", kids.join(" ")), None);
    f.object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>", None);
    f.object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>", None);
    f.object(&meta.info(), None);
    let xmp = meta.xmp();
    f.object(&format!("<< /Type /Metadata /Subtype /XML /Length {} >>", xmp.len()), Some(xmp.as_bytes()));
    f.compressed(&format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8"), &pixels);
    for (i, content) in streams.iter().enumerate() {
        let mut body = content.clone();
        body.push_str(&footer(i + 1, total));
        f.object(
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /XObject << /Im1 7 0 R >> >> /Contents {} 0 R >>",
                PAGE.0,
                PAGE.1,
                9 + 2 * i
            ),
            None,
        );
        f.compressed("", body.as_bytes());
    }
    f.finish("/Root 1 0 R /Info 5 0 R")
}

#[cfg(test)]
mod tests {
    use super::*;
    use netlabcraft_core::{Ipv4Net, VlanId};

    const NOW: &str = "2026-11-14T09:30:05Z";

    fn has(pdf: &[u8], needle: &str) -> bool {
        pdf.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    /// The data of every stream object.
    fn streams(pdf: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(i) = pdf[at..].windows(8).position(|w| w == b"\nstream\n") {
            let start = at + i + 8;
            let Some(end) = pdf[start..].windows(10).position(|w| w == b"\nendstream") else { break };
            out.push(pdf[start..start + end].to_vec());
            at = start + end + 10;
        }
        out
    }

    fn inflated(pdf: &[u8]) -> String {
        streams(pdf)
            .iter()
            .filter_map(|s| miniz_oxide::inflate::decompress_to_vec_zlib(s).ok())
            .filter(|d| d.len() < 1_000_000)
            .map(|d| String::from_utf8_lossy(&d).into_owned())
            .collect::<Vec<_>>()
            .join("\n--\n")
    }

    fn lab() -> Topology {
        let mut t = Topology::default();
        let r = t.add_device(DeviceKind::Router, Some("R1"), 0.0, 0.0).unwrap();
        let s = t.add_device(DeviceKind::Switch, Some("SW1"), 0.0, 0.0).unwrap();
        let p = t.add_device(DeviceKind::Pc, Some("PC1"), 0.0, 0.0).unwrap();
        t.connect(End { device: r, port: 0 }, End { device: s, port: 0 }).unwrap();
        t.connect(End { device: p, port: 0 }, End { device: s, port: 1 }).unwrap();
        if let Some(d) = t.devices.iter_mut().find(|d| d.id == r) {
            d.ports[0].ip = "192.168.1.1/24".parse::<Ipv4Net>().ok();
        }
        t
    }

    /// Checks every cross-reference entry and `startxref`; returns the number of objects.
    fn xref_ok(pdf: &[u8]) -> usize {
        let xref = pdf.windows(6).rposition(|w| w == b"\nxref\n").unwrap() + 1;
        let table = String::from_utf8(pdf[xref..].to_vec()).unwrap();
        let size: usize = table.lines().nth(1).unwrap().strip_prefix("0 ").unwrap().parse().unwrap();
        for (i, line) in table.lines().skip(3).take(size - 1).enumerate() {
            let at: usize = line[..10].parse().unwrap();
            assert!(pdf[at..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
        let start: usize = table.split("startxref\n").nth(1).unwrap().lines().next().unwrap().parse().unwrap();
        assert_eq!(start, xref);
        size - 1
    }

    #[test]
    fn a_picture_page_and_a_report_with_a_valid_cross_reference_table() {
        let pdf = document("Lab (draft) \\ niño", NOW, &lab(), 2, 1, &[255, 0, 0, 0, 0, 255]);
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        let objects = xref_ok(&pdf);
        // 7 fixed objects, then two per page.
        assert_eq!((objects - 7) % 2, 0);
        let pages = (objects - 7) / 2;
        assert!(pages >= 2, "a picture page and a report page");
        assert!(has(&pdf, &format!("/Count {pages} ")));
        assert_eq!(pdf.windows(12).filter(|w| w == b"/Type /Page ").count(), pages);
        let text = inflated(&pdf);
        assert!(text.contains("(Lab \\(draft\\) \\\\ ni\\361o)"), "title escaped, Latin-1 as an octal escape");
        assert!(text.contains("(R1   ") && text.contains("(SW1   ") && text.contains("(192.168.1.1/24)"));
        assert!(text.contains("(Generated with NetLabCraft)") && text.contains(&format!("(Page 1 of {pages})")));
        // Hostile sizes don't panic: short pixel data is padded white, nothing is huge.
        assert!(!document("", "", &Topology::default(), 0, 0, &[]).is_empty());
        assert!(!document("", "", &Topology::default(), 3, 3, &[1]).is_empty());
    }

    #[test]
    fn the_file_describes_itself() {
        let pdf = document("My lab (v2) \\ \u{1f4e1}\n", NOW, &lab(), 2, 1, &[0; 6]);
        assert!(has(&pdf, &format!("/Creator ({CREATOR})")) && has(&pdf, &format!("/Producer ({CREATOR})")));
        assert!(CREATOR.starts_with("NetLabCraft 0."));
        assert!(has(&pdf, "/Subject (Network lab report generated with NetLabCraft)"));
        assert!(has(&pdf, "/CreationDate (D:20261114093005+00'00')"));
        // A title with a non-Latin character is a UTF-16BE hex string.
        assert!(has(&pdf, "/Title <FEFF"));
        assert!(has(&pdf, "<xmp:CreateDate>2026-11-14T09:30:05Z</xmp:CreateDate>") && has(&pdf, "/Metadata 6 0 R"));
        // A plain title and no clock: a literal string, escaped, and no date at all.
        let plain = document("A (b) \\ c", "", &lab(), 1, 1, &[0; 3]);
        assert!(has(&plain, "/Title (A \\(b\\) \\\\ c)") && !has(&plain, "/CreationDate"));
        assert!(date_digits("nonsense").is_none() && date_digits(NOW).as_deref() == Some("20261114093005"));
        assert!(!xml_escape("a<b&\"c\"").contains('<') && xml_escape("a<b").contains("&lt;"));
    }

    #[test]
    fn the_picture_is_compressed() {
        // A flat picture 1200 x 800: 2.88 MB of pixels.
        let rgb: Vec<u8> = (0..1200 * 800).flat_map(|i| if i % 997 < 3 { [20, 20, 20] } else { [200, 220, 240] }).collect();
        let pdf = document("Lab", NOW, &lab(), 1200, 800, &rgb);
        assert!(pdf.len() < rgb.len() / 20, "{} bytes for {} of pixels", pdf.len(), rgb.len());
        // The image still inflates to its pixels.
        assert!(streams(&pdf).iter().filter_map(|s| miniz_oxide::inflate::decompress_to_vec_zlib(s).ok()).any(|d| d == rgb));
    }

    #[test]
    fn a_huge_lab_is_capped() {
        let mut t = Topology::default();
        while t.add_device(DeviceKind::Pc, None, 0.0, 0.0).is_ok() {}
        assert!(t.devices.len() > MAX_DEVICES);
        for d in &mut t.devices {
            d.ports[0].description = "x".repeat(5_000);
        }
        let pdf = document("Huge", NOW, &t, 4, 4, &[0; 48]);
        let pages = xref_ok(&pdf).saturating_sub(7) / 2;
        assert!(pages <= MAX_REPORT_PAGES + 1, "{pages} pages");
        assert!(pdf.len() < 2_000_000, "{} bytes", pdf.len());
        assert!(inflated(&pdf).contains("Report cut"));
    }

    #[test]
    fn the_report_lists_what_the_model_holds() {
        let mut t = lab();
        let sw = t.devices.iter().position(|d| d.name == "SW1").unwrap();
        t.devices[sw].ports[1].switchport.access_vlan = VlanId::new(10).unwrap();
        t.devices[sw].ports[0].switchport.mode = SwitchportMode::Trunk;
        let (rows, cut) = report_rows(&t);
        assert!(!cut);
        let cells: Vec<&Vec<String>> = rows.iter().filter_map(|r| if let Row::Cells(c) = r { Some(c) } else { None }).collect();
        assert!(cells.iter().any(|c| c.get(1).is_some_and(|v| v == "192.168.1.1/24")));
        assert!(cells.iter().any(|c| c.get(3).is_some_and(|v| v == "access VLAN 10")));
        assert!(cells.iter().any(|c| c.get(3).is_some_and(|v| v.starts_with("trunk native 1"))));
        assert!(cells.iter().any(|c| c.len() == 3 && c.first().is_some_and(|v| v.starts_with("R1 / "))));
        assert!(rows.iter().any(|r| matches!(r, Row::Note(n) if n.contains("not cabled"))));
    }
}
