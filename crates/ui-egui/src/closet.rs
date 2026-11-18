//! The wiring-closet view: racks, shelves and tables with the devices that stand on them, patch
//! panels, wall mounts and power distribution devices, and every cable drawn from port to port
//! over the front panels. Everything is drawn in code. The view acts only through engine
//! commands (`furniture.*`, `passive.*`, `patch.*`, `power.*`, `device.place`).
//!
//! Drag a device or a passive device onto furniture to put it there (or off it to take it off);
//! drag a furniture's name bar to move it; drag from one port to another to patch them; drag from
//! a power outlet to a device to plug it in. Right-click for the rest.

use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use netlabcraft_core::DeviceId;
use netlabcraft_model::cable::{Cable, Medium};
use netlabcraft_model::furniture::{
    Cell, ClosetLayout, FRAME_FOOT, FRAME_HEAD, FittingKind, FurnitureKind, Item, Placed, Pole, RACK_UNIT_H, TABLE_DEPTH_RANGE, TABLE_WIDTH_RANGE,
};
use netlabcraft_model::{Device, DeviceKind, End, Topology};
use serde_json::{Value, json};

use crate::i18n::{tl, tr, trf};
use crate::icons_ports::{self, Connector};
use crate::theme::{self, Tokens};
use crate::{PacketApp, Tool};

/// What a right-click opened a menu on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Furniture(u32),
    Item(Item),
    Port(Pole),
    Outlet(u32, u32),
    Cable(End),
    Empty(f32, f32),
}

/// What is being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Item(Item),
    Furniture(u32),
    Wire(Pole),
    Plug(u32, u32),
}

/// Something with a name that can be renamed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Thing {
    Furniture(u32),
    Passive(u32),
}

/// The closet view's own state.
#[derive(Debug, Clone, Default)]
pub struct State {
    drag: Option<Drag>,
    panning: bool,
    context: Option<Target>,
    /// The cable picked by a click.
    cable: Option<End>,
    /// The port a click with a cable tool started a cable at.
    pub(crate) wire: Option<Pole>,
    /// Where the sockets were drawn last frame (the tests click on them).
    #[cfg(test)]
    pub(crate) seen_ports: Vec<(Pole, Rect)>,
    /// The Rename dialog: what and the name being typed.
    pub rename: Option<(Thing, String)>,
    /// The Resize dialog: the rack or shelf and the size being typed.
    pub resize: Option<(u32, String)>,
    /// A table's depth being typed in the Resize dialog (the first text is its width).
    pub resize_depth: String,
}

/// A kind of furniture by name (`Table` has its own row: the word also names a data table).
fn furniture_label(kind: FurnitureKind) -> &'static str {
    if kind == FurnitureKind::Table { crate::i18n::tlc("furniture", "Table") } else { tl(kind.label()) }
}

/// The wiring closet being looked at, if any.
pub(crate) fn current(app: &PacketApp) -> Option<u32> {
    let place = app.session.place_view?;
    app.session.topology.place(place).filter(|p| p.kind.holds_things()).map(|p| p.id)
}

/// The strip at the top of the workspace that holds the closet's title; the fit keeps the furniture below it.
pub(crate) const TITLE_BAND: f32 = 40.0;

/// Draws the title of the closet (`Wiring closet: Main Wiring Closet`) at the top left of the workspace, on a
/// plate of its own so it reads over any furniture, and returns the plate.
pub(crate) fn title_plate(p: &Painter, at: Pos2, text: String, tk: &Tokens) -> Rect {
    let galley = p.layout_no_wrap(text, FontId::proportional(16.0), tk.text);
    let plate = Align2::LEFT_TOP.anchor_size(at, galley.size()).expand2(vec2(9.0, 4.0));
    p.rect_filled(plate, 5.0, tk.canvas);
    p.rect_stroke(plate, 5.0, Stroke::new(1.0, tk.chrome_line), StrokeKind::Inside);
    p.galley(plate.min + vec2(9.0, 4.0), galley, tk.text);
    plate
}

/// How many rack units tall the fit frames a rack at most: a taller rack runs off the bottom of the view, and the
/// right or middle button, the wheel and the arrow of the zoom buttons bring the rest in.
pub(crate) const FIT_UNITS: f32 = 8.0;
/// The least the fit zooms out to: below it the jacks stop being readable, so the closet scrolls instead.
pub(crate) const MIN_FIT_ZOOM: f32 = 0.6;
/// How much of the closet has to stay on the screen when it is panned, in pixels.
const KEEP_IN_VIEW: f32 = 80.0;

/// What the fit frames: the furniture (the loose items on the floor are not worth shrinking the closet for),
/// no taller than [`FIT_UNITS`] rack units; a closet with no furniture is framed whole.
pub(crate) fn fit_box(layout: &ClosetLayout) -> Option<Cell> {
    let Some((x0, y0, x1, y1)) = layout.frames.iter().map(|f| f.1).fold(None, |acc: Option<(f32, f32, f32, f32)>, c| {
        Some(match acc {
            None => (c.x, c.y, c.x + c.w, c.y + c.h),
            Some((a, b, d, e)) => (a.min(c.x), b.min(c.y), d.max(c.x + c.w), e.max(c.y + c.h)),
        })
    }) else {
        return layout.bounds();
    };
    let cap = FRAME_HEAD + FIT_UNITS * RACK_UNIT_H + FRAME_FOOT;
    Some(Cell { x: x0, y: y0, w: x1 - x0, h: (y1 - y0).min(cap) })
}

/// Where `b` goes on a workspace of `size` at `zoom`: centred when it fits, else against the top left, always
/// below the title band. Returns the pan.
pub(crate) fn place_box(size: Vec2, b: Cell, zoom: f32) -> Vec2 {
    let free = vec2(size.x, (size.y - TITLE_BAND).max(1.0));
    let x = if b.w * zoom <= free.x { (free.x - b.w * zoom) / 2.0 } else { 24.0 };
    let y = if b.h * zoom <= free.y { TITLE_BAND + (free.y - b.h * zoom) / 2.0 } else { TITLE_BAND + 18.0 };
    vec2(x, y) - vec2(b.x, b.y) * zoom
}

/// The zoom and the pan that frame `b` on a workspace of `size`: as large as the width allows, no larger than the
/// height allows, and never below [`MIN_FIT_ZOOM`] (which scrolls instead of shrinking).
pub(crate) fn fit_view(size: Vec2, b: Cell) -> (f32, Vec2) {
    let free = vec2(size.x, (size.y - TITLE_BAND).max(1.0));
    let zoom = (free.x / (b.w + 48.0)).min(free.y / (b.h + 36.0)).clamp(MIN_FIT_ZOOM, 1.8);
    (zoom, place_box(size, b, zoom))
}

/// Keeps at least [`KEEP_IN_VIEW`] pixels of the closet `b` (in closet units) on a workspace of `size`, so panning
/// can never lose it.
pub(crate) fn clamp_pan(pan: Vec2, zoom: f32, b: Cell, size: Vec2) -> Vec2 {
    let clamp = |p: f32, lo: f32, hi: f32| p.max(lo).min(hi);
    vec2(
        clamp(pan.x, KEEP_IN_VIEW - (b.x + b.w) * zoom, size.x - KEEP_IN_VIEW - b.x * zoom),
        clamp(pan.y, KEEP_IN_VIEW - (b.y + b.h) * zoom, size.y - KEEP_IN_VIEW - b.y * zoom),
    )
}

/// Pans and zooms so the furniture of the closet is in view at a size the jacks can be read at.
pub(crate) fn fit(app: &mut PacketApp, place: u32) {
    let Some(b) = fit_box(&app.session.topology.closet_layout(place)) else { return };
    let size = app.ui.canvas.size;
    if size.x < 1.0 || size.y < 1.0 {
        return;
    }
    (app.ui.zoom, app.ui.pan) = fit_view(size, b);
}

/// Zoom reset: the true size, the furniture's top left in view.
pub(crate) fn reset(app: &mut PacketApp, place: u32) {
    app.ui.zoom = 1.0;
    let size = app.ui.canvas.size;
    app.ui.pan = match fit_box(&app.session.topology.closet_layout(place)) {
        Some(b) if size.x >= 1.0 && size.y >= 1.0 => place_box(size, b, 1.0),
        _ => vec2(0.0, 0.0),
    };
}

/// Brings the pan back if it carried the closet out of view.
pub(crate) fn keep_in_view(app: &mut PacketApp, place: u32) {
    let size = app.ui.canvas.size;
    if let Some(b) = app.session.topology.closet_layout(place).bounds()
        && size.x >= 1.0
        && size.y >= 1.0
    {
        app.ui.pan = clamp_pan(app.ui.pan, app.ui.zoom, b, size);
    }
}

/// Pans and zooms so the whole closet, the loose items too, is in view (the old fit; the tests tap on sockets).
#[cfg(test)]
pub(crate) fn fit_all(app: &mut PacketApp, place: u32) {
    let Some(b) = app.session.topology.closet_layout(place).bounds() else { return };
    let size = app.ui.canvas.size;
    let free = vec2(size.x, (size.y - TITLE_BAND).max(1.0));
    let zoom = (free.x / (b.w + 48.0)).min(free.y / (b.h + 36.0)).clamp(0.25, 1.8);
    app.ui.zoom = zoom;
    app.ui.pan = vec2((free.x - b.w * zoom) / 2.0, TITLE_BAND + (free.y - b.h * zoom) / 2.0) - vec2(b.x, b.y) * zoom;
}

/// A drawn scene: where everything is on the screen, for the pointer.
struct Scene {
    origin: Vec2,
    zoom: f32,
    /// Furniture: its id, the whole frame and its name bar.
    frames: Vec<(u32, Rect, Rect)>,
    /// Items: their cells.
    items: Vec<(Item, Rect)>,
    /// Sockets of passive devices and devices.
    ports: Vec<(Pole, Rect)>,
    /// Outlets of power distribution devices.
    outlets: Vec<(u32, u32, Rect)>,
    /// Cables as drawn: one end of each and its polyline.
    cables: Vec<(End, Vec<Pos2>)>,
}

impl Scene {
    fn screen(&self, x: f32, y: f32) -> Pos2 {
        pos2(x * self.zoom, y * self.zoom) + self.origin
    }

    fn world(&self, p: Pos2) -> (f32, f32) {
        let v = (p - self.origin) / self.zoom;
        (v.x, v.y)
    }

    fn rect(&self, c: &Cell) -> Rect {
        Rect::from_min_size(self.screen(c.x, c.y), vec2(c.w, c.h) * self.zoom)
    }

    fn hit_port(&self, p: Pos2) -> Option<Pole> {
        self.ports.iter().rev().find(|(_, r)| r.expand(2.0).contains(p)).map(|x| x.0)
    }

    fn hit_outlet(&self, p: Pos2) -> Option<(u32, u32)> {
        self.outlets.iter().rev().find(|(_, _, r)| r.expand(2.0).contains(p)).map(|x| (x.0, x.1))
    }

    fn hit_item(&self, p: Pos2) -> Option<Item> {
        self.items.iter().rev().find(|(_, r)| r.contains(p)).map(|x| x.0)
    }

    fn hit_head(&self, p: Pos2) -> Option<u32> {
        self.frames.iter().rev().find(|(_, _, h)| h.contains(p)).map(|x| x.0)
    }

    fn frame_at(&self, p: Pos2) -> Option<u32> {
        self.frames.iter().rev().find(|(_, w, _)| w.contains(p)).map(|x| x.0)
    }

    fn hit_cable(&self, p: Pos2) -> Option<End> {
        self.cables
            .iter()
            .find(|(_, pts)| pts.windows(2).any(|w| matches!(w, [a, b] if crate::physical::segment_distance(p, *a, *b) < 5.0)))
            .map(|c| c.0)
    }
}

// ---------------------------------------------------------------------------------------------
// Colours.

const RACK_BLUE: Color32 = Color32::from_rgb(0x1f, 0x5a, 0xa8);
const RAIL: Color32 = Color32::from_rgb(0x8a, 0x5a, 0x1c);
const SHELF_DARK: Color32 = Color32::from_rgb(0x2f, 0x32, 0x37);
const INVENTORY_GREEN: Color32 = Color32::from_rgb(0x3d, 0x62, 0x4a);
const PEGBOARD_TAN: Color32 = Color32::from_rgb(0xa9, 0x84, 0x56);
const PANEL_GREY: Color32 = Color32::from_rgb(0x6b, 0x72, 0x7c);
/// The colours of cables hung on a pegboard, by peg.
const HUNG: [Color32; 5] = [
    Color32::from_rgb(0x2f, 0x80, 0xed),
    Color32::from_rgb(0xe5, 0x53, 0x3d),
    Color32::from_rgb(0x2f, 0xa8, 0x4f),
    Color32::from_rgb(0xf0, 0xa0, 0x1a),
    Color32::from_rgb(0x8e, 0x44, 0xad),
];
const PLATE: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x24);

/// A cable's colour by kind: straight-through green, cross-over orange, fiber gold, console light
/// blue, serial red, coaxial brown, phone teal, IoT purple.
pub(crate) fn cable_color(kind: Cable, tk: &Tokens) -> Color32 {
    if kind.id().contains("fiber") {
        return Color32::from_rgb(0xd4, 0xa0, 0x17);
    }
    match kind {
        Cable::Straight | Cable::Auto => Color32::from_rgb(0x3f, 0xa3, 0x4d),
        Cable::Cross => Color32::from_rgb(0xe8, 0x76, 0x0c),
        Cable::Console => tk.console_cable,
        Cable::SerialDce | Cable::SerialDte => tk.serial_cable,
        Cable::Coaxial => Color32::from_rgb(0x8d, 0x6e, 0x63),
        Cable::Phone => Color32::from_rgb(0x00, 0x89, 0x7b),
        Cable::IotCustom => tk.iot_cable,
        _ => tk.cable,
    }
}

pub(crate) fn body_color(d: &Device) -> Color32 {
    let m = d.catalogue_model();
    if m.wan() {
        return Color32::from_rgb(0x2f, 0x80, 0x8c);
    }
    match d.kind {
        DeviceKind::Router => Color32::from_rgb(0x2b, 0x30, 0x38),
        DeviceKind::Switch => Color32::from_rgb(0x44, 0x68, 0x96),
        DeviceKind::Hub => Color32::from_rgb(0x4d, 0x82, 0xb3),
        DeviceKind::Phone => Color32::from_rgb(0x40, 0x44, 0x4c),
        _ => Color32::from_rgb(0x9a, 0xa2, 0xad),
    }
}

// ---------------------------------------------------------------------------------------------
// Front panels.

/// `n` socket rectangles in rows inside `area`: at most `max_cols` to a row, row by row.
fn socket_rects(area: Rect, n: usize, zoom: f32, max_cols: usize) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let rows = n.div_ceil(max_cols.max(1));
    let cols = n.div_ceil(rows);
    let cw = area.width() / cols as f32;
    let ch = area.height() / rows as f32;
    let w = (cw * 0.78).clamp(2.0, 16.0 * zoom.max(1.0));
    let h = (ch * 0.62).clamp(2.0, 12.0 * zoom.max(1.0));
    (0..n)
        .map(|i| {
            let (r, c) = (i / cols, i % cols);
            Rect::from_center_size(pos2(area.left() + cw * (c as f32 + 0.5), area.top() + ch * (r as f32 + 0.5)), vec2(w, h))
        })
        .collect()
}

/// What a socket is drawn as when its cell is too small for a connector to be told apart: a block in the
/// connector's colour with a dark opening, and a lit edge when the link is up (or red when it is down).
fn socket_block(p: &Painter, cell: Rect, conn: Connector, up: Option<bool>, tk: &Tokens) {
    let fill = match conn {
        Connector::Rj45 | Connector::Combo => tk.port_rim,
        Connector::Console => tk.console_cable,
        Connector::Serial => tk.serial_cable,
        Connector::Sfp | Connector::Lc => tk.tile_blue,
        Connector::Coax => tk.tile_gold,
        Connector::Usb | Connector::Rs232 | Connector::Rj11 | Connector::Pin | Connector::Radio => tk.tile_grey,
        Connector::Inlet | Connector::Outlet => tk.port_hole,
    };
    let r = cell.expand2(vec2(0.0, cell.height() * 0.3));
    p.rect_filled(r, 0.5, fill);
    p.rect_filled(r.shrink2(vec2(r.width() * 0.22, r.height() * 0.22)), 0.0, tk.port_hole);
    if let Some(up) = up {
        let strip = Rect::from_min_max(r.left_top(), pos2(r.right(), r.top() + (r.height() * 0.22).max(1.0)));
        p.rect_filled(strip, 0.0, if up { tk.link_up } else { tk.link_down });
    }
}

/// Whether a jack cell is too small for its connector to be drawn: it becomes a [`socket_block`].
fn too_small(cell: Rect) -> bool {
    cell.width() < 6.0 || cell.height() < 5.0
}

/// The per-kind detail a device's front panel carries beside its jacks, in the strip of width `extra` left
/// of the power inlet (`right` is what the inlet and the margin take): a router's vents (and a modular
/// router's blank slots), a firewall's grille, an access point's ring, a server's drive bays, a PC's optical
/// drive and power button, a printer's tray, a phone's keypad, a laptop's hinge. Returns `extra`.
fn front_detail(p: &Painter, d: &Device, r: Rect, zoom: f32, right: f32, tk: &Tokens) -> f32 {
    let m = d.catalogue_model();
    let id = m.id;
    let z = zoom;
    let body = body_color(d);
    let dark = theme::shade(body, 0.4);
    let light = theme::shade(body, 1.4);
    let extra = if id.starts_with("Firewall") {
        28.0
    } else if m.wan() {
        0.0
    } else {
        match d.kind {
            DeviceKind::Router if id.contains("-M") => 34.0,
            DeviceKind::Router => 22.0,
            DeviceKind::Server => 46.0,
            DeviceKind::Pc => 36.0,
            DeviceKind::Printer => 30.0,
            DeviceKind::Phone => 24.0,
            DeviceKind::Switch if matches!(id, "AccessPoint" | "LAP" | "Home-Router" | "Home-Gateway") => 24.0,
            _ => 0.0,
        }
    };
    if d.kind == DeviceKind::Laptop {
        p.rect_filled(Rect::from_min_size(r.min + vec2(4.0 * z, 1.5 * z), vec2(r.width() - 8.0 * z, (2.0 * z).max(1.0))), 0.0, light);
        p.circle_filled(pos2(r.center().x, r.top() + 2.5 * z), (0.9 * z).max(0.7), dark);
    }
    if extra <= 0.0 {
        return 0.0;
    }
    let x1 = r.right() - right * 0.9;
    let det = Rect::from_min_max(pos2(x1 - extra * z + 3.0 * z, r.top() + 4.5 * z), pos2(x1 - 2.0 * z, r.bottom() - 3.5 * z));
    if det.width() < 3.0 || det.height() < 3.0 {
        return extra * z;
    }
    let line = Stroke::new((1.0 * z).max(0.8), dark);
    let steps = |n: usize, f: &mut dyn FnMut(usize, f32)| {
        for i in 0..n {
            f(i, (i as f32 + 0.5) / n as f32);
        }
    };
    if id.starts_with("Firewall") {
        // A louvred grille.
        steps(6, &mut |_, f| {
            let x = det.left() + det.width() * f;
            p.line_segment([pos2(x - 1.2 * z, det.bottom()), pos2(x + 1.2 * z, det.top())], line);
        });
        p.rect_filled(Rect::from_min_max(det.left_bottom() - vec2(0.0, 2.0 * z), det.right_bottom()), 0.0, Color32::from_rgb(0xc8, 0x3a, 0x2e));
    } else {
        match d.kind {
            DeviceKind::Router if id.contains("-M") => {
                // Module slots: two blank plates with their screws and one carrying a lit card.
                steps(3, &mut |i, f| {
                    let w = det.width() / 3.0 - 1.5 * z;
                    let slot = Rect::from_min_size(pos2(det.left() + det.width() * (f - 0.5 / 3.0) + 0.5 * z, det.top()), vec2(w, det.height()));
                    p.rect_filled(slot, 1.0, if i == 0 { Color32::from_rgb(0xd9, 0xb2, 0x4c) } else { theme::shade(body, 0.7) });
                    p.rect_stroke(slot, 1.0, line, StrokeKind::Inside);
                    if i > 0 {
                        for y in [slot.top() + 2.0 * z, slot.bottom() - 2.0 * z] {
                            p.circle_filled(pos2(slot.center().x, y), (0.8 * z).max(0.6), light);
                        }
                    }
                });
            }
            DeviceKind::Router => {
                steps(4, &mut |_, f| {
                    let y = det.top() + det.height() * f;
                    p.line_segment([pos2(det.left() + 2.0 * z, y), pos2(det.right() - 2.0 * z, y)], Stroke::new((1.4 * z).max(0.8), dark));
                });
            }
            DeviceKind::Server => {
                // Four drive bays with a handle and a light each.
                steps(4, &mut |_, f| {
                    let w = det.width() / 4.0 - 1.5 * z;
                    let bay = Rect::from_min_size(pos2(det.left() + det.width() * (f - 0.125) + 0.4 * z, det.top()), vec2(w, det.height()));
                    p.rect_filled(bay, 1.0, dark);
                    p.rect_filled(
                        Rect::from_min_size(bay.left_top() + vec2(0.0, bay.height() * 0.55), vec2(bay.width(), (1.6 * z).max(1.0))),
                        0.0,
                        light,
                    );
                    p.circle_filled(bay.center_top() + vec2(0.0, 2.0 * z), (0.8 * z).max(0.6), tk.link_up);
                });
            }
            DeviceKind::Pc => {
                // An optical drive slot over a power button.
                let slot = Rect::from_min_size(det.left_top() + vec2(0.0, 1.0 * z), vec2(det.width(), det.height() * 0.3));
                p.rect_filled(slot, 1.0, dark);
                p.rect_filled(Rect::from_center_size(slot.center(), vec2(slot.width() * 0.8, (1.2 * z).max(1.0))), 0.0, light);
                p.circle_filled(pos2(det.left() + det.width() * 0.25, det.bottom() - det.height() * 0.28), (2.2 * z).max(1.5), dark);
                p.circle_stroke(
                    pos2(det.left() + det.width() * 0.25, det.bottom() - det.height() * 0.28),
                    (2.2 * z).max(1.5),
                    Stroke::new(0.8, light),
                );
                for k in 0..2 {
                    p.rect_filled(
                        Rect::from_center_size(
                            pos2(det.right() - (3.0 + 5.0 * k as f32) * z, det.bottom() - det.height() * 0.28),
                            vec2(3.0 * z, 1.8 * z),
                        ),
                        0.0,
                        dark,
                    );
                }
            }
            DeviceKind::Printer => {
                // A paper tray with a sheet in it.
                p.rect_filled(det, 1.0, dark);
                let sheet = det.shrink2(vec2(2.0 * z, det.height() * 0.2));
                p.rect_filled(sheet, 0.0, Color32::from_gray(0xee));
                p.line_segment([sheet.left_center(), sheet.right_center()], Stroke::new(0.8, Color32::from_gray(0x90)));
            }
            DeviceKind::Phone => {
                // A keypad.
                steps(3, &mut |_, fx| {
                    steps(3, &mut |_, fy| {
                        p.circle_filled(pos2(det.left() + det.width() * fx, det.top() + det.height() * fy), (1.0 * z).max(0.8), light);
                    });
                });
            }
            _ => {
                // An access point: a ring light under its antennas.
                let c = det.center();
                let rad = (det.height() * 0.4).min(det.width() * 0.4);
                p.circle_stroke(c, rad, Stroke::new((1.4 * z).max(1.0), light));
                p.circle_filled(c, rad * 0.4, tk.link_up);
            }
        }
    }
    extra * z
}

/// A device's front panel: its body, a power light, an antenna for a radio, one connector drawn as
/// its real jack for every port that takes a cable (as many as the model has) and a power inlet.
/// Returns the sockets by port.
fn device_panel(p: &Painter, t: &Topology, d: &Device, r: Rect, zoom: f32, tk: &Tokens, rear: bool) -> Vec<(usize, Rect)> {
    let body = body_color(d);
    let edge = theme::shade(body, 0.55);
    let rounding = 3.0 * zoom;
    let radio = (0..d.ports.len()).any(|i| d.port_medium(i) == Some(Medium::Wireless));
    if radio {
        for fx in [0.1, 0.9] {
            let x = r.left() + r.width() * fx;
            p.line_segment([pos2(x, r.top()), pos2(x, r.top() - 9.0 * zoom)], Stroke::new((3.0 * zoom).max(1.5), edge));
        }
    }
    p.rect_filled(r, rounding, body);
    // A lit top edge and a shaded foot give the plate some depth.
    let lit = Stroke::new(1.0, theme::shade(body, 1.3));
    p.line_segment([pos2(r.left() + rounding, r.top() + 1.0), pos2(r.right() - rounding, r.top() + 1.0)], lit);
    p.rect_filled(
        Rect::from_min_max(pos2(r.left() + 1.0, r.bottom() - 2.0 * zoom), pos2(r.right() - 1.0, r.bottom() - 0.5)),
        0.0,
        theme::shade(body, 0.82),
    );
    p.rect_stroke(r, rounding, Stroke::new(1.0, edge), StrokeKind::Inside);
    if d.catalogue_model().id.starts_with("Firewall") {
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), (3.0 * zoom).max(2.0))), 0.0, Color32::from_rgb(0xc8, 0x3a, 0x2e));
    }
    let on = !d.powered_off;
    p.circle_filled(pos2(r.left() + 7.0 * zoom, r.center().y), (2.6 * zoom).max(1.5), if on { tk.link_up } else { Color32::from_gray(0x70) });
    let inlet = icons_ports::has_inlet(d.kind);
    let right = if inlet { 27.0 * zoom } else { 7.0 * zoom };
    let extra = front_detail(p, d, r, zoom, right, tk);
    let mut ports: Vec<(usize, Connector)> = (0..d.ports.len())
        .filter_map(|i| d.ports.get(i).and_then(|pt| icons_ports::connector_for(&pt.name, d.port_medium(i))).map(|c| (i, c)))
        .collect();
    // Seen from behind the jacks are in mirrored order.
    if rear {
        ports.reverse();
    }
    let area = Rect::from_min_max(r.min + vec2(15.0 * zoom, 7.0 * zoom), r.max - vec2(right + extra, 3.0 * zoom));
    if inlet {
        icons_ports::draw(p, Connector::Inlet, icons_ports::inlet_rect(r, zoom), false, tk);
    }
    let max_cols = if ports.len() <= 12 {
        12
    } else if ports.len() <= 28 {
        ports.len().div_ceil(2)
    } else {
        24
    };
    let mut out = Vec::new();
    for ((port, conn), cell) in ports.iter().zip(socket_rects(area, ports.len(), zoom, max_cols)) {
        let end = End { device: d.id, port: *port };
        let up = (on && t.link_at(end).is_some()).then(|| netlabcraft_sim::port_up(t, end));
        if too_small(cell) {
            socket_block(p, cell, *conn, up, tk);
        } else {
            let jack = icons_ports::fit(*conn, cell);
            icons_ports::draw(p, *conn, jack, false, tk);
            if matches!(conn, Connector::Rj45 | Connector::Combo | Connector::Sfp) {
                icons_ports::jack_lights(p, jack, up, zoom, tk);
            }
        }
        out.push((*port, cell));
    }
    if !on {
        p.rect_filled(r, rounding, Color32::from_black_alpha(90));
    }
    out
}

/// A patch panel, a wall mount or a power distribution device: its body and one socket (or
/// outlet) for every port. Returns the sockets in port order.
fn fitting_panel(p: &Painter, kind: FittingKind, ports: u32, on: bool, plugged: &[u32], r: Rect, zoom: f32, tk: &Tokens) -> Vec<Rect> {
    let n = ports as usize;
    let rounding = 3.0 * zoom;
    match kind {
        FittingKind::CopperPatchPanel | FittingKind::FiberPatchPanel => {
            let body = Color32::from_rgb(0xc9, 0xcf, 0xd6);
            p.rect_filled(r, rounding, body);
            p.rect_stroke(r, rounding, Stroke::new(1.0, theme::shade(body, 0.6)), StrokeKind::Inside);
            let area = Rect::from_min_max(r.min + vec2(14.0 * zoom, 8.0 * zoom), r.max - vec2(14.0 * zoom, 5.0 * zoom));
            let rects = socket_rects(area, n, zoom, if kind.is_fiber() { 12 } else { 24 });
            for (i, sr) in rects.iter().enumerate() {
                let conn = if kind.is_fiber() { Connector::Lc } else { Connector::Rj45 };
                if too_small(*sr) {
                    socket_block(p, *sr, conn, None, tk);
                } else {
                    icons_ports::draw(p, conn, icons_ports::fit(conn, *sr), false, tk);
                }
                if zoom >= 0.9 && (i + 1).is_multiple_of(4) {
                    p.text(
                        pos2(sr.center().x, sr.top() - 1.0),
                        Align2::CENTER_BOTTOM,
                        (i + 1).to_string(),
                        FontId::proportional(7.0 * zoom),
                        Color32::from_gray(0x55),
                    );
                }
            }
            for x in [r.left() + 5.0 * zoom, r.right() - 5.0 * zoom] {
                p.circle_filled(pos2(x, r.center().y), (1.8 * zoom).max(1.0), Color32::from_gray(0x7a));
            }
            rects
        }
        FittingKind::CopperWallMount | FittingKind::FiberWallMount => {
            // A wall plate: a square faceplate with its jacks in a row.
            let side = r.height().min(r.width() * 0.5);
            let plate = Rect::from_center_size(r.center(), vec2(side * (0.9 + 0.25 * n as f32).min(2.2), side));
            p.rect_filled(plate, 4.0 * zoom, Color32::from_rgb(0xf1, 0xf1, 0xea));
            p.rect_stroke(plate, 4.0 * zoom, Stroke::new(1.0, Color32::from_gray(0x9a)), StrokeKind::Inside);
            let area = plate.shrink2(vec2(6.0 * zoom, side * 0.2));
            let rects = socket_rects(area, n, zoom * 1.4, 4);
            for sr in &rects {
                let conn = if kind.is_fiber() { Connector::Lc } else { Connector::Rj45 };
                icons_ports::draw(p, conn, icons_ports::fit(conn, *sr), false, tk);
            }
            rects
        }
        FittingKind::PowerStrip => {
            p.rect_filled(r, rounding, Color32::from_rgb(0x4b, 0x4f, 0x56));
            p.rect_stroke(r, rounding, Stroke::new(1.0, Color32::from_gray(0x25)), StrokeKind::Inside);
            // The switch at the left: a rocker with a light.
            let sw = Rect::from_center_size(pos2(r.left() + 13.0 * zoom, r.center().y), vec2(10.0 * zoom, 16.0 * zoom).min(r.size() * 0.8));
            p.rect_filled(sw, 2.0, Color32::from_gray(0x20));
            p.circle_filled(sw.center(), (2.6 * zoom).max(1.5), if on { tk.link_up } else { tk.link_down });
            let area = Rect::from_min_max(r.min + vec2(26.0 * zoom, 6.0 * zoom), r.max - vec2(8.0 * zoom, 4.0 * zoom));
            let rects = socket_rects(area, n, zoom, 16);
            for (i, sr) in rects.iter().enumerate() {
                let used = plugged.contains(&(i as u32));
                icons_ports::draw(p, Connector::Outlet, icons_ports::fit(Connector::Outlet, *sr), used, tk);
            }
            rects
        }
    }
}

/// A small picture of a passive device for the device box.
pub(crate) fn fitting_icon(p: &Painter, r: Rect, kind: FittingKind, tk: &Tokens) {
    let (w, h) = (r.width(), r.height());
    match kind {
        FittingKind::CopperPatchPanel | FittingKind::FiberPatchPanel => {
            let body = Rect::from_center_size(r.center(), vec2(w * 0.96, h * 0.42));
            p.rect_filled(body, 2.0, Color32::from_rgb(0xc9, 0xcf, 0xd6));
            p.rect_stroke(body, 2.0, Stroke::new(1.0, Color32::from_gray(0x7a)), StrokeKind::Inside);
            for i in 0..6 {
                let c = pos2(body.left() + body.width() * (i as f32 + 0.5) / 6.0, body.center().y);
                if kind.is_fiber() {
                    p.circle_filled(c, h * 0.07, Color32::from_rgb(0x1b, 0x5e, 0x20));
                } else {
                    p.rect_filled(Rect::from_center_size(c, vec2(w * 0.09, h * 0.14)), 1.0, Color32::from_rgb(0x20, 0x22, 0x26));
                }
            }
        }
        FittingKind::CopperWallMount | FittingKind::FiberWallMount => {
            let plate = Rect::from_center_size(r.center(), vec2(w * 0.58, h * 0.8));
            p.rect_filled(plate, 3.0, Color32::from_rgb(0xf1, 0xf1, 0xea));
            p.rect_stroke(plate, 3.0, Stroke::new(1.0, tk.muted), StrokeKind::Inside);
            for dy in [-0.17, 0.17] {
                let jack = Rect::from_center_size(plate.center() + vec2(0.0, h * dy), vec2(w * 0.3, h * 0.2));
                p.rect_filled(jack, 1.0, if kind.is_fiber() { Color32::from_rgb(0x1b, 0x5e, 0x20) } else { Color32::from_rgb(0x20, 0x22, 0x26) });
            }
        }
        FittingKind::PowerStrip => {
            let body = Rect::from_center_size(r.center(), vec2(w * 0.96, h * 0.36));
            p.rect_filled(body, 2.0, Color32::from_rgb(0x4b, 0x4f, 0x56));
            for i in 0..5 {
                let c = pos2(body.left() + body.width() * (i as f32 + 1.0) / 6.5, body.center().y);
                p.rect_filled(Rect::from_center_size(c, vec2(w * 0.1, h * 0.18)), 1.0, Color32::from_gray(0x2c));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Furniture.

fn draw_furniture(p: &Painter, kind: FurnitureKind, size: u32, name: &str, whole: Rect, head: Rect, zoom: f32, tk: &Tokens, picked: bool) {
    let z = zoom;
    let frame = match kind {
        FurnitureKind::Shelf => SHELF_DARK,
        FurnitureKind::InventoryShelf => INVENTORY_GREEN,
        FurnitureKind::CablePegboard => PEGBOARD_TAN,
        FurnitureKind::ControlPanel => PANEL_GREY,
        _ => RACK_BLUE,
    };
    p.rect_filled(whole, 3.0 * z, frame);
    let top = head.bottom();
    let foot = FRAME_FOOT * z;
    match kind {
        FurnitureKind::Rack => {
            let inner = Rect::from_min_max(pos2(whole.left() + 28.0 * z, top), pos2(whole.right() - 28.0 * z, whole.bottom() - foot));
            p.rect_filled(inner, 0.0, tk.canvas);
            for side in [inner.left() - 8.0 * z, inner.right()] {
                let rail = Rect::from_min_max(pos2(side, top), pos2(side + 8.0 * z, inner.bottom()));
                p.rect_filled(rail, 0.0, RAIL);
                for u in 0..size {
                    let y = top + (u as f32 + 0.5) * RACK_UNIT_H * z;
                    p.circle_filled(pos2(rail.center().x, y), (1.4 * z).max(0.8), Color32::from_gray(0x20));
                }
            }
            for u in 0..size {
                let y = top + (u as f32 + 0.5) * RACK_UNIT_H * z;
                p.text(
                    pos2(whole.left() + 10.0 * z, y),
                    Align2::CENTER_CENTER,
                    (u + 1).to_string(),
                    FontId::proportional((9.0 * z).clamp(6.0, 14.0)),
                    Color32::from_gray(0xe0),
                );
            }
        }
        FurnitureKind::Shelf => {
            for i in 0..size {
                let y = top + i as f32 * netlabcraft_model::furniture::SHELF_LEVEL_H * z;
                let level = Rect::from_min_size(
                    pos2(whole.left() + 16.0 * z, y),
                    vec2(whole.width() - 32.0 * z, (netlabcraft_model::furniture::SHELF_LEVEL_H - 8.0) * z),
                );
                p.rect_filled(level, 0.0, tk.canvas);
            }
        }
        FurnitureKind::InventoryShelf => {
            // Levels split into labelled bins, where devices wait in storage.
            let bins = netlabcraft_model::furniture::SHELF_CAPACITY;
            for i in 0..size {
                let y = top + i as f32 * netlabcraft_model::furniture::SHELF_LEVEL_H * z;
                let level = Rect::from_min_size(
                    pos2(whole.left() + 16.0 * z, y),
                    vec2(whole.width() - 32.0 * z, (netlabcraft_model::furniture::SHELF_LEVEL_H - 8.0) * z),
                );
                p.rect_filled(level, 0.0, tk.canvas);
                let bin_w = level.width() / bins as f32;
                for b in 0..bins {
                    let x = level.left() + b as f32 * bin_w;
                    if b > 0 {
                        p.line_segment([pos2(x, level.top()), pos2(x, level.bottom())], Stroke::new((2.0 * z).max(1.0), frame));
                    }
                    let tag = Rect::from_min_size(pos2(x + 6.0 * z, level.top() + 4.0 * z), vec2(34.0 * z, 11.0 * z));
                    p.rect_filled(tag, 2.0 * z, Color32::from_gray(0xe8));
                    p.rect_stroke(tag, 2.0 * z, Stroke::new(1.0, Color32::from_gray(0x80)), StrokeKind::Inside);
                }
            }
        }
        FurnitureKind::CablePegboard => {
            // A board with rows of hooks.
            let inner = Rect::from_min_max(pos2(whole.left() + 16.0 * z, top + 4.0 * z), pos2(whole.right() - 16.0 * z, whole.bottom() - foot));
            p.rect_filled(inner, 2.0 * z, theme::shade(PEGBOARD_TAN, 0.9));
            let hook = Stroke::new((2.4 * z).max(1.0), Color32::from_gray(0x4a));
            for r in 0..size {
                for c in 0..netlabcraft_model::furniture::PEGS_PER_ROW {
                    let pos = peg_pos(whole, top, z, r, c);
                    p.circle_filled(pos, (2.6 * z).max(1.0), Color32::from_gray(0x3a));
                    p.line_segment([pos, pos + vec2(0.0, 14.0 * z)], hook);
                    p.line_segment([pos + vec2(0.0, 14.0 * z), pos + vec2(6.0 * z, 8.0 * z)], hook);
                }
            }
        }
        FurnitureKind::ControlPanel => {
            // A wall panel with a DIN rail down the middle and a slot for each module.
            let inner = Rect::from_min_max(pos2(whole.left() + 28.0 * z, top), pos2(whole.right() - 28.0 * z, whole.bottom() - foot));
            p.rect_filled(inner, 0.0, theme::shade(PANEL_GREY, 1.5));
            let rail = Rect::from_min_max(pos2(whole.center().x - 9.0 * z, top), pos2(whole.center().x + 9.0 * z, inner.bottom()));
            p.rect_filled(rail, 0.0, Color32::from_gray(0xb8));
            p.rect_stroke(rail, 0.0, Stroke::new(1.0, Color32::from_gray(0x70)), StrokeKind::Inside);
            for u in 0..size {
                let y = top + (u as f32 + 0.5) * RACK_UNIT_H * z;
                p.rect_filled(
                    Rect::from_center_size(rail.center_top() + vec2(0.0, y - top), vec2(4.0 * z, 14.0 * z)),
                    2.0 * z,
                    Color32::from_gray(0x50),
                );
                p.text(
                    pos2(whole.left() + 10.0 * z, y),
                    Align2::CENTER_CENTER,
                    (u + 1).to_string(),
                    FontId::proportional((9.0 * z).clamp(6.0, 14.0)),
                    Color32::from_gray(0xe0),
                );
            }
        }
        FurnitureKind::Table => {
            let inner = Rect::from_min_max(pos2(whole.left() + 16.0 * z, top + 8.0 * z), pos2(whole.right() - 16.0 * z, whole.bottom() - foot));
            p.rect_filled(inner, 0.0, tk.canvas);
            p.rect_filled(Rect::from_min_max(pos2(whole.left(), top), pos2(whole.right(), top + 8.0 * z)), 0.0, SHELF_DARK);
        }
    }
    p.rect_filled(head, 3.0 * z, theme::shade(frame, 0.8));
    p.text(head.center(), Align2::CENTER_CENTER, name, FontId::proportional((15.0 * z).clamp(9.0, 24.0)), Color32::WHITE);
    if picked {
        p.rect_stroke(whole.expand(3.0), 4.0, Stroke::new(2.0, tk.selection), StrokeKind::Outside);
    }
}

/// Where the hook `col` of row `row` of a pegboard is.
fn peg_pos(whole: Rect, top: f32, z: f32, row: u32, col: u32) -> Pos2 {
    let cols = netlabcraft_model::furniture::PEGS_PER_ROW as f32;
    pos2(whole.left() + (col as f32 + 0.5) * whole.width() / cols, top + (row as f32 + 0.3) * netlabcraft_model::furniture::PEG_ROW_H * z)
}

/// The cables hung on a pegboard: each a loop over its hook, in a colour of its own.
fn draw_hangs(p: &Painter, t: &Topology, furniture: u32, whole: Rect, head: Rect, z: f32) {
    let per = netlabcraft_model::furniture::PEGS_PER_ROW;
    for h in t.hangs_on(furniture) {
        let pos = peg_pos(whole, head.bottom(), z, h.peg / per, h.peg % per);
        let (x, y) = (pos.x, pos.y);
        let mut pts = vec![pos2(x - 8.0 * z, y + 40.0 * z), pos2(x - 8.0 * z, y + 2.0 * z)];
        for i in 1..8 {
            let a = std::f32::consts::PI * (1.0 - i as f32 / 8.0);
            pts.push(pos2(x + 8.0 * z * a.cos(), y + 2.0 * z - 8.0 * z * a.sin()));
        }
        pts.push(pos2(x + 8.0 * z, y + 2.0 * z));
        pts.push(pos2(x + 8.0 * z, y + 40.0 * z));
        p.add(egui::Shape::line(pts, Stroke::new((3.0 * z).max(1.5), HUNG[h.peg as usize % HUNG.len()])));
    }
}

// ---------------------------------------------------------------------------------------------
// Cables.

/// A cubic curve from `a` to `b` that leaves each end towards the other one, sideways when they
/// are far apart across and downwards when they are above each other; `lane` shifts it so
/// cables side by side stay apart.
fn curve(a: Pos2, b: Pos2, lane: f32, zoom: f32) -> Vec<Pos2> {
    let d = b - a;
    let k = (d.length() * 0.5).clamp(30.0 * zoom, 220.0 * zoom);
    let (c1, c2) = if d.x.abs() > d.y.abs() * 1.5 {
        let s = if d.x >= 0.0 { 1.0 } else { -1.0 };
        (a + vec2(s * k, lane), b + vec2(-s * k, lane))
    } else {
        let s = if d.y >= 0.0 { 1.0 } else { -1.0 };
        (a + vec2(lane, s * k), b + vec2(lane, -s * k))
    };
    (0..=28)
        .map(|i| {
            let t = i as f32 / 28.0;
            let u = 1.0 - t;
            let f = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
            pos2(f(a.x, c1.x, c2.x, b.x), f(a.y, c1.y, c2.y, b.y))
        })
        .collect()
}

fn pole_json(t: &Topology, p: Pole) -> Value {
    match p {
        Pole::Panel { fitting, port } => json!({"panel": fitting, "port": port.saturating_add(1)}),
        Pole::Device(e) => {
            let name = t.device(e.device).and_then(|d| d.ports.get(e.port)).map(|p| p.name.clone()).unwrap_or_default();
            json!({"device": e.device.0, "port": name})
        }
    }
}

fn item_json(i: Item) -> Value {
    match i {
        Item::Device(d) => json!({"device": d.0}),
        Item::Fitting(f) => json!({"passive": f}),
    }
}

/// The fill of an item's name plate and the colour of its text: a dark plate in a rack (light text), the
/// canvas elsewhere (the theme's text). The text colour always contrasts with the fill.
fn plate_colors(in_rack: bool, tk: &Tokens) -> (Color32, Color32) {
    let fill = if in_rack { PLATE } else { tk.canvas };
    let text = if in_rack { theme::readable_on(PLATE) } else { tk.text };
    (fill, text)
}

/// An item's name on its plate, centred under `top`. The text is laid out in the colour that reads over
/// the plate: a galley that carries its own colour ignores the one `Painter::galley` is given, which once
/// left dark text on a dark plate.
fn name_plate(p: &Painter, top: Pos2, name: String, size: f32, in_rack: bool, tk: &Tokens) -> Rect {
    let (fill, text) = plate_colors(in_rack, tk);
    let galley = p.layout_no_wrap(name, FontId::proportional(size), text);
    let at = Align2::CENTER_TOP.anchor_size(top, galley.size());
    let plate = at.expand2(vec2(3.0, 0.0));
    p.rect_filled(plate, 3.0, if in_rack { fill } else { fill.gamma_multiply(0.85) });
    p.galley(at.min, galley, text);
    plate
}

// ---------------------------------------------------------------------------------------------
// Drawing the scene.

/// Draws the closet and returns where everything is.
fn build(app: &PacketApp, p: &Painter, origin: Vec2, place: u32) -> Scene {
    let t = &app.session.topology;
    let tk = theme::tokens(app.ui.dark);
    let zoom = app.ui.zoom;
    let layout = t.closet_layout(place);
    let mut sc = Scene { origin, zoom, frames: Vec::new(), items: Vec::new(), ports: Vec::new(), outlets: Vec::new(), cables: Vec::new() };
    let picked = |f: u32| app.ui.physical.closet.rename.as_ref().is_some_and(|(th, _)| *th == Thing::Furniture(f));
    let mut kinds: BTreeMap<u32, FurnitureKind> = BTreeMap::new();
    let mut backs: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for (id, cell) in &layout.frames {
        let Some(f) = t.furniture_piece(*id) else { continue };
        kinds.insert(*id, f.kind);
        let whole = sc.rect(cell);
        let head = Rect::from_min_size(whole.min, vec2(whole.width(), FRAME_HEAD * zoom));
        let shown = if f.back {
            backs.insert(*id);
            trf("{} (rear)", &[&f.name])
        } else {
            f.name.clone()
        };
        draw_furniture(p, f.kind, f.size, &shown, whole, head, zoom, &tk, picked(*id));
        if f.kind == FurnitureKind::CablePegboard {
            draw_hangs(p, t, *id, whole, head, zoom);
        }
        sc.frames.push((*id, whole, head));
    }
    let label_h = 15.0 * zoom;
    // The name plates are drawn last of all but the far-side labels, so a cable runs under a plate and never over one.
    let mut plates: Vec<(Pos2, String, f32, bool)> = Vec::new();
    let mut panels: BTreeMap<DeviceId, Rect> = BTreeMap::new();
    let mut strips: BTreeMap<u32, Vec<Rect>> = BTreeMap::new();
    for placed in &layout.items {
        let Placed { item, cell, furniture } = *placed;
        let c = sc.rect(&cell);
        let in_rack = furniture.and_then(|f| kinds.get(&f)).is_some_and(|k| k.is_rack_like());
        let panel = if in_rack {
            Rect::from_min_max(c.min + vec2(0.0, zoom), pos2(c.right(), c.bottom() - label_h))
        } else {
            Rect::from_min_size(pos2(c.left() + 8.0 * zoom, c.bottom() - label_h - 64.0 * zoom), vec2(c.width() - 16.0 * zoom, 64.0 * zoom))
        };
        if in_rack {
            // Rack ears: the plate the item is screwed to the rails by.
            for x in [panel.left() - 5.0 * zoom, panel.right()] {
                p.rect_filled(Rect::from_min_max(pos2(x, panel.top()), pos2(x + 5.0 * zoom, panel.bottom())), 0.0, Color32::from_gray(0xa8));
                p.circle_filled(pos2(x + 2.5 * zoom, panel.center().y), (1.2 * zoom).max(0.8), Color32::from_gray(0x40));
            }
        }
        let rear = furniture.is_some_and(|f| backs.contains(&f));
        let name = if rear { trf("{} (rear)", &[&t.item_name(item)]) } else { t.item_name(item) };
        match item {
            Item::Device(d) => {
                let Some(dev) = t.device(d) else { continue };
                if app.ui.selected == Some(d) {
                    p.rect_stroke(panel.expand(4.0), 4.0, Stroke::new(2.0, tk.selection), StrokeKind::Outside);
                }
                for (port, sr) in device_panel(p, t, dev, panel, zoom, &tk, rear) {
                    sc.ports.push((Pole::Device(End { device: d, port }), sr));
                }
                // Stored on an inventory shelf: dimmed.
                let stored = furniture.and_then(|f| kinds.get(&f)).is_some_and(|k| *k == FurnitureKind::InventoryShelf);
                if stored {
                    p.rect_filled(panel, 2.0, Color32::from_black_alpha(90));
                } else if dev.powered_off {
                    // Switched off, or cut by a power strip: the front panel is dimmed too.
                    p.rect_filled(panel, 2.0, Color32::from_black_alpha(70));
                }
                panels.insert(d, panel);
            }
            Item::Fitting(i) => {
                let Some(f) = t.fitting(i) else { continue };
                let plugged: Vec<u32> = t.plugs_in(i).iter().map(|g| g.outlet).collect();
                let rects = fitting_panel(p, f.kind, f.ports, f.on, &plugged, panel, zoom, &tk);
                if f.kind == FittingKind::PowerStrip {
                    for (n, r) in rects.iter().enumerate() {
                        sc.outlets.push((i, n as u32, *r));
                    }
                    strips.insert(i, rects);
                } else {
                    for (n, r) in rects.iter().enumerate() {
                        sc.ports.push((Pole::Panel { fitting: i, port: n as u32 }, *r));
                    }
                }
            }
        }
        plates.push((pos2(c.center().x, c.bottom() - label_h + 1.0), name, (12.0 * zoom).clamp(8.0, 20.0), in_rack));
        sc.items.push((item, c));
    }
    // Power cords: from a strip's outlet up to what is plugged into it.
    for g in &t.physical.plugs {
        let (Some(outlet), Some(panel)) = (strips.get(&g.strip).and_then(|v| v.get(g.outlet as usize)), panels.get(&g.device)) else { continue };
        let end = match t.device(g.device) {
            Some(d) if icons_ports::has_inlet(d.kind) => icons_ports::inlet_rect(*panel, zoom).center(),
            _ => pos2(panel.right() - 6.0 * zoom, panel.center().y),
        };
        let pts = curve(outlet.center(), end, 0.0, zoom);
        p.add(egui::Shape::line(pts, Stroke::new((1.6 * zoom).max(1.0), Color32::from_gray(0x16))));
    }
    // Patch wires: cords between panel ports and drops to device ports.
    let pole_pos = |pole: Pole| sc.ports.iter().find(|(q, _)| *q == pole).map(|(_, r)| r.center());
    let mut lanes = 0usize;
    let mut wires: Vec<(Vec<Pos2>, Color32)> = Vec::new();
    for w in &t.physical.patch_wires {
        let (Some(a), Some(b)) = (pole_pos(w.a), pole_pos(w.b)) else { continue };
        let fiber = [w.a, w.b].iter().any(|q| matches!(q, Pole::Panel { fitting, .. } if t.fitting(*fitting).is_some_and(|f| f.kind.is_fiber())));
        let color = if fiber { Color32::from_rgb(0xd4, 0xa0, 0x17) } else { Color32::from_rgb(0x6a, 0x7a, 0x8a) };
        lanes += 1;
        let lane = ((lanes % 5) as f32 - 2.0) * 4.0 * zoom;
        wires.push((curve(a, b, lane, zoom), color));
    }
    for (pts, color) in wires {
        p.add(egui::Shape::line(pts, Stroke::new((2.0 * zoom).max(1.0), color)));
    }
    // Cables, from port to port, in their kind's colour; one that is flagged has a red halo and
    // the picked one (or one of the selected device) a bright one.
    let patched: Vec<(End, End)> = t.patch_paths().iter().map(|x| (x.a, x.b)).collect();
    let mut cables: Vec<(End, End, Cable, bool)> = Vec::new();
    for l in &t.links {
        if patched.iter().any(|(a, b)| (*a == l.a && *b == l.b) || (*a == l.b && *b == l.a)) {
            continue;
        }
        cables.push((l.a, l.b, t.resolved_cable(l), netlabcraft_sim::cable_problem(t, l.a).is_some()));
    }
    for c in &t.consoles {
        let port_of =
            |id: DeviceId| t.device(id).and_then(|d| d.console_port_name().and_then(|n| d.port_index(n)).map(|port| End { device: id, port }));
        if let (Some(a), Some(b)) = (port_of(c.pc), port_of(c.device)) {
            cables.push((a, b, Cable::Console, false));
        }
    }
    let end_pos = |e: End| sc.ports.iter().find(|(q, _)| *q == Pole::Device(e)).map(|(_, r)| r.center());
    let mut drawn: Vec<(End, Vec<Pos2>)> = Vec::new();
    let mut stubs: Vec<(Pos2, String, Color32)> = Vec::new();
    for (n, (a, b, kind, problem)) in cables.iter().enumerate() {
        let color = cable_color(*kind, &tk);
        let width = (2.4 * zoom).clamp(1.4, 4.0);
        let hot = app.ui.physical.closet.cable.is_some_and(|c| c == *a || c == *b);
        let near = app.ui.selected.is_some_and(|d| d == a.device || d == b.device);
        match (end_pos(*a), end_pos(*b)) {
            (Some(pa), Some(pb)) => {
                let lane = ((n % 7) as f32 - 3.0) * 5.0 * zoom;
                let pts = curve(pa, pb, lane, zoom);
                if hot || near {
                    p.add(egui::Shape::line(pts.clone(), Stroke::new(width + 5.0, tk.selection.gamma_multiply(if hot { 0.7 } else { 0.35 }))));
                }
                if *problem {
                    p.add(egui::Shape::line(pts.clone(), Stroke::new(width + 5.0, tk.link_down.gamma_multiply(0.55))));
                }
                p.add(egui::Shape::line(pts.clone(), Stroke::new(width, color)));
                for q in [pa, pb] {
                    p.circle_filled(q, width * 0.9, color);
                }
                drawn.push((*a, pts));
            }
            (one, other) => {
                // A cable that leaves this closet: a stub up from its port, labelled with the far side.
                let (here, far) = if one.is_some() { (one, *b) } else { (other, *a) };
                let Some(pt) = here else { continue };
                let name = t.device(far.device).map(|d| d.display_name().to_string()).unwrap_or_default();
                let place_name = t.home_of(far.device).and_then(|h| t.place(h.place)).map(|x| tr(&x.name).to_string()).unwrap_or_default();
                let tip = pt - vec2(0.0, 34.0 * zoom);
                let pts = vec![pt, tip];
                if *problem {
                    p.add(egui::Shape::line(pts.clone(), Stroke::new(width + 5.0, tk.link_down.gamma_multiply(0.55))));
                }
                p.add(egui::Shape::line(pts.clone(), Stroke::new(width, color)));
                stubs.push((tip, format!("{name} ({place_name})"), color));
                let own = if one.is_some() { *a } else { *b };
                drawn.push((own, pts));
            }
        }
    }
    for (top, name, size, in_rack) in plates {
        name_plate(p, top, name, size, in_rack, &tk);
    }
    for (tip, text, color) in stubs {
        let galley = p.layout_no_wrap(text, FontId::proportional((11.0 * zoom).clamp(8.0, 16.0)), tk.text);
        let at = Align2::CENTER_BOTTOM.anchor_size(tip, galley.size());
        let plate = at.expand2(vec2(3.0, 1.0));
        p.rect_filled(plate, 3.0, tk.canvas.gamma_multiply(0.92));
        p.rect_stroke(plate, 3.0, Stroke::new(1.2, color), StrokeKind::Inside);
        p.galley(at.min, galley, tk.text);
    }
    sc.cables = drawn;
    sc
}

/// Marks the slot (rack units, shelf level or the table top) something dropped there would take.
fn slot_highlight(p: &Painter, sc: &Scene, f: &netlabcraft_model::furniture::Furniture, slot: u32, units: u32, tk: &Tokens) {
    let (w, h) = f.dims();
    let (x, y, ww, hh) = match f.kind {
        FurnitureKind::Rack | FurnitureKind::ControlPanel => {
            (f.x + 28.0, f.y + FRAME_HEAD + slot as f32 * RACK_UNIT_H, w - 56.0, units as f32 * RACK_UNIT_H)
        }
        FurnitureKind::Shelf | FurnitureKind::InventoryShelf => (
            f.x + 16.0,
            f.y + FRAME_HEAD + slot as f32 * netlabcraft_model::furniture::SHELF_LEVEL_H,
            w - 32.0,
            netlabcraft_model::furniture::SHELF_LEVEL_H - 8.0,
        ),
        FurnitureKind::Table | FurnitureKind::CablePegboard => (f.x + 16.0, f.y + FRAME_HEAD, w - 32.0, h - FRAME_HEAD - FRAME_FOOT),
    };
    let r = sc.rect(&Cell { x, y, w: ww, h: hh });
    p.rect_filled(r, 2.0, tk.selection.gamma_multiply(0.18));
    p.rect_stroke(r, 2.0, Stroke::new(1.5, tk.selection), StrokeKind::Inside);
}

/// A cable being made by clicks: the first port lit, a line to the pointer in the cable's colour,
/// and the ports the cable cannot join to it dimmed.
fn draw_cable_feedback(app: &PacketApp, p: &Painter, sc: &Scene, pointer: Option<Pos2>) {
    let (Tool::Cable(kind), Some(first)) = (app.ui.tool, app.ui.physical.closet.wire) else { return };
    let tk = theme::tokens(app.ui.dark);
    let t = &app.session.topology;
    if let Some((_, r)) = sc.ports.iter().find(|(q, _)| *q == first) {
        p.rect_stroke(r.expand(2.0), 2.0, Stroke::new(2.0, tk.selection), StrokeKind::Outside);
        if let Some(at) = pointer {
            p.add(egui::Shape::line(curve(r.center(), at, 0.0, sc.zoom), Stroke::new(2.5, cable_color(kind, &tk))));
        }
    }
    for (pole, r) in &sc.ports {
        if *pole != first && port_refusal(t, kind, first, *pole).is_some() {
            p.rect_filled(*r, 2.0, tk.ground.gamma_multiply(0.6));
        }
    }
}

/// Why a cable of `kind` cannot go between these two ports, if it cannot (a busy device port, or
/// the cable not fitting the two). Panel ports are left to the patch wire command.
fn port_refusal(t: &Topology, kind: Cable, a: Pole, b: Pole) -> Option<String> {
    if let Some(why) = busy(t, a).or_else(|| busy(t, b)) {
        return Some(why);
    }
    let (Pole::Device(ea), Pole::Device(eb)) = (a, b) else { return None };
    if ea.device == eb.device {
        return Some(tl("A cable joins two different devices").into());
    }
    let generic = !matches!(kind, Cable::Console | Cable::Usb | Cable::Octal | Cable::IotCustom);
    if generic && let Some(why) = t.cable_misfit(kind, ea, eb) {
        return Some(crate::i18n::tr_any(&why));
    }
    None
}

/// Why a device port cannot start a cable: it already has one.
fn busy(t: &Topology, pole: Pole) -> Option<String> {
    let Pole::Device(e) = pole else { return None };
    let peer = t.peer(e)?;
    Some(trf("{} is already cabled to {}", &[&t.end_label(e), &t.end_label(peer)]))
}

/// A click on a port with a cable tool: the first one starts the cable, the second finishes it
/// (a device-to-device cable through `link.add`, anything with a patch panel port through
/// `patch.wire`, as a drag does).
fn cable_click(app: &mut PacketApp, sc: &Scene, kind: Cable, at: Pos2) {
    let Some(pole) = sc.hit_port(at) else { return };
    let Some(first) = app.ui.physical.closet.wire else {
        match busy(&app.session.topology, pole) {
            Some(why) => app.ui.status = Some((why, true)),
            None => app.ui.physical.closet.wire = Some(pole),
        }
        return;
    };
    if first == pole {
        app.ui.physical.closet.wire = None;
        return;
    }
    let t = &app.session.topology;
    if let Some(why) = port_refusal(t, kind, first, pole) {
        app.ui.status = Some((why, true));
        return;
    }
    let made = match (first, pole) {
        (Pole::Device(a), Pole::Device(b)) => {
            let name = |e: End| t.device(e.device).and_then(|d| d.ports.get(e.port)).map(|p| p.name.clone()).unwrap_or_default();
            let params = json!({"a": a.device.0, "a_port": name(a), "b": b.device.0, "b_port": name(b), "cable": kind.id()});
            app.run("link.add", params)
        }
        _ => {
            let params = json!({"a": pole_json(t, first), "b": pole_json(t, pole)});
            app.run("patch.wire", params)
        }
    };
    // The tool stays picked for the next cable; a refused pair leaves the first port lit.
    if made.is_some() {
        app.ui.physical.closet.wire = None;
    }
}

/// What is being dragged, drawn over the scene: the slot an item would take, or the rubber band.
fn draw_feedback(app: &PacketApp, p: &Painter, sc: &Scene, pointer: Option<Pos2>) {
    draw_cable_feedback(app, p, sc, pointer);
    // A tile from the device box over a rack, shelf or table: the slot it would take.
    if app.ui.tile_drag.is_some()
        && let Some(at) = pointer
        && let Some(f) = sc.frame_at(at).and_then(|id| app.session.topology.furniture_piece(id))
        && let Some(slot) = f.slot_at(sc.world(at).1)
    {
        slot_highlight(p, sc, f, slot, 1, &theme::tokens(app.ui.dark));
    }
    let (Some(drag), Some(at)) = (app.ui.physical.closet.drag, pointer) else { return };
    let tk = theme::tokens(app.ui.dark);
    let t = &app.session.topology;
    match drag {
        Drag::Item(item) => {
            let Some(f) = sc.frame_at(at).and_then(|id| t.furniture_piece(id)) else { return };
            let (_, wy) = sc.world(at);
            let Some(slot) = f.slot_at(wy) else { return };
            let units = t.mount_of(item).map_or(1, |m| m.units);
            slot_highlight(p, sc, f, slot, units, &tk);
        }
        Drag::Wire(from) => {
            if let Some((_, r)) = sc.ports.iter().find(|(q, _)| *q == from) {
                p.add(egui::Shape::line(curve(r.center(), at, 0.0, sc.zoom), Stroke::new(2.0, tk.selection)));
            }
        }
        Drag::Plug(strip, outlet) => {
            if let Some((_, _, r)) = sc.outlets.iter().find(|(s, o, _)| *s == strip && *o == outlet) {
                p.add(egui::Shape::line(curve(r.center(), at, 0.0, sc.zoom), Stroke::new(2.0, tk.selection)));
            }
        }
        Drag::Furniture(_) => {}
    }
}

// ---------------------------------------------------------------------------------------------
// The view.

/// Draws the wiring closet and handles the pointer. `rect` is the workspace, `resp` its response.
pub(crate) fn show(app: &mut PacketApp, ui: &mut Ui, resp: &egui::Response, painter: &Painter, rect: Rect, place: u32) {
    let origin = rect.min.to_vec2() + app.ui.pan;
    let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
    let sc = build(app, painter, origin, place);
    #[cfg(test)]
    {
        app.ui.physical.closet.seen_ports = sc.ports.clone();
    }
    // The pointer while a tile is dragged in from the device box (the canvas is not hovered then).
    let feedback_at = if app.ui.tile_drag.is_some() { ui.input(|i| i.pointer.latest_pos()) } else { pointer };
    draw_feedback(app, painter, &sc, feedback_at);
    // A tile released here goes where it was dropped: onto the slot there, else the floor.
    if let Some((id, at)) = app.ui.tile_drop.take() {
        place_tool(app, &sc, id, at, place);
    }
    app.ui.pointer = resp.hover_pos().map(|p| sc.world(p));
    accessible_nodes(app, ui, &sc, rect);
    interact(app, ui, resp, &sc, place);
}

/// A screen-reader node for every piece of furniture and every item, like the logical workspace's.
/// The nodes take focus (Tab, the screen reader's cursor) but no pointer events, so the drags and
/// menus of the view are not in their way; Enter on a device in focus opens it.
fn accessible_nodes(app: &mut PacketApp, ui: &mut Ui, sc: &Scene, canvas: Rect) {
    let t = &app.session.topology;
    let mut nodes: Vec<(egui::Id, Rect, String, bool, Option<Item>)> = Vec::new();
    for (id, _, head) in &sc.frames {
        let Some(f) = t.furniture_piece(*id) else { continue };
        let label = match f.kind {
            FurnitureKind::Rack => trf("{}, {} units", &[&f.name, &f.size]),
            FurnitureKind::Shelf | FurnitureKind::InventoryShelf => trf("{}, {} levels", &[&f.name, &f.size]),
            FurnitureKind::ControlPanel => trf("{}, {} DIN slots", &[&f.name, &f.size]),
            FurnitureKind::CablePegboard => trf("{}, {} rows of hooks", &[&f.name, &f.size]),
            FurnitureKind::Table => trf("{}, {} by {} cells", &[&f.name, &f.table_cols(), &f.table_rows()]),
        };
        let label = if f.back { trf("{}, rear view", &[&label]) } else { label };
        nodes.push((egui::Id::unique(("closet-furniture", *id)), *head, label, false, None));
    }
    for (item, cell) in &sc.items {
        let name = t.item_name(*item);
        let label = match t.mount_of(*item).and_then(|m| t.furniture_piece(m.furniture)) {
            Some(f) => trf("{}, on {}", &[&name, &f.name]),
            None => name,
        };
        let (key, selected) = match item {
            Item::Device(d) => ((0u8, d.0), app.ui.selected == Some(*d)),
            Item::Fitting(i) => ((1u8, *i), false),
        };
        nodes.push((egui::Id::unique(("closet-item", key.0, key.1)), *cell, label, selected, Some(*item)));
    }
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let (mut focus, mut open) = (None, None);
    for (id, rect, label, selected, item) in &nodes {
        let r = ui.interact(rect.intersect(canvas), *id, Sense::focusable_noninteractive());
        r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, *selected, label.clone()));
        if r.gained_focus() {
            focus = *item;
        }
        if r.has_focus() && enter {
            open = *item;
        }
    }
    // Every port is a node of its own: Tab to a port and Enter starts a cable there, Tab to
    // another and Enter finishes it (the same two steps as the two clicks of the cable tool).
    let mut start = None;
    for (pole, rect) in &sc.ports {
        let visible = rect.intersect(canvas);
        if visible.width() <= 0.0 || visible.height() <= 0.0 {
            continue;
        }
        let text = match pole {
            Pole::Device(e) => t.end_label(*e),
            Pole::Panel { fitting, port } => format!("{} {}", t.fitting(*fitting).map_or(String::new(), |f| f.name.clone()), port.saturating_add(1)),
        };
        let label = trf("Port, {}", &[&text]);
        let id = egui::Id::unique(("closet-port", format!("{pole:?}")));
        let r = ui.interact(visible, id, Sense::focusable_noninteractive());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
        if r.has_focus() && enter {
            start = Some(rect.center());
        }
    }
    if let Some(at) = start {
        let kind = if let Tool::Cable(k) = app.ui.tool { k } else { Cable::Auto };
        cable_click(app, sc, kind, at);
    }
    // Moving focus to a device selects it, as on the logical workspace.
    if let Some(Item::Device(d)) = focus {
        app.ui.selected = Some(d);
    }
    if let Some(Item::Device(d)) = open {
        crate::open_console(app, d);
    }
}

fn interact(app: &mut PacketApp, ui: &Ui, resp: &egui::Response, sc: &Scene, place: u32) {
    let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
    if resp.drag_started() {
        // The right or middle button pans; the left button moves what it grabbed, or does nothing on empty ground.
        let pan_button = ui.input(|i| i.pointer.middle_down() || i.pointer.secondary_down());
        app.ui.physical.closet.panning = pan_button;
        let select_tool = app.ui.tool == Tool::Select && !pan_button;
        if select_tool && let Some(p) = pointer {
            let drag = if let Some(pole) = sc.hit_port(p) {
                Some(Drag::Wire(pole))
            } else if let Some((s, o)) = sc.hit_outlet(p) {
                Some(Drag::Plug(s, o))
            } else if let Some(item) = sc.hit_item(p) {
                Some(Drag::Item(item))
            } else {
                sc.hit_head(p).map(Drag::Furniture)
            };
            if let Some(d) = drag {
                if let Drag::Furniture(_) = d {
                    app.run("edit.checkpoint", json!({}));
                }
                if let Drag::Item(Item::Device(dev)) = d {
                    app.ui.selected = Some(dev);
                }
                app.ui.physical.closet.drag = Some(d);
                app.ui.physical.closet.panning = false;
            }
        }
    }
    if resp.dragged() {
        match app.ui.physical.closet.drag {
            Some(Drag::Furniture(id)) => {
                let delta = resp.drag_delta() / sc.zoom;
                if let Some(f) = app.session.topology.furniture_piece(id).cloned() {
                    app.run("furniture.move", json!({"furniture": id, "x": f.x + delta.x, "y": f.y + delta.y, "transient": true}));
                }
            }
            Some(_) => {}
            None if app.ui.physical.closet.panning => app.ui.pan += resp.drag_delta(),
            None => {}
        }
    }
    if resp.drag_stopped() {
        let at = pointer.or_else(|| ui.input(|i| i.pointer.latest_pos()));
        let drag = app.ui.physical.closet.drag.take();
        app.ui.physical.closet.panning = false;
        if let (Some(drag), Some(at)) = (drag, at) {
            drop_on(app, sc, drag, at);
        }
    }
    if resp.double_clicked()
        && let Some(p) = pointer
        && let Some(Item::Device(d)) = sc.hit_item(p)
    {
        crate::open_console(app, d);
        return;
    }
    // Right-click drops a cable being made; the tool stays picked.
    let cancelled = resp.secondary_clicked() && app.ui.physical.closet.wire.take().is_some();
    if cancelled {
        app.ui.physical.closet.context = None;
    }
    if resp.secondary_clicked()
        && !cancelled
        && let Some(p) = pointer
    {
        let (wx, wy) = sc.world(p);
        app.ui.physical.closet.context = Some(if let Some(pole) = sc.hit_port(p) {
            Target::Port(pole)
        } else if let Some((s, o)) = sc.hit_outlet(p) {
            Target::Outlet(s, o)
        } else if let Some(item) = sc.hit_item(p) {
            if let Item::Device(d) = item {
                app.ui.selected = Some(d);
            }
            Target::Item(item)
        } else if let Some(f) = sc.hit_head(p) {
            Target::Furniture(f)
        } else if let Some(e) = sc.hit_cable(p) {
            Target::Cable(e)
        } else {
            Target::Empty(wx, wy)
        });
    }
    if let Some(target) = app.ui.physical.closet.context {
        resp.context_menu(|ui| context_menu(app, ui, target, place));
    }
    if resp.clicked()
        && let Some(p) = pointer
    {
        let hit_item = sc.hit_item(p);
        if let Tool::Cable(kind) = app.ui.tool {
            cable_click(app, sc, kind, p);
            return;
        }
        if let Tool::Place(id) = app.ui.tool {
            if hit_item.is_none() {
                place_tool(app, sc, id, p, place);
            }
            return;
        }
        app.ui.physical.closet.cable = if hit_item.is_none() && sc.hit_port(p).is_none() { sc.hit_cable(p) } else { None };
        app.ui.selected = match hit_item {
            Some(Item::Device(d)) => Some(d),
            _ => None,
        };
        match app.ui.physical.closet.cable {
            Some(end) => crate::cable_window::select(app, end),
            None => app.ui.selected_cable = None,
        }
    }
}

/// A drag ended at `at`.
fn drop_on(app: &mut PacketApp, sc: &Scene, drag: Drag, at: Pos2) {
    match drag {
        Drag::Item(item) => {
            let t = &app.session.topology;
            let target = sc.frame_at(at).and_then(|id| t.furniture_piece(id).map(|f| (f.id, f.slot_at(sc.world(at).1))));
            match target {
                Some((furniture, slot)) => {
                    let now = t.mount_of(item);
                    if now.is_some_and(|m| m.furniture == furniture && Some(m.slot) == slot) {
                        return;
                    }
                    let units = now.map_or(1, |m| m.units);
                    let mut p = item_json(item);
                    p["furniture"] = json!(furniture);
                    p["units"] = json!(units);
                    if let Some(s) = slot {
                        p["slot"] = json!(s + 1);
                    }
                    app.run("furniture.mount", p);
                }
                None => {
                    if t.mount_of(item).is_some() {
                        app.run("furniture.unmount", item_json(item));
                    }
                }
            }
        }
        Drag::Wire(from) => {
            if let Some(to) = sc.hit_port(at).filter(|q| *q != from) {
                let t = &app.session.topology;
                let params = json!({"a": pole_json(t, from), "b": pole_json(t, to)});
                app.run("patch.wire", params);
            }
        }
        Drag::Plug(strip, outlet) => {
            if let Some(Item::Device(d)) = sc.hit_item(at) {
                app.run("power.plug", json!({"device": d.0, "strip": strip, "outlet": outlet.saturating_add(1)}));
            }
        }
        Drag::Furniture(_) => {}
    }
}

/// A click with a palette tile picked: a passive device, or a device, goes where it was clicked
/// (on the furniture there, else on the floor).
fn place_tool(app: &mut PacketApp, sc: &Scene, id: &'static str, at: Pos2, place: u32) {
    let (wx, wy) = sc.world(at);
    let t = &app.session.topology;
    let spot = sc.frame_at(at).and_then(|f| t.furniture_piece(f).map(|x| (x.id, x.slot_at(wy))));
    if FittingKind::from_id(id).is_some() {
        let mut p = json!({"kind": id, "place": place});
        if let Some((f, slot)) = spot {
            p["furniture"] = json!(f);
            if let Some(s) = slot {
                p["slot"] = json!(s + 1);
            }
        }
        app.run("passive.add", p);
        return;
    }
    if let Some(v) = app.run("device.add", json!({"model": id, "x": wx.round(), "y": wy.round()})) {
        let dev = v.get("id").and_then(Value::as_u64).and_then(|i| u32::try_from(i).ok());
        app.ui.selected = dev.map(DeviceId);
        if let (Some(d), Some((f, slot))) = (dev, spot) {
            let mut p = json!({"device": d, "furniture": f});
            if let Some(s) = slot {
                p["slot"] = json!(s + 1);
            }
            app.run("furniture.mount", p);
        }
    }
}

fn context_menu(app: &mut PacketApp, ui: &mut Ui, target: Target, place: u32) {
    let t = &app.session.topology;
    let furniture: Vec<(u32, String)> = t.furniture_in(place).iter().map(|f| (f.id, f.name.clone())).collect();
    let strips: Vec<(u32, String)> =
        t.fittings_in(place).iter().filter(|f| f.kind == FittingKind::PowerStrip).map(|f| (f.id, f.name.clone())).collect();
    match target {
        Target::Furniture(id) => {
            let Some(f) = t.furniture_piece(id) else {
                ui.close();
                return;
            };
            ui.label(egui::RichText::new(&f.name).strong());
            if ui.button(tl("Rename…")).clicked() {
                app.ui.physical.closet.rename = Some((Thing::Furniture(id), f.name.clone()));
                ui.close();
            }
            if ui.button(tl("Resize…")).clicked() {
                let table = f.kind == FurnitureKind::Table;
                app.ui.physical.closet.resize = Some((id, if table { f.table_cols() } else { f.size }.to_string()));
                app.ui.physical.closet.resize_depth = f.table_rows().to_string();
                ui.close();
            }
            if f.kind == FurnitureKind::Rack && ui.button(if f.back { tl("Turn to the Front") } else { tl("Turn to the Back") }).clicked() {
                app.run("furniture.turn", json!({"furniture": id}));
                ui.close();
            }
            if ui.button(tl("Remove")).clicked() {
                app.run("furniture.remove", json!({"furniture": id}));
                ui.close();
            }
        }
        Target::Item(item) => {
            let name = t.item_name(item);
            let mounted = t.mount_of(item).is_some();
            let strip_kind = matches!(item, Item::Fitting(i) if t.fitting(i).is_some_and(|f| f.kind == FittingKind::PowerStrip));
            ui.label(egui::RichText::new(&name).strong());
            match item {
                Item::Device(d) => {
                    if ui.button(tl("Open Device")).clicked() {
                        crate::open_console(app, d);
                        ui.close();
                    }
                }
                Item::Fitting(i) => {
                    if ui.button(tl("Rename…")).clicked() {
                        app.ui.physical.closet.rename = Some((Thing::Passive(i), name.clone()));
                        ui.close();
                    }
                    if strip_kind {
                        if let Some(f) = t.fitting(i) {
                            ui.label(crate::i18n::trf("Load {} of {} W", &[&t.strip_load(i), &f.rating_w]));
                            if f.tripped {
                                ui.label(egui::RichText::new(tl("Breaker tripped")).strong());
                            }
                        }
                        let on = t.fitting(i).is_some_and(|f| f.on);
                        if ui.button(if on { tl("Switch Off") } else { tl("Switch On") }).clicked() {
                            app.run("power.strip", json!({"strip": i, "on": !on}));
                            ui.close();
                        }
                    }
                }
            }
            ui.menu_button(tl("Move to"), |ui| {
                for (id, n) in &furniture {
                    if ui.button(n).clicked() {
                        let mut p = item_json(item);
                        p["furniture"] = json!(id);
                        app.run("furniture.mount", p);
                        ui.close();
                    }
                }
            });
            if mounted && ui.button(tl("Take off the furniture")).clicked() {
                app.run("furniture.unmount", item_json(item));
                ui.close();
            }
            if let Item::Device(d) = item {
                ui.menu_button(tl("Plug into"), |ui| {
                    for (id, n) in &strips {
                        if ui.button(n).clicked() {
                            app.run("power.plug", json!({"device": d.0, "strip": id}));
                            ui.close();
                        }
                    }
                });
                if app.session.topology.plug_of(d).is_some() && ui.button(tl("Unplug")).clicked() {
                    app.run("power.unplug", json!({"device": d.0}));
                    ui.close();
                }
            } else if let Item::Fitting(i) = item
                && ui.button(tl("Remove")).clicked()
            {
                app.run("passive.remove", json!({"passive": i}));
                ui.close();
            }
        }
        Target::Port(pole) => {
            if t.patch_wires_at(pole).is_empty() {
                ui.label(tl("Drag from this port to another to wire them."));
            } else if ui.button(tl("Remove Patch Wires")).clicked() {
                let params = json!({"a": pole_json(t, pole)});
                app.run("patch.unwire", params);
                ui.close();
            }
        }
        Target::Outlet(strip, outlet) => {
            match t.plugs_in(strip).iter().find(|g| g.outlet == outlet).map(|g| g.device) {
                Some(d) => {
                    if ui.button(tl("Unplug")).clicked() {
                        app.run("power.unplug", json!({"device": d.0}));
                        ui.close();
                    }
                }
                None => {
                    ui.label(tl("Drag from this outlet to a device to plug it in."));
                }
            };
        }
        Target::Cable(end) => {
            let Some(l) = t.link_at(end).copied() else {
                ui.close();
                return;
            };
            ui.label(egui::RichText::new(format!("{} - {}", t.end_label(l.a), t.end_label(l.b))).strong());
            if let Some(why) = netlabcraft_sim::cable_problem(t, l.a) {
                ui.label(egui::RichText::new(crate::i18n::tr_any(&why)).color(theme::tokens(app.ui.dark).error));
            }
            let boards: Vec<(u32, String)> =
                t.furniture_in(place).iter().filter(|f| f.kind == FurnitureKind::CablePegboard).map(|f| (f.id, f.name.clone())).collect();
            crate::cable_window::menu_items(app, ui, netlabcraft_model::cable_ref::CableRef::Link(l.a), true);
            if !boards.is_empty() {
                ui.menu_button(tl("Hang on a Peg"), |ui| {
                    for (id, n) in &boards {
                        if ui.button(n).clicked() {
                            let mut params = pole_json(&app.session.topology, Pole::Device(l.a));
                            params["furniture"] = json!(id);
                            app.run("furniture.hang", params);
                            ui.close();
                        }
                    }
                });
            }
            if app.session.topology.hang_of(l.a).is_some() && ui.button(tl("Take off the Peg")).clicked() {
                let params = pole_json(&app.session.topology, Pole::Device(l.a));
                app.run("furniture.unhang", params);
                ui.close();
            }
        }
        Target::Empty(x, y) => {
            ui.menu_button(tl("Loose Devices"), |ui| {
                let list = app.session.topology.floor_is_list(place);
                for (label, layout, on) in [(tl("As a Grid"), "grid", !list), (tl("As a List"), "list", list)] {
                    if ui.selectable_label(on, label).clicked() {
                        app.run("closet.floor", json!({"place": place, "layout": layout}));
                        ui.close();
                    }
                }
            });
            ui.menu_button(tl("Add to this closet"), |ui| {
                for kind in FurnitureKind::ALL {
                    if ui.button(furniture_label(kind)).clicked() {
                        app.run("furniture.add", json!({"kind": kind.id(), "place": place, "x": x.round(), "y": y.round()}));
                        ui.close();
                    }
                }
                ui.separator();
                for kind in FittingKind::ALL {
                    if ui.button(tl(kind.label())).clicked() {
                        app.run("passive.add", json!({"kind": kind.id(), "place": place}));
                        ui.close();
                    }
                }
            });
        }
    }
}

/// The Resize dialog of a rack (units), a shelf (levels) or a table (width and depth, in cells).
fn resize_window(app: &mut PacketApp, ctx: &egui::Context) {
    let Some((id, mut text)) = app.ui.physical.closet.resize.clone() else { return };
    let mut depth = app.ui.physical.closet.resize_depth.clone();
    let Some(f) = app.session.topology.furniture_piece(id) else {
        app.ui.physical.closet.resize = None;
        return;
    };
    let (kind, name) = (f.kind, f.name.clone());
    let (lo, hi) = kind.size_range();
    let (mut open, mut ok) = (true, false);
    egui::Window::new(tl("Resize")).id(egui::Id::unique("closet_resize")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label(egui::RichText::new(&name).strong());
        let l = ui.label(match kind {
            FurnitureKind::Table => trf("Width in cells ({} to {}):", &[&TABLE_WIDTH_RANGE.0, &TABLE_WIDTH_RANGE.1]),
            FurnitureKind::Rack => trf("Units ({} to {}):", &[&lo, &hi]),
            _ => trf("Levels ({} to {}):", &[&lo, &hi]),
        });
        let r = ui.add(egui::TextEdit::singleline(&mut text)).labelled_by(l.id);
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ok = true;
        }
        if kind == FurnitureKind::Table {
            let l = ui.label(trf("Depth in cells ({} to {}):", &[&TABLE_DEPTH_RANGE.0, &TABLE_DEPTH_RANGE.1]));
            let r = ui.add(egui::TextEdit::singleline(&mut depth)).labelled_by(l.id);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
        }
        if ui.button(tl("OK")).clicked() {
            ok = true;
        }
    });
    app.ui.physical.closet.resize_depth = depth.clone();
    app.ui.physical.closet.resize = if ok {
        match (text.trim().parse::<u32>(), depth.trim().parse::<u32>()) {
            (Ok(width), Ok(depth)) if kind == FurnitureKind::Table => {
                app.run("furniture.resize", json!({"furniture": id, "width": width, "depth": depth}));
            }
            (Ok(size), _) if kind != FurnitureKind::Table => {
                app.run("furniture.resize", json!({"furniture": id, "size": size}));
            }
            _ => app.ui.status = Some((tl("Enter a whole number").to_string(), true)),
        }
        None
    } else if open {
        Some((id, text))
    } else {
        None
    };
}

/// The Rename and Resize dialogs.
pub fn windows(app: &mut PacketApp, ctx: &egui::Context) {
    resize_window(app, ctx);
    let Some((thing, mut name)) = app.ui.physical.closet.rename.clone() else { return };
    let (mut open, mut ok) = (true, false);
    egui::Window::new(tl("Rename")).id(egui::Id::unique("closet_rename")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let l = ui.label(tl("Name:"));
        let r = ui.add(egui::TextEdit::singleline(&mut name)).labelled_by(l.id);
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ok = true;
        }
        if ui.button(tl("OK")).clicked() {
            ok = true;
        }
    });
    app.ui.physical.closet.rename = if ok {
        match thing {
            Thing::Furniture(id) => app.run("furniture.rename", json!({"furniture": id, "name": name})),
            Thing::Passive(id) => app.run("passive.rename", json!({"passive": id, "name": name})),
        };
        None
    } else if open {
        Some((thing, name))
    } else {
        None
    };
}

#[cfg(test)]
mod label_tests {
    use super::*;
    use egui::{Context, LayerId, Shape};

    /// The colour the first text on a frame is painted in (the galley's own, else the fallback).
    fn painted_text_color(dark: bool, in_rack: bool) -> (Color32, Color32) {
        let tk = theme::tokens(dark);
        let ctx = Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.ctx().layer_painter(LayerId::background());
            name_plate(&p, pos2(100.0, 100.0), "Switch0".to_string(), 12.0, in_rack, &tk);
        });
        let text = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                Shape::Text(t) => {
                    let own = t.galley.job.sections.first().map(|x| x.format.color).unwrap_or(Color32::PLACEHOLDER);
                    Some(if own == Color32::PLACEHOLDER { t.fallback_color } else { own })
                }
                _ => None,
            })
            .unwrap_or(Color32::TRANSPARENT);
        out.textures_delta.clear();
        let fill = plate_colors(in_rack, &tk).0;
        (text, fill)
    }

    #[test]
    fn the_name_on_a_plate_contrasts_with_the_plate_in_both_themes() {
        for dark in [false, true] {
            for in_rack in [false, true] {
                let (text, fill) = painted_text_color(dark, in_rack);
                assert_ne!(text, Color32::TRANSPARENT, "the name was not painted");
                assert!(theme::contrast(text, fill) >= 4.5, "dark={dark} in_rack={in_rack}: {text:?} on {fill:?}");
            }
        }
    }

    #[test]
    fn the_title_is_drawn_on_a_plate_that_holds_its_text_in_both_themes() {
        for dark in [false, true] {
            let tk = theme::tokens(dark);
            let ctx = Context::default();
            let mut plate = Rect::NOTHING;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let p = ui.ctx().layer_painter(LayerId::background());
                plate = title_plate(&p, pos2(12.0, 8.0), "Wiring closet: Main Wiring Closet".to_string(), &tk);
            });
            let mut backing = Rect::NOTHING;
            let mut text = Rect::NOTHING;
            for s in &out.shapes {
                match &s.shape {
                    Shape::Rect(r) if r.fill == tk.canvas => backing = backing.union(r.rect),
                    Shape::Text(t) => text = text.union(t.visual_bounding_rect()),
                    _ => {}
                }
            }
            out.textures_delta.clear();
            assert!(text.is_positive(), "the title was not painted");
            assert!(backing.contains_rect(text), "dark={dark}: the title is not on its plate: {text:?} vs {backing:?}");
            assert!(plate.contains_rect(text));
            assert!(theme::contrast(tk.text, tk.canvas) >= 4.5, "dark={dark}");
        }
    }

    #[test]
    fn small_jacks_become_blocks_and_every_port_keeps_its_socket() {
        use netlabcraft_engine::Session;
        let mut s = Session::new();
        s.execute("device.add", serde_json::json!({"model": "Switch-48", "name": "S", "x": 0, "y": 0})).unwrap();
        assert!(too_small(Rect::from_min_size(pos2(0.0, 0.0), vec2(5.0, 3.0))));
        assert!(!too_small(Rect::from_min_size(pos2(0.0, 0.0), vec2(14.0, 10.0))));
        assert!(!too_small(Rect::from_min_size(pos2(0.0, 0.0), vec2(6.5, 5.5))), "real connectors from a smaller size on");
        let dev = s.topology.find("S").unwrap();
        for zoom in [0.5f32, 0.8, 2.0] {
            let tk = theme::tokens(false);
            let ctx = Context::default();
            let mut sockets = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let p = ui.ctx().layer_painter(LayerId::background());
                sockets =
                    device_panel(&p, &s.topology, dev, Rect::from_min_size(pos2(10.0, 10.0), vec2(300.0 * zoom, 30.0 * zoom)), zoom, &tk, false);
            });
            out.textures_delta.clear();
            assert!(sockets.len() >= 48, "zoom {zoom}: {} sockets", sockets.len());
            let mut centres: Vec<(i32, i32)> = sockets.iter().map(|(_, r)| (r.center().x as i32, r.center().y as i32)).collect();
            centres.sort_unstable();
            centres.dedup();
            assert!(centres.len() > 24, "zoom {zoom}: the sockets overlap");
        }
    }

    #[test]
    fn the_fit_favours_readable_jacks_over_showing_every_rack_unit() {
        let rack = Cell { x: 60.0, y: 40.0, w: 340.0, h: FRAME_HEAD + 14.0 * RACK_UNIT_H + FRAME_FOOT };
        let shelf = Cell { x: 460.0, y: 40.0, w: 800.0, h: 300.0 };
        let layout = ClosetLayout { frames: vec![(1, rack), (2, shelf)], items: Vec::new() };
        let b = fit_box(&layout).unwrap();
        assert_eq!((b.x, b.y, b.w), (60.0, 40.0, 1200.0));
        assert_eq!(b.h, FRAME_HEAD + FIT_UNITS * RACK_UNIT_H + FRAME_FOOT, "a tall rack is framed {FIT_UNITS} units deep");
        let size = vec2(1500.0, 530.0);
        let (zoom, pan) = fit_view(size, b);
        let whole = (size.y - TITLE_BAND) / (rack.h + 36.0);
        assert!(zoom >= 1.0 && zoom > whole + 0.2, "zoom {zoom} against {whole} for the whole closet");
        assert!(pan.y + b.y * zoom >= TITLE_BAND, "the furniture starts below the title band");
        assert!(pan.x + b.x * zoom >= 0.0 && pan.x + (b.x + b.w) * zoom <= size.x, "the furniture is as wide as the view");
        // A small window scrolls instead of shrinking the jacks.
        assert_eq!(fit_view(vec2(300.0, 300.0), b).0, MIN_FIT_ZOOM);
        // A closet with no furniture is framed by its items.
        let loose = ClosetLayout { frames: Vec::new(), items: Vec::new() };
        assert!(fit_box(&loose).is_none());
    }

    #[test]
    fn panning_can_never_lose_the_closet() {
        let b = Cell { x: 60.0, y: 40.0, w: 1200.0, h: 700.0 };
        let size = vec2(1500.0, 530.0);
        for pan in [vec2(5000.0, 5000.0), vec2(-9000.0, -9000.0), vec2(0.0, 0.0), vec2(30.0, -120.0)] {
            let p = clamp_pan(pan, 1.0, b, size);
            assert!(p.x + b.x < size.x && p.x + b.x + b.w > 0.0, "{pan:?} -> {p:?}");
            assert!(p.y + b.y < size.y && p.y + b.y + b.h > 0.0, "{pan:?} -> {p:?}");
        }
        assert_eq!(clamp_pan(vec2(30.0, -120.0), 1.0, b, size), vec2(30.0, -120.0), "a pan in range is left alone");
    }

    #[test]
    fn plate_colors_always_contrast() {
        for dark in [false, true] {
            for in_rack in [false, true] {
                let tk = theme::tokens(dark);
                let (fill, text) = plate_colors(in_rack, &tk);
                assert!(theme::contrast(text, fill) >= 4.5);
            }
        }
    }
}
