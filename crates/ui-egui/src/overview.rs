//! View ▸ Show Viewport (a small map of the workspace you click to pan) and View ▸ Show Workspace List (every
//! place, furniture and device with its name, model and type, as a tree).

use crate::prefs_look::HintExt;
use egui::{Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use std::collections::BTreeSet;

use egui::Ui;
use netlabcraft_core::DeviceId;
use netlabcraft_model::furniture::Item;
use netlabcraft_model::{Shown, Topology};
use serde_json::json;

use crate::i18n::{tl, tr};
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
        let resp = resp.hint(tl("Click or drag to move the view of the workspace"));
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

/// What a row of the workspace list stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListPick {
    Place(u32),
    Furniture(u32),
    Passive(u32),
    Device(DeviceId),
}

/// The tree being drawn: what was clicked and double-clicked this frame.
struct Walk<'a> {
    t: &'a Topology,
    selected: Option<DeviceId>,
    pick: Option<ListPick>,
    clicked: Option<ListPick>,
    opened: Option<ListPick>,
    shown: BTreeSet<DeviceId>,
}

impl Walk<'_> {
    /// One row: a label that a click selects and a double click opens, and a note beside it.
    fn row(&mut self, ui: &mut Ui, what: ListPick, label: &str, note: String) {
        let on = match what {
            ListPick::Device(d) => self.selected == Some(d),
            other => self.pick == Some(other),
        };
        let r = ui.selectable_label(on, label);
        if !note.is_empty() {
            ui.weak(note);
        }
        if r.double_clicked() {
            self.opened = Some(what);
        } else if r.clicked() {
            self.clicked = Some(what);
        }
    }

    /// A row with an arrow that folds what is inside it away.
    fn branch(&mut self, ui: &mut Ui, key: (u8, u32), what: ListPick, label: &str, note: String, body: impl FnOnce(&mut Self, &mut Ui)) {
        let id = ui.make_persistent_id(("workspace_list", key.0, key.1));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| self.row(ui, what, label, note))
            .body(|ui| body(self, ui));
    }

    fn device(&mut self, ui: &mut Ui, id: DeviceId) {
        let Some(d) = self.t.device(id) else { return };
        self.shown.insert(id);
        let note = format!("{} · {}", tl(d.catalogue_model().label), tl(d.kind.label()));
        self.row(ui, ListPick::Device(id), d.display_name(), note);
    }

    fn item(&mut self, ui: &mut Ui, item: Item) {
        match item {
            Item::Device(d) => {
                ui.horizontal(|ui| self.device(ui, d));
            }
            Item::Fitting(i) => {
                let Some(f) = self.t.fitting(i) else { return };
                let (name, note) = (f.name.clone(), tl(f.kind.label()).to_string());
                ui.horizontal(|ui| self.row(ui, ListPick::Passive(i), &name, note));
            }
        }
    }

    /// A place with the places inside it, then (a closet or a container) its furniture with what
    /// stands on it, then what stands loose.
    fn place(&mut self, ui: &mut Ui, id: u32) {
        let Some(place) = self.t.place(id) else { return };
        let t = self.t;
        let name = tr(&place.name).to_string();
        self.branch(ui, (0, id), ListPick::Place(id), &name, tl(place.kind.label()).to_string(), |w, ui| {
            for child in t.places_in(Some(id)) {
                w.place(ui, child.id);
            }
            if !place.kind.holds_things() {
                return;
            }
            for f in t.furniture_in(id) {
                w.branch(ui, (1, f.id), ListPick::Furniture(f.id), &f.name, tl(f.kind.label()).to_string(), |w, ui| {
                    for m in t.mounts_on(f.id) {
                        w.item(ui, m.item);
                    }
                });
            }
            for placed in t.closet_layout(id).items.iter().filter(|p| p.furniture.is_none()) {
                w.item(ui, placed.item);
            }
        });
    }
}

/// The workspace list window: a tree of places, the furniture in them and the devices on that
/// furniture (the ones standing loose in a place come last). A click selects a row; a double click
/// opens it: a device's window, or the place (a furniture's or a passive device's own).
pub fn list_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.show_workspace_list {
        return;
    }
    let mut open = true;
    let (mut clicked, mut opened) = (None, None);
    egui::Window::new(tl("Workspace List")).open(&mut open).default_width(420.0).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        let t = &app.session.topology;
        if t.devices.is_empty() && t.physical.furniture.is_empty() {
            ui.label(tl("Nothing on the workspace yet."));
            return;
        }
        let mut walk = Walk { t, selected: app.ui.selected, pick: app.ui.list_pick, clicked: None, opened: None, shown: BTreeSet::new() };
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for root in t.places_in(None) {
                walk.place(ui, root.id);
            }
            // A device no place holds (never expected) is still listed.
            let rest: Vec<DeviceId> = t.devices.iter().map(|d| d.id).filter(|d| !walk.shown.contains(d)).collect();
            if !rest.is_empty() {
                ui.strong(tl("Other devices"));
                for d in rest {
                    ui.horizontal(|ui| walk.device(ui, d));
                }
            }
        });
        ui.weak(tl("Click a row to select it; double-click to open it."));
        (clicked, opened) = (walk.clicked, walk.opened);
    });
    app.ui.show_workspace_list = open;
    if let Some(what) = clicked.or(opened) {
        match what {
            ListPick::Device(d) => app.ui.selected = Some(d),
            other => app.ui.list_pick = Some(other),
        }
    }
    let place_of = |what: ListPick| match what {
        ListPick::Place(p) => Some(p),
        ListPick::Furniture(f) => app.session.topology.furniture_piece(f).map(|x| x.place),
        ListPick::Passive(i) => app.session.topology.fitting(i).map(|x| x.place),
        ListPick::Device(_) => None,
    };
    match opened {
        Some(ListPick::Device(d)) => crate::open_console(app, d),
        Some(what) => {
            if let Some(place) = place_of(what) {
                app.run("view.enter_place", json!({"id": place}));
            }
        }
        None => {}
    }
}
