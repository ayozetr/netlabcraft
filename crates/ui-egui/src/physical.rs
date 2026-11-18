//! The physical workspace: the Intercity map with its cities, a city's buildings, a building's
//! wiring closets and rooms, and the devices that stand in each. Cables are drawn between what
//! stands for their ends at the level shown, with their lengths; one too long for copper is red.
//! Everything acts through engine commands (`place.*`, `device.place`, `link.length`, `view.*`).

use std::collections::BTreeMap;

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use netlabcraft_core::DeviceId;
use netlabcraft_model::End;
use netlabcraft_model::physical::{Place, PlaceKind};
use serde_json::{Value, json};

use crate::i18n::{tl, tr, trf};
use crate::theme::{self, Tokens};
use crate::{PacketApp, Tool, icons};

/// A place's icon size, and a device's, in map units.
const PLACE_ICON: f32 = 84.0;
const DEVICE_ICON: f32 = 46.0;

/// What a right-click opened a menu on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Place(u32),
    Device(DeviceId),
    Cable(End),
    Empty(f32, f32),
}

/// The physical view's own state (drags, open menu, dialogs).
#[derive(Debug, Clone, Default)]
pub struct PhysState {
    drag_place: Option<u32>,
    drag_device: Option<DeviceId>,
    /// Where the dragged place or device would stand without aligning (workspace units).
    drag_raw: (f32, f32),
    panning: bool,
    /// When (input time) a click last opened a place, so the second click of a double click does not
    /// also open whatever stands under the pointer on the new map.
    entered: f64,
    context: Option<Target>,
    /// The Rename Place dialog: place and the name being typed.
    pub renaming: Option<(u32, String)>,
    /// The Cable Length dialog: one end of the cable and the metres being typed.
    pub length: Option<(End, String)>,
    /// The Grid Preferences window.
    pub(crate) grid: crate::physical_grid::State,
    /// The wiring-closet view's state (furniture, drags, the Rename dialog).
    pub(crate) closet: crate::closet::State,
    /// The `+` menu of the bar.
    pub(crate) menu: crate::physical_bar::MenuState,
}

/// What stands for a device at the level shown: itself, or the place (city, building or closet)
/// on this map that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Place(u32),
    Device(DeviceId),
}

struct View {
    origin: Vec2,
    zoom: f32,
    level: Option<u32>,
    places: Vec<Place>,
    /// Devices standing right here, with their positions.
    devices: Vec<(DeviceId, f32, f32)>,
    /// How every device appears here (absent: elsewhere).
    shown: BTreeMap<DeviceId, Node>,
}

impl View {
    fn new(app: &PacketApp, origin: Vec2) -> View {
        let t = &app.session.topology;
        let level = app.session.place_view;
        let mut paths: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let mut shown = BTreeMap::new();
        let mut devices = Vec::new();
        for h in t.homes() {
            let path = paths.entry(h.place).or_insert_with(|| t.place_path(Some(h.place)).iter().map(|p| p.id).collect());
            let node = match level {
                None => path.first().map(|c| Node::Place(*c)),
                Some(v) => match path.iter().position(|p| *p == v) {
                    Some(i) if i + 1 == path.len() => Some(Node::Device(h.device)),
                    Some(i) => path.get(i + 1).map(|c| Node::Place(*c)),
                    None => None,
                },
            };
            if let Some(n) = node {
                if n == Node::Device(h.device) {
                    devices.push((h.device, h.x, h.y));
                }
                shown.insert(h.device, n);
            }
        }
        View { origin, zoom: app.ui.zoom, level, places: t.places_in(level), devices, shown }
    }

    fn screen(&self, x: f32, y: f32) -> Pos2 {
        pos2(x * self.zoom, y * self.zoom) + self.origin
    }

    fn world(&self, p: Pos2) -> (f32, f32) {
        let v = (p - self.origin) / self.zoom;
        (v.x, v.y)
    }

    fn place_rect(&self, p: &Place) -> Rect {
        Rect::from_center_size(self.screen(p.x, p.y), Vec2::splat(PLACE_ICON * self.zoom))
    }

    fn device_rect(&self, x: f32, y: f32) -> Rect {
        Rect::from_center_size(self.screen(x, y), Vec2::splat(DEVICE_ICON * self.zoom))
    }

    fn node_pos(&self, n: Node) -> Option<Pos2> {
        match n {
            Node::Place(id) => self.places.iter().find(|p| p.id == id).map(|p| self.screen(p.x, p.y)),
            Node::Device(d) => self.devices.iter().find(|x| x.0 == d).map(|x| self.screen(x.1, x.2)),
        }
    }

    fn hit_place(&self, at: Pos2) -> Option<u32> {
        self.places.iter().rev().find(|p| self.place_rect(p).expand(4.0).contains(at)).map(|p| p.id)
    }

    fn hit_device(&self, at: Pos2) -> Option<DeviceId> {
        self.devices.iter().rev().find(|d| self.device_rect(d.1, d.2).expand(4.0).contains(at)).map(|d| d.0)
    }
}

/// The cables drawn at this level: between two different stand-ins, grouped by pair.
struct Run {
    a: Pos2,
    b: Pos2,
    /// One end of each cable in the run, its length and whether it carries nothing.
    cables: Vec<(End, u32, bool)>,
    /// A cable leaving the place shown: where its far end stands (drawn as a short stub).
    exit: Option<String>,
}

fn runs(app: &PacketApp, view: &View) -> Vec<Run> {
    let t = &app.session.topology;
    let mut by_pair: BTreeMap<(Node, Node), Vec<(End, u32, bool)>> = BTreeMap::new();
    let mut exits: Vec<(Node, String, (End, u32, bool))> = Vec::new();
    for l in &t.links {
        let len = t.cable_length(l);
        let cable = (l.a, len.meters, netlabcraft_sim::cable_problem(t, l.a).is_some());
        match (view.shown.get(&l.a.device), view.shown.get(&l.b.device)) {
            (Some(na), Some(nb)) if na != nb => {
                let key = if na <= nb { (*na, *nb) } else { (*nb, *na) };
                by_pair.entry(key).or_default().push(cable);
            }
            (Some(n), None) | (None, Some(n)) => {
                let far = if view.shown.contains_key(&l.a.device) { l.b.device } else { l.a.device };
                let name = t.device(far).map(|d| d.name.clone()).unwrap_or_default();
                let place = t.home_of(far).and_then(|h| t.place(h.place)).map(|p| crate::i18n::tr(&p.name).to_string()).unwrap_or_default();
                exits.push((*n, format!("{name} ({place})"), cable));
            }
            _ => {}
        }
    }
    let mut out: Vec<Run> =
        by_pair.into_iter().filter_map(|((x, y), cables)| Some(Run { a: view.node_pos(x)?, b: view.node_pos(y)?, cables, exit: None })).collect();
    // Stubs fan out below each icon so several stay apart.
    let mut fan: BTreeMap<Node, usize> = BTreeMap::new();
    for (n, to, cable) in exits {
        let Some(a) = view.node_pos(n) else { continue };
        let k = fan.entry(n).or_default();
        // Below the icon, alternating right and left, clear of its label (degrees; y points down).
        const ANGLES: [f32; 8] = [55.0, 125.0, 25.0, 155.0, 75.0, 105.0, 5.0, 175.0];
        let angle = ANGLES.get(*k % ANGLES.len()).copied().unwrap_or(90.0).to_radians();
        *k += 1;
        let b = a + vec2(angle.cos(), angle.sin()) * 120.0 * view.zoom;
        out.push(Run { a, b, cables: vec![cable], exit: Some(to) });
    }
    out
}

/// A cable length for people: metres, or kilometres from 1 km; feet, or miles from 1 mile,
/// when the metric system is off (Preferences).
pub(crate) fn length_text(m: u32, metric: bool) -> String {
    if metric {
        return if m >= 1_000 { format!("{:.1} km", f64::from(m) / 1000.0) } else { format!("{m} m") };
    }
    let feet = f64::from(m) / 0.3048;
    if feet >= 5280.0 { format!("{:.1} mi", feet / 5280.0) } else { format!("{} ft", feet.round()) }
}

pub(crate) fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

fn hit_cable(runs: &[Run], at: Pos2) -> Option<End> {
    runs.iter().find(|r| segment_distance(at, r.a, r.b) < 6.0).and_then(|r| r.cables.first().map(|c| c.0))
}

/// The kind of place a map adds (none inside a closet).
pub fn child_kind(app: &PacketApp) -> Option<PlaceKind> {
    match app.session.place_view.and_then(|p| app.session.topology.place(p)) {
        None => Some(PlaceKind::City),
        Some(p) => p.kind.child_kind(),
    }
}

/// "New City", "New Building", "New Wiring Closet".
pub fn new_label(kind: PlaceKind) -> &'static str {
    match kind {
        PlaceKind::City => "New City",
        PlaceKind::Building => "New Building",
        PlaceKind::Closet => "New Wiring Closet",
        PlaceKind::Container => "New Container",
    }
}

/// Adds a place of the kind this map holds; `at` in map units (else the engine picks a spot).
pub fn add_place(app: &mut PacketApp, at: Option<(f32, f32)>) {
    let Some(kind) = child_kind(app) else { return };
    add_kind(app, kind, at);
}

/// Adds a place of a given kind in the place being looked at.
pub fn add_kind(app: &mut PacketApp, kind: PlaceKind, at: Option<(f32, f32)>) {
    let mut p = json!({"kind": kind.id(), "parent": app.session.place_view});
    if let Some((x, y)) = at {
        let (x, y) = crate::physical_grid::snap_xy(app, x, y);
        p["x"] = json!(x);
        p["y"] = json!(y);
    }
    if let Some(v) = app.run("place.add", p) {
        let name = v.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        app.info(trf("Added {}. Click it to open it.", &[&name]));
    }
}

/// Opens a place (click, menu, path bar).
pub fn enter(app: &mut PacketApp, id: u32) {
    if app.run("view.enter_place", json!({"id": id})).is_some() {
        fit(app);
    }
}

/// Up one level (`top`: to the Intercity map).
pub fn leave(app: &mut PacketApp, top: bool) {
    if app.run(if top { "view.intercity" } else { "view.place_up" }, json!({})).is_some() {
        fit(app);
    }
}

/// Pans so what this level shows sits in the middle.
pub fn fit(app: &mut PacketApp) {
    if let Some(place) = crate::closet::current(app) {
        crate::closet::fit(app, place);
        return;
    }
    let view = View::new(app, Vec2::ZERO);
    let mut pts: Vec<(f32, f32)> = view.places.iter().map(|p| (p.x, p.y)).chain(view.devices.iter().map(|d| (d.1, d.2))).collect();
    // A background picture is part of what the map shows.
    if let Some((w, h)) = crate::pictures::extent(app, view.level) {
        pts.extend([(0.0, 0.0), (w, h)]);
    }
    if pts.is_empty() || app.ui.canvas.size.x < 1.0 {
        return;
    }
    let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let center = vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    app.ui.pan = app.ui.canvas.size / 2.0 - center * app.ui.zoom;
}

/// The caption of the level shown.
fn caption(app: &PacketApp) -> String {
    match app.session.place_view.and_then(|p| app.session.topology.place(p)) {
        None => tl("Intercity: the cities of the lab").to_string(),
        Some(p) => format!("{}: {}", tr(p.kind.label()), tr(&p.name)),
    }
}

/// Draws the physical workspace and handles the pointer.
pub fn show(app: &mut PacketApp, ui: &mut Ui) {
    let tk = theme::tokens(app.ui.dark);
    let rect = ui.max_rect();
    app.ui.canvas_rect = Some(rect);
    app.ui.canvas.size = rect.size();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Canvas, true, tl("Physical workspace")));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, tk.ground);
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
    // A wiring closet is drawn by its own view: furniture, front panels and cables from port to port.
    if let Some(place) = crate::closet::current(app) {
        crate::closet::keep_in_view(app, place);
        crate::pictures::draw_place(app, ui.ctx(), &painter, rect.min.to_vec2() + app.ui.pan, app.ui.zoom, Some(place));
        crate::closet::show(app, ui, &resp, &painter, rect, place);
        crate::closet::title_plate(&painter, rect.min + vec2(12.0, 8.0), caption(app), &tk);
        return;
    }
    let view = View::new(app, rect.min.to_vec2() + app.ui.pan);
    app.ui.pointer = resp.hover_pos().map(|p| view.world(p));
    crate::pictures::draw_place(app, ui.ctx(), &painter, view.origin, view.zoom, view.level);

    // The grid of this level, when its box is ticked (Grid Preferences).
    crate::physical_grid::draw(app, &painter, rect, view.origin, view.zoom, &tk);

    if app.ui.prefs.show_wireless_signals {
        draw_wireless(app, &painter, &view, &tk);
    }
    let runs = runs(app, &view);
    draw_runs(&painter, &view, &runs, app.ui.prefs.metric, &tk);
    draw_places(app, ui.ctx(), &painter, &view, &tk);
    draw_devices(app, &painter, &view, &tk);
    painter.text(rect.min + vec2(12.0, 10.0), Align2::LEFT_TOP, caption(app), FontId::proportional(17.0), tk.muted);
    cable_popup(app, &resp, &runs);
    interact(app, ui, &resp, &view, &runs);
}

/// The cables of the run under the pointer, as a tooltip (Preferences ▸ Show Cable Info Popup).
fn cable_popup(app: &PacketApp, resp: &egui::Response, runs: &[Run]) {
    if !app.ui.prefs.cable_popup || app.ui.physical.drag_place.is_some() || app.ui.physical.drag_device.is_some() {
        return;
    }
    let Some(at) = resp.hover_pos() else { return };
    let Some(run) = runs.iter().find(|r| segment_distance(at, r.a, r.b) < 6.0) else { return };
    let t = &app.session.topology;
    let metric = app.ui.prefs.metric;
    resp.clone().on_hover_ui_at_pointer(|ui| {
        for (end, meters, bad) in run.cables.iter().take(8) {
            let Some(l) = t.link_at(*end) else { continue };
            let kind = crate::i18n::tr(t.resolved_cable(l).label());
            ui.label(format!("{} - {}", t.end_label(l.a), t.end_label(l.b)));
            ui.label(format!("{kind}, {}", length_text(*meters, metric)));
            if *bad && let Some(why) = netlabcraft_sim::cable_problem(t, l.a) {
                ui.label(egui::RichText::new(crate::i18n::tr_any(&why)).color(theme::tokens(app.ui.dark).error));
            }
        }
        if run.cables.len() > 8 {
            ui.label(crate::i18n::trf("and {} more", &[&(run.cables.len() - 8)]));
        }
    });
}

fn draw_runs(painter: &egui::Painter, view: &View, runs: &[Run], metric: bool, tk: &Tokens) {
    for r in runs {
        let bad = r.cables.iter().any(|c| c.2);
        let color = if bad { tk.link_down } else { tk.cable };
        let stroke = Stroke::new((2.0 * view.zoom).clamp(1.0, 4.0), color);
        let longest = r.cables.iter().map(|c| c.1).max().unwrap_or(0);
        let (text, mid) = match &r.exit {
            // A cable leaving this place: a dashed stub, labelled at its end with the far side.
            Some(to) => {
                painter.extend(egui::Shape::dashed_line(&[r.a, r.b], stroke, 6.0, 4.0));
                (trf("to {}: {}", &[to, &length_text(longest, metric)]), r.b)
            }
            None => {
                painter.line_segment([r.a, r.b], stroke);
                let text = if r.cables.len() == 1 {
                    length_text(longest, metric)
                } else {
                    trf("{} cables, up to {}", &[&r.cables.len(), &length_text(longest, metric)])
                };
                (text, r.a + (r.b - r.a) / 2.0)
            }
        };
        let font = FontId::proportional((14.0 * view.zoom).clamp(10.0, 20.0));
        let galley = painter.layout_no_wrap(text, font, color);
        let size = galley.size() + vec2(8.0, 4.0);
        // A stub's label hangs off its end, on the side it points to.
        let align = match r.exit {
            Some(_) if r.b.x > r.a.x + 1.0 => Align2::LEFT_CENTER,
            Some(_) if r.b.x < r.a.x - 1.0 => Align2::RIGHT_CENTER,
            Some(_) => Align2::CENTER_TOP,
            None => Align2::CENTER_CENTER,
        };
        let bg = align.anchor_size(mid, size);
        painter.rect_filled(bg, 4.0, tk.ground);
        painter.rect_stroke(bg, 4.0, Stroke::new(1.0, color), StrokeKind::Inside);
        painter.galley(bg.min + vec2(4.0, 2.0), galley, color);
    }
}

/// View Wireless Signals: each radio standing here covers a faint disc (metres on a map with a
/// layout, the logical range without one), tinted by its channel, and the hosts that are
/// associated with it (`wifi_ap_of`, walls included) get a thin line to it.
fn draw_wireless(app: &PacketApp, painter: &egui::Painter, view: &View, tk: &Tokens) {
    let t = &app.session.topology;
    let at = |id: DeviceId| view.devices.iter().find(|d| d.0 == id).map(|d| view.screen(d.1, d.2));
    let radios = crate::wireless::radios(t);
    for r in &radios {
        let Some(c) = at(r.device) else { continue };
        let reach = if t.implicit_layout() { r.units } else { r.metres / t.map_scale(view.level.unwrap_or(netlabcraft_model::backdrop::INTERCITY)) };
        crate::wireless::draw_area(
            painter,
            c,
            reach * view.zoom,
            crate::wireless::tint(r.channel),
            &r.ssid,
            tk.text,
            crate::wireless::Look::of(&app.ui.prefs),
        );
    }
    for (host, ap) in crate::wireless::associations(t) {
        let (Some(a), Some(b)) = (at(host), at(ap)) else { continue };
        let channel = radios.iter().find(|r| r.device == ap).map_or(6, |r| r.channel);
        crate::wireless::draw_line(painter, a, b, crate::wireless::tint(channel));
    }
}

fn draw_places(app: &mut PacketApp, ctx: &egui::Context, painter: &egui::Painter, view: &View, tk: &Tokens) {
    let edge = theme::shade(tk.place, if app.ui.dark { 0.45 } else { 0.5 });
    for p in &view.places {
        let r = view.place_rect(p);
        if app.ui.physical.drag_place == Some(p.id) {
            painter.rect_stroke(r.expand(6.0), 8.0, Stroke::new(1.5, tk.selection), StrokeKind::Outside);
        }
        // A picture chosen for the place (Set Background Image ▸ Place Icon) replaces the drawn icon.
        if !crate::pictures::draw_icon(app, ctx, painter, r, p.id, edge) {
            match p.kind {
                PlaceKind::City => icons::city(painter, r, tk.place, edge),
                PlaceKind::Building => icons::building(painter, r, tk.place, edge),
                PlaceKind::Closet => icons::closet(painter, r, tk.place, edge),
                PlaceKind::Container => icons::container(painter, r, tk.place, edge),
            }
        }
        let inside = view.shown.values().filter(|n| **n == Node::Place(p.id)).count();
        let font = FontId::proportional((16.0 * view.zoom).clamp(9.0, 24.0));
        let mut y = r.bottom() + 2.0;
        let count = if inside == 1 { tl("1 device").to_string() } else { trf("{} devices", &[&inside]) };
        for (line, color) in [(tr(&p.name).to_string(), tk.text), (count, tk.muted)] {
            let galley = painter.layout_no_wrap(line, font.clone(), color);
            let at = Align2::CENTER_TOP.anchor_size(pos2(r.center().x, y), galley.size());
            y = at.bottom();
            // On a soft plate of the ground colour, so a background picture does not hide the letters.
            painter.rect_filled(at.expand2(vec2(3.0, 0.0)), 4.0, tk.ground.gamma_multiply(0.82));
            painter.galley(at.min, galley, color);
        }
    }
}

fn draw_devices(app: &PacketApp, painter: &egui::Painter, view: &View, tk: &Tokens) {
    let t = &app.session.topology;
    // Cell towers: the coverage circle, in metres on a city map (workspace units on the implicit layout).
    let range = if t.implicit_layout() {
        netlabcraft_model::cellular::CELL_RANGE
    } else {
        netlabcraft_model::cellular::CELL_RANGE_M as f32 / t.map_scale(view.level.unwrap_or(netlabcraft_model::backdrop::INTERCITY))
    };
    for (id, x, y) in &view.devices {
        if t.device(*id).is_some_and(|d| d.is_tower()) {
            let c = view.screen(*x, *y);
            let r = range * view.zoom;
            let col = crate::icons_families::CELL_ORANGE;
            painter.circle_filled(c, r, col.gamma_multiply(0.1));
            let pts: Vec<Pos2> = (0..=96)
                .map(|i| c + vec2((i as f32 / 96.0 * std::f32::consts::TAU).cos(), (i as f32 / 96.0 * std::f32::consts::TAU).sin()) * r)
                .collect();
            painter.add(egui::Shape::dashed_line(&pts, Stroke::new(1.2, col.gamma_multiply(0.55)), 10.0, 8.0));
        }
    }
    for (id, x, y) in &view.devices {
        let Some(d) = t.device(*id) else { continue };
        let r = view.device_rect(*x, *y);
        if app.ui.selected == Some(*id) || app.ui.physical.drag_device == Some(*id) {
            painter.rect_filled(r.expand(6.0), 8.0, tk.selection.gamma_multiply(0.15));
            painter.rect_stroke(r.expand(6.0), 8.0, Stroke::new(1.5, tk.selection), StrokeKind::Outside);
        }
        crate::icons_families::model_icon(painter, r, d.kind, d.catalogue_model().id, d.catalogue_model().multilayer(), tk);
        let galley = painter.layout_no_wrap(d.display_name().to_string(), FontId::proportional((16.0 * view.zoom).clamp(8.0, 24.0)), tk.text);
        let at = Align2::CENTER_TOP.anchor_size(pos2(r.center().x, r.bottom() + 2.0), galley.size());
        // On a soft plate of the ground colour, so the cables under it don't cross the letters.
        painter.rect_filled(at.expand2(vec2(3.0, 0.0)), 4.0, tk.ground.gamma_multiply(0.82));
        painter.galley(at.min, galley, tk.text);
    }
}

fn interact(app: &mut PacketApp, ui: &Ui, resp: &egui::Response, view: &View, runs: &[Run]) {
    let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
    if resp.drag_started() {
        // The right or middle button pans; the left button moves what it grabbed, or does nothing on empty ground.
        let pan_button = ui.input(|i| i.pointer.middle_down() || i.pointer.secondary_down());
        let st = &mut app.ui.physical;
        st.panning = pan_button;
        let select = app.ui.tool == Tool::Select && !pan_button;
        if select && let Some(p) = pointer.and_then(|p| view.hit_place(p)) {
            app.run("edit.checkpoint", json!({}));
            app.ui.physical.drag_place = Some(p);
            app.ui.physical.drag_raw = view.places.iter().find(|x| x.id == p).map_or((0.0, 0.0), |x| (x.x, x.y));
            app.ui.physical.panning = false;
        } else if select && let Some(d) = pointer.and_then(|p| view.hit_device(p)) {
            app.run("edit.checkpoint", json!({}));
            app.ui.physical.drag_device = Some(d);
            app.ui.physical.drag_raw = app.session.topology.home_of(d).map_or((0.0, 0.0), |h| (h.x, h.y));
            app.ui.physical.panning = false;
            app.ui.selected = Some(d);
        }
    }
    if resp.dragged() {
        let delta = resp.drag_delta() / view.zoom;
        // Aligned, the object snaps to the grid while the pointer keeps its true position.
        if app.ui.physical.drag_place.is_some() || app.ui.physical.drag_device.is_some() {
            app.ui.physical.drag_raw.0 += delta.x;
            app.ui.physical.drag_raw.1 += delta.y;
        }
        let (x, y) = {
            let (rx, ry) = app.ui.physical.drag_raw;
            crate::physical_grid::snap_xy(app, rx, ry)
        };
        if let Some(id) = app.ui.physical.drag_place {
            app.run("place.move", json!({"id": id, "x": x, "y": y, "transient": true}));
        } else if let Some(d) = app.ui.physical.drag_device {
            if let Some(place) = app.session.place_view {
                app.run("device.place", json!({"device": d.0, "place": place, "x": x, "y": y, "transient": true}));
            }
        } else if app.ui.physical.panning {
            app.ui.pan += resp.drag_delta();
        }
    }
    if resp.drag_stopped() {
        let st = &mut app.ui.physical;
        st.drag_place = None;
        st.drag_device = None;
        st.panning = false;
    }
    if resp.double_clicked()
        && let Some(p) = pointer
        && let Some(d) = view.hit_device(p)
        && ui.input(|i| i.time) - app.ui.physical.entered > 0.6
    {
        crate::open_console(app, d);
        return;
    }
    if resp.secondary_clicked()
        && let Some(p) = pointer
    {
        app.ui.physical.context = Some(if let Some(id) = view.hit_place(p) {
            Target::Place(id)
        } else if let Some(d) = view.hit_device(p) {
            app.ui.selected = Some(d);
            Target::Device(d)
        } else if let Some(e) = hit_cable(runs, p) {
            Target::Cable(e)
        } else {
            let (x, y) = view.world(p);
            Target::Empty(x, y)
        });
    }
    if let Some(target) = app.ui.physical.context {
        resp.context_menu(|ui| context_menu(app, ui, target));
    }
    // A tile dragged from the device box and released over the workspace.
    if let Some((model, at)) = app.ui.tile_drop.take()
        && view.hit_place(at).is_none()
        && view.hit_device(at).is_none()
    {
        place_device(app, view, model, at);
    }
    if resp.clicked()
        && let Some(p) = pointer
    {
        // One click opens a city, a building or a closet.
        if let Some(id) = view.hit_place(p) {
            app.ui.physical.entered = ui.input(|i| i.time);
            enter(app, id);
            return;
        }
        match app.ui.tool {
            // A device placed while looking at a closet (or a building's rooms) stands there.
            Tool::Place(model) if view.hit_place(p).is_none() && view.hit_device(p).is_none() => place_device(app, view, model, p),
            _ => app.ui.selected = view.hit_device(p),
        }
    }
}

/// Puts a device model where the pointer is, in the place being looked at (a click with the tile
/// picked, or the tile dropped).
fn place_device(app: &mut PacketApp, view: &View, model: &'static str, p: Pos2) {
    if view.level.is_none_or(|l| app.session.topology.place(l).is_none_or(|x| x.kind == PlaceKind::City)) {
        app.info(tl("Devices stand in wiring closets and buildings: open one first."));
        return;
    }
    let (x, y) = view.world(p);
    let (x, y) = if app.ui.prefs.align_physical { crate::physical_grid::snap_xy(app, x, y) } else { (x, y) };
    app.run("device.add", json!({"model": model, "x": x.round(), "y": y.round()}));
}

/// Every place a device may stand in, by its full path (closets; buildings for hosts).
fn destinations(app: &PacketApp, host: bool) -> Vec<(u32, String)> {
    let t = &app.session.topology;
    t.places()
        .iter()
        .filter(|p| p.kind.holds_things() || (host && p.kind == PlaceKind::Building))
        .map(|p| (p.id, t.place_path(Some(p.id)).iter().map(|x| tr(&x.name)).collect::<Vec<_>>().join(" › ")))
        .collect()
}

fn context_menu(app: &mut PacketApp, ui: &mut Ui, target: Target) {
    let t = &app.session.topology;
    match target {
        Target::Place(id) => {
            let Some(p) = t.place(id) else {
                ui.close();
                return;
            };
            let working = t.working_closet();
            ui.label(egui::RichText::new(format!("{} ({})", tr(&p.name), tr(p.kind.label()))).strong());
            if ui.button(tl("Open")).clicked() {
                enter(app, id);
                ui.close();
            }
            if ui.button(tl("Rename Place…")).clicked() {
                app.ui.physical.renaming = Some((id, p.name.clone()));
                ui.close();
            }
            if p.kind == PlaceKind::Closet && ui.add_enabled(working != Some(id), egui::Button::new(tl("Set as Working Closet"))).clicked() {
                app.run("place.working.set", json!({"id": id}));
                ui.close();
            }
            if ui.button(tl("Set Background Image…")).clicked() {
                app.open_pictures(false, id);
                ui.close();
            }
            if ui.button(tl("Remove Place")).clicked() {
                app.run("place.remove", json!({"id": id}));
                ui.close();
            }
            if ui.button(tl("Grid Preferences…")).clicked() {
                app.ui.physical.grid.open = true;
                ui.close();
            }
        }
        Target::Device(d) => {
            let Some(dev) = t.device(d) else {
                ui.close();
                return;
            };
            let (name, host) = (dev.name.clone(), dev.kind.is_host());
            let here = t.home_of(d).map(|h| h.place);
            ui.label(egui::RichText::new(&name).strong());
            if ui.button(tl("Open Device")).clicked() {
                crate::open_console(app, d);
                ui.close();
            }
            ui.menu_button(tl("Move to"), |ui| {
                for (id, path) in destinations(app, host) {
                    if ui.add_enabled(Some(id) != here, egui::Button::new(&path)).clicked() {
                        app.run("device.place", json!({"device": d.0, "place": id}));
                        ui.close();
                    }
                }
            });
        }
        Target::Cable(end) => {
            let Some(l) = t.link_at(end).copied() else {
                ui.close();
                return;
            };
            let len = t.cable_length(&l);
            ui.label(
                egui::RichText::new(format!("{} - {}: {}", t.end_label(l.a), t.end_label(l.b), length_text(len.meters, app.ui.prefs.metric)))
                    .strong(),
            );
            if let Some(why) = netlabcraft_sim::cable_problem(t, l.a) {
                ui.label(egui::RichText::new(crate::i18n::tr_any(&why)).color(theme::tokens(app.ui.dark).error));
            }
            crate::cable_window::menu_items(app, ui, l.a, true);
        }
        Target::Empty(x, y) => {
            if ui.button(tl("Set Background Image…")).clicked() {
                app.open_pictures(false, app.session.place_view.unwrap_or(netlabcraft_model::backdrop::INTERCITY));
                ui.close();
            }
            if let Some(kind) = child_kind(app) {
                if ui.button(tl(new_label(kind))).clicked() {
                    add_place(app, Some((x, y)));
                    ui.close();
                }
            } else {
                ui.label(tl("Right-click a device to move it to another place."));
            }
            if ui.button(tl("Grid Preferences…")).clicked() {
                app.ui.physical.grid.open = true;
                ui.close();
            }
            let can_container = crate::physical_bar::reason(app, crate::physical_bar::Create::Container).is_none();
            if can_container && ui.button(tl(new_label(PlaceKind::Container))).clicked() {
                add_kind(app, PlaceKind::Container, Some((x, y)));
                ui.close();
            }
        }
    }
}

/// The Rename Place and Cable Length dialogs (and the closet's Rename dialog).
pub fn windows(app: &mut PacketApp, ctx: &egui::Context) {
    crate::closet::windows(app, ctx);
    crate::physical_grid::window(app, ctx);
    if let Some((id, mut name)) = app.ui.physical.renaming.clone() {
        let (mut open, mut ok) = (true, false);
        egui::Window::new(tl("Rename Place")).id(egui::Id::unique("place_rename")).open(&mut open).collapsible(false).resizable(false).show(
            ctx,
            |ui| {
                let l = ui.label(tl("Place name:"));
                let r = ui.add(egui::TextEdit::singleline(&mut name)).labelled_by(l.id);
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ok = true;
                }
                if ui.button(tl("OK")).clicked() {
                    ok = true;
                }
            },
        );
        app.ui.physical.renaming = if ok {
            app.run("place.rename", json!({"id": id, "name": name}));
            None
        } else if open {
            Some((id, name))
        } else {
            None
        };
    }
    if let Some((end, mut meters)) = app.ui.physical.length.clone() {
        let (mut open, mut set, mut clear) = (true, false, false);
        let label =
            app.session.topology.link_at(end).map(|l| format!("{} - {}", app.session.topology.end_label(l.a), app.session.topology.end_label(l.b)));
        egui::Window::new(tl("Cable Length")).id(egui::Id::unique("cable_length")).open(&mut open).collapsible(false).resizable(false).show(
            ctx,
            |ui| {
                if let Some(l) = &label {
                    ui.label(l);
                }
                ui.label(egui::RichText::new(tl("Copper Ethernet carries a signal up to 100 m.")).small());
                ui.horizontal(|ui| {
                    let l = ui.label(tl("Length (m):"));
                    let r = ui.add(egui::TextEdit::singleline(&mut meters).desired_width(90.0)).labelled_by(l.id);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        set = true;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(tl("OK")).clicked() {
                        set = true;
                    }
                    if ui.button(tl("Length from the places")).clicked() {
                        clear = true;
                    }
                });
            },
        );
        let dev =
            app.session.topology.device(end.device).map(|d| (d.name.clone(), d.ports.get(end.port).map(|p| p.name.clone()).unwrap_or_default()));
        app.ui.physical.length = if let (true, Some((d, port))) = (set || clear, dev) {
            let m: Value = if clear {
                Value::Null
            } else {
                match meters.trim().parse::<u32>() {
                    Ok(m) => json!(m),
                    Err(_) => json!(meters.trim()),
                }
            };
            app.run("link.length", json!({"device": d, "port": port, "meters": m}));
            None
        } else if open && label.is_some() {
            Some((end, meters))
        } else {
            None
        };
    }
}
