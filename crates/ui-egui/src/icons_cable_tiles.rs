//! The pictures on the Connections tiles: one upright connector per cable kind, drawn in code
//! (original artwork, no files). Every kind has its own colour and shape: RJ45-style plugs for
//! the console and copper cables, a narrower RJ11-style plug for the phone, two duplex fiber
//! connectors with an `S` or `M` mark, a coaxial connector, D-shell connectors (pins for the
//! DTE end, sockets for the DCE end), an octal fan, bare wires for the IoT cable and a USB
//! plug with its trident. The cable lines the canvas draws are in `icons_families.rs` and
//! `canvas.rs` and are not used here.
//!
//! The drawing is built as a list of shapes ([`cable_tile_shapes`]) so a test can compare the
//! kinds and check they stay inside the tile; [`cable_tile`] paints them.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};
use netlabcraft_model::cable::Cable;

use crate::theme::{Tokens, shade};

/// Paints the tile picture of `kind` inside `rect` (the tile's icon square).
pub fn cable_tile(p: &Painter, rect: Rect, kind: Cable, tk: &Tokens) {
    p.extend(cable_tile_shapes(rect, kind, tk));
}

/// The shapes of the tile picture of `kind`, all inside `rect`.
pub fn cable_tile_shapes(rect: Rect, kind: Cable, tk: &Tokens) -> Vec<Shape> {
    let mut t = Tile { r: rect.shrink(rect.width() * 0.04), tk, out: Vec::new() };
    match kind {
        Cable::Auto => t.bolt(),
        Cable::Console => t.rj45(0.5, 0.3, tk.tile_sky, 8),
        Cable::Straight => t.rj45(0.5, 0.3, tk.tile_green, 8),
        Cable::Cross => t.rj45(0.5, 0.3, tk.tile_orange, 8),
        Cable::FiberSingle => t.fiber(false),
        Cable::FiberMulti => t.fiber(true),
        Cable::Phone => t.rj45(0.5, 0.2, tk.tile_grey, 4),
        Cable::Coaxial => t.coax(),
        Cable::SerialDce => t.serial(false),
        Cable::SerialDte => t.serial(true),
        Cable::Octal => t.octal(),
        Cable::IotCustom => t.bare_wires(),
        Cable::Usb => t.usb(),
    }
    t.out
}

/// A drawing surface in unit coordinates (0 to 1 across the tile's icon square).
struct Tile<'a> {
    r: Rect,
    tk: &'a Tokens,
    out: Vec<Shape>,
}

/// The outline of a filled part: the same colour, darker.
fn edge(c: Color32) -> Color32 {
    shade(c, 0.55)
}

impl Tile<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + x * self.r.width(), self.r.top() + y * self.r.height())
    }

    fn rc(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.at(x0, y0), self.at(x1, y1))
    }

    /// One device pixel at 28 px; thinner never than 0.8.
    fn lw(&self) -> f32 {
        (self.r.width() / 26.0).max(0.8)
    }

    fn part(&mut self, r: Rect, rounding: f32, fill: Color32) {
        self.out.push(Shape::rect_filled(r, rounding, fill));
        self.out.push(Shape::rect_stroke(r, rounding, Stroke::new(self.lw(), edge(fill)), StrokeKind::Inside));
    }

    fn poly(&mut self, pts: &[(f32, f32)], fill: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.at(x, y)).collect();
        self.out.push(Shape::convex_polygon(pts, fill, Stroke::new(self.lw(), edge(fill))));
    }

    fn seg(&mut self, a: (f32, f32), b: (f32, f32), width: f32, c: Color32) {
        self.out.push(Shape::line_segment([self.at(a.0, a.1), self.at(b.0, b.1)], Stroke::new(width, c)));
    }

    fn dot(&mut self, c: (f32, f32), radius: f32, fill: Color32) {
        self.out.push(Shape::circle_filled(self.at(c.0, c.1), radius * self.r.width(), fill));
    }

    /// The strain-relief boot under a plug and the short cable stub below it.
    fn boot(&mut self, cx: f32, top: f32, half: f32, fill: Color32) {
        let c = shade(fill, 0.7);
        self.poly(&[(cx - half, top), (cx + half, top), (cx + half * 0.6, top + 0.2), (cx - half * 0.6, top + 0.2)], c);
        let w = self.lw() * 2.2;
        self.seg((cx, top + 0.2), (cx, 1.0), w, shade(fill, 0.55));
    }

    /// An upright RJ-style plug: contacts at the top, the boot and a cable stub below.
    fn rj45(&mut self, cx: f32, hw: f32, fill: Color32, pins: usize) {
        let gold = self.tk.tile_gold;
        let face = self.rc(cx - hw, 0.08, cx + hw, 0.72);
        self.part(face, 1.0, fill);
        let window = self.rc(cx - hw * 0.82, 0.12, cx + hw * 0.82, 0.36);
        self.out.push(Shape::rect_filled(window, 0.5, shade(fill, 0.4)));
        let n = pins.max(2);
        for i in 0..n {
            let x = cx - hw * 0.7 + hw * 1.4 * (i as f32 / (n - 1) as f32);
            self.seg((x, 0.15), (x, 0.33), self.lw() * 0.9, gold);
        }
        self.boot(cx, 0.72, hw * 0.75, fill);
    }

    /// Two duplex fiber connectors side by side, held by a clip, with an `S` or `M` mark.
    fn fiber(&mut self, multi: bool) {
        let blue = self.tk.tile_blue;
        let clip = self.rc(0.26, 0.36, 0.62, 0.47);
        self.part(clip, 0.5, shade(blue, 0.8));
        for cx in [0.22, 0.62] {
            let ferrule = self.rc(cx - 0.045, 0.05, cx + 0.045, 0.27);
            self.part(ferrule, 0.5, shade(blue, 1.7));
            let body = self.rc(cx - 0.13, 0.27, cx + 0.13, 0.63);
            self.part(body, 1.0, blue);
            self.boot(cx, 0.63, 0.09, blue);
        }
        // The mark, drawn as strokes so it needs no font.
        let (x0, x1, y0, y1) = (0.8, 0.96, 0.74, 0.96);
        let ym = (y0 + y1) / 2.0;
        let pts: Vec<(f32, f32)> = if multi {
            vec![(x0, y1), (x0, y0), ((x0 + x1) / 2.0, ym), (x1, y0), (x1, y1)]
        } else {
            vec![(x1, y0), (x0, y0), (x0, ym), (x1, ym), (x1, y1), (x0, y1)]
        };
        let pts: Vec<Pos2> = pts.into_iter().map(|(x, y)| self.at(x, y)).collect();
        self.out.push(Shape::line(pts, Stroke::new(self.lw() * 1.1, self.tk.text)));
    }

    /// A coaxial connector: the centre pin, a knurled collar, the nut and the body.
    fn coax(&mut self) {
        let gold = self.tk.tile_gold;
        let cx = 0.5;
        self.seg((cx, 0.04), (cx, 0.22), self.lw() * 1.6, shade(gold, 1.3));
        let collar = self.rc(cx - 0.17, 0.2, cx + 0.17, 0.44);
        self.part(collar, 1.0, gold);
        for y in [0.26, 0.32, 0.38] {
            self.seg((cx - 0.13, y), (cx + 0.13, y), self.lw() * 0.8, shade(gold, 0.6));
        }
        let nut = self.rc(cx - 0.27, 0.44, cx + 0.27, 0.6);
        self.part(nut, 1.0, shade(gold, 0.85));
        self.poly(&[(cx - 0.19, 0.6), (cx + 0.19, 0.6), (cx + 0.1, 0.82), (cx - 0.1, 0.82)], shade(gold, 0.7));
        self.seg((cx, 0.82), (cx, 1.0), self.lw() * 2.2, shade(gold, 0.5));
    }

    /// A D-shell connector with its flange and screws. `pins` shows pins (the DTE end); false
    /// shows sockets (the DCE end): light dots on a dark face against dark rings on a light one.
    fn dshell(&mut self, top: f32, height: f32, fill: Color32, pins: bool) {
        let flange = self.rc(0.04, top, 0.96, top + 0.09);
        self.part(flange, 1.0, shade(fill, 0.85));
        for x in [0.11, 0.89] {
            self.dot((x, top + 0.045), 0.025, shade(fill, 0.35));
        }
        let (a, b) = (top + 0.09, top + height);
        self.poly(&[(0.1, a), (0.9, a), (0.8, b), (0.2, b)], fill);
        let face = if pins { shade(fill, 0.4) } else { shade(fill, 1.6) };
        let (fa, fb) = (a + 0.05, b - 0.05);
        self.poly(&[(0.17, fa), (0.83, fa), (0.74, fb), (0.26, fb)], face);
        for (row, n) in [(0.3f32, 5usize), (0.72, 4)] {
            let y = fa + (fb - fa) * row;
            let half = 0.33 - 0.07 * row;
            for i in 0..n {
                let x = 0.5 + half * 0.8 * ((i as f32 / (n - 1) as f32) * 2.0 - 1.0);
                if pins {
                    self.dot((x, y), 0.035, shade(fill, 1.9));
                } else {
                    self.out.push(Shape::circle_stroke(self.at(x, y), 0.04 * self.r.width(), Stroke::new(self.lw(), shade(fill, 0.35))));
                }
            }
        }
    }

    /// Serial DTE (pins) or DCE (sockets): a blue D-shell with its hood and cable.
    fn serial(&mut self, dte: bool) {
        let blue = self.tk.tile_blue;
        self.dshell(0.1, 0.52, blue, dte);
        self.poly(&[(0.3, 0.62), (0.7, 0.62), (0.6, 0.84), (0.4, 0.84)], shade(blue, 0.7));
        self.seg((0.5, 0.84), (0.5, 1.0), self.lw() * 2.2, shade(blue, 0.55));
    }

    /// Octal: a green D-shell with a short fan of cables.
    fn octal(&mut self) {
        let green = self.tk.tile_green;
        self.dshell(0.03, 0.42, green, true);
        self.poly(&[(0.34, 0.45), (0.66, 0.45), (0.58, 0.56), (0.42, 0.56)], shade(green, 0.7));
        let c = shade(green, 0.6);
        for i in 0..5 {
            let f = i as f32 / 4.0;
            self.seg((0.5 + (f - 0.5) * 0.14, 0.56), (0.12 + 0.76 * f, 0.99), self.lw() * 1.4, c);
        }
    }

    /// IoT custom: a cable end whose sheath is stripped back to a few bare coloured wires.
    fn bare_wires(&mut self) {
        let tk = *self.tk;
        let sheath = self.rc(0.38, 0.56, 0.62, 1.0);
        self.part(sheath, 0.5, tk.tile_grey);
        self.poly(&[(0.38, 0.56), (0.62, 0.56), (0.56, 0.44), (0.44, 0.44)], shade(tk.tile_grey, 0.8));
        let wires = [tk.serial_cable, tk.ok, tk.selection, tk.fiber_cable];
        for (i, c) in wires.into_iter().enumerate() {
            let f = i as f32 / 3.0;
            let from = (0.45 + 0.1 * f, 0.44);
            let to = (0.18 + 0.64 * f, 0.1);
            self.seg(from, to, self.lw() * 1.7, c);
            // The bare copper at the tip.
            let near = (from.0 + (to.0 - from.0) * 0.78, from.1 + (to.1 - from.1) * 0.78);
            self.seg(near, to, self.lw() * 1.7, tk.tile_gold);
        }
    }

    /// USB: a dark rectangular plug carrying the trident mark, with its boot and cable.
    fn usb(&mut self) {
        let dark = self.tk.tile_dark;
        let mark = self.tk.chrome;
        let body = self.rc(0.2, 0.08, 0.8, 0.68);
        self.part(body, 1.5, dark);
        let w = self.lw() * 1.2;
        self.seg((0.5, 0.57), (0.5, 0.22), w, mark);
        self.dot((0.5, 0.58), 0.06, mark);
        let tip = [(0.5, 0.13), (0.43, 0.24), (0.57, 0.24)].map(|(x, y)| self.at(x, y));
        self.out.push(Shape::convex_polygon(tip.to_vec(), mark, Stroke::NONE));
        // Left branch ends in a ring, right branch in a square.
        let left = [self.at(0.5, 0.46), self.at(0.38, 0.4)];
        self.out.push(Shape::line_segment(left, Stroke::new(w, mark)));
        self.dot((0.35, 0.38), 0.045, mark);
        let right = [self.at(0.5, 0.37), self.at(0.62, 0.31)];
        self.out.push(Shape::line_segment(right, Stroke::new(w, mark)));
        self.out.push(Shape::rect_filled(self.rc(0.6, 0.26, 0.68, 0.34), 0.0, mark));
        self.boot(0.5, 0.68, 0.24, dark);
    }

    /// The automatic cable: a lightning bolt (the right kind is picked for you).
    fn bolt(&mut self) {
        let fill = self.tk.fiber_cable;
        let k = self.r.width() * 0.46;
        let c = self.r.center();
        let pt = |x: f32, y: f32| c + egui::vec2(x * k, y * k);
        let (a, b, cc, d, e, f) = (pt(0.25, -1.0), pt(-0.55, 0.1), pt(-0.05, 0.1), pt(-0.3, 1.0), pt(0.6, -0.15), pt(0.1, -0.15));
        // Two convex halves: the fill tessellator wants convex polygons.
        self.out.push(Shape::convex_polygon(vec![a, b, cc, f], fill, Stroke::NONE));
        self.out.push(Shape::convex_polygon(vec![f, cc, d, e], fill, Stroke::NONE));
        self.out.push(Shape::closed_line(vec![a, b, cc, d, e, f], Stroke::new(self.lw(), edge(fill))));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::theme::tokens;

    #[test]
    fn every_cable_tile_draws_inside_its_square_and_differs_in_both_themes() {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), egui::vec2(28.0, 28.0));
        for dark in [false, true] {
            let tk = tokens(dark);
            let mut seen = HashSet::new();
            for kind in Cable::ALL {
                let shapes = cable_tile_shapes(rect, kind, &tk);
                assert!(shapes.len() >= 3, "{kind:?} draws something");
                for s in &shapes {
                    let b = s.visual_bounding_rect();
                    assert!(rect.expand(1.5).contains_rect(b), "{kind:?} stays in its tile: {b:?}");
                }
                assert!(seen.insert(format!("{shapes:?}")), "{kind:?} differs from every other kind (dark: {dark})");
            }
            assert_eq!(seen.len(), Cable::ALL.len());
        }
        // The two themes paint a kind differently.
        for kind in Cable::ALL {
            let light = format!("{:?}", cable_tile_shapes(rect, kind, &tokens(false)));
            let dark = format!("{:?}", cable_tile_shapes(rect, kind, &tokens(true)));
            assert_ne!(light, dark, "{kind:?}");
        }
    }
}
