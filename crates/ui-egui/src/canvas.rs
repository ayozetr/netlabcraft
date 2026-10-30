//! The topology canvas: devices, cables, link lights, and the packets of the event list.

use egui::{Align2, FontId, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use packetcraft_core::DeviceId;
use packetcraft_model::cable::Cable;
use packetcraft_model::{Cluster, Device, End, Shape, Shown, ifname};
use packetcraft_sim::port_up;
use serde_json::{Value, json};

use crate::i18n::tl;
use crate::theme::{self, Tokens};
use crate::{Mode, PacketApp, Pdu, Tool, console, icons};

/// Icon size in canvas units.
pub const ICON: f32 = 46.0;
const GRID: f32 = 40.0;

/// A two-click action in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    CableFrom(End),
    /// A console cable started at a device's Console or RS-232 port.
    ConsoleCable(DeviceId),
    PingFrom(DeviceId),
}

#[derive(Debug, Clone, Default)]
pub struct CanvasState {
    drag: Option<DeviceId>,
    drag_note: Option<u32>,
    /// Where a drawing being dragged out started (workspace units).
    draw_from: Option<(f32, f32)>,
    panning: bool,
    pending: Option<Pending>,
    /// The port chooser: device and where it opened (screen).
    port_menu: Option<(DeviceId, Pos2)>,
    /// For measuring note text during hit tests.
    ctx: Option<egui::Context>,
    /// Spanning trees for the link lights, for the topology with this fingerprint.
    stp_for: u64,
    stp: std::collections::BTreeMap<u16, packetcraft_sim::stp::VlanTree>,
    context: Option<DeviceId>,
    /// A cluster or drawing right-clicked (its menu is open).
    context_cluster: Option<u32>,
    context_note: Option<u32>,
    drag_cluster: Option<u32>,
    /// Dragging every picked device together.
    drag_group: bool,
    /// A freehand stroke being drawn, and the corners of a polygon being placed (workspace units).
    stroke: Vec<(f32, f32)>,
    pub(crate) poly: Vec<(f32, f32)>,
    /// Where a selection rectangle started (screen).
    band: Option<Pos2>,
    /// The workspace's size on screen (last frame).
    pub(crate) size: Vec2,
}

impl CanvasState {
    pub fn cancel(&mut self) {
        self.pending = None;
        self.port_menu = None;
        self.stroke.clear();
        self.poly.clear();
        self.band = None;
    }
}

/// Most points a freehand stroke records while dragging (the engine simplifies it further).
const MAX_STROKE: usize = 4_000;

/// The status-bar hint for the current tool.
pub fn hint(app: &PacketApp) -> String {
    if app.session.physical && !matches!(app.ui.tool, Tool::Place(_)) {
        return crate::i18n::tl("Double-click a place to open it · drag to arrange · right-click for more (move devices, cable lengths)").into();
    }
    match (app.ui.tool, app.ui.canvas.pending) {
        (Tool::Select, _) if app.session.topology.devices.is_empty() => {
            crate::i18n::tl("Pick a device in the palette and click the canvas to place it, or open a sample from File.").into()
        }
        (Tool::Select, _) if !app.ui.picked.is_empty() || !app.ui.picked_clusters.is_empty() => {
            crate::i18n::tl("Several picked: drag one to move them all · Ctrl+G groups them into a cluster").into()
        }
        (Tool::Select, _) => {
            crate::i18n::tl("Click a device to configure it · drag to move · Shift+drag or Shift+click to pick several · scroll to pan").into()
        }
        (Tool::Place(m), _) => crate::i18n::trf("Click the workspace to place a {}. Esc to stop.", &[&m]),
        (Tool::Draw(packetcraft_model::Shape::Freehand), _) => crate::i18n::tl("Drag on the workspace to draw freehand.").into(),
        (Tool::Draw(packetcraft_model::Shape::Polygon), _) => {
            crate::i18n::tl("Click the corners of the polygon; double-click or Enter to finish, Esc to cancel.").into()
        }
        (Tool::Draw(_), _) => crate::i18n::tl("Drag on the workspace to draw. Drawings sit behind devices; move them with Select.").into(),
        (Tool::Note, _) => crate::i18n::tl("Click the workspace to place a note. Drag notes to move them; double-click to edit.").into(),
        (Tool::Cable(Cable::Console), None) => crate::i18n::tl("Console cable: click a PC (RS-232) and a router or switch (Console).").into(),
        (Tool::Cable(_), None) => crate::i18n::tl("Click the first device and pick a port.").into(),
        (Tool::Cable(_), Some(Pending::CableFrom(e))) => {
            crate::i18n::trf("From {}: click the second device and pick a port. Esc cancels.", &[&app.session.topology.end_label(e)])
        }
        (Tool::Cable(_), Some(Pending::ConsoleCable(d))) => {
            let name = app.session.topology.device(d).map_or(String::new(), |x| format!("{} {}", x.name, x.console_port_name().unwrap_or("")));
            crate::i18n::trf("From {}: click the second device. Esc cancels.", &[&name])
        }
        (Tool::Ping, None) => crate::i18n::tl("Simple PDU: click the source device.").into(),
        (Tool::Ping, Some(_)) => crate::i18n::tl("Simple PDU: now click the destination device.").into(),
        (Tool::Inspect, _) => crate::i18n::tl("Inspect: click a device to see its ARP, MAC and routing tables.").into(),
        (Tool::Delete, _) => crate::i18n::tl("Click a device or a cable to delete it.").into(),
        _ => String::new(),
    }
}

struct View {
    origin: Vec2,
    zoom: f32,
    /// The cluster shown (none: the top level), and how each device appears in it.
    level: Option<u32>,
    shown: std::collections::BTreeMap<DeviceId, Shown>,
}

impl View {
    fn screen(&self, x: f32, y: f32) -> Pos2 {
        pos2(x * self.zoom, y * self.zoom) + self.origin
    }

    fn world(&self, p: Pos2) -> (f32, f32) {
        let v = (p - self.origin) / self.zoom;
        (v.x, v.y)
    }

    fn device_rect(&self, d: &Device) -> Rect {
        Rect::from_center_size(self.screen(d.x, d.y), Vec2::splat(ICON * self.zoom))
    }

    fn cluster_rect(&self, c: &Cluster) -> Rect {
        Rect::from_center_size(self.screen(c.x, c.y), vec2(ICON * 1.6, ICON * 1.1) * self.zoom)
    }

    /// The device is drawn as itself in this view.
    fn visible(&self, d: DeviceId) -> bool {
        self.shown.get(&d) == Some(&Shown::Device)
    }

    /// Where a device appears in this view: itself, or the cluster icon holding it.
    fn stand_in(&self, t: &packetcraft_model::Topology, d: DeviceId) -> Option<(Shown, Pos2)> {
        match *self.shown.get(&d)? {
            Shown::Device => t.device(d).map(|x| (Shown::Device, self.screen(x.x, x.y))),
            Shown::Cluster(c) => t.cluster(c).map(|x| (Shown::Cluster(c), self.screen(x.x, x.y))),
        }
    }
}

fn hit_device(app: &PacketApp, view: &View, p: Pos2) -> Option<DeviceId> {
    app.session.topology.devices.iter().rev().filter(|d| view.visible(d.id)).find(|d| view.device_rect(d).expand(6.0).contains(p)).map(|d| d.id)
}

fn hit_cluster(app: &PacketApp, view: &View, p: Pos2) -> Option<u32> {
    let t = &app.session.topology;
    t.clusters_in(view.level).collect::<Vec<_>>().into_iter().rev().find(|c| view.cluster_rect(c).expand(4.0).contains(p)).map(|c| c.id)
}

/// The screen rectangle of a workspace note.
fn note_rect(ctx: &egui::Context, view: &View, a: &packetcraft_model::Annotation) -> Rect {
    let font = FontId::proportional((15.0 * view.zoom).clamp(8.0, 24.0));
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(a.text.clone(), font, egui::Color32::BLACK));
    Rect::from_min_size(view.screen(a.x, a.y), galley.size()).expand2(vec2(6.0, 3.0))
}

fn hit_note(app: &PacketApp, view: &View, p: Pos2) -> Option<u32> {
    let ctx = app.ui.canvas.ctx.as_ref()?;
    let notes: Vec<&packetcraft_model::Annotation> = app.session.topology.annotations.iter().filter(|a| a.cluster == view.level).collect();
    // Text notes sit on top of drawings.
    notes
        .iter()
        .rev()
        .find(|a| a.shape == Shape::Text && note_rect(ctx, view, a).contains(p))
        .or_else(|| notes.iter().rev().find(|a| a.shape != Shape::Text && shape_hit(view, a, p)))
        .map(|a| a.id)
}

/// The screen rectangle of a drawing (normalised).
fn shape_rect(view: &View, a: &packetcraft_model::Annotation) -> Rect {
    Rect::from_two_pos(view.screen(a.x, a.y), view.screen(a.x + a.w, a.y + a.h))
}

/// A freehand stroke's or polygon's points on screen.
fn path_points(view: &View, a: &packetcraft_model::Annotation) -> Vec<Pos2> {
    a.points.iter().map(|p| view.screen(a.x + p[0], a.y + p[1])).collect()
}

fn near_path(pts: &[Pos2], p: Pos2, closed: bool) -> bool {
    let close = closed.then(|| pts.last().zip(pts.first())).flatten();
    pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(close.map(|(a, b)| (*a, *b))).any(|(a, b)| segment_distance(p, a, b) < 6.0)
}

/// Whether `p` is inside a polygon (even-odd rule).
fn inside_polygon(pts: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    let n = pts.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + n - 1) % n)) else { continue };
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

fn shape_hit(view: &View, a: &packetcraft_model::Annotation, p: Pos2) -> bool {
    match a.shape {
        Shape::Line => segment_distance(p, view.screen(a.x, a.y), view.screen(a.x + a.w, a.y + a.h)) < 6.0,
        Shape::Freehand => near_path(&path_points(view, a), p, false),
        Shape::Polygon => {
            let pts = path_points(view, a);
            inside_polygon(&pts, p) || near_path(&pts, p, true)
        }
        _ => shape_rect(view, a).contains(p),
    }
}

fn cross(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    (b - a).x * (c - a).y - (b - a).y * (c - a).x
}

/// Splits a simple polygon (convex or not) into triangles by ear clipping. A self-crossing
/// polygon gets as many triangles as can be found; the outline is drawn anyway.
fn triangulate(pts: &[Pos2]) -> Vec<[u32; 3]> {
    let n = pts.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    let area: f32 = (0..n).filter_map(|i| Some(cross(Pos2::ZERO, *pts.get(i)?, *pts.get((i + 1) % n)?))).sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut idx: Vec<usize> = (0..n).collect();
    let at = |i: usize| pts.get(i).copied().unwrap_or(Pos2::ZERO);
    while idx.len() > 3 {
        let m = idx.len();
        let ear = (0..m).find_map(|i| {
            let (a, b, c) = (*idx.get((i + m - 1) % m)?, *idx.get(i)?, *idx.get((i + 1) % m)?);
            let (pa, pb, pc) = (at(a), at(b), at(c));
            if cross(pa, pb, pc) * sign <= 0.0 {
                return None;
            }
            let blocked = idx.iter().filter(|j| ![a, b, c].contains(j)).any(|j| {
                let q = at(*j);
                cross(pa, pb, q) * sign >= 0.0 && cross(pb, pc, q) * sign >= 0.0 && cross(pc, pa, q) * sign >= 0.0
            });
            (!blocked).then_some((i, [a, b, c]))
        });
        let Some((i, tri)) = ear else { break };
        out.push(tri.map(|v| v as u32));
        idx.remove(i);
    }
    if let [a, b, c] = idx.as_slice() {
        out.push([*a as u32, *b as u32, *c as u32]);
    }
    out
}

const DRAW_COLORS: [&str; 5] = ["#bcd9f2", "#c8ecc0", "#fde9a8", "#f6c6c6", "#d9cff2"];

fn next_color(app: &PacketApp) -> &'static str {
    let n = app.session.topology.annotations.iter().filter(|a| a.shape != packetcraft_model::Shape::Text).count();
    DRAW_COLORS.get(n % DRAW_COLORS.len()).copied().unwrap_or("#bcd9f2")
}

fn paint_shape(painter: &egui::Painter, view: &View, a: &packetcraft_model::Annotation) {
    let [r, g, b] = a.color.as_deref().and_then(packetcraft_model::parse_color).unwrap_or([0xbc, 0xd9, 0xf2]);
    let fill = egui::Color32::from_rgb(r, g, b);
    let edge = Stroke::new(1.0, crate::theme::shade(fill, 0.7));
    match a.shape {
        packetcraft_model::Shape::Rectangle => {
            let rect = shape_rect(view, a);
            painter.rect_filled(rect, 2.0, fill);
            painter.rect_stroke(rect, 2.0, edge, StrokeKind::Inside);
        }
        packetcraft_model::Shape::Ellipse => {
            let rect = shape_rect(view, a);
            let pts: Vec<Pos2> = (0..48)
                .map(|i| i as f32 / 48.0 * std::f32::consts::TAU)
                .map(|t| rect.center() + vec2(t.cos() * rect.width() / 2.0, t.sin() * rect.height() / 2.0))
                .collect();
            painter.add(egui::Shape::convex_polygon(pts, fill, edge));
        }
        packetcraft_model::Shape::Line => {
            painter.line_segment(
                [view.screen(a.x, a.y), view.screen(a.x + a.w, a.y + a.h)],
                Stroke::new((3.0 * view.zoom).max(1.5), crate::theme::shade(fill, 0.6)),
            );
        }
        Shape::Freehand => {
            let stroke = Stroke::new((3.0 * view.zoom).max(1.5), crate::theme::shade(fill, 0.6));
            painter.add(egui::Shape::line(path_points(view, a), stroke));
        }
        Shape::Polygon => {
            let pts = path_points(view, a);
            let mut mesh = egui::Mesh::default();
            for p in &pts {
                mesh.colored_vertex(*p, fill);
            }
            for [i, j, k] in triangulate(&pts) {
                mesh.add_triangle(i, j, k);
            }
            painter.add(mesh);
            painter.add(egui::Shape::closed_line(pts, edge));
        }
        packetcraft_model::Shape::Text => {}
    }
}

fn draw_shapes(app: &PacketApp, painter: &egui::Painter, view: &View) {
    for a in app.session.topology.annotations.iter().filter(|a| a.shape != packetcraft_model::Shape::Text && a.cluster == view.level) {
        paint_shape(painter, view, a);
    }
}

/// A drawing in progress: the freehand stroke so far, or the polygon's corners and the pointer.
fn draw_in_progress(app: &PacketApp, painter: &egui::Painter, view: &View, pointer: Option<Pos2>) {
    let c = &app.ui.canvas;
    let [r, g, b] = packetcraft_model::parse_color(next_color(app)).unwrap_or([0xbc, 0xd9, 0xf2]);
    let color = crate::theme::shade(egui::Color32::from_rgb(r, g, b), 0.6);
    let stroke = Stroke::new((3.0 * view.zoom).max(1.5), color);
    if c.stroke.len() > 1 {
        painter.add(egui::Shape::line(c.stroke.iter().map(|(x, y)| view.screen(*x, *y)).collect(), stroke));
    }
    if !c.poly.is_empty() {
        let mut pts: Vec<Pos2> = c.poly.iter().map(|(x, y)| view.screen(*x, *y)).collect();
        for p in &pts {
            painter.circle_filled(*p, 3.5, color);
        }
        pts.extend(pointer);
        painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
    }
}

fn draw_notes(app: &PacketApp, painter: &egui::Painter, view: &View, tk: &Tokens) {
    for a in app.session.topology.annotations.iter().filter(|a| a.shape == packetcraft_model::Shape::Text && a.cluster == view.level) {
        let r = note_rect(painter.ctx(), view, a);
        painter.rect_filled(r, 2.0, if app.ui.dark { tk.chrome } else { egui::Color32::from_rgb(0xf3, 0xf4, 0xf6) });
        let font = FontId::proportional((15.0 * view.zoom).clamp(8.0, 24.0));
        painter.text(r.min + vec2(6.0, 3.0), Align2::LEFT_TOP, &a.text, font, tk.text);
    }
}

/// The note editor (Place Note, double-click on a note).
pub fn note_editor(app: &mut PacketApp, ctx: &egui::Context) {
    let Some((id, mut text)) = app.ui.editing_note.clone() else { return };
    let mut open = true;
    let mut action = None;
    egui::Window::new(tl("Note")).id(egui::Id::unique("note_editor")).open(&mut open).collapsible(false).default_size([320.0, 140.0]).show(
        ctx,
        |ui| {
            let l = ui.label(tl("Text shown on the workspace:"));
            let r = ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY).desired_rows(3)).labelled_by(l.id);
            r.request_focus();
            ui.horizontal(|ui| {
                if ui.button(tl("OK")).clicked() {
                    action = Some(true);
                }
                if ui.button(tl("Delete Note")).clicked() {
                    action = Some(false);
                }
            });
        },
    );
    match action {
        Some(true) => {
            let t = if text.trim().is_empty() { "Note".to_string() } else { text };
            app.run("note.edit", json!({"id": id, "text": t}));
            app.ui.editing_note = None;
        }
        Some(false) => {
            app.run("note.remove", json!({"id": id}));
            app.ui.editing_note = None;
        }
        None if !open => app.ui.editing_note = None,
        None => app.ui.editing_note = Some((id, text)),
    }
}

/// The console cable under the pointer: the PC at its RS-232 end.
fn hit_console(app: &PacketApp, view: &View, p: Pos2) -> Option<DeviceId> {
    let t = &app.session.topology;
    t.consoles.iter().filter(|c| view.visible(c.pc) && view.visible(c.device)).find_map(|c| {
        let (a, b) = (t.device(c.pc)?, t.device(c.device)?);
        (segment_distance(p, view.screen(a.x, a.y), view.screen(b.x, b.y)) < 6.0).then_some(c.pc)
    })
}

fn hit_link(app: &PacketApp, view: &View, p: Pos2) -> Option<End> {
    let t = &app.session.topology;
    t.links.iter().filter(|l| view.visible(l.a.device) && view.visible(l.b.device)).find_map(|l| {
        let (pa, pb) = cable_ends(t, view, l)?;
        (segment_distance(p, pa, pb) < 6.0).then_some(l.a)
    })
}

/// Where a cable is drawn. Cables between the same two devices (an EtherChannel, or a
/// redundant pair) sit side by side instead of on top of each other.
fn cable_ends(t: &packetcraft_model::Topology, view: &View, l: &packetcraft_model::Link) -> Option<(Pos2, Pos2)> {
    let (a, b) = (t.device(l.a.device)?, t.device(l.b.device)?);
    let (pa, pb) = (view.screen(a.x, a.y), view.screen(b.x, b.y));
    let pair = |x: &packetcraft_model::Link| (x.a.device.min(x.b.device), x.a.device.max(x.b.device));
    let same: Vec<&packetcraft_model::Link> = t.links.iter().filter(|x| pair(x) == pair(l)).collect();
    let len = (pb - pa).length();
    if same.len() < 2 || len < 1.0 {
        return Some((pa, pb));
    }
    let k = same.iter().position(|x| *x == l)?;
    // The side is measured from the lower device id, so both ends agree.
    let d = if l.a.device <= l.b.device { (pb - pa) / len } else { (pa - pb) / len };
    let off = vec2(-d.y, d.x) * (k as f32 - (same.len() - 1) as f32 / 2.0) * 18.0 * view.zoom;
    Some((pa + off, pb + off))
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// The first address configured on a device (for the ping tool).
fn first_address(d: &Device) -> Option<String> {
    d.ports
        .iter()
        .find_map(|p| p.ip.map(|ip| ip.addr.to_string()))
        .or_else(|| d.ports.iter().flat_map(|p| &p.subifs).find_map(|s| s.ip.map(|ip| ip.addr.to_string())))
}

pub fn show(app: &mut PacketApp, ui: &mut Ui) {
    let tk = theme::tokens(app.ui.dark);
    app.ui.canvas.ctx = Some(ui.ctx().clone());
    let rect = ui.max_rect();
    app.ui.canvas_rect = Some(rect);
    app.ui.canvas.size = rect.size();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, tk.canvas);

    // Pan with scroll, zoom with Ctrl+scroll or pinch, around the pointer.
    if resp.hovered() {
        let (scroll, zoom_delta, pointer) = ui.input(|i| (i.smooth_scroll_delta(), i.zoom_delta(), i.pointer.hover_pos()));
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            let old = app.ui.zoom;
            let new = (old * zoom_delta).clamp(0.25, 3.0);
            if let Some(p) = pointer {
                let anchor = p - rect.min.to_vec2() - app.ui.pan;
                app.ui.pan += anchor.to_vec2() * (1.0 - new / old);
            }
            app.ui.zoom = new;
        } else {
            app.ui.pan += scroll;
        }
    }
    let level = app.session.view;
    let view = View { origin: rect.min.to_vec2() + app.ui.pan, zoom: app.ui.zoom, level, shown: app.session.topology.shown_map(level) };
    app.ui.pointer = resp.hover_pos().map(|p| view.world(p));

    // Grid (Options ▸ Show grid).
    let step = GRID * view.zoom;
    if app.ui.show_grid && step >= 8.0 {
        let off = vec2(view.origin.x.rem_euclid(step), view.origin.y.rem_euclid(step));
        let mut x = rect.left() + off.x;
        while x < rect.right() {
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, tk.grid));
            x += step;
        }
        let mut y = rect.top() + off.y;
        while y < rect.bottom() {
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, tk.grid));
            y += step;
        }
    }

    draw_shapes(app, &painter, &view);
    // The drawing being dragged out sits with the other drawings, under the cables and their
    // port labels, as it will once placed.
    if let (Some((x0, y0)), Some(p), Tool::Draw(shape)) = (app.ui.canvas.draw_from, ui.input(|i| i.pointer.hover_pos()), app.ui.tool) {
        let (x1, y1) = view.world(p);
        let preview = packetcraft_model::Annotation {
            id: 0,
            x: x0,
            y: y0,
            text: String::new(),
            shape,
            w: x1 - x0,
            h: y1 - y0,
            color: Some(next_color(app).to_string()),
            points: Vec::new(),
            cluster: None,
        };
        paint_shape(&painter, &view, &preview);
    }
    draw_links(app, &painter, &view, &tk);
    draw_notes(app, &painter, &view, &tk);
    draw_in_progress(app, &painter, &view, ui.input(|i| i.pointer.hover_pos()));
    draw_packet(app, ui, &painter, &view, &tk);
    draw_clusters(app, &painter, &view, &tk);
    draw_devices(app, &painter, &view, &tk);
    if let (Some(a), Some(b)) = (app.ui.canvas.band, ui.input(|i| i.pointer.hover_pos())) {
        let r = Rect::from_two_pos(a, b);
        painter.rect_filled(r, 0.0, tk.selection.gamma_multiply(0.08));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, tk.selection), StrokeKind::Inside);
    }

    // A cable being drawn follows the pointer.
    let drawing_from = match app.ui.canvas.pending {
        Some(Pending::CableFrom(e)) => Some(e.device),
        Some(Pending::ConsoleCable(d)) => Some(d),
        _ => None,
    };
    if let (Some(from), Some(p)) = (drawing_from, ui.input(|i| i.pointer.hover_pos()))
        && let Some(d) = app.session.topology.device(from)
    {
        let from = view.screen(d.x, d.y);
        painter.extend(egui::Shape::dashed_line(&[from, p], Stroke::new(2.0, tk.selection), 6.0, 4.0));
    }
    if let (Some(Pending::PingFrom(src)), Some(p)) = (app.ui.canvas.pending, ui.input(|i| i.pointer.hover_pos()))
        && let Some(d) = app.session.topology.device(src)
    {
        painter.extend(egui::Shape::dashed_line(
            &[view.screen(d.x, d.y), p],
            Stroke::new(1.5, theme::protocol_color(packetcraft_wire::Protocol::Icmp)),
            4.0,
            4.0,
        ));
        icons::envelope(&painter, p + vec2(14.0, 14.0), 18.0, theme::protocol_color(packetcraft_wire::Protocol::Icmp), tk.text);
    }

    interact(app, ui, &resp, &view);
    port_menu(app, ui);
}

/// Whether the spanning tree blocks a switch port (in its access VLAN, or a trunk's native VLAN).
fn stp_blocked(app: &mut PacketApp, end: End) -> bool {
    let t = &app.session.topology;
    let Some(d) = t.device(end.device).filter(|d| d.kind == packetcraft_model::DeviceKind::Switch) else { return false };
    let Some(p) = d.ports.get(end.port) else { return false };
    let vlan = if t.is_trunk(end) { p.switchport.native_vlan } else { p.switchport.access_vlan };
    let f = packetcraft_sim::stp::fingerprint(t);
    let c = &mut app.ui.canvas;
    if c.stp_for != f {
        c.stp_for = f;
        c.stp.clear();
    }
    let tree = c.stp.entry(vlan.get()).or_insert_with(|| packetcraft_sim::stp::compute(t, vlan));
    // Amber while blocked, and while listening or learning after coming up.
    !tree.forwards(end.device, end.port) || app.session.sim.stp_converging(end.device, end.port)
}

fn draw_links(app: &mut PacketApp, painter: &egui::Painter, view: &View, tk: &Tokens) {
    let ends: Vec<End> = app.session.topology.links.iter().flat_map(|l| [l.a, l.b]).collect();
    let blocked: Vec<End> = ends.into_iter().filter(|e| stp_blocked(app, *e)).collect();
    let t = &app.session.topology;
    let mut lights: Vec<(End, Pos2)> = Vec::new();
    // Cables that leave a cluster run to its icon; one line per pair of icons.
    let mut outside = std::collections::BTreeSet::new();
    for l in &t.links {
        let (Some((sa, pa)), Some((sb, pb))) = (view.stand_in(t, l.a.device), view.stand_in(t, l.b.device)) else { continue };
        if (sa == Shown::Device && sb == Shown::Device) || sa == sb {
            continue;
        }
        let key = |s: Shown, d: DeviceId| match s {
            Shown::Device => (0, d.0),
            Shown::Cluster(c) => (1, c),
        };
        let (ka, kb) = (key(sa, l.a.device), key(sb, l.b.device));
        if outside.insert((ka.min(kb), ka.max(kb))) {
            painter.line_segment([pa, pb], Stroke::new((2.2 * view.zoom).max(1.0), tk.cable));
        }
    }
    for l in t.links.iter().filter(|l| view.visible(l.a.device) && view.visible(l.b.device)) {
        let Some((pa, pb)) = cable_ends(t, view, l) else { continue };
        // How far this cable sits from the straight line between its devices (side-by-side cables).
        let spread = match (t.device(l.a.device), t.device(l.b.device)) {
            (Some(a), Some(b)) => ((pa - view.screen(a.x, a.y)) + (pb - view.screen(b.x, b.y))) * 0.5,
            _ => Vec2::ZERO,
        };
        let len = (pb - pa).length();
        let kind = t.resolved_cable(l);
        let width = (2.2 * view.zoom).max(1.0);
        if kind.is_serial() && len >= 1.0 {
            // A serial cable: red, with a zigzag in the middle; a clock by the DCE end.
            let red = Stroke::new(width, tk.serial_cable);
            let d = (pb - pa) / len;
            let n = vec2(-d.y, d.x);
            let mid = pa + (pb - pa) * 0.5;
            let z = 9.0 * view.zoom;
            painter.add(egui::Shape::line(vec![pa, mid - d * z, mid + n * z * 0.9, mid - n * z * 0.9, mid + d * z, pb], red));
            // Opposite the port labels, which sit above a lone cable.
            let (from, toward) = if t.dce_of(l.a) == Some(l.b) { (pb, -d) } else { (pa, d) };
            let n_end = vec2(-toward.y, toward.x);
            let below = if n_end.y > 0.0 { n_end } else { -n_end };
            // Past the name lines under the icon when the cable leaves downwards.
            let down = toward.y.max(0.0);
            let clock = from + toward * (ICON * 0.5 + 34.0 + 40.0 * down * down) * view.zoom + below * 13.0 * view.zoom;
            let r = 6.0 * view.zoom.clamp(0.6, 1.4);
            painter.circle(clock, r, tk.chrome, Stroke::new(1.2, red.color));
            painter.line_segment([clock, clock - vec2(0.0, r * 0.7)], Stroke::new(1.2, red.color));
            painter.line_segment([clock, clock + vec2(r * 0.55, 0.0)], Stroke::new(1.2, red.color));
        } else {
            match kind {
                Cable::Cross => {
                    painter.extend(egui::Shape::dashed_line(&[pa, pb], Stroke::new(width, tk.cable), 10.0 * view.zoom, 5.0 * view.zoom));
                }
                Cable::Fiber => {
                    painter.line_segment([pa, pb], Stroke::new(width, tk.fiber_cable));
                }
                _ => {
                    painter.line_segment([pa, pb], Stroke::new(width, tk.cable));
                }
            }
        }
        if len < 1.0 {
            continue;
        }
        let dir = (pb - pa) / len;
        for (end, at, sign) in [(l.a, pa, 1.0), (l.b, pb, -1.0)] {
            // Cables leaving downwards clear the two label lines under the icon.
            let below = (dir * sign).y.max(0.0);
            let inset = (ICON * 0.5 + 26.0 + 40.0 * below * below) * view.zoom;
            let light = at + dir * sign * inset.min(len * 0.45);
            lights.push((end, light));
            let color = if !port_up(t, end) {
                tk.link_down
            } else if blocked.contains(&end) {
                tk.link_blocked
            } else {
                tk.link_up
            };
            // A small triangle pointing along the cable, away from the device.
            let d = dir * sign;
            let n = vec2(-d.y, d.x);
            let sz = 5.5 * view.zoom.clamp(0.6, 1.4);
            painter.add(egui::Shape::convex_polygon(
                vec![light + d * sz, light - d * sz * 0.6 + n * sz * 0.8, light - d * sz * 0.6 - n * sz * 0.8],
                color,
                Stroke::NONE,
            ));
            if view.zoom >= 0.6 {
                let name = t.device(end.device).and_then(|d| d.ports.get(end.port)).map(|p| ifname::short(&p.name)).unwrap_or_default();
                // Side by side cables label outwards; a lone cable labels above.
                let side = if spread.length() > 0.5 {
                    if n.dot(spread) > 0.0 { n } else { -n }
                } else if n.y > 0.0 {
                    -n
                } else {
                    n
                };
                painter.text(light + side * 14.0 + d * 8.0, Align2::CENTER_CENTER, name, FontId::proportional(16.0 * view.zoom.min(1.3)), tk.text);
            }
        }
    }
    // Console cables: light blue and dashed, from the PC to the device; no link lights (they
    // carry no frames).
    for c in t.consoles.iter().filter(|c| view.visible(c.pc) && view.visible(c.device)) {
        let (Some(a), Some(b)) = (t.device(c.pc), t.device(c.device)) else { continue };
        let (pa, pb) = (view.screen(a.x, a.y), view.screen(b.x, b.y));
        let stroke = Stroke::new((2.0 * view.zoom).max(1.0), tk.console_cable);
        painter.extend(egui::Shape::dashed_line(&[pa, pb], stroke, 5.0 * view.zoom, 4.0 * view.zoom));
    }
    // Wi-Fi associations: a dashed line from each host to its access point.
    for d in t.devices.iter().filter(|d| d.kind.is_host() && view.visible(d.id)) {
        for p in 0..d.ports.len() {
            let end = End { device: d.id, port: p };
            if t.link_at(end).is_some() {
                continue;
            }
            let Some(ap) = t.wifi_ap_of(end).and_then(|e| t.device(e.device)).filter(|ap| view.visible(ap.id)) else { continue };
            let (a, b) = (view.screen(d.x, d.y), view.screen(ap.x, ap.y));
            let stroke = Stroke::new((1.6 * view.zoom).max(1.0), tk.link_up);
            painter.add(egui::Shape::dashed_line(&[a, b], stroke, 8.0 * view.zoom, 6.0 * view.zoom));
        }
    }
    // An EtherChannel: a ring around its bundled cables, next to each switch.
    for d in t.devices.iter().filter(|d| d.kind == packetcraft_model::DeviceKind::Switch && view.visible(d.id)) {
        for c in packetcraft_sim::channel::channels(t, d) {
            let pts: Vec<Pos2> =
                c.bundled().iter().filter_map(|p| lights.iter().find(|(e, _)| e.device == d.id && e.port == *p).map(|(_, at)| *at)).collect();
            if pts.len() < 2 {
                continue;
            }
            let center = pts.iter().fold(Vec2::ZERO, |s, p| s + p.to_vec2()) / pts.len() as f32;
            let r = pts.iter().map(|p| (p.to_vec2() - center).length()).fold(0.0, f32::max) + 9.0 * view.zoom;
            painter.circle_stroke(center.to_pos2(), r, Stroke::new((1.6 * view.zoom).max(1.0), tk.text));
        }
    }
}

fn draw_devices(app: &PacketApp, painter: &egui::Painter, view: &View, tk: &Tokens) {
    for d in app.session.topology.devices.iter().filter(|d| view.visible(d.id)) {
        let r = view.device_rect(d);
        if app.ui.selected == Some(d.id) || app.ui.picked.contains(&d.id) {
            painter.rect_filled(r.expand(6.0), 8.0, tk.selection.gamma_multiply(0.15));
            painter.rect_stroke(r.expand(6.0), 8.0, Stroke::new(1.5, tk.selection), StrokeKind::Outside);
        }
        if matches!(app.ui.canvas.pending, Some(Pending::CableFrom(e)) if e.device == d.id)
            || matches!(app.ui.canvas.pending, Some(Pending::ConsoleCable(x) | Pending::PingFrom(x)) if x == d.id)
        {
            painter.rect_stroke(r.expand(6.0), 8.0, Stroke::new(2.0, tk.selection), StrokeKind::Outside);
        }
        icons::device(painter, r, d.kind, tk);
        icons::model_marks(painter, r, d.catalogue_model().id, d.catalogue_model().multilayer());
        if d.powered_off {
            icons::powered_off_mark(painter, r);
        }
        // Two centred lines under the icon: model, then name.
        let font = FontId::proportional((16.0 * view.zoom).clamp(8.0, 24.0));
        let mut y = r.bottom() + 2.0;
        for line in [d.catalogue_model().label, d.display_name()] {
            let galley = painter.layout_no_wrap(line.to_string(), font.clone(), tk.text);
            let label = Align2::CENTER_TOP.anchor_size(pos2(r.center().x, y), galley.size());
            y = label.bottom();
            painter.galley(label.min, galley, tk.text);
        }
    }
}

/// The event selected in the event list, as an envelope travelling along its cable.
fn draw_packet(app: &PacketApp, ui: &Ui, painter: &egui::Painter, view: &View, tk: &Tokens) {
    let Some(sel) = app.ui.events.selected else { return };
    let t = &app.session.topology;
    let Some(ev) = app.session.sim.trace().iter().find(|e| e.id == sel) else { return };
    let (Some((sa, a)), Some((sb, b))) = (view.stand_in(t, ev.from.device), view.stand_in(t, ev.to.device)) else { return };
    if sa != Shown::Device && sa == sb {
        // Inside one closed cluster: nothing to show here.
        return;
    }
    let cable = t.links.iter().find(|l| l.a == ev.from || l.b == ev.from).filter(|_| sa == Shown::Device && sb == Shown::Device);
    let (pa, pb) = match cable.and_then(|l| cable_ends(t, view, l)) {
        // Along the cable it took, from the sender's end.
        Some((x, y)) if t.links.iter().any(|l| l.a == ev.from) => (x, y),
        Some((x, y)) => (y, x),
        None => (a, b),
    };
    let progress = app.ui.events.progress(ui.input(|i| i.time));
    let at = pa + (pb - pa) * progress;
    painter.line_segment([pa, pb], Stroke::new((4.0 * view.zoom).max(2.0), theme::protocol_color(ev.protocol).gamma_multiply(0.5)));
    icons::envelope(painter, at, (22.0 * view.zoom).clamp(14.0, 34.0), theme::protocol_color(ev.protocol), tk.text);
    if ev.dropped.is_some() && progress >= 0.99 {
        let s = Stroke::new(3.0, tk.error);
        let r = Rect::from_center_size(pb, Vec2::splat(ICON * 0.5 * view.zoom));
        painter.line_segment([r.left_top(), r.right_bottom()], s);
        painter.line_segment([r.right_top(), r.left_bottom()], s);
    }
}

/// Cluster icons in this view: a cloud with the number of devices inside, and the name under it.
fn draw_clusters(app: &PacketApp, painter: &egui::Painter, view: &View, tk: &Tokens) {
    let t = &app.session.topology;
    for c in t.clusters_in(view.level) {
        let r = view.cluster_rect(c);
        if app.ui.picked_clusters.contains(&c.id) {
            painter.rect_filled(r.expand(6.0), 8.0, tk.selection.gamma_multiply(0.15));
            painter.rect_stroke(r.expand(6.0), 8.0, Stroke::new(1.5, tk.selection), StrokeKind::Outside);
        }
        icons::cloud(painter, r, tk.cluster, theme::shade(tk.cluster, 0.55));
        let inside = view.shown.values().filter(|s| **s == Shown::Cluster(c.id)).count();
        let font = FontId::proportional((15.0 * view.zoom).clamp(8.0, 24.0));
        painter.text(r.center() + vec2(0.0, r.height() * 0.08), Align2::CENTER_CENTER, inside.to_string(), font.clone(), tk.text);
        let galley = painter.layout_no_wrap(c.name.clone(), FontId::proportional((16.0 * view.zoom).clamp(8.0, 24.0)), tk.text);
        let label = Align2::CENTER_TOP.anchor_size(pos2(r.center().x, r.bottom() + 2.0), galley.size());
        painter.galley(label.min, galley, tk.text);
    }
}

/// A rectangle dragged out with Shift picks the devices and clusters inside it.
fn finish_band(app: &mut PacketApp, view: &View, a: Pos2, b: Pos2) {
    let r = Rect::from_two_pos(a, b);
    let t = &app.session.topology;
    let devs: Vec<DeviceId> = t.devices.iter().filter(|d| view.visible(d.id) && r.contains(view.screen(d.x, d.y))).map(|d| d.id).collect();
    let cls: Vec<u32> = t.clusters_in(view.level).filter(|c| r.contains(view.screen(c.x, c.y))).map(|c| c.id).collect();
    if let Some(s) = app.ui.selected.take() {
        app.ui.picked.insert(s);
    }
    app.ui.picked.extend(devs);
    app.ui.picked_clusters.extend(cls);
}

/// Pans so what the current view shows (devices and cluster icons) sits in the middle.
pub(crate) fn fit_view(app: &mut PacketApp) {
    let t = &app.session.topology;
    let level = app.session.view;
    let shown = t.shown_map(level);
    let pts: Vec<(f32, f32)> = t
        .devices
        .iter()
        .filter(|d| shown.get(&d.id) == Some(&Shown::Device))
        .map(|d| (d.x, d.y))
        .chain(t.clusters_in(level).map(|c| (c.x, c.y)))
        .collect();
    if pts.is_empty() || app.ui.canvas.size.x < 1.0 {
        return;
    }
    let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let center = vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    app.ui.pan = app.ui.canvas.size / 2.0 - center * app.ui.zoom;
}

/// Closes the polygon being placed into a drawing.
pub(crate) fn finish_polygon(app: &mut PacketApp) {
    let pts: Vec<[f32; 2]> = std::mem::take(&mut app.ui.canvas.poly).into_iter().map(|(x, y)| [x, y]).collect();
    if pts.len() < 3 {
        app.info(tl("A polygon needs at least three corners."));
        return;
    }
    let color = next_color(app);
    app.run("note.add", json!({"shape": "polygon", "points": pts, "color": color}));
}

/// Shapes dragged out from corner to corner.
fn box_shape(t: Tool) -> Option<Shape> {
    match t {
        Tool::Draw(s @ (Shape::Rectangle | Shape::Ellipse | Shape::Line)) => Some(s),
        _ => None,
    }
}

fn interact(app: &mut PacketApp, ui: &Ui, resp: &egui::Response, view: &View) {
    let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
    let middle = ui.input(|i| i.pointer.middle_down());
    let shift = ui.input(|i| i.modifiers.shift);

    // Freehand: the path is recorded while dragging.
    if app.ui.tool == Tool::Draw(Shape::Freehand) && !middle {
        if resp.drag_started()
            && let Some(p) = pointer
        {
            app.ui.canvas.stroke = vec![view.world(p)];
            return;
        }
        if resp.drag_stopped() && !app.ui.canvas.stroke.is_empty() {
            let pts: Vec<[f32; 2]> = std::mem::take(&mut app.ui.canvas.stroke).into_iter().map(|(x, y)| [x, y]).collect();
            if pts.len() >= 2 {
                let color = next_color(app);
                app.run("note.add", json!({"shape": "freehand", "points": pts, "color": color}));
            }
            return;
        }
        if resp.dragged() && !app.ui.canvas.stroke.is_empty() {
            if let Some(p) = pointer {
                let (x, y) = view.world(p);
                let c = &mut app.ui.canvas;
                let far = c.stroke.last().is_none_or(|(lx, ly)| (x - lx).hypot(y - ly) * view.zoom >= 2.0);
                if far && c.stroke.len() < MAX_STROKE {
                    c.stroke.push((x, y));
                }
            }
            return;
        }
    }
    // Polygon: clicks place corners; double-click or Enter closes it.
    if app.ui.tool == Tool::Draw(Shape::Polygon) {
        let enter = !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if resp.double_clicked() || (enter && !app.ui.canvas.poly.is_empty()) {
            finish_polygon(app);
            return;
        }
        if resp.clicked()
            && let Some(p) = pointer
        {
            let (x, y) = view.world(p);
            let c = &mut app.ui.canvas;
            let far = c.poly.last().is_none_or(|(lx, ly)| (x - lx).hypot(y - ly) * view.zoom >= 3.0);
            if far && c.poly.len() < packetcraft_model::MAX_POLYGON_POINTS {
                c.poly.push((x, y));
            }
            return;
        }
    }

    if resp.drag_started()
        && box_shape(app.ui.tool).is_some()
        && !middle
        && let Some(p) = pointer
    {
        app.ui.canvas.draw_from = Some(view.world(p));
        return;
    }
    if resp.drag_stopped()
        && let (Some((x0, y0)), Some(shape), Some(p)) = (app.ui.canvas.draw_from.take(), box_shape(app.ui.tool), pointer)
    {
        let (x1, y1) = view.world(p);
        let (w, h) = (x1 - x0, y1 - y0);
        if w.abs() + h.abs() > 6.0 {
            let id = match shape {
                Shape::Rectangle => "rectangle",
                Shape::Ellipse => "ellipse",
                _ => "line",
            };
            let color = next_color(app);
            app.run("note.add", json!({"shape": id, "x": x0, "y": y0, "w": w, "h": h, "color": color}));
        }
        return;
    }
    if app.ui.canvas.draw_from.is_some() {
        return;
    }
    if resp.drag_started() {
        app.ui.canvas.panning = true;
        let select = !middle && app.ui.tool == Tool::Select;
        if select && let Some(d) = pointer.and_then(|p| hit_device(app, view, p)) {
            app.run("edit.checkpoint", json!({}));
            app.ui.canvas.drag = Some(d);
            app.ui.canvas.panning = false;
            if app.ui.picked.contains(&d) && app.ui.picked.len() + app.ui.picked_clusters.len() > 1 {
                app.ui.canvas.drag_group = true;
            } else {
                app.ui.selected = Some(d);
            }
        } else if select && let Some(c) = pointer.and_then(|p| hit_cluster(app, view, p)) {
            app.run("edit.checkpoint", json!({}));
            app.ui.canvas.panning = false;
            if app.ui.picked_clusters.contains(&c) && app.ui.picked.len() + app.ui.picked_clusters.len() > 1 {
                app.ui.canvas.drag_group = true;
            } else {
                app.ui.canvas.drag_cluster = Some(c);
            }
        } else if !middle
            && matches!(app.ui.tool, Tool::Select | Tool::Note)
            && let Some(n) = pointer.and_then(|p| hit_note(app, view, p))
        {
            app.run("edit.checkpoint", json!({}));
            app.ui.canvas.drag_note = Some(n);
            app.ui.canvas.panning = false;
        } else if select && shift {
            app.ui.canvas.band = pointer;
            app.ui.canvas.panning = false;
        }
    }
    if resp.dragged() {
        let delta = resp.drag_delta() / view.zoom;
        if app.ui.canvas.drag_group {
            let t = &app.session.topology;
            let devs: Vec<(DeviceId, f32, f32)> =
                app.ui.picked.iter().filter_map(|d| t.device(*d)).map(|d| (d.id, d.x + delta.x, d.y + delta.y)).collect();
            let cls: Vec<(u32, f32, f32)> =
                app.ui.picked_clusters.iter().filter_map(|c| t.cluster(*c)).map(|c| (c.id, c.x + delta.x, c.y + delta.y)).collect();
            for (d, x, y) in devs {
                app.run("device.move", json!({"device": d.0, "x": x, "y": y, "transient": true}));
            }
            for (c, x, y) in cls {
                app.run("cluster.move", json!({"id": c, "x": x, "y": y, "transient": true}));
            }
        } else if let Some(d) = app.ui.canvas.drag {
            if let Some(dev) = app.session.topology.device(d) {
                let (x, y) = (dev.x + delta.x, dev.y + delta.y);
                app.run("device.move", json!({"device": d.0, "x": x, "y": y, "transient": true}));
            }
        } else if let Some(c) = app.ui.canvas.drag_cluster {
            if let Some(cl) = app.session.topology.cluster(c) {
                let (x, y) = (cl.x + delta.x, cl.y + delta.y);
                app.run("cluster.move", json!({"id": c, "x": x, "y": y, "transient": true}));
            }
        } else if let Some(n) = app.ui.canvas.drag_note {
            if let Some(a) = app.session.topology.annotations.iter().find(|a| a.id == n) {
                let (x, y) = (a.x + delta.x, a.y + delta.y);
                app.run("note.move", json!({"id": n, "x": x, "y": y, "transient": true}));
            }
        } else if app.ui.canvas.panning {
            app.ui.pan += resp.drag_delta();
        }
    }
    if resp.drag_stopped() {
        // Snap to half the grid so labs stay tidy.
        let snap = |v: f32| (v / (GRID / 2.0)).round() * (GRID / 2.0);
        let moved: Vec<DeviceId> =
            if app.ui.canvas.drag_group { app.ui.picked.iter().copied().collect() } else { app.ui.canvas.drag.into_iter().collect() };
        for d in moved {
            if let Some(dev) = app.session.topology.device(d) {
                let (x, y) = (snap(dev.x), snap(dev.y));
                app.run("device.move", json!({"device": d.0, "x": x, "y": y, "transient": true}));
            }
        }
        let c = &mut app.ui.canvas;
        c.drag = None;
        c.drag_note = None;
        c.drag_cluster = None;
        c.drag_group = false;
        c.panning = false;
        if let (Some(a), Some(b)) = (c.band.take(), pointer) {
            finish_band(app, view, a, b);
        }
    }

    if resp.double_clicked()
        && let Some(d) = pointer.and_then(|p| hit_device(app, view, p))
    {
        console::open(app, d);
        return;
    }
    if resp.double_clicked()
        && let Some(c) = pointer.and_then(|p| hit_cluster(app, view, p))
    {
        app.enter_cluster(c);
        return;
    }
    if resp.double_clicked()
        && let Some(n) = pointer.and_then(|p| hit_note(app, view, p))
        && app.session.topology.annotations.iter().any(|a| a.id == n && a.shape == Shape::Text)
    {
        let text = app.session.topology.annotations.iter().find(|a| a.id == n).map(|a| a.text.clone()).unwrap_or_default();
        app.ui.editing_note = Some((n, text));
        return;
    }

    if resp.secondary_clicked() {
        let c = &mut app.ui.canvas;
        (c.context, c.context_cluster, c.context_note) = (None, None, None);
        if let Some(p) = pointer {
            if let Some(d) = hit_device(app, view, p) {
                app.ui.canvas.context = Some(d);
                if !app.ui.picked.contains(&d) {
                    app.ui.picked.clear();
                    app.ui.picked_clusters.clear();
                    app.ui.selected = Some(d);
                }
            } else if let Some(cl) = hit_cluster(app, view, p) {
                app.ui.canvas.context_cluster = Some(cl);
            } else {
                app.ui.canvas.context_note = hit_note(app, view, p);
            }
        }
    }
    if let Some(d) = app.ui.canvas.context {
        resp.context_menu(|ui| context_menu(app, ui, d));
    } else if let Some(c) = app.ui.canvas.context_cluster {
        resp.context_menu(|ui| cluster_menu(app, ui, c));
    } else if let Some(n) = app.ui.canvas.context_note {
        resp.context_menu(|ui| note_menu(app, ui, n));
    }

    if !resp.clicked() {
        return;
    }
    let Some(p) = pointer else { return };
    app.ui.canvas.port_menu = None;
    let hit = hit_device(app, view, p);
    match app.ui.tool {
        Tool::Select if shift => {
            // Shift+click adds to (or takes from) the picked devices and clusters.
            if let Some(s) = app.ui.selected.take() {
                app.ui.picked.insert(s);
            }
            if let Some(d) = hit {
                if !app.ui.picked.remove(&d) {
                    app.ui.picked.insert(d);
                }
            } else if let Some(c) = hit_cluster(app, view, p)
                && !app.ui.picked_clusters.remove(&c)
            {
                app.ui.picked_clusters.insert(c);
            }
        }
        Tool::Select => {
            // One click opens the device, as learners expect; dragging moves it instead.
            app.ui.picked.clear();
            app.ui.picked_clusters.clear();
            app.ui.selected = hit;
            if let Some(d) = hit {
                console::open(app, d);
            } else if let Some(c) = hit_cluster(app, view, p) {
                app.ui.picked_clusters.insert(c);
            }
        }
        Tool::Draw(_) => {}
        Tool::Inspect => {
            if hit.is_some() {
                app.ui.inspect = hit;
            }
        }
        Tool::Note => {
            if let Some(n) = hit_note(app, view, p) {
                let text = app.session.topology.annotations.iter().find(|a| a.id == n).map(|a| a.text.clone()).unwrap_or_default();
                app.ui.editing_note = Some((n, text));
            } else if hit.is_none() {
                let (x, y) = view.world(p);
                if let Some(v) = app.run("note.add", json!({"x": x, "y": y, "text": "Note"}))
                    && let Some(id) = v.get("id").and_then(Value::as_u64).and_then(|i| u32::try_from(i).ok())
                {
                    app.ui.editing_note = Some((id, String::new()));
                }
            }
        }
        Tool::Place(model) => {
            if hit.is_none() {
                let (x, y) = view.world(p);
                let snap = |v: f32| (v / (GRID / 2.0)).round() * (GRID / 2.0);
                if let Some(v) = app.run("device.add", json!({"model": model, "x": snap(x), "y": snap(y)})) {
                    app.ui.selected = v.get("id").and_then(Value::as_u64).and_then(|i| u32::try_from(i).ok()).map(DeviceId);
                }
            }
        }
        Tool::Cable(Cable::Console) => match hit {
            Some(d) => console_click(app, d),
            None => app.ui.canvas.cancel(),
        },
        Tool::Cable(_) => match hit {
            Some(d) => app.ui.canvas.port_menu = Some((d, p)),
            None => app.ui.canvas.cancel(),
        },
        Tool::Ping => match (app.ui.canvas.pending, hit) {
            (Some(Pending::PingFrom(src)), Some(dst)) if src != dst => {
                app.ui.canvas.pending = None;
                simple_pdu(app, src, dst, None);
            }
            (_, Some(src)) => app.ui.canvas.pending = Some(Pending::PingFrom(src)),
            (_, None) => app.ui.canvas.cancel(),
        },
        Tool::Delete => {
            if let Some(d) = hit {
                app.run("device.remove", json!({"device": d.0}));
                if app.ui.selected == Some(d) {
                    app.ui.selected = None;
                }
                app.ui.picked.remove(&d);
            } else if let Some(c) = hit_cluster(app, view, p) {
                // Deleting a cluster ungroups it; its devices stay.
                app.run("cluster.dissolve", json!({"id": c}));
                app.ui.picked_clusters.remove(&c);
            } else if let Some(n) = hit_note(app, view, p) {
                app.run("note.remove", json!({"id": n}));
            } else if let Some(end) = hit_link(app, view, p) {
                let port = app.session.topology.device(end.device).and_then(|d| d.ports.get(end.port)).map(|p| p.name.clone()).unwrap_or_default();
                app.run("link.remove", json!({"device": end.device.0, "port": port}));
            } else if let Some(pc) = hit_console(app, view, p) {
                app.run("link.remove", json!({"device": pc.0, "port": packetcraft_model::cable::RS232_PORT}));
            }
        }
    }
}

/// A cluster's right-click menu.
fn cluster_menu(app: &mut PacketApp, ui: &mut Ui, c: u32) {
    let Some(name) = app.session.topology.cluster(c).map(|x| x.name.clone()) else {
        ui.close();
        return;
    };
    ui.label(egui::RichText::new(&name).strong());
    if ui.button(tl("Open Cluster")).clicked() {
        app.enter_cluster(c);
        ui.close();
    }
    if ui.button(tl("Rename Cluster…")).clicked() {
        app.ui.renaming_cluster = Some((c, name));
        ui.close();
    }
    if ui.button(tl("Uncluster")).clicked() {
        app.run("cluster.dissolve", json!({"id": c}));
        app.ui.picked_clusters.remove(&c);
        ui.close();
    }
}

/// Colours offered for drawings: name and `#rrggbb`.
const NAMED_COLORS: [(&str, &str); 6] =
    [("Blue", "#bcd9f2"), ("Green", "#c8ecc0"), ("Yellow", "#fde9a8"), ("Red", "#f6c6c6"), ("Purple", "#d9cff2"), ("Grey", "#d5d8dc")];

/// A note's or drawing's right-click menu.
fn note_menu(app: &mut PacketApp, ui: &mut Ui, n: u32) {
    let Some(shape) = app.session.topology.annotations.iter().find(|a| a.id == n).map(|a| a.shape) else {
        ui.close();
        return;
    };
    if shape != Shape::Text {
        ui.label(tl("Colour"));
        ui.horizontal(|ui| {
            for (name, hex) in NAMED_COLORS {
                let [r, g, b] = packetcraft_model::parse_color(hex).unwrap_or([0xbc, 0xd9, 0xf2]);
                if ui
                    .add(egui::Button::new(egui::RichText::new(tl(name)).color(egui::Color32::BLACK)).fill(egui::Color32::from_rgb(r, g, b)))
                    .clicked()
                {
                    app.run("note.color", json!({"id": n, "color": hex}));
                    ui.close();
                }
            }
        });
    }
    if ui.button(tl("Delete")).clicked() {
        app.run("note.remove", json!({"id": n}));
        ui.close();
    }
}

/// The Rename Cluster dialog.
pub fn cluster_rename_window(app: &mut PacketApp, ctx: &egui::Context) {
    let Some((id, mut name)) = app.ui.renaming_cluster.clone() else { return };
    let mut open = true;
    let mut ok = false;
    egui::Window::new(tl("Rename Cluster")).id(egui::Id::unique("cluster_rename")).open(&mut open).collapsible(false).resizable(false).show(
        ctx,
        |ui| {
            let l = ui.label(tl("Cluster name:"));
            let r = ui.add(egui::TextEdit::singleline(&mut name)).labelled_by(l.id);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            if ui.button(tl("OK")).clicked() {
                ok = true;
            }
        },
    );
    if ok {
        app.run("cluster.rename", json!({"id": id, "name": name}));
        app.ui.renaming_cluster = None;
    } else if open {
        app.ui.renaming_cluster = Some((id, name));
    } else {
        app.ui.renaming_cluster = None;
    }
}

const PDU_COLORS: [egui::Color32; 6] = [
    egui::Color32::from_rgb(0x8e, 0x44, 0xad),
    egui::Color32::from_rgb(0x27, 0xae, 0x60),
    egui::Color32::from_rgb(0xe6, 0x7e, 0x22),
    egui::Color32::from_rgb(0x29, 0x80, 0xb9),
    egui::Color32::from_rgb(0xc0, 0x39, 0x2b),
    egui::Color32::from_rgb(0x16, 0xa0, 0x85),
];

/// Sends one echo request from `src` to `dst` (a simple PDU), recording it in the PDU list
/// (`row`: an existing row to update, else a new one).
fn simple_pdu(app: &mut PacketApp, src: DeviceId, dst: DeviceId, row: Option<usize>) {
    let t = &app.session.topology;
    let (Some(s), Some(d)) = (t.device(src), t.device(dst)) else { return };
    let (sname, dname) = (s.name.clone(), d.name.clone());
    let Some(addr) = first_address(d) else {
        app.ui.status = Some((crate::i18n::trf("{} has no IP address to send a PDU to", &[&dname]), true));
        return;
    };
    let first_event = app.session.sim.trace().back().map_or(0, |e| e.id);
    let sent_at = app.session.sim.now_us() as f64 / 1e6;
    let Some(v) = app.run("net.ping", json!({"from": src.0, "to": addr, "count": 1})) else { return };
    let result = v.pointer("/probes/0/result").and_then(Value::as_str).unwrap_or("timeout").to_string();
    let ok = result == "reply";
    let msg = match result.as_str() {
        "reply" => crate::i18n::trf("{} to {} ({}): successful", &[&sname, &dname, &addr]),
        "unreachable" => crate::i18n::trf("{} to {} ({}): failed, destination unreachable", &[&sname, &dname, &addr]),
        "ttl-exceeded" => crate::i18n::trf("{} to {} ({}): failed, TTL ran out", &[&sname, &dname, &addr]),
        _ => crate::i18n::trf("{} to {} ({}): failed, no answer", &[&sname, &dname, &addr]),
    };
    app.ui.status = Some((msg, !ok));
    match row.and_then(|i| app.ui.pdus.get_mut(i)) {
        Some(p) => {
            p.ok = Some(ok);
            p.time_s = sent_at;
        }
        None => {
            let num = app.ui.pdus.iter().map(|p| p.num).max().unwrap_or(0) + 1;
            let color = PDU_COLORS.get((num as usize) % PDU_COLORS.len()).copied().unwrap_or(egui::Color32::GRAY);
            app.ui.pdus.push(Pdu { num, source: src, destination: dst, address: addr, ok: Some(ok), color, time_s: sent_at });
        }
    }
    // In simulation mode, replay what just happened.
    if app.ui.mode == Mode::Simulation {
        app.ui.events.start_from(first_event + 1);
    }
}

#[cfg(test)]
pub fn simple_pdu_for_tests(app: &mut PacketApp, src: DeviceId, dst: DeviceId) {
    simple_pdu(app, src, dst, None);
}

/// Sends the PDU in row `i` of the list again.
pub fn fire(app: &mut PacketApp, i: usize) {
    if let Some(p) = app.ui.pdus.get(i) {
        let (s, d) = (p.source, p.destination);
        simple_pdu(app, s, d, Some(i));
    }
}

/// The Inspect tool's window: a device's dynamic tables.
pub fn inspect_window(app: &mut PacketApp, ctx: &egui::Context) {
    let Some(d) = app.ui.inspect else { return };
    let Some(dev) = app.session.topology.device(d) else {
        app.ui.inspect = None;
        return;
    };
    let mut open = true;
    let t = &app.session.topology;
    let st = app.session.sim.state(d);
    egui::Window::new(crate::i18n::trf("Inspect {}", &[&dev.name]))
        .id(egui::Id::unique(("inspect", d.0)))
        .open(&mut open)
        .default_size([460.0, 300.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if dev.kind == packetcraft_model::DeviceKind::Switch {
                    ui.strong("MAC Table");
                    egui::Grid::new(("mac", d.0)).striped(true).num_columns(3).show(ui, |ui| {
                        for h in ["VLAN", "MAC Address", "Port"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for ((vlan, mac), e) in st.map(|s| s.macs.iter().collect::<Vec<_>>()).unwrap_or_default() {
                            ui.label(vlan.to_string());
                            ui.monospace(mac.dotted());
                            ui.label(dev.ports.get(e.port).map_or("?".to_string(), |p| p.name.clone()));
                            ui.end_row();
                        }
                    });
                } else {
                    ui.strong("ARP Table");
                    egui::Grid::new(("arp", d.0)).striped(true).num_columns(3).show(ui, |ui| {
                        for h in ["IP Address", "MAC Address", "Interface"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for (ip, e) in st.map(|s| s.arp.iter().collect::<Vec<_>>()).unwrap_or_default() {
                            ui.label(ip.to_string());
                            ui.monospace(e.mac.to_string());
                            ui.label(&e.iface);
                            ui.end_row();
                        }
                    });
                    ui.add_space(8.0);
                    ui.strong("Routing Table");
                    egui::Grid::new(("routes", d.0)).striped(true).num_columns(4).show(ui, |ui| {
                        for h in ["Type", "Network", "Next Hop", "Interface"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for r in packetcraft_sim::routing_table(t, dev) {
                            ui.label(match r.code {
                                'C' => "Connected",
                                'L' => "Local",
                                _ => "Static",
                            });
                            ui.label(r.net.to_string());
                            ui.label(r.via.map_or("-".to_string(), |v| v.to_string()));
                            ui.label(r.iface.unwrap_or_else(|| "-".into()));
                            ui.end_row();
                        }
                    });
                }
            });
        });
    if !open {
        app.ui.inspect = None;
    }
}

fn context_menu(app: &mut PacketApp, ui: &mut Ui, d: DeviceId) {
    let Some(dev) = app.session.topology.device(d) else {
        ui.close();
        return;
    };
    let cabled: Vec<(String, String)> = dev
        .ports
        .iter()
        .enumerate()
        .filter_map(|(i, p)| app.session.topology.peer(End { device: d, port: i }).map(|peer| (p.name.clone(), app.session.topology.end_label(peer))))
        .collect();
    ui.label(egui::RichText::new(&dev.name).strong());
    if ui.button(tl("Open console")).clicked() {
        console::open(app, d);
        ui.close();
    }
    if !cabled.is_empty() {
        ui.menu_button(tl("Disconnect"), |ui| {
            for (port, peer) in &cabled {
                if ui.button(format!("{port}  (to {peer})")).clicked() {
                    app.run("link.remove", json!({"device": d.0, "port": port}));
                    ui.close();
                }
            }
        });
    }
    if ui.button(tl("Group into Cluster")).clicked() {
        app.ui.picked.insert(d);
        app.group_selection();
        ui.close();
    }
    if ui.button(tl("Delete")).clicked() {
        app.run("device.remove", json!({"device": d.0}));
        app.ui.selected = None;
        ui.close();
    }
}

/// The console cable tool: each device has one console-cable port, so a click picks it.
fn console_click(app: &mut PacketApp, d: DeviceId) {
    let Some(dev) = app.session.topology.device(d) else { return };
    if dev.console_port_name().is_none() {
        let msg = crate::i18n::trf("{} has no Console or RS-232 port.", &[&dev.name]);
        app.ui.status = Some((msg, true));
        return;
    }
    match app.ui.canvas.pending {
        Some(Pending::ConsoleCable(a)) if a != d => {
            app.ui.canvas.pending = None;
            if let Some(v) = app.run("link.add", json!({"a": a.0, "b": d.0, "cable": "console"})) {
                let msg = format!("Connected {} and {}", v["a"].as_str().unwrap_or(""), v["b"].as_str().unwrap_or(""));
                app.info(msg);
            }
        }
        _ => app.ui.canvas.pending = Some(Pending::ConsoleCable(d)),
    }
}

/// The port chooser of the cable tool.
fn port_menu(app: &mut PacketApp, ui: &Ui) {
    let Some((d, at)) = app.ui.canvas.port_menu else { return };
    let Some(dev) = app.session.topology.device(d).cloned() else {
        app.ui.canvas.port_menu = None;
        return;
    };
    let mut chosen = None;
    let mut close = false;
    // A port the cable doesn't fit (or, for the second end, that doesn't fit the first) is
    // greyed out.
    let kind = match app.ui.tool {
        Tool::Cable(k) => k,
        _ => Cable::Auto,
    };
    let first = match app.ui.canvas.pending {
        Some(Pending::CableFrom(a)) if a.device != d => Some(a),
        _ => None,
    };
    egui::Area::new(Id::unique("port_menu")).fixed_pos(at + vec2(8.0, 8.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new(crate::i18n::trf("{}: choose a port", &[&dev.name])).strong());
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (i, p) in dev.ports.iter().enumerate() {
                    let end = End { device: d, port: i };
                    match app.session.topology.peer(end) {
                        Some(peer) => {
                            ui.add_enabled(false, egui::Button::new(format!("{}  (to {})", p.name, app.session.topology.end_label(peer))));
                        }
                        None if app.session.topology.cable_misfit(kind, first.unwrap_or(end), end).is_some() => {
                            let why = app.session.topology.cable_misfit(kind, first.unwrap_or(end), end).unwrap_or_default();
                            ui.add_enabled(false, egui::Button::new(format!("{}  ({})", p.name, crate::i18n::tl("other cable type"))))
                                .on_disabled_hover_text(why);
                        }
                        None => {
                            if ui.button(&p.name).clicked() {
                                chosen = Some(end);
                            }
                        }
                    }
                }
            });
            if ui.button(tl("Cancel")).clicked() {
                close = true;
            }
        });
    });
    if close {
        app.ui.canvas.cancel();
    }
    let Some(end) = chosen else { return };
    app.ui.canvas.port_menu = None;
    match app.ui.canvas.pending {
        Some(Pending::CableFrom(a)) if a.device != end.device => {
            app.ui.canvas.pending = None;
            let port = |e: End| app.session.topology.device(e.device).and_then(|d| d.ports.get(e.port)).map(|p| p.name.clone()).unwrap_or_default();
            let (pa, pb) = (port(a), port(end));
            if let Some(v) = app.run("link.add", json!({"a": a.device.0, "a_port": pa, "b": end.device.0, "b_port": pb, "cable": kind.id()})) {
                let msg = format!("Connected {} and {}", v["a"].as_str().unwrap_or(""), v["b"].as_str().unwrap_or(""));
                app.info(msg);
            }
        }
        _ => app.ui.canvas.pending = Some(Pending::CableFrom(end)),
    }
}

#[cfg(test)]
pub fn triangulate_for_tests(pts: &[Pos2]) -> Vec<[u32; 3]> {
    triangulate(pts)
}
