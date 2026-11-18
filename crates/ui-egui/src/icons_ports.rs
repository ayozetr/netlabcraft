//! The connectors on the front panels of the wiring-closet view, drawn in code (original artwork):
//! an RJ45 jack with its latch notch and contacts, an SFP cage with its pull tab, a console jack in
//! its own colour, USB-A, a serial D-shell, a duplex LC fiber port, a coaxial F connector, an RJ11
//! phone jack, an IEC power inlet, a mains outlet, a pin-header socket and a radio mark. Which connector a
//! port gets depends only on its name and medium ([`connector_for`]); colours come from [`Tokens`].
//! Every connector is drawn into whatever rectangle it is given (the closet's small jacks, the device
//! window's large ones): the same code adds detail (a bevel, a contact comb, jackscrews, the SFP tab's
//! label) once the rectangle is big enough to show it.

use egui::{Color32, Painter, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use netlabcraft_model::cable::Medium;

use crate::theme::{Tokens, shade};

/// The kind of connector a port is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connector {
    /// A copper Ethernet jack.
    Rj45,
    /// An uplink with a copper jack and an SFP cage: the jack with a pull tab.
    Combo,
    /// An SFP or SFP+ cage (fiber-only ports).
    Sfp,
    /// A router's or switch's console jack (an RJ45 in its own colour).
    Console,
    /// A USB-A port.
    Usb,
    /// A WAN serial port (a D-shell).
    Serial,
    /// A PC's RS-232 port (a D-shell).
    Rs232,
    /// A duplex LC port (fiber patch panels and wall mounts).
    Lc,
    /// A coaxial F connector.
    Coax,
    /// An RJ11 phone jack.
    Rj11,
    /// A power cord inlet (IEC).
    Inlet,
    /// A mains outlet of a power strip.
    Outlet,
    /// A socket of a board's pin header (a jumper wire plugs into it).
    Pin,
    /// A radio (Wi-Fi or cellular): it takes no cable, so it is a mark, not a jack.
    Radio,
}

/// The connector a device port is drawn as; `None` for a port that takes no cable (a radio).
pub fn connector_for(name: &str, medium: Option<Medium>) -> Option<Connector> {
    if netlabcraft_model::cable::is_usb_port_name(name) {
        return Some(Connector::Usb);
    }
    let medium = medium.unwrap_or_else(|| netlabcraft_model::models::medium_by_name(name));
    match medium {
        Medium::Wireless => None,
        Medium::Copper => Some(Connector::Rj45),
        Medium::Combo => Some(Connector::Combo),
        Medium::Fiber => Some(Connector::Sfp),
        Medium::Console => Some(Connector::Console),
        Medium::Rs232 => Some(Connector::Rs232),
        Medium::Serial => Some(Connector::Serial),
        Medium::Coaxial => Some(Connector::Coax),
        Medium::Phone => Some(Connector::Rj11),
    }
}

/// What the device window draws for a port: its connector, or the radio mark for a port that takes no
/// cable (it is still a port: it has a state and can be pointed at).
pub fn panel_connector(name: &str, medium: Option<Medium>) -> Connector {
    connector_for(name, medium).unwrap_or(Connector::Radio)
}

/// Width over height of a connector's face, so a jack keeps its shape in whatever cell it is given.
pub fn aspect(c: Connector) -> f32 {
    match c {
        Connector::Rj45 | Connector::Combo | Connector::Console => 1.15,
        Connector::Sfp => 1.7,
        Connector::Usb => 2.3,
        Connector::Serial | Connector::Rs232 => 1.8,
        Connector::Lc => 1.9,
        Connector::Coax => 1.0,
        Connector::Rj11 => 0.95,
        Connector::Inlet => 1.25,
        Connector::Outlet => 1.2,
        Connector::Pin | Connector::Radio => 1.0,
    }
}

/// The height of a connector in the device window's front panel (its width follows from [`aspect`]):
/// large enough to show the latch, the contacts and the SFP tab's label.
pub fn panel_height(c: Connector) -> f32 {
    match c {
        Connector::Rj45 | Connector::Console | Connector::Combo | Connector::Rj11 => 34.0,
        Connector::Sfp => 28.0,
        Connector::Usb => 18.0,
        Connector::Serial | Connector::Rs232 => 24.0,
        Connector::Lc => 22.0,
        Connector::Coax => 30.0,
        Connector::Inlet => 28.0,
        Connector::Outlet => 30.0,
        Connector::Pin => 24.0,
        Connector::Radio => 28.0,
    }
}

/// The size of a connector in the device window's front panel.
pub fn panel_size(c: Connector) -> egui::Vec2 {
    let h = panel_height(c);
    vec2(h * aspect(c), h)
}

/// Whether `r` is big enough for the extra detail (bevels, comb, jackscrews, labels).
fn large(r: Rect) -> bool {
    r.width() >= 30.0
}

/// The largest rectangle of the connector's shape centred in `cell`.
pub fn fit(c: Connector, cell: Rect) -> Rect {
    let a = aspect(c);
    let (w, h) = if cell.width() / cell.height().max(0.01) > a { (cell.height() * a, cell.height()) } else { (cell.width(), cell.width() / a) };
    Rect::from_center_size(cell.center(), vec2(w, h))
}

fn edge(c: Color32) -> Stroke {
    Stroke::new(0.8, shade(c, 0.5))
}

/// Draws connector `c` in the largest shape that fits `cell`, centred; returns where it was drawn.
pub fn draw_in(p: &Painter, c: Connector, cell: Rect, used: bool, tk: &Tokens) -> Rect {
    let r = fit(c, cell);
    draw(p, c, r, used, tk);
    r
}

/// Draws connector `c` filling `r` (use [`fit`] to give it its shape). `used` darkens an outlet that
/// has a plug in it.
pub fn draw(p: &Painter, c: Connector, r: Rect, used: bool, tk: &Tokens) {
    match c {
        Connector::Rj45 => rj45(p, r, tk.port_rim, 8, tk),
        Connector::Console => rj45(p, r, tk.console_cable, 8, tk),
        Connector::Combo => {
            rj45(p, r, tk.port_rim, 8, tk);
            let tab = Rect::from_min_max(
                pos2(r.center().x - r.width() * 0.22, r.bottom() - r.height() * 0.1),
                pos2(r.center().x + r.width() * 0.22, r.bottom() + r.height() * if large(r) { 0.16 } else { 0.1 }),
            );
            p.rect_filled(tab, 0.5, tk.tile_blue);
            if large(r) {
                p.rect_stroke(tab, 0.5, edge(tk.tile_blue), StrokeKind::Inside);
            }
        }
        Connector::Rj11 => {
            let narrow = Rect::from_center_size(r.center(), vec2(r.width() * 0.66, r.height()));
            rj45(p, narrow, tk.tile_grey, 4, tk);
        }
        Connector::Sfp => sfp(p, r, tk),
        Connector::Lc => lc(p, r, tk),
        Connector::Usb => usb(p, r, tk),
        Connector::Serial => d_shell(p, r, tk.serial_cable, tk),
        Connector::Rs232 => d_shell(p, r, tk.port_rim, tk),
        Connector::Coax => coax(p, r, tk),
        Connector::Inlet => inlet(p, r, tk),
        Connector::Outlet => outlet(p, r, used, tk),
        Connector::Pin => pin(p, r, used, tk),
        Connector::Radio => radio(p, r, tk),
    }
}

/// An RJ45 jack: a housing in `rim`, a dark opening with the latch notch on top, a row of gold contacts.
/// Large: a lit bevel on the housing, the notch's shoulders and the plastic comb the contacts sit in.
fn rj45(p: &Painter, r: Rect, rim: Color32, contacts: usize, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    let big = large(r);
    let round = if big { 2.5 } else { 1.0 };
    p.rect_filled(r, round, rim);
    if big {
        let hi = Stroke::new(1.0, shade(rim, 1.35));
        p.line_segment([r.left_top() + vec2(round, 0.8), r.right_top() + vec2(-round, 0.8)], hi);
        p.line_segment([r.left_top() + vec2(0.8, round), r.left_bottom() + vec2(0.8, -round)], hi);
    }
    p.rect_stroke(r, round, edge(rim), StrokeKind::Inside);
    let o = r.shrink2(vec2(w * 0.12, h * 0.14));
    let notch_h = o.height() * 0.3;
    let body = Rect::from_min_max(pos2(o.left(), o.top() + notch_h), o.max);
    p.rect_filled(body, 0.5, tk.port_hole);
    let notch_w = o.width() * 0.4;
    p.rect_filled(
        Rect::from_min_max(pos2(o.center().x - notch_w / 2.0, o.top()), pos2(o.center().x + notch_w / 2.0, body.top() + 0.5)),
        0.0,
        tk.port_hole,
    );
    let ch = body.height() * 0.42;
    if big {
        // The comb the contacts sit in, and the shoulders either side of the latch notch.
        let comb = Rect::from_min_max(pos2(body.left() + 1.0, body.bottom() - ch * 1.25), pos2(body.right() - 1.0, body.bottom()));
        p.rect_filled(comb, 0.5, shade(tk.port_hole, 1.25));
        let shoulder = Stroke::new(0.8, shade(tk.port_hole, 1.5));
        for side in [-1.0, 1.0] {
            let x = o.center().x + side * notch_w / 2.0;
            p.line_segment([pos2(x, body.top()), pos2(x, o.top() + 0.5)], shoulder);
        }
    }
    if w >= 6.0 {
        let stroke = Stroke::new((w * 0.045).max(0.6), tk.port_contact);
        for i in 0..contacts {
            let x = body.left() + body.width() * (i as f32 + 0.5) / contacts as f32;
            p.line_segment([pos2(x, body.bottom() - ch), pos2(x, body.bottom() - 0.4)], stroke);
        }
    }
}

/// An SFP cage: a flat metal cage, a dark opening with the module's face and two bores, a coloured pull tab.
/// Large: the tab is taller and carries the word SFP, and the module's bail crosses its face.
fn sfp(p: &Painter, r: Rect, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    let big = large(r);
    p.rect_filled(r, 1.0, tk.port_rim);
    p.rect_stroke(r, 1.0, edge(tk.port_rim), StrokeKind::Inside);
    let o = r.shrink2(vec2(w * 0.08, h * 0.16));
    p.rect_filled(o, 0.5, tk.port_hole);
    let m = o.shrink2(vec2(o.width() * 0.08, o.height() * 0.14));
    p.rect_filled(m, 0.5, shade(tk.port_hole, 1.22));
    if m.width() >= 9.0 {
        for side in [-1.0, 1.0] {
            p.circle_filled(m.center() + vec2(side * m.width() * 0.22, 0.0), (m.height() * 0.2).max(0.8), tk.port_hole);
        }
    }
    if big {
        let bail = Stroke::new(0.8, shade(tk.port_hole, 1.7));
        let y = m.top() + m.height() * 0.12;
        p.line_segment([pos2(m.left(), y), pos2(m.right(), y)], bail);
        let tab = Rect::from_min_max(pos2(r.center().x - w * 0.24, r.bottom() - h * 0.2), pos2(r.center().x + w * 0.24, r.bottom() + h * 0.18));
        p.rect_filled(tab, 1.0, tk.tile_blue);
        p.rect_stroke(tab, 1.0, edge(tk.tile_blue), StrokeKind::Inside);
        let font = egui::FontId::proportional((tab.height() * 0.78).max(7.0));
        p.text(tab.center(), egui::Align2::CENTER_CENTER, "SFP", font, crate::theme::readable_on(tk.tile_blue));
    } else {
        let tab = Rect::from_min_max(pos2(r.center().x - w * 0.26, r.bottom() - h * 0.14), pos2(r.center().x + w * 0.26, r.bottom() + h * 0.08));
        p.rect_filled(tab, 0.5, tk.tile_blue);
    }
}

/// A duplex LC adapter: a blue housing with two square bores, each with a ceramic ferrule.
fn lc(p: &Painter, r: Rect, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    p.rect_filled(r, 1.0, tk.tile_blue);
    p.rect_stroke(r, 1.0, edge(tk.tile_blue), StrokeKind::Inside);
    for s in [-1.0, 1.0] {
        let c = r.center() + vec2(s * w * 0.23, 0.0);
        let bore = Rect::from_center_size(c, vec2(w * 0.33, h * 0.64));
        p.rect_filled(bore, 0.5, tk.port_hole);
        p.circle_filled(c, (h * 0.15).max(0.7), Color32::from_gray(0xe8));
    }
}

/// A USB-A port: a flat housing, a dark opening and the tongue with its contacts.
fn usb(p: &Painter, r: Rect, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    p.rect_filled(r, 0.8, tk.port_rim);
    p.rect_stroke(r, 0.8, edge(tk.port_rim), StrokeKind::Inside);
    let o = r.shrink2(vec2(w * 0.08, h * 0.18));
    p.rect_filled(o, 0.4, tk.port_hole);
    let tongue = Rect::from_min_max(
        pos2(o.left() + o.width() * 0.08, o.center().y - o.height() * 0.02),
        pos2(o.right() - o.width() * 0.04, o.bottom() - o.height() * 0.12),
    );
    p.rect_filled(tongue, 0.3, tk.tile_blue);
    if w >= 12.0 {
        for i in 0..4 {
            let x = tongue.left() + tongue.width() * (i as f32 + 1.0) / 5.0;
            p.circle_filled(pos2(x, tongue.center().y), (h * 0.07).max(0.5), tk.port_contact);
        }
    }
}

/// A D-shell (DE-9): a trapezoid shell in `shell`, a dark socket and two rows of contacts.
/// Large: the two jackscrews on the flange and a ring round each of the nine pins.
fn d_shell(p: &Painter, r: Rect, shell: Color32, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    let big = large(r);
    let trapezoid = |r: Rect, slant: f32| vec![r.left_top(), r.right_top(), pos2(r.right() - slant, r.bottom()), pos2(r.left() + slant, r.bottom())];
    p.add(Shape::convex_polygon(trapezoid(r, w * 0.14), shell, edge(shell)));
    let inner = r.shrink2(vec2(w * 0.1, h * 0.2));
    p.add(Shape::convex_polygon(trapezoid(inner, inner.width() * 0.12), tk.port_hole, Stroke::NONE));
    if big {
        for x in [r.left() + w * 0.05, r.right() - w * 0.05] {
            let c = pos2(x, r.top() + h * 0.11);
            p.circle_filled(c, h * 0.075, shade(shell, 0.6));
            p.line_segment([c - vec2(h * 0.05, 0.0), c + vec2(h * 0.05, 0.0)], Stroke::new(0.7, shade(shell, 1.3)));
        }
    }
    if w >= 12.0 {
        let dot = (h * if big { 0.075 } else { 0.07 }).max(0.5);
        for (row, n) in [(0.36, 5usize), (0.68, 4)] {
            for i in 0..n {
                let x = inner.left() + inner.width() * (i as f32 + 0.5 + if n == 4 { 0.5 } else { 0.0 }) / 5.0;
                let c = pos2(x, inner.top() + inner.height() * row);
                p.circle_filled(c, dot, tk.port_contact);
                if big {
                    p.circle_stroke(c, dot * 1.7, Stroke::new(0.6, shade(tk.port_hole, 1.7)));
                }
            }
        }
    }
}

/// A coaxial F connector: a round collar, a dark ring and the centre pin.
/// Large: a hexagonal nut round the collar and the thread's rings.
fn coax(p: &Painter, r: Rect, tk: &Tokens) {
    let (c, rad) = (r.center(), r.width().min(r.height()) / 2.0);
    if large(r) {
        let hex: Vec<egui::Pos2> = (0..6)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_6 + i as f32 * std::f32::consts::FRAC_PI_3;
                c + vec2(a.cos(), a.sin()) * rad
            })
            .collect();
        p.add(Shape::convex_polygon(hex, tk.port_rim, edge(tk.port_rim)));
        p.circle_stroke(c, rad * 0.78, Stroke::new(0.8, shade(tk.port_rim, 0.6)));
        p.circle_stroke(c, rad * 0.7, Stroke::new((rad * 0.12).max(0.6), tk.tile_gold));
        p.circle_filled(c, rad * 0.52, tk.port_hole);
        p.circle_filled(c, (rad * 0.16).max(0.7), tk.port_contact);
        return;
    }
    p.circle_filled(c, rad, tk.port_rim);
    p.circle_stroke(c, rad * 0.86, Stroke::new((rad * 0.14).max(0.6), tk.tile_gold));
    p.circle_filled(c, rad * 0.58, tk.port_hole);
    p.circle_filled(c, (rad * 0.2).max(0.7), tk.port_contact);
}

/// An IEC power inlet: a shell with its lower corners cut, a dark opening and three pins.
fn inlet(p: &Painter, r: Rect, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    let shape = |r: Rect, cut: f32| {
        vec![
            r.left_top(),
            r.right_top(),
            pos2(r.right(), r.bottom() - cut),
            pos2(r.right() - cut, r.bottom()),
            pos2(r.left() + cut, r.bottom()),
            pos2(r.left(), r.bottom() - cut),
        ]
    };
    p.add(Shape::convex_polygon(shape(r, h * 0.28), tk.port_rim, edge(tk.port_rim)));
    let inner = r.shrink2(vec2(w * 0.12, h * 0.14));
    p.add(Shape::convex_polygon(shape(inner, inner.height() * 0.26), tk.port_hole, Stroke::NONE));
    let pin = vec2((w * 0.1).max(1.0), h * 0.3);
    for dx in [-0.24, 0.24] {
        p.rect_filled(Rect::from_center_size(pos2(r.center().x + w * dx, inner.top() + h * 0.28), pin), 0.0, tk.port_contact);
    }
    p.rect_filled(Rect::from_center_size(pos2(r.center().x, inner.bottom() - h * 0.22), pin), 0.0, tk.port_contact);
}

/// A mains outlet: a faceplate, a round recess, two pin holes and the earth clips.
fn outlet(p: &Painter, r: Rect, used: bool, tk: &Tokens) {
    let (c, rad) = (r.center(), r.width().min(r.height()) / 2.0);
    p.rect_filled(r, 2.0, shade(tk.port_rim, 1.12));
    p.rect_stroke(r, 2.0, edge(tk.port_rim), StrokeKind::Inside);
    p.circle_filled(c, rad * 0.86, if used { shade(tk.port_hole, 0.8) } else { shade(tk.port_rim, 0.5) });
    for s in [-1.0, 1.0] {
        p.circle_filled(c + vec2(s * rad * 0.34, 0.0), (rad * 0.15).max(0.7), tk.port_hole);
        p.rect_filled(Rect::from_center_size(c + vec2(0.0, s * rad * 0.8), vec2(rad * 0.5, (rad * 0.16).max(0.8))), 0.0, tk.port_contact);
    }
}

/// The colour of a light that is off.
pub fn off_light(tk: &Tokens) -> Color32 {
    shade(tk.port_hole, 1.6)
}

/// The link and activity lights over a jack: `up` is `Some` for a cabled port (green or red for the
/// link, amber activity while it is up) and `None` for a free one (both off).
pub fn jack_lights(p: &Painter, jack: Rect, up: Option<bool>, zoom: f32, tk: &Tokens) {
    let off = off_light(tk);
    let link = match up {
        Some(true) => tk.link_up,
        Some(false) => tk.link_down,
        None => off,
    };
    let act = if up == Some(true) { tk.port_activity } else { off };
    lights(p, jack, link, act, zoom);
}

/// A pair of lights over a jack (link on the left, activity on the right) in the colours given;
/// `zoom` sizes them.
pub fn lights(p: &Painter, jack: Rect, link: Color32, act: Color32, zoom: f32) {
    if jack.width() < 7.0 {
        return;
    }
    let rad = (0.9 * zoom).max(0.9);
    let y = jack.top() - rad - 0.8;
    p.circle_filled(pos2(jack.left() + jack.width() * 0.25, y), rad, link);
    p.circle_filled(pos2(jack.left() + jack.width() * 0.75, y), rad, act);
}

/// A socket of a board's pin header: black plastic with a gold pin, or, with a jumper wire plugged in
/// (`used`), the wire's coloured housing.
fn pin(p: &Painter, r: Rect, used: bool, tk: &Tokens) {
    p.rect_filled(r, 1.0, shade(tk.port_hole, 1.5));
    p.rect_stroke(r, 1.0, edge(tk.port_rim), StrokeKind::Inside);
    if used {
        let housing = r.shrink(r.width() * 0.14);
        p.rect_filled(housing, 1.0, tk.iot_cable);
        p.rect_stroke(housing, 1.0, edge(tk.iot_cable), StrokeKind::Inside);
        p.rect_filled(Rect::from_center_size(r.center(), vec2(r.width() * 0.2, r.height() * 0.2)), 0.0, tk.port_hole);
    } else {
        p.rect_filled(Rect::from_center_size(r.center(), vec2(r.width() * 0.28, r.height() * 0.28)), 0.0, tk.port_contact);
    }
}

/// A radio mark: a dot with three waves above it.
fn radio(p: &Painter, r: Rect, tk: &Tokens) {
    let h = r.height();
    let base = pos2(r.center().x, r.bottom() - h * 0.2);
    let col = tk.port_rim;
    p.circle_filled(base, (h * 0.07).max(1.0), col);
    for k in 1..=3 {
        let rad = h * 0.2 * k as f32;
        let pts: Vec<egui::Pos2> = (0..=8)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 - 0.8 + i as f32 * 1.6 / 8.0;
                base + vec2(a.cos(), a.sin()) * rad
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new((h * 0.06).max(0.8), col)));
    }
}

/// Where a power cord meets a device's panel: the centre of its inlet.
pub fn inlet_rect(panel: Rect, zoom: f32) -> Rect {
    let h = (panel.height() * 0.5).min(11.0 * zoom);
    Rect::from_center_size(pos2(panel.right() - 13.0 * zoom, panel.center().y), vec2(h * aspect(Connector::Inlet), h))
}

/// Whether a device's front panel shows a power inlet (the rack-mounted kinds; a PC, a phone or a
/// thing has its own plug somewhere else).
pub fn has_inlet(kind: netlabcraft_model::DeviceKind) -> bool {
    use netlabcraft_model::DeviceKind as K;
    matches!(kind, K::Router | K::Switch | K::Hub | K::Server)
}

/// A point inside `r`, for tests.
#[cfg(test)]
fn inside(r: Rect, p: egui::Pos2) -> bool {
    r.expand(0.01).contains(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT, contrast};

    #[test]
    fn a_port_is_drawn_as_its_real_connector() {
        let c = |n: &str, m: Option<Medium>| connector_for(n, m);
        assert_eq!(c("FastEthernet0/1", Some(Medium::Copper)), Some(Connector::Rj45));
        assert_eq!(c("GigabitEthernet0/1", None), Some(Connector::Rj45));
        assert_eq!(c("GigabitEthernet0/25", Some(Medium::Fiber)), Some(Connector::Sfp));
        assert_eq!(c("GigabitEthernet0/25", Some(Medium::Combo)), Some(Connector::Combo));
        assert_eq!(c("Console", Some(Medium::Console)), Some(Connector::Console));
        assert_eq!(c("RS-232", Some(Medium::Rs232)), Some(Connector::Rs232));
        assert_eq!(c("Serial0/0/0", None), Some(Connector::Serial));
        assert_eq!(c("Coaxial0", None), Some(Connector::Coax));
        assert_eq!(c("Phone0", None), Some(Connector::Rj11));
        assert_eq!(c("USB", None), Some(Connector::Usb));
        assert_eq!(c("Wireless0", None), None, "a radio is not drawn");
        assert_eq!(c("Cellular0/0", None), None);
        assert_eq!(c("Wireless0", Some(Medium::Wireless)), None);
    }

    #[test]
    fn every_connector_keeps_its_shape_inside_its_cell() {
        for c in [
            Connector::Rj45,
            Connector::Combo,
            Connector::Sfp,
            Connector::Console,
            Connector::Usb,
            Connector::Serial,
            Connector::Rs232,
            Connector::Lc,
            Connector::Coax,
            Connector::Rj11,
            Connector::Inlet,
            Connector::Outlet,
            Connector::Pin,
            Connector::Radio,
        ] {
            // From a closet's tiny jack to the device window's large one, wide cells and tall ones.
            for size in [vec2(6.0, 4.0), vec2(16.0, 12.0), vec2(40.0, 20.0), vec2(52.0, 78.0), vec2(120.0, 40.0), vec2(30.0, 90.0)] {
                let cell = Rect::from_min_size(pos2(10.0, 20.0), size);
                let r = fit(c, cell);
                assert!(inside(cell, r.min) && inside(cell, r.max), "{c:?} leaves a {size:?} cell");
                assert!((r.width() / r.height() - aspect(c)).abs() < 0.01, "{c:?} loses its aspect in a {size:?} cell");
            }
            let panel = panel_size(c);
            assert!(panel.x > 0.0 && panel.y > 0.0 && (panel.x / panel.y - aspect(c)).abs() < 0.01, "{c:?} keeps its shape in the panel");
        }
    }

    #[test]
    fn a_port_with_no_cable_is_drawn_as_a_radio_mark_in_the_device_window() {
        assert_eq!(panel_connector("Wireless0", None), Connector::Radio);
        assert_eq!(panel_connector("Cellular0/0", None), Connector::Radio);
        assert_eq!(panel_connector("FastEthernet0", None), Connector::Rj45);
        assert_eq!(panel_connector("Serial0/0/0", None), Connector::Serial);
        assert_eq!(panel_connector("Coaxial0", None), Connector::Coax);
        assert_eq!(panel_connector("Phone0", None), Connector::Rj11);
        assert_eq!(panel_connector("GigabitEthernet0/1", Some(Medium::Combo)), Connector::Combo);
    }

    #[test]
    fn the_console_jack_differs_from_the_data_jack_in_both_themes() {
        for tk in [LIGHT, DARK] {
            assert_ne!(tk.console_cable, tk.port_rim);
            assert!(contrast(tk.port_hole, tk.port_rim) >= 3.0, "the opening reads against the housing");
            assert!(contrast(tk.port_contact, tk.port_hole) >= 3.0, "the contacts read against the opening");
        }
    }

    #[test]
    fn only_rack_kinds_show_an_inlet() {
        use netlabcraft_model::DeviceKind as K;
        assert!(has_inlet(K::Router) && has_inlet(K::Switch) && has_inlet(K::Server));
        assert!(!has_inlet(K::Phone) && !has_inlet(K::Iot) && !has_inlet(K::Pc));
    }
}
