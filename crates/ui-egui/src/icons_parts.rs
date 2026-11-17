//! Marks for the models added with the full device box (original artwork, drawn in code): a
//! round badge in the corner of a network or end device, and a picture on the puck of a smart
//! thing, a power-grid thing or a component.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

const DARK: Color32 = Color32::from_rgb(0x2a, 0x33, 0x40);
const RED: Color32 = Color32::from_rgb(0xd6, 0x45, 0x3a);
const GREEN: Color32 = Color32::from_rgb(0x2e, 0x8b, 0x57);
const BLUE: Color32 = Color32::from_rgb(0x3a, 0x8f, 0xd6);
const YELLOW: Color32 = Color32::from_rgb(0xf6, 0xd3, 0x4a);

fn badge_of(model_id: &str) -> bool {
    matches!(
        model_id,
        "Repeater"
            | "Coax-Splitter"
            | "Bridge-2"
            | "Switch-IND-8"
            | "Router-IND"
            | "Net-Controller"
            | "VoIP-Adapter"
            | "Analog-Phone"
            | "Generic-Wired"
            | "Generic-Wireless"
            | "Data-Historian"
            | "HMI-Panel"
            | "PLC"
    )
}

fn puck_of(model_id: &str) -> bool {
    model_id.starts_with("Part-")
        || matches!(
            model_id,
            "IoT-Streetlight"
                | "IoT-Traffic"
                | "IoT-Parking"
                | "IoT-Drain"
                | "IoT-Air"
                | "IoT-Blower"
                | "IoT-Battery"
                | "IoT-Meter"
                | "IoT-Solar"
                | "IoT-Wind"
                | "IoT-Thing"
        )
}

/// Draws the mark of a model: a badge for the network and end devices, a picture for the things.
pub fn mark(p: &Painter, rect: Rect, model_id: &str) {
    if puck_of(model_id) {
        puck(p, rect, model_id);
    } else if badge_of(model_id) {
        badge(p, rect, model_id);
    }
}

fn thin(w: f32) -> Stroke {
    Stroke::new((w / 30.0).max(1.0), DARK)
}

/// The corner badge: a white disc with a dark edge and the model's symbol inside.
fn badge(p: &Painter, rect: Rect, model_id: &str) {
    let w = rect.width();
    let c = pos2(rect.right() - w * 0.2, rect.bottom() - w * 0.22);
    let r = w * 0.19;
    p.circle_filled(c, r, Color32::WHITE);
    p.circle_stroke(c, r, thin(w));
    let s = r * 0.62;
    let line = Stroke::new((w / 26.0).max(1.0), DARK);
    match model_id {
        "Repeater" => {
            // An amplifier: a triangle pointing along the line.
            p.add(Shape::convex_polygon(vec![c + vec2(-s, -s), c + vec2(s, 0.0), c + vec2(-s, s)], DARK, Stroke::NONE));
        }
        "Coax-Splitter" => {
            p.line_segment([c + vec2(-s, 0.0), c], line);
            p.line_segment([c, c + vec2(s, -s * 0.8)], line);
            p.line_segment([c, c + vec2(s, s * 0.8)], line);
        }
        "Bridge-2" => {
            // An arch between two posts.
            let pts: Vec<Pos2> = (0..=8)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 8.0);
                    c + vec2(a.cos() * s, a.sin() * s * 0.9 + s * 0.4)
                })
                .collect();
            p.add(Shape::line(pts, line));
            p.line_segment([c + vec2(-s, s * 0.4), c + vec2(-s, s)], line);
            p.line_segment([c + vec2(s, s * 0.4), c + vec2(s, s)], line);
        }
        "Switch-IND-8" | "Router-IND" => {
            // A gear.
            for k in 0..6 {
                let a = k as f32 * std::f32::consts::PI / 3.0;
                p.line_segment([c, c + vec2(a.cos(), a.sin()) * s * 1.1], Stroke::new((w / 14.0).max(2.0), DARK));
            }
            p.circle_filled(c, s * 0.7, DARK);
            p.circle_filled(c, s * 0.3, Color32::WHITE);
        }
        "Net-Controller" => {
            // A root with three branches.
            for dx in [-s, 0.0, s] {
                let leaf = c + vec2(dx, s);
                p.line_segment([c + vec2(0.0, -s * 0.5), leaf], line);
                p.circle_filled(leaf, s * 0.28, DARK);
            }
            p.circle_filled(c + vec2(0.0, -s * 0.5), s * 0.35, RED);
        }
        "VoIP-Adapter" => {
            // A modular phone plug: a square with a latch notch.
            let plug = Rect::from_center_size(c, vec2(s * 1.4, s * 1.4));
            p.rect_stroke(plug, 1.0, line, StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, -s * 0.7), vec2(s * 0.6, s * 0.35)), 0.0, DARK);
            for dx in [-0.4f32, 0.0, 0.4] {
                p.line_segment([c + vec2(dx * s, -s * 0.2), c + vec2(dx * s, s * 0.5)], Stroke::new(1.0, DARK));
            }
        }
        "Analog-Phone" => {
            // The coiled cord.
            let pts: Vec<Pos2> = (0..=10).map(|i| c + vec2(-s + i as f32 * s * 0.2, if i % 2 == 0 { -s * 0.4 } else { s * 0.4 })).collect();
            p.add(Shape::line(pts, line));
        }
        "Generic-Wired" => {
            // An Ethernet socket: a square with a notch.
            p.rect_stroke(Rect::from_center_size(c, vec2(s * 1.5, s * 1.1)), 1.0, line, StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, s * 0.6), vec2(s * 0.7, s * 0.3)), 0.0, DARK);
        }
        "Generic-Wireless" => {
            p.circle_filled(c + vec2(0.0, s * 0.6), s * 0.18, DARK);
            for k in 1..=2 {
                let rr = s * 0.55 * k as f32;
                let pts: Vec<Pos2> = (0..=8)
                    .map(|i| {
                        let a = -std::f32::consts::FRAC_PI_2 - 0.8 + i as f32 * 1.6 / 8.0;
                        c + vec2(0.0, s * 0.6) + vec2(a.cos(), a.sin()) * rr
                    })
                    .collect();
                p.add(Shape::line(pts, line));
            }
        }
        "Data-Historian" => {
            // A database drum: three stacked discs.
            for k in 0..3 {
                let y = c.y - s * 0.6 + k as f32 * s * 0.55;
                p.rect_filled(Rect::from_center_size(pos2(c.x, y + s * 0.2), vec2(s * 1.3, s * 0.55)), s * 0.2, DARK);
                p.rect_filled(Rect::from_center_size(pos2(c.x, y), vec2(s * 1.3, s * 0.35)), s * 0.17, BLUE);
            }
        }
        "HMI-Panel" => {
            // A bar chart.
            for (i, h) in [0.5f32, 1.0, 0.75].into_iter().enumerate() {
                let x = c.x - s * 0.7 + i as f32 * s * 0.7;
                p.rect_filled(Rect::from_min_max(pos2(x - s * 0.2, c.y + s * 0.8 - h * s * 1.6), pos2(x + s * 0.2, c.y + s * 0.8)), 0.0, GREEN);
            }
        }
        _ => {
            // PLC: ladder logic, two rails and two rungs.
            for dx in [-0.6f32, 0.6] {
                p.line_segment([c + vec2(dx * s, -s), c + vec2(dx * s, s)], line);
            }
            for dy in [-0.4f32, 0.4] {
                p.line_segment([c + vec2(-0.6 * s, dy * s), c + vec2(0.6 * s, dy * s)], line);
            }
        }
    }
}

/// The picture on a thing's puck.
fn puck(p: &Painter, rect: Rect, model_id: &str) {
    let w = rect.width();
    let c = rect.center() + vec2(0.0, w * 0.12);
    let white = Color32::WHITE;
    let line = Stroke::new((w / 24.0).max(1.0), DARK);
    match model_id {
        "IoT-Streetlight" => {
            p.line_segment([c + vec2(-w * 0.06, w * 0.2), c + vec2(-w * 0.06, -w * 0.14)], Stroke::new((w / 16.0).max(1.5), white));
            p.line_segment([c + vec2(-w * 0.06, -w * 0.14), c + vec2(w * 0.12, -w * 0.14)], Stroke::new((w / 16.0).max(1.5), white));
            p.circle_filled(c + vec2(w * 0.12, -w * 0.1), w * 0.07, YELLOW);
        }
        "IoT-Traffic" => {
            let body = Rect::from_center_size(c, vec2(w * 0.2, w * 0.36));
            p.rect_filled(body, w * 0.03, DARK);
            for (i, col) in [RED, YELLOW, GREEN].into_iter().enumerate() {
                p.circle_filled(pos2(c.x, body.top() + w * 0.07 + i as f32 * w * 0.11), w * 0.04, col);
            }
        }
        "IoT-Parking" => {
            let sign = Rect::from_center_size(c, vec2(w * 0.3, w * 0.3));
            p.rect_filled(sign, w * 0.03, BLUE);
            p.text(sign.center(), egui::Align2::CENTER_CENTER, "P", egui::FontId::proportional(w * 0.26), white);
        }
        "IoT-Drain" => {
            let grate = Rect::from_center_size(c + vec2(0.0, w * 0.03), vec2(w * 0.34, w * 0.2));
            p.rect_filled(grate, 1.0, white);
            for k in 0..4 {
                let x = grate.left() + w * 0.05 + k as f32 * w * 0.08;
                p.line_segment([pos2(x, grate.top() + 1.0), pos2(x, grate.bottom() - 1.0)], line);
            }
            p.circle_filled(pos2(c.x, c.y - w * 0.14), w * 0.04, BLUE);
        }
        "IoT-Air" => {
            for (dx, dy, r) in [(-0.09f32, 0.02f32, 0.09f32), (0.06, -0.03, 0.11), (0.1, 0.05, 0.07)] {
                p.circle_filled(c + vec2(dx * w, dy * w), r * w, Color32::from_rgb(0xdd, 0xe3, 0xea));
            }
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, w * 0.08), vec2(w * 0.3, w * 0.07)), 0.0, Color32::from_rgb(0xdd, 0xe3, 0xea));
        }
        "IoT-Blower" => crate::icons::fan_mark(p, rect),
        "IoT-Battery" => {
            let body = Rect::from_center_size(c, vec2(w * 0.34, w * 0.2));
            p.rect_filled(body, w * 0.02, white);
            p.rect_stroke(body, w * 0.02, line, StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(body.right_center() + vec2(w * 0.03, 0.0), vec2(w * 0.05, w * 0.08)), 0.0, white);
            for k in 0..3 {
                p.rect_filled(
                    Rect::from_min_size(body.left_top() + vec2(w * 0.03 + k as f32 * w * 0.1, w * 0.03), vec2(w * 0.08, w * 0.14)),
                    0.0,
                    GREEN,
                );
            }
        }
        "IoT-Meter" => {
            p.circle_filled(c, w * 0.18, white);
            p.circle_stroke(c, w * 0.18, line);
            let bolt = vec![
                c + vec2(w * 0.03, -w * 0.13),
                c + vec2(-w * 0.07, w * 0.02),
                c + vec2(0.0, w * 0.02),
                c + vec2(-w * 0.03, w * 0.13),
                c + vec2(w * 0.07, -w * 0.03),
                c + vec2(0.0, -w * 0.03),
            ];
            p.add(Shape::convex_polygon(bolt, YELLOW, Stroke::new(1.0, DARK)));
        }
        "IoT-Solar" => {
            let panel =
                vec![c + vec2(-w * 0.2, w * 0.14), c + vec2(-w * 0.1, -w * 0.08), c + vec2(w * 0.22, -w * 0.08), c + vec2(w * 0.12, w * 0.14)];
            p.add(Shape::convex_polygon(panel, Color32::from_rgb(0x2a, 0x4f, 0x9a), Stroke::new(1.0, white)));
            p.line_segment([c + vec2(-w * 0.05, w * 0.14), c + vec2(w * 0.05, -w * 0.08)], Stroke::new(1.0, white));
            p.line_segment([c + vec2(-w * 0.15, w * 0.03), c + vec2(w * 0.17, w * 0.03)], Stroke::new(1.0, white));
            p.circle_filled(c + vec2(w * 0.18, -w * 0.17), w * 0.05, YELLOW);
        }
        "IoT-Wind" => {
            let hub = c + vec2(0.0, -w * 0.07);
            p.line_segment([hub, c + vec2(0.0, w * 0.2)], Stroke::new((w / 18.0).max(1.5), white));
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3.0 - 1.2;
                p.line_segment([hub, hub + vec2(a.cos(), a.sin()) * w * 0.17], Stroke::new((w / 22.0).max(1.2), white));
            }
            p.circle_filled(hub, w * 0.025, white);
        }
        "IoT-Thing" => {
            // A puzzle piece: a square with a knob on top and one on the right.
            let sq = Rect::from_center_size(c, vec2(w * 0.24, w * 0.24));
            p.rect_filled(sq, w * 0.02, BLUE);
            p.circle_filled(sq.center_top() - vec2(0.0, w * 0.02), w * 0.06, BLUE);
            p.circle_filled(sq.right_center() + vec2(w * 0.02, 0.0), w * 0.06, BLUE);
        }
        "Part-LED" => {
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, -w * 0.03), vec2(w * 0.16, w * 0.22)), w * 0.08, RED);
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, w * 0.09), vec2(w * 0.22, w * 0.03)), 0.0, RED);
            for dx in [-0.04f32, 0.04] {
                p.line_segment([c + vec2(dx * w, w * 0.1), c + vec2(dx * w, w * 0.2)], Stroke::new(1.0, white));
            }
        }
        "Part-Buzzer" => {
            p.circle_filled(c + vec2(-w * 0.05, 0.0), w * 0.13, DARK);
            p.circle_stroke(c + vec2(-w * 0.05, 0.0), w * 0.13, Stroke::new(1.0, white));
            p.circle_filled(c + vec2(-w * 0.05, 0.0), w * 0.03, white);
            for k in 1..=2 {
                let r = w * 0.06 * k as f32 + w * 0.1;
                let pts: Vec<Pos2> = (0..=6)
                    .map(|i| {
                        let a = -0.7 + i as f32 * 1.4 / 6.0;
                        c + vec2(w * 0.03, 0.0) + vec2(a.cos(), a.sin()) * r
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new(1.0, white)));
            }
        }
        "Part-Motor" => {
            p.circle_filled(c, w * 0.17, white);
            p.circle_stroke(c, w * 0.17, line);
            p.text(c, egui::Align2::CENTER_CENTER, "M", egui::FontId::proportional(w * 0.2), DARK);
        }
        "Part-Relay" => {
            let coil = Rect::from_center_size(c + vec2(0.0, w * 0.08), vec2(w * 0.3, w * 0.1));
            p.rect_filled(coil, 0.0, white);
            p.rect_stroke(coil, 0.0, line, StrokeKind::Inside);
            p.line_segment([c + vec2(-w * 0.15, -w * 0.08), c + vec2(w * 0.1, -w * 0.16)], Stroke::new((w / 20.0).max(1.2), white));
            p.circle_filled(c + vec2(-w * 0.15, -w * 0.08), w * 0.025, white);
            p.circle_filled(c + vec2(w * 0.15, -w * 0.08), w * 0.025, white);
        }
        "Part-LCD" => {
            let screen = Rect::from_center_size(c, vec2(w * 0.38, w * 0.2));
            p.rect_filled(screen, w * 0.02, Color32::from_rgb(0x7f, 0xc8, 0x6a));
            p.rect_stroke(screen, w * 0.02, line, StrokeKind::Inside);
            for dy in [-0.03f32, 0.03] {
                p.line_segment([c + vec2(-w * 0.14, dy * w), c + vec2(w * 0.14, dy * w)], Stroke::new(1.0, DARK));
            }
        }
        "Part-Button" => {
            let base = Rect::from_center_size(c + vec2(0.0, w * 0.06), vec2(w * 0.3, w * 0.12));
            p.rect_filled(base, w * 0.02, white);
            p.circle_filled(c + vec2(0.0, -w * 0.04), w * 0.11, RED);
            p.circle_stroke(c + vec2(0.0, -w * 0.04), w * 0.11, thin(w));
        }
        "Part-Pot" => {
            p.circle_filled(c, w * 0.17, DARK);
            p.circle_stroke(c, w * 0.17, Stroke::new(1.0, white));
            p.line_segment([c, c + vec2(w * 0.09, -w * 0.1)], Stroke::new((w / 22.0).max(1.2), white));
        }
        "Part-Ultrasonic" => {
            let body = Rect::from_center_size(c, vec2(w * 0.4, w * 0.2));
            p.rect_filled(body, w * 0.02, BLUE);
            for dx in [-0.1f32, 0.1] {
                p.circle_filled(c + vec2(dx * w, 0.0), w * 0.07, white);
                p.circle_filled(c + vec2(dx * w, 0.0), w * 0.035, DARK);
            }
        }
        "Part-Light" => {
            p.circle_filled(c, w * 0.09, YELLOW);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                p.line_segment([c + vec2(a.cos(), a.sin()) * w * 0.13, c + vec2(a.cos(), a.sin()) * w * 0.19], Stroke::new(1.2, YELLOW));
            }
        }
        _ => {
            // Part-Switch: a toggle with its lever up.
            let plate = Rect::from_center_size(c, vec2(w * 0.2, w * 0.3));
            p.rect_filled(plate, w * 0.03, white);
            p.rect_stroke(plate, w * 0.03, line, StrokeKind::Inside);
            p.line_segment([c + vec2(0.0, w * 0.04), c + vec2(0.0, -w * 0.08)], Stroke::new((w / 16.0).max(1.5), DARK));
            p.circle_filled(c + vec2(0.0, -w * 0.09), w * 0.03, RED);
        }
    }
}
