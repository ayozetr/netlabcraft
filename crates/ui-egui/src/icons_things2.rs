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
    HOME.contains(&model_id) || CITY.contains(&model_id)
}

/// The models of the Smart Home group added in this wave.
const HOME: [&str; 8] =
    ["IoT-Appliance", "IoT-BTSpeaker", "IoT-CeilingFan", "IoT-Furnace", "IoT-Humidifier", "IoT-Humiture", "IoT-Player", "IoT-SoundDet"];

/// The models of the Smart City, Industrial and Actuators groups added in this wave.
const CITY: [&str; 12] = [
    "IoT-Pressure",
    "IoT-Beacon",
    "IoT-CO2",
    "IoT-Fire",
    "IoT-FireSprinkler",
    "IoT-Car",
    "IoT-RFIDCard",
    "IoT-RFIDReader",
    "IoT-LED",
    "IoT-Trip",
    "IoT-WindDet",
    "IoT-Signal",
];

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
        "IoT-SoundDet" => equaliser(p, c, w, line),
        _ => city(p, c, w, line, model_id),
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

/// An arc of a circle round `c`, from angle `from` to `to` (radians), as a line.
fn arc(p: &Painter, c: Pos2, r: f32, from: f32, to: f32, stroke: Stroke) {
    let pts: Vec<Pos2> = (0..=10)
        .map(|i| {
            let a = from + (to - from) * i as f32 / 10.0;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, stroke));
}

fn city(p: &Painter, c: Pos2, w: f32, line: Stroke, model_id: &str) {
    let wave = Stroke::new((w / 26.0).max(1.0), BLUE);
    match model_id {
        "IoT-Pressure" => {
            // A round gauge with a needle and ticks along its upper arc.
            p.circle_filled(c, w * 0.18, WHITE);
            p.circle_stroke(c, w * 0.18, line);
            for k in 0..=6 {
                let a = std::f32::consts::PI * (1.0 + k as f32 / 6.0);
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * w * 0.13, c + d * w * 0.16], Stroke::new(1.0, DARK));
            }
            p.line_segment([c, c + vec2(w * 0.08, -w * 0.09)], Stroke::new((w / 22.0).max(1.0), RED));
            p.circle_filled(c, w * 0.025, DARK);
        }
        "IoT-Beacon" => {
            // A dot sending rings to both sides.
            p.circle_filled(c, w * 0.045, WHITE);
            for r in [0.1f32, 0.17] {
                arc(p, c, w * r, -0.7, 0.7, wave);
                arc(p, c, w * r, std::f32::consts::PI - 0.7, std::f32::consts::PI + 0.7, wave);
            }
        }
        "IoT-CO2" => {
            p.circle_filled(c, w * 0.18, WHITE);
            p.circle_stroke(c, w * 0.18, Stroke::new((w / 28.0).max(1.0), GREEN));
            p.text(c, egui::Align2::CENTER_CENTER, "CO2", egui::FontId::proportional(w * 0.13), DARK);
        }
        "IoT-Fire" => flame(p, c, w * 1.6),
        "IoT-FireSprinkler" => {
            // A ceiling sprinkler: a pipe, a red glass bulb and a deflector throwing water down.
            p.line_segment([c + vec2(0.0, -w * 0.2), c + vec2(0.0, -w * 0.08)], Stroke::new((w / 16.0).max(1.5), WHITE));
            p.circle_filled(c + vec2(0.0, -w * 0.05), w * 0.03, RED);
            p.line_segment([c + vec2(-w * 0.09, -w * 0.01), c + vec2(w * 0.09, -w * 0.01)], Stroke::new((w / 18.0).max(1.5), WHITE));
            for k in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
                p.line_segment([c + vec2(k * w * 0.05, w * 0.02), c + vec2(k * w * 0.15, w * 0.17)], wave);
            }
        }
        "IoT-Car" => {
            // A car from the side: body, cabin, windows, wheels and a puff from the exhaust.
            let body = Rect::from_center_size(c + vec2(0.0, w * 0.03), vec2(w * 0.4, w * 0.1));
            p.rect_filled(body, w * 0.03, RED);
            p.add(Shape::convex_polygon(
                vec![
                    body.left_top() + vec2(w * 0.08, 0.0),
                    body.left_top() + vec2(w * 0.13, -w * 0.08),
                    body.right_top() + vec2(-w * 0.1, -w * 0.08),
                    body.right_top() + vec2(-w * 0.04, 0.0),
                ],
                RED,
                Stroke::NONE,
            ));
            p.rect_filled(
                Rect::from_center_size(body.center_top() + vec2(0.0, -w * 0.04), vec2(w * 0.13, w * 0.05)),
                0.0,
                Color32::from_rgb(0xcf, 0xe8, 0xf7),
            );
            for dx in [-0.12f32, 0.12] {
                p.circle_filled(body.center_bottom() + vec2(dx * w, 0.0), w * 0.045, DARK);
                p.circle_filled(body.center_bottom() + vec2(dx * w, 0.0), w * 0.018, WHITE);
            }
            p.circle_filled(body.left_bottom() + vec2(-w * 0.035, -w * 0.01), w * 0.025, Color32::from_rgb(0xb8, 0xc0, 0xc8));
        }
        "IoT-RFIDCard" => {
            // A card with a chip and a few waves.
            let card = Rect::from_center_size(c, vec2(w * 0.3, w * 0.2));
            p.rect_filled(card, w * 0.03, WHITE);
            p.rect_stroke(card, w * 0.03, line, StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(card.left_center() + vec2(w * 0.07, 0.0), vec2(w * 0.07, w * 0.06)), w * 0.01, YELLOW);
            arc(p, card.right_center() - vec2(w * 0.08, 0.0), w * 0.04, -0.8, 0.8, wave);
            arc(p, card.right_center() - vec2(w * 0.08, 0.0), w * 0.07, -0.8, 0.8, wave);
        }
        "IoT-RFIDReader" => {
            // A reader plate sending waves upward.
            let plate = Rect::from_center_size(c + vec2(0.0, w * 0.09), vec2(w * 0.3, w * 0.1));
            p.rect_filled(plate, w * 0.02, DARK);
            p.rect_stroke(plate, w * 0.02, Stroke::new(1.0, WHITE), StrokeKind::Inside);
            for r in [0.07f32, 0.12, 0.17] {
                arc(
                    p,
                    plate.center_top() + vec2(0.0, w * 0.02),
                    w * r,
                    -std::f32::consts::PI * 0.8,
                    -std::f32::consts::PI * 0.2,
                    Stroke::new((w / 28.0).max(1.0), WHITE),
                );
            }
        }
        "IoT-LED" => {
            // Red, green and blue discs that overlap into white: a colour LED.
            for (dx, dy, col) in [(-0.06f32, -0.04f32, [0xe0, 0x30, 0x30]), (0.06, -0.04, [0x30, 0xb0, 0x50]), (0.0, 0.06, [0x30, 0x70, 0xe0])] {
                p.circle_filled(c + vec2(dx * w, dy * w), w * 0.1, Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 190));
            }
        }
        "IoT-Trip" => {
            // Two posts and the beam between them, with a red lamp on each.
            for dx in [-0.17f32, 0.17] {
                p.rect_filled(Rect::from_center_size(c + vec2(dx * w, w * 0.03), vec2(w * 0.04, w * 0.22)), w * 0.01, WHITE);
                p.circle_filled(c + vec2(dx * w, -w * 0.06), w * 0.03, RED);
            }
            for k in 0..5 {
                let x = c.x - w * 0.11 + k as f32 * w * 0.055;
                p.line_segment([pos2(x, c.y - w * 0.06), pos2(x + w * 0.03, c.y - w * 0.06)], Stroke::new((w / 28.0).max(1.0), RED));
            }
        }
        "IoT-WindDet" => {
            // An anemometer: three cups on arms round a hub.
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3.0 - 1.2;
                let tip = c + vec2(a.cos(), a.sin()) * w * 0.15;
                p.line_segment([c, tip], Stroke::new((w / 28.0).max(1.0), WHITE));
                p.circle_filled(tip, w * 0.045, WHITE);
                p.circle_stroke(tip, w * 0.045, Stroke::new(1.0, DARK));
            }
            p.circle_filled(c, w * 0.03, DARK);
        }
        _ => {
            // IoT-Signal: a sine wave on a dark screen.
            let screen = Rect::from_center_size(c, vec2(w * 0.38, w * 0.24));
            p.rect_filled(screen, w * 0.02, DARK);
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    pos2(
                        screen.left() + w * 0.02 + t * (screen.width() - w * 0.04),
                        screen.center().y + (t * std::f32::consts::TAU * 2.0).sin() * w * 0.07,
                    )
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new((w / 26.0).max(1.0), Color32::from_rgb(0x7f, 0xe0, 0x8a))));
        }
    }
}

/// A flame: a red tongue with a yellow one inside, `k` times a small unit tall.
fn flame(p: &Painter, c: Pos2, k: f32) {
    let tongue = |size: f32, col: Color32| {
        let base = c + vec2(0.0, size * 0.55);
        p.add(Shape::convex_polygon(
            vec![
                base + vec2(-size * 0.4, 0.0),
                base + vec2(-size * 0.5, -size * 0.45),
                base + vec2(-size * 0.1, -size * 0.8),
                base + vec2(0.0, -size * 1.1),
                base + vec2(size * 0.25, -size * 0.7),
                base + vec2(size * 0.5, -size * 0.4),
                base + vec2(size * 0.4, 0.0),
            ],
            col,
            Stroke::NONE,
        ));
    };
    tongue(k * 0.2, RED);
    tongue(k * 0.11, YELLOW);
}
