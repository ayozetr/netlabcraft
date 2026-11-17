//! The connectors on the front panels of the wiring-closet view, drawn in code (original artwork):
//! an RJ45 jack with its latch notch and contacts, an SFP cage with its pull tab, a console jack in
//! its own colour, USB-A, a serial D-shell, a duplex LC fiber port, a coaxial F connector, an RJ11
//! phone jack, an IEC power inlet and a mains outlet. Which connector a port gets depends only on its
//! name and medium ([`connector_for`]); colours come from [`Tokens`].

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
    }
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
                pos2(r.center().x + r.width() * 0.22, r.bottom() + r.height() * 0.1),
            );
            p.rect_filled(tab, 0.5, tk.tile_blue);
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
    }
}

/// An RJ45 jack: a housing in `rim`, a dark opening with the latch notch on top, a row of gold contacts.
fn rj45(p: &Painter, r: Rect, rim: Color32, contacts: usize, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    p.rect_filled(r, 1.0, rim);
    p.rect_stroke(r, 1.0, edge(rim), StrokeKind::Inside);
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
    if w >= 6.0 {
        let ch = body.height() * 0.42;
        let stroke = Stroke::new((w * 0.045).max(0.6), tk.port_contact);
        for i in 0..contacts {
            let x = body.left() + body.width() * (i as f32 + 0.5) / contacts as f32;
            p.line_segment([pos2(x, body.bottom() - ch), pos2(x, body.bottom() - 0.4)], stroke);
        }
    }
}

/// An SFP cage: a flat metal cage, a dark opening with the module's face and two bores, a coloured pull tab.
fn sfp(p: &Painter, r: Rect, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    p.rect_filled(r, 1.0, tk.port_rim);
    p.rect_stroke(r, 1.0, edge(tk.port_rim), StrokeKind::Inside);
    let o = r.shrink2(vec2(w * 0.08, h * 0.16));
    p.rect_filled(o, 0.5, tk.port_hole);
    let m = o.shrink2(vec2(o.width() * 0.08, o.height() * 0.14));
    p.rect_filled(m, 0.5, shade(tk.port_hole, 1.22));
    if m.width() >= 9.0 {
        for s in [-1.0, 1.0] {
            p.circle_filled(m.center() + vec2(s * m.width() * 0.22, 0.0), (m.height() * 0.2).max(0.8), tk.port_hole);
        }
    }
    let tab = Rect::from_min_max(pos2(r.center().x - w * 0.26, r.bottom() - h * 0.14), pos2(r.center().x + w * 0.26, r.bottom() + h * 0.08));
    p.rect_filled(tab, 0.5, tk.tile_blue);
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
fn d_shell(p: &Painter, r: Rect, shell: Color32, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    let trapezoid = |r: Rect, slant: f32| vec![r.left_top(), r.right_top(), pos2(r.right() - slant, r.bottom()), pos2(r.left() + slant, r.bottom())];
    p.add(Shape::convex_polygon(trapezoid(r, w * 0.14), shell, edge(shell)));
    let inner = r.shrink2(vec2(w * 0.1, h * 0.2));
    p.add(Shape::convex_polygon(trapezoid(inner, inner.width() * 0.12), tk.port_hole, Stroke::NONE));
    if w >= 12.0 {
        let dot = (h * 0.07).max(0.5);
        for (row, n) in [(0.36, 5usize), (0.68, 4)] {
            for i in 0..n {
                let x = inner.left() + inner.width() * (i as f32 + 0.5 + if n == 4 { 0.5 } else { 0.0 }) / 5.0;
                p.circle_filled(pos2(x, inner.top() + inner.height() * row), dot, tk.port_contact);
            }
        }
    }
}

/// A coaxial F connector: a round collar, a dark ring and the centre pin.
fn coax(p: &Painter, r: Rect, tk: &Tokens) {
    let (c, rad) = (r.center(), r.width().min(r.height()) / 2.0);
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

/// The link and activity lights over a jack: `up` is `Some` for a cabled port (green or red for the
/// link, amber activity while it is up) and `None` for a free one (both off).
pub fn jack_lights(p: &Painter, jack: Rect, up: Option<bool>, zoom: f32, tk: &Tokens) {
    if jack.width() < 7.0 {
        return;
    }
    let rad = (0.9 * zoom).max(0.9);
    let y = jack.top() - rad - 0.8;
    let off = shade(tk.port_hole, 1.6);
    let link = match up {
        Some(true) => tk.link_up,
        Some(false) => tk.link_down,
        None => off,
    };
    let act = if up == Some(true) { tk.port_activity } else { off };
    p.circle_filled(pos2(jack.left() + jack.width() * 0.25, y), rad, link);
    p.circle_filled(pos2(jack.left() + jack.width() * 0.75, y), rad, act);
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
        let cell = Rect::from_min_size(pos2(10.0, 20.0), vec2(40.0, 20.0));
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
        ] {
            let r = fit(c, cell);
            assert!(inside(cell, r.min) && inside(cell, r.max), "{c:?} leaves its cell");
            assert!((r.width() / r.height() - aspect(c)).abs() < 0.01, "{c:?} loses its aspect");
        }
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
