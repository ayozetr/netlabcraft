//! Colours. Every colour the UI paints comes from [`Tokens`]. The layout and palette follow the
//! look of the reference app (navy workspace bars, light grey tool areas, white workspace);
//! the values are our own.

use egui::Color32;
use netlabcraft_model::DeviceKind;
use netlabcraft_wire::Protocol;

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub canvas: Color32,
    pub grid: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub cable: Color32,
    /// Fiber cables (orange), console cables (light blue), serial cables (red).
    pub fiber_cable: Color32,
    pub console_cable: Color32,
    pub serial_cable: Color32,
    /// IoT custom cables (a board's pin to a smart thing).
    pub iot_cable: Color32,
    /// The connector pictures on the Connections tiles: console, copper straight-through and
    /// octal, cross-over, fiber and serial, phone and IoT sheath, coaxial and bare copper, USB.
    pub tile_sky: Color32,
    pub tile_green: Color32,
    pub tile_orange: Color32,
    pub tile_blue: Color32,
    pub tile_grey: Color32,
    pub tile_gold: Color32,
    pub tile_dark: Color32,
    pub link_up: Color32,
    pub link_down: Color32,
    /// A port the spanning tree blocks.
    pub link_blocked: Color32,
    pub selection: Color32,
    pub device_fill: Color32,
    pub error: Color32,
    pub console_bg: Color32,
    pub console_text: Color32,
    /// The workspace and time bars.
    pub bar: Color32,
    pub bar_text: Color32,
    /// The active pill on a bar (Logical, Realtime…).
    pub pill: Color32,
    pub pill_text: Color32,
    /// Toolbars and the bottom device box.
    pub chrome: Color32,
    pub chrome_line: Color32,
    pub glyph: Color32,
    pub ok: Color32,
    /// A cluster's cloud.
    pub cluster: Color32,
    /// The physical view's ground, and the fill of its places (cities, buildings, closets).
    pub ground: Color32,
    pub place: Color32,
    /// The connectors drawn on front panels in the closet: the metal housing, the dark opening,
    /// the gold contacts and the amber activity light.
    pub port_rim: Color32,
    pub port_hole: Color32,
    pub port_contact: Color32,
    pub port_activity: Color32,
    /// The paints the device pictures are drawn with (smart things, components, boards, the near-duplicate
    /// models): the outline, white plastic, bare metal, glass, the accent colours and the two board greens.
    pub art_ink: Color32,
    /// The outline of the pictures: the ink on the light theme, a lighter slate on the dark one so thin parts keep their edge.
    pub art_edge: Color32,
    pub art_paper: Color32,
    pub art_steel: Color32,
    pub art_glass: Color32,
    pub art_red: Color32,
    pub art_amber: Color32,
    pub art_green: Color32,
    pub art_blue: Color32,
    pub art_earth: Color32,
    pub art_pcb_green: Color32,
    pub art_pcb_blue: Color32,
}

pub const LIGHT: Tokens = Tokens {
    canvas: Color32::from_rgb(0xff, 0xff, 0xff),
    grid: Color32::from_rgb(0xe9, 0xec, 0xf0),
    text: Color32::from_rgb(0x1a, 0x1d, 0x22),
    muted: Color32::from_rgb(0x5f, 0x66, 0x70),
    cable: Color32::from_rgb(0x14, 0x14, 0x14),
    fiber_cable: Color32::from_rgb(0xe8, 0x76, 0x0c),
    console_cable: Color32::from_rgb(0x2c, 0xa4, 0xe0),
    serial_cable: Color32::from_rgb(0xc8, 0x3a, 0x2e),
    iot_cable: Color32::from_rgb(0x9a, 0x4c, 0xc8),
    tile_sky: Color32::from_rgb(0x4a, 0xb4, 0xea),
    tile_green: Color32::from_rgb(0x4c, 0xb0, 0x3c),
    tile_orange: Color32::from_rgb(0xec, 0x7c, 0x1c),
    tile_blue: Color32::from_rgb(0x2a, 0x6c, 0xd0),
    tile_grey: Color32::from_rgb(0x9a, 0xa0, 0xa8),
    tile_gold: Color32::from_rgb(0xc8, 0x92, 0x2c),
    tile_dark: Color32::from_rgb(0x3a, 0x3f, 0x48),
    link_up: Color32::from_rgb(0x3c, 0xc8, 0x2c),
    link_down: Color32::from_rgb(0xe0, 0x2a, 0x2a),
    link_blocked: Color32::from_rgb(0xf0, 0x9a, 0x1a),
    selection: Color32::from_rgb(0x1e, 0x6f, 0xd9),
    device_fill: Color32::from_rgb(0xff, 0xff, 0xff),
    error: Color32::from_rgb(0xc6, 0x28, 0x28),
    console_bg: Color32::from_rgb(0xff, 0xff, 0xff),
    console_text: Color32::from_rgb(0x10, 0x10, 0x10),
    bar: Color32::from_rgb(0x0f, 0x2a, 0x4e),
    bar_text: Color32::from_rgb(0xff, 0xff, 0xff),
    pill: Color32::from_rgb(0x1c, 0x6e, 0xd4),
    pill_text: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome_line: Color32::from_rgb(0xd2, 0xd5, 0xda),
    glyph: Color32::from_rgb(0x1e, 0x20, 0x24),
    ok: Color32::from_rgb(0x2e, 0x9e, 0x3e),
    cluster: Color32::from_rgb(0xcf, 0xe2, 0xf6),
    ground: Color32::from_rgb(0xf3, 0xf6, 0xee),
    place: Color32::from_rgb(0xe6, 0xdc, 0xc8),
    port_rim: Color32::from_rgb(0xb4, 0xba, 0xc2),
    port_hole: Color32::from_rgb(0x14, 0x17, 0x1b),
    port_contact: Color32::from_rgb(0xd9, 0xa8, 0x2b),
    port_activity: Color32::from_rgb(0xf2, 0xa3, 0x1b),
    art_ink: Color32::from_rgb(0x1c, 0x26, 0x34),
    art_edge: Color32::from_rgb(0x1c, 0x26, 0x34),
    art_paper: Color32::from_rgb(0xf6, 0xf8, 0xfa),
    art_steel: Color32::from_rgb(0xa9, 0xb4, 0xc0),
    art_glass: Color32::from_rgb(0xcf, 0xe6, 0xf5),
    art_red: Color32::from_rgb(0xd6, 0x45, 0x3a),
    art_amber: Color32::from_rgb(0xf2, 0xb8, 0x2e),
    art_green: Color32::from_rgb(0x2e, 0x9e, 0x5a),
    art_blue: Color32::from_rgb(0x2f, 0x7f, 0xd0),
    art_earth: Color32::from_rgb(0x8a, 0x5a, 0x2e),
    art_pcb_green: Color32::from_rgb(0x1f, 0x7a, 0x4a),
    art_pcb_blue: Color32::from_rgb(0x1f, 0x58, 0x9a),
};

pub const DARK: Tokens = Tokens {
    canvas: Color32::from_rgb(0x1e, 0x21, 0x27),
    grid: Color32::from_rgb(0x2a, 0x2e, 0x36),
    text: Color32::from_rgb(0xe4, 0xe8, 0xee),
    muted: Color32::from_rgb(0x9a, 0xa2, 0xae),
    cable: Color32::from_rgb(0xd0, 0xd4, 0xda),
    fiber_cable: Color32::from_rgb(0xff, 0x9a, 0x3c),
    console_cable: Color32::from_rgb(0x7c, 0xcc, 0xff),
    serial_cable: Color32::from_rgb(0xff, 0x6e, 0x5e),
    iot_cable: Color32::from_rgb(0xc8, 0x8c, 0xf0),
    tile_sky: Color32::from_rgb(0x6c, 0xc8, 0xf6),
    tile_green: Color32::from_rgb(0x5c, 0xc8, 0x4c),
    tile_orange: Color32::from_rgb(0xff, 0x94, 0x38),
    tile_blue: Color32::from_rgb(0x4c, 0x8c, 0xf0),
    tile_grey: Color32::from_rgb(0xb4, 0xba, 0xc4),
    tile_gold: Color32::from_rgb(0xe0, 0xae, 0x48),
    tile_dark: Color32::from_rgb(0x9a, 0xa2, 0xae),
    link_up: Color32::from_rgb(0x4c, 0xdc, 0x3c),
    link_down: Color32::from_rgb(0xf0, 0x5a, 0x5a),
    link_blocked: Color32::from_rgb(0xff, 0xb0, 0x3a),
    selection: Color32::from_rgb(0x5a, 0x9c, 0xff),
    device_fill: Color32::from_rgb(0x2a, 0x2f, 0x38),
    error: Color32::from_rgb(0xff, 0x6b, 0x6b),
    console_bg: Color32::from_rgb(0x12, 0x14, 0x18),
    console_text: Color32::from_rgb(0xe0, 0xe4, 0xea),
    bar: Color32::from_rgb(0x0b, 0x1d, 0x36),
    bar_text: Color32::from_rgb(0xe8, 0xee, 0xf6),
    pill: Color32::from_rgb(0x1f, 0x63, 0xbd),
    pill_text: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome: Color32::from_rgb(0x26, 0x29, 0x30),
    chrome_line: Color32::from_rgb(0x3a, 0x3f, 0x48),
    glyph: Color32::from_rgb(0xdd, 0xe2, 0xe8),
    ok: Color32::from_rgb(0x4c, 0xc8, 0x5c),
    cluster: Color32::from_rgb(0x3a, 0x58, 0x78),
    ground: Color32::from_rgb(0x1f, 0x25, 0x22),
    place: Color32::from_rgb(0x6a, 0x63, 0x58),
    port_rim: Color32::from_rgb(0x8b, 0x92, 0x9c),
    port_hole: Color32::from_rgb(0x0c, 0x0e, 0x11),
    port_contact: Color32::from_rgb(0xe6, 0xb8, 0x3c),
    port_activity: Color32::from_rgb(0xff, 0xb4, 0x3a),
    art_ink: Color32::from_rgb(0x10, 0x15, 0x1c),
    art_edge: Color32::from_rgb(0x8a, 0x99, 0xad),
    art_paper: Color32::from_rgb(0xe8, 0xec, 0xf0),
    art_steel: Color32::from_rgb(0x9a, 0xa6, 0xb4),
    art_glass: Color32::from_rgb(0xb4, 0xd2, 0xe6),
    art_red: Color32::from_rgb(0xea, 0x5a, 0x4e),
    art_amber: Color32::from_rgb(0xf6, 0xc6, 0x48),
    art_green: Color32::from_rgb(0x4c, 0xc0, 0x76),
    art_blue: Color32::from_rgb(0x4a, 0x96, 0xe6),
    art_earth: Color32::from_rgb(0x9c, 0x6c, 0x3c),
    art_pcb_green: Color32::from_rgb(0x24, 0x8a, 0x54),
    art_pcb_blue: Color32::from_rgb(0x2a, 0x68, 0xb0),
};

pub fn tokens(dark: bool) -> Tokens {
    if dark { DARK } else { LIGHT }
}

/// The body colour of a device kind's icon.
pub fn device_color(kind: DeviceKind) -> Color32 {
    match kind {
        DeviceKind::Router => Color32::from_rgb(0x2d, 0x7f, 0xb8),
        DeviceKind::Switch => Color32::from_rgb(0x2f, 0x8f, 0xa8),
        DeviceKind::Hub => Color32::from_rgb(0x5a, 0x86, 0x9c),
        DeviceKind::Pc | DeviceKind::Laptop => Color32::from_rgb(0x4b, 0x83, 0xb5),
        DeviceKind::Server => Color32::from_rgb(0x2a, 0x8e, 0x9e),
        DeviceKind::Printer => Color32::from_rgb(0x5c, 0x7a, 0x96),
        DeviceKind::Phone => Color32::from_rgb(0x3f, 0x6f, 0x8f),
        DeviceKind::Iot => Color32::from_rgb(0x3a, 0x8f, 0x7a),
    }
}

/// The colour of a protocol in the event list and on the packets drawn on the canvas.
pub fn protocol_color(p: Protocol) -> Color32 {
    match p {
        Protocol::Arp => Color32::from_rgb(0xe8, 0xa3, 0x1c),
        Protocol::Icmp => Color32::from_rgb(0xc7, 0x4f, 0xc9),
        Protocol::Dhcp => Color32::from_rgb(0x2e, 0xa0, 0x8f),
        Protocol::Dns => Color32::from_rgb(0x8a, 0x6d, 0xd8),
        Protocol::Http => Color32::from_rgb(0x3c, 0x9c, 0x3c),
        Protocol::Tcp => Color32::from_rgb(0x5d, 0x7f, 0xa8),
        Protocol::Udp => Color32::from_rgb(0x3f, 0x8f, 0xe0),
        Protocol::Ipv4 => Color32::from_rgb(0x3b, 0xa3, 0x6b),
        Protocol::Stp => Color32::from_rgb(0x6f, 0x8a, 0x2c),
        Protocol::Ospf => Color32::from_rgb(0xb0, 0x5a, 0x2a),
        Protocol::Rip => Color32::from_rgb(0x7a, 0x5c, 0xa8),
        Protocol::Eigrp => Color32::from_rgb(0xb8, 0x3a, 0x3a),
        Protocol::Lldp => Color32::from_rgb(0x4f, 0x7d, 0x7a),
        Protocol::Lacp => Color32::from_rgb(0x8c, 0x6a, 0x3f),
        Protocol::Radius => Color32::from_rgb(0xa8, 0x4f, 0x6b),
        Protocol::Ftp => Color32::from_rgb(0x2f, 0x7a, 0x55),
        Protocol::Smtp => Color32::from_rgb(0xb8, 0x6b, 0x2e),
        Protocol::Pop3 => Color32::from_rgb(0x6b, 0x4f, 0xa8),
        Protocol::Tacacs => Color32::from_rgb(0x8f, 0x3f, 0x5a),
        Protocol::Snmp => Color32::from_rgb(0x3f, 0x6f, 0x8f),
        Protocol::Hsrp => Color32::from_rgb(0x2a, 0x8a, 0xa8),
        Protocol::Ntp => Color32::from_rgb(0x9a, 0x7b, 0x2e),
        Protocol::Syslog => Color32::from_rgb(0x8a, 0x3a, 0x6a),
        Protocol::Tftp => Color32::from_rgb(0x3a, 0x6a, 0x3a),
        Protocol::Icmpv6 => Color32::from_rgb(0xd9, 0x5f, 0x8e),
        Protocol::Ndp => Color32::from_rgb(0xc9, 0x8a, 0x2e),
        Protocol::Ipv6 => Color32::from_rgb(0x2f, 0x8f, 0x8a),
        Protocol::Gre => Color32::from_rgb(0x4a, 0x6f, 0xb5),
        Protocol::Esp => Color32::from_rgb(0x9c, 0x3d, 0x3d),
        Protocol::Ah => Color32::from_rgb(0xb5, 0x6a, 0x3d),
        Protocol::Isakmp => Color32::from_rgb(0xb0, 0x5a, 0x7a),
        Protocol::Wlan => Color32::from_rgb(0x2e, 0x8b, 0x57),
        Protocol::Eapol => Color32::from_rgb(0xb5, 0x8a, 0x1f),
        Protocol::Capwap => Color32::from_rgb(0x5a, 0x6e, 0xc4),
        Protocol::Sip => Color32::from_rgb(0x2a, 0x9d, 0x8f),
        Protocol::Tls => Color32::from_rgb(0x1f, 0x7a, 0x5c),
        Protocol::Ssh => Color32::from_rgb(0x6a, 0x4c, 0x9c),
        Protocol::Cell => Color32::from_rgb(0xc2, 0x5b, 0x2a),
        Protocol::Pppoe => Color32::from_rgb(0x3d, 0x8f, 0xb8),
        Protocol::Other => Color32::from_rgb(0x88, 0x88, 0x88),
    }
}

/// Pale and dark variants of a colour, for the shaded icons.
/// The fill of a drawing (rectangle, ellipse, polygon) as shown: the colour itself on the light
/// workspace; in dark mode a deep tint of it, so the light names and port labels over it stay
/// readable.
pub fn drawing_fill(c: Color32, tk: &Tokens) -> Color32 {
    if tk.text.r() < 128 {
        return c;
    }
    let mix = |a: u8, b: u8| ((f32::from(a) * 0.3 + f32::from(b) * 0.7).round().clamp(0.0, 255.0)) as u8;
    Color32::from_rgb(mix(c.r(), tk.canvas.r()), mix(c.g(), tk.canvas.g()), mix(c.b(), tk.canvas.b()))
}

pub fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| -> u8 {
        let v = f32::from(v);
        (if f >= 1.0 { v + (255.0 - v) * (f - 1.0) } else { v * f }).clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// The relative luminance of a colour (WCAG 2.x), from 0 (black) to 1 (white).
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// The contrast ratio of two colours (WCAG 2.x), from 1 (the same) to 21 (black on white).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Near-white or near-black text, whichever reads better over `fill`.
pub fn readable_on(fill: Color32) -> Color32 {
    let (light, dark) = (Color32::from_rgb(0xf4, 0xf6, 0xf8), Color32::from_rgb(0x14, 0x17, 0x1b));
    if contrast(light, fill) >= contrast(dark, fill) { light } else { dark }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_is_symmetric_and_spans_one_to_twenty_one() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.05);
        assert!((contrast(Color32::RED, Color32::RED) - 1.0).abs() < 1e-4);
        let (a, b) = (Color32::from_rgb(10, 80, 160), Color32::from_rgb(240, 200, 20));
        assert!((contrast(a, b) - contrast(b, a)).abs() < 1e-4);
    }

    #[test]
    fn readable_text_contrasts_with_every_fill() {
        for fill in
            [Color32::BLACK, Color32::WHITE, LIGHT.canvas, DARK.canvas, Color32::from_rgb(0x1c, 0x1f, 0x24), Color32::from_rgb(0x44, 0x68, 0x96)]
        {
            assert!(contrast(readable_on(fill), fill) >= 4.5, "{fill:?}");
        }
    }
}
