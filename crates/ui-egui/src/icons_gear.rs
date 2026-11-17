//! The small parts that tell the routers and the switches apart (original artwork, drawn in code). Every
//! router shares one drum and every switch one flat box, so the family look stays, and each model adds what
//! it really has, read from its catalogue ports: a router a row of jacks on its band (green strip over a
//! Gigabit jack, amber over a Fast Ethernet one, a trapezoid serial jack with its cable stub, a row of amber
//! telephone lines on the access server) and blank plates for its empty module bays; a switch a row of
//! ports on its front (one row of eight, two rows of twelve, two dense rows of twenty-four), a Gigabit
//! uplink or two blue fibre cages after them, and a lightning badge when it gives power over Ethernet.

use egui::{Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use netlabcraft_model::DeviceKind;
use netlabcraft_model::cable::Medium;
use netlabcraft_model::models::{self, Model};

use crate::icons_things::Art;
use crate::theme::{Tokens, device_color, shade};

/// How many ports of each sort a model has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Ports {
    pub fast: usize,
    pub gigabit: usize,
    pub fibre: usize,
    pub serial: usize,
    pub modem: usize,
}

/// Counts the ports of a model by the name of their run and the medium it takes.
pub(crate) fn count_ports(m: &Model) -> Ports {
    let mut c = Ports::default();
    for run in m.ports {
        let n = run.last.saturating_sub(run.first) as usize + 1;
        if run.prefix.starts_with("Serial") {
            c.serial += n;
        } else if run.prefix.starts_with("Phone") {
            c.modem += n;
        } else if run.prefix.starts_with("FastEthernet") {
            c.fast += n;
        } else if run.prefix.starts_with("GigabitEthernet") {
            if run.medium == Some(Medium::Fiber) {
                c.fibre += n;
            } else {
                c.gigabit += n;
            }
        }
    }
    c
}

/// Whether [`marks`] adds parts to this model: the routers and the switches, not the industrial ones (which
/// have their own band) nor the firewalls.
pub fn is_gear(model_id: &str) -> bool {
    (model_id.starts_with("Router-") || model_id.starts_with("Switch-") || model_id == "Access-Server") && !model_id.contains("-IND")
}

/// Draws the parts of a router or a switch over the icon of its kind, filling `rect`.
pub fn marks(p: &Painter, rect: Rect, model_id: &str, tk: &Tokens) {
    let Some(m) = models::find(model_id) else { return };
    let a = Art::new(p, rect, tk);
    let ports = count_ports(m);
    match m.kind {
        DeviceKind::Router => router(&a, m, ports),
        DeviceKind::Switch => switch(&a, m.id, ports),
        _ => {}
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Jack {
    Gigabit,
    Fast,
    Serial,
    Modem,
}

/// A router: the jacks in a row along the drum's front band, and a plate for each empty module bay under them.
fn router(a: &Art, m: &Model, n: Ports) {
    let w = a.w;
    let (cx, cy) = (a.r.center().x, a.r.center().y);
    let (rx, ry) = (w * 0.46, w * 0.17);
    let top_y = cy - w * 0.08;
    let side_h = w * 0.24;
    // The middle of the band at `dx` from the drum's centre: the band follows the drum's curve.
    let band_y = |dx: f32| top_y + ry * (1.0 - (dx / rx).powi(2)).max(0.0).sqrt() + side_h * 0.5;
    let mut jacks: Vec<Jack> = Vec::new();
    jacks.extend(std::iter::repeat_n(Jack::Gigabit, n.gigabit));
    jacks.extend(std::iter::repeat_n(Jack::Fast, n.fast));
    jacks.extend(std::iter::repeat_n(Jack::Serial, n.serial));
    jacks.extend(std::iter::repeat_n(Jack::Modem, n.modem));
    let bays = m.slots().len();
    let up = if bays > 0 { -side_h * 0.2 } else { 0.0 };
    let step = (w * 0.105).min(w * 0.7 / jacks.len().max(1) as f32);
    for (i, jack) in jacks.iter().enumerate() {
        let dx = (i as f32 - (jacks.len() as f32 - 1.0) / 2.0) * step;
        let at = pos2(cx + dx, band_y(dx) + up);
        match jack {
            Jack::Gigabit => ethernet_jack(a, at, false),
            Jack::Fast => ethernet_jack(a, at, true),
            Jack::Serial => serial_jack(a, at),
            Jack::Modem => modem_jack(a, at),
        }
    }
    let body = device_color(DeviceKind::Router);
    for k in 0..bays {
        let dx = (k as f32 - (bays as f32 - 1.0) / 2.0) * w * 0.205;
        let at = pos2(cx + dx, band_y(dx) + side_h * 0.2);
        let plate = Rect::from_center_size(at, vec2(w * 0.17, w * 0.06));
        a.p.rect_filled(plate, w * 0.01, shade(body, 1.05));
        a.p.rect_stroke(plate, w * 0.01, Stroke::new(a.lw() * 0.8, shade(body, 0.4)), StrokeKind::Inside);
        for side in [-1.0f32, 1.0] {
            a.dot(at + vec2(side * w * 0.068, 0.0), 0.011, shade(body, 0.4));
        }
    }
}

/// An Ethernet jack: a light shell with a dark opening, and a strip over it that says the speed.
fn ethernet_jack(a: &Art, at: Pos2, fast: bool) {
    let (t, w) = (a.tk, a.w);
    let body = Rect::from_center_size(at, vec2(w * 0.085, w * 0.09));
    a.p.rect_filled(body, 0.0, shade(t.art_paper, 0.9));
    a.p.rect_filled(body.shrink2(vec2(w * 0.016, w * 0.014)), 0.0, t.art_ink);
    let strip = Rect::from_min_size(body.left_top() - vec2(0.0, w * 0.014), vec2(body.width(), (w * 0.014).max(1.0)));
    a.p.rect_filled(strip, 0.0, if fast { t.art_amber } else { t.art_green });
}

/// A serial WAN jack: a trapezoid shell with its cable stub hanging from it.
fn serial_jack(a: &Art, at: Pos2) {
    let (t, w) = (a.tk, a.w);
    a.bar(at + vec2(0.0, w * 0.03), at + vec2(0.0, w * 0.085), 0.024, t.serial_cable);
    let shell = |k: f32| {
        vec![
            at + vec2(-w * 0.05 * k, -w * 0.035 * k),
            at + vec2(w * 0.05 * k, -w * 0.035 * k),
            at + vec2(w * 0.034 * k, w * 0.03 * k),
            at + vec2(-w * 0.034 * k, w * 0.03 * k),
        ]
    };
    a.p.add(Shape::convex_polygon(shell(1.0), t.art_steel, Stroke::NONE));
    a.p.add(Shape::convex_polygon(shell(0.55), t.art_ink, Stroke::NONE));
}

/// A telephone line of the access server's modems: a small amber jack.
fn modem_jack(a: &Art, at: Pos2) {
    let (t, w) = (a.tk, a.w);
    a.p.rect_filled(Rect::from_center_size(at, vec2(w * 0.055, w * 0.065)), 0.0, t.art_amber);
    a.dot(at, 0.014, t.art_ink);
}

/// A switch: a row of ports across the dark front face (one row up to twelve, two beyond), the uplinks after
/// them, and the lightning badge of a PoE model before them.
fn switch(a: &Art, id: &str, n: Ports) {
    let (t, w) = (a.tk, a.w);
    let cy = a.r.center().y;
    let x0 = a.r.left() - w * 0.015;
    let x1 = a.r.right() - w * 0.175;
    let (y0, y1) = (cy + w * 0.09, cy + w * 0.23);
    let ymid = (y0 + y1) / 2.0;
    let mut left = x0;
    if id.contains("PoE") {
        let c = pos2(x0 + w * 0.055, ymid);
        a.p.circle_filled(c, w * 0.058, t.art_amber);
        for tri in [[vec2(0.012, -0.04), vec2(-0.026, 0.006), vec2(0.004, 0.006)], [vec2(-0.004, -0.006), vec2(0.026, -0.006), vec2(-0.012, 0.04)]] {
            a.p.add(Shape::convex_polygon(tri.iter().map(|v| c + *v * w).collect(), t.art_ink, Stroke::NONE));
        }
        left = x0 + w * 0.13;
    }
    let (down, copper_uplinks) = if n.fast > 0 { (n.fast, n.gigabit) } else { (n.gigabit, 0) };
    let uplinks = copper_uplinks + n.fibre;
    let up_w = w * 0.085;
    let right = x1 - uplinks as f32 * up_w - if uplinks > 0 { w * 0.03 } else { 0.0 };
    let rows = if down <= 12 { 1usize } else { 2 };
    let cols = down.div_ceil(rows).max(1);
    let cell_w = (right - left) / cols as f32;
    let row_h = (y1 - y0) / rows as f32;
    let (pw, ph) = ((cell_w * 0.72).max(w * 0.012), (row_h * 0.72).min(w * 0.085));
    let shell = shade(t.art_paper, 0.9);
    for i in 0..down {
        let at = pos2(left + cell_w * ((i % cols) as f32 + 0.5), y0 + row_h * ((i / cols) as f32 + 0.5));
        a.p.rect_filled(Rect::from_center_size(at, vec2(pw, ph)), 0.0, shell);
    }
    for k in 0..uplinks {
        let at = pos2(right + w * 0.03 + up_w * (k as f32 + 0.5), ymid);
        let cage = Rect::from_center_size(at, vec2(up_w * 0.82, (y1 - y0) * 0.8));
        let fibre = k >= copper_uplinks;
        a.p.rect_filled(cage, 0.0, if fibre { t.art_blue } else { t.art_steel });
        a.p.rect_filled(cage.shrink2(vec2(cage.width() * 0.22, cage.height() * 0.28)), 0.0, t.art_ink);
    }
}
