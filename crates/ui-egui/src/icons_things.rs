//! Pictures of the smart things, the components and the programmable boards (original artwork, drawn in
//! code). Each family has its own silhouette and accent colour instead of one tile with a glyph on it:
//! a thermostat is a round dial, a smart plug a plug body with its prongs, a door lock a door with a
//! handle, a TV a screen on a stand, a traffic light a three-lamp housing, a battery a cell with
//! segments, a solar panel a tilted grid, a turbine a tower with blades; an LED has a dome and legs, a
//! relay is a block with pins, a board is a circuit board with chips and pin headers.
//!
//! Everything is laid out in fractions of the icon's width and painted with the theme's `art_*` tokens,
//! with one outline weight and one grounding shadow so the models read as a set from 28 px up. The small
//! Wi-Fi marker in the corner says the thing is wireless.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

use crate::theme::{Tokens, shade};

/// Whether [`thing_icon`] draws this model: every smart thing and component and the programmable boards.
pub fn is_thing(model_id: &str) -> bool {
    model_id.starts_with("IoT-") || model_id.starts_with("Part-") || matches!(model_id, "PLC" | "Board-SBC" | "Board-MCU")
}

/// The painter, the icon's box and the palette, with the drawing helpers every picture shares.
pub(crate) struct Art<'a> {
    pub(crate) p: &'a Painter,
    pub(crate) r: Rect,
    pub(crate) w: f32,
    pub(crate) tk: &'a Tokens,
}

impl<'a> Art<'a> {
    pub(crate) fn new(p: &'a Painter, r: Rect, tk: &'a Tokens) -> Self {
        Art { p, r, w: r.width(), tk }
    }

    /// The outline weight.
    pub(crate) fn lw(&self) -> f32 {
        (self.w / 30.0).max(1.0)
    }

    pub(crate) fn ink(&self) -> Stroke {
        Stroke::new(self.lw(), self.tk.art_edge)
    }

    /// A point of the icon, `x` and `y` as fractions of its width.
    pub(crate) fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + self.w * x, self.r.top() + self.w * y)
    }

    pub(crate) fn rc(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.at(x0, y0), self.at(x1, y1))
    }

    /// A shaded, outlined rounded box: a lit top edge, a shaded foot.
    pub(crate) fn boxed(&self, r: Rect, round: f32, fill: Color32) {
        let rr = self.w * round;
        self.p.rect_filled(r, rr, fill);
        let inset = rr.max(self.lw());
        if r.width() > inset * 2.5 && r.height() > self.lw() * 5.0 {
            let y = r.top() + self.lw() * 1.4;
            self.p.line_segment([pos2(r.left() + inset, y), pos2(r.right() - inset, y)], Stroke::new(self.lw() * 0.8, shade(fill, 1.35)));
            let y = r.bottom() - self.lw() * 1.4;
            self.p.line_segment([pos2(r.left() + inset, y), pos2(r.right() - inset, y)], Stroke::new(self.lw() * 0.8, shade(fill, 0.7)));
        }
        self.p.rect_stroke(r, rr, self.ink(), StrokeKind::Inside);
    }

    /// A flat fill with no outline (a screen, a label, a lamp).
    pub(crate) fn plain(&self, r: Rect, round: f32, fill: Color32) {
        self.p.rect_filled(r, self.w * round, fill);
    }

    /// A convex outlined polygon.
    pub(crate) fn poly(&self, pts: Vec<Pos2>, fill: Color32) {
        self.p.add(Shape::convex_polygon(pts, fill, self.ink()));
    }

    /// A convex polygon with no outline.
    pub(crate) fn poly_plain(&self, pts: Vec<Pos2>, fill: Color32) {
        self.p.add(Shape::convex_polygon(pts, fill, Stroke::NONE));
    }

    /// An outlined disc of radius `r` (a fraction of the width), with a small highlight.
    pub(crate) fn disc(&self, c: Pos2, r: f32, fill: Color32) {
        let rr = self.w * r;
        self.p.circle_filled(c, rr, fill);
        if r > 0.07 {
            self.p.circle_filled(c - vec2(rr * 0.35, rr * 0.35), rr * 0.2, shade(fill, 1.45));
        }
        self.p.circle_stroke(c, rr, self.ink());
    }

    /// A flat dot with no outline.
    pub(crate) fn dot(&self, c: Pos2, r: f32, fill: Color32) {
        self.p.circle_filled(c, (self.w * r).max(0.8), fill);
    }

    /// A line `wid` thick (a fraction of the width).
    pub(crate) fn line(&self, from: Pos2, to: Pos2, wid: f32, col: Color32) {
        self.p.line_segment([from, to], Stroke::new((self.w * wid).max(1.0), col));
    }

    /// A rod: a dark line with a lighter one along it.
    pub(crate) fn bar(&self, from: Pos2, to: Pos2, wid: f32, col: Color32) {
        self.line(from, to, wid + 0.04, self.tk.art_edge);
        self.line(from, to, wid, col);
    }

    /// An arc of the circle of radius `r` (a fraction of the width) around `c`, from angle `a0` to `a1`.
    pub(crate) fn arc(&self, c: Pos2, r: f32, a0: f32, a1: f32, wid: f32, col: Color32) {
        let pts: Vec<Pos2> = (0..=14)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / 14.0;
                c + vec2(a.cos(), a.sin()) * self.w * r
            })
            .collect();
        self.p.add(Shape::line(pts, Stroke::new((self.w * wid).max(1.0), col)));
    }

    /// A wavy vertical line from `y0` to `y1` at `x`.
    pub(crate) fn wave(&self, x: f32, y0: f32, y1: f32, amp: f32, wid: f32, col: Color32) {
        let pts: Vec<Pos2> = (0..=12)
            .map(|i| {
                let t = i as f32 / 12.0;
                self.at(x + (t * 3.0 * PI).sin() * amp, y0 + (y1 - y0) * t)
            })
            .collect();
        self.p.add(Shape::line(pts, Stroke::new((self.w * wid).max(1.0), col)));
    }

    pub(crate) fn text(&self, c: Pos2, s: &str, size: f32, col: Color32) {
        self.p.text(c, Align2::CENTER_CENTER, s, FontId::proportional(self.w * size), col);
    }

    /// Overlapping discs drawn as one outlined blob (a cloud, a puff of smoke).
    pub(crate) fn blob(&self, discs: &[(f32, f32, f32)], fill: Color32) {
        for (x, y, r) in discs {
            self.p.circle_filled(self.at(*x, *y), self.w * r + self.lw(), self.tk.art_edge);
        }
        for (x, y, r) in discs {
            self.p.circle_filled(self.at(*x, *y), self.w * r, fill);
        }
    }

    /// The soft shadow every picture stands on.
    pub(crate) fn ground(&self) {
        self.p.add(Shape::convex_polygon(ellipse(self.at(0.5, 0.93), self.w * 0.34, self.w * 0.045), Color32::from_black_alpha(34), Stroke::NONE));
    }

    /// The Wi-Fi marker in the top right corner.
    pub(crate) fn wifi(&self) {
        let c = self.at(0.87, 0.17);
        let halo = Color32::from_white_alpha(150);
        for (r, a) in [(0.06f32, 0.8f32), (0.105, 0.7)] {
            self.arc(c, r, -FRAC_PI_2 - a, -FRAC_PI_2 + a, 0.05, halo);
        }
        self.p.circle_filled(c, self.w * 0.04, halo);
        for (r, a) in [(0.06f32, 0.8f32), (0.105, 0.7)] {
            self.arc(c, r, -FRAC_PI_2 - a, -FRAC_PI_2 + a, 0.028, self.tk.art_green);
        }
        self.dot(c, 0.028, self.tk.art_green);
    }
}

pub(crate) fn ellipse(c: Pos2, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..24).map(|i| i as f32 * TAU / 24.0).map(|a| pos2(c.x + rx * a.cos(), c.y + ry * a.sin())).collect()
}

/// The upper half of a disc, as a convex polygon whose flat side is at the bottom.
pub(crate) fn half_disc(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..=16).map(|i| PI + PI * i as f32 / 16.0).map(|a| c + vec2(a.cos(), a.sin()) * r).collect()
}

/// Draws the picture of a smart thing, component or board filling `rect`. `wireless` adds the Wi-Fi marker.
pub fn thing_icon(p: &Painter, rect: Rect, model_id: &str, wireless: bool, tk: &Tokens) {
    let a = Art::new(p, rect, tk);
    a.ground();
    match model_id {
        "IoT-Sensor" => sensor(&a),
        "IoT-Motion" => motion(&a),
        "IoT-Smoke" => smoke(&a),
        "IoT-Light" => light(&a),
        "IoT-Door" => door(&a),
        "IoT-Siren" => siren(&a),
        "IoT-Webcam" => webcam(&a),
        "IoT-Thermostat" => thermostat(&a),
        "IoT-Heater" => heater(&a),
        "IoT-AC" => air_conditioner(&a),
        "IoT-Humidity" => humidity(&a),
        "IoT-Window" => window(&a),
        "IoT-Soil" => soil(&a),
        "IoT-Sprinkler" => sprinkler(&a),
        "IoT-CO" => carbon_monoxide(&a),
        "IoT-Plug" => plug(&a),
        "IoT-Garage" => garage(&a),
        "IoT-TV" => television(&a, false),
        "IoT-TVE" => television(&a, true),
        "IoT-Speaker" => speaker(&a),
        "IoT-Water" => water(&a),
        "IoT-Device" => fan(&a),
        "IoT-Streetlight" => streetlight(&a),
        "IoT-Traffic" => traffic(&a),
        "IoT-Parking" => parking(&a),
        "IoT-Drain" => drain(&a),
        "IoT-Air" => air(&a),
        "IoT-Blower" => blower(&a),
        "IoT-Battery" => battery(&a),
        "IoT-Meter" => meter(&a),
        "IoT-Solar" => solar(&a),
        "IoT-Wind" => turbine(&a),
        "Part-LED" => led(&a),
        "Part-Buzzer" => buzzer(&a),
        "Part-Motor" => motor(&a),
        "Part-Relay" => relay(&a),
        "Part-LCD" => lcd(&a),
        "Part-Button" => button(&a),
        "Part-Pot" => potentiometer(&a),
        "Part-Ultrasonic" => ultrasonic(&a),
        "Part-Light" => photoresistor(&a),
        "Part-Switch" => toggle(&a),
        "Board-SBC" => board_sbc(&a),
        "Board-MCU" => board_mcu(&a),
        "PLC" => plc(&a),
        _ => cube(&a),
    }
    if wireless {
        a.wifi();
    }
}

// ---------------------------------------------------------------------------------------------
// Smart things.

fn sensor(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.28, 0.08, 0.72, 0.92), 0.07, t.art_steel);
    a.boxed(a.rc(0.42, 0.16, 0.58, 0.64), 0.06, t.art_paper);
    a.plain(a.rc(0.46, 0.38, 0.54, 0.66), 0.0, t.art_red);
    a.disc(a.at(0.5, 0.73), 0.12, t.art_red);
    for k in 0..4 {
        let y = 0.2 + k as f32 * 0.09;
        a.line(a.at(0.62, y), a.at(0.68, y), 0.014, t.art_ink);
    }
}

fn motion(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.66);
    a.arc(c, 0.4, PI + 0.3, TAU - 0.3, 0.025, t.art_amber);
    a.arc(c, 0.34, PI + 0.5, TAU - 0.5, 0.02, t.art_amber);
    a.poly(half_disc(c, a.w * 0.28), t.art_paper);
    for k in 1..4 {
        let ang = PI + k as f32 * PI / 4.0;
        a.line(c, c + vec2(ang.cos(), ang.sin()) * a.w * 0.28, 0.012, t.art_steel);
    }
    a.boxed(a.rc(0.18, 0.66, 0.82, 0.84), 0.05, t.art_steel);
    a.dot(a.at(0.7, 0.75), 0.025, t.art_red);
}

fn smoke(a: &Art) {
    let t = a.tk;
    a.blob(&[(0.3, 0.26, 0.09), (0.44, 0.17, 0.11), (0.6, 0.22, 0.09), (0.72, 0.3, 0.07)], shade(t.art_steel, 1.2));
    a.poly(ellipse(a.at(0.5, 0.72), a.w * 0.42, a.w * 0.17), t.art_paper);
    a.poly(ellipse(a.at(0.5, 0.68), a.w * 0.32, a.w * 0.12), shade(t.art_paper, 0.9));
    for k in 0..4 {
        let x = 0.34 + k as f32 * 0.1;
        a.line(a.at(x, 0.65), a.at(x + 0.02, 0.72), 0.014, t.art_steel);
    }
    a.dot(a.at(0.5, 0.68), 0.03, t.art_red);
}

fn light(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.38);
    for k in 0..7 {
        let ang = -PI * 0.95 + k as f32 * PI * 0.9 / 6.0;
        let d = vec2(ang.cos(), ang.sin());
        a.line(c + d * a.w * 0.3, c + d * a.w * 0.4, 0.03, t.art_amber);
    }
    a.poly(vec![a.at(0.4, 0.56), a.at(0.6, 0.56), a.at(0.58, 0.72), a.at(0.42, 0.72)], t.art_glass);
    a.disc(c, 0.23, t.art_amber);
    a.line(a.at(0.44, 0.44), a.at(0.5, 0.36), 0.014, t.art_ink);
    a.line(a.at(0.5, 0.36), a.at(0.56, 0.44), 0.014, t.art_ink);
    a.boxed(a.rc(0.41, 0.72, 0.59, 0.86), 0.03, t.art_steel);
    a.line(a.at(0.42, 0.79), a.at(0.58, 0.79), 0.012, t.art_ink);
}

fn door(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.22, 0.06, 0.78, 0.92), 0.03, t.art_earth);
    a.plain(a.rc(0.29, 0.13, 0.71, 0.4), 0.02, shade(t.art_earth, 0.75));
    a.plain(a.rc(0.29, 0.5, 0.71, 0.84), 0.02, shade(t.art_earth, 0.75));
    a.boxed(a.rc(0.55, 0.38, 0.72, 0.6), 0.03, t.art_steel);
    a.dot(a.at(0.635, 0.43), 0.022, t.art_green);
    a.bar(a.at(0.63, 0.54), a.at(0.4, 0.54), 0.035, t.art_paper);
    a.disc(a.at(0.63, 0.54), 0.045, t.art_steel);
}

fn siren(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.66);
    for s in [-1.0f32, 1.0] {
        let (a0, a1) = if s < 0.0 { (PI * 0.82, PI * 1.18) } else { (-PI * 0.18, PI * 0.18) };
        a.arc(c, 0.42, a0, a1, 0.026, t.art_red);
        a.arc(c, 0.36, a0, a1, 0.02, t.art_red);
    }
    a.line(c + vec2(0.0, -a.w * 0.38), c + vec2(0.0, -a.w * 0.46), 0.03, t.art_amber);
    a.poly(half_disc(c, a.w * 0.28), t.art_red);
    a.arc(c, 0.2, PI + 0.4, PI + 1.3, 0.025, shade(t.art_red, 1.6));
    a.boxed(a.rc(0.16, 0.66, 0.84, 0.82), 0.04, t.art_steel);
}

fn webcam(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.44, 0.5, 0.56, 0.76), 0.01, t.art_steel);
    a.boxed(a.rc(0.26, 0.74, 0.74, 0.86), 0.05, t.art_steel);
    a.boxed(a.rc(0.16, 0.12, 0.84, 0.56), 0.1, shade(t.art_steel, 0.8));
    a.disc(a.at(0.5, 0.34), 0.16, t.art_ink);
    a.disc(a.at(0.5, 0.34), 0.1, t.art_blue);
    a.dot(a.at(0.46, 0.3), 0.025, t.art_paper);
    a.dot(a.at(0.76, 0.2), 0.022, t.art_red);
}

fn thermostat(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.5);
    a.disc(c, 0.42, t.art_steel);
    a.disc(c, 0.33, t.art_paper);
    for k in 0..9 {
        let ang = PI * 0.75 + k as f32 * PI * 1.5 / 8.0;
        let d = vec2(ang.cos(), ang.sin());
        a.line(c + d * a.w * 0.25, c + d * a.w * 0.3, 0.014, t.art_ink);
    }
    a.arc(c, 0.19, PI * 0.8, PI * 1.5, 0.04, t.art_blue);
    a.arc(c, 0.19, PI * 1.5, PI * 2.2, 0.04, t.art_red);
    a.line(c, c + vec2(0.1 * a.w, -0.1 * a.w), 0.03, t.art_ink);
    a.dot(c, 0.035, t.art_ink);
}

fn heater(a: &Art) {
    let t = a.tk;
    for k in 0..3 {
        a.wave(0.3 + k as f32 * 0.2, 0.3, 0.08, 0.03, 0.028, t.art_red);
    }
    a.boxed(a.rc(0.1, 0.34, 0.9, 0.82), 0.05, t.art_paper);
    for k in 0..6 {
        let x = 0.2 + k as f32 * 0.12;
        a.line(a.at(x, 0.4), a.at(x, 0.76), 0.05, shade(t.art_steel, 0.85));
    }
    a.plain(a.rc(0.16, 0.82, 0.26, 0.9), 0.0, t.art_ink);
    a.plain(a.rc(0.74, 0.82, 0.84, 0.9), 0.0, t.art_ink);
}

fn air_conditioner(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.06, 0.16, 0.94, 0.52), 0.06, t.art_paper);
    a.plain(a.rc(0.12, 0.42, 0.88, 0.5), 0.03, shade(t.art_steel, 0.9));
    a.plain(a.rc(0.7, 0.23, 0.86, 0.32), 0.015, t.art_ink);
    a.dot(a.at(0.74, 0.275), 0.018, t.art_green);
    for k in 0..3 {
        let x = 0.12 + k as f32 * 0.06;
        a.line(a.at(x, 0.24), a.at(x + 0.04, 0.24), 0.014, t.art_steel);
    }
    for k in 0..3 {
        let x = 0.26 + k as f32 * 0.24;
        let pts: Vec<Pos2> = (0..=10)
            .map(|i| {
                let f = i as f32 / 10.0;
                a.at(x + (k as f32 - 1.0) * 0.07 * f * f, 0.56 + 0.34 * f)
            })
            .collect();
        a.p.add(Shape::line(pts, Stroke::new((a.w * 0.032).max(1.0), t.art_blue)));
    }
}

/// A drop of radius `r` around `(cx, cy)` with its point at `tip_y`.
fn droplet(a: &Art, cx: f32, cy: f32, r: f32, tip_y: f32, fill: Color32) {
    let c = a.at(cx, cy);
    let d = cy - tip_y;
    let span = (r / d).clamp(0.0, 1.0).acos();
    let mut pts = vec![a.at(cx, tip_y)];
    for i in 0..=20 {
        let ang = -FRAC_PI_2 + span + (TAU - 2.0 * span) * i as f32 / 20.0;
        pts.push(c + vec2(ang.cos(), ang.sin()) * a.w * r);
    }
    a.poly(pts, fill);
    a.dot(c + vec2(-a.w * r * 0.35, -a.w * r * 0.1), r * 0.18, Color32::from_white_alpha(170));
}

fn humidity(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.3, 0.72, 0.7, 0.9), 0.04, t.art_steel);
    a.line(a.at(0.4, 0.81), a.at(0.6, 0.81), 0.014, t.art_ink);
    droplet(a, 0.5, 0.46, 0.23, 0.06, t.art_blue);
    a.text(a.at(0.5, 0.5), "%", 0.2, t.art_paper);
}

fn window(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.12, 0.08, 0.88, 0.9), 0.03, t.art_paper);
    for (x0, y0) in [(0.19, 0.15), (0.52, 0.15), (0.19, 0.5), (0.52, 0.5)] {
        a.plain(a.rc(x0, y0, x0 + 0.29, y0 + 0.31), 0.01, t.art_glass);
        a.line(a.at(x0 + 0.05, y0 + 0.24), a.at(x0 + 0.13, y0 + 0.08), 0.02, Color32::from_white_alpha(200));
    }
    a.boxed(a.rc(0.68, 0.8, 0.82, 0.88), 0.01, t.art_steel);
    a.dot(a.at(0.75, 0.84), 0.018, t.art_green);
}

fn soil(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.36, 0.08, 0.64, 0.32), 0.05, t.art_green);
    a.dot(a.at(0.5, 0.2), 0.03, t.art_paper);
    a.boxed(a.rc(0.45, 0.32, 0.55, 0.72), 0.01, t.art_steel);
    a.poly(vec![a.at(0.08, 0.9), a.at(0.92, 0.9), a.at(0.82, 0.58), a.at(0.18, 0.58)], t.art_earth);
    a.plain(a.rc(0.43, 0.56, 0.57, 0.6), 0.0, shade(t.art_earth, 0.55));
    for (x, y) in [(0.28, 0.7), (0.7, 0.75), (0.4, 0.82)] {
        a.dot(a.at(x, y), 0.02, t.art_blue);
    }
}

fn sprinkler(a: &Art) {
    let t = a.tk;
    for k in [-1.0f32, 0.0, 1.0] {
        let tip = a.at(0.5 + k * 0.3, 0.16 + 0.12 * k.abs());
        let from = a.at(0.5, 0.52);
        let ctl = a.at(0.5 + k * 0.16, 0.08);
        let pts: Vec<Pos2> = (0..=12)
            .map(|i| {
                let f = i as f32 / 12.0;
                let u = 1.0 - f;
                pos2(u * u * from.x + 2.0 * u * f * ctl.x + f * f * tip.x, u * u * from.y + 2.0 * u * f * ctl.y + f * f * tip.y)
            })
            .collect();
        a.p.add(Shape::line(pts, Stroke::new((a.w * 0.03).max(1.0), t.art_blue)));
        a.dot(tip, 0.03, t.art_blue);
    }
    a.boxed(a.rc(0.44, 0.52, 0.56, 0.82), 0.01, t.art_steel);
    a.boxed(a.rc(0.38, 0.46, 0.62, 0.56), 0.03, t.art_ink);
    a.plain(a.rc(0.06, 0.82, 0.94, 0.9), 0.02, t.art_green);
}

fn carbon_monoxide(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.1, 0.16, 0.9, 0.84), 0.12, t.art_paper);
    a.disc(a.at(0.5, 0.5), 0.26, t.art_steel);
    a.disc(a.at(0.5, 0.5), 0.2, t.art_paper);
    a.text(a.at(0.5, 0.5), "CO", 0.2, t.art_ink);
    a.dot(a.at(0.8, 0.26), 0.03, t.art_red);
    for k in 0..3 {
        a.line(a.at(0.17, 0.68 + k as f32 * 0.05), a.at(0.27, 0.68 + k as f32 * 0.05), 0.014, t.art_steel);
    }
}

fn plug(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.34, 0.06, 0.43, 0.34), 0.01, t.art_steel);
    a.boxed(a.rc(0.57, 0.06, 0.66, 0.34), 0.01, t.art_steel);
    a.boxed(a.rc(0.2, 0.3, 0.8, 0.88), 0.11, t.art_paper);
    a.disc(a.at(0.5, 0.59), 0.13, t.art_steel);
    a.dot(a.at(0.5, 0.59), 0.05, t.art_green);
    a.line(a.at(0.3, 0.4), a.at(0.7, 0.4), 0.012, t.art_steel);
}

fn garage(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.12, 0.44, 0.88, 0.9), 0.02, t.art_paper);
    for k in 1..4 {
        let y = 0.44 + k as f32 * 0.115;
        a.line(a.at(0.12, y), a.at(0.88, y), 0.016, t.art_steel);
    }
    for k in 0..3 {
        let x = 0.24 + k as f32 * 0.2;
        a.plain(a.rc(x, 0.48, x + 0.14, 0.52), 0.005, t.art_glass);
    }
    a.dot(a.at(0.5, 0.82), 0.02, t.art_ink);
    a.poly(vec![a.at(0.04, 0.46), a.at(0.5, 0.08), a.at(0.96, 0.46)], t.art_red);
}

fn television(a: &Art, wired: bool) {
    let t = a.tk;
    a.boxed(a.rc(0.46, 0.64, 0.54, 0.78), 0.01, t.art_steel);
    a.boxed(a.rc(0.28, 0.78, 0.72, 0.86), 0.04, t.art_steel);
    a.boxed(a.rc(0.06, 0.12, 0.94, 0.68), 0.04, shade(t.art_ink, 1.5));
    a.plain(a.rc(0.12, 0.18, 0.88, 0.62), 0.02, t.art_blue);
    a.line(a.at(0.2, 0.55), a.at(0.38, 0.23), 0.04, Color32::from_white_alpha(70));
    a.line(a.at(0.32, 0.57), a.at(0.44, 0.33), 0.025, Color32::from_white_alpha(70));
    if wired {
        a.p.add(Shape::line(vec![a.at(0.72, 0.84), a.at(0.82, 0.9), a.at(0.94, 0.86)], Stroke::new((a.w * 0.03).max(1.0), t.art_green)));
        a.plain(a.rc(0.9, 0.82, 0.97, 0.9), 0.01, t.art_green);
    } else {
        a.dot(a.at(0.82, 0.66), 0.016, t.art_green);
    }
}

fn speaker(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.22, 0.06, 0.78, 0.92), 0.07, shade(t.art_ink, 1.6));
    a.disc(a.at(0.5, 0.27), 0.085, t.art_steel);
    a.dot(a.at(0.5, 0.27), 0.03, t.art_ink);
    a.disc(a.at(0.5, 0.64), 0.2, t.art_steel);
    a.disc(a.at(0.5, 0.64), 0.13, shade(t.art_ink, 1.2));
    a.dot(a.at(0.5, 0.64), 0.05, t.art_steel);
    a.dot(a.at(0.5, 0.12), 0.015, t.art_amber);
}

fn water(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.2, 0.14, 0.8, 0.88), 0.08, t.art_glass);
    a.plain(a.rc(0.23, 0.5, 0.77, 0.85), 0.05, t.art_blue);
    a.line(a.at(0.23, 0.5), a.at(0.77, 0.5), 0.02, Color32::from_white_alpha(170));
    a.boxed(a.rc(0.3, 0.06, 0.7, 0.16), 0.03, t.art_steel);
    a.bar(a.at(0.5, 0.16), a.at(0.5, 0.48), 0.014, t.art_steel);
    a.disc(a.at(0.5, 0.5), 0.045, t.art_red);
    for k in 0..4 {
        a.line(a.at(0.68, 0.28 + k as f32 * 0.14), a.at(0.77, 0.28 + k as f32 * 0.14), 0.014, t.art_ink);
    }
}

fn fan(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.46, 0.68, 0.54, 0.84), 0.01, t.art_steel);
    a.boxed(a.rc(0.28, 0.82, 0.72, 0.9), 0.04, t.art_steel);
    let c = a.at(0.5, 0.4);
    a.disc(c, 0.34, t.art_glass);
    a.arc(c, 0.24, 0.0, TAU, 0.014, t.art_steel);
    for k in 0..3 {
        let ang = k as f32 * TAU / 3.0 - FRAC_PI_2;
        let tip = c + vec2(ang.cos(), ang.sin()) * a.w * 0.26;
        let side = c + vec2((ang + 0.85).cos(), (ang + 0.85).sin()) * a.w * 0.2;
        a.poly(vec![c, tip, side], t.art_paper);
    }
    a.disc(c, 0.05, t.art_ink);
}

fn streetlight(a: &Art) {
    let t = a.tk;
    a.poly_plain(vec![a.at(0.5, 0.3), a.at(0.72, 0.3), a.at(0.86, 0.9), a.at(0.36, 0.9)], t.art_amber.gamma_multiply(0.3));
    a.boxed(a.rc(0.14, 0.86, 0.42, 0.92), 0.01, t.art_steel);
    a.boxed(a.rc(0.23, 0.22, 0.31, 0.88), 0.01, t.art_steel);
    a.bar(a.at(0.27, 0.24), a.at(0.5, 0.14), 0.05, t.art_steel);
    a.poly(vec![a.at(0.4, 0.12), a.at(0.82, 0.12), a.at(0.76, 0.26), a.at(0.46, 0.26)], shade(t.art_steel, 0.7));
    a.poly_plain(vec![a.at(0.5, 0.26), a.at(0.72, 0.26), a.at(0.7, 0.31), a.at(0.52, 0.31)], t.art_amber);
}

fn traffic(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.46, 0.82, 0.54, 0.94), 0.01, t.art_steel);
    a.boxed(a.rc(0.28, 0.04, 0.72, 0.84), 0.07, shade(t.art_ink, 1.5));
    for (k, col) in [t.art_red, t.art_amber, t.art_green].into_iter().enumerate() {
        let y = 0.2 + k as f32 * 0.24;
        let lit = k == 0;
        a.disc(a.at(0.5, y), 0.095, if lit { col } else { shade(col, 0.55) });
        a.line(a.at(0.37, y - 0.115), a.at(0.63, y - 0.115), 0.03, t.art_ink);
    }
}

fn parking(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.46, 0.56, 0.54, 0.82), 0.01, t.art_steel);
    a.poly(ellipse(a.at(0.5, 0.85), a.w * 0.22, a.w * 0.06), t.art_steel);
    a.dot(a.at(0.5, 0.85), 0.025, t.art_green);
    a.boxed(a.rc(0.16, 0.06, 0.84, 0.6), 0.06, t.art_blue);
    a.p.rect_stroke(a.rc(0.2, 0.1, 0.8, 0.56), a.w * 0.04, Stroke::new(a.lw(), t.art_paper), StrokeKind::Inside);
    a.text(a.at(0.5, 0.33), "P", 0.4, t.art_paper);
}

fn drain(a: &Art) {
    let t = a.tk;
    droplet(a, 0.5, 0.2, 0.1, 0.02, t.art_blue);
    a.boxed(a.rc(0.04, 0.34, 0.96, 0.44), 0.02, shade(t.art_steel, 1.1));
    a.boxed(a.rc(0.1, 0.44, 0.9, 0.84), 0.02, shade(t.art_ink, 1.4));
    for k in 0..7 {
        let x = 0.17 + k as f32 * 0.11;
        a.line(a.at(x, 0.5), a.at(x, 0.78), 0.05, t.art_steel);
    }
}

fn air(a: &Art) {
    let t = a.tk;
    a.blob(&[(0.28, 0.32, 0.12), (0.46, 0.22, 0.15), (0.64, 0.28, 0.13), (0.76, 0.35, 0.1), (0.5, 0.36, 0.14)], t.art_paper);
    a.boxed(a.rc(0.24, 0.58, 0.76, 0.9), 0.05, t.art_steel);
    for (k, (h, col)) in [(0.12f32, t.art_green), (0.2, t.art_amber), (0.08, t.art_green)].into_iter().enumerate() {
        let x = 0.32 + k as f32 * 0.14;
        a.plain(a.rc(x, 0.84 - h, x + 0.09, 0.84), 0.0, col);
    }
}

fn blower(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.44, 0.12, 0.94, 0.38), 0.03, t.art_steel);
    a.plain(a.rc(0.9, 0.1, 0.96, 0.4), 0.0, t.art_ink);
    let c = a.at(0.46, 0.58);
    a.disc(c, 0.34, t.art_steel);
    a.p.circle_stroke(c, a.w * 0.27, Stroke::new(a.lw() * 0.8, shade(t.art_steel, 0.7)));
    for k in 0..6 {
        let ang = k as f32 * TAU / 6.0;
        let mid = c + vec2((ang + 0.3).cos(), (ang + 0.3).sin()) * a.w * 0.14;
        let tip = c + vec2((ang + 0.7).cos(), (ang + 0.7).sin()) * a.w * 0.24;
        a.p.add(Shape::line(vec![c, mid, tip], Stroke::new((a.w * 0.035).max(1.0), t.art_paper)));
    }
    a.disc(c, 0.05, t.art_ink);
}

fn battery(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.84, 0.42, 0.94, 0.58), 0.02, t.art_steel);
    a.boxed(a.rc(0.06, 0.26, 0.86, 0.74), 0.07, t.art_paper);
    for k in 0..4 {
        let x = 0.12 + k as f32 * 0.185;
        a.plain(a.rc(x, 0.33, x + 0.15, 0.67), 0.02, if k < 3 { t.art_green } else { shade(t.art_steel, 1.1) });
    }
}

fn meter(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.3, 0.8, 0.7, 0.92), 0.03, t.art_steel);
    let c = a.at(0.5, 0.46);
    a.disc(c, 0.38, t.art_steel);
    a.disc(c, 0.31, t.art_glass);
    a.boxed(a.rc(0.32, 0.26, 0.68, 0.39), 0.02, t.art_ink);
    a.text(a.at(0.5, 0.325), "0427", 0.1, t.art_green);
    a.p.add(Shape::line(
        vec![a.at(0.54, 0.47), a.at(0.45, 0.59), a.at(0.55, 0.59), a.at(0.46, 0.72)],
        Stroke::new((a.w * 0.05).max(1.2), t.art_amber),
    ));
}

fn solar(a: &Art) {
    let t = a.tk;
    a.bar(a.at(0.36, 0.66), a.at(0.3, 0.9), 0.025, t.art_steel);
    a.bar(a.at(0.66, 0.66), a.at(0.7, 0.9), 0.025, t.art_steel);
    let (bl, br, tr, tl) = (a.at(0.08, 0.7), a.at(0.76, 0.7), a.at(0.94, 0.26), a.at(0.26, 0.26));
    a.poly(vec![bl, br, tr, tl], t.art_blue);
    for k in 1..4 {
        let f = k as f32 / 4.0;
        a.p.line_segment([bl + (br - bl) * f, tl + (tr - tl) * f], Stroke::new(a.lw() * 0.7, t.art_paper));
    }
    for k in 1..3 {
        let f = k as f32 / 3.0;
        a.p.line_segment([bl + (tl - bl) * f, br + (tr - br) * f], Stroke::new(a.lw() * 0.7, t.art_paper));
    }
    let sun = a.at(0.14, 0.14);
    for k in 0..8 {
        let ang = k as f32 * TAU / 8.0;
        let d = vec2(ang.cos(), ang.sin());
        a.line(sun + d * a.w * 0.08, sun + d * a.w * 0.12, 0.02, t.art_amber);
    }
    a.disc(sun, 0.06, t.art_amber);
}

fn turbine(a: &Art) {
    let t = a.tk;
    a.poly(vec![a.at(0.46, 0.9), a.at(0.54, 0.9), a.at(0.525, 0.42), a.at(0.475, 0.42)], t.art_paper);
    a.boxed(a.rc(0.36, 0.88, 0.64, 0.93), 0.01, t.art_steel);
    let hub = a.at(0.5, 0.4);
    for k in 0..3 {
        let ang = k as f32 * TAU / 3.0 - FRAC_PI_2 + 0.35;
        let d = vec2(ang.cos(), ang.sin());
        let n = vec2(-d.y, d.x);
        a.poly(vec![hub + n * a.w * 0.035, hub + d * a.w * 0.36, hub - n * a.w * 0.02], t.art_paper);
    }
    a.disc(hub, 0.055, t.art_steel);
}

/// The generic thing: a small cube.
fn cube(a: &Art) {
    let t = a.tk;
    a.poly(vec![a.at(0.5, 0.52), a.at(0.86, 0.3), a.at(0.86, 0.7), a.at(0.5, 0.92)], shade(t.art_blue, 0.65));
    a.poly(vec![a.at(0.14, 0.3), a.at(0.5, 0.52), a.at(0.5, 0.92), a.at(0.14, 0.7)], shade(t.art_blue, 0.85));
    a.poly(vec![a.at(0.5, 0.08), a.at(0.86, 0.3), a.at(0.5, 0.52), a.at(0.14, 0.3)], shade(t.art_blue, 1.25));
    a.dot(a.at(0.5, 0.3), 0.04, t.art_paper);
    a.dot(a.at(0.32, 0.67), 0.03, t.art_paper);
}

// ---------------------------------------------------------------------------------------------
// Components.

fn led(a: &Art) {
    let t = a.tk;
    let c = a.at(0.5, 0.32);
    for k in 0..5 {
        let ang = -PI * 0.9 + k as f32 * PI * 0.8 / 4.0;
        let d = vec2(ang.cos(), ang.sin());
        a.line(c + d * a.w * 0.26, c + d * a.w * 0.34, 0.025, t.art_amber);
    }
    a.bar(a.at(0.43, 0.64), a.at(0.43, 0.92), 0.02, t.art_steel);
    a.bar(a.at(0.57, 0.64), a.at(0.57, 0.84), 0.02, t.art_steel);
    let mut pts = vec![a.at(0.34, 0.62)];
    pts.extend((0..=14).map(|i| PI + PI * i as f32 / 14.0).map(|ang| c + vec2(ang.cos(), ang.sin()) * a.w * 0.16));
    pts.push(a.at(0.66, 0.62));
    a.poly(pts, t.art_red);
    a.arc(c, 0.1, PI + 0.5, PI + 1.4, 0.025, shade(t.art_red, 1.7));
    a.boxed(a.rc(0.3, 0.58, 0.7, 0.66), 0.01, shade(t.art_red, 0.8));
}

fn buzzer(a: &Art) {
    let t = a.tk;
    for s in [-1.0f32, 1.0] {
        a.bar(a.at(0.5 + s * 0.12, 0.8), a.at(0.5 + s * 0.12, 0.93), 0.02, t.art_steel);
    }
    let c = a.at(0.45, 0.48);
    a.disc(c, 0.33, shade(t.art_ink, 1.5));
    a.p.circle_stroke(c, a.w * 0.26, Stroke::new(a.lw() * 0.8, shade(t.art_ink, 2.4)));
    a.disc(c, 0.06, t.art_ink);
    a.text(a.at(0.3, 0.3), "+", 0.12, t.art_paper);
    for k in 0..2 {
        a.arc(c, 0.4 + k as f32 * 0.07, -0.55, 0.55, 0.026, t.art_amber);
    }
}

fn motor(a: &Art) {
    let t = a.tk;
    a.bar(a.at(0.66, 0.5), a.at(0.94, 0.5), 0.05, t.art_paper);
    a.plain(a.rc(0.04, 0.34, 0.14, 0.4), 0.0, t.art_amber);
    a.plain(a.rc(0.04, 0.6, 0.14, 0.66), 0.0, t.art_amber);
    a.boxed(a.rc(0.12, 0.26, 0.5, 0.74), 0.05, t.art_steel);
    a.boxed(a.rc(0.5, 0.3, 0.7, 0.7), 0.03, shade(t.art_steel, 0.75));
    for k in 0..3 {
        let x = 0.54 + k as f32 * 0.05;
        a.line(a.at(x, 0.33), a.at(x, 0.67), 0.012, t.art_ink);
    }
    a.text(a.at(0.31, 0.5), "M", 0.22, t.art_ink);
}

fn relay(a: &Art) {
    let t = a.tk;
    for k in 0..5 {
        let x = 0.24 + k as f32 * 0.13;
        a.boxed(a.rc(x - 0.015, 0.78, x + 0.015, 0.93), 0.0, t.art_steel);
    }
    a.boxed(a.rc(0.12, 0.2, 0.88, 0.8), 0.05, t.art_blue);
    a.boxed(a.rc(0.2, 0.28, 0.8, 0.68), 0.03, t.art_paper);
    // The contact: a lever between two terminals, and the coil's zigzag under it.
    a.dot(a.at(0.32, 0.4), 0.03, t.art_ink);
    a.dot(a.at(0.68, 0.4), 0.03, t.art_ink);
    a.line(a.at(0.32, 0.4), a.at(0.6, 0.31), 0.025, t.art_ink);
    let zig: Vec<Pos2> = (0..=8).map(|i| a.at(0.3 + i as f32 * 0.05, if i % 2 == 0 { 0.6 } else { 0.52 })).collect();
    a.p.add(Shape::line(zig, Stroke::new((a.w * 0.022).max(1.0), t.art_ink)));
}

fn lcd(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.04, 0.2, 0.96, 0.82), 0.04, t.art_pcb_green);
    for k in 0..12 {
        a.plain(a.rc(0.12 + k as f32 * 0.065, 0.23, 0.15 + k as f32 * 0.065, 0.28), 0.0, t.art_amber);
    }
    a.boxed(a.rc(0.1, 0.32, 0.9, 0.74), 0.02, shade(t.art_green, 1.15));
    for row in 0..2 {
        for k in 0..8 {
            let x = 0.14 + k as f32 * 0.09;
            let y = 0.37 + row as f32 * 0.17;
            a.plain(a.rc(x, y, x + 0.065, y + 0.13), 0.0, shade(t.art_ink, 1.1));
        }
    }
}

fn button(a: &Art) {
    let t = a.tk;
    for y in [0.32, 0.68] {
        a.bar(a.at(0.12, y), a.at(0.3, y), 0.025, t.art_steel);
        a.bar(a.at(0.7, y), a.at(0.88, y), 0.025, t.art_steel);
    }
    a.boxed(a.rc(0.22, 0.16, 0.78, 0.84), 0.06, shade(t.art_ink, 1.7));
    a.disc(a.at(0.5, 0.5), 0.2, t.art_red);
    a.arc(a.at(0.5, 0.5), 0.13, PI + 0.3, PI + 1.3, 0.025, shade(t.art_red, 1.7));
}

fn potentiometer(a: &Art) {
    let t = a.tk;
    for x in [0.34, 0.5, 0.66] {
        a.bar(a.at(x, 0.74), a.at(x, 0.93), 0.02, t.art_steel);
    }
    a.boxed(a.rc(0.16, 0.56, 0.84, 0.78), 0.04, t.art_blue);
    let c = a.at(0.5, 0.4);
    for k in 0..7 {
        let ang = PI * 0.8 + k as f32 * PI * 1.4 / 6.0;
        let d = vec2(ang.cos(), ang.sin());
        a.line(c + d * a.w * 0.32, c + d * a.w * 0.37, 0.016, t.art_ink);
    }
    a.disc(c, 0.26, t.art_steel);
    a.disc(c, 0.18, shade(t.art_steel, 1.2));
    a.line(c, c + vec2(0.08 * a.w, -0.12 * a.w), 0.035, t.art_ink);
}

fn ultrasonic(a: &Art) {
    let t = a.tk;
    for k in 0..4 {
        a.plain(a.rc(0.36 + k as f32 * 0.09, 0.74, 0.4 + k as f32 * 0.09, 0.9), 0.0, t.art_amber);
    }
    a.boxed(a.rc(0.04, 0.22, 0.96, 0.76), 0.04, t.art_pcb_blue);
    for x in [0.27, 0.73] {
        a.disc(a.at(x, 0.49), 0.19, t.art_steel);
        a.disc(a.at(x, 0.49), 0.13, shade(t.art_ink, 1.3));
        a.p.circle_stroke(a.at(x, 0.49), a.w * 0.07, Stroke::new(a.lw() * 0.7, t.art_steel));
    }
    a.plain(a.rc(0.46, 0.4, 0.54, 0.5), 0.01, t.art_steel);
    a.plain(a.rc(0.42, 0.56, 0.58, 0.6), 0.0, t.art_amber);
}

fn photoresistor(a: &Art) {
    let t = a.tk;
    for k in 0..3 {
        let o = k as f32 * 0.09;
        a.bar(a.at(0.14 + o, 0.08 + o * 0.2), a.at(0.28 + o, 0.22 + o * 0.2), 0.022, t.art_amber);
    }
    a.bar(a.at(0.42, 0.7), a.at(0.42, 0.93), 0.02, t.art_steel);
    a.bar(a.at(0.62, 0.7), a.at(0.62, 0.93), 0.02, t.art_steel);
    let c = a.at(0.54, 0.5);
    a.disc(c, 0.24, t.art_earth);
    a.p.circle_stroke(c, a.w * 0.19, Stroke::new(a.lw() * 0.7, shade(t.art_amber, 0.8)));
    let zig: Vec<Pos2> = (0..=8).map(|i| a.at(0.4 + i as f32 * 0.035, if i % 2 == 0 { 0.4 } else { 0.58 })).collect();
    a.p.add(Shape::line(zig, Stroke::new((a.w * 0.025).max(1.0), t.art_amber)));
}

fn toggle(a: &Art) {
    let t = a.tk;
    for x in [0.34, 0.5, 0.66] {
        a.bar(a.at(x, 0.76), a.at(x, 0.93), 0.02, t.art_steel);
    }
    a.boxed(a.rc(0.18, 0.58, 0.82, 0.78), 0.04, t.art_steel);
    a.boxed(a.rc(0.34, 0.5, 0.66, 0.6), 0.02, shade(t.art_steel, 1.2));
    a.bar(a.at(0.5, 0.52), a.at(0.64, 0.14), 0.05, t.art_paper);
    a.disc(a.at(0.64, 0.13), 0.06, t.art_red);
}

// ---------------------------------------------------------------------------------------------
// Boards.

fn board_sbc(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.04, 0.16, 0.96, 0.84), 0.04, t.art_pcb_green);
    for (x, y) in [(0.1, 0.22), (0.9, 0.22), (0.1, 0.78), (0.9, 0.78)] {
        a.dot(a.at(x, y), 0.03, t.art_amber);
        a.dot(a.at(x, y), 0.015, t.art_ink);
    }
    for k in 0..10 {
        let x = 0.17 + k as f32 * 0.052;
        a.plain(a.rc(x, 0.21, x + 0.035, 0.25), 0.0, t.art_amber);
        a.plain(a.rc(x, 0.26, x + 0.035, 0.3), 0.0, t.art_amber);
    }
    a.boxed(a.rc(0.3, 0.38, 0.58, 0.68), 0.02, shade(t.art_ink, 1.2));
    a.dot(a.at(0.34, 0.42), 0.015, t.art_paper);
    a.boxed(a.rc(0.64, 0.4, 0.72, 0.5), 0.01, shade(t.art_ink, 1.4));
    a.boxed(a.rc(0.76, 0.38, 0.97, 0.62), 0.02, t.art_steel);
    a.plain(a.rc(0.8, 0.43, 0.93, 0.57), 0.01, t.art_ink);
    a.boxed(a.rc(0.78, 0.66, 0.97, 0.78), 0.02, t.art_steel);
    a.boxed(a.rc(0.06, 0.46, 0.2, 0.66), 0.01, shade(t.art_ink, 1.5));
}

fn board_mcu(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.26, 0.04, 0.74, 0.9), 0.04, t.art_pcb_blue);
    let zig: Vec<Pos2> = (0..=8).map(|i| a.at(0.34 + i as f32 * 0.04, if i % 2 == 0 { 0.1 } else { 0.18 })).collect();
    a.p.add(Shape::line(zig, Stroke::new((a.w * 0.022).max(1.0), t.art_amber)));
    a.boxed(a.rc(0.34, 0.24, 0.66, 0.44), 0.02, t.art_steel);
    a.boxed(a.rc(0.36, 0.5, 0.64, 0.72), 0.02, shade(t.art_ink, 1.2));
    a.dot(a.at(0.4, 0.54), 0.015, t.art_paper);
    for k in 0..8 {
        let y = 0.2 + k as f32 * 0.085;
        a.plain(a.rc(0.285, y, 0.325, y + 0.04), 0.0, t.art_amber);
        a.plain(a.rc(0.675, y, 0.715, y + 0.04), 0.0, t.art_amber);
    }
    a.boxed(a.rc(0.4, 0.86, 0.6, 0.95), 0.01, t.art_steel);
    a.dot(a.at(0.58, 0.8), 0.018, t.art_green);
}

fn plc(a: &Art) {
    let t = a.tk;
    a.boxed(a.rc(0.02, 0.86, 0.98, 0.92), 0.01, t.art_steel);
    a.boxed(a.rc(0.08, 0.1, 0.92, 0.86), 0.04, shade(t.art_steel, 1.15));
    for (y0, y1) in [(0.1, 0.26), (0.7, 0.86)] {
        a.boxed(a.rc(0.08, y0, 0.92, y1), 0.03, shade(t.art_ink, 1.3));
        for k in 0..6 {
            a.disc(a.at(0.17 + k as f32 * 0.133, (y0 + y1) / 2.0), 0.04, t.art_steel);
        }
    }
    a.plain(a.rc(0.08, 0.26, 0.15, 0.7), 0.0, t.art_amber);
    for (k, col) in [t.art_green, t.art_green, t.art_amber, t.art_red].into_iter().enumerate() {
        a.dot(a.at(0.24 + k as f32 * 0.09, 0.33), 0.022, col);
    }
    a.boxed(a.rc(0.22, 0.4, 0.84, 0.62), 0.02, t.art_paper);
    a.text(a.at(0.53, 0.51), "PLC", 0.15, t.art_ink);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use egui::{Context, LayerId};
    use netlabcraft_model::models::MODELS;

    use super::*;
    use crate::theme;

    /// What `model_icon` paints for a model in a 64 px cell: a fingerprint of every shape and the box they cover.
    fn painted(model: &netlabcraft_model::models::Model, dark: bool) -> (String, Rect) {
        let tk = theme::tokens(dark);
        let cell = Rect::from_min_size(pos2(100.0, 100.0), vec2(64.0, 64.0));
        let ctx = Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.ctx().layer_painter(LayerId::background());
            crate::icons_families::model_icon(&p, cell, model.kind, model.id, model.multilayer(), &tk);
        });
        out.textures_delta.clear();
        let mut seen = String::new();
        let mut area = Rect::NOTHING;
        for s in &out.shapes {
            match &s.shape {
                Shape::Text(t) => seen.push_str(&format!("T{}{:?}", t.galley.job.text, t.pos)),
                other => seen.push_str(&format!("{other:?}")),
            }
            area = area.union(s.shape.visual_bounding_rect());
        }
        (seen, area)
    }

    #[test]
    fn every_model_draws_something_inside_its_cell_in_both_themes() {
        let cell = Rect::from_min_size(pos2(100.0, 100.0), vec2(64.0, 64.0));
        for dark in [false, true] {
            for m in MODELS {
                let (_, area) = painted(m, dark);
                assert!(area.is_positive(), "{} draws nothing", m.id);
                assert!(cell.expand(cell.width() * 0.3).contains_rect(area), "{} spills out of its cell: {area:?}", m.id);
                if is_thing(m.id) {
                    assert!(area.width().max(area.height()) >= 0.55 * cell.width(), "{} is too small: {area:?}", m.id);
                }
            }
        }
    }

    #[test]
    fn the_things_components_and_boards_all_look_different() {
        for dark in [false, true] {
            let mut seen: BTreeMap<String, &str> = BTreeMap::new();
            for m in MODELS.iter().filter(|m| is_thing(m.id) || crate::icons_network::is_network_part(m.id)) {
                let (look, _) = painted(m, dark);
                if let Some(other) = seen.insert(look, m.id) {
                    panic!("{} and {other} are drawn the same (dark={dark})", m.id);
                }
            }
            assert!(seen.len() > 40, "only {} models were checked", seen.len());
        }
    }

    #[test]
    fn the_near_duplicates_are_told_apart() {
        let model = |id: &str| MODELS.iter().find(|m| m.id == id).unwrap_or_else(|| panic!("no model {id}"));
        for (a, b) in [
            ("Cloud", "PSTN"),
            ("Repeater", "Coax-Splitter"),
            ("Switch-IND-8", "Switch-8"),
            ("Router-IND", "Router-2"),
            ("Firewall-5", "Firewall-3"),
            ("Generic-Wired", "PC"),
            ("Generic-Wireless", "Laptop-WiFi"),
            ("VoIP-Adapter", "Analog-Phone"),
            ("VoIP-Adapter", "IP-Phone"),
            ("IoT-TV", "IoT-TVE"),
        ] {
            for dark in [false, true] {
                assert_ne!(painted(model(a), dark).0, painted(model(b), dark).0, "{a} and {b} look the same (dark={dark})");
            }
        }
    }

    #[test]
    fn a_wired_thing_has_no_wifi_marker_and_a_wireless_one_has() {
        let tk = theme::tokens(false);
        let count = |wireless: bool| {
            let ctx = Context::default();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let p = ui.ctx().layer_painter(LayerId::background());
                thing_icon(&p, Rect::from_min_size(pos2(0.0, 0.0), vec2(64.0, 64.0)), "IoT-Plug", wireless, &tk);
            });
            out.textures_delta.clear();
            out.shapes.len()
        };
        assert!(count(true) > count(false));
    }
}
