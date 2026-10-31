//! Icons for the device families, drawn in code (original artwork): the smart things' marks,
//! the home gateway's mark, the cloud and the modems, and the coaxial and phone cables.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

use crate::theme::Tokens;

/// What a smart thing wears on its puck.
pub fn iot_mark(p: &Painter, rect: Rect, model_id: &str) {
    let w = rect.width();
    let c = rect.center() + vec2(0.0, w * 0.12);
    let white = Color32::WHITE;
    let dark = Color32::from_rgb(0x2a, 0x33, 0x40);
    let red = Color32::from_rgb(0xd6, 0x45, 0x3a);
    match model_id {
        "IoT-Motion" => {
            // An eye: what moves in front of it is seen.
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = i as f32 / 16.0 * std::f32::consts::TAU;
                    c + vec2(a.cos() * w * 0.24, a.sin() * w * 0.12)
                })
                .collect();
            p.add(Shape::convex_polygon(pts, white, Stroke::new(1.0, dark)));
            p.circle_filled(c, w * 0.08, dark);
        }
        "IoT-Smoke" => {
            // A ceiling disc with three puffs above it.
            p.circle_filled(c + vec2(0.0, w * 0.06), w * 0.16, white);
            p.circle_stroke(c + vec2(0.0, w * 0.06), w * 0.16, Stroke::new(1.0, dark));
            for (i, dx) in [-0.1f32, 0.0, 0.1].into_iter().enumerate() {
                p.circle_filled(c + vec2(dx * w, -w * (0.12 + 0.03 * i as f32)), w * 0.05, Color32::from_rgb(0xb0, 0xb6, 0xbe));
            }
        }
        "IoT-Light" => {
            p.circle_filled(c - vec2(0.0, w * 0.03), w * 0.15, Color32::from_rgb(0xf6, 0xd3, 0x4a));
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, w * 0.15), vec2(w * 0.12, w * 0.08)), 1.0, white);
        }
        "IoT-Door" => {
            // A door with its handle.
            let door = Rect::from_center_size(c, vec2(w * 0.26, w * 0.4));
            p.rect_filled(door, 1.5, white);
            p.rect_stroke(door, 1.5, Stroke::new(1.0, dark), StrokeKind::Inside);
            p.circle_filled(pos2(door.right() - w * 0.06, door.center().y), w * 0.025, dark);
        }
        "IoT-Siren" => {
            p.add(Shape::convex_polygon(
                vec![c + vec2(-w * 0.14, w * 0.16), c + vec2(w * 0.14, w * 0.16), c + vec2(w * 0.09, -w * 0.1), c + vec2(-w * 0.09, -w * 0.1)],
                red,
                Stroke::new(1.0, dark),
            ));
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, w * 0.18), vec2(w * 0.34, w * 0.05)), 1.0, white);
        }
        _ => {
            // A webcam: a round lens on a small stand.
            p.circle_filled(c - vec2(0.0, w * 0.04), w * 0.15, white);
            p.circle_filled(c - vec2(0.0, w * 0.04), w * 0.08, dark);
            p.circle_filled(c - vec2(w * 0.03, w * 0.07), w * 0.025, white);
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, w * 0.17), vec2(w * 0.2, w * 0.04)), 1.0, white);
        }
    }
}

/// A home gateway's mark: a small house over the access-point box.
pub fn gateway_mark(p: &Painter, rect: Rect) {
    let w = rect.width();
    let c = pos2(rect.right() - w * 0.16, rect.top() + w * 0.12);
    let green = Color32::from_rgb(0x2e, 0x8b, 0x57);
    p.add(Shape::convex_polygon(vec![c + vec2(0.0, -w * 0.12), c + vec2(w * 0.13, 0.0), c + vec2(-w * 0.13, 0.0)], green, Stroke::NONE));
    p.rect_filled(Rect::from_min_max(c + vec2(-w * 0.09, 0.0), c + vec2(w * 0.09, w * 0.12)), 0.0, green);
    p.rect_filled(Rect::from_min_max(c + vec2(-w * 0.025, w * 0.04), c + vec2(w * 0.025, w * 0.12)), 0.0, Color32::WHITE);
}

/// Whether a model is drawn whole by [`wan_icon`] (the cloud and the modems).
pub fn is_wan(model_id: &str) -> bool {
    matches!(model_id, "Cloud" | "DSL-Modem" | "Cable-Modem")
}

/// The cloud and the modems.
pub fn wan_icon(p: &Painter, rect: Rect, model_id: &str, tk: &Tokens) {
    let w = rect.width();
    if model_id == "Cloud" {
        let fill = Color32::from_rgb(0xd9, 0xe6, 0xf5);
        let edge = Color32::from_rgb(0x4a, 0x6f, 0x9c);
        crate::icons::cloud(p, rect.shrink(w * 0.02), fill, edge);
        // Three port marks under the cloud: Ethernet, coaxial, phone.
        let y = rect.center().y + w * 0.08;
        p.rect_filled(Rect::from_center_size(pos2(rect.center().x - w * 0.18, y), vec2(w * 0.1, w * 0.08)), 1.0, edge);
        p.circle_filled(pos2(rect.center().x, y), w * 0.05, edge);
        p.circle_filled(pos2(rect.center().x, y), w * 0.02, fill);
        p.rect_filled(Rect::from_center_size(pos2(rect.center().x + w * 0.18, y), vec2(w * 0.07, w * 0.07)), 0.5, edge);
        return;
    }
    // A modem: a small upright box with a row of lights and the socket of its line.
    let body = Rect::from_center_size(rect.center() + vec2(0.0, w * 0.06), vec2(w * 0.78, w * 0.46));
    let base = Color32::from_rgb(0x55, 0x6b, 0x82);
    p.rect_filled(body.translate(vec2(0.0, w * 0.05)), w * 0.06, crate::theme::shade(base, 0.6));
    p.rect_filled(body, w * 0.06, base);
    p.rect_stroke(body, w * 0.06, Stroke::new(1.0, crate::theme::shade(base, 0.5)), StrokeKind::Inside);
    for i in 0..4 {
        let at = pos2(body.left() + w * (0.12 + 0.1 * i as f32), body.top() + w * 0.1);
        p.circle_filled(at, w * 0.03, if i < 3 { tk.link_up } else { Color32::from_rgb(0xf2, 0xc2, 0x3a) });
    }
    let jack = pos2(body.right() - w * 0.16, body.center().y + w * 0.04);
    if model_id == "Cable-Modem" {
        p.circle_filled(jack, w * 0.08, Color32::from_rgb(0xc8, 0xa2, 0x4a));
        p.circle_filled(jack, w * 0.03, Color32::from_rgb(0x2a, 0x2a, 0x2a));
    } else {
        p.rect_filled(Rect::from_center_size(jack, vec2(w * 0.13, w * 0.11)), 1.0, Color32::WHITE);
        p.rect_filled(Rect::from_center_size(jack + vec2(0.0, w * 0.02), vec2(w * 0.07, w * 0.05)), 0.5, Color32::from_rgb(0x2a, 0x2a, 0x2a));
    }
    // Waves for the line it bridges.
    let s = Stroke::new((w / 28.0).max(1.0), Color32::WHITE);
    let from = pos2(body.left() + w * 0.1, body.bottom() - w * 0.12);
    let pts: Vec<Pos2> = (0..=12).map(|i| from + vec2(i as f32 * w * 0.04, (i as f32 * 1.2).sin() * w * 0.03)).collect();
    p.add(Shape::line(pts, s));
}

/// A coaxial cable: a thick sheath with its core showing.
pub fn coax_line(p: &Painter, a: Pos2, b: Pos2, width: f32, tk: &Tokens) {
    p.line_segment([a, b], Stroke::new(width * 2.0, tk.cable));
    p.line_segment([a, b], Stroke::new((width * 0.5).max(1.0), tk.chrome));
}

/// A phone (RJ-11) cable: a thin line with small dots along it.
pub fn phone_line(p: &Painter, a: Pos2, b: Pos2, width: f32, tk: &Tokens) {
    let s = Stroke::new((width * 0.7).max(1.0), tk.serial_cable);
    p.line_segment([a, b], s);
    let len = (b - a).length();
    let n = (len / 14.0).floor().clamp(1.0, 64.0) as usize;
    for i in 1..n {
        p.circle_filled(a + (b - a) * (i as f32 / n as f32), width * 0.8, tk.serial_cable);
    }
}

/// A device's icon: the cloud and the modems drawn whole, every other model as its kind with
/// its marks.
pub fn model_icon(p: &Painter, rect: Rect, kind: netlabcraft_model::DeviceKind, model_id: &str, multilayer: bool, tk: &Tokens) {
    if is_wan(model_id) {
        wan_icon(p, rect, model_id, tk);
    } else if matches!(model_id, "Board-SBC" | "Board-MCU") {
        board_icon(p, rect, model_id == "Board-SBC");
    } else if matches!(model_id, "Tablet" | "Smartphone") {
        touch_icon(p, rect, model_id == "Tablet");
    } else {
        crate::icons::device(p, rect, kind, tk);
        crate::icons::model_marks(p, rect, model_id, multilayer);
    }
}

/// A tablet (wide) or a smartphone (tall): a dark body, a bright screen with a few app squares
/// and the Wi-Fi waves over it.
pub fn touch_icon(p: &Painter, rect: Rect, tablet: bool) {
    let w = rect.width();
    let size = if tablet { vec2(w * 0.86, w * 0.62) } else { vec2(w * 0.46, w * 0.82) };
    let body = Rect::from_center_size(rect.center() + vec2(0.0, w * 0.06), size);
    let edge = Color32::from_rgb(0x1f, 0x24, 0x2b);
    p.rect_filled(body.translate(vec2(0.0, w * 0.03)), w * 0.08, crate::theme::shade(edge, 0.7));
    p.rect_filled(body, w * 0.08, edge);
    let screen = body.shrink2(if tablet { vec2(w * 0.07, w * 0.05) } else { vec2(w * 0.04, w * 0.08) });
    p.rect_filled(screen, w * 0.03, Color32::from_rgb(0x4b, 0x83, 0xb5));
    let teal = Color32::from_rgb(0x9a, 0xe6, 0xf2);
    let (cols, rows) = if tablet { (4, 2) } else { (2, 3) };
    let cell = vec2(screen.width() / cols as f32, screen.height() / rows as f32);
    for r in 0..rows {
        for c in 0..cols {
            let at = screen.left_top() + vec2((c as f32 + 0.5) * cell.x, (r as f32 + 0.5) * cell.y);
            p.rect_filled(Rect::from_center_size(at, cell * 0.5), w * 0.015, teal);
        }
    }
    // Wi-Fi waves above the body.
    let c = pos2(body.center().x, body.top() - w * 0.02);
    for (i, r) in [0.06f32, 0.11, 0.16].into_iter().enumerate() {
        let pts: Vec<Pos2> = (0..=8)
            .map(|k| {
                let a = -std::f32::consts::FRAC_PI_2 - 0.7 + k as f32 * 1.4 / 8.0;
                c + vec2(a.cos(), a.sin()) * w * r
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new((w / 30.0).max(1.0), if i == 0 { edge } else { Color32::from_rgb(0x2e, 0x8b, 0x57) })));
    }
}

/// The Wi-Fi things' marks (thermostat, heater, air conditioner, humidity, window, soil
/// moisture, sprinkler, carbon monoxide, smart plug, garage door), with small Wi-Fi waves in
/// the corner of the puck.
pub fn wifi_thing_mark(p: &Painter, rect: Rect, model_id: &str) {
    let w = rect.width();
    let c = rect.center() + vec2(0.0, w * 0.12);
    let white = Color32::WHITE;
    let dark = Color32::from_rgb(0x2a, 0x33, 0x40);
    let red = Color32::from_rgb(0xd6, 0x45, 0x3a);
    let blue = Color32::from_rgb(0x3a, 0x8f, 0xd6);
    let thin = Stroke::new((w / 32.0).max(1.0), dark);
    match model_id {
        "IoT-Thermostat" => {
            // A round dial with its needle.
            p.circle_filled(c, w * 0.17, white);
            p.circle_stroke(c, w * 0.17, thin);
            p.line_segment([c, c + vec2(w * 0.09, -w * 0.08)], Stroke::new((w / 22.0).max(1.0), red));
            p.circle_filled(c, w * 0.025, dark);
        }
        "IoT-Heater" => {
            // A radiator: three fins with a wavy heat line above.
            for k in -1..=1 {
                let fin = Rect::from_center_size(c + vec2(k as f32 * w * 0.1, w * 0.04), vec2(w * 0.07, w * 0.22));
                p.rect_filled(fin, w * 0.02, white);
                p.rect_stroke(fin, w * 0.02, thin, StrokeKind::Inside);
            }
            let pts: Vec<Pos2> =
                (0..=10).map(|i| c + vec2(-w * 0.14 + i as f32 * w * 0.028, -w * 0.13 + (i as f32 * 1.3).sin() * w * 0.025)).collect();
            p.add(Shape::line(pts, Stroke::new((w / 26.0).max(1.0), red)));
        }
        "IoT-AC" => {
            // A wall unit with a snowflake.
            let unit = Rect::from_center_size(c - vec2(0.0, w * 0.04), vec2(w * 0.36, w * 0.14));
            p.rect_filled(unit, w * 0.03, white);
            p.rect_stroke(unit, w * 0.03, thin, StrokeKind::Inside);
            let f = c + vec2(0.0, w * 0.14);
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::PI / 3.0;
                let d = vec2(a.cos(), a.sin()) * w * 0.08;
                p.line_segment([f - d, f + d], Stroke::new((w / 26.0).max(1.0), blue));
            }
        }
        "IoT-Humidity" => {
            // A drop.
            p.add(Shape::convex_polygon(drop_points(c, w), blue, Stroke::new(1.0, dark)));
        }
        "IoT-Window" => {
            // A window frame of four panes.
            let frame = Rect::from_center_size(c, vec2(w * 0.32, w * 0.34));
            p.rect_filled(frame, 1.5, Color32::from_rgb(0xcf, 0xe8, 0xf7));
            p.rect_stroke(frame, 1.5, Stroke::new((w / 24.0).max(1.0), white), StrokeKind::Inside);
            p.line_segment([frame.center_top(), frame.center_bottom()], Stroke::new((w / 24.0).max(1.0), white));
            p.line_segment([frame.left_center(), frame.right_center()], Stroke::new((w / 24.0).max(1.0), white));
        }
        "IoT-Soil" => {
            // A probe in a mound of earth.
            let earth = Color32::from_rgb(0x8a, 0x5a, 0x2e);
            p.add(Shape::convex_polygon(
                vec![c + vec2(-w * 0.2, w * 0.16), c + vec2(w * 0.2, w * 0.16), c + vec2(w * 0.12, w * 0.04), c + vec2(-w * 0.12, w * 0.04)],
                earth,
                Stroke::NONE,
            ));
            p.rect_filled(Rect::from_center_size(c - vec2(0.0, w * 0.06), vec2(w * 0.05, w * 0.26)), 1.0, white);
            p.circle_filled(c - vec2(0.0, w * 0.2), w * 0.04, Color32::from_rgb(0x2e, 0x8b, 0x57));
        }
        "IoT-Sprinkler" => {
            // A sprinkler head throwing three jets.
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, w * 0.12), vec2(w * 0.08, w * 0.14)), 1.0, white);
            for k in [-1.0f32, 0.0, 1.0] {
                p.line_segment([c + vec2(0.0, w * 0.04), c + vec2(k * w * 0.16, -w * 0.12)], Stroke::new((w / 26.0).max(1.0), blue));
                p.circle_filled(c + vec2(k * w * 0.17, -w * 0.13), w * 0.025, blue);
            }
        }
        "IoT-CO" => {
            // A detector disc with the letters CO.
            p.circle_filled(c, w * 0.17, white);
            p.circle_stroke(c, w * 0.17, Stroke::new((w / 28.0).max(1.0), red));
            p.text(c, egui::Align2::CENTER_CENTER, "CO", egui::FontId::proportional(w * 0.16), dark);
        }
        "IoT-Plug" => {
            // A socket: a rounded face with two holes.
            let face = Rect::from_center_size(c, vec2(w * 0.3, w * 0.3));
            p.rect_filled(face, w * 0.07, white);
            p.rect_stroke(face, w * 0.07, thin, StrokeKind::Inside);
            for dx in [-0.06f32, 0.06] {
                p.circle_filled(c + vec2(dx * w, 0.0), w * 0.03, dark);
            }
        }
        _ => {
            // A garage: a roof over a door of slats.
            let door = Rect::from_center_size(c + vec2(0.0, w * 0.05), vec2(w * 0.32, w * 0.22));
            p.add(Shape::convex_polygon(
                vec![door.left_top() - vec2(w * 0.04, 0.0), door.right_top() + vec2(w * 0.04, 0.0), door.center_top() - vec2(0.0, w * 0.12)],
                white,
                Stroke::NONE,
            ));
            p.rect_filled(door, 1.0, white);
            for k in 1..4 {
                let y = door.top() + k as f32 * door.height() / 4.0;
                p.line_segment([egui::pos2(door.left(), y), egui::pos2(door.right(), y)], thin);
            }
        }
    }
    // Wi-Fi: two small arcs at the top right.
    let at = pos2(rect.right() - w * 0.2, rect.top() + w * 0.3);
    for r in [0.05f32, 0.09] {
        let pts: Vec<Pos2> = (0..=6)
            .map(|k| {
                let a = -std::f32::consts::FRAC_PI_2 - 0.7 + k as f32 * 1.4 / 6.0;
                at + vec2(a.cos(), a.sin()) * w * r
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new((w / 30.0).max(1.0), white)));
    }
}

/// A water drop's outline: a point on top and a round bottom.
fn drop_points(c: Pos2, w: f32) -> Vec<Pos2> {
    let mut pts = vec![c - vec2(0.0, w * 0.17)];
    let centre = c + vec2(0.0, w * 0.05);
    let r = w * 0.11;
    pts.extend((0..=12).map(|i| {
        let a = -0.35 + i as f32 * (std::f32::consts::PI + 0.7) / 12.0;
        centre + vec2(a.cos(), a.sin()) * r
    }));
    pts
}

/// An IoT custom cable: a purple line of short dashes with a small pin plug at each end.
pub fn pin_line(p: &Painter, a: Pos2, b: Pos2, width: f32, tk: &Tokens) {
    let s = Stroke::new((width * 0.9).max(1.0), tk.iot_cable);
    p.extend(Shape::dashed_line(&[a, b], s, width * 2.5, width * 1.5));
    for at in [a, b] {
        p.rect_filled(Rect::from_center_size(at, vec2(width * 2.4, width * 2.4)), 1.0, tk.iot_cable);
    }
}

/// A programmable board's icon: a green (single-board computer) or blue (microcontroller)
/// circuit board with a chip and a row of pins.
pub fn board_icon(p: &Painter, rect: Rect, sbc: bool) {
    let w = rect.width();
    let body = Rect::from_center_size(rect.center() + vec2(0.0, w * 0.04), vec2(w * 0.86, w * 0.56));
    let base = if sbc { Color32::from_rgb(0x1f, 0x6e, 0x43) } else { Color32::from_rgb(0x1d, 0x4f, 0x8a) };
    p.rect_filled(body.translate(vec2(0.0, w * 0.04)), w * 0.05, crate::theme::shade(base, 0.6));
    p.rect_filled(body, w * 0.05, base);
    let chip = Rect::from_center_size(body.center() + vec2(-w * 0.1, w * 0.04), vec2(w * 0.24, w * 0.24));
    p.rect_filled(chip, w * 0.02, Color32::from_rgb(0x22, 0x22, 0x26));
    let gold = Color32::from_rgb(0xd8, 0xc8, 0x8a);
    for k in 0..8 {
        let x = body.left() + w * 0.08 + k as f32 * w * 0.09;
        p.rect_filled(Rect::from_center_size(pos2(x, body.top() + w * 0.07), vec2(w * 0.045, w * 0.045)), 0.5, gold);
    }
    if sbc {
        p.rect_filled(
            Rect::from_min_size(body.right_center() + vec2(-w * 0.22, -w * 0.06), vec2(w * 0.18, w * 0.16)),
            1.0,
            Color32::from_rgb(0xb8, 0xbc, 0xc4),
        );
    } else {
        let at = body.right_center() + vec2(-w * 0.22, w * 0.02);
        let pts: Vec<Pos2> = (0..6).map(|i| at + vec2((i / 2) as f32 * w * 0.07, if (i + 1) % 4 < 2 { -w * 0.08 } else { w * 0.08 })).collect();
        p.add(Shape::line(pts, Stroke::new((w / 30.0).max(1.0), gold)));
    }
}
