//! Corner badges for the models that wear one over their kind's icon (original artwork, drawn in code): the
//! bridge, the industrial switch and router, the network controller, the data historian and the operator
//! panel. The smart things, the components, the programmable controller and the near-duplicate network
//! models are drawn whole by `icons_things` and `icons_network`.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

const DARK: Color32 = Color32::from_rgb(0x2a, 0x33, 0x40);
const RED: Color32 = Color32::from_rgb(0xd6, 0x45, 0x3a);
const GREEN: Color32 = Color32::from_rgb(0x2e, 0x8b, 0x57);
const BLUE: Color32 = Color32::from_rgb(0x3a, 0x8f, 0xd6);

fn badge_of(model_id: &str) -> bool {
    matches!(model_id, "Bridge-2" | "Switch-IND-8" | "Router-IND" | "Net-Controller" | "Data-Historian" | "HMI-Panel")
}

/// Draws the corner badge of a model that has one.
pub fn mark(p: &Painter, rect: Rect, model_id: &str) {
    if badge_of(model_id) {
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
        _ => {}
    }
}
