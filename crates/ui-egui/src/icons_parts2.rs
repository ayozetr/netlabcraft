//! Pictures of the second set of components (original artwork, drawn in code): the real electronic part
//! or device each one stands for, in the same style as `icons_things` (fractions of the icon's width, the
//! theme's `art_*` colours, one outline weight, legible from 28 px up).
//!
//! Actuators: an air cooler, an alarm bell, ceiling and floor sprinklers, a dimmable LED, a heating
//! element, an RGB LED, a servo and a speaker. Sensors: a flex sensor, a generic environment sensor (a
//! globe), a generic sensor module, humidity and humiture sensors, a membrane potentiometer, a metal
//! (inductive proximity) sensor, a motion sensor, three switches, a smoke sensor, a temperature sensor, a
//! trip wire, a water detector, a water level sensor and an anemometer.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Color32, Pos2, Shape, Stroke, pos2, vec2};

use crate::icons_things::{Art, ellipse, half_disc};
use crate::theme::shade;

/// Draws a model of this set; `false`: it is not one of them.
pub(crate) fn draw(a: &Art, model_id: &str) -> bool {
    match model_id {
        "Part-AirCooler" => air_cooler(a),
        "Part-Alarm" => alarm(a),
        "Part-CeilingSprinkler" => ceiling_sprinkler(a),
        "Part-FloorSprinkler" => floor_sprinkler(a),
        "Part-DimLED" => dimmable_led(a),
        "Part-HeatingElement" => heating_element(a),
        "Part-RGBLED" => rgb_led(a),
        "Part-Servo" => servo(a),
        "Part-Speaker" => speaker(a),
        "Part-Flex" => flex(a),
        "Part-EnvSensor" => environment(a),
        "Part-Generic" => generic(a),
        "Part-Humidity" => humidity(a),
        "Part-Humiture" => humiture(a),
        "Part-Membrane" => membrane(a),
        "Part-Metal" => metal(a),
        "Part-Motion" => motion(a),
        "Part-LatchSwitch" => latching_switch(a),
        "Part-Rocker" => rocker(a),
        "Part-PushToggle" => toggle_button(a),
        "Part-Smoke" => smoke(a),
        "Part-Temp" => temperature(a),
        "Part-Trip" => trip_wire(a),
        "Part-WaterDetector" => water_detector(a),
        "Part-WaterLevel" => water_level(a),
        "Part-Wind" => wind(a),
        _ => return false,
    }
    true
}

/// Component legs: steel bars from `y0` to `y1` at each `x`.
fn legs(a: &Art, xs: &[f32], y0: f32, y1: f32) {
    for x in xs {
        a.bar(a.at(*x, y0), a.at(*x, y1), 0.02, a.tk.art_steel);
    }
}

/// A drop: a disc with a point above it.
fn drop_shape(a: &Art, cx: f32, cy: f32, r: f32, fill: Color32) {
    let c = a.at(cx, cy);
    let rr = a.w * r;
    a.poly_plain(vec![c + vec2(-rr * 0.85, -rr * 0.45), c + vec2(0.0, -rr * 2.1), c + vec2(rr * 0.85, -rr * 0.45)], fill);
    a.p.circle_filled(c, rr, fill);
    a.p.circle_filled(c - vec2(rr * 0.35, rr * 0.3), rr * 0.25, Color32::from_white_alpha(150));
}

/// A line of dots on a curve (a spray of drops or a sound wave).
fn jet(a: &Art, from: Pos2, to: Pos2, ctl: Pos2, col: Color32) {
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let f = i as f32 / 12.0;
            let u = 1.0 - f;
            pos2(u * u * from.x + 2.0 * u * f * ctl.x + f * f * to.x, u * u * from.y + 2.0 * u * f * ctl.y + f * f * to.y)
        })
        .collect();
    a.p.add(Shape::line(pts, Stroke::new((a.w * 0.03).max(1.0), col)));
    a.dot(to, 0.03, col);
}

// ---------------------------------------------------------------------------------------------
// Actuators.

fn air_cooler(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.14, 0.1, 0.86, 0.66), 0.06, t.art_paper);
    let c = a.at(0.5, 0.38);
    a.disc(c, 0.21, shade(t.art_steel, 0.9));
    for k in 0..3 {
        let ang = k as f32 * TAU / 3.0 + 0.4;
        let d = vec2(ang.cos(), ang.sin());
        a.poly_plain(vec![c, c + d * a.w * 0.19, c + vec2((ang + 0.8).cos(), (ang + 0.8).sin()) * a.w * 0.14], t.art_blue);
    }
    a.dot(c, 0.03, t.art_ink);
    a.plain(a.rc(0.2, 0.62, 0.8, 0.66), 0.0, shade(t.art_steel, 0.8));
    for k in 0..3 {
        a.wave(0.28 + k as f32 * 0.22, 0.72, 0.94, 0.025, 0.028, t.art_blue);
    }
    a.plain(a.rc(0.18, 0.66, 0.26, 0.72), 0.0, t.art_ink);
    a.plain(a.rc(0.74, 0.66, 0.82, 0.72), 0.0, t.art_ink);
}

fn alarm(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.56);
    for s in [-1.0f32, 1.0] {
        let (a0, a1) = if s < 0.0 { (PI * 0.8, PI * 1.2) } else { (-PI * 0.2, PI * 0.2) };
        a.arc(c, 0.44, a0, a1, 0.03, t.art_amber);
        a.arc(c, 0.37, a0, a1, 0.024, t.art_amber);
    }
    let mut pts = half_disc(c + vec2(0.0, -a.w * 0.06), a.w * 0.24);
    pts.push(a.at(0.82, 0.7));
    pts.push(a.at(0.18, 0.7));
    a.poly(pts, t.art_red);
    a.arc(c + vec2(0.0, -a.w * 0.06), 0.16, PI + 0.4, PI + 1.3, 0.025, shade(t.art_red, 1.7));
    a.dot(a.at(0.5, 0.22), 0.04, t.art_ink);
    a.boxed(a.rc(0.12, 0.68, 0.88, 0.76), 0.03, shade(t.art_red, 0.8));
    a.disc(a.at(0.5, 0.84), 0.07, t.art_ink);
}

fn ceiling_sprinkler(a: &Art) {
    let t = a.tk;
    a.plain(a.rc(0.04, 0.04, 0.96, 0.12), 0.01, t.art_steel);
    a.bar(a.at(0.5, 0.12), a.at(0.5, 0.3), 0.06, t.art_steel);
    a.boxed(a.rc(0.4, 0.28, 0.6, 0.42), 0.03, t.art_red);
    a.boxed(a.rc(0.3, 0.42, 0.7, 0.48), 0.02, t.art_steel);
    for k in [-2.0f32, -1.0, 0.0, 1.0, 2.0] {
        let from = a.at(0.5 + k * 0.05, 0.5);
        let to = a.at(0.5 + k * 0.19, 0.86 - k.abs() * 0.04);
        a.line(from, to, 0.022, t.art_blue);
        a.dot(to, 0.03, t.art_blue);
    }
}

fn floor_sprinkler(a: &Art) {
    let t = a.tk;
    a.plain(a.rc(0.04, 0.82, 0.96, 0.92), 0.02, t.art_steel);
    a.p.add(Shape::convex_polygon(ellipse(a.at(0.5, 0.86), a.w * 0.34, a.w * 0.045), shade(t.art_blue, 1.3), Stroke::NONE));
    a.boxed(a.rc(0.38, 0.62, 0.62, 0.84), 0.03, t.art_steel);
    a.boxed(a.rc(0.43, 0.5, 0.57, 0.64), 0.02, t.art_ink);
    for k in [-1.0f32, 0.0, 1.0] {
        jet(a, a.at(0.5, 0.5), a.at(0.5 + k * 0.34, 0.72 - k.abs() * 0.1), a.at(0.5 + k * 0.2, 0.06 + k.abs() * 0.08), t.art_blue);
    }
}

fn dimmable_led(a: &Art) {
    let t = a.tk;
    let c = a.at(0.4, 0.34);
    for k in 0..5 {
        let ang = -PI * 0.95 + k as f32 * PI * 0.85 / 4.0;
        let d = vec2(ang.cos(), ang.sin());
        a.line(c + d * a.w * 0.24, c + d * a.w * 0.32, 0.025, t.art_amber);
    }
    legs(a, &[0.34, 0.46], 0.62, 0.92);
    let mut pts = vec![a.at(0.26, 0.6)];
    pts.extend((0..=14).map(|i| PI + PI * i as f32 / 14.0).map(|ang| c + vec2(ang.cos(), ang.sin()) * a.w * 0.14));
    pts.push(a.at(0.54, 0.6));
    a.poly(pts, t.art_amber);
    a.arc(c, 0.08, PI + 0.5, PI + 1.4, 0.025, shade(t.art_amber, 1.7));
    a.boxed(a.rc(0.22, 0.56, 0.58, 0.64), 0.01, shade(t.art_amber, 0.8));
    // The dimmer: a slider with its knob two thirds up.
    a.boxed(a.rc(0.72, 0.14, 0.82, 0.86), 0.03, shade(t.art_ink, 1.6));
    a.plain(a.rc(0.75, 0.2, 0.79, 0.8), 0.01, t.art_ink);
    a.boxed(a.rc(0.68, 0.34, 0.86, 0.46), 0.02, t.art_paper);
}

fn heating_element(a: &Art) {
    let t = a.tk;
    for k in 0..3 {
        a.wave(0.28 + k as f32 * 0.22, 0.4, 0.06, 0.03, 0.028, t.art_red);
    }
    a.boxed(a.rc(0.08, 0.72, 0.92, 0.88), 0.04, shade(t.art_earth, 1.4));
    for x in [0.16f32, 0.84] {
        a.bar(a.at(x, 0.5), a.at(x, 0.74), 0.045, t.art_steel);
    }
    let zig: Vec<Pos2> = (0..=10).map(|i| a.at(0.16 + i as f32 * 0.068, if i % 2 == 0 { 0.5 } else { 0.68 })).collect();
    a.p.add(Shape::line(zig.clone(), Stroke::new((a.w * 0.07).max(1.5), t.art_ink)));
    a.p.add(Shape::line(zig, Stroke::new((a.w * 0.045).max(1.0), t.art_red)));
}

fn rgb_led(a: &Art) {
    let t = a.tk;
    legs(a, &[0.34, 0.45, 0.56, 0.67], 0.7, 0.94);
    let c = a.at(0.5, 0.42);
    a.disc(c, 0.3, t.art_paper);
    let tint = |r: u8, g: u8, b: u8| Color32::from_rgba_unmultiplied(r, g, b, 190);
    let r = a.w * 0.17;
    a.p.circle_filled(c + vec2(-a.w * 0.09, -a.w * 0.07), r, tint(0xe0, 0x30, 0x30));
    a.p.circle_filled(c + vec2(a.w * 0.09, -a.w * 0.07), r, tint(0x30, 0xc0, 0x50));
    a.p.circle_filled(c + vec2(0.0, a.w * 0.09), r, tint(0x30, 0x60, 0xe0));
    a.p.circle_stroke(c, a.w * 0.3, a.ink());
    a.boxed(a.rc(0.2, 0.68, 0.8, 0.74), 0.01, t.art_steel);
}

fn servo(a: &Art) {
    let t = a.tk;
    for (i, col) in [t.art_amber, t.art_red, shade(t.art_ink, 2.0)].into_iter().enumerate() {
        a.line(a.at(0.02, 0.5 + i as f32 * 0.09), a.at(0.16, 0.5 + i as f32 * 0.09), 0.025, col);
    }
    a.boxed(a.rc(0.04, 0.8, 0.94, 0.88), 0.02, shade(t.art_blue, 0.7));
    a.boxed(a.rc(0.14, 0.34, 0.74, 0.8), 0.04, shade(t.art_blue, 0.85));
    a.boxed(a.rc(0.08, 0.7, 0.8, 0.8), 0.02, shade(t.art_blue, 0.7));
    let c = a.at(0.58, 0.3);
    a.boxed(a.rc(0.5, 0.26, 0.74, 0.4), 0.03, shade(t.art_blue, 0.85));
    a.arc(c, 0.3, -PI * 0.95, -PI * 0.05, 0.016, t.art_steel);
    a.bar(c, c + vec2(a.w * 0.22, -a.w * 0.2), 0.05, t.art_paper);
    a.disc(c, 0.07, t.art_steel);
    a.dot(c, 0.025, t.art_ink);
}

fn speaker(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.06, 0.34, 0.3, 0.66), 0.03, shade(t.art_ink, 1.6));
    a.poly(vec![a.at(0.3, 0.34), a.at(0.54, 0.12), a.at(0.54, 0.88), a.at(0.3, 0.66)], t.art_steel);
    a.dot(a.at(0.18, 0.5), 0.05, t.art_steel);
    let c = a.at(0.54, 0.5);
    for (k, r) in [0.16f32, 0.26, 0.36].into_iter().enumerate() {
        a.arc(c, r, -0.7, 0.7, 0.03, if k == 2 { shade(t.art_blue, 1.2) } else { t.art_blue });
    }
}

// ---------------------------------------------------------------------------------------------
// Sensors.

fn flex(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 1.0);
    let pts: Vec<Pos2> = (0..=16).map(|i| -FRAC_PI_2 - 0.9 + 1.8 * i as f32 / 16.0).map(|ang| c + vec2(ang.cos(), ang.sin()) * a.w * 0.5).collect();
    a.p.add(Shape::line(pts.clone(), Stroke::new((a.w * 0.15).max(2.0), t.art_edge)));
    a.p.add(Shape::line(pts.clone(), Stroke::new((a.w * 0.11).max(1.5), t.art_ink)));
    a.p.add(Shape::line(pts, Stroke::new((a.w * 0.03).max(1.0), t.art_amber)));
    legs(a, &[0.07, 0.15], 0.72, 0.93);
    // A bend arrow above the strip.
    a.arc(a.at(0.5, 0.56), 0.32, -PI * 0.88, -PI * 0.12, 0.025, t.art_red);
    a.poly_plain(vec![a.at(0.84, 0.5), a.at(0.74, 0.44), a.at(0.77, 0.56)], t.art_red);
}

fn environment(a: &Art) {
    let t = a.tk;
    a.bar(a.at(0.5, 0.74), a.at(0.5, 0.88), 0.05, t.art_steel);
    a.boxed(a.rc(0.28, 0.86, 0.72, 0.92), 0.02, t.art_steel);
    let c = a.at(0.5, 0.44);
    a.disc(c, 0.34, t.art_blue);
    let g = shade(t.art_green, 1.1);
    a.poly_plain(vec![a.at(0.3, 0.3), a.at(0.46, 0.22), a.at(0.5, 0.36), a.at(0.38, 0.52), a.at(0.28, 0.44)], g);
    a.poly_plain(vec![a.at(0.58, 0.46), a.at(0.74, 0.4), a.at(0.7, 0.62), a.at(0.58, 0.66)], g);
    a.p.add(Shape::closed_line(ellipse(c, a.w * 0.15, a.w * 0.34), Stroke::new(a.lw() * 0.7, t.art_paper)));
    a.p.add(Shape::closed_line(ellipse(c, a.w * 0.34, a.w * 0.12), Stroke::new(a.lw() * 0.7, t.art_paper)));
    a.p.circle_stroke(c, a.w * 0.34, a.ink());
}

fn generic(a: &Art) {
    let t = a.tk;
    legs(a, &[0.3, 0.5, 0.7], 0.72, 0.93);
    a.boxed(a.rc(0.1, 0.18, 0.9, 0.76), 0.05, t.art_pcb_blue);
    a.boxed(a.rc(0.2, 0.26, 0.8, 0.68), 0.03, shade(t.art_ink, 1.2));
    let pts: Vec<Pos2> = (0..=20).map(|i| a.at(0.24 + i as f32 * 0.026, 0.47 - (i as f32 * 0.5 * PI).sin() * 0.1)).collect();
    a.p.add(Shape::line(pts, Stroke::new((a.w * 0.03).max(1.0), t.art_green)));
    a.dot(a.at(0.15, 0.23), 0.02, t.art_amber);
    a.dot(a.at(0.85, 0.23), 0.02, t.art_amber);
}

fn humidity(a: &Art) {
    let t = a.tk;
    legs(a, &[0.3, 0.43, 0.57, 0.7], 0.72, 0.93);
    a.boxed(a.rc(0.14, 0.1, 0.86, 0.74), 0.06, shade(t.art_pcb_blue, 1.3));
    for row in 0..3 {
        for col in 0..5 {
            a.dot(a.at(0.26 + col as f32 * 0.12, 0.2 + row as f32 * 0.09), 0.016, t.art_ink);
        }
    }
    drop_shape(a, 0.5, 0.58, 0.1, t.art_paper);
}

fn humiture(a: &Art) {
    let t = a.tk;
    legs(a, &[0.3, 0.43, 0.57, 0.7], 0.72, 0.93);
    a.boxed(a.rc(0.12, 0.1, 0.88, 0.74), 0.06, shade(t.art_pcb_blue, 1.3));
    for col in 0..6 {
        a.dot(a.at(0.22 + col as f32 * 0.11, 0.18), 0.016, t.art_ink);
    }
    drop_shape(a, 0.34, 0.56, 0.09, t.art_paper);
    a.plain(a.rc(0.6, 0.28, 0.68, 0.56), 0.04, t.art_paper);
    a.disc(a.at(0.64, 0.6), 0.07, t.art_red);
    a.plain(a.rc(0.62, 0.42, 0.66, 0.56), 0.0, t.art_red);
}

fn membrane(a: &Art) {
    let t = a.tk;
    a.plain(a.rc(0.76, 0.42, 0.96, 0.58), 0.0, shade(t.art_paper, 0.9));
    for k in 0..3 {
        a.plain(a.rc(0.8 + k as f32 * 0.05, 0.42, 0.82 + k as f32 * 0.05, 0.58), 0.0, t.art_amber);
    }
    a.boxed(a.rc(0.04, 0.34, 0.78, 0.66), 0.03, t.art_paper);
    a.plain(a.rc(0.08, 0.4, 0.74, 0.6), 0.02, shade(t.art_steel, 1.15));
    for k in 0..9 {
        a.line(a.at(0.12 + k as f32 * 0.07, 0.46), a.at(0.12 + k as f32 * 0.07, 0.54), 0.012, t.art_ink);
    }
    // A fingertip pressing at 40 %.
    let f = a.at(0.4, 0.5);
    a.disc(f, 0.07, t.art_red);
    a.arc(f, 0.13, 0.0, TAU, 0.02, t.art_red);
    a.line(a.at(0.4, 0.06), a.at(0.4, 0.28), 0.05, shade(t.art_earth, 1.6));
}

fn metal(a: &Art) {
    let t = a.tk;
    a.bar(a.at(0.0, 0.5), a.at(0.1, 0.5), 0.04, t.art_amber);
    a.boxed(a.rc(0.1, 0.32, 0.62, 0.68), 0.04, t.art_steel);
    for k in 0..6 {
        a.line(a.at(0.2 + k as f32 * 0.07, 0.34), a.at(0.2 + k as f32 * 0.07, 0.66), 0.014, shade(t.art_steel, 0.6));
    }
    a.boxed(a.rc(0.62, 0.36, 0.7, 0.64), 0.02, t.art_blue);
    a.dot(a.at(0.16, 0.4), 0.025, t.art_green);
    let c = a.at(0.7, 0.5);
    for r in [0.1f32, 0.17] {
        a.arc(c, r, -0.9, 0.9, 0.018, shade(t.art_blue, 1.3));
    }
    // A metal bolt head close to its face.
    let h = a.at(0.88, 0.5);
    let hex: Vec<Pos2> = (0..6).map(|i| i as f32 * TAU / 6.0).map(|ang| h + vec2(ang.cos(), ang.sin()) * a.w * 0.11).collect();
    a.poly(hex, shade(t.art_steel, 0.8));
    a.disc(h, 0.04, shade(t.art_steel, 1.3));
    legs(a, &[0.3, 0.45], 0.7, 0.9);
}

fn motion(a: &Art) {
    let t = a.tk;
    legs(a, &[0.34, 0.5, 0.66], 0.78, 0.94);
    a.boxed(a.rc(0.08, 0.34, 0.92, 0.8), 0.04, t.art_pcb_green);
    let c = a.at(0.5, 0.52);
    a.p.circle_filled(c, a.w * 0.26, t.art_paper);
    for k in 0..6 {
        let ang = k as f32 * TAU / 6.0;
        a.line(c, c + vec2(ang.cos(), ang.sin()) * a.w * 0.26, 0.012, shade(t.art_steel, 0.9));
    }
    a.p.circle_stroke(c, a.w * 0.26, a.ink());
    a.p.circle_stroke(c, a.w * 0.12, Stroke::new(a.lw() * 0.7, shade(t.art_steel, 0.9)));
    for r in [0.14f32, 0.22] {
        a.arc(a.at(0.5, 0.3), r, -FRAC_PI_2 - 0.8, -FRAC_PI_2 + 0.8, 0.026, t.art_amber);
    }
}

fn latching_switch(a: &Art) {
    let t = a.tk;
    legs(a, &[0.38, 0.5, 0.62], 0.78, 0.94);
    let c = a.at(0.5, 0.46);
    a.disc(c, 0.36, t.art_steel);
    a.p.circle_stroke(c, a.w * 0.29, Stroke::new(a.lw() * 0.8, shade(t.art_steel, 0.6)));
    a.disc(c, 0.24, shade(t.art_ink, 1.5));
    a.arc(c, 0.11, -FRAC_PI_2 + 0.6, -FRAC_PI_2 + TAU - 0.6, 0.035, t.art_green);
    a.line(c + vec2(0.0, -a.w * 0.15), c, 0.035, t.art_green);
    a.dot(a.at(0.5, 0.1), 0.03, t.art_green);
}

fn rocker(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.12, 0.08, 0.88, 0.92), 0.07, t.art_paper);
    a.boxed(a.rc(0.2, 0.16, 0.8, 0.84), 0.04, shade(t.art_ink, 1.5));
    a.poly(vec![a.at(0.26, 0.2), a.at(0.74, 0.2), a.at(0.74, 0.54), a.at(0.26, 0.5)], t.art_red);
    a.poly(vec![a.at(0.26, 0.5), a.at(0.74, 0.54), a.at(0.74, 0.8), a.at(0.26, 0.8)], shade(t.art_red, 0.6));
    a.line(a.at(0.5, 0.28), a.at(0.5, 0.4), 0.05, t.art_paper);
    a.disc(a.at(0.5, 0.68), 0.07, shade(t.art_red, 0.6));
    a.p.circle_stroke(a.at(0.5, 0.68), a.w * 0.07, Stroke::new(a.lw() * 0.9, t.art_paper));
}

fn toggle_button(a: &Art) {
    let t = a.tk;
    for (x, y) in [(0.22f32, 0.88f32), (0.78, 0.88)] {
        a.bar(a.at(x, y - 0.1), a.at(x, y + 0.04), 0.02, t.art_steel);
    }
    a.boxed(a.rc(0.2, 0.4, 0.8, 0.84), 0.05, shade(t.art_ink, 1.7));
    a.disc(a.at(0.5, 0.58), 0.17, t.art_red);
    // Two arrows chasing each other over the button: a state that flips with each press.
    let c = a.at(0.5, 0.3);
    a.arc(c, 0.17, PI + 0.3, TAU - 0.5, 0.035, t.art_amber);
    a.arc(c, 0.17, 0.3, PI - 0.5, 0.035, t.art_amber);
    a.poly_plain(vec![a.at(0.68, 0.2), a.at(0.74, 0.34), a.at(0.62, 0.32)], t.art_amber);
    a.poly_plain(vec![a.at(0.32, 0.4), a.at(0.26, 0.26), a.at(0.38, 0.28)], t.art_amber);
}

fn smoke(a: &Art) {
    let t = a.tk;
    a.blob(&[(0.3, 0.2, 0.09), (0.44, 0.13, 0.1), (0.6, 0.18, 0.09), (0.74, 0.12, 0.07)], shade(t.art_steel, 1.3));
    legs(a, &[0.34, 0.5, 0.66], 0.78, 0.94);
    a.boxed(a.rc(0.12, 0.62, 0.88, 0.8), 0.04, t.art_pcb_blue);
    a.boxed(a.rc(0.26, 0.3, 0.74, 0.68), 0.12, shade(t.art_steel, 0.85));
    for k in 0..5 {
        a.line(a.at(0.3 + k as f32 * 0.1, 0.34), a.at(0.3 + k as f32 * 0.1, 0.64), 0.012, shade(t.art_steel, 0.55));
    }
    for k in 0..4 {
        a.line(a.at(0.28, 0.38 + k as f32 * 0.08), a.at(0.72, 0.38 + k as f32 * 0.08), 0.012, shade(t.art_steel, 0.55));
    }
}

fn temperature(a: &Art) {
    let t = a.tk;
    legs(a, &[0.22, 0.36, 0.5], 0.62, 0.93);
    let c = a.at(0.36, 0.38);
    let mut pts = half_disc(c, a.w * 0.2);
    pts.push(a.at(0.56, 0.62));
    pts.push(a.at(0.16, 0.62));
    a.poly(pts, shade(t.art_ink, 1.5));
    a.arc(c, 0.13, PI + 0.4, PI + 1.3, 0.02, shade(t.art_ink, 3.0));
    // The thermometer beside it.
    a.boxed(a.rc(0.68, 0.1, 0.8, 0.62), 0.05, t.art_paper);
    a.plain(a.rc(0.72, 0.34, 0.76, 0.66), 0.0, t.art_red);
    a.disc(a.at(0.74, 0.72), 0.1, t.art_red);
    for k in 0..3 {
        a.line(a.at(0.84, 0.18 + k as f32 * 0.1), a.at(0.9, 0.18 + k as f32 * 0.1), 0.014, t.art_ink);
    }
}

fn trip_wire(a: &Art) {
    let t = a.tk;
    for x in [0.06f32, 0.82] {
        a.boxed(a.rc(x, 0.34, x + 0.12, 0.9), 0.02, shade(t.art_ink, 1.6));
        a.dot(a.at(x + 0.06, 0.42), 0.025, t.art_red);
    }
    let wire = vec![a.at(0.18, 0.5), a.at(0.5, 0.56), a.at(0.82, 0.5)];
    a.p.add(Shape::line(wire, Stroke::new((a.w * 0.025).max(1.0), t.art_red)));
    a.poly(vec![a.at(0.42, 0.55), a.at(0.58, 0.55), a.at(0.5, 0.68)], t.art_amber);
    // Something crossing the line.
    a.bar(a.at(0.5, 0.06), a.at(0.5, 0.3), 0.06, t.art_paper);
    a.poly(vec![a.at(0.38, 0.28), a.at(0.62, 0.28), a.at(0.5, 0.42)], t.art_paper);
}

fn water_detector(a: &Art) {
    let t = a.tk;
    legs(a, &[0.3, 0.5, 0.7], 0.72, 0.93);
    a.boxed(a.rc(0.1, 0.1, 0.9, 0.74), 0.04, t.art_pcb_blue);
    for k in 0..4 {
        let y = 0.18 + k as f32 * 0.12;
        a.line(a.at(0.16, y), a.at(0.84, y), 0.022, t.art_amber);
    }
    drop_shape(a, 0.34, 0.5, 0.075, t.art_glass);
    drop_shape(a, 0.64, 0.4, 0.06, t.art_glass);
}

fn water_level(a: &Art) {
    let t = a.tk;
    legs(a, &[0.38, 0.5, 0.62], 0.84, 0.95);
    a.boxed(a.rc(0.28, 0.04, 0.72, 0.86), 0.04, t.art_pcb_blue);
    for k in 0..6 {
        a.line(a.at(0.34, 0.12 + k as f32 * 0.11), a.at(0.66, 0.12 + k as f32 * 0.11), 0.022, t.art_amber);
    }
    // The water, up to a wavy surface.
    a.plain(a.rc(0.28, 0.54, 0.72, 0.86), 0.0, Color32::from_rgba_unmultiplied(0x3a, 0x8f, 0xd6, 190));
    let surface: Vec<Pos2> = (0..=12).map(|i| a.at(0.28 + i as f32 * 0.0367, 0.54 + (i as f32 * 1.4).sin() * 0.018)).collect();
    a.p.add(Shape::line(surface, Stroke::new((a.w * 0.03).max(1.0), t.art_paper)));
    a.p.rect_stroke(a.rc(0.28, 0.04, 0.72, 0.86), a.w * 0.04, a.ink(), egui::StrokeKind::Inside);
    a.line(a.at(0.84, 0.2), a.at(0.84, 0.82), 0.014, t.art_ink);
    a.poly_plain(vec![a.at(0.8, 0.28), a.at(0.88, 0.28), a.at(0.84, 0.2)], t.art_ink);
    a.poly_plain(vec![a.at(0.8, 0.74), a.at(0.88, 0.74), a.at(0.84, 0.82)], t.art_ink);
}

fn wind(a: &Art) {
    let t = a.tk;
    a.bar(a.at(0.5, 0.5), a.at(0.5, 0.88), 0.05, t.art_steel);
    a.boxed(a.rc(0.3, 0.86, 0.7, 0.93), 0.02, t.art_steel);
    let hub = a.at(0.5, 0.44);
    for k in 0..3 {
        let ang = k as f32 * TAU / 3.0 - FRAC_PI_2 + 0.5;
        let d = vec2(ang.cos(), ang.sin());
        let tip = hub + d * a.w * 0.3;
        a.bar(hub, tip, 0.03, t.art_steel);
        a.disc(tip + d * a.w * 0.04, 0.1, t.art_paper);
    }
    a.disc(hub, 0.06, t.art_blue);
}
