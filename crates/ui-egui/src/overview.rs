//! View ▸ Show Viewport (a small map of the workspace you click to pan) and View ▸ Show Workspace List (every
//! device with its name, model and type).

use egui::{Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use netlabcraft_model::Shown;

use crate::i18n::tl;
use crate::{PacketApp, theme};

/// The map's size on screen.
const MAP: egui::Vec2 = vec2(220.0, 150.0);

/// Workspace units kept around the outermost device so it does not sit on the map's edge.
const MARGIN: f32 = 60.0;

/// The part of the logical workspace the canvas shows now (workspace units), when its size is known.
fn visible(app: &PacketApp) -> Option<Rect> {
    let size = app.ui.canvas.size;
    let zoom = app.ui.zoom;
    if size.x < 1.0 || size.y < 1.0 || !(zoom.is_finite() && zoom > 0.0) {
        return None;
    }
    Some(Rect::from_min_size(pos2(-app.ui.pan.x / zoom, -app.ui.pan.y / zoom), size / zoom))
}

/// What the map shows: each device and cluster of the level on view, as a point in workspace units.
fn points(app: &PacketApp) -> (Vec<egui::Pos2>, Vec<egui::Pos2>) {
    let t = &app.session.topology;
    let level = app.session.view;
    let shown = t.shown_map(level);
    let devices = t.devices.iter().filter(|d| shown.get(&d.id) == Some(&Shown::Device)).map(|d| pos2(d.x, d.y)).collect();
    let clusters = t.clusters_in(level).map(|c| pos2(c.x, c.y)).collect();
    (devices, clusters)
}

/// The viewport window: the devices as dots and the visible part as a frame; a click or a drag centres the
/// workspace on that point.
pub fn viewport_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.show_viewport {
        return;
    }
    let mut open = true;
    let corner = app.ui.canvas_rect.map_or(pos2(900.0, 200.0), |r| r.right_top() + vec2(-12.0, 12.0));
    egui::Window::new(tl("Viewport")).open(&mut open).resizable(false).pivot(egui::Align2::RIGHT_TOP).default_pos(corner).show(ctx, |ui| {
        if app.session.physical {
            ui.label(tl("The viewport shows the logical view."));
            return;
        }
        let tk = theme::tokens(ui.visuals().dark_mode);
        let (devices, clusters) = points(app);
        let view = visible(app);
        // The map covers every device and the part on view, so the frame is always inside it.
        let mut bounds = view.unwrap_or(Rect::NOTHING);
        for p in devices.iter().chain(&clusters) {
            bounds = bounds.union(Rect::from_min_size(*p, egui::Vec2::ZERO));
        }
        if !bounds.is_finite() {
            ui.label(tl("Nothing on the workspace yet."));
            return;
        }
        let bounds = bounds.expand(MARGIN);
        let (resp, painter) = ui.allocate_painter(MAP, Sense::click_and_drag());
        let rect = resp.rect;
        let scale = (rect.width() / bounds.width().max(1.0)).min(rect.height() / bounds.height().max(1.0));
        let to_map = |p: egui::Pos2| rect.center() + (p - bounds.center()) * scale;
        painter.rect_filled(rect, 2.0, tk.canvas);
        painter.rect_stroke(rect, 2.0, Stroke::new(1.0, tk.chrome_line), StrokeKind::Inside);
        for p in &clusters {
            painter.rect_filled(Rect::from_center_size(to_map(*p), vec2(7.0, 5.0)), 1.0, tk.cluster);
        }
        for p in &devices {
            painter.circle_filled(to_map(*p), 2.5, tk.glyph);
        }
        if let Some(v) = view {
            let frame = Rect::from_min_max(to_map(v.min), to_map(v.max));
            painter.rect_filled(frame, 0.0, tk.selection.gamma_multiply(0.12));
            painter.rect_stroke(frame, 0.0, Stroke::new(1.5, tk.selection), StrokeKind::Inside);
        }
        let resp = resp.on_hover_text(tl("Click or drag to move the view of the workspace"));
        if (resp.clicked() || resp.dragged())
            && let Some(at) = resp.interact_pointer_pos()
            && scale > 0.0
        {
            let w = bounds.center() + (at - rect.center()) / scale;
            app.ui.pan = app.ui.canvas.size / 2.0 - vec2(w.x, w.y) * app.ui.zoom;
        }
    });
    app.ui.show_viewport = open;
}

/// The workspace list window: one row per device. A click selects the device, a double click opens its window.
pub fn list_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.show_workspace_list {
        return;
    }
    let mut open = true;
    let mut picked = None;
    let mut opened = None;
    egui::Window::new(tl("Workspace List")).open(&mut open).default_width(380.0).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        let t = &app.session.topology;
        if t.devices.is_empty() {
            ui.label(tl("Nothing on the workspace yet."));
            return;
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            egui::Grid::new("workspace_list_grid").striped(true).num_columns(3).spacing([18.0, 4.0]).show(ui, |ui| {
                for head in ["Name", "Model", "Type"] {
                    ui.strong(tl(head));
                }
                ui.end_row();
                for d in &t.devices {
                    let row = ui.selectable_label(app.ui.selected == Some(d.id), d.display_name());
                    ui.label(tl(d.catalogue_model().label));
                    ui.label(tl(d.kind.label()));
                    ui.end_row();
                    if row.double_clicked() {
                        opened = Some(d.id);
                    } else if row.clicked() {
                        picked = Some(d.id);
                    }
                }
            });
        });
        ui.weak(tl("Click a device to select it; double-click to open it."));
    });
    app.ui.show_workspace_list = open;
    if let Some(d) = picked.or(opened) {
        app.ui.selected = Some(d);
    }
    if let Some(d) = opened {
        crate::open_console(app, d);
    }
}
