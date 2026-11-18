//! Pictures in the interface: the textures of the lab's pictures, the background drawn under the
//! logical workspace and under the map of a place, the picture a place wears instead of its drawn
//! icon, and the Set Background Image window (Shift+I, the picture button on the workspace bar,
//! and the context menu of a place).
//!
//! The window edits a draft and sends it to the engine in one command on Apply (`place.scene.set`
//! or `workspace.background.set`), so Apply is one undo step. Nothing is read from a path here:
//! the shell shows its file dialog (`UiRequest::PickImage`) and hands the bytes back to
//! [`PacketApp::image_picked`], which has the engine check and shrink them.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{Color32, ColorImage, Context, Painter, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};
use netlabcraft_model::Topology;
use netlabcraft_model::backdrop::{Background, BgMode, INTERCITY, MAX_CONTAINER_M, MAX_CUSTOM, UseKind, VIEW_H, VIEW_W, base64_encode};
use serde_json::{Value, json};

use crate::i18n::{tl, trf};
use crate::prefs_look::HintExt;
use crate::{PacketApp, UiRequest};

/// Most tiles one tiled background draws in a frame (a tiny picture on a large view draws none).
const MAX_TILES: i64 = 4_000;
/// The preview's size in the window.
const PREVIEW: Vec2 = vec2(200.0, 140.0);

/// The textures made from the lab's pictures, by picture id.
#[derive(Debug, Clone, Default)]
pub struct Textures {
    map: BTreeMap<String, Option<TextureHandle>>,
}

/// A texture of the bytes of a picture, no larger than the graphics card takes.
fn load(ctx: &Context, name: &str, data: &[u8]) -> Option<TextureHandle> {
    let max = ctx.input(|i| i.max_texture_side).clamp(256, 4096) as u32;
    let (w, h, px) = netlabcraft_engine::images::decode_display(data, max).ok()?;
    let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
    Some(ctx.load_texture(name, img, TextureOptions::LINEAR))
}

impl Textures {
    /// The texture of a picture the lab holds (none: it is not held, or cannot be read; asked again
    /// only after the picture changes).
    pub fn get(&mut self, ctx: &Context, t: &Topology, id: &str) -> Option<TextureHandle> {
        if let Some(tex) = self.map.get(id) {
            return tex.clone();
        }
        let tex = t.images.get(id).and_then(|img| load(ctx, &format!("picture-{id}"), &img.data));
        self.map.insert(id.to_string(), tex.clone());
        tex
    }

    /// Forgets the textures of pictures the lab no longer holds.
    pub fn keep_only(&mut self, t: &Topology) {
        self.map.retain(|id, _| t.images.get(id).is_some());
    }
}

/// Draws a background picture. `origin` is where the map's (0, 0) is on the screen; the painter's
/// clip rectangle bounds the work.
pub fn paint(painter: &Painter, origin: Vec2, zoom: f32, bg: &Background, dims: (u32, u32), tex: &TextureHandle) {
    let (sw, sh) = bg.shown_size(dims.0, dims.1);
    let (tw, th) = (sw * zoom, sh * zoom);
    if !tw.is_finite() || !th.is_finite() || tw < 0.5 || th < 0.5 {
        return;
    }
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let o = origin.to_pos2();
    if bg.mode != BgMode::Tiled {
        painter.image(tex.id(), Rect::from_min_size(o, vec2(tw, th)), uv, Color32::WHITE);
        return;
    }
    let clip = painter.clip_rect();
    let range = |lo: f32, hi: f32, at: f32, size: f32| (((lo - at) / size).floor() as i64, ((hi - at) / size).ceil() as i64);
    let (x0, x1) = range(clip.left(), clip.right(), o.x, tw);
    let (y0, y1) = range(clip.top(), clip.bottom(), o.y, th);
    if (x1 - x0).max(0).saturating_mul((y1 - y0).max(0)) > MAX_TILES {
        return;
    }
    for j in y0..y1 {
        for i in x0..x1 {
            let rect = Rect::from_min_size(pos2(o.x + i as f32 * tw, o.y + j as f32 * th), vec2(tw, th));
            painter.image(tex.id(), rect, uv, Color32::WHITE);
        }
    }
}

fn paint_stored(app: &mut PacketApp, ctx: &Context, painter: &Painter, origin: Vec2, zoom: f32, bg: &Background) {
    let Some(dims) = app.session.topology.images.get(&bg.image).map(|i| (i.w, i.h)) else { return };
    if let Some(tex) = app.ui.pictures.textures.get(ctx, &app.session.topology, &bg.image) {
        paint(painter, origin, zoom, bg, dims, &tex);
    }
}

/// The background of the logical workspace.
pub fn draw_logical(app: &mut PacketApp, ctx: &Context, painter: &Painter, origin: Vec2, zoom: f32) {
    if let Some(bg) = app.session.topology.background.clone() {
        paint_stored(app, ctx, painter, origin, zoom, &bg);
    }
}

/// The background of the map inside a place (`None`: the Intercity map).
pub fn draw_place(app: &mut PacketApp, ctx: &Context, painter: &Painter, origin: Vec2, zoom: f32, place: Option<u32>) {
    let bg = app.session.topology.scene(place.unwrap_or(INTERCITY)).and_then(|s| s.background.clone());
    if let Some(bg) = bg {
        paint_stored(app, ctx, painter, origin, zoom, &bg);
    }
}

/// A picture cropped to a square with rounded corners, in `r`.
pub fn paint_icon(painter: &Painter, r: Rect, tex: &TextureHandle, dims: (u32, u32), edge: Color32) {
    let (w, h) = (dims.0.max(1) as f32, dims.1.max(1) as f32);
    let uv = if w > h {
        let k = h / w;
        Rect::from_min_max(pos2((1.0 - k) / 2.0, 0.0), pos2((1.0 + k) / 2.0, 1.0))
    } else {
        let k = w / h;
        Rect::from_min_max(pos2(0.0, (1.0 - k) / 2.0), pos2(1.0, (1.0 + k) / 2.0))
    };
    let radius = r.width() * 0.16;
    painter.add(egui::epaint::RectShape::filled(r, radius, Color32::WHITE).with_texture(tex.id(), uv));
    painter.rect_stroke(r, radius, Stroke::new(1.5, edge), StrokeKind::Outside);
}

/// The icon picture of a place, drawn in `r`; false when it has none (draw the built-in icon).
pub fn draw_icon(app: &mut PacketApp, ctx: &Context, painter: &Painter, r: Rect, place: u32, edge: Color32) -> bool {
    let Some(id) = app.session.topology.scene(place).and_then(|s| s.icon.clone()) else { return false };
    let Some(dims) = app.session.topology.images.get(&id).map(|i| (i.w, i.h)) else { return false };
    let Some(tex) = app.ui.pictures.textures.get(ctx, &app.session.topology, &id) else { return false };
    paint_icon(painter, r, &tex, dims, edge);
    true
}

/// A picture chosen in this window and not applied yet (already checked and made to fit).
#[derive(Debug, Clone)]
struct Picked {
    data: Arc<Vec<u8>>,
    w: u32,
    h: u32,
    tex: Option<TextureHandle>,
}

/// Where a picture of the draft stands.
#[derive(Debug, Clone, Default)]
enum Slot {
    /// The one the lab holds now.
    Stored,
    /// None.
    #[default]
    Empty,
    /// One just chosen.
    New(Picked),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Background,
    Icon,
}

/// The Set Background Image window: its draft.
#[derive(Debug, Clone, Default)]
pub struct Window {
    pub open: bool,
    /// The logical workspace (else the place below).
    pub logical: bool,
    /// The place (0: the Intercity map).
    pub place: u32,
    pub tab: Tab,
    bg: Slot,
    icon: Slot,
    pub(crate) mode: BgMode,
    custom_w: u32,
    custom_h: u32,
    pub(crate) width_m: f32,
    pub(crate) length_m: f32,
    height_m: f32,
    /// The size was edited (else it follows the picture and the scale the place has).
    pub(crate) touched: bool,
    /// What the map area is drawn at now, to notice a change of layout.
    last_shown: (f32, f32),
    /// Metres per unit when nothing was edited: the place's own scale.
    base_scale: f32,
    /// Which tab asked the shell for a picture.
    pick_for: Option<Tab>,
}

/// What the interface keeps about pictures: the textures and the window's draft.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub textures: Textures,
    pub window: Window,
}

impl Window {
    fn stored_background(&self, t: &Topology) -> Option<Background> {
        if self.logical { t.background.clone() } else { t.scene(self.place).and_then(|s| s.background.clone()) }
    }

    fn stored_dims(&self, t: &Topology) -> Option<(u32, u32)> {
        self.stored_background(t).and_then(|b| t.images.get(&b.image).map(|i| (i.w, i.h)))
    }

    /// The size of the picture in the draft.
    fn dims(&self, t: &Topology) -> Option<(u32, u32)> {
        match &self.bg {
            Slot::Stored => self.stored_dims(t),
            Slot::Empty => None,
            Slot::New(p) => Some((p.w, p.h)),
        }
    }

    fn layout(&self) -> Background {
        let custom = match self.mode {
            BgMode::Width => self.custom_w,
            BgMode::Height => self.custom_h,
            _ => 0,
        };
        Background { image: String::new(), mode: self.mode, custom }
    }

    /// How big the background is drawn, in map units (the map area when there is no picture).
    fn shown(&self, t: &Topology) -> (f32, f32) {
        self.dims(t).map_or((VIEW_W, VIEW_H), |(w, h)| self.layout().shown_size(w, h))
    }

    /// The draft as the lab has it now.
    fn reset(&mut self, t: &Topology) {
        let cur = self.stored_background(t);
        let dims = self.stored_dims(t);
        self.mode = cur.as_ref().map_or(BgMode::Original, |b| b.mode);
        let (iw, ih) = dims.unwrap_or((1000, 700));
        self.custom_w = cur.as_ref().filter(|b| b.mode == BgMode::Width).map_or(iw, |b| b.custom).clamp(1, MAX_CUSTOM);
        self.custom_h = cur.as_ref().filter(|b| b.mode == BgMode::Height).map_or(ih, |b| b.custom).clamp(1, MAX_CUSTOM);
        self.bg = if cur.is_some() { Slot::Stored } else { Slot::Empty };
        let scene = if self.logical { None } else { t.scene(self.place) };
        self.icon = if scene.is_some_and(|s| s.icon.is_some()) { Slot::Stored } else { Slot::Empty };
        let size = scene.and_then(|s| s.size);
        self.base_scale = if self.logical { 1.0 } else { t.map_scale(self.place) };
        self.touched = size.is_some();
        self.height_m = size.map_or(3.0, |z| z.height);
        let (sw, sh) = self.shown(t);
        self.width_m = size.map_or(sw * self.base_scale, |z| z.width);
        self.length_m = size.map_or(sh * self.base_scale, |z| z.length);
        self.last_shown = (sw, sh);
        self.pick_for = None;
    }

    /// Metres per map unit the draft gives the place.
    fn scale(&self, t: &Topology) -> f32 {
        let (sw, _) = self.shown(t);
        if sw > 0.0 { self.width_m / sw } else { self.base_scale }
    }

    /// What Apply sends.
    fn command(&self, t: &Topology) -> (&'static str, Value) {
        let layout = self.layout();
        let mut spec = json!({"mode": layout.mode.id()});
        if let (true, Some(o)) = (layout.custom > 0, spec.as_object_mut()) {
            o.insert("custom".into(), json!(layout.custom));
        }
        let stored = self.stored_background(t).is_some();
        if let (Slot::New(p), Some(o)) = (&self.bg, spec.as_object_mut()) {
            o.insert("data".into(), json!(base64_encode(&p.data)));
        }
        if self.logical {
            return match &self.bg {
                Slot::Empty => ("workspace.background.clear", json!({})),
                _ => ("workspace.background.set", spec),
            };
        }
        let mut p = json!({"place": self.place});
        match &self.bg {
            Slot::Empty if stored => p["background"] = Value::Null,
            Slot::Empty => {}
            _ => p["background"] = spec,
        }
        let had_icon = t.scene(self.place).is_some_and(|s| s.icon.is_some());
        match &self.icon {
            Slot::Empty if had_icon => p["icon"] = Value::Null,
            Slot::New(i) => p["icon"] = json!({"data": base64_encode(&i.data)}),
            _ => {}
        }
        if self.touched {
            p["size"] = json!({"width": self.width_m, "length": self.length_m, "height": self.height_m, "scale": self.scale(t)});
        }
        ("place.scene.set", p)
    }
}

impl PacketApp {
    /// Opens the Set Background Image window for the workspace being shown: the logical
    /// workspace, or the place being looked at.
    pub fn open_background_window(&mut self) {
        let place = self.session.place_view.unwrap_or(INTERCITY);
        self.open_pictures(!self.session.physical, place);
    }

    /// Opens the window for a place (0: the Intercity map) or the logical workspace.
    pub fn open_pictures(&mut self, logical: bool, place: u32) {
        let mut w = std::mem::take(&mut self.ui.pictures.window);
        w.open = true;
        w.logical = logical;
        w.place = place;
        w.tab = Tab::Background;
        w.reset(&self.session.topology);
        self.ui.pictures.window = w;
    }

    /// The shell read the picture the person chose with Browse: check it, make it fit the limits
    /// and put it in the draft.
    pub fn image_picked(&mut self, bytes: Vec<u8>) {
        let Some(tab) = self.ui.pictures.window.pick_for.take() else { return };
        match netlabcraft_engine::images::prepare(&bytes) {
            Ok(p) => {
                if let Some((w, h)) = p.scaled_from {
                    self.info(trf("Scaled down from {} × {} to {} × {} to fit the limits of a lab", &[&w, &h, &p.w, &p.h]));
                }
                let w = &mut self.ui.pictures.window;
                let slot = Slot::New(Picked { data: Arc::new(p.data), w: p.w, h: p.h, tex: None });
                match tab {
                    Tab::Background => {
                        w.bg = slot;
                        w.custom_w = p.w.clamp(1, MAX_CUSTOM);
                        w.custom_h = p.h.clamp(1, MAX_CUSTOM);
                    }
                    Tab::Icon => w.icon = slot,
                }
            }
            Err(e) => self.ui.status = Some((crate::i18n::tr_any(&e), true)),
        }
    }
}

/// The window, when it is open.
pub fn windows(app: &mut PacketApp, ctx: &Context) {
    let t = &app.session.topology;
    app.ui.pictures.textures.keep_only(t);
    if !app.ui.pictures.window.open {
        return;
    }
    let mut w = std::mem::take(&mut app.ui.pictures.window);
    let mut open = true;
    egui::Window::new(tl("Set Background Image"))
        .id(egui::Id::new("set_background_image"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(crate::BELOW_TOOLBARS)
        .show(ctx, |ui| body(app, ctx, ui, &mut w));
    // A place that is gone closes it.
    if !w.logical && w.place != INTERCITY && app.session.topology.place(w.place).is_none() {
        open = false;
    }
    w.open = open;
    app.ui.pictures.window = w;
}

fn body(app: &mut PacketApp, ctx: &Context, ui: &mut Ui, w: &mut Window) {
    let place_tab = !w.logical && w.place != INTERCITY;
    if place_tab {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut w.tab, Tab::Background, tl("Background Image"));
            ui.selectable_value(&mut w.tab, Tab::Icon, tl("Place Icon"));
        });
        ui.separator();
    } else {
        w.tab = Tab::Background;
    }
    match w.tab {
        Tab::Background => background_tab(app, ctx, ui, w),
        Tab::Icon => icon_tab(app, ctx, ui, w),
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(tl("Reset")).clicked() {
            let t = app.session.topology.clone();
            w.reset(&t);
        }
        if ui.button(tl("Apply")).clicked() {
            let (cmd, params) = w.command(&app.session.topology);
            if app.run(cmd, params).is_some() {
                let t = app.session.topology.clone();
                w.reset(&t);
            }
        }
    });
}

/// Browse and Remove Picture, for one slot.
fn pick_buttons(app: &mut PacketApp, ui: &mut Ui, w: &mut Window, tab: Tab) {
    ui.horizontal(|ui| {
        if ui.button(tl("Browse…")).clicked() {
            w.pick_for = Some(tab);
            app.requests.push(UiRequest::PickImage);
        }
        let has = !matches!(if tab == Tab::Icon { &w.icon } else { &w.bg }, Slot::Empty);
        if ui.add_enabled(has, egui::Button::new(tl("Remove Picture"))).clicked() {
            match tab {
                Tab::Background => w.bg = Slot::Empty,
                Tab::Icon => w.icon = Slot::Empty,
            }
        }
    });
    ui.label(RichText::new(tl("Browse… picks a PNG or JPEG picture.")).small());
}

fn size_text(dims: Option<(u32, u32)>) -> String {
    dims.map_or_else(|| tl("No picture").to_string(), |(w, h)| trf("{} × {} px", &[&w, &h]))
}

fn background_tab(app: &mut PacketApp, ctx: &Context, ui: &mut Ui, w: &mut Window) {
    pick_buttons(app, ui, w, Tab::Background);
    let dims = w.dims(&app.session.topology);
    ui.add_space(4.0);
    ui.label(RichText::new(tl("Sizes")).strong());
    egui::Grid::new("picture_sizes").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
        ui.label(tl("Picture:"));
        ui.label(size_text(dims));
        ui.end_row();
        ui.label(tl("Map area:"));
        ui.label(trf("{} × {} px", &[&(VIEW_W as u32), &(VIEW_H as u32)]));
        ui.end_row();
        ui.label(tl("Recommended picture:"));
        ui.label(trf("{} × {} px", &[&(VIEW_W as u32), &(VIEW_H as u32)]));
        ui.end_row();
    });
    ui.add_space(4.0);
    preview(app, ctx, ui, w);
    ui.add_space(4.0);
    ui.label(RichText::new(tl("Layout")).strong());
    ui.radio_value(&mut w.mode, BgMode::Original, tl("Original size"));
    ui.radio_value(&mut w.mode, BgMode::Tiled, tl("Tiled"));
    ui.radio_value(&mut w.mode, BgMode::Fit, tl("Stretch to the map area"));
    ui.horizontal(|ui| {
        let radio = ui.radio_value(&mut w.mode, BgMode::Width, tl("Custom width"));
        ui.add_enabled(w.mode == BgMode::Width, egui::DragValue::new(&mut w.custom_w).range(1..=MAX_CUSTOM).suffix(" px")).labelled_by(radio.id);
    });
    ui.horizontal(|ui| {
        let radio = ui.radio_value(&mut w.mode, BgMode::Height, tl("Custom height"));
        ui.add_enabled(w.mode == BgMode::Height, egui::DragValue::new(&mut w.custom_h).range(1..=MAX_CUSTOM).suffix(" px")).labelled_by(radio.id);
    });
    if !w.logical {
        physical_properties(app, ui, w);
    }
}

/// The map area with the background as it would be drawn.
fn preview(app: &mut PacketApp, ctx: &Context, ui: &mut Ui, w: &mut Window) {
    let tk = crate::theme::tokens(app.ui.dark);
    let (rect, resp) = ui.allocate_exact_size(PREVIEW, Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, tl("Preview of the background")));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, if w.logical { tk.canvas } else { tk.ground });
    let zoom = (rect.width() / VIEW_W).min(rect.height() / VIEW_H);
    let area = Rect::from_min_size(rect.min, vec2(VIEW_W, VIEW_H) * zoom);
    let painter = painter.with_clip_rect(area);
    let layout = w.layout();
    let t = &app.session.topology;
    let shown = match &mut w.bg {
        Slot::New(p) => {
            if p.tex.is_none() {
                p.tex = load(ctx, "picture-draft", &p.data);
            }
            p.tex.clone().map(|tex| (tex, (p.w, p.h)))
        }
        Slot::Stored => w.stored_background(t).and_then(|b| {
            let dims = t.images.get(&b.image).map(|i| (i.w, i.h))?;
            Some((app.ui.pictures.textures.get(ctx, t, &b.image)?, dims))
        }),
        Slot::Empty => None,
    };
    if let Some((tex, dims)) = shown {
        paint(&painter, rect.min.to_vec2(), zoom, &layout, dims, &tex);
    }
    ui.painter().rect_stroke(area, 2.0, Stroke::new(1.0, tk.chrome_line), StrokeKind::Outside);
}

fn physical_properties(app: &mut PacketApp, ui: &mut Ui, w: &mut Window) {
    let t = &app.session.topology;
    let (sw, sh) = w.shown(t);
    // Without edits the size follows the picture; with edits the width stays and the length follows.
    if !w.touched {
        w.width_m = sw * w.base_scale;
        w.length_m = sh * w.base_scale;
    } else if (sw, sh) != w.last_shown && sw > 0.0 {
        w.length_m = w.width_m * sh / sw;
    }
    w.last_shown = (sw, sh);
    ui.add_space(4.0);
    ui.label(RichText::new(tl("Size of the place")).strong());
    let range = 0.01..=MAX_CONTAINER_M;
    let (mut width, mut length, mut height) = (w.width_m, w.length_m, w.height_m);
    egui::Grid::new("place_size").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
        for (label, value) in [("Width (m):", &mut width), ("Length (m):", &mut length), ("Height (m):", &mut height)] {
            let l = ui.label(tl(label));
            ui.add(egui::DragValue::new(value).range(range.clone()).speed(0.1).max_decimals(3)).labelled_by(l.id);
            ui.end_row();
        }
        ui.label(tl("Volume:"));
        ui.label(trf("{} m³", &[&crate::environments::fmt(f64::from(w.width_m) * f64::from(w.length_m) * f64::from(w.height_m))]));
        ui.end_row();
        ui.label(tl("Scale:"));
        ui.label(trf("{} m per pixel", &[&crate::environments::fmt(f64::from(w.scale(&app.session.topology)))]));
        ui.end_row();
    });
    let changed = |a: f32, b: f32| (a - b).abs() > f32::EPSILON * a.abs().max(1.0);
    if changed(width, w.width_m) {
        w.width_m = width;
        w.length_m = if sw > 0.0 { width * sh / sw } else { length };
        w.touched = true;
    } else if changed(length, w.length_m) {
        w.length_m = length;
        w.width_m = if sh > 0.0 { length * sw / sh } else { width };
        w.touched = true;
    }
    if changed(height, w.height_m) {
        w.height_m = height;
        w.touched = true;
    }
}

fn icon_tab(app: &mut PacketApp, ctx: &Context, ui: &mut Ui, w: &mut Window) {
    ui.label(tl("The picture this place wears on the map above it, instead of its drawn icon."));
    pick_buttons(app, ui, w, Tab::Icon);
    ui.add_space(4.0);
    let tk = crate::theme::tokens(app.ui.dark);
    let (rect, resp) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, tl("Preview of the icon")));
    let painter = ui.painter_at(rect.expand(4.0));
    let edge = crate::theme::shade(tk.place, 0.5);
    let t = &app.session.topology;
    let stored_icon = t.scene(w.place).and_then(|s| s.icon.clone());
    let shown = match &mut w.icon {
        Slot::New(p) => {
            if p.tex.is_none() {
                p.tex = load(ctx, "icon-draft", &p.data);
            }
            p.tex.clone().map(|tex| (tex, (p.w, p.h)))
        }
        Slot::Stored => stored_icon.and_then(|id| {
            let dims = t.images.get(&id).map(|i| (i.w, i.h))?;
            Some((app.ui.pictures.textures.get(ctx, t, &id)?, dims))
        }),
        Slot::Empty => None,
    };
    let drawn = shown.is_some();
    if let Some((tex, dims)) = shown {
        paint_icon(&painter, rect, &tex, dims, edge);
    }
    if !drawn {
        painter.rect_stroke(rect, 12.0, Stroke::new(1.0, tk.chrome_line), StrokeKind::Inside);
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, tl("No picture"), egui::FontId::proportional(13.0), tk.muted);
    }
}

/// The text of a byte count.
pub fn bytes_text(n: usize) -> String {
    if n >= 1024 * 1024 { format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)) } else { format!("{} KB", n.div_ceil(1024).max(1)) }
}

/// Preferences ▸ Image Cleanup: the table of the pictures the open lab uses, with a button that
/// stops each use (one undo step), and a note when there are none.
pub fn cleanup_table(app: &mut PacketApp, ui: &mut Ui) {
    let uses = app.session.topology.image_uses();
    let ctx = ui.ctx().clone();
    let mut remove: Option<(String, u32)> = None;
    egui::Grid::new("image_cleanup").num_columns(4).spacing([24.0, 4.0]).show(ui, |ui| {
        for h in ["Remove", "Type", "Size", "Image"] {
            ui.label(RichText::new(tl(h)).strong());
        }
        ui.end_row();
        for u in &uses {
            let t = &app.session.topology;
            let name = match u.place {
                INTERCITY => tl("Intercity").to_string(),
                id => t.place(id).map(|p| crate::i18n::tr(&p.name).to_string()).unwrap_or_default(),
            };
            let kind = match u.kind {
                UseKind::Logical => tl("Workspace background").to_string(),
                UseKind::Background => trf("Background of {}", &[&name]),
                UseKind::Icon => trf("Icon of {}", &[&name]),
            };
            let dims = t.images.get(&u.image).map(|i| (i.w, i.h));
            let size = t.images.get(&u.image).map(|i| trf("{} × {} px, {}", &[&i.w, &i.h, &bytes_text(i.data.len())])).unwrap_or_default();
            if ui.button(tl("Remove")).hint(kind.clone()).clicked() {
                remove = Some((u.kind.id().to_string(), u.place));
            }
            ui.label(kind);
            ui.label(size);
            let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 3.0, Color32::from_gray(128).gamma_multiply(0.3));
            if let (Some(d), Some(tex)) = (dims, app.ui.pictures.textures.get(&ctx, &app.session.topology, &u.image)) {
                let k = (32.0 / d.0.max(1) as f32).min(32.0 / d.1.max(1) as f32);
                let r = Rect::from_center_size(rect.center(), vec2(d.0 as f32 * k, d.1 as f32 * k));
                painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            ui.end_row();
        }
    });
    if uses.is_empty() {
        ui.separator();
        ui.label(tl("The lab holds no images yet: set one with the Set Background Image button."));
    }
    if let Some((kind, place)) = remove {
        app.run("image.remove", json!({"kind": kind, "place": place}));
    }
}
