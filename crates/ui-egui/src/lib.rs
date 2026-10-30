//! PacketCraft L6: the egui interface.
//!
//! [`PacketApp`] draws everything inside one `egui::Ui`. The arrangement follows the reference
//! app learners know (menus and two toolbars on top, a navy workspace bar, the workspace, a navy
//! time bar with the Realtime/Simulation switch, and the device box with the PDU list at the
//! bottom); every picture is drawn in code (`icons.rs`). It is thin: it reads the engine's
//! [`Session`] and acts through [`PacketApp::run`]. File dialogs and other host work go out as
//! [`UiRequest`]s for the shell (desktop or web).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::BTreeMap;

use egui::{Color32, Key, KeyboardShortcut, Modifiers, RichText, Sense, vec2};
use packetcraft_core::DeviceId;
use packetcraft_engine::{EngineError, Session, sample};

use serde_json::{Value, json};

pub mod activity;
mod bottom;
mod canvas;
mod console;
mod events;
pub mod i18n;
pub mod icons;
mod mail;
mod services;
pub mod theme;

#[cfg(test)]
mod tests;

use crate::i18n::tl;
use icons::Glyph;

/// Work the shell does for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiRequest {
    /// Show an open dialog, then run `file.open` with the chosen path.
    Open,
    /// Write `json` (the lab) to `path`, or ask where first.
    Save { json: String, path: Option<String> },
    /// Ask where to write a pcap capture.
    ExportPcap(Vec<u8>),
    /// Ask where to write a text file (a device configuration).
    SaveText { name: String, text: String },
    /// Pick a text file for a device: its startup configuration (`merge: false`) or lines to
    /// merge into the running configuration; then call [`PacketApp::text_opened`].
    OpenText { device: DeviceId, merge: bool },
    /// Pick an .html file to add to a server's web site; then call
    /// [`PacketApp::web_file_imported`].
    ImportWebFile { device: DeviceId },
    /// Close the app.
    Quit,
}

/// The active workspace tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Inspect,
    /// Place a device of a catalogue model (`models::MODELS` id).
    Place(&'static str),
    Cable,
    /// Place a note on the workspace.
    Note,
    /// Drag out a rectangle, ellipse or line.
    Draw(packetcraft_model::Shape),
    /// Click a source then a destination: one echo request (a simple PDU).
    Ping,
    Delete,
}

/// Realtime: the network just runs. Simulation: the event list is shown and replayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Realtime,
    Simulation,
}

/// One row of the PDU list.
#[derive(Debug, Clone, PartialEq)]
pub struct Pdu {
    pub num: u32,
    pub source: DeviceId,
    pub destination: DeviceId,
    pub address: String,
    pub ok: Option<bool>,
    pub color: Color32,
    pub time_s: f64,
}

/// View state that isn't part of the lab.
#[derive(Debug, Clone)]
pub struct UiState {
    pub dark: bool,
    pub show_grid: bool,
    pub tool: Tool,
    pub mode: Mode,
    pub selected: Option<DeviceId>,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub windows: BTreeMap<DeviceId, console::DeviceWindow>,
    pub inspect: Option<DeviceId>,
    pub show_notes: bool,
    /// The Simulation Panel is shown in simulation mode ("Event List" toggle).
    pub show_event_list: bool,
    pub show_about: bool,
    pub status: Option<(String, bool)>,
    pub events: events::EventView,
    pub pdus: Vec<Pdu>,
    pub bottom: bottom::BoxState,
    /// The pointer over the workspace, in workspace units.
    pub pointer: Option<(f32, f32)>,
    /// The device Copy put on the clipboard.
    pub clipboard: Option<DeviceId>,
    /// The workspace note being edited, and its text.
    pub editing_note: Option<(u32, String)>,
    /// Labs opened or saved recently, newest first (the desktop shell fills it).
    pub recent: Vec<String>,
    /// The person picked another interface language (the shell saves the choice).
    pub lang_changed: bool,
    /// Activity instructions and wizard.
    pub activity: activity::ActivityView,
    canvas: canvas::CanvasState,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            dark: false,
            show_grid: false,
            tool: Tool::Select,
            mode: Mode::Realtime,
            selected: None,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            windows: BTreeMap::new(),
            inspect: None,
            show_notes: false,
            show_event_list: true,
            show_about: false,
            status: None,
            events: events::EventView::default(),
            pdus: Vec::new(),
            bottom: bottom::BoxState::default(),
            pointer: None,
            clipboard: None,
            editing_note: None,
            recent: Vec::new(),
            lang_changed: false,
            activity: activity::ActivityView::default(),
            canvas: canvas::CanvasState::default(),
        }
    }
}

/// Opens (or raises) a device's window.
pub fn open_console(app: &mut PacketApp, d: DeviceId) {
    console::open(app, d);
}

/// Opens a host's Desktop: `"prompt"`, `"ip"` (IP Configuration) or the tiles (anything else).
pub fn open_desktop_app(app: &mut PacketApp, d: DeviceId, which: &str) {
    console::open(app, d);
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.tab = console::Tab::Desktop;
        w.desktop = match which {
            "prompt" => Some(console::DesktopApp::Prompt),
            "ip" => Some(console::DesktopApp::IpConfig),
            "email" => Some(console::DesktopApp::Email),
            _ => None,
        };
    }
}

/// Opens a server's Services tab.
pub fn open_services_tab(app: &mut PacketApp, d: DeviceId) {
    console::open(app, d);
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.tab = console::Tab::Services;
    }
}

/// Opens a server's Services tab on one page (`http`, `dhcp`, `dns`, `syslog`, `ntp`, `tftp`,
/// `aaa`).
pub fn open_services_page(app: &mut PacketApp, d: DeviceId, page: &str) {
    use services::Page;
    open_services_tab(app, d);
    let p = match page {
        "dhcp" => Page::Dhcp,
        "dns" => Page::Dns,
        "syslog" => Page::Syslog,
        "ntp" => Page::Ntp,
        "tftp" => Page::Tftp,
        "aaa" => Page::Aaa,
        "ftp" => Page::Ftp,
        "email" => Page::Email,
        _ => Page::Http,
    };
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.svc.page = p;
    }
}

/// Opens a host's Web Browser on a URL (typed in, not yet fetched).
pub fn open_browser(app: &mut PacketApp, d: DeviceId, url: &str) {
    console::open(app, d);
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.tab = console::Tab::Desktop;
        w.desktop = Some(console::DesktopApp::Browser);
        w.set_url(url);
    }
}

/// Opens a device's Config tab.
pub fn open_config_tab(app: &mut PacketApp, d: DeviceId) {
    console::open(app, d);
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.tab = console::Tab::Config;
    }
}

/// The application.
pub struct PacketApp {
    pub session: Session,
    pub ui: UiState,
    requests: Vec<UiRequest>,
}

/// A square toolbar button with a glyph; `label` is its tooltip and accessible name.
fn tool_button(ui: &mut egui::Ui, g: Glyph, label: &str, selected: bool, enabled: bool) -> egui::Response {
    let tk = theme::tokens(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(vec2(38.0, 36.0), if enabled { Sense::click() } else { Sense::hover() });
    let resp = resp.on_hover_text(crate::i18n::tr(label));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, enabled, selected, label));
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect.shrink(1.0), 3.0, tk.pill.gamma_multiply(0.22));
        p.rect_stroke(rect.shrink(1.0), 3.0, egui::Stroke::new(1.0, tk.pill), egui::StrokeKind::Inside);
    } else if resp.hovered() && enabled {
        p.rect_filled(rect.shrink(1.0), 3.0, tk.chrome_line.gamma_multiply(0.6));
    }
    let color = if enabled { tk.glyph } else { tk.glyph.gamma_multiply(0.35) };
    let color = if matches!(g, Glyph::SimplePdu | Glyph::ComplexPdu) && enabled { Color32::from_rgb(0x3b, 0x3f, 0x46) } else { color };
    icons::glyph(&p, egui::Rect::from_center_size(rect.center(), vec2(30.0, 30.0)), g, color);
    resp
}

/// A rounded tab on a navy bar (Logical / Physical, Realtime / Simulation).
fn pill(ui: &mut egui::Ui, g: Glyph, label: &str, active: bool) -> egui::Response {
    let tk = theme::tokens(ui.visuals().dark_mode);
    let font = egui::FontId::proportional(20.0);
    let label = crate::i18n::tr(label);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, tk.pill_text);
    let size = vec2(galley.size().x + 38.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Tab, true, active, label));
    let p = ui.painter_at(rect);
    let fill = if active {
        tk.pill
    } else if resp.hovered() {
        tk.pill.gamma_multiply(0.35)
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, 8.0, fill);
    let border = if active { egui::Stroke::new(1.5, tk.bar_text) } else { egui::Stroke::new(1.0, tk.bar_text.gamma_multiply(0.45)) };
    p.rect_stroke(rect, 8.0, border, egui::StrokeKind::Inside);
    icons::glyph(&p, egui::Rect::from_min_size(rect.min + vec2(5.0, 4.0), vec2(22.0, 22.0)), g, tk.pill_text);
    p.galley(rect.min + vec2(31.0, (rect.height() - galley.size().y) / 2.0), galley, tk.pill_text);
    resp
}

/// A rounded navy button with white text (New, Delete, Reset Simulation…).
pub(crate) fn navy_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let label = crate::i18n::tr(label);
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(16.0), Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(16.0, 10.0), Sense::click());
    paint_navy(ui, rect, &resp, galley, label);
    resp
}

/// [`navy_button`] filling the available width.
pub(crate) fn navy_button_wide(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let label = crate::i18n::tr(label);
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(16.0), Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y + 10.0), Sense::click());
    paint_navy(ui, rect, &resp, galley, label);
    resp
}

fn paint_navy(ui: &egui::Ui, rect: egui::Rect, resp: &egui::Response, galley: std::sync::Arc<egui::Galley>, label: &str) {
    let tk = theme::tokens(ui.visuals().dark_mode);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, label));
    ui.painter().rect_filled(rect, 8.0, if resp.hovered() { tk.pill } else { tk.bar });
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

/// The play-control pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlayGlyph {
    Back,
    Play,
    Pause,
    Forward,
}

/// A navy rounded square play-control button.
pub(crate) fn play_button(ui: &mut egui::Ui, g: PlayGlyph, label: &str, size: f32) -> egui::Response {
    let tk = theme::tokens(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let resp = resp.on_hover_text(crate::i18n::tr(label));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, label));
    let p = ui.painter_at(rect);
    p.rect_filled(rect.shrink(1.0), 7.0, if resp.hovered() { tk.pill } else { tk.bar });
    p.rect_stroke(rect.shrink(1.0), 7.0, egui::Stroke::new(1.5, Color32::WHITE.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    let r = rect.shrink(size * 0.28);
    let w = Color32::WHITE;
    let tri = |a: egui::Pos2, b: egui::Pos2, c: egui::Pos2| egui::Shape::convex_polygon(vec![a, b, c], w, egui::Stroke::NONE);
    match g {
        PlayGlyph::Back => {
            p.rect_filled(egui::Rect::from_min_size(r.left_top(), vec2(r.width() * 0.18, r.height())), 0.0, w);
            p.add(tri(r.right_top(), r.right_bottom(), r.left_center() + vec2(r.width() * 0.2, 0.0)));
        }
        PlayGlyph::Forward => {
            p.rect_filled(egui::Rect::from_min_size(r.right_top() - vec2(r.width() * 0.18, 0.0), vec2(r.width() * 0.18, r.height())), 0.0, w);
            p.add(tri(r.left_top(), r.left_bottom(), r.right_center() - vec2(r.width() * 0.2, 0.0)));
        }
        PlayGlyph::Play => {
            p.add(tri(r.left_top(), r.left_bottom(), r.right_center()));
        }
        PlayGlyph::Pause => {
            p.rect_filled(egui::Rect::from_min_size(r.left_top() + vec2(r.width() * 0.1, 0.0), vec2(r.width() * 0.28, r.height())), 2.0, w);
            p.rect_filled(egui::Rect::from_min_size(r.left_top() + vec2(r.width() * 0.62, 0.0), vec2(r.width() * 0.28, r.height())), 2.0, w);
        }
    }
    resp
}

/// `hh:mm:ss` of simulated time.
pub fn clock(us: u64) -> String {
    let s = us / 1_000_000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

impl PacketApp {
    pub fn new(session: Session) -> PacketApp {
        PacketApp { session, ui: UiState::default(), requests: Vec::new() }
    }

    /// Runs an engine command; failures go to the status line.
    pub fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.error(&e);
                None
            }
        }
    }

    fn error(&mut self, e: &EngineError) {
        self.ui.status = Some((e.to_string(), true));
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.ui.status = Some((msg.into(), false));
    }

    /// Host work the shell should do now.
    pub fn take_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Opens a lab file chosen by the shell.
    pub fn open_path(&mut self, path: &str) {
        if self.run("file.open", json!({"path": path})).is_some() {
            self.after_replace();
            self.info(crate::i18n::trf("Opened {}", &[&path]));
        }
    }

    /// Opens lab contents read by the shell (web).
    pub fn open_json(&mut self, json_text: &str, name: Option<&str>) {
        if self.run("file.open", json!({"json": json_text})).is_some() {
            self.session.path = name.map(str::to_string);
            self.after_replace();
        }
    }

    /// A text file the shell read for an [`UiRequest::OpenText`].
    pub fn text_opened(&mut self, device: DeviceId, merge: bool, text: &str) {
        if merge {
            if let Some(v) = self.run("device.configure", json!({"device": device.0, "lines": text})) {
                let errors = v.get("errors").and_then(Value::as_array).map_or(0, Vec::len);
                self.info(if errors == 0 {
                    "Configuration merged.".to_string()
                } else {
                    crate::i18n::trf("Configuration merged; {} line(s) were not understood.", &[&errors])
                });
            }
        } else if self.run("device.startup", json!({"device": device.0, "action": "load", "text": text})).is_some() {
            self.info(tl("Startup configuration loaded: `reload` the device to use it."));
        }
    }

    /// A web page the shell read for an [`UiRequest::ImportWebFile`].
    pub fn web_file_imported(&mut self, device: DeviceId, name: &str, text: &str) {
        let Some(d) = self.session.topology.device(device) else { return };
        let mut sv = d.services.clone();
        let name: String = name.chars().filter(|c| !c.is_whitespace() && *c != '/').take(64).collect();
        sv.http.files.retain(|f| f.name != name);
        sv.http.files.push(packetcraft_model::services::WebFile {
            name: name.clone(),
            content: text.chars().take(packetcraft_model::services::MAX_FILE).collect(),
        });
        if let Ok(v) = serde_json::to_value(&sv)
            && self.run("server.services", json!({"device": device.0, "services": v})).is_some()
        {
            self.info(crate::i18n::trf("Added {} to the web site.", &[&name]));
        }
    }

    /// The shell wrote the lab to `path`.
    pub fn saved_to(&mut self, path: &str) {
        self.session.path = Some(path.to_string());
        self.session.dirty = false;
        self.info(crate::i18n::trf("Saved {}", &[&path]));
    }

    fn after_replace(&mut self) {
        self.ui.windows.clear();
        self.ui.selected = None;
        self.ui.inspect = None;
        self.ui.pdus.clear();
        self.ui.events = events::EventView::default();
        if !self.session.topology.notes.is_empty() {
            self.info(tl("This lab has notes: open them with the notes button."));
        }
    }

    pub fn new_lab(&mut self, sample: Option<&str>) {
        let params = sample.map_or_else(|| json!({}), |s| json!({"sample": s}));
        if self.run("file.new", params).is_some() {
            self.after_replace();
            self.ui.pan = egui::Vec2::ZERO;
            self.ui.zoom = 1.0;
        }
    }

    pub fn save(&mut self, save_as: bool) {
        if let Some(v) = self.run("file.save", json!({})) {
            let json = v.get("json").and_then(Value::as_str).unwrap_or_default().to_string();
            let path = if save_as { None } else { self.session.path.clone() };
            self.requests.push(UiRequest::Save { json, path });
        }
    }

    /// The window title.
    pub fn title(&self) -> String {
        format!("PacketCraft - {}", self.session.title())
    }

    /// Applies the theme (call at start and when it changes).
    pub fn apply_theme(&self, ctx: &egui::Context) {
        let tk = theme::tokens(self.ui.dark);
        let mut v = if self.ui.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        // Text fields with a visible border, as in the reference app's forms.
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, tk.chrome_line);
        v.extreme_bg_color = tk.canvas;
        ctx.set_visuals(v);
    }

    fn copy(&mut self) {
        if let Some(d) = self.ui.selected {
            self.ui.clipboard = Some(d);
            let name = self.session.topology.device(d).map(|x| x.name.clone()).unwrap_or_default();
            self.info(crate::i18n::trf("Copied {}", &[&name]));
        }
    }

    fn paste(&mut self) {
        let Some(d) = self.ui.clipboard else { return };
        if let Some(v) = self.run("device.duplicate", json!({"device": d.0})) {
            self.ui.selected = v.get("id").and_then(Value::as_u64).and_then(|i| u32::try_from(i).ok()).map(DeviceId);
        }
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.ui.tool = t;
        self.ui.canvas.cancel();
    }

    fn zoom(&mut self, factor: Option<f32>) {
        self.ui.zoom = match factor {
            Some(f) => (self.ui.zoom * f).clamp(0.25, 3.0),
            None => 1.0,
        };
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let sc = |m, k| KeyboardShortcut::new(m, k);
        let cmd = Modifiers::COMMAND;
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::N))) {
            self.new_lab(None);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::O))) {
            self.requests.push(UiRequest::Open);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd | Modifiers::SHIFT, Key::S))) {
            self.save(true);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::S))) {
            self.save(false);
        }
        // Text fields keep their own undo and Delete.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd | Modifiers::SHIFT, Key::Z)) || i.consume_shortcut(&sc(cmd, Key::Y))) {
            self.run("edit.redo", json!({}));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::Z))) {
            self.run("edit.undo", json!({}));
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete))
            && let Some(d) = self.ui.selected.take()
        {
            self.run("device.remove", json!({"device": d.0}));
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.set_tool(Tool::Select);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::C))) {
            self.copy();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::V))) {
            self.paste();
        }
        for (k, t) in [(Key::I, Tool::Inspect), (Key::P, Tool::Ping), (Key::N, Tool::Note)] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k)) {
                self.set_tool(t);
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::SHIFT, Key::S))) {
            self.ui.mode = if self.ui.mode == Mode::Realtime { Mode::Simulation } else { Mode::Realtime };
        }
    }

    /// Draws the whole app into `ui` (the root of the window).
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let tk = theme::tokens(self.ui.dark);
        self.shortcuts(&ctx);
        let chrome = egui::Frame::new().fill(tk.chrome).inner_margin(egui::Margin::symmetric(6, 4));
        let bar = egui::Frame::new().fill(tk.bar).inner_margin(egui::Margin::symmetric(6, 3));
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").frame(chrome.stroke(egui::Stroke::new(1.0, tk.chrome_line))).show(ui, |ui| self.main_toolbar(ui));
        egui::Panel::top("tools").frame(chrome.stroke(egui::Stroke::new(1.0, tk.chrome_line))).show(ui, |ui| self.tool_bar(ui));
        egui::Panel::top("workspace_bar").frame(bar).show(ui, |ui| self.workspace_bar(ui));
        egui::Panel::bottom("device_box").frame(chrome).exact_size(160.0).show(ui, |ui| bottom::show(self, ui));
        egui::Panel::bottom("time_bar").frame(bar).show(ui, |ui| self.time_bar(ui));
        if self.ui.mode == Mode::Simulation && self.ui.show_event_list {
            egui::Panel::right("simulation_panel").resizable(true).default_size(420.0).show(ui, |ui| events::panel(self, ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| canvas::show(self, ui));
        console::windows(self, &ctx);
        canvas::inspect_window(self, &ctx);
        canvas::note_editor(self, &ctx);
        events::pdu_window(self, &ctx);
        self.notes_window(&ctx);
        activity::windows(self, &ctx);
        self.about_window(&ctx);
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tl("File"), |ui| {
                if ui.button(tl("New")).clicked() {
                    self.new_lab(None);
                }
                ui.menu_button(tl("New from sample"), |ui| {
                    for s in sample::SAMPLES {
                        if ui.button(crate::i18n::tr(s.title)).on_hover_text(crate::i18n::tr(s.description)).clicked() {
                            self.new_lab(Some(s.id));
                        }
                    }
                });
                if ui.button(tl("Open...")).clicked() {
                    self.requests.push(UiRequest::Open);
                }
                if !self.ui.recent.is_empty() {
                    ui.menu_button(tl("Open Recent"), |ui| {
                        for p in self.ui.recent.clone() {
                            let name = std::path::Path::new(&p).file_name().map_or(p.clone(), |n| n.to_string_lossy().to_string());
                            if ui.button(name).on_hover_text(&p).clicked() {
                                self.open_path(&p);
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button(tl("Save")).clicked() {
                    self.save(false);
                }
                if ui.button(tl("Save As...")).clicked() {
                    self.save(true);
                }
                if ui.button(tl("Export Capture (pcap)...")).on_hover_text(tl("The event list as a capture any packet analyser opens")).clicked() {
                    self.requests.push(UiRequest::ExportPcap(self.session.pcap()));
                }
                ui.separator();
                if ui.button(tl("Exit")).clicked() {
                    self.requests.push(UiRequest::Quit);
                }
            });
            ui.menu_button(tl("Edit"), |ui| {
                if ui.add_enabled(self.session.can_undo(), egui::Button::new(tl("Undo"))).clicked() {
                    self.run("edit.undo", json!({}));
                }
                if ui.add_enabled(self.session.can_redo(), egui::Button::new(tl("Redo"))).clicked() {
                    self.run("edit.redo", json!({}));
                }
                ui.separator();
                if ui.add_enabled(self.ui.selected.is_some(), egui::Button::new(tl("Delete"))).clicked()
                    && let Some(d) = self.ui.selected.take()
                {
                    self.run("device.remove", json!({"device": d.0}));
                }
            });
            ui.menu_button(tl("Options"), |ui| {
                if ui.checkbox(&mut self.ui.dark, tl("Dark interface")).changed() {
                    self.apply_theme(ui.ctx());
                }
                ui.checkbox(&mut self.ui.show_grid, tl("Show grid"));
                ui.menu_button(tl("Language"), |ui| {
                    for l in i18n::Lang::ALL {
                        if ui.radio(i18n::lang() == l, l.name()).clicked() {
                            i18n::set_lang(l);
                            self.ui.lang_changed = true;
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button(tl("View"), |ui| {
                if ui.button(tl("Zoom In")).clicked() {
                    self.zoom(Some(1.25));
                }
                if ui.button(tl("Zoom Reset")).clicked() {
                    self.zoom(None);
                    self.ui.pan = egui::Vec2::ZERO;
                }
                if ui.button(tl("Zoom Out")).clicked() {
                    self.zoom(Some(0.8));
                }
            });
            ui.menu_button(tl("Tools"), |ui| {
                if ui.button(tl("Inspect")).clicked() {
                    self.set_tool(Tool::Inspect);
                    ui.close();
                }
                if ui.button(tl("Add Simple PDU")).clicked() {
                    self.set_tool(Tool::Ping);
                    ui.close();
                }
                if ui.button(tl("Reset Simulation")).clicked() {
                    self.run("sim.reset", json!({}));
                }
            });
            ui.menu_button(tl("Extensions"), |ui| {
                if ui.button(tl("Activity Wizard…")).clicked() {
                    self.ui.activity.show_wizard = true;
                    ui.close();
                }
                if ui.add_enabled(self.session.topology.activity.is_some(), egui::Button::new(tl("Activity Instructions"))).clicked() {
                    self.ui.activity.show_instructions = true;
                    ui.close();
                }
                ui.separator();
                ui.label(tl("Agents and scripts drive PacketCraft through"));
                ui.monospace("packetcraft-cli mcp");
                ui.monospace("packetcraft-cli run --cmd ...");
            });
            ui.menu_button(tl("Window"), |ui| {
                ui.checkbox(&mut self.ui.show_notes, tl("Lab Notes"));
                let mut sim = self.ui.mode == Mode::Simulation;
                if ui.checkbox(&mut sim, tl("Simulation Panel")).changed() {
                    self.ui.mode = if sim { Mode::Simulation } else { Mode::Realtime };
                }
            });
            ui.menu_button(tl("Help"), |ui| {
                if ui.button(tl("About")).clicked() {
                    self.ui.show_about = true;
                    ui.close();
                }
            });
        });
    }

    fn main_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if tool_button(ui, Glyph::New, "New (Ctrl+N)", false, true).clicked() {
                self.new_lab(None);
            }
            if tool_button(ui, Glyph::Open, "Open (Ctrl+O)", false, true).clicked() {
                self.requests.push(UiRequest::Open);
            }
            if tool_button(ui, Glyph::Save, "Save (Ctrl+S)", false, true).clicked() {
                self.save(false);
            }
            ui.add_space(8.0);
            if tool_button(ui, Glyph::Info, "Network Information (the lab's notes)", self.ui.show_notes, true).clicked() {
                self.ui.show_notes = !self.ui.show_notes;
            }
            ui.add_space(8.0);
            if tool_button(ui, Glyph::Copy, "Copy (Ctrl+C): the selected device", false, self.ui.selected.is_some()).clicked() {
                self.copy();
            }
            if tool_button(ui, Glyph::Paste, "Paste (Ctrl+V): a copy of the copied device", false, self.ui.clipboard.is_some()).clicked() {
                self.paste();
            }
            if tool_button(ui, Glyph::Undo, "Undo (Ctrl+Z)", false, self.session.can_undo()).clicked() {
                self.run("edit.undo", json!({}));
            }
            if tool_button(ui, Glyph::Redo, "Redo (Ctrl+Y)", false, self.session.can_redo()).clicked() {
                self.run("edit.redo", json!({}));
            }
            ui.add_space(8.0);
            if tool_button(ui, Glyph::ZoomIn, "Zoom In", false, true).clicked() {
                self.zoom(Some(1.25));
            }
            if tool_button(ui, Glyph::ZoomReset, "Zoom Reset", false, true).clicked() {
                self.zoom(None);
                self.ui.pan = egui::Vec2::ZERO;
            }
            if tool_button(ui, Glyph::ZoomOut, "Zoom Out", false, true).clicked() {
                self.zoom(Some(0.8));
            }
            ui.add_space(8.0);
            if tool_button(ui, Glyph::Capture, "Export Capture (pcap)", false, true).clicked() {
                self.requests.push(UiRequest::ExportPcap(self.session.pcap()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if tool_button(ui, Glyph::Help, "About PacketCraft", false, true).clicked() {
                    self.ui.show_about = true;
                }
            });
        });
    }

    fn tool_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let t = self.ui.tool;
            for (g, tool, label) in [
                (Glyph::Select, Tool::Select, "Select (Esc)"),
                (Glyph::Inspect, Tool::Inspect, "Inspect (I)"),
                (Glyph::Delete, Tool::Delete, "Delete (Del)"),
            ] {
                if tool_button(ui, g, label, t == tool, true).clicked() {
                    self.set_tool(tool);
                }
            }
            tool_button(ui, Glyph::Resize, "Resize Shape (not available yet)", false, false);
            ui.add_space(8.0);
            if tool_button(ui, Glyph::Note, "Place Note (N)", t == Tool::Note, true).clicked() {
                self.set_tool(Tool::Note);
            }
            for (g, label, shape) in [
                (Glyph::Line, "Draw Line", packetcraft_model::Shape::Line),
                (Glyph::Rectangle, "Draw Rectangle", packetcraft_model::Shape::Rectangle),
                (Glyph::Ellipse, "Draw Ellipse", packetcraft_model::Shape::Ellipse),
            ] {
                if tool_button(ui, g, &crate::i18n::trf("{} (drag on the workspace)", &[&crate::i18n::tr(label)]), t == Tool::Draw(shape), true)
                    .clicked()
                {
                    self.set_tool(Tool::Draw(shape));
                }
            }
            tool_button(ui, Glyph::Freeform, "Draw Freeform (not available yet)", false, false);
            ui.add_space(8.0);
            if tool_button(ui, Glyph::SimplePdu, "Add Simple PDU (P)", t == Tool::Ping, true).clicked() {
                self.set_tool(Tool::Ping);
            }
            tool_button(ui, Glyph::ComplexPdu, "Add Complex PDU (not available yet)", false, false);
        });
    }

    fn workspace_bar(&mut self, ui: &mut egui::Ui) {
        let tk = theme::tokens(self.ui.dark);
        ui.horizontal(|ui| {
            pill(ui, Glyph::Logical, "Logical", true);
            if pill(ui, Glyph::Physical, "Physical", false).clicked() {
                self.info(tl("The physical workspace (buildings, closets, racks) is not available yet."));
            }
            let coords = self.ui.pointer.map_or(String::new(), |(x, y)| format!("x: {}, y: {}", x.round() as i64, y.round() as i64));
            ui.label(RichText::new(coords).color(tk.bar_text).size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (rect, _) = ui.allocate_exact_size(vec2(104.0, 28.0), Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_stroke(rect, 8.0, egui::Stroke::new(1.5, tk.bar_text), egui::StrokeKind::Inside);
                icons::glyph(&p, egui::Rect::from_min_size(rect.min + vec2(5.0, 4.0), vec2(20.0, 20.0)), Glyph::Clock, tk.pill_text);
                p.text(
                    rect.min + vec2(29.0, 14.0),
                    egui::Align2::LEFT_CENTER,
                    clock(self.session.sim.now_us()),
                    egui::FontId::proportional(17.0),
                    tk.pill_text,
                );
                for (g, label) in [
                    (Glyph::Physical, "Set Background (not available yet)"),
                    (Glyph::ZoomReset, "Navigation: back to the start of the workspace"),
                    (Glyph::Undo, "Back to the parent cluster (Root is the top)"),
                ] {
                    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::click());
                    let resp = resp.on_hover_text(crate::i18n::tr(label));
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, label));
                    let p = ui.painter_at(r);
                    if resp.hovered() {
                        p.rect_filled(r.shrink(1.5), 7.0, tk.pill.gamma_multiply(0.5));
                    }
                    p.rect_stroke(r, 8.0, egui::Stroke::new(1.5, tk.bar_text), egui::StrokeKind::Inside);
                    icons::glyph(&p, egui::Rect::from_center_size(r.center(), vec2(20.0, 20.0)), g, tk.pill_text);
                    if resp.clicked() && g == Glyph::ZoomReset {
                        self.ui.pan = egui::Vec2::ZERO;
                        self.ui.zoom = 1.0;
                    }
                }
                ui.label(RichText::new(tl("Root")).color(tk.bar_text).size(17.0));
            });
        });
    }

    fn time_bar(&mut self, ui: &mut egui::Ui) {
        let tk = theme::tokens(self.ui.dark);
        ui.horizontal(|ui| {
            let now = self.session.sim.now_us();
            let time = if self.ui.mode == Mode::Simulation {
                format!("{}: {}.{:03}", tl("Time"), clock(now), (now / 1000) % 1000)
            } else {
                format!("{}: {}", tl("Time"), clock(now))
            };
            ui.label(RichText::new(time).color(tk.bar_text).size(18.0));
            let white = |ui: &mut egui::Ui, g: Glyph, label: &str| {
                let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
                let resp = resp.on_hover_text(crate::i18n::tr(label));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, label));
                let p = ui.painter_at(rect);
                if resp.hovered() {
                    p.rect_filled(rect, 4.0, tk.pill.gamma_multiply(0.4));
                }
                p.rect_stroke(rect.shrink(1.0), 8.0, egui::Stroke::new(1.5, tk.bar_text), egui::StrokeKind::Inside);
                icons::glyph(&p, egui::Rect::from_center_size(rect.center(), vec2(22.0, 22.0)), g, tk.bar_text);
                resp
            };
            if white(ui, Glyph::PowerCycle, "Power Cycle Devices (forget ARP and MAC tables)").clicked() {
                self.run("sim.reset", json!({}));
                self.info(tl("Devices power-cycled: dynamic tables cleared."));
            }
            if self.ui.mode == Mode::Simulation {
                ui.label(RichText::new(tl("PLAY CONTROLS:")).color(tk.bar_text).size(16.0));
                events::play_controls(self, ui, 30.0);
            } else if white(ui, Glyph::FastForward, "Fast Forward Time (30 s)").clicked() {
                self.run("sim.advance", json!({"seconds": 30}));
            }
            ui.add_space(12.0);
            let (msg, color) = match &self.ui.status {
                Some((m, true)) => (m.clone(), Color32::from_rgb(0xff, 0x9a, 0x9a)),
                Some((m, false)) => (m.clone(), tk.bar_text),
                None => (canvas::hint(self), tk.bar_text.gamma_multiply(0.55)),
            };
            ui.label(RichText::new(msg).color(color).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill(ui, Glyph::Simulation, "Simulation", self.ui.mode == Mode::Simulation).clicked() {
                    self.ui.mode = Mode::Simulation;
                }
                if pill(ui, Glyph::Clock, "Realtime", self.ui.mode == Mode::Realtime).clicked() {
                    self.ui.mode = Mode::Realtime;
                    self.ui.events.playing = false;
                }
                if self.ui.mode == Mode::Simulation && pill(ui, Glyph::Note, "Event List", self.ui.show_event_list).clicked() {
                    self.ui.show_event_list = !self.ui.show_event_list;
                }
                if self.session.sim.storm_detected() {
                    ui.label(RichText::new(tl("Switching loop detected")).color(Color32::from_rgb(0xff, 0x9a, 0x9a)))
                        .on_hover_text(tl("A frame circled the network; there is no spanning tree yet, so avoid redundant switch links"));
                }
            });
        });
    }

    fn notes_window(&mut self, ctx: &egui::Context) {
        if !self.ui.show_notes {
            return;
        }
        let mut open = true;
        // Sample notes show in the interface's language; an edit saves what was edited.
        let shown = crate::i18n::tr(&self.session.topology.notes).to_string();
        let mut text = shown.clone();
        let right = if self.ui.mode == Mode::Simulation { -440.0 } else { -12.0 };
        egui::Window::new(tl("Lab Notes")).open(&mut open).default_size([320.0, 160.0]).anchor(egui::Align2::RIGHT_TOP, [right, 190.0]).show(
            ctx,
            |ui| {
                let l = ui.label(tl("Notes saved with the lab:"));
                let r = ui
                    .add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY).hint_text(tl("Instructions or notes")))
                    .labelled_by(l.id);
                if r.lost_focus() && text != shown {
                    self.run("lab.notes", json!({"text": text}));
                }
            },
        );
        self.ui.show_notes = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.ui.show_about {
            return;
        }
        let mut open = true;
        egui::Window::new(tl("About PacketCraft")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.heading(tl("PacketCraft"));
            ui.label(crate::i18n::trf("Version {}", &[&env!("CARGO_PKG_VERSION")]));
            ui.label(tl("An open-source network simulator: build labs, configure routers and switches, and watch every frame."));
            ui.label(tl("Licensed under MIT or Apache-2.0."));
            ui.separator();
            ui.label(RichText::new(tl("Independent clean-room project; not affiliated with any network equipment vendor.")).small());
        });
        self.ui.show_about = open;
    }
}
