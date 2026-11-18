//! The physical workspace's grid and its Grid Preferences window.
//!
//! Every level of the physical workspace that has a map (the Intercity map, a city, a building,
//! and the generic container once it exists) can show a grid whose lines stand a number of
//! metres apart. The setting is a view choice of this program, so it lives in [`Prefs`] and the
//! preferences file (the web build keeps it for the session); it is one setting per level, not
//! per lab. A wiring closet is drawn in its own units and has no scale, so it has no grid.
//!
//! The lines are thin and drawn under the cables and the devices. How many are drawn is bounded:
//! a spacing that would put lines closer than [`MIN_STEP_PX`] on screen, or more than
//! [`MAX_LINES`] across the view, draws none and says so in the corner label.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use netlabcraft_model::physical::{CITY_UNIT_M, INTERCITY_UNIT_M, PlaceKind};

use crate::PacketApp;
use crate::i18n::{tl, trf};
use crate::icons::{self, Glyph};
use crate::prefs_look::HintExt;
use crate::theme::Tokens;

/// Smallest and largest spacing the window and the preferences file accept, in metres.
pub const MIN_SPACING_M: f32 = 0.1;
pub const MAX_SPACING_M: f32 = 1_000_000.0;
/// Lines closer than this on screen are not drawn (a grid of dust helps nobody).
pub const MIN_STEP_PX: f32 = 8.0;
/// Most lines drawn across one view in one direction.
pub const MAX_LINES: usize = 400;
/// What an aligned object snaps to on a level whose grid is not ticked: half of the old fixed
/// map grid, in map units, so labs saved before the grid became a setting keep their feel.
pub const LEGACY_SNAP: f32 = 40.0;
/// The default colour of the lines: a light grey that reads on the light and the dark ground.
pub const DEFAULT_COLOR: [u8; 3] = [0x96, 0x9b, 0xa5];

/// The levels that have a grid scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLevel {
    Intercity,
    City,
    Building,
    /// The generic container. No place of this kind exists yet: when it lands, [`current_level`]
    /// maps it here and [`metres_per_unit`] gives its scale.
    Container,
}

impl GridLevel {
    pub const ALL: [GridLevel; 4] = [GridLevel::Intercity, GridLevel::City, GridLevel::Building, GridLevel::Container];

    /// The row label (an English catalog key).
    pub fn label(self) -> &'static str {
        match self {
            GridLevel::Intercity => "Intercity",
            GridLevel::City => "City",
            GridLevel::Building => "Building",
            GridLevel::Container => "Generic Container",
        }
    }

    /// The name in the preferences file.
    fn key(self) -> &'static str {
        match self {
            GridLevel::Intercity => "intercity",
            GridLevel::City => "city",
            GridLevel::Building => "building",
            GridLevel::Container => "container",
        }
    }

    fn index(self) -> usize {
        match self {
            GridLevel::Intercity => 0,
            GridLevel::City => 1,
            GridLevel::Building => 2,
            GridLevel::Container => 3,
        }
    }

    /// The spacing a level starts with, in metres.
    pub fn default_spacing(self) -> f32 {
        match self {
            GridLevel::Intercity => 2000.0,
            GridLevel::City => 200.0,
            GridLevel::Building => 20.0,
            GridLevel::Container => 1.0,
        }
    }
}

/// A spacing as the settings keep it: finite and inside the bounds, or `None`.
pub fn valid_spacing(v: f32) -> Option<f32> {
    (v.is_finite() && v > 0.0).then(|| v.clamp(MIN_SPACING_M, MAX_SPACING_M))
}

/// The grid settings: per level whether it is drawn and its spacing, and the colour.
#[derive(Debug, Clone, PartialEq)]
pub struct GridPrefs {
    on: [bool; 4],
    x: [f32; 4],
    y: [f32; 4],
    /// The line colour, as red, green and blue.
    pub color: [u8; 3],
}

impl Default for GridPrefs {
    fn default() -> Self {
        let d = GridLevel::ALL.map(GridLevel::default_spacing);
        GridPrefs { on: [false; 4], x: d, y: d, color: DEFAULT_COLOR }
    }
}

impl GridPrefs {
    pub fn on(&self, level: GridLevel) -> bool {
        self.on.get(level.index()).copied().unwrap_or(false)
    }

    pub fn set_on(&mut self, level: GridLevel, on: bool) {
        if let Some(v) = self.on.get_mut(level.index()) {
            *v = on;
        }
    }

    /// The spacing of a level, in metres (Grid-X, Grid-Y).
    pub fn spacing(&self, level: GridLevel) -> (f32, f32) {
        let i = level.index();
        (self.x.get(i).copied().unwrap_or(1.0), self.y.get(i).copied().unwrap_or(1.0))
    }

    /// Sets the spacing; a value that is not positive and finite leaves that axis as it was, one
    /// out of range is brought inside the bounds.
    pub fn set_spacing(&mut self, level: GridLevel, x: f32, y: f32) {
        let i = level.index();
        if let (Some(v), Some(slot)) = (valid_spacing(x), self.x.get_mut(i)) {
            *slot = v;
        }
        if let (Some(v), Some(slot)) = (valid_spacing(y), self.y.get_mut(i)) {
            *slot = v;
        }
    }

    pub fn color32(&self) -> Color32 {
        Color32::from_rgb(self.color[0], self.color[1], self.color[2])
    }

    /// The settings as lines of the preferences file.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for l in GridLevel::ALL {
            let (x, y) = self.spacing(l);
            out.push_str(&format!("grid-{}={}\ngrid-{}-x={x}\ngrid-{}-y={y}\n", l.key(), if self.on(l) { "on" } else { "off" }, l.key(), l.key()));
        }
        out.push_str(&format!("grid-color=#{:02x}{:02x}{:02x}\n", self.color[0], self.color[1], self.color[2]));
        out
    }

    /// Reads one line of the file; `false` when the key is not a grid key. A value that does not
    /// read leaves the setting as it was.
    pub fn apply(&mut self, key: &str, value: &str) -> bool {
        let Some(rest) = key.strip_prefix("grid-") else { return false };
        if rest == "color" {
            if let Some(c) = parse_hex(value) {
                self.color = c;
            }
            return true;
        }
        for l in GridLevel::ALL {
            let Some(tail) = rest.strip_prefix(l.key()) else { continue };
            match tail {
                "" => {
                    if matches!(value, "on" | "off") {
                        self.set_on(l, value == "on");
                    }
                }
                "-x" | "-y" => {
                    if let Ok(v) = value.parse::<f32>() {
                        let (x, y) = self.spacing(l);
                        if tail == "-x" {
                            self.set_spacing(l, v, y);
                        } else {
                            self.set_spacing(l, x, v);
                        }
                    }
                }
                _ => continue,
            }
            return true;
        }
        false
    }
}

/// `#rrggbb` (the `#` optional).
fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let part = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([part(0)?, part(2)?, part(4)?])
}

/// The window's own state.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The Grid Preferences window is open.
    pub open: bool,
    /// The colour palette and the red, green and blue fields are shown.
    picker: bool,
}

/// The level the physical workspace is showing; none inside a wiring closet.
pub fn current_level(app: &PacketApp) -> Option<GridLevel> {
    match app.session.place_view.and_then(|p| app.session.topology.place(p)) {
        None => Some(GridLevel::Intercity),
        Some(p) => match p.kind {
            PlaceKind::City => Some(GridLevel::City),
            PlaceKind::Building => Some(GridLevel::Building),
            PlaceKind::Closet => None,
        },
    }
}

/// How many metres one map unit of this level stands for.
///
/// The one place that knows the scale. A place that was given a background picture or a size
/// (a scale of its own) answers here once that lands; until then a level has its fixed unit.
pub fn metres_per_unit(level: GridLevel) -> f32 {
    match level {
        GridLevel::Intercity => INTERCITY_UNIT_M,
        GridLevel::City | GridLevel::Building | GridLevel::Container => CITY_UNIT_M,
    }
}

/// The grid lines to draw in a view, as screen coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines {
    /// x of each vertical line.
    pub xs: Vec<f32>,
    /// y of each horizontal line.
    pub ys: Vec<f32>,
    /// Lines in at least one direction were left out because they would be too many or too close.
    pub dense: bool,
}

/// One direction: the positions of lines `step_px` apart (anchored at `origin`) inside `lo..hi`,
/// or `None` when they would be too close or too many.
fn axis(lo: f32, hi: f32, origin: f32, step_px: f32) -> Option<Vec<f32>> {
    if !(step_px.is_finite() && lo.is_finite() && hi.is_finite() && origin.is_finite()) || hi <= lo {
        return Some(Vec::new());
    }
    if step_px < MIN_STEP_PX || (hi - lo) / step_px > MAX_LINES as f32 {
        return None;
    }
    let first = ((lo - origin) / step_px).ceil();
    let mut out = Vec::new();
    for k in 0..=MAX_LINES {
        let p = origin + (first + k as f32) * step_px;
        if p >= hi {
            break;
        }
        out.push(p);
    }
    Some(out)
}

/// Where the lines of a grid with these spacings fall in `rect` (`origin` is where the map's
/// zero sits on screen, `zoom` the screen pixels of one map unit, `scale` the metres of one unit).
pub fn lines(rect: Rect, origin: Vec2, zoom: f32, spacing: (f32, f32), scale: f32) -> Lines {
    let unit = |m: f32| m / scale * zoom;
    let (sx, sy) = (unit(spacing.0), unit(spacing.1));
    if !(scale.is_finite() && scale > 0.0 && zoom.is_finite() && zoom > 0.0) {
        return Lines::default();
    }
    let xs = axis(rect.left(), rect.right(), origin.x, sx);
    let ys = axis(rect.top(), rect.bottom(), origin.y, sy);
    Lines { dense: xs.is_none() || ys.is_none(), xs: xs.unwrap_or_default(), ys: ys.unwrap_or_default() }
}

/// A snapped value: `v` (map units) rounded to the nearest multiple of `step_m` metres.
pub fn snap_to(v: f32, step_m: f32, scale: f32) -> f32 {
    let unit = step_m / scale;
    if !(v.is_finite() && unit.is_finite()) || unit <= f32::EPSILON {
        return v;
    }
    (v / unit).round() * unit
}

/// A position a dragged or placed object takes. Not aligned: where it is. Aligned: on the grid
/// of the level shown when that grid is ticked, else on the old half-grid.
pub fn snap_xy(app: &PacketApp, x: f32, y: f32) -> (f32, f32) {
    if !app.ui.prefs.align_physical {
        return (x, y);
    }
    if let Some(l) = current_level(app)
        && app.ui.prefs.grid.on(l)
    {
        let (sx, sy) = app.ui.prefs.grid.spacing(l);
        let s = metres_per_unit(l);
        return (snap_to(x, sx, s), snap_to(y, sy, s));
    }
    ((x / LEGACY_SNAP).round() * LEGACY_SNAP, (y / LEGACY_SNAP).round() * LEGACY_SNAP)
}

/// A distance for the corner label: metres, or feet when the metric system is off.
pub fn spacing_text(m: f32, metric: bool) -> String {
    let num = |v: f64| {
        let r = (v * 100.0).round() / 100.0;
        if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r}") }
    };
    if metric { format!("{} m", num(f64::from(m))) } else { format!("{} ft", num(f64::from(m) / 0.3048)) }
}

/// Draws the grid of the level shown, under everything else.
pub(crate) fn draw(app: &PacketApp, painter: &egui::Painter, rect: Rect, origin: Vec2, zoom: f32, tk: &Tokens) {
    let Some(level) = current_level(app) else { return };
    if !app.ui.prefs.grid.on(level) {
        return;
    }
    let spacing = app.ui.prefs.grid.spacing(level);
    let l = lines(rect, origin, zoom, spacing, metres_per_unit(level));
    let stroke = Stroke::new(1.0, app.ui.prefs.grid.color32().gamma_multiply(0.55));
    for x in &l.xs {
        painter.line_segment([pos2(*x, rect.top()), pos2(*x, rect.bottom())], stroke);
    }
    for y in &l.ys {
        painter.line_segment([pos2(rect.left(), *y), pos2(rect.right(), *y)], stroke);
    }
    let metric = app.ui.prefs.metric;
    let mut text = if (spacing.0 - spacing.1).abs() < f32::EPSILON {
        spacing_text(spacing.0, metric)
    } else {
        format!("{} × {}", spacing_text(spacing.0, metric), spacing_text(spacing.1, metric))
    };
    if l.dense {
        text = trf("{}: zoom in to see the lines", &[&text]);
    }
    let galley = painter.layout_no_wrap(text, FontId::proportional(14.0), tk.muted);
    let at = Align2::LEFT_BOTTOM.anchor_size(rect.left_bottom() + vec2(12.0, -10.0), galley.size());
    painter.rect_filled(at.expand2(vec2(4.0, 2.0)), 4.0, tk.ground.gamma_multiply(0.85));
    painter.galley(at.min, galley, tk.muted);
}

/// Shift+G and the Show grid item: the logical grid on the logical workspace, the grid of the
/// level shown on the physical one.
pub fn toggle(app: &mut PacketApp) {
    if !app.session.physical {
        app.ui.show_grid = !app.ui.show_grid;
        return;
    }
    match current_level(app) {
        Some(l) => {
            let on = !app.ui.prefs.grid.on(l);
            app.ui.prefs.grid.set_on(l, on);
        }
        None => app.info(tl("A wiring closet has no scale, so it has no grid.")),
    }
}

/// Whether the grid of the view shown is drawn.
pub fn showing(app: &PacketApp) -> bool {
    if !app.session.physical {
        return app.ui.show_grid;
    }
    current_level(app).is_some_and(|l| app.ui.prefs.grid.on(l))
}

/// Options ▸ Show grid, for whichever workspace is showing.
pub(crate) fn menu_item(app: &mut PacketApp, ui: &mut Ui) {
    let mut on = showing(app);
    let enabled = !app.session.physical || current_level(app).is_some();
    let r = ui.add_enabled(enabled, egui::Checkbox::new(&mut on, tl("Show grid")));
    let r = r.hint(crate::shortcuts::text(crate::shortcuts::Act::ShowGrid));
    if r.changed() {
        toggle(app);
    }
}

/// The four-squares button of the physical workspace bar.
pub(crate) fn bar_button(app: &mut PacketApp, ui: &mut Ui, tk: &Tokens) {
    let on = app.ui.physical.grid.open;
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::click());
    let label = tl("Grid Preferences");
    let resp = resp.hint(label);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, on, label));
    let p = ui.painter_at(r);
    if on {
        p.rect_filled(r.shrink(1.5), 7.0, tk.pill);
    } else if resp.hovered() {
        p.rect_filled(r.shrink(1.5), 7.0, tk.pill.gamma_multiply(0.5));
    }
    p.rect_stroke(r, 8.0, Stroke::new(1.5, tk.bar_text), egui::StrokeKind::Inside);
    icons::glyph(&p, Rect::from_center_size(r.center(), vec2(20.0, 20.0)), Glyph::Grid, tk.pill_text);
    if resp.clicked() {
        app.ui.physical.grid.open = !on;
    }
}

/// Palette of the colour picker (the first is the default).
const PALETTE: [[u8; 3]; 10] = [
    DEFAULT_COLOR,
    [0xc8, 0xcc, 0xd2],
    [0x64, 0x6e, 0x7d],
    [0x28, 0x2c, 0x34],
    [0x3c, 0x78, 0xc8],
    [0x2a, 0x9d, 0x8f],
    [0x4c, 0xa0, 0x50],
    [0xe0, 0x8a, 0x2a],
    [0xd0, 0x4a, 0x4a],
    [0x8a, 0x5a, 0xc0],
];

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// The Grid Preferences window.
pub(crate) fn window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.physical.grid.open {
        return;
    }
    let mut open = true;
    let mut done = false;
    let mut prefs = app.ui.prefs.grid.clone();
    let mut picker = app.ui.physical.grid.picker;
    egui::Window::new(tl("Grid Preferences"))
        .id(egui::Id::unique("physical_grid_window"))
        .default_pos([40.0, crate::BELOW_VIEW_BAR + 10.0])
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(tl("Grid Scale")).strong());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(tl(
                    "Set the grid spacing, in metres, for each level of the physical workspace. A level's grid is drawn once its box is checked.",
                ));
                egui::Grid::new("physical_grid_levels").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("");
                    ui.label(tl("Grid-X (m)"));
                    ui.label(tl("Grid-Y (m)"));
                    ui.end_row();
                    for level in GridLevel::ALL {
                        let name = tl(level.label());
                        let mut on = prefs.on(level);
                        if ui.checkbox(&mut on, name).changed() {
                            prefs.set_on(level, on);
                        }
                        let (mut x, mut y) = prefs.spacing(level);
                        let (x0, y0) = (x, y);
                        for (v, axis_label) in [(&mut x, tl("Grid-X (m)")), (&mut y, tl("Grid-Y (m)"))] {
                            let speed = (level.default_spacing() / 50.0).max(0.1);
                            let r = ui.add(egui::DragValue::new(v).range(MIN_SPACING_M..=MAX_SPACING_M).speed(speed).max_decimals(2));
                            let full = format!("{name} {axis_label}");
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::SpinButton, true, full.clone()));
                        }
                        if (x, y) != (x0, y0) {
                            prefs.set_spacing(level, x, y);
                        }
                        ui.end_row();
                    }
                });
                ui.label(egui::RichText::new(tl("A wiring closet has no scale, so it has no grid.")).weak());
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tl("Grid Line Color")).strong());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                    ui.painter().rect_filled(r, 3.0, prefs.color32());
                    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color), egui::StrokeKind::Inside);
                    if ui.button(tl("Select Grid Color")).clicked() {
                        picker = !picker;
                    }
                });
                if picker {
                    ui.horizontal_wrapped(|ui| {
                        for c in PALETTE {
                            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                            let label = trf("Grid color {}", &[&hex(c)]);
                            resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, prefs.color == c, label.clone()));
                            ui.painter().rect_filled(r, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
                            let edge = if prefs.color == c {
                                ui.visuals().selection.stroke
                            } else {
                                Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.5))
                            };
                            ui.painter().rect_stroke(r, 3.0, edge, egui::StrokeKind::Inside);
                            if resp.clicked() {
                                prefs.color = c;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        for (i, name) in [tl("Red"), tl("Green"), tl("Blue")].into_iter().enumerate() {
                            let l = ui.label(name);
                            if let Some(v) = prefs.color.get_mut(i) {
                                ui.add(egui::DragValue::new(v).range(0..=255)).labelled_by(l.id);
                            }
                        }
                    });
                }
            });
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tl("OK")).clicked() {
                    done = true;
                }
            });
        });
    app.ui.prefs.grid = prefs;
    app.ui.physical.grid.picker = picker;
    app.ui.physical.grid.open = open && !done;
}

impl PacketApp {
    /// Opens the Grid Preferences window (the bar button, View and the place menu do the same).
    pub fn open_grid_preferences(&mut self) {
        self.ui.physical.grid.open = true;
    }
}

impl PacketApp {
    /// Shows the colour palette of the Grid Preferences window (the sheets look at it).
    pub fn open_grid_colours(&mut self) {
        self.ui.physical.grid.picker = true;
    }
}
