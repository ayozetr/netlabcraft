//! Icons, drawn in code (original artwork; no files, no vendor pictures). Devices are shaded
//! solids in the blue family network diagrams use; toolbar glyphs are simple line drawings.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};
use netlabcraft_model::DeviceKind;

use crate::theme::{Tokens, device_color, shade};

/// The mark a multilayer switch carries over its switch icon: a small router drum (it routes).
pub fn multilayer_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let c = device_color(DeviceKind::Router);
    let center = pos2(rect.center().x + w * 0.02, rect.center().y - w * 0.3);
    let (rx, ry) = (w * 0.3, w * 0.12);
    p.add(Shape::convex_polygon(
        ellipse_points(center, rx, ry, 0.0, std::f32::consts::TAU),
        shade(c, 1.3),
        Stroke::new((w / 30.0).max(1.0), shade(c, 0.6)),
    ));
    let s = Stroke::new((w / 28.0).max(1.0), Color32::WHITE);
    arrow(p, center + vec2(-rx * 0.15, 0.0), center + vec2(-rx * 0.8, 0.0), s);
    arrow(p, center + vec2(rx * 0.15, 0.0), center + vec2(rx * 0.8, 0.0), s);
}

/// The mark an access point carries over its box: two antennas with radio waves.
pub fn access_point_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let dark = Color32::from_rgb(0x2a, 0x33, 0x40);
    let s = Stroke::new((w / 22.0).max(1.0), dark);
    for x in [0.3, 0.7] {
        let base = pos2(rect.left() + w * x, rect.center().y - w * 0.12);
        p.line_segment([base, base - vec2(0.0, w * 0.32)], s);
        p.circle_filled(base - vec2(0.0, w * 0.32), w * 0.04, dark);
    }
    let c = pos2(rect.center().x, rect.center().y - w * 0.42);
    let wave = Stroke::new((w / 30.0).max(1.0), Color32::from_rgb(0x2e, 0x8b, 0x57));
    for r in [0.12, 0.2] {
        let pts: Vec<Pos2> = (0..=8)
            .map(|i| {
                let a = std::f32::consts::PI * (1.15 + 0.7 * i as f32 / 8.0);
                c + vec2(a.cos(), a.sin()) * w * r
            })
            .collect();
        p.add(Shape::line(pts, wave));
    }
}

/// The mark a wireless LAN controller carries: a small panel with a hub dot fanning out to
/// three access-point dots (it runs the radios it is linked to).
pub fn wlc_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let dark = Color32::from_rgb(0x2a, 0x33, 0x40);
    let green = Color32::from_rgb(0x2e, 0x8b, 0x57);
    let panel = Rect::from_center_size(pos2(rect.center().x, rect.center().y - w * 0.36), vec2(w * 0.62, w * 0.3));
    p.rect_filled(panel, w * 0.05, Color32::from_rgb(0xf4, 0xf6, 0xf9));
    p.rect_stroke(panel, w * 0.05, Stroke::new((w / 30.0).max(1.0), dark), StrokeKind::Inside);
    let hub = pos2(panel.center().x, panel.bottom() - w * 0.07);
    let s = Stroke::new((w / 34.0).max(1.0), dark);
    for dx in [-0.2, 0.0, 0.2] {
        let tip = pos2(panel.center().x + w * dx, panel.top() + w * 0.08);
        p.line_segment([hub, tip], s);
        p.circle_filled(tip, w * 0.035, green);
    }
    p.circle_filled(hub, w * 0.045, dark);
}

/// The mark a lightweight access point adds to the access-point mark: a small link to its
/// controller (a tether ending in a square).
pub fn lap_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let c = Color32::from_rgb(0x5a, 0x6e, 0xc4);
    let from = pos2(rect.right() - w * 0.22, rect.center().y - w * 0.05);
    let to = pos2(rect.right() - w * 0.04, rect.center().y - w * 0.28);
    p.line_segment([from, to], Stroke::new((w / 26.0).max(1.0), c));
    p.rect_filled(Rect::from_center_size(to, vec2(w * 0.13, w * 0.13)), w * 0.02, c);
}

/// The marks a catalogue model wears over its kind's icon.
pub fn model_marks(p: &Painter, rect: Rect, model_id: &str, multilayer: bool) {
    if multilayer {
        multilayer_mark(p, rect);
    }
    match model_id {
        "AccessPoint" | "Home-Router" => access_point_mark(p, rect),
        "LAP" => {
            access_point_mark(p, rect);
            lap_mark(p, rect);
        }
        "WLC" => wlc_mark(p, rect),
        "Firewall-5" => firewall_mark(p, rect),
        "Router-M2" | "Router-M3" => modular_mark(p, rect),
        "IoT-Sensor" => thermometer_mark(p, rect),
        "IoT-Device" => fan_mark(p, rect),
        "IoT-Motion" | "IoT-Smoke" | "IoT-Light" | "IoT-Door" | "IoT-Siren" | "IoT-Webcam" => crate::icons_families::iot_mark(p, rect, model_id),
        "Home-Gateway" => {
            access_point_mark(p, rect);
            crate::icons_families::gateway_mark(p, rect);
        }
        _ => {}
    }
}

/// A smart sensor: a thermometer on the puck.
pub fn thermometer_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let c = rect.center() + vec2(0.0, w * 0.12);
    let red = Color32::from_rgb(0xd6, 0x45, 0x3a);
    let tube = Rect::from_center_size(c - vec2(0.0, w * 0.06), vec2(w * 0.1, w * 0.3));
    p.rect_filled(tube, w * 0.05, Color32::WHITE);
    p.rect_filled(Rect::from_min_max(pos2(tube.left() + w * 0.025, tube.center().y), pos2(tube.right() - w * 0.025, tube.bottom())), 0.0, red);
    p.circle_filled(pos2(c.x, tube.bottom()), w * 0.08, Color32::WHITE);
    p.circle_filled(pos2(c.x, tube.bottom()), w * 0.055, red);
}

/// A smart device: a three-bladed fan on the puck.
pub fn fan_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let c = rect.center() + vec2(0.0, w * 0.12);
    p.circle_filled(c, w * 0.21, Color32::WHITE);
    let blade = Color32::from_rgb(0x2f, 0x5d, 0x7c);
    for k in 0..3 {
        let a = k as f32 * std::f32::consts::TAU / 3.0;
        let tip = c + vec2(a.cos(), a.sin()) * w * 0.18;
        let side = c + vec2((a + 0.7).cos(), (a + 0.7).sin()) * w * 0.13;
        p.add(Shape::convex_polygon(vec![c, tip, side], blade, Stroke::NONE));
    }
    p.circle_filled(c, w * 0.035, blade);
}

/// A modular router: a row of card bays on the drum's side band, one holding a card (a lit
/// plate with two sockets), the others blank.
pub fn modular_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let plate = Color32::from_rgb(0x2b, 0x31, 0x3a);
    let card = Color32::from_rgb(0xd9, 0xb2, 0x4c);
    let y = rect.center().y + w * 0.16;
    let (bw, bh) = (w * 0.2, w * 0.11);
    for k in 0..3 {
        let x = rect.center().x + (k as f32 - 1.0) * (bw + w * 0.04);
        let bay = Rect::from_center_size(pos2(x, y), vec2(bw, bh));
        p.rect_filled(bay, w * 0.015, if k == 0 { card } else { plate });
        if k == 0 {
            for s in [-1.0, 1.0] {
                p.rect_filled(Rect::from_center_size(pos2(x + s * bw * 0.22, y), vec2(bw * 0.28, bh * 0.45)), 0.0, plate);
            }
        }
        p.rect_stroke(bay, w * 0.015, Stroke::new((w / 60.0).max(0.5), Color32::WHITE), StrokeKind::Inside);
    }
}

/// A switched-off device: a dark veil and a small power symbol.
pub fn powered_off_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    p.rect_filled(rect.expand(w * 0.04), w * 0.08, Color32::from_black_alpha(110));
    let c = pos2(rect.right() - w * 0.1, rect.top() + w * 0.1);
    p.circle_filled(c, w * 0.11, Color32::from_rgb(0x30, 0x30, 0x30));
    let s = Stroke::new((w / 28.0).max(1.0), Color32::WHITE);
    let r = w * 0.065;
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + 0.6 + i as f32 * (std::f32::consts::TAU - 1.2) / 12.0;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, s));
    p.line_segment([c - vec2(0.0, r * 1.25), c - vec2(0.0, r * 0.2)], s);
}

/// The firewall appliance: a brick wall standing in front of the router drum (it routes, and
/// it stops what isn't allowed), with a small shield on its top edge.
pub fn firewall_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let brick = Color32::from_rgb(0xb5, 0x4a, 0x2c);
    let mortar = Color32::from_rgb(0xf1, 0xe4, 0xd6);
    let wall = Rect::from_min_max(pos2(rect.left() + w * 0.04, rect.center().y - w * 0.12), pos2(rect.right() - w * 0.04, rect.center().y + w * 0.3));
    p.rect_filled(wall, w * 0.03, brick);
    let line = Stroke::new((w / 32.0).max(1.0), mortar);
    let rows = 3;
    let row_h = wall.height() / rows as f32;
    for r in 1..rows {
        let y = wall.top() + row_h * r as f32;
        p.line_segment([pos2(wall.left(), y), pos2(wall.right(), y)], line);
    }
    for r in 0..rows {
        let (top, bottom) = (wall.top() + row_h * r as f32, wall.top() + row_h * (r + 1) as f32);
        // Bricks a third of the wall wide, every other row shifted by half a brick.
        let offset = if r % 2 == 0 { 1.0 / 6.0 } else { 1.0 / 3.0 };
        for k in 0..3 {
            let x = wall.left() + wall.width() * (offset + k as f32 / 3.0);
            if x > wall.left() + w * 0.02 && x < wall.right() - w * 0.02 {
                p.line_segment([pos2(x, top), pos2(x, bottom)], line);
            }
        }
    }
    p.rect_stroke(wall, w * 0.03, Stroke::new((w / 30.0).max(1.0), shade(brick, 0.55)), StrokeKind::Inside);
    // The shield: a pointed badge over the wall's top right corner.
    let c = pos2(wall.right() - w * 0.14, wall.top() - w * 0.02);
    let s = w * 0.13;
    let shield = vec![c + vec2(-s, -s * 0.9), c + vec2(s, -s * 0.9), c + vec2(s, s * 0.1), c + vec2(0.0, s * 1.1), c + vec2(-s, s * 0.1)];
    p.add(Shape::convex_polygon(shield, Color32::from_rgb(0x2e, 0x8b, 0x57), Stroke::new((w / 32.0).max(1.0), Color32::WHITE)));
}

/// Draws a device icon filling `rect`.
pub fn device(p: &Painter, rect: Rect, kind: DeviceKind, _tk: &Tokens) {
    let c = device_color(kind);
    let (light, dark) = (shade(c, 1.45), shade(c, 0.62));
    let w = rect.width();
    let white = Stroke::new((w / 20.0).max(1.0), Color32::WHITE);
    match kind {
        DeviceKind::Router => {
            // A short drum seen from slightly above: dark side band, lit top ellipse, four
            // white arrows on the top (two in, two out).
            let cx = rect.center().x;
            let (rx, ry) = (w * 0.46, w * 0.17);
            let top_y = rect.center().y - w * 0.08;
            let side_h = w * 0.24;
            let mut side = ellipse_points(pos2(cx, top_y + side_h), rx, ry, 0.0, std::f32::consts::PI);
            side.extend(ellipse_points(pos2(cx, top_y), rx, ry, std::f32::consts::PI, 0.0));
            p.add(Shape::convex_polygon(side, dark, Stroke::NONE));
            p.add(Shape::convex_polygon(ellipse_points(pos2(cx, top_y), rx, ry, 0.0, std::f32::consts::TAU), light, Stroke::new(1.0, dark)));
            let a = rx * 0.55;
            let squash = ry / rx;
            for (dx, dy, out) in [(1.0, 0.0, true), (-1.0, 0.0, true), (0.0, 1.0, false), (0.0, -1.0, false)] {
                let d = vec2(dx, dy * squash);
                let (from, to) = if out {
                    (pos2(cx, top_y) + d * a * 0.25, pos2(cx, top_y) + d * a)
                } else {
                    (pos2(cx, top_y) + d * a, pos2(cx, top_y) + d * a * 0.25)
                };
                arrow(p, from, to, white);
            }
        }
        DeviceKind::Switch => {
            // A flat box in perspective: lit top face, dark front face, opposing arrows on top.
            let left = rect.left() - w * 0.06;
            let right = rect.right() + w * 0.06;
            let skew = w * 0.2;
            let top = rect.center().y - w * 0.2;
            let mid = rect.center().y + w * 0.06;
            let bottom = mid + w * 0.2;
            let top_face = vec![pos2(left + skew, top), pos2(right, top), pos2(right - skew, mid), pos2(left, mid)];
            p.add(Shape::convex_polygon(
                vec![pos2(left, mid), pos2(right - skew, mid), pos2(right - skew, bottom), pos2(left, bottom)],
                dark,
                Stroke::NONE,
            ));
            p.add(Shape::convex_polygon(
                vec![pos2(right - skew, mid), pos2(right, top), pos2(right, bottom - skew * 0.0 - (mid - top)), pos2(right - skew, bottom)],
                shade(c, 0.5),
                Stroke::NONE,
            ));
            p.add(Shape::convex_polygon(top_face, light, Stroke::new(1.0, dark)));
            let cy = (top + mid) / 2.0;
            let (x1, x2) = (left + w * 0.2, right - w * 0.2);
            arrow(p, pos2(x1, cy - w * 0.035), pos2(x2, cy - w * 0.035), white);
            arrow(p, pos2(x2 - skew * 0.3, cy + w * 0.035), pos2(x1 - skew * 0.3, cy + w * 0.035), white);
        }
        DeviceKind::Pc => {
            // A monitor with a lit screen on a stand, a small tower behind it on the right.
            let tower = Rect::from_min_size(rect.min + vec2(w * 0.7, w * 0.22), vec2(w * 0.24, w * 0.56));
            p.rect_filled(tower, w * 0.03, shade(c, 0.75));
            p.circle_filled(pos2(tower.center().x, tower.top() + w * 0.08), w * 0.025, Color32::from_rgb(0x8f, 0xf0, 0x8f));
            let screen = Rect::from_min_size(rect.min + vec2(w * 0.04, w * 0.12), vec2(w * 0.7, w * 0.48));
            p.rect_filled(screen, w * 0.05, dark);
            p.rect_filled(screen.shrink(w * 0.06), w * 0.02, light);
            p.line_segment(
                [screen.left_top() + vec2(w * 0.1, w * 0.12), screen.left_top() + vec2(w * 0.3, w * 0.06)],
                Stroke::new(w / 18.0, Color32::WHITE.gamma_multiply(0.7)),
            );
            let neck = Rect::from_min_size(pos2(screen.center().x - w * 0.06, screen.bottom()), vec2(w * 0.12, w * 0.12));
            p.rect_filled(neck, 0.0, dark);
            let foot = Rect::from_center_size(pos2(screen.center().x, neck.bottom() + w * 0.04), vec2(w * 0.42, w * 0.08));
            p.rect_filled(foot, w * 0.03, shade(c, 0.5));
        }
        DeviceKind::Laptop => {
            // An open laptop: lit screen above a keyboard base in perspective.
            let screen = Rect::from_min_size(rect.min + vec2(w * 0.18, w * 0.12), vec2(w * 0.64, w * 0.44));
            p.rect_filled(screen, w * 0.04, dark);
            p.rect_filled(screen.shrink(w * 0.05), w * 0.02, light);
            let base = vec![
                pos2(screen.left(), screen.bottom()),
                pos2(screen.right(), screen.bottom()),
                pos2(rect.right() - w * 0.04, screen.bottom() + w * 0.2),
                pos2(rect.left() + w * 0.04, screen.bottom() + w * 0.2),
            ];
            p.add(Shape::convex_polygon(base, shade(c, 0.8), Stroke::new(1.0, dark)));
            for i in 1..4 {
                let y = screen.bottom() + w * 0.05 * i as f32;
                p.line_segment(
                    [pos2(screen.left() + w * 0.016 * i as f32, y), pos2(screen.right() - w * 0.016 * i as f32, y)],
                    Stroke::new(1.0, shade(c, 1.3)),
                );
            }
        }
        DeviceKind::Printer => {
            // A printer: body, paper in the tray on top, a sheet coming out.
            let body = Rect::from_min_size(rect.min + vec2(w * 0.08, w * 0.36), vec2(w * 0.84, w * 0.36));
            let paper = Rect::from_min_size(rect.min + vec2(w * 0.26, w * 0.14), vec2(w * 0.48, w * 0.28));
            p.rect_filled(paper, 1.0, Color32::WHITE);
            p.rect_stroke(paper, 1.0, Stroke::new(1.0, dark), StrokeKind::Inside);
            p.rect_filled(body, w * 0.05, c);
            p.rect_filled(Rect::from_min_size(body.left_top(), vec2(body.width(), w * 0.08)), w * 0.04, light);
            let out = Rect::from_min_size(pos2(rect.left() + w * 0.24, body.bottom() - w * 0.06), vec2(w * 0.52, w * 0.16));
            p.rect_filled(out, 1.0, Color32::WHITE);
            p.rect_stroke(out, 1.0, Stroke::new(1.0, dark), StrokeKind::Inside);
            p.circle_filled(pos2(body.right() - w * 0.1, body.center().y), w * 0.03, Color32::from_rgb(0x8f, 0xf0, 0x8f));
        }
        DeviceKind::Iot => {
            // A smart thing: a rounded puck with two signal arcs above it (the model mark
            // says what it is).
            let body = Rect::from_center_size(rect.center() + vec2(0.0, w * 0.12), vec2(w * 0.7, w * 0.56));
            p.rect_filled(body.translate(vec2(0.0, w * 0.05)), w * 0.14, dark);
            p.rect_filled(body, w * 0.14, c);
            p.rect_stroke(body, w * 0.14, Stroke::new(1.0, dark), StrokeKind::Inside);
            let top = pos2(body.center().x, body.top() - w * 0.04);
            for (k, r) in [0.12f32, 0.22].into_iter().enumerate() {
                let pts: Vec<Pos2> = (0..=8)
                    .map(|i| {
                        let a = -std::f32::consts::FRAC_PI_2 - 0.7 + i as f32 * 1.4 / 8.0;
                        top + vec2(a.cos(), a.sin()) * w * r
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new((w / 24.0).max(1.0), if k == 0 { light } else { c })));
            }
        }
        DeviceKind::Phone => {
            // A desk phone: a sloped body with a small screen and a 3x4 key grid, the handset
            // lying across its left side.
            let body = vec![
                pos2(rect.left() + w * 0.12, rect.top() + w * 0.3),
                pos2(rect.right() - w * 0.08, rect.top() + w * 0.22),
                pos2(rect.right() - w * 0.02, rect.bottom() - w * 0.12),
                pos2(rect.left() + w * 0.04, rect.bottom() - w * 0.12),
            ];
            p.add(Shape::convex_polygon(body, c, Stroke::new(1.0, dark)));
            let screen = Rect::from_min_size(pos2(rect.left() + w * 0.42, rect.top() + w * 0.32), vec2(w * 0.4, w * 0.16));
            p.rect_filled(screen, w * 0.02, Color32::from_rgb(0xd8, 0xf0, 0xe0));
            for r in 0..4 {
                for k in 0..3 {
                    let at = pos2(rect.left() + w * (0.5 + k as f32 * 0.12), rect.top() + w * (0.56 + r as f32 * 0.07));
                    p.circle_filled(at, w * 0.025, light);
                }
            }
            let handset = Rect::from_min_size(pos2(rect.left() + w * 0.1, rect.top() + w * 0.2), vec2(w * 0.22, w * 0.62));
            p.rect_filled(handset, w * 0.1, dark);
            p.rect_filled(handset.shrink(w * 0.04), w * 0.08, shade(c, 0.85));
        }
        DeviceKind::Hub => {
            // A small flat box with arrows pointing outwards both ways.
            let body = Rect::from_center_size(rect.center(), vec2(w * 0.92, w * 0.36));
            p.rect_filled(body.translate(vec2(0.0, w * 0.06)), w * 0.06, dark);
            p.rect_filled(body, w * 0.06, light);
            p.rect_stroke(body, w * 0.06, Stroke::new(1.0, dark), StrokeKind::Inside);
            let y = body.center().y;
            arrow(p, pos2(body.center().x - w * 0.06, y), pos2(body.left() + w * 0.1, y), white);
            arrow(p, pos2(body.center().x + w * 0.06, y), pos2(body.right() - w * 0.1, y), white);
        }
        DeviceKind::Server => {
            // A tower: lit side, dark front with drive slots and a power light.
            let front = Rect::from_min_size(pos2(rect.center().x - w * 0.2, rect.top() + w * 0.12), vec2(w * 0.34, w * 0.8));
            let depth = w * 0.12;
            p.add(Shape::convex_polygon(
                vec![
                    front.right_top(),
                    front.right_top() + vec2(depth, -depth * 0.6),
                    front.right_bottom() + vec2(depth, -depth * 0.6),
                    front.right_bottom(),
                ],
                shade(c, 0.5),
                Stroke::NONE,
            ));
            p.add(Shape::convex_polygon(
                vec![
                    front.left_top(),
                    front.left_top() + vec2(depth, -depth * 0.6),
                    front.right_top() + vec2(depth, -depth * 0.6),
                    front.right_top(),
                ],
                light,
                Stroke::NONE,
            ));
            p.rect_filled(front, 1.0, c);
            for i in 0..3 {
                let y = front.top() + w * (0.12 + 0.12 * i as f32);
                p.line_segment([pos2(front.left() + w * 0.06, y), pos2(front.right() - w * 0.06, y)], Stroke::new(w / 22.0, shade(c, 1.6)));
            }
            p.circle_filled(pos2(front.center().x, front.bottom() - w * 0.1), w * 0.035, Color32::from_rgb(0x8f, 0xf0, 0x8f));
        }
    }
}

fn ellipse_points(c: Pos2, rx: f32, ry: f32, from: f32, to: f32) -> Vec<Pos2> {
    let n = 24;
    (0..=n).map(|i| from + (to - from) * i as f32 / n as f32).map(|a| pos2(c.x + rx * a.cos(), c.y + ry * a.sin())).collect()
}

fn arrow(p: &Painter, from: Pos2, to: Pos2, s: Stroke) {
    p.line_segment([from, to], s);
    let len = (to - from).length();
    if len < 0.5 {
        return;
    }
    let dir = (to - from) / len;
    let n = Vec2::new(-dir.y, dir.x);
    let head = len.min(12.0) * 0.5;
    p.line_segment([to, to - dir * head + n * head * 0.6], s);
    p.line_segment([to, to - dir * head - n * head * 0.6], s);
}

/// A PDU envelope (closed), as the simulation draws travelling frames.
pub fn envelope(p: &Painter, center: Pos2, size: f32, fill: Color32, stroke: Color32) {
    let r = Rect::from_center_size(center, vec2(size, size * 0.66));
    p.rect_filled(r, 2.0, fill);
    p.rect_stroke(r, 2.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    p.line_segment([r.left_top(), r.center()], Stroke::new(1.0, stroke));
    p.line_segment([r.right_top(), r.center()], Stroke::new(1.0, stroke));
}

/// A cluster's picture: a cloud of round lobes on a flat base, filled with `fill` and
/// outlined with `edge` (original drawing).
pub fn cloud(p: &Painter, rect: Rect, fill: Color32, edge: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + h * y);
    // (x, y) as fractions of the box, radius as a fraction of its height.
    let lobes = [(0.22, 0.64, 0.30), (0.42, 0.42, 0.38), (0.66, 0.44, 0.33), (0.80, 0.65, 0.26), (0.50, 0.68, 0.30)];
    let line = (h / 22.0).max(1.0);
    for (x, y, r) in lobes {
        p.circle_filled(at(x, y), h * r + line, edge);
    }
    for (x, y, r) in lobes {
        p.circle_filled(at(x, y), h * r, fill);
    }
    // A soft highlight on the top lobe.
    p.circle_filled(at(0.38, 0.32), h * 0.12, shade(fill, 1.12));
}

/// A city on the Intercity map: three towers of different heights on a ground line, with rows
/// of windows (original drawing).
pub fn city(p: &Painter, rect: Rect, fill: Color32, edge: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let line = Stroke::new((h / 28.0).max(1.0), edge);
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + h * y);
    // (left, top, right) as fractions; every tower stands on the ground at 0.92.
    let towers = [(0.06, 0.40, 0.34), (0.36, 0.08, 0.64), (0.66, 0.28, 0.94)];
    for (i, (l, t, r)) in towers.into_iter().enumerate() {
        let body = Rect::from_min_max(at(l, t), at(r, 0.92));
        let tone = if i == 1 { fill } else { shade(fill, 0.88) };
        p.rect_filled(body, 1.5, tone);
        p.rect_stroke(body, 1.5, line, StrokeKind::Inside);
        let rows = ((0.86 - t) / 0.12).floor().max(1.0) as usize;
        for row in 0..rows {
            let y = t + 0.06 + row as f32 * 0.12;
            for col in 0..2 {
                let x = l + (r - l) * (0.22 + 0.36 * col as f32);
                p.rect_filled(Rect::from_min_size(at(x, y), vec2(w * (r - l) * 0.2, h * 0.05)), 0.0, edge.gamma_multiply(0.7));
            }
        }
    }
    p.line_segment([at(0.0, 0.92), at(1.0, 0.92)], Stroke::new(line.width * 1.5, edge));
}

/// A building in a city: a block under a flat roof edge, a grid of windows and a door
/// (original drawing).
pub fn building(p: &Painter, rect: Rect, fill: Color32, edge: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let line = Stroke::new((h / 28.0).max(1.0), edge);
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + h * y);
    let body = Rect::from_min_max(at(0.14, 0.16), at(0.86, 0.94));
    p.rect_filled(body, 1.0, fill);
    p.rect_stroke(body, 1.0, line, StrokeKind::Inside);
    p.rect_filled(Rect::from_min_max(at(0.08, 0.08), at(0.92, 0.17)), 1.0, edge);
    for row in 0..3 {
        for col in 0..3 {
            let r = Rect::from_min_size(at(0.22 + 0.21 * col as f32, 0.25 + 0.17 * row as f32), vec2(w * 0.13, h * 0.1));
            p.rect_filled(r, 0.0, shade(fill, 0.75));
            p.rect_stroke(r, 0.0, Stroke::new(line.width * 0.6, edge), StrokeKind::Inside);
        }
    }
    let door = Rect::from_min_max(at(0.42, 0.76), at(0.58, 0.94));
    p.rect_filled(door, 0.0, edge.gamma_multiply(0.85));
}

/// A wiring closet: a rack cabinet with mounted units and their port lights (original drawing).
pub fn closet(p: &Painter, rect: Rect, fill: Color32, edge: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let line = Stroke::new((h / 28.0).max(1.0), edge);
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + h * y);
    let cab = Rect::from_min_max(at(0.24, 0.06), at(0.76, 0.94));
    p.rect_filled(cab, 2.0, shade(fill, 0.8));
    p.rect_stroke(cab, 2.0, line, StrokeKind::Inside);
    // Feet.
    p.rect_filled(Rect::from_min_max(at(0.26, 0.94), at(0.32, 0.98)), 0.0, edge);
    p.rect_filled(Rect::from_min_max(at(0.68, 0.94), at(0.74, 0.98)), 0.0, edge);
    for i in 0..5 {
        let unit = Rect::from_min_max(at(0.3, 0.13 + 0.15 * i as f32), at(0.7, 0.23 + 0.15 * i as f32));
        p.rect_filled(unit, 1.0, fill);
        p.rect_stroke(unit, 1.0, Stroke::new(line.width * 0.6, edge), StrokeKind::Inside);
        for j in 0..3 {
            let c = unit.left_center() + vec2(unit.width() * (0.18 + 0.14 * j as f32), 0.0);
            let lit = if (i + j) % 3 == 0 { Color32::from_rgb(0xf0, 0xa0, 0x1a) } else { Color32::from_rgb(0x3c, 0xc8, 0x2c) };
            p.circle_filled(c, (h * 0.018).max(1.0), lit);
        }
    }
}

/// Toolbar and bar glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    New,
    Open,
    Save,
    Undo,
    Redo,
    ZoomIn,
    ZoomReset,
    ZoomOut,
    Capture,
    Help,
    Select,
    Inspect,
    Delete,
    Note,
    SimplePdu,
    ComplexPdu,
    Logical,
    Physical,
    Clock,
    Simulation,
    PowerCycle,
    FastForward,
    Search,
    NetworkDevices,
    EndDevices,
    Connections,
    Cable,
    Copy,
    Paste,
    Info,
    Resize,
    Line,
    Rectangle,
    Ellipse,
    Freeform,
    Polygon,
    Cluster,
}

/// Draws a glyph in `rect` with colour `c`.
pub fn glyph(p: &Painter, rect: Rect, g: Glyph, c: Color32) {
    let s = Stroke::new((rect.width() / 11.0).clamp(1.4, 2.6), c);
    let r = rect.shrink(rect.width() * 0.14);
    let (w, h) = (r.width(), r.height());
    match g {
        Glyph::New => {
            let page = vec![
                r.left_top(),
                r.left_top() + vec2(w * 0.6, 0.0),
                r.right_top() + vec2(-w * 0.15, h * 0.25),
                r.right_bottom() + vec2(-w * 0.15, 0.0),
                r.left_bottom(),
            ];
            p.add(Shape::closed_line(page, s));
        }
        Glyph::Open => {
            p.add(Shape::closed_line(
                vec![
                    r.left_bottom(),
                    r.left_top() + vec2(0.0, h * 0.15),
                    r.left_top() + vec2(w * 0.35, h * 0.15),
                    r.left_top() + vec2(w * 0.45, h * 0.3),
                    r.right_top() + vec2(0.0, h * 0.3),
                    r.right_bottom(),
                ],
                s,
            ));
            p.line_segment([r.left_bottom(), r.left_top() + vec2(w * 0.15, h * 0.5)], s);
            p.line_segment([r.left_top() + vec2(w * 0.15, h * 0.5), r.right_top() + vec2(w * 0.1, h * 0.5)], s);
        }
        Glyph::Save => {
            p.rect_stroke(r, 2.0, s, StrokeKind::Inside);
            p.rect_filled(Rect::from_min_size(r.left_top() + vec2(w * 0.22, 0.0), vec2(w * 0.5, h * 0.32)), 0.0, c);
            p.rect_stroke(Rect::from_min_size(r.left_top() + vec2(w * 0.18, h * 0.55), vec2(w * 0.64, h * 0.45)), 0.0, s, StrokeKind::Inside);
        }
        Glyph::Undo | Glyph::Redo => {
            let flip = if g == Glyph::Redo { -1.0 } else { 1.0 };
            let cx = r.center().x;
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| std::f32::consts::PI * (0.2 + 1.1 * i as f32 / 12.0))
                .map(|a| pos2(cx + flip * (w * 0.35) * a.cos(), r.center().y + h * 0.1 - (h * 0.35) * a.sin()))
                .collect();
            if let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) {
                p.add(Shape::line(pts, s));
                let _ = first;
                p.line_segment([last, last + vec2(flip * w * 0.18, -h * 0.05)], s);
                p.line_segment([last, last + vec2(flip * w * 0.05, -h * 0.22)], s);
            }
        }
        Glyph::ZoomIn | Glyph::ZoomOut | Glyph::ZoomReset | Glyph::Inspect | Glyph::Search => {
            let lens = pos2(r.left() + w * 0.4, r.top() + h * 0.4);
            p.circle_stroke(lens, w * 0.32, s);
            p.line_segment([lens + vec2(w * 0.23, h * 0.23), r.right_bottom()], Stroke::new(2.4, c));
            match g {
                Glyph::ZoomIn => {
                    p.line_segment([lens - vec2(w * 0.15, 0.0), lens + vec2(w * 0.15, 0.0)], s);
                    p.line_segment([lens - vec2(0.0, h * 0.15), lens + vec2(0.0, h * 0.15)], s);
                }
                Glyph::ZoomOut => {
                    p.line_segment([lens - vec2(w * 0.15, 0.0), lens + vec2(w * 0.15, 0.0)], s);
                }
                Glyph::ZoomReset => {
                    p.circle_filled(lens, w * 0.07, c);
                }
                _ => {}
            }
        }
        Glyph::Capture => {
            p.rect_stroke(r, 2.0, s, StrokeKind::Inside);
            let mut wave = Vec::new();
            for i in 0..=8 {
                let x = r.left() + w * (0.1 + 0.1 * i as f32);
                let y = r.center().y + if i % 2 == 0 { -h * 0.2 } else { h * 0.2 };
                wave.push(pos2(x, y));
            }
            p.add(Shape::line(wave, s));
        }
        Glyph::Help => {
            p.text(rect.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(rect.height() * 0.8), c);
        }
        Glyph::Select => {
            p.add(Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], s, 3.0, 2.0));
        }
        Glyph::Delete => {
            let body = vec![
                r.left_top() + vec2(w * 0.3, h * 0.2),
                r.right_top() + vec2(0.0, h * 0.2),
                r.right_bottom() + vec2(0.0, -h * 0.2),
                r.left_bottom() + vec2(w * 0.3, -h * 0.2),
                r.left_top() + vec2(0.0, h * 0.5),
            ];
            p.add(Shape::closed_line(body, s));
            let x = pos2(r.left() + w * 0.65, r.center().y);
            p.line_segment([x + vec2(-w * 0.12, -h * 0.12), x + vec2(w * 0.12, h * 0.12)], s);
            p.line_segment([x + vec2(-w * 0.12, h * 0.12), x + vec2(w * 0.12, -h * 0.12)], s);
        }
        Glyph::Note => {
            p.rect_stroke(Rect::from_min_size(r.left_top() + vec2(w * 0.15, 0.0), vec2(w * 0.7, h)), 1.0, s, StrokeKind::Inside);
            for i in 0..3 {
                let y = r.top() + h * (0.3 + 0.2 * i as f32);
                p.line_segment([pos2(r.left() + w * 0.3, y), pos2(r.right() - w * 0.3, y)], s);
            }
        }
        Glyph::SimplePdu | Glyph::ComplexPdu => {
            let env = Rect::from_center_size(r.center(), vec2(w, h * 0.7));
            p.rect_filled(env, 1.5, c);
            if g == Glyph::SimplePdu {
                p.line_segment([env.left_top(), env.center()], Stroke::new(1.4, Color32::WHITE));
                p.line_segment([env.right_top(), env.center()], Stroke::new(1.4, Color32::WHITE));
            } else {
                let flap = vec![env.left_top(), env.center_top() - vec2(0.0, h * 0.35), env.right_top()];
                p.add(Shape::convex_polygon(flap, c, Stroke::NONE));
                p.line_segment([env.left_top(), env.center()], Stroke::new(1.4, Color32::WHITE));
                p.line_segment([env.right_top(), env.center()], Stroke::new(1.4, Color32::WHITE));
            }
        }
        Glyph::Logical => {
            let pts = [r.center_top() + vec2(0.0, h * 0.1), r.left_bottom() + vec2(w * 0.1, -h * 0.1), r.right_bottom() - vec2(w * 0.1, h * 0.1)];
            p.line_segment([pts[0], pts[1]], s);
            p.line_segment([pts[0], pts[2]], s);
            for q in pts {
                p.circle_filled(q, w * 0.14, c);
            }
        }
        Glyph::Physical => {
            p.rect_stroke(r, 1.0, s, StrokeKind::Inside);
            for i in 1..4 {
                let y = r.top() + h * 0.25 * i as f32;
                p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, c));
            }
        }
        Glyph::Clock => {
            p.circle_stroke(r.center(), w * 0.5, s);
            p.line_segment([r.center(), r.center() - vec2(0.0, h * 0.3)], s);
            p.line_segment([r.center(), r.center() + vec2(w * 0.22, 0.0)], s);
        }
        Glyph::Simulation => {
            // A stopwatch-like timer with a play triangle.
            p.circle_stroke(r.center() + vec2(0.0, h * 0.08), w * 0.42, s);
            p.line_segment([r.center_top(), r.center_top() - vec2(0.0, h * 0.12)], s);
            let tri = vec![r.center() + vec2(-w * 0.12, -h * 0.12), r.center() + vec2(w * 0.18, h * 0.08), r.center() + vec2(-w * 0.12, h * 0.28)];
            p.add(Shape::convex_polygon(tri, c, Stroke::NONE));
        }
        Glyph::PowerCycle => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| -std::f32::consts::FRAC_PI_2 + 0.5 + (std::f32::consts::TAU - 1.0) * i as f32 / 16.0)
                .map(|a| r.center() + vec2(a.cos(), a.sin()) * w * 0.45)
                .collect();
            p.add(Shape::line(pts, s));
            p.line_segment([r.center(), r.center() - vec2(0.0, h * 0.5)], s);
        }
        Glyph::FastForward => {
            for dx in [0.0, w * 0.45] {
                let tri = vec![r.left_top() + vec2(dx, h * 0.1), r.left_top() + vec2(dx + w * 0.5, h * 0.5), r.left_bottom() + vec2(dx, -h * 0.1)];
                p.add(Shape::convex_polygon(tri, c, Stroke::NONE));
            }
        }
        Glyph::NetworkDevices => {
            device(p, rect, DeviceKind::Router, &crate::theme::LIGHT);
        }
        Glyph::EndDevices => {
            device(p, rect, DeviceKind::Pc, &crate::theme::LIGHT);
        }
        Glyph::Copy | Glyph::Paste => {
            let back = Rect::from_min_size(r.left_top(), vec2(w * 0.62, h * 0.72));
            let front = Rect::from_min_size(r.left_top() + vec2(w * 0.32, h * 0.28), vec2(w * 0.62, h * 0.72));
            if g == Glyph::Copy {
                p.rect_stroke(back, 1.0, s, StrokeKind::Inside);
            } else {
                p.rect_filled(back, 2.0, c);
                p.rect_filled(Rect::from_min_size(back.center_top() - vec2(w * 0.12, h * 0.05), vec2(w * 0.24, h * 0.12)), 1.0, Color32::WHITE);
            }
            p.rect_filled(front, 1.0, Color32::WHITE);
            p.rect_stroke(front, 1.0, s, StrokeKind::Inside);
            for i in 1..3 {
                let y = front.top() + front.height() * 0.3 * i as f32;
                p.line_segment([pos2(front.left() + w * 0.1, y), pos2(front.right() - w * 0.1, y)], Stroke::new(1.0, c));
            }
        }
        Glyph::Info => {
            p.circle_filled(r.center(), w * 0.5, c);
            p.text(r.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(h * 0.8), Color32::WHITE);
        }
        Glyph::Resize => {
            p.add(Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], Stroke::new(1.2, c), 3.0, 2.0));
            arrow(p, r.center(), r.right_bottom() - vec2(w * 0.05, h * 0.05), s);
        }
        Glyph::Line => {
            p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(2.4, c));
        }
        Glyph::Rectangle => {
            p.rect_filled(Rect::from_center_size(r.center(), vec2(w, h * 0.55)), 1.0, c);
        }
        Glyph::Ellipse => {
            p.add(Shape::convex_polygon(ellipse_points(r.center(), w * 0.5, h * 0.3, 0.0, std::f32::consts::TAU), c, Stroke::NONE));
        }
        Glyph::Freeform => {
            let pts: Vec<Pos2> =
                (0..=12).map(|i| i as f32 / 12.0).map(|t| pos2(r.left() + w * t, r.center().y + (t * 9.0).sin() * h * 0.3)).collect();
            p.add(Shape::line(pts, Stroke::new(2.2, c)));
        }
        Glyph::Polygon => {
            let pts = vec![
                pos2(r.left(), r.top() + h * 0.35),
                pos2(r.left() + w * 0.45, r.top()),
                pos2(r.right(), r.top() + h * 0.3),
                pos2(r.left() + w * 0.75, r.bottom()),
                pos2(r.left() + w * 0.3, r.top() + h * 0.7),
            ];
            p.add(Shape::convex_polygon(pts.clone(), c.gamma_multiply(0.35), Stroke::NONE));
            p.add(Shape::closed_line(pts, Stroke::new(1.8, c)));
        }
        Glyph::Cluster => {
            cloud(p, Rect::from_center_size(r.center(), vec2(w, w * 0.7)), c, c);
        }
        Glyph::Connections | Glyph::Cable => {
            let s = Stroke::new(2.0, c);
            let a = r.left_bottom();
            let b = r.right_top();
            let m1 = pos2(a.x + w * 0.35, a.y);
            let m2 = pos2(b.x - w * 0.35, b.y);
            p.add(Shape::line(vec![a, m1, m2, b], s));
            p.circle_filled(a, 2.5, c);
            p.circle_filled(b, 2.5, c);
        }
    }
}

/// The picture of a cable kind on its Connections tile: a cable running corner to corner with
/// a plug at each end, drawn the way the workspace draws that kind (straight-through solid,
/// cross-over dashed, fiber orange, console light blue and dashed, serial red with a zigzag and
/// a clock at the DCE end); the automatic cable carries a small spark.
pub fn cable(p: &Painter, rect: Rect, kind: netlabcraft_model::cable::Cable, tk: &Tokens) {
    use netlabcraft_model::cable::Cable;
    let r = rect.shrink(rect.width() * 0.12);
    let (w, h) = (r.width(), r.height());
    let a = pos2(r.left() + w * 0.08, r.bottom() - h * 0.08);
    let b = pos2(r.right() - w * 0.08, r.top() + h * 0.08);
    let width = (rect.width() / 12.0).clamp(1.6, 2.8);
    let color = match kind {
        Cable::Fiber => tk.fiber_cable,
        Cable::Console => tk.console_cable,
        Cable::SerialDce | Cable::SerialDte => tk.serial_cable,
        _ => tk.cable,
    };
    let s = Stroke::new(width, color);
    match kind {
        Cable::Cross => p.extend(Shape::dashed_line(&[a, b], s, w * 0.16, w * 0.09)),
        Cable::Console => {
            // A loose curve, as a rollover cable lies on the desk.
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| i as f32 / 16.0)
                .map(|t| a + (b - a) * t + vec2(1.0, 1.0).normalized() * (t * std::f32::consts::PI).sin() * w * 0.18)
                .collect();
            p.extend(Shape::dashed_line(&pts, s, w * 0.12, w * 0.08));
        }
        Cable::SerialDce | Cable::SerialDte => {
            let d = (b - a).normalized();
            let n = vec2(-d.y, d.x);
            let mid = a + (b - a) * 0.5;
            let z = w * 0.12;
            p.add(Shape::line(vec![a, mid - d * z, mid + n * z, mid - n * z, mid + d * z, b], s));
            // The clock: by the end clicked first for DCE, by the far end for DTE.
            let at = if kind == Cable::SerialDce { a + (b - a) * 0.22 } else { a + (b - a) * 0.78 };
            let c = at + n * w * 0.16;
            let rad = w * 0.11;
            p.circle(c, rad, tk.chrome, Stroke::new(1.2, color));
            p.line_segment([c, c - vec2(0.0, rad * 0.7)], Stroke::new(1.1, color));
            p.line_segment([c, c + vec2(rad * 0.55, 0.0)], Stroke::new(1.1, color));
        }
        Cable::Fiber => {
            // A glow around the core.
            p.line_segment([a, b], Stroke::new(width * 2.2, color.gamma_multiply(0.3)));
            p.line_segment([a, b], s);
        }
        Cable::Straight | Cable::Auto => {
            p.line_segment([a, b], s);
        }
    }
    // Plugs: small squares at both ends.
    let plug = w * 0.13;
    for end in [a, b] {
        p.rect_filled(Rect::from_center_size(end, vec2(plug, plug)), 1.5, color);
    }
    if kind == Cable::Auto {
        // A four-pointed spark above the middle: the right kind is picked for you.
        let c = pos2(r.left() + w * 0.3, r.top() + h * 0.3);
        let k = w * 0.16;
        let pts = vec![
            c + vec2(0.0, -k),
            c + vec2(k * 0.25, -k * 0.25),
            c + vec2(k, 0.0),
            c + vec2(k * 0.25, k * 0.25),
            c + vec2(0.0, k),
            c + vec2(-k * 0.25, k * 0.25),
            c + vec2(-k, 0.0),
            c + vec2(-k * 0.25, -k * 0.25),
        ];
        p.add(Shape::closed_line(pts, Stroke::new(1.4, tk.fiber_cable)));
        p.circle_filled(c, k * 0.3, tk.fiber_cable);
    }
}
