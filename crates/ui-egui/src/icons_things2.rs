//! Pictures for the smart things added with the second wave of the device box (original artwork,
//! drawn in code): home appliances and monitors, city things and a signal generator. They are drawn
//! like the rest of `icons_things`: a silhouette of the thing's own, laid out in fractions of the
//! icon's width and painted with the theme's `art_*` tokens.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Color32, Pos2, Shape, Stroke, vec2};

use crate::icons_things::Art;
use crate::theme::shade;

/// The models this module draws.
const MODELS: [&str; 20] = [
    "IoT-Appliance",
    "IoT-BTSpeaker",
    "IoT-CeilingFan",
    "IoT-Furnace",
    "IoT-Humidifier",
    "IoT-Humiture",
    "IoT-Player",
    "IoT-SoundDet",
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

/// Whether this module draws the model's picture.
pub fn draws(model_id: &str) -> bool {
    MODELS.contains(&model_id)
}

/// Draws the picture of one of this module's models.
pub(crate) fn icon(a: &Art, model_id: &str) {
    match model_id {
        "IoT-Appliance" => appliance(a),
        "IoT-BTSpeaker" => bluetooth_speaker(a),
        "IoT-CeilingFan" => ceiling_fan(a),
        "IoT-Furnace" => furnace(a),
        "IoT-Humidifier" => humidifier(a),
        "IoT-Humiture" => humiture(a),
        "IoT-Player" => player(a),
        "IoT-SoundDet" => sound_detector(a),
        "IoT-Pressure" => pressure(a),
        "IoT-Beacon" => beacon(a),
        "IoT-CO2" => carbon_dioxide(a),
        "IoT-Fire" => fire_monitor(a),
        "IoT-FireSprinkler" => fire_sprinkler(a),
        "IoT-Car" => car(a),
        "IoT-RFIDCard" => rfid_card(a),
        "IoT-RFIDReader" => rfid_reader(a),
        "IoT-LED" => smart_led(a),
        "IoT-Trip" => trip_sensor(a),
        "IoT-WindDet" => wind_detector(a),
        _ => signal_generator(a),
    }
}

/// A flame as a pair of outlined tongues, its base at `(x, y)` and `size` tall (fractions).
fn flame(a: &Art, x: f32, y: f32, size: f32) {
    let t = a.tk;
    for (k, col) in [(1.0f32, t.art_red), (0.55, t.art_amber)] {
        let s = size * k;
        let pts = vec![
            a.at(x - s * 0.34, y),
            a.at(x - s * 0.42, y - s * 0.4),
            a.at(x - s * 0.1, y - s * 0.62),
            a.at(x, y - s),
            a.at(x + s * 0.2, y - s * 0.6),
            a.at(x + s * 0.42, y - s * 0.36),
            a.at(x + s * 0.34, y),
        ];
        if k == 1.0 {
            a.poly(pts, col);
        } else {
            a.poly_plain(pts, col);
        }
    }
}

fn appliance(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.2, 0.1, 0.8, 0.9), 0.08, t.art_paper);
    // The power symbol: a ring open at the top and a bar through the gap.
    a.arc(a.at(0.5, 0.52), 0.18, -FRAC_PI_2 + 0.6, -FRAC_PI_2 + TAU - 0.6, 0.05, t.art_green);
    a.line(a.at(0.5, 0.28), a.at(0.5, 0.5), 0.05, t.art_green);
    a.plain(a.rc(0.28, 0.9, 0.38, 0.95), 0.0, t.art_ink);
    a.plain(a.rc(0.62, 0.9, 0.72, 0.95), 0.0, t.art_ink);
    a.dot(a.at(0.69, 0.2), 0.02, t.art_green);
}

fn bluetooth_speaker(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.12, 0.08, 0.58, 0.92), 0.07, shade(t.art_ink, 1.6));
    a.disc(a.at(0.35, 0.26), 0.07, t.art_steel);
    a.dot(a.at(0.35, 0.26), 0.025, t.art_ink);
    a.disc(a.at(0.35, 0.64), 0.16, t.art_steel);
    a.disc(a.at(0.35, 0.64), 0.1, shade(t.art_ink, 1.2));
    a.dot(a.at(0.35, 0.64), 0.04, t.art_steel);
    // The Bluetooth rune, in one stroke, on a blue disc.
    let c = a.at(0.77, 0.3);
    a.disc(c, 0.16, t.art_blue);
    let k = 0.11;
    let pts: Vec<Pos2> = [(-0.45f32, -0.3f32), (0.45, 0.3), (0.0, 0.7), (0.0, -0.7), (0.45, -0.3), (-0.45, 0.3)]
        .into_iter()
        .map(|(x, y)| a.at(0.77 + x * k, 0.3 + y * k))
        .collect();
    a.p.add(Shape::line(pts, Stroke::new((a.w * 0.025).max(1.0), Color32::WHITE)));
}

fn ceiling_fan(a: &Art) {
    let t = a.tk;
    // Seen from below: four broad wooden blades round a lamp hub.
    let c = (0.5f32, 0.5f32);
    for k in 0..4 {
        let ang = k as f32 * FRAC_PI_2 + 0.45;
        let (d, s) = ((ang.cos(), ang.sin()), (-ang.sin(), ang.cos()));
        let at = |along: f32, across: f32| a.at(c.0 + d.0 * along + s.0 * across, c.1 + d.1 * along + s.1 * across);
        a.poly(vec![at(0.1, -0.04), at(0.38, -0.08), at(0.4, 0.07), at(0.1, 0.05)], t.art_earth);
    }
    a.disc(a.at(c.0, c.1), 0.12, t.art_steel);
    a.disc(a.at(c.0, c.1), 0.06, t.art_amber);
}

fn furnace(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.14, 0.08, 0.86, 0.9), 0.05, t.art_steel);
    a.plain(a.rc(0.26, 0.4, 0.74, 0.82), 0.03, t.art_ink);
    flame(a, 0.5, 0.78, 0.34);
    for k in 0..3 {
        a.line(a.at(0.3 + k as f32 * 0.2, 0.16), a.at(0.3 + k as f32 * 0.2, 0.28), 0.03, shade(t.art_steel, 0.7));
    }
    a.dot(a.at(0.78, 0.18), 0.025, t.art_red);
    a.plain(a.rc(0.2, 0.9, 0.3, 0.95), 0.0, t.art_ink);
    a.plain(a.rc(0.7, 0.9, 0.8, 0.95), 0.0, t.art_ink);
}

fn humidifier(a: &Art) {
    let t = a.tk;
    for x in [0.34, 0.5, 0.66] {
        a.wave(x, 0.26, 0.04, 0.03, 0.025, t.art_blue);
    }
    a.boxed(a.rc(0.44, 0.26, 0.56, 0.4), 0.02, t.art_steel);
    a.boxed(a.rc(0.2, 0.38, 0.8, 0.9), 0.09, t.art_paper);
    a.plain(a.rc(0.28, 0.5, 0.72, 0.8), 0.05, t.art_glass);
    a.plain(a.rc(0.28, 0.64, 0.72, 0.8), 0.05, t.art_blue);
    a.dot(a.at(0.7, 0.45), 0.02, t.art_green);
}

fn humiture(a: &Art) {
    let t = a.tk;
    // A thermometer and a drop side by side.
    a.boxed(a.rc(0.12, 0.08, 0.4, 0.8), 0.12, t.art_paper);
    a.plain(a.rc(0.22, 0.16, 0.3, 0.62), 0.04, t.art_glass);
    a.plain(a.rc(0.24, 0.38, 0.28, 0.66), 0.0, t.art_red);
    a.disc(a.at(0.26, 0.72), 0.085, t.art_red);
    let c = a.at(0.67, 0.62);
    let mut pts = vec![a.at(0.67, 0.16)];
    pts.extend((0..=14).map(|i| {
        let ang = -0.4 + (PI + 0.8) * i as f32 / 14.0;
        c + vec2(ang.cos(), ang.sin()) * a.w * 0.19
    }));
    a.poly(pts, t.art_blue);
    a.arc(a.at(0.67, 0.64), 0.12, 0.5, 1.5, 0.02, Color32::from_white_alpha(170));
}

fn player(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.27, 0.06, 0.73, 0.94), 0.08, shade(t.art_ink, 1.7));
    a.plain(a.rc(0.33, 0.13, 0.67, 0.4), 0.02, t.art_glass);
    a.poly_plain(vec![a.at(0.45, 0.19), a.at(0.58, 0.265), a.at(0.45, 0.34)], t.art_ink);
    a.disc(a.at(0.5, 0.67), 0.16, t.art_steel);
    a.disc(a.at(0.5, 0.67), 0.06, t.art_ink);
    a.dot(a.at(0.5, 0.52), 0.015, t.art_steel);
}

fn sound_detector(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.1, 0.2, 0.9, 0.8), 0.06, t.art_paper);
    a.plain(a.rc(0.16, 0.27, 0.84, 0.73), 0.03, t.art_ink);
    for (k, h) in [0.2f32, 0.34, 0.42, 0.26, 0.36].into_iter().enumerate() {
        let x = 0.22 + k as f32 * 0.12;
        a.plain(a.rc(x, 0.68 - h, x + 0.07, 0.68), 0.0, if h > 0.4 { t.art_amber } else { t.art_green });
    }
    a.dot(a.at(0.5, 0.12), 0.04, t.art_steel);
    a.line(a.at(0.5, 0.16), a.at(0.5, 0.2), 0.03, t.art_steel);
}

fn pressure(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.5);
    a.disc(c, 0.42, t.art_steel);
    a.disc(c, 0.35, t.art_paper);
    for k in 0..=8 {
        let ang = PI * 0.8 + k as f32 * PI * 1.4 / 8.0;
        let d = vec2(ang.cos(), ang.sin());
        a.p.line_segment([c + d * a.w * 0.27, c + d * a.w * 0.32], Stroke::new(a.lw(), t.art_ink));
    }
    let needle = PI * 0.8 + PI * 1.4 * 0.62;
    a.p.line_segment([c, c + vec2(needle.cos(), needle.sin()) * a.w * 0.26], Stroke::new(a.lw() * 1.6, t.art_red));
    a.dot(c, 0.04, t.art_ink);
    a.text(a.at(0.5, 0.74), "kPa", 0.1, t.art_ink);
}

fn beacon(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.5);
    a.bar(a.at(0.5, 0.58), a.at(0.5, 0.88), 0.05, t.art_steel);
    a.boxed(a.rc(0.36, 0.86, 0.64, 0.93), 0.02, t.art_steel);
    for r in [0.2f32, 0.3, 0.4] {
        a.arc(c, r, -0.75, 0.75, 0.035, t.art_blue);
        a.arc(c, r, PI - 0.75, PI + 0.75, 0.035, t.art_blue);
    }
    a.disc(c, 0.1, t.art_blue);
    a.dot(c, 0.035, Color32::WHITE);
}

fn carbon_dioxide(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.1, 0.16, 0.9, 0.84), 0.12, t.art_paper);
    a.disc(a.at(0.5, 0.5), 0.26, t.art_steel);
    a.disc(a.at(0.5, 0.5), 0.2, t.art_paper);
    a.text(a.at(0.5, 0.5), "CO2", 0.15, t.art_ink);
    a.dot(a.at(0.8, 0.26), 0.03, t.art_green);
    for k in 0..3 {
        a.line(a.at(0.17, 0.68 + k as f32 * 0.05), a.at(0.27, 0.68 + k as f32 * 0.05), 0.014, t.art_steel);
    }
}

fn fire_monitor(a: &Art) {
    let t = a.tk;
    flame(a, 0.5, 0.66, 0.6);
    a.boxed(a.rc(0.2, 0.7, 0.8, 0.9), 0.06, t.art_steel);
    a.dot(a.at(0.34, 0.8), 0.03, t.art_red);
    for k in 0..3 {
        a.line(a.at(0.5 + k as f32 * 0.09, 0.76), a.at(0.5 + k as f32 * 0.09, 0.84), 0.014, t.art_ink);
    }
}

fn fire_sprinkler(a: &Art) {
    let t = a.tk;
    a.bar(a.at(0.5, 0.05), a.at(0.5, 0.36), 0.05, t.art_steel);
    a.boxed(a.rc(0.42, 0.34, 0.58, 0.5), 0.03, t.art_steel);
    a.disc(a.at(0.5, 0.57), 0.04, t.art_red);
    a.bar(a.at(0.28, 0.66), a.at(0.72, 0.66), 0.035, t.art_steel);
    for (k, dx) in [-0.22f32, -0.11, 0.0, 0.11, 0.22].into_iter().enumerate() {
        let end = a.at(0.5 + dx * 1.2, 0.88 + if k % 2 == 0 { 0.02 } else { 0.0 });
        a.line(a.at(0.5 + dx * 0.5, 0.69), end, 0.02, t.art_blue);
        a.dot(end, 0.02, t.art_blue);
    }
}

fn car(a: &Art) {
    let t = a.tk;
    a.blob(&[(0.07, 0.8, 0.03), (0.03, 0.72, 0.025)], shade(t.art_steel, 1.2));
    a.poly(vec![a.at(0.26, 0.5), a.at(0.34, 0.3), a.at(0.64, 0.3), a.at(0.78, 0.5)], t.art_red);
    a.poly_plain(vec![a.at(0.33, 0.47), a.at(0.38, 0.35), a.at(0.48, 0.35), a.at(0.48, 0.47)], t.art_glass);
    a.poly_plain(vec![a.at(0.52, 0.47), a.at(0.52, 0.35), a.at(0.62, 0.35), a.at(0.7, 0.47)], t.art_glass);
    a.boxed(a.rc(0.1, 0.48, 0.92, 0.74), 0.08, t.art_red);
    a.plain(a.rc(0.86, 0.54, 0.91, 0.6), 0.01, t.art_amber);
    for x in [0.28, 0.72] {
        a.disc(a.at(x, 0.75), 0.1, t.art_ink);
        a.dot(a.at(x, 0.75), 0.04, t.art_steel);
    }
}

fn rfid_card(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.1, 0.28, 0.9, 0.74), 0.07, t.art_paper);
    a.boxed(a.rc(0.18, 0.4, 0.36, 0.58), 0.02, t.art_amber);
    a.line(a.at(0.18, 0.49), a.at(0.36, 0.49), 0.012, t.art_ink);
    a.line(a.at(0.27, 0.4), a.at(0.27, 0.58), 0.012, t.art_ink);
    for r in [0.07f32, 0.12, 0.17] {
        a.arc(a.at(0.5, 0.51), r, -0.8, 0.8, 0.03, t.art_blue);
    }
    a.line(a.at(0.62, 0.64), a.at(0.82, 0.64), 0.014, t.art_steel);
}

fn rfid_reader(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.12, 0.62, 0.88, 0.9), 0.06, shade(t.art_ink, 1.7));
    a.plain(a.rc(0.2, 0.69, 0.5, 0.83), 0.02, t.art_steel);
    a.dot(a.at(0.72, 0.76), 0.035, t.art_green);
    for r in [0.12f32, 0.2, 0.28, 0.36] {
        a.arc(a.at(0.5, 0.6), r, PI * 1.2, PI * 1.8, 0.03, t.art_blue);
    }
}

fn smart_led(a: &Art) {
    let t = a.tk;
    // Red, green and blue discs that overlap into white: a colour LED.
    for (x, y, col) in [(0.38, 0.28, [0xe0u8, 0x30, 0x30]), (0.62, 0.28, [0x30, 0xb0, 0x50]), (0.5, 0.47, [0x30, 0x70, 0xe0])] {
        a.p.circle_filled(a.at(x, y), a.w * 0.2, Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 200));
    }
    a.p.circle_stroke(a.at(0.5, 0.37), a.w * 0.34, a.ink());
    a.boxed(a.rc(0.34, 0.68, 0.66, 0.78), 0.02, t.art_steel);
    for x in [0.42, 0.58] {
        a.line(a.at(x, 0.78), a.at(x, 0.92), 0.03, t.art_steel);
    }
    a.dot(a.at(0.82, 0.12), 0.02, t.art_amber);
}

fn trip_sensor(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.1, 0.16, 0.24, 0.9), 0.03, t.art_steel);
    a.boxed(a.rc(0.76, 0.16, 0.9, 0.9), 0.03, t.art_steel);
    a.disc(a.at(0.17, 0.28), 0.04, t.art_red);
    a.disc(a.at(0.83, 0.28), 0.04, t.art_red);
    for k in 0..5 {
        let x = 0.3 + k as f32 * 0.1;
        a.line(a.at(x, 0.28), a.at(x + 0.06, 0.28), 0.025, t.art_red);
    }
    // Something crossing the beam.
    a.disc(a.at(0.5, 0.52), 0.07, t.art_paper);
    a.poly(vec![a.at(0.42, 0.9), a.at(0.58, 0.9), a.at(0.55, 0.62), a.at(0.45, 0.62)], t.art_paper);
}

fn wind_detector(a: &Art) {
    let t = a.tk;
    let hub = (0.5f32, 0.38f32);
    a.bar(a.at(hub.0, hub.1), a.at(0.5, 0.88), 0.045, t.art_steel);
    a.boxed(a.rc(0.34, 0.86, 0.66, 0.93), 0.02, t.art_steel);
    for k in 0..3 {
        let ang = k as f32 * TAU / 3.0 - FRAC_PI_2 + 0.3;
        let tip = (hub.0 + ang.cos() * 0.28, hub.1 + ang.sin() * 0.28);
        a.line(a.at(hub.0, hub.1), a.at(tip.0, tip.1), 0.025, t.art_ink);
        a.disc(a.at(tip.0, tip.1), 0.075, t.art_paper);
    }
    a.disc(a.at(hub.0, hub.1), 0.045, t.art_ink);
}

fn signal_generator(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.08, 0.18, 0.92, 0.84), 0.06, t.art_steel);
    a.plain(a.rc(0.14, 0.25, 0.86, 0.58), 0.03, t.art_ink);
    let pts: Vec<Pos2> = (0..=28)
        .map(|i| {
            let x = i as f32 / 28.0;
            a.at(0.17 + x * 0.66, 0.415 + (x * TAU * 2.0).sin() * 0.1)
        })
        .collect();
    a.p.add(Shape::line(pts, Stroke::new((a.w * 0.025).max(1.0), t.art_green)));
    a.disc(a.at(0.24, 0.72), 0.06, t.art_paper);
    a.disc(a.at(0.42, 0.72), 0.06, t.art_paper);
    a.line(a.at(0.24, 0.72), a.at(0.26, 0.67), 0.012, t.art_ink);
    a.line(a.at(0.42, 0.72), a.at(0.45, 0.68), 0.012, t.art_ink);
    a.disc(a.at(0.78, 0.72), 0.07, t.art_amber);
    a.dot(a.at(0.78, 0.72), 0.025, t.art_ink);
}
