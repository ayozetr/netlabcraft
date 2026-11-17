//! Pictures for the smart things added with the second wave of the device box (original
//! artwork, drawn in code): home appliances and monitors, then city and industrial things.
//! Each is a small picture on the thing's puck, in the same palette as `icons_parts`.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

const DARK: Color32 = Color32::from_rgb(0x2a, 0x33, 0x40);
const RED: Color32 = Color32::from_rgb(0xd6, 0x45, 0x3a);
const GREEN: Color32 = Color32::from_rgb(0x2e, 0x8b, 0x57);
const BLUE: Color32 = Color32::from_rgb(0x3a, 0x8f, 0xd6);
const YELLOW: Color32 = Color32::from_rgb(0xf6, 0xd3, 0x4a);
const WHITE: Color32 = Color32::WHITE;

/// Whether this module draws the model's picture.
pub fn draws(model_id: &str) -> bool {
    HOME.contains(&model_id)
}

/// The models of the Smart Home group added in this wave.
const HOME: [&str; 8] =
    ["IoT-Appliance", "IoT-BTSpeaker", "IoT-CeilingFan", "IoT-Furnace", "IoT-Humidifier", "IoT-Humiture", "IoT-Player", "IoT-SoundDet"];

/// Draws the picture of a model on its puck; `false`: not one of this module's models.
pub fn mark(p: &Painter, rect: Rect, model_id: &str) -> bool {
    if !draws(model_id) {
        return false;
    }
    let w = rect.width();
    let c = rect.center() + vec2(0.0, w * 0.12);
    let line = Stroke::new((w / 24.0).max(1.0), DARK);
    match model_id {
        "IoT-Appliance" => appliance(p, c, w, line),
        "IoT-BTSpeaker" => {
            speaker(p, c - vec2(w * 0.07, 0.0), w, line);
            bluetooth(p, c + vec2(w * 0.14, -w * 0.02), w * 0.2, BLUE);
        }
        "IoT-CeilingFan" => ceiling_fan(p, c, w),
        "IoT-Furnace" => furnace(p, c, w, line),
        "IoT-Humidifier" => humidifier(p, c, w, line),
        "IoT-Humiture" => {
            thermometer(p, c - vec2(w * 0.09, 0.0), w);
            water_drop(p, c + vec2(w * 0.12, w * 0.03), w * 0.55);
        }
        "IoT-Player" => player(p, c, w),
        _ => equaliser(p, c, w, line),
    }
    true
}

fn appliance(p: &Painter, c: Pos2, w: f32, line: Stroke) {
    let body = Rect::from_center_size(c, vec2(w * 0.3, w * 0.34));
    p.rect_filled(body, w * 0.04, WHITE);
    p.rect_stroke(body, w * 0.04, line, StrokeKind::Inside);
    // The power symbol: a ring open at the top and a bar through the gap.
    let ring: Vec<Pos2> = (0..=16)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + 0.6 + i as f32 * (std::f32::consts::TAU - 1.2) / 16.0;
            c + vec2(a.cos(), a.sin()) * w * 0.08
        })
        .collect();
    let power = Stroke::new((w / 20.0).max(1.2), GREEN);
    p.add(Shape::line(ring, power));
    p.line_segment([c + vec2(0.0, -w * 0.11), c + vec2(0.0, -w * 0.02)], power);
}

fn speaker(p: &Painter, c: Pos2, w: f32, line: Stroke) {
    let cabinet = Rect::from_center_size(c, vec2(w * 0.22, w * 0.34));
    p.rect_filled(cabinet, w * 0.03, WHITE);
    p.rect_stroke(cabinet, w * 0.03, line, StrokeKind::Inside);
    p.circle_filled(c - vec2(0.0, w * 0.1), w * 0.035, DARK);
    p.circle_filled(c + vec2(0.0, w * 0.06), w * 0.075, DARK);
}

/// The Bluetooth rune: one stroke through a stem, in a coloured disc.
fn bluetooth(p: &Painter, c: Pos2, r: f32, col: Color32) {
    p.circle_filled(c, r, col);
    let k = r * 0.8;
    let pts: Vec<Pos2> = [(-0.45, -0.3), (0.45, 0.3), (0.0, 0.7), (0.0, -0.7), (0.45, -0.3), (-0.45, 0.3)]
        .into_iter()
        .map(|(x, y): (f32, f32)| c + vec2(x * k, y * k))
        .collect();
    p.add(Shape::line(pts, Stroke::new((r / 4.5).max(1.0), WHITE)));
}

fn ceiling_fan(p: &Painter, c: Pos2, w: f32) {
    // Seen from below: four broad blades round a hub.
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2 + 0.4;
        let (dir, side) = (vec2(a.cos(), a.sin()), vec2(-a.sin(), a.cos()));
        let blade = vec![
            c + dir * w * 0.06 - side * w * 0.025,
            c + dir * w * 0.2 - side * w * 0.05,
            c + dir * w * 0.2 + side * w * 0.05,
            c + dir * w * 0.06 + side * w * 0.04,
        ];
        p.add(Shape::convex_polygon(blade, WHITE, Stroke::new(1.0, DARK)));
    }
    p.circle_filled(c, w * 0.05, DARK);
}

fn furnace(p: &Painter, c: Pos2, w: f32, line: Stroke) {
    let body = Rect::from_center_size(c, vec2(w * 0.32, w * 0.34));
    p.rect_filled(body, w * 0.03, WHITE);
    p.rect_stroke(body, w * 0.03, line, StrokeKind::Inside);
    let door = Rect::from_center_size(c + vec2(0.0, w * 0.03), vec2(w * 0.2, w * 0.15));
    p.rect_filled(door, w * 0.02, DARK);
    // A flame in the door: an outer red tongue and an inner yellow one.
    let tongue = |size: f32, col: Color32| {
        let base = door.center_bottom() - vec2(0.0, w * 0.01);
        p.add(Shape::convex_polygon(
            vec![
                base + vec2(-size * 0.5, 0.0),
                base + vec2(-size * 0.3, -size * 1.0),
                base + vec2(0.0, -size * 1.5),
                base + vec2(size * 0.3, -size * 0.9),
                base + vec2(size * 0.5, 0.0),
            ],
            col,
            Stroke::NONE,
        ));
    };
    tongue(w * 0.07, RED);
    tongue(w * 0.04, YELLOW);
    for dx in [-0.09f32, 0.0, 0.09] {
        p.line_segment([body.center_top() + vec2(dx * w, w * 0.03), body.center_top() + vec2(dx * w, w * 0.07)], line);
    }
}

/// A water drop of height about `w * k * 0.3`, point up.
fn water_drop(p: &Painter, c: Pos2, k: f32) {
    let r = k * 0.12;
    let mut pts = vec![c + vec2(0.0, -r * 2.2)];
    pts.extend((0..=10).map(|i| {
        let a = -0.35 + i as f32 * (std::f32::consts::PI + 0.7) / 10.0;
        c + vec2(a.cos() * r, a.sin() * r + r * 0.2)
    }));
    p.add(Shape::convex_polygon(pts, BLUE, Stroke::new(1.0, DARK)));
}

fn humidifier(p: &Painter, c: Pos2, w: f32, line: Stroke) {
    let tank = Rect::from_center_size(c + vec2(0.0, w * 0.07), vec2(w * 0.26, w * 0.2));
    p.rect_filled(tank, w * 0.05, WHITE);
    p.rect_stroke(tank, w * 0.05, line, StrokeKind::Inside);
    p.rect_filled(Rect::from_center_size(tank.center_top() - vec2(0.0, w * 0.02), vec2(w * 0.07, w * 0.05)), w * 0.01, DARK);
    // Mist: three little wavy lines rising from the nozzle.
    for dx in [-0.07f32, 0.0, 0.07] {
        let top = c + vec2(dx * w, -w * 0.06);
        let pts: Vec<Pos2> = (0..=6).map(|i| top + vec2((i as f32 * 1.6).sin() * w * 0.018, -i as f32 * w * 0.022)).collect();
        p.add(Shape::line(pts, Stroke::new((w / 28.0).max(1.0), BLUE)));
    }
}

fn thermometer(p: &Painter, c: Pos2, w: f32) {
    let tube = Rect::from_center_size(c - vec2(0.0, w * 0.04), vec2(w * 0.07, w * 0.26));
    p.rect_filled(tube, w * 0.035, WHITE);
    p.rect_filled(Rect::from_min_max(pos2(tube.left() + w * 0.02, tube.center().y), pos2(tube.right() - w * 0.02, tube.bottom())), 0.0, RED);
    p.circle_filled(pos2(c.x, tube.bottom()), w * 0.06, WHITE);
    p.circle_filled(pos2(c.x, tube.bottom()), w * 0.042, RED);
}

fn player(p: &Painter, c: Pos2, w: f32) {
    let body = Rect::from_center_size(c, vec2(w * 0.24, w * 0.36));
    p.rect_filled(body, w * 0.04, DARK);
    p.rect_stroke(body, w * 0.04, Stroke::new(1.0, WHITE), StrokeKind::Inside);
    let screen = Rect::from_center_size(body.center_top() + vec2(0.0, w * 0.09), vec2(w * 0.17, w * 0.1));
    p.rect_filled(screen, w * 0.01, Color32::from_rgb(0x7f, 0xc8, 0x6a));
    p.add(Shape::convex_polygon(
        vec![screen.center() + vec2(-w * 0.02, -w * 0.03), screen.center() + vec2(w * 0.035, 0.0), screen.center() + vec2(-w * 0.02, w * 0.03)],
        DARK,
        Stroke::NONE,
    ));
    // The click wheel.
    let wheel = body.center_bottom() - vec2(0.0, w * 0.08);
    p.circle_filled(wheel, w * 0.06, WHITE);
    p.circle_filled(wheel, w * 0.02, DARK);
}

fn equaliser(p: &Painter, c: Pos2, w: f32, line: Stroke) {
    let face = Rect::from_center_size(c, vec2(w * 0.36, w * 0.28));
    p.rect_filled(face, w * 0.03, WHITE);
    p.rect_stroke(face, w * 0.03, line, StrokeKind::Inside);
    for (i, h) in [0.35f32, 0.7, 1.0, 0.55, 0.8].into_iter().enumerate() {
        let x = face.left() + w * 0.05 + i as f32 * w * 0.065;
        let bottom = face.bottom() - w * 0.04;
        p.rect_filled(Rect::from_min_max(pos2(x - w * 0.02, bottom - h * w * 0.18), pos2(x + w * 0.02, bottom)), 0.0, BLUE);
    }
}
