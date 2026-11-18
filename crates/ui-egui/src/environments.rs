//! Options ▸ Environments (Shift+E): the surroundings of a place.
//!
//! View mode reads the variables of one place (a tree of the lab's places picks it): what it sets
//! itself, what it inherits from the places above it, or the default, at the environment clock's
//! time. Edit mode sets the clock's ratio to simulated time and edits the place: a graph over
//! 24 hours (click to add or change a point, right-click to remove one, with time and value
//! snaps) and an Advanced tab (a time line, the keyframes one by one, export and import, notes,
//! and which variables are active). Everything is an engine command (`environment.*`); this file
//! only reads the lab and sends them. What is active, the snaps and the filter belong to the
//! window and are not saved with the lab.

use crate::prefs_look::HintExt;
use std::collections::BTreeSet;

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};
use netlabcraft_model::Topology;
use netlabcraft_model::environments::{self as env, DAY_SECONDS, GROUPS, INTERCITY, Key, Source, VARS, Var};
use serde_json::json;

use crate::i18n::{tl, tr, trf};
use crate::{PacketApp, UiRequest};

/// The tabs of Edit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Graph,
    Advanced,
}

/// Time snaps of the graph, in seconds (0: none), and the words that name them.
pub const TIME_SNAPS: [(&str, u32); 5] = [("None", 0), ("1 minute", 60), ("5 minutes", 300), ("15 minutes", 900), ("1 hour", 3_600)];
/// Value snaps of the graph, in the variable's own unit (0: none).
pub const VALUE_SNAPS: [(&str, f64); 4] = [("None", 0.0), ("0.1", 0.1), ("1", 1.0), ("10", 10.0)];
/// The steps of the Advanced tab's time line.
pub const INCREMENTS: [(&str, u32); 5] = [("1 second", 1), ("1 minute", 60), ("5 minutes", 300), ("15 minutes", 900), ("1 hour", 3_600)];

/// How far from a keyframe (in points) a click still means that keyframe.
const NEAR: f32 = 8.0;

/// What the window remembers.
#[derive(Debug, Clone)]
pub struct EnvUi {
    pub open: bool,
    pub edit: bool,
    pub tab: Tab,
    /// The place shown (none: the lab's default place).
    pub place: Option<u32>,
    pub filter: String,
    /// The filter that Search applied.
    pub applied: String,
    pub on_top: bool,
    /// Variables drawn in the graph, and the one a click edits.
    pub shown: Vec<&'static str>,
    pub selected: Option<&'static str>,
    /// Variables switched off in the tree (View mode and the graph legend skip them).
    pub hidden: BTreeSet<&'static str>,
    pub time_snap: usize,
    pub value_snap: usize,
    /// The Advanced tab's time line and the value to put there (none: what the place has now).
    pub key_time: u32,
    pub key_value: Option<f64>,
    pub increment: usize,
    pub editing_time: bool,
    pub time_text: String,
    /// The ratio being typed or dragged, until it is committed.
    pub scale: Option<(u32, u32)>,
    /// The notes being typed, with the place they belong to.
    pub notes: Option<(u32, String)>,
}

impl Default for EnvUi {
    fn default() -> Self {
        EnvUi {
            open: false,
            edit: false,
            tab: Tab::Graph,
            place: None,
            filter: String::new(),
            applied: String::new(),
            on_top: false,
            shown: Vec::new(),
            selected: None,
            hidden: BTreeSet::new(),
            time_snap: 2,
            value_snap: 0,
            key_time: 0,
            key_value: None,
            increment: 1,
            editing_time: false,
            time_text: String::new(),
            scale: None,
            notes: None,
        }
    }
}

impl PacketApp {
    pub fn open_environments(&mut self) {
        self.ui.env_ui.open = true;
    }

    /// The shell read an environment file for a place: load it.
    pub fn environment_imported(&mut self, place: u32, text: &str) {
        if let Some(v) = self.run("environment.import", json!({"place": place, "text": text})) {
            let read = v["imported"]["variables"].as_u64().unwrap_or(0);
            let skipped = v["imported"]["skipped"].as_u64().unwrap_or(0);
            self.info(trf("Imported an environment: {} variables read, {} skipped", &[&read, &skipped]));
        }
    }
}

/// The Options menu's entry.
pub fn menu_item(app: &mut PacketApp, ui: &mut egui::Ui) {
    if ui.add(egui::Button::new(tl("Environments")).shortcut_text(crate::shortcuts::text(crate::shortcuts::Act::Environments))).clicked() {
        app.open_environments();
        ui.close();
    }
}

const SERIES: [Color32; 12] = [
    Color32::from_rgb(230, 25, 75),
    Color32::from_rgb(60, 180, 75),
    Color32::from_rgb(0, 130, 200),
    Color32::from_rgb(245, 130, 48),
    Color32::from_rgb(145, 30, 180),
    Color32::from_rgb(0, 170, 170),
    Color32::from_rgb(240, 50, 230),
    Color32::from_rgb(170, 110, 40),
    Color32::from_rgb(128, 128, 0),
    Color32::from_rgb(70, 70, 70),
    Color32::from_rgb(200, 160, 0),
    Color32::from_rgb(0, 90, 60),
];

fn series_color(id: &str) -> Color32 {
    let i = VARS.iter().position(|v| v.id == id).unwrap_or(0);
    SERIES.get(i % SERIES.len()).copied().unwrap_or(Color32::GRAY)
}

/// A number as short text: no trailing zeros, no decimals once it is large.
pub fn fmt(v: f64) -> String {
    let s = if v.abs() >= 1_000.0 { format!("{v:.0}") } else { format!("{v:.6}") };
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

fn place_name(t: &Topology, id: u32) -> String {
    if id == INTERCITY { tl("Intercity").to_string() } else { t.place(id).map(|p| tr(&p.name).to_string()).unwrap_or_default() }
}

/// The places as a tree, depth first: id, name and depth (0 is the Intercity map).
pub fn place_items(t: &Topology) -> Vec<(u32, String, usize)> {
    let mut out = vec![(INTERCITY, tl("Intercity").to_string(), 0)];
    for city in t.places_in(None) {
        out.push((city.id, tr(&city.name).to_string(), 1));
        for building in t.places_in(Some(city.id)) {
            out.push((building.id, tr(&building.name).to_string(), 2));
            for closet in t.places_in(Some(building.id)) {
                out.push((closet.id, tr(&closet.name).to_string(), 3));
            }
        }
    }
    out
}

/// The place the window shows.
pub fn current_place(app: &PacketApp) -> u32 {
    let t = &app.session.topology;
    app.ui.env_ui.place.filter(|p| t.env_place_exists(*p)).unwrap_or_else(|| t.default_place())
}

/// A variable at a place now: its value and where it comes from (as words).
fn effective(app: &PacketApp, place: u32, v: &Var, time: u32) -> (f64, String) {
    let t = &app.session.topology;
    match t.env_value(place, v.id, time) {
        Some((x, Source::Own)) => (x, tl("set here").to_string()),
        Some((x, Source::Inherited(from))) => (x, trf("from {}", &[&place_name(t, from)])),
        _ => match v.drives.and_then(|w| app.session.sim.environment().and_then(|c| c.reading(w))) {
            Some(r) => (v.from_reading(r), tl("smart-home room").to_string()),
            None => (v.default, tl("default value").to_string()),
        },
    }
}

fn is_active(app: &PacketApp, v: &Var) -> bool {
    !app.ui.env_ui.hidden.contains(v.id)
}

fn matches(v: &Var, filter: &str) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty() || v.label.to_lowercase().contains(&f) || v.group.to_lowercase().contains(&f) || v.id.contains(&f)
}

/// The window.
pub fn windows(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.env_ui.open {
        return;
    }
    let mut open = true;
    let mut w = egui::Window::new(tl("Environments")).open(&mut open).default_size([760.0, 560.0]).default_pos(crate::BELOW_TOOLBARS);
    if app.ui.env_ui.on_top {
        w = w.order(egui::Order::Foreground);
    }
    w.show(ctx, |ui| body(app, ui));
    app.ui.env_ui.open = open;
}

fn body(app: &mut PacketApp, ui: &mut egui::Ui) {
    let place = current_place(app);
    location(app, ui, place);
    ui.separator();
    if app.ui.env_ui.edit {
        edit_mode(app, ui, place);
    } else {
        view_mode(app, ui, place);
    }
    ui.separator();
    ui.checkbox(&mut app.ui.env_ui.on_top, tl("Always on Top"));
}

fn location(app: &mut PacketApp, ui: &mut egui::Ui, place: u32) {
    let items = place_items(&app.session.topology);
    let current = items.iter().find(|i| i.0 == place).map(|i| i.1.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(tl("Location:"));
        egui::ComboBox::from_id_salt("env_place").selected_text(current).width(240.0).show_ui(ui, |ui| {
            for (id, name, depth) in &items {
                ui.horizontal(|ui| {
                    ui.add_space(14.0 * *depth as f32);
                    if ui.selectable_label(*id == place, name).clicked() {
                        app.ui.env_ui.place = Some(*id);
                        app.ui.env_ui.notes = None;
                        app.ui.env_ui.key_value = None;
                    }
                });
            }
        });
        if app.ui.env_ui.edit {
            if ui.button(tl("View Mode")).clicked() {
                app.ui.env_ui.edit = false;
            }
        } else if ui.button(tl("Edit")).hint(tl("Change this place's environment")).clicked() {
            app.ui.env_ui.edit = true;
        }
    });
}

fn clock_time(app: &PacketApp) -> u32 {
    app.session.sim.env_time(&app.session.topology)
}

fn view_mode(app: &mut PacketApp, ui: &mut egui::Ui, place: u32) {
    let time = clock_time(app);
    let paused = app.session.topology.environments.clock.paused();
    ui.horizontal(|ui| {
        ui.label(tl("Current Time:"));
        if app.ui.env_ui.editing_time {
            let r = ui.add(egui::TextEdit::singleline(&mut app.ui.env_ui.time_text).desired_width(84.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(tl("Set")).clicked() || enter {
                let text = app.ui.env_ui.time_text.clone();
                if env::parse_time(&text).is_some() {
                    app.run("environment.clock", json!({"time": text}));
                    app.ui.env_ui.editing_time = false;
                } else {
                    app.ui.status = Some((tl("Enter a time of day as HH:MM:SS.").to_string(), true));
                }
            }
            if ui.button(tl("Cancel")).clicked() {
                app.ui.env_ui.editing_time = false;
            }
        } else {
            ui.label(RichText::new(env::format_time(time)).strong().monospace());
            if ui.button(tl("Edit")).hint(tl("Set the time of day")).clicked() {
                app.ui.env_ui.editing_time = true;
                app.ui.env_ui.time_text = env::format_time(time);
            }
        }
        if ui.button(if paused { tl("Resume") } else { tl("Pause") }).clicked() {
            app.run("environment.clock", json!({"paused": !paused}));
        }
    });
    ui.horizontal(|ui| {
        ui.label(tl("Filter:"));
        ui.add(egui::TextEdit::singleline(&mut app.ui.env_ui.filter).desired_width(180.0));
        if ui.button(tl("Search")).clicked() {
            app.ui.env_ui.applied = app.ui.env_ui.filter.clone();
        }
        if ui.button(tl("Reset")).clicked() {
            app.ui.env_ui.filter.clear();
            app.ui.env_ui.applied.clear();
        }
    });
    let filter = app.ui.env_ui.applied.clone();
    let mut any = false;
    egui::ScrollArea::vertical().id_salt("env_view").max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
        for group in GROUPS {
            let vars: Vec<&Var> = VARS.iter().filter(|v| v.group == group && is_active(app, v) && matches(v, &filter)).collect();
            if vars.is_empty() {
                continue;
            }
            any = true;
            egui::CollapsingHeader::new(RichText::new(tl(group)).strong()).id_salt(("env_group", group)).default_open(true).show(ui, |ui| {
                egui::Grid::new(("env_grid", group)).num_columns(3).striped(true).show(ui, |ui| {
                    for v in vars {
                        let (x, from) = effective(app, place, v, time);
                        ui.label(tr(v.label));
                        ui.label(format!("{} {}", fmt(x), v.unit));
                        ui.label(RichText::new(from).weak().small());
                        ui.end_row();
                    }
                });
            });
        }
    });
    if !any {
        ui.label(tl("No variables match the filter."));
    }
    if let Some(e) = app.session.topology.place_env(place).filter(|e| e.show_notes && !e.notes.is_empty()) {
        ui.separator();
        ui.label(RichText::new(tl("Notes")).strong());
        ui.label(&e.notes);
    }
}

fn edit_mode(app: &mut PacketApp, ui: &mut egui::Ui, place: u32) {
    let c = app.session.topology.environments.clock;
    let (mut real, mut sim) = app.ui.env_ui.scale.unwrap_or((c.real_minutes, c.sim_minutes));
    ui.horizontal(|ui| {
        ui.label(tl("Simulation Time Scale:"));
        let a = ui.add(egui::DragValue::new(&mut real).range(1..=env::MAX_SCALE));
        ui.label(tl("minute(s) real equals"));
        let b = ui.add(egui::DragValue::new(&mut sim).range(1..=env::MAX_SCALE));
        ui.label(tl("simulated minute(s)"));
        if a.changed() || b.changed() {
            app.ui.env_ui.scale = Some((real, sim));
        }
        let typed = (a.changed() || b.changed()) && !a.dragged() && !b.dragged();
        if a.drag_stopped() || b.drag_stopped() || typed {
            app.ui.env_ui.scale = None;
            app.run("environment.clock", json!({"real_minutes": real, "sim_minutes": sim}));
        }
    });
    ui.horizontal(|ui| {
        for (tab, label) in [(Tab::Graph, "Keyframe Graph"), (Tab::Advanced, "Advanced")] {
            if ui.selectable_label(app.ui.env_ui.tab == tab, tl(label)).clicked() {
                app.ui.env_ui.tab = tab;
            }
        }
    });
    ui.separator();
    match app.ui.env_ui.tab {
        Tab::Graph => graph_tab(app, ui, place),
        Tab::Advanced => advanced_tab(app, ui, place),
    }
}

/// The time under a click on the graph and the value there, with the snaps; a click within a few
/// points of a keyframe means that keyframe's time.
pub fn graph_point(rect: Rect, pos: egui::Pos2, v: &Var, time_snap: u32, value_snap: f64, keys: &[Key]) -> (u32, f64) {
    let fx = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
    let mut t = (f64::from(fx) * f64::from(DAY_SECONDS)).round() as u32;
    // A snap of 0 is none: the division has no answer and the time stays.
    t = (t + time_snap / 2).checked_div(time_snap).map_or(t, |q| q * time_snap);
    t = t.min(DAY_SECONDS - 1);
    if let Some(k) = near_key(rect, keys, pos.x) {
        t = k;
    }
    let fy = f64::from(((rect.bottom() - pos.y) / rect.height().max(1.0)).clamp(0.0, 1.0));
    let mut val = v.min + fy * (v.max - v.min);
    if value_snap > 0.0 {
        val = (val / value_snap).round() * value_snap;
    }
    (t, (val.clamp(v.min, v.max) * 1e6).round() / 1e6)
}

/// The time of the keyframe nearest to an x position on the graph, when one is close.
pub fn near_key(rect: Rect, keys: &[Key], x: f32) -> Option<u32> {
    keys.iter()
        .map(|k| (k.t, (rect.left() + rect.width() * k.t as f32 / DAY_SECONDS as f32 - x).abs()))
        .filter(|(_, d)| *d <= NEAR)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(t, _)| t)
}

fn to_pos(rect: Rect, v: &Var, t: u32, value: f64) -> egui::Pos2 {
    let fy = ((value - v.min) / (v.max - v.min).max(f64::EPSILON)).clamp(0.0, 1.0) as f32;
    pos2(rect.left() + rect.width() * t as f32 / DAY_SECONDS as f32, rect.bottom() - rect.height() * fy)
}

fn own_keys(t: &Topology, place: u32, id: &str) -> Vec<Key> {
    t.place_env(place).and_then(|e| e.setting(id)).map(|s| s.keys.clone()).unwrap_or_default()
}

fn snap_combo<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, label: &'static str, current: &mut usize, options: &[(&'static str, T)]) {
    ui.label(tl(label));
    let text = options.get(*current).map(|o| tl(o.0)).unwrap_or("");
    egui::ComboBox::from_id_salt(id).selected_text(text).show_ui(ui, |ui| {
        for (i, (name, _)) in options.iter().enumerate() {
            if ui.selectable_label(*current == i, tl(name)).clicked() {
                *current = i;
            }
        }
    });
}

fn graph_tab(app: &mut PacketApp, ui: &mut egui::Ui, place: u32) {
    ui.horizontal(|ui| {
        let (mut ts, mut vs) = (app.ui.env_ui.time_snap, app.ui.env_ui.value_snap);
        snap_combo(ui, "env_tsnap", "Time Snap:", &mut ts, &TIME_SNAPS);
        snap_combo(ui, "env_vsnap", "Value Snap:", &mut vs, &VALUE_SNAPS);
        app.ui.env_ui.time_snap = ts;
        app.ui.env_ui.value_snap = vs;
    });
    ui.label(
        RichText::new(tl("Click a variable to show it, then click the graph to add or change a point. Right-click a point to remove it.")).small(),
    );
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(210.0);
            egui::ScrollArea::vertical().id_salt("env_legend").max_height(330.0).show(ui, |ui| {
                let list: Vec<&Var> = VARS.iter().filter(|v| is_active(app, v)).collect();
                for v in list {
                    let on = app.ui.env_ui.shown.contains(&v.id);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        if on {
                            ui.painter().rect_filled(r, 2.0, series_color(v.id));
                        }
                        if ui.selectable_label(app.ui.env_ui.selected == Some(v.id), tr(v.label)).clicked() {
                            toggle_shown(app, v.id);
                        }
                    });
                }
            });
        });
        graph(app, ui, place);
    });
}

fn toggle_shown(app: &mut PacketApp, id: &'static str) {
    let s = &mut app.ui.env_ui;
    if s.shown.contains(&id) && s.selected == Some(id) {
        s.shown.retain(|x| *x != id);
        s.selected = s.shown.last().copied();
    } else {
        if !s.shown.contains(&id) {
            s.shown.push(id);
        }
        s.selected = Some(id);
    }
    s.key_value = None;
}

fn graph(app: &mut PacketApp, ui: &mut egui::Ui, place: u32) {
    let tk = crate::theme::tokens(app.ui.dark);
    let (resp, painter) = ui.allocate_painter(vec2(ui.available_width().max(320.0), 330.0), Sense::click());
    let rect = Rect::from_min_max(resp.rect.min + vec2(52.0, 8.0), resp.rect.max - vec2(8.0, 22.0));
    painter.rect_filled(rect, 2.0, tk.canvas);
    let label = FontId::proportional(10.5);
    for h in (0..=24).step_by(3) {
        let x = rect.left() + rect.width() * h as f32 / 24.0;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, tk.grid));
        painter.text(pos2(x, rect.bottom() + 3.0), Align2::CENTER_TOP, format!("{:02}:00", h % 24), label.clone(), tk.muted);
    }
    for q in 0..=4 {
        let y = rect.bottom() - rect.height() * q as f32 / 4.0;
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, tk.grid));
    }
    let selected = app.ui.env_ui.selected.and_then(env::var_by_id);
    if let Some(v) = selected {
        painter.text(pos2(rect.left() - 4.0, rect.top()), Align2::RIGHT_TOP, fmt(v.max), label.clone(), tk.text);
        painter.text(pos2(rect.left() - 4.0, rect.bottom()), Align2::RIGHT_BOTTOM, fmt(v.min), label.clone(), tk.text);
        painter.text(pos2(rect.left() - 4.0, rect.center().y), Align2::RIGHT_CENTER, v.unit, label.clone(), tk.muted);
    }
    let t = &app.session.topology;
    for id in &app.ui.env_ui.shown {
        let Some(v) = env::var_by_id(id) else { continue };
        let colour = series_color(id);
        let setting = t.env_setting(place, id);
        let inherited = setting.is_some_and(|(from, _)| from != place);
        let pts: Vec<egui::Pos2> = (0..=96u32)
            .map(|i| {
                let at = (i * 900).min(DAY_SECONDS - 1);
                let value = setting.and_then(|(_, s)| env::interpolate(&s.keys, at)).unwrap_or(v.default);
                to_pos(rect, v, i * 900, value)
            })
            .collect();
        let weight = if selected.is_some_and(|s| s.id == v.id) { 2.2 } else { 1.4 };
        painter.add(egui::Shape::line(pts, Stroke::new(weight, if inherited || setting.is_none() { colour.gamma_multiply(0.5) } else { colour })));
        for k in own_keys(t, place, id) {
            let p = to_pos(rect, v, k.t, k.v);
            painter.circle_filled(p, 4.0, colour);
            painter.circle_stroke(p, 4.0, Stroke::new(1.0, tk.text));
        }
    }
    let now = clock_time(app);
    let nx = rect.left() + rect.width() * now as f32 / DAY_SECONDS as f32;
    painter.line_segment([pos2(nx, rect.top()), pos2(nx, rect.bottom())], Stroke::new(1.5, tk.selection));
    let Some(v) = selected else {
        painter.text(rect.center(), Align2::CENTER_CENTER, tl("Pick a variable on the left"), FontId::proportional(13.0), tk.muted);
        return;
    };
    let keys = own_keys(&app.session.topology, place, v.id);
    let (tsnap, vsnap) = (TIME_SNAPS.get(app.ui.env_ui.time_snap).map_or(0, |x| x.1), VALUE_SNAPS.get(app.ui.env_ui.value_snap).map_or(0.0, |x| x.1));
    if let Some(p) = resp.hover_pos().filter(|p| rect.contains(*p)) {
        let (at, value) = graph_point(rect, p, v, tsnap, vsnap, &keys);
        painter.text(
            rect.left_top() + vec2(6.0, 4.0),
            Align2::LEFT_TOP,
            format!("{}  {} {}", env::format_time(at), fmt(value), v.unit),
            label,
            tk.text,
        );
    }
    if let Some(p) = resp.interact_pointer_pos().filter(|p| rect.contains(*p)) {
        if resp.clicked() {
            let (at, value) = graph_point(rect, p, v, tsnap, vsnap, &keys);
            app.run("environment.keyframe.add", json!({"place": place, "variable": v.id, "time": at, "value": value}));
        } else if resp.secondary_clicked()
            && let Some(at) = near_key(rect, &keys, p.x)
        {
            app.run("environment.keyframe.remove", json!({"place": place, "variable": v.id, "time": at}));
        }
    }
}

fn advanced_tab(app: &mut PacketApp, ui: &mut egui::Ui, place: u32) {
    let mut physics = app.session.topology.environments.physics;
    if ui
        .checkbox(&mut physics, tl("Each place has its own physics"))
        .hint(tl("Parts change the place they stand in by rates, and the place settles back when they stop"))
        .changed()
    {
        app.run("environment.physics", json!({"enabled": physics}));
    }
    let step = INCREMENTS.get(app.ui.env_ui.increment).map_or(60, |x| x.1);
    ui.horizontal(|ui| {
        ui.label(tl("Time Line:"));
        if ui.button("-").clicked() {
            app.ui.env_ui.key_time = (app.ui.env_ui.key_time + DAY_SECONDS - step % DAY_SECONDS) % DAY_SECONDS;
            app.ui.env_ui.key_value = None;
        }
        ui.label(RichText::new(env::format_time(app.ui.env_ui.key_time)).strong().monospace());
        if ui.button("+").clicked() {
            app.ui.env_ui.key_time = (app.ui.env_ui.key_time + step) % DAY_SECONDS;
            app.ui.env_ui.key_value = None;
        }
        let mut inc = app.ui.env_ui.increment;
        snap_combo(ui, "env_inc", "Time Increment:", &mut inc, &INCREMENTS);
        app.ui.env_ui.increment = inc;
    });
    // The variable the keyframes below are of.
    let active: Vec<&Var> = VARS.iter().filter(|v| is_active(app, v)).collect();
    let current = app.ui.env_ui.selected.and_then(env::var_by_id).or_else(|| active.first().copied());
    ui.horizontal(|ui| {
        ui.label(tl("Variable:"));
        egui::ComboBox::from_id_salt("env_var").selected_text(current.map(|v| tr(v.label)).unwrap_or("")).width(220.0).show_ui(ui, |ui| {
            for v in &active {
                if ui.selectable_label(current.is_some_and(|c| c.id == v.id), tr(v.label)).clicked() {
                    app.ui.env_ui.selected = Some(v.id);
                    app.ui.env_ui.key_value = None;
                }
            }
        });
    });
    if let Some(v) = current {
        keyframes(app, ui, place, v);
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(tl("Export File…")).clicked()
            && let Some(r) = app.run("environment.export", json!({"place": place}))
        {
            let name = r["file"].as_str().unwrap_or("environment.json").to_string();
            let text = r["text"].as_str().unwrap_or_default().to_string();
            app.requests.push(UiRequest::SaveText { name, text });
        }
        if ui.button(tl("Import File…")).clicked() {
            app.requests.push(UiRequest::ImportEnvironment { place });
        }
        if ui.button(tl("Remove Custom Environment")).hint(tl("The place inherits every variable again")).clicked() {
            app.run("environment.clear", json!({"place": place}));
        }
    });
    notes(app, ui, place);
    ui.separator();
    tree(app, ui);
}

fn keyframes(app: &mut PacketApp, ui: &mut egui::Ui, place: u32, v: &'static Var) {
    let keys = own_keys(&app.session.topology, place, v.id);
    let kt = app.ui.env_ui.key_time;
    let at = keys.iter().position(|k| k.t == kt).map_or(0, |i| i + 1);
    ui.horizontal(|ui| {
        ui.label(trf("Keyframe {} of {}", &[&at, &keys.len()]));
        if ui.add_enabled(!keys.is_empty(), egui::Button::new(tl("Previous"))).clicked() {
            let to = keys.iter().rev().find(|k| k.t < kt).or(keys.last());
            if let Some(k) = to {
                app.ui.env_ui.key_time = k.t;
                app.ui.env_ui.key_value = None;
            }
        }
        if ui.add_enabled(!keys.is_empty(), egui::Button::new(tl("Next"))).clicked() {
            let to = keys.iter().find(|k| k.t > kt).or(keys.first());
            if let Some(k) = to {
                app.ui.env_ui.key_time = k.t;
                app.ui.env_ui.key_value = None;
            }
        }
    });
    let shown = app.ui.env_ui.key_value.unwrap_or_else(|| app.session.topology.env_value(place, v.id, kt).map_or(v.default, |x| x.0));
    let mut value = shown;
    ui.horizontal(|ui| {
        ui.label(tl("Value:"));
        let speed = ((v.max - v.min) / 500.0).clamp(0.001, 10.0);
        if ui.add(egui::DragValue::new(&mut value).range(v.min..=v.max).speed(speed).max_decimals(6).suffix(format!(" {}", v.unit))).changed() {
            app.ui.env_ui.key_value = Some(value);
        }
        if ui.button(tl("Add Keyframe")).clicked() {
            app.run("environment.keyframe.add", json!({"place": place, "variable": v.id, "time": kt, "value": value}));
            app.ui.env_ui.key_value = None;
        }
        if ui.add_enabled(keys.iter().any(|k| k.t == kt), egui::Button::new(tl("Remove"))).clicked() {
            app.run("environment.keyframe.remove", json!({"place": place, "variable": v.id, "time": kt}));
        }
        if ui.add_enabled(!keys.is_empty(), egui::Button::new(tl("Inherit Again"))).clicked() {
            app.run("environment.clear", json!({"place": place, "variable": v.id}));
        }
    });
}

fn notes(app: &mut PacketApp, ui: &mut egui::Ui, place: u32) {
    let (stored, show) = app.session.topology.place_env(place).map(|e| (e.notes.clone(), e.show_notes)).unwrap_or_default();
    if app.ui.env_ui.notes.as_ref().is_none_or(|(p, _)| *p != place) {
        app.ui.env_ui.notes = Some((place, stored.clone()));
    }
    ui.label(RichText::new(tl("Notes")).strong());
    let mut draft = app.ui.env_ui.notes.as_ref().map(|n| n.1.clone()).unwrap_or_default();
    let r = ui.add(egui::TextEdit::multiline(&mut draft).desired_rows(3).desired_width(f32::INFINITY));
    if r.changed() {
        app.ui.env_ui.notes = Some((place, draft.clone()));
    }
    if r.lost_focus() && draft != stored {
        app.run("environment.set", json!({"place": place, "notes": draft}));
    }
    let mut show_now = show;
    if ui.checkbox(&mut show_now, tl("Show Notes in View Mode")).changed() {
        app.run("environment.set", json!({"place": place, "show_notes": show_now}));
    }
}

fn tree(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(tl("Variables")).strong());
        if ui.button(tl("Show All")).clicked() {
            app.ui.env_ui.hidden.clear();
        }
        if ui.button(tl("Hide All")).clicked() {
            app.ui.env_ui.hidden = VARS.iter().map(|v| v.id).collect();
        }
    });
    egui::ScrollArea::vertical().id_salt("env_tree").max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
        for group in GROUPS {
            let ids: Vec<&'static str> = VARS.iter().filter(|v| v.group == group).map(|v| v.id).collect();
            let mut all = ids.iter().all(|i| !app.ui.env_ui.hidden.contains(i));
            let id = ui.make_persistent_id(("env_tree", group));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    if ui.checkbox(&mut all, tl(group)).changed() {
                        for i in &ids {
                            if all {
                                app.ui.env_ui.hidden.remove(i);
                            } else {
                                app.ui.env_ui.hidden.insert(*i);
                            }
                        }
                    }
                })
                .body(|ui| {
                    for v in VARS.iter().filter(|v| v.group == group) {
                        let mut on = !app.ui.env_ui.hidden.contains(v.id);
                        if ui.checkbox(&mut on, tr(v.label)).changed() {
                            if on {
                                app.ui.env_ui.hidden.remove(v.id);
                            } else {
                                app.ui.env_ui.hidden.insert(v.id);
                            }
                        }
                    }
                });
        }
    });
}
