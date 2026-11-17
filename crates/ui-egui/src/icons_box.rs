//! The pictures of the device box's categories and subcategories, drawn in code (original
//! artwork): the groups that are not one device (a house, a city block, a factory, a pylon, a
//! folder, an LED, a knob) and the device each other group shows.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use netlabcraft_model::DeviceKind;

use crate::bottom::{Category, Sub};
use crate::icons::{self, Glyph};
use crate::theme::Tokens;

/// A category button's picture.
pub fn category_icon(p: &Painter, rect: Rect, cat: Category, tk: &Tokens) {
    match cat {
        Category::NetworkDevices => icons::glyph(p, rect, Glyph::NetworkDevices, tk.glyph),
        Category::EndDevices => icons::glyph(p, rect, Glyph::EndDevices, tk.glyph),
        Category::Components => crate::icons_families::board_icon(p, rect, false),
        Category::Connections => icons::glyph(p, rect, Glyph::Connections, tk.glyph),
        Category::Miscellaneous => folder(p, rect, tk),
    }
}

/// A subcategory button's picture.
pub fn sub_icon(p: &Painter, rect: Rect, sub: Sub, tk: &Tokens) {
    use crate::icons_families::{model_icon, wan_icon};
    match sub {
        Sub::Routers => icons::device(p, rect, DeviceKind::Router, tk),
        Sub::Switches => icons::device(p, rect, DeviceKind::Switch, tk),
        Sub::Hubs => icons::device(p, rect, DeviceKind::Hub, tk),
        Sub::Wireless => model_icon(p, rect, DeviceKind::Switch, "AccessPoint", false, tk),
        Sub::Security => model_icon(p, rect, DeviceKind::Router, "Firewall-5", false, tk),
        Sub::Wan => wan_icon(p, rect, "Cloud", tk),
        Sub::Hosts => icons::device(p, rect, DeviceKind::Pc, tk),
        Sub::Home => house(p, rect, tk),
        Sub::SmartCity => city(p, rect, tk),
        Sub::Industrial => factory(p, rect, tk),
        Sub::PowerGrid => pylon(p, rect, tk),
        Sub::Boards => crate::icons_families::board_icon(p, rect, true),
        Sub::Actuators => led(p, rect, tk),
        Sub::Sensors => knob(p, rect, tk),
        Sub::Cables => icons::glyph(p, rect, Glyph::Cable, tk.glyph),
        Sub::Cabling => crate::closet::fitting_icon(p, rect, netlabcraft_model::furniture::FittingKind::CopperPatchPanel, tk),
        Sub::Recent => icons::glyph(p, rect, Glyph::Clock, tk.glyph),
    }
}

fn body(tk: &Tokens) -> Color32 {
    tk.tile_blue
}

fn edge(tk: &Tokens) -> Stroke {
    Stroke::new(1.0, tk.glyph)
}

fn window(p: &Painter, at: Pos2, size: f32) {
    p.rect_filled(Rect::from_center_size(at, vec2(size, size)), 0.0, Color32::from_rgb(0xf4, 0xe6, 0x9a));
}

/// A house: a roof over a wall with a door and a window.
fn house(p: &Painter, rect: Rect, tk: &Tokens) {
    let w = rect.width();
    let c = rect.center();
    let wall = Rect::from_center_size(c + vec2(0.0, w * 0.1), vec2(w * 0.58, w * 0.4));
    p.rect_filled(wall, 1.0, body(tk));
    p.rect_stroke(wall, 1.0, edge(tk), StrokeKind::Inside);
    let roof = vec![pos2(wall.left() - w * 0.08, wall.top()), pos2(c.x, rect.top() + w * 0.1), pos2(wall.right() + w * 0.08, wall.top())];
    p.add(Shape::convex_polygon(roof, tk.tile_orange, edge(tk)));
    p.rect_filled(Rect::from_min_max(pos2(c.x - w * 0.07, wall.bottom() - w * 0.22), pos2(c.x + w * 0.07, wall.bottom())), 0.0, tk.tile_dark);
    window(p, pos2(wall.left() + w * 0.12, wall.top() + w * 0.12), w * 0.1);
    window(p, pos2(wall.right() - w * 0.12, wall.top() + w * 0.12), w * 0.1);
}

/// A city block: three towers of different heights with lit windows.
fn city(p: &Painter, rect: Rect, tk: &Tokens) {
    let w = rect.width();
    let base = rect.bottom() - w * 0.1;
    for (dx, width, height) in [(-0.3f32, 0.26f32, 0.5f32), (0.0, 0.28, 0.78), (0.3, 0.24, 0.38)] {
        let t = Rect::from_min_max(
            pos2(rect.center().x + dx * w - width * w * 0.5, base - height * w),
            pos2(rect.center().x + dx * w + width * w * 0.5, base),
        );
        p.rect_filled(t, 0.5, body(tk));
        p.rect_stroke(t, 0.5, edge(tk), StrokeKind::Inside);
        for row in 0..((height * 5.0) as i32) {
            window(p, pos2(t.center().x, t.top() + w * 0.08 + row as f32 * w * 0.12), w * 0.06);
        }
    }
}

/// A factory: a saw-tooth roof, a chimney and a smoke puff.
fn factory(p: &Painter, rect: Rect, tk: &Tokens) {
    let w = rect.width();
    let wall = Rect::from_min_max(pos2(rect.left() + w * 0.1, rect.center().y), pos2(rect.right() - w * 0.1, rect.bottom() - w * 0.1));
    p.rect_filled(wall, 0.5, body(tk));
    let top = wall.top();
    let saw = vec![
        pos2(wall.left(), top),
        pos2(wall.left(), top - w * 0.18),
        pos2(wall.left() + w * 0.27, top),
        pos2(wall.left() + w * 0.27, top - w * 0.18),
        pos2(wall.left() + w * 0.54, top),
        pos2(wall.left() + w * 0.54, top - w * 0.18),
        pos2(wall.right(), top),
    ];
    p.add(Shape::convex_polygon(saw, tk.tile_grey, edge(tk)));
    p.rect_stroke(wall, 0.5, edge(tk), StrokeKind::Inside);
    let chimney = Rect::from_min_max(pos2(wall.right() - w * 0.2, top - w * 0.36), pos2(wall.right() - w * 0.08, top));
    p.rect_filled(chimney, 0.0, tk.tile_dark);
    p.circle_filled(chimney.center_top() - vec2(w * 0.04, w * 0.06), w * 0.06, tk.tile_grey);
    window(p, pos2(wall.center().x, wall.center().y + w * 0.06), w * 0.14);
}

/// A transmission pylon: a lattice tower with two cross arms and a wire.
fn pylon(p: &Painter, rect: Rect, tk: &Tokens) {
    let w = rect.width();
    let s = Stroke::new((w / 16.0).max(1.2), tk.glyph);
    let top = pos2(rect.center().x, rect.top() + w * 0.06);
    let (l, r) = (pos2(rect.center().x - w * 0.24, rect.bottom() - w * 0.06), pos2(rect.center().x + w * 0.24, rect.bottom() - w * 0.06));
    p.line_segment([top, l], s);
    p.line_segment([top, r], s);
    for (y, half) in [(0.3f32, 0.4f32), (0.52, 0.28)] {
        let at = rect.top() + w * y;
        p.line_segment([pos2(rect.center().x - w * half, at), pos2(rect.center().x + w * half, at)], s);
    }
    let (a, b) = (pos2(rect.center().x - w * 0.18, rect.top() + w * 0.3), pos2(rect.center().x + w * 0.1, rect.top() + w * 0.52));
    p.line_segment([a, b], Stroke::new(1.0, tk.glyph));
    p.line_segment(
        [pos2(rect.center().x + w * 0.18, rect.top() + w * 0.3), pos2(rect.center().x - w * 0.1, rect.top() + w * 0.52)],
        Stroke::new(1.0, tk.glyph),
    );
    p.add(Shape::line(
        vec![pos2(rect.left(), rect.top() + w * 0.34), pos2(rect.center().x - w * 0.4, rect.top() + w * 0.3)],
        Stroke::new(1.0, tk.tile_orange),
    ));
    p.add(Shape::line(
        vec![pos2(rect.center().x + w * 0.4, rect.top() + w * 0.3), pos2(rect.right(), rect.top() + w * 0.34)],
        Stroke::new(1.0, tk.tile_orange),
    ));
}

/// A folder: a tab and a body.
fn folder(p: &Painter, rect: Rect, tk: &Tokens) {
    let w = rect.width();
    let c = rect.center();
    let tab = Rect::from_min_max(pos2(c.x - w * 0.38, c.y - w * 0.28), pos2(c.x - w * 0.05, c.y - w * 0.14));
    let sheet = Rect::from_center_size(c + vec2(0.0, w * 0.06), vec2(w * 0.78, w * 0.5));
    p.rect_filled(tab, 1.5, tk.tile_gold);
    p.rect_filled(sheet, 1.5, tk.tile_gold);
    p.rect_stroke(sheet, 1.5, edge(tk), StrokeKind::Inside);
}

/// An LED: a domed lamp with two legs.
fn led(p: &Painter, rect: Rect, tk: &Tokens) {
    let w = rect.width();
    let c = rect.center();
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, -w * 0.02), vec2(w * 0.3, w * 0.42)), w * 0.14, Color32::from_rgb(0xd6, 0x45, 0x3a));
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, w * 0.2), vec2(w * 0.4, w * 0.06)), 0.0, Color32::from_rgb(0xd6, 0x45, 0x3a));
    for dx in [-0.07f32, 0.07] {
        p.line_segment([c + vec2(dx * w, w * 0.23), c + vec2(dx * w, w * 0.42)], Stroke::new(1.5, tk.glyph));
    }
    p.circle_filled(c + vec2(-w * 0.05, -w * 0.1), w * 0.04, Color32::WHITE);
}

/// A knob: a dial with its pointer, for the sensors.
fn knob(p: &Painter, rect: Rect, tk: &Tokens) {
    let w = rect.width();
    let c = rect.center();
    p.circle_filled(c, w * 0.34, tk.tile_dark);
    p.circle_stroke(c, w * 0.34, Stroke::new(1.5, tk.glyph));
    p.line_segment([c, c + vec2(w * 0.17, -w * 0.2)], Stroke::new(2.0, Color32::WHITE));
    for k in 0..5 {
        let a = -2.6 + k as f32 * 0.55;
        p.circle_filled(c + vec2(a.cos(), a.sin()) * w * 0.42, w * 0.02, tk.glyph);
    }
}
