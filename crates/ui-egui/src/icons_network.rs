//! Pictures for the models that used to be a base icon with a small badge and so looked like their
//! neighbours (original artwork, drawn in code): the repeater and the coax splitter, the VoIP adapter and
//! the analog phone, the generic wired and wireless end devices, and the marks that set the industrial
//! switch and router and the compact firewall apart from the standard ones. Colours come from the theme.

use std::f32::consts::{FRAC_PI_2, PI};

use egui::{Painter, Pos2, Rect, Shape, Stroke, pos2};

use crate::icons_things::Art;
use crate::theme::{Tokens, device_color, shade};

/// Whether [`icon`] draws this model whole.
pub fn is_network_part(model_id: &str) -> bool {
    matches!(model_id, "Repeater" | "Coax-Splitter" | "VoIP-Adapter" | "Analog-Phone" | "Generic-Wired" | "Generic-Wireless")
}

/// Draws one of the models [`is_network_part`] names, filling `rect`.
pub fn icon(p: &Painter, rect: Rect, model_id: &str, tk: &Tokens) {
    let a = Art::new(p, rect, tk);
    a.ground();
    match model_id {
        "Repeater" => repeater(&a),
        "Coax-Splitter" => coax_splitter(&a),
        "VoIP-Adapter" => voip_adapter(&a),
        "Analog-Phone" => analog_phone(&a),
        "Generic-Wireless" => generic(&a, true),
        _ => generic(&a, false),
    }
}

/// A repeater: a box with the amplifier triangle on its front and a cable stub at each end.
fn repeater(a: &Art) {
    let t = a.tk;
    let body = device_color(netlabcraft_model::DeviceKind::Hub);
    a.bar(a.at(0.02, 0.5), a.at(0.14, 0.5), 0.06, t.art_steel);
    a.bar(a.at(0.86, 0.5), a.at(0.98, 0.5), 0.06, t.art_steel);
    a.boxed(a.rc(0.12, 0.28, 0.88, 0.72), 0.05, body);
    a.poly(vec![a.at(0.4, 0.35), a.at(0.64, 0.5), a.at(0.4, 0.65)], t.art_paper);
    for k in 0..3 {
        a.dot(a.at(0.2 + k as f32 * 0.07, 0.35), 0.02, t.art_green);
    }
    a.arc(a.at(0.72, 0.5), 0.1, -0.7, 0.7, 0.02, t.art_paper);
    a.arc(a.at(0.72, 0.5), 0.16, -0.7, 0.7, 0.02, t.art_paper);
}

/// A coax splitter: a metal body with one F connector in and two out.
fn coax_splitter(a: &Art) {
    let t = a.tk;
    let brass = t.tile_gold;
    a.bar(a.at(0.1, 0.5), a.at(0.4, 0.5), 0.085, shade(brass, 1.1));
    a.bar(a.at(0.6, 0.5), a.at(0.86, 0.28), 0.075, shade(brass, 1.1));
    a.bar(a.at(0.6, 0.5), a.at(0.86, 0.72), 0.075, shade(brass, 1.1));
    for at in [a.at(0.1, 0.5), a.at(0.88, 0.27), a.at(0.88, 0.73)] {
        a.disc(at, 0.075, brass);
        a.dot(at, 0.025, t.art_ink);
    }
    let c = a.at(0.5, 0.5);
    let hex: Vec<Pos2> = (0..6).map(|k| (k as f32 * PI / 3.0) + FRAC_PI_2).map(|ang| c + egui::vec2(ang.cos(), ang.sin()) * a.w * 0.2).collect();
    a.poly(hex, t.art_steel);
    a.text(c, "F", 0.16, t.art_ink);
}

/// A VoIP adapter: a small box with its phone jack, its network jack and its lights.
fn voip_adapter(a: &Art) {
    let t = a.tk;
    let body = device_color(netlabcraft_model::DeviceKind::Phone);
    a.p.add(Shape::line(
        vec![a.at(0.22, 0.74), a.at(0.16, 0.84), a.at(0.26, 0.9), a.at(0.2, 0.96)],
        Stroke::new((a.w * 0.025).max(1.0), t.art_steel),
    ));
    a.boxed(a.rc(0.08, 0.26, 0.92, 0.76), 0.06, body);
    for k in 0..4 {
        a.dot(a.at(0.18 + k as f32 * 0.08, 0.34), 0.02, if k < 3 { t.art_green } else { t.art_amber });
    }
    a.text(a.at(0.68, 0.36), "VoIP", 0.13, t.art_paper);
    // The phone jack (narrow) and the network jack (wide), each with its contacts.
    for (r, pins) in [(a.rc(0.16, 0.48, 0.34, 0.68), 4), (a.rc(0.42, 0.48, 0.66, 0.68), 8)] {
        a.plain(r, 0.01, t.art_steel);
        a.plain(r.shrink2(egui::vec2(a.w * 0.025, a.w * 0.03)), 0.0, t.art_ink);
        for k in 0..pins {
            let x = r.left() + r.width() * (k as f32 + 0.5) / pins as f32;
            a.p.circle_filled(pos2(x, r.bottom() - a.w * 0.05), (a.w * 0.008).max(0.7), t.tile_gold);
        }
    }
    a.dot(a.at(0.8, 0.58), 0.03, t.art_ink);
}

/// An analog phone: a desk set with a rotary dial and the handset across its cradle.
fn analog_phone(a: &Art) {
    let t = a.tk;
    let body = device_color(netlabcraft_model::DeviceKind::Phone);
    a.poly(vec![a.at(0.08, 0.88), a.at(0.92, 0.88), a.at(0.82, 0.46), a.at(0.18, 0.46)], shade(body, 0.8));
    a.disc(a.at(0.5, 0.67), 0.15, t.art_paper);
    for k in 0..8 {
        let ang = PI * 0.3 + k as f32 * PI * 1.4 / 7.0;
        a.dot(a.at(0.5, 0.67) + egui::vec2(ang.cos(), ang.sin()) * a.w * 0.1, 0.017, t.art_ink);
    }
    a.dot(a.at(0.5, 0.67), 0.03, t.art_steel);
    // The handset.
    a.bar(a.at(0.2, 0.34), a.at(0.8, 0.34), 0.07, shade(body, 0.6));
    for x in [0.18, 0.82] {
        a.boxed(a.rc(x - 0.07, 0.3, x + 0.07, 0.5), 0.04, shade(body, 0.6));
    }
}

/// A generic end device: a neutral box with one Ethernet jack (and its cable), or with an antenna and
/// Wi-Fi waves when it is wireless.
fn generic(a: &Art, wireless: bool) {
    let t = a.tk;
    if wireless {
        for x in [0.24, 0.76] {
            a.bar(a.at(x, 0.36), a.at(x, 0.18), 0.03, t.art_ink);
        }
        for k in 0..3 {
            a.arc(a.at(0.5, 0.3), 0.14 + k as f32 * 0.08, -PI * 0.8, -PI * 0.2, 0.026, t.art_green);
        }
    } else {
        a.bar(a.at(0.5, 0.72), a.at(0.5, 0.92), 0.03, t.tile_green);
    }
    a.boxed(a.rc(0.1, 0.34, 0.9, 0.76), 0.06, t.art_steel);
    a.plain(a.rc(0.16, 0.4, 0.84, 0.52), 0.02, shade(t.art_steel, 0.7));
    for k in 0..6 {
        a.line(a.at(0.22 + k as f32 * 0.1, 0.43), a.at(0.22 + k as f32 * 0.1, 0.49), 0.012, t.art_ink);
    }
    if !wireless {
        let jack = a.rc(0.41, 0.56, 0.59, 0.7);
        a.plain(jack, 0.01, t.art_ink);
        a.plain(jack.shrink(a.w * 0.025), 0.0, t.art_steel);
        a.plain(jack.shrink(a.w * 0.045), 0.0, t.art_ink);
    }
    a.dot(a.at(0.8, 0.64), 0.025, t.art_green);
}

/// The hazard band an industrial switch or router wears across its front: yellow with dark slanted
/// stripes, over the standard picture.
pub fn industrial_band(p: &Painter, rect: Rect, router: bool, tk: &Tokens) {
    let a = Art::new(p, rect, tk);
    let (x0, x1, y0, y1) = if router { (0.16, 0.84, 0.6, 0.7) } else { (0.08, 0.8, 0.63, 0.74) };
    a.plain(a.rc(x0, y0, x1, y1), 0.0, tk.art_amber);
    let h = y1 - y0;
    let mut x = x0;
    while x + h < x1 {
        a.line(a.at(x, y1), a.at(x + h, y0), 0.034, tk.art_ink);
        x += h * 1.8;
    }
    a.p.rect_stroke(a.rc(x0, y0, x1, y1), 0.0, a.ink(), egui::StrokeKind::Inside);
}

/// The compact firewall's mark over the router drum: a shorter brick wall with a padlock instead of the
/// shield the larger firewall carries.
pub fn firewall_compact(p: &Painter, rect: Rect, tk: &Tokens) {
    let a = Art::new(p, rect, tk);
    let brick = tk.art_red;
    let wall = a.rc(0.2, 0.52, 0.8, 0.84);
    a.p.rect_filled(wall, a.w * 0.02, brick);
    let mortar = shade(brick, 1.7);
    let line = Stroke::new(a.lw() * 0.9, mortar);
    let mid = wall.center().y;
    a.p.line_segment([wall.left_center(), wall.right_center()], line);
    for (top, bottom, shift) in [(wall.top(), mid, 0.0f32), (mid, wall.bottom(), 0.5)] {
        for k in 0..3 {
            let x = wall.left() + wall.width() * ((k as f32 + shift) / 3.0);
            if x > wall.left() + a.w * 0.03 && x < wall.right() - a.w * 0.03 {
                a.p.line_segment([pos2(x, top), pos2(x, bottom)], line);
            }
        }
    }
    a.p.rect_stroke(wall, a.w * 0.02, Stroke::new(a.lw(), shade(brick, 0.5)), egui::StrokeKind::Inside);
    // The padlock: a shackle over a body, on the wall's top edge.
    let c = a.at(0.5, 0.47);
    a.arc(c + egui::vec2(0.0, -a.w * 0.02), 0.075, PI, 2.0 * PI, 0.035, tk.art_ink);
    a.arc(c + egui::vec2(0.0, -a.w * 0.02), 0.075, PI, 2.0 * PI, 0.02, tk.art_steel);
    a.boxed(Rect::from_center_size(c + egui::vec2(0.0, a.w * 0.04), egui::vec2(a.w * 0.22, a.w * 0.16)), 0.03, tk.art_green);
    a.dot(c + egui::vec2(0.0, a.w * 0.04), 0.02, tk.art_ink);
}

/// The mark that makes the public telephone network's cloud a telephone one: a dial and a handset where
/// the plain cloud has its three port marks.
pub fn pstn_mark(p: &Painter, rect: Rect, tk: &Tokens) {
    let a = Art::new(p, rect, tk);
    let ink = shade(tk.art_green, 0.85);
    // The dial.
    let c = a.at(0.36, 0.52);
    a.p.circle_filled(c, a.w * 0.12, tk.art_paper);
    a.p.circle_stroke(c, a.w * 0.12, Stroke::new(a.lw(), ink));
    for k in 0..6 {
        let ang = PI * 0.2 + k as f32 * PI * 1.5 / 5.0;
        a.p.circle_filled(c + egui::vec2(ang.cos(), ang.sin()) * a.w * 0.075, (a.w * 0.014).max(0.8), ink);
    }
    a.p.circle_filled(c, a.w * 0.025, ink);
    // The handset: a curved bar with an ear piece and a mouth piece hanging from its ends.
    let h = a.at(0.68, 0.52);
    let pts: Vec<Pos2> = (0..=10)
        .map(|i| PI * (1.2 + 0.6 * i as f32 / 10.0))
        .map(|ang| h + egui::vec2(ang.cos(), ang.sin()) * a.w * 0.16 + egui::vec2(0.0, a.w * 0.06))
        .collect();
    a.p.add(Shape::line(pts.clone(), Stroke::new((a.w * 0.05).max(1.5), ink)));
    for end in [pts.first(), pts.last()].into_iter().flatten() {
        a.p.line_segment([*end, *end + egui::vec2(0.0, a.w * 0.1)], Stroke::new((a.w * 0.085).max(2.0), ink));
    }
}
