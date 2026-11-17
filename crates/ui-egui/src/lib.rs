//! NetLabCraft L6: the egui interface.
//!
//! [`PacketApp`] draws everything inside one `egui::Ui`. The arrangement follows the reference
//! app learners know (menus and two toolbars on top, a navy workspace bar, the workspace, a navy
//! time bar with the Realtime/Simulation switch, and the device box with the PDU list at the
//! bottom); every picture is drawn in code (`icons.rs`). It is thin: it reads the engine's
//! [`Session`] and acts through [`PacketApp::run`]. File dialogs and other host work go out as
//! [`UiRequest`]s for the shell (desktop or web).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use crate::prefs_look::HintExt;
use std::collections::BTreeMap;

use egui::{Color32, RichText, Sense, vec2};
use netlabcraft_core::DeviceId;
use netlabcraft_engine::{EngineError, Session, sample};

pub use print::{PdfUse, PrintChoices, PrintJob};

use serde_json::{Value, json};

pub mod activity;
mod board_page;
mod bottom;
mod canvas;
mod cell_page;
mod closet;
mod cloud_page;
mod console;
mod credits;
pub mod environments;
mod events;
pub mod fonts;
mod hardware;
mod help;
mod host_apps;
pub mod i18n;
pub mod icons;
mod icons_box;
mod icons_cable_tiles;
pub mod icons_families;
pub mod icons_gear;
mod icons_network;
mod icons_parts;
mod icons_ports;
mod icons_things;
pub mod instructor;
mod iot_page;
mod mail;
mod mib;
mod overview;
mod palette;
pub mod pdf;
pub mod pdu_dialog;
mod phone;
mod physical;
pub mod png;
pub mod prefs;
mod prefs_admin;
mod prefs_look;
pub mod prefs_misc;
mod prefs_tabs;
mod print;
mod programming;
mod services;
mod shortcuts;
mod sniffer_page;
pub mod sound;
pub mod theme;
mod touch;
mod wifi_form;
mod wireless;
mod wlan_page;

#[cfg(test)]
mod tests;
mod tests_closet;
mod tests_environments;
mod tests_families;
#[cfg(test)]
mod tests_prefs;
mod tests_prefs2;

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
    /// Ask where to write a PNG image of the workspace.
    ExportImage(Vec<u8>),
    /// Keep the report PDF: ask where to write it (desktop) or download it (web). `name` is the
    /// suggested file name.
    SavePdf { name: String, bytes: Vec<u8> },
    /// Open the report PDF in the system's PDF viewer (desktop; only after
    /// [`UiState::print_choices`] says a viewer exists).
    OpenPdf { name: String, bytes: Vec<u8> },
    /// Send the report PDF to the default printer (desktop; only when
    /// [`PrintChoices::direct_unavailable`] is `None`).
    PrintPdf { name: String, bytes: Vec<u8> },
    /// Pick a text file for a device: its startup configuration (`merge: false`) or lines to
    /// merge into the running configuration; then call [`PacketApp::text_opened`].
    OpenText { device: DeviceId, merge: bool },
    /// Pick an .html file to add to a server's web site; then call
    /// [`PacketApp::web_file_imported`].
    ImportWebFile { device: DeviceId },
    /// Pick an environment file (JSON) for a place; then call [`PacketApp::environment_imported`].
    ImportEnvironment { place: u32 },
    /// Play a short sound (the bytes of a WAV file); a shell with no way to play one ignores it.
    PlaySound(Vec<u8>),
    /// Ask for a folder (desktop), then call [`PacketApp::folder_picked`].
    PickFolder,
    /// Write a backup copy of the open lab (desktop): the shell makes the file name from `stem`
    /// and the time, in a `backups` folder inside `folder` (or in its settings folder), and runs
    /// `file.backup`.
    Backup { stem: String, folder: Option<String> },
    /// Close the app.
    Quit,
}

/// How far an arrow key moves the selected device (workspace units).
pub(crate) const GRID_STEP: f32 = 20.0;

/// The active workspace tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Inspect,
    /// Place a device of a catalogue model (`models::MODELS` id).
    Place(&'static str),
    /// Cable two ports with a cable of this kind.
    Cable(netlabcraft_model::cable::Cable),
    /// Place a note on the workspace.
    Note,
    /// Drag out a rectangle, ellipse or line.
    Draw(netlabcraft_model::Shape),
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

/// A native window's inner size and, when the platform tells, its position (points).
pub type NativeGeometry = ([f32; 2], Option<[f32; 2]>);

/// Most native-window geometries remembered across sessions (one per device name).
pub const MAX_REMEMBERED_WINDOWS: usize = 256;

/// A remembered geometry made safe to use: finite numbers, a size no smaller than the window
/// takes and no larger than 8192 points, a position within 32768 points of the origin (a
/// position outside that is dropped, the size kept). `None` when the size is not usable.
fn clean_geometry(g: NativeGeometry) -> Option<NativeGeometry> {
    let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
    let ([w, h], pos) = g;
    if !(ok(w, 320.0, 8192.0) && ok(h, 240.0, 8192.0)) {
        return None;
    }
    Some(([w, h], pos.filter(|p| ok(p[0], -32768.0, 32768.0) && ok(p[1], -32768.0, 32768.0))))
}

/// The remembered native-window geometries as text, one line per device: `W H X Y NAME`, with
/// `-` for an unknown position. The desktop shell keeps it between sessions.
pub fn native_geometry_to_text(map: &BTreeMap<String, NativeGeometry>) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for (name, g) in map.iter().take(MAX_REMEMBERED_WINDOWS) {
        let Some(([w, h], pos)) = clean_geometry(*g) else { continue };
        if name.trim().is_empty() || name.contains(['\n', '\r']) {
            continue;
        }
        let (x, y) = pos.map_or(("-".to_string(), "-".to_string()), |p| (format!("{:.0}", p[0]), format!("{:.0}", p[1])));
        let _ = writeln!(s, "{w:.0} {h:.0} {x} {y} {name}");
    }
    s
}

/// The geometries `native_geometry_to_text` wrote. Hostile or damaged text gives fewer entries,
/// never an error: a line that does not read, or whose numbers are not usable, is skipped.
pub fn native_geometry_from_text(text: &str) -> BTreeMap<String, NativeGeometry> {
    let mut out = BTreeMap::new();
    for line in text.lines().take(MAX_REMEMBERED_WINDOWS * 2) {
        let mut it = line.splitn(5, ' ');
        let (Some(w), Some(h), Some(x), Some(y), Some(name)) = (it.next(), it.next(), it.next(), it.next(), it.next()) else { continue };
        let num = |s: &str| s.parse::<f32>().ok();
        let (Some(w), Some(h)) = (num(w), num(h)) else { continue };
        let pos = match (num(x), num(y)) {
            (Some(x), Some(y)) => Some([x, y]),
            _ => None,
        };
        let name = name.trim();
        if name.is_empty() || out.len() >= MAX_REMEMBERED_WINDOWS {
            continue;
        }
        if let Some(g) = clean_geometry(([w, h], pos)) {
            out.insert(name.to_string(), g);
        }
    }
    out
}

/// Which toolbars View ▸ Toolbars shows: the main bar (file, edit, zoom), the secondary bar (tools) and the
/// bottom bar (the device box). All three are shown until the person hides one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toolbars {
    pub main: bool,
    pub secondary: bool,
    pub bottom: bool,
}

impl Default for Toolbars {
    fn default() -> Self {
        Toolbars { main: true, secondary: true, bottom: true }
    }
}

impl Toolbars {
    /// The choice as text, one `name on|off` line per bar. The desktop shell keeps it between sessions.
    pub fn to_text(self) -> String {
        let word = |on: bool| if on { "on" } else { "off" };
        format!("main {}\nsecondary {}\nbottom {}\n", word(self.main), word(self.secondary), word(self.bottom))
    }

    /// What `to_text` wrote. Damaged or hostile text keeps the default (shown) for what it does not say clearly.
    pub fn from_text(text: &str) -> Toolbars {
        let mut t = Toolbars::default();
        for line in text.lines().take(16) {
            let mut words = line.split_whitespace();
            let (Some(name), Some(value)) = (words.next(), words.next()) else { continue };
            let on = match value {
                "on" => true,
                "off" => false,
                _ => continue,
            };
            match name {
                "main" => t.main = on,
                "secondary" => t.secondary = on,
                "bottom" => t.bottom = on,
                _ => {}
            }
        }
        t
    }
}

/// View state that isn't part of the lab.
#[derive(Debug, Clone)]
pub struct UiState {
    pub dark: bool,
    pub show_grid: bool,
    /// The Open Samples window (Ctrl+Shift+T).
    pub show_samples: bool,
    /// The toolbars shown (View ▸ Toolbars; the desktop shell saves the choice).
    pub toolbars: Toolbars,
    /// View ▸ Show Viewport: the overview of the workspace you click to pan.
    pub show_viewport: bool,
    /// View ▸ Show Workspace List: every device with its name, model and type.
    pub show_workspace_list: bool,
    /// The Help window's page, while it is open.
    pub(crate) help: Option<help::Page>,
    pub tool: Tool,
    pub mode: Mode,
    pub selected: Option<DeviceId>,
    /// Devices and clusters picked together (Shift+click, Shift+drag), for grouping or moving.
    pub picked: std::collections::BTreeSet<DeviceId>,
    pub picked_clusters: std::collections::BTreeSet<u32>,
    /// The cluster being renamed, and the name typed so far.
    pub renaming_cluster: Option<(u32, String)>,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub windows: BTreeMap<DeviceId, console::DeviceWindow>,
    /// Device windows open as separate operating-system windows (Options menu, saved by the
    /// desktop shell). Ignored where the backend has no native viewports (the web build).
    pub native_windows: bool,
    /// Size and position a device's native window had last, kept for the session.
    pub(crate) native_geometry: BTreeMap<DeviceId, NativeGeometry>,
    /// Size and position by device name, for the next session: the desktop shell loads it at
    /// start and saves it when it changes (see `native_geometry_to_text`). A window with no
    /// geometry of its own this session opens with the one under its device's name.
    pub native_remembered: BTreeMap<String, NativeGeometry>,
    /// The cable whose accessible node has the keyboard focus (named by its first end), drawn
    /// highlighted.
    pub cable_focus: Option<netlabcraft_model::End>,
    /// The geometry each open native window was opened with (fixed while it is open).
    pub(crate) native_start: BTreeMap<DeviceId, NativeGeometry>,
    /// Native windows to bring to the front on their next frame.
    pub(crate) raise: std::collections::BTreeSet<DeviceId>,
    pub inspect: Option<DeviceId>,
    pub show_notes: bool,
    /// The Simulation Panel is shown in simulation mode ("Event List" toggle).
    pub show_event_list: bool,
    pub show_about: bool,
    /// About window tab: false = the app, true = its contributors.
    pub about_credits: bool,
    pub status: Option<(String, bool)>,
    pub events: events::EventView,
    /// The Create Complex PDU window, while open.
    pub complex: Option<pdu_dialog::Draft>,
    pub bottom: bottom::BoxState,
    /// The pointer over the workspace, in workspace units.
    pub pointer: Option<(f32, f32)>,
    /// A device-box tile being dragged to the workspace: the model or passive device id and its name.
    pub tile_drag: Option<(&'static str, &'static str)>,
    /// A tile released over the workspace, to be placed there: the id and where (screen position).
    pub tile_drop: Option<(&'static str, egui::Pos2)>,
    /// The device Copy put on the clipboard.
    pub clipboard: Option<DeviceId>,
    /// The workspace note being edited, and its text.
    pub editing_note: Option<(u32, String)>,
    /// The rectangle, ellipse or line showing its resize handles.
    pub selected_note: Option<u32>,
    /// The Palette window: the default look of new drawings.
    pub(crate) palette: palette::State,
    /// Labs opened or saved recently, newest first (the desktop shell fills it).
    pub recent: Vec<String>,
    /// The person picked another interface language (the shell saves the choice).
    pub lang_changed: bool,
    /// Where the workspace was drawn last frame (points), and a screenshot asked for it.
    pub canvas_rect: Option<egui::Rect>,
    pub image_pending: bool,
    /// The pending screenshot is for a PDF (File ▸ Save as PDF or Print), not for the PNG export.
    pub pdf_for: Option<PdfUse>,
    /// What the shell can do with a PDF besides saving it; `None` (the web build) makes Print
    /// download the file like Save as PDF.
    pub print_choices: Option<PrintChoices>,
    /// A PDF waiting in the Print window for the person's choice.
    pub print_job: Option<PrintJob>,
    /// Where the screenshot lands when it is ready.
    pub shot: std::sync::Arc<netlabcraft_core::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
    /// Activity instructions and wizard.
    pub activity: activity::ActivityView,
    canvas: canvas::CanvasState,
    /// The physical workspace (drags, menus, dialogs).
    pub physical: physical::PhysState,
    /// Options ▸ Preferences: what the interface shows and does (the shell keeps it).
    pub prefs: prefs::Prefs,
    /// Which of the Preferences, User Profile, Command Log and Algorithm Settings windows are open.
    pub prefs_ui: prefs::WindowState,
    /// Options ▸ Environments: the window's state.
    pub env_ui: environments::EnvUi,
}

impl UiState {
    /// Whether a device window is open as an operating-system window of its own.
    pub fn native_window_open(&self) -> bool {
        !self.native_start.is_empty()
    }
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            dark: false,
            show_grid: false,
            show_samples: false,
            toolbars: Toolbars::default(),
            show_viewport: false,
            show_workspace_list: false,
            help: None,
            tool: Tool::Select,
            mode: Mode::Realtime,
            selected: None,
            picked: Default::default(),
            picked_clusters: Default::default(),
            renaming_cluster: None,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            windows: BTreeMap::new(),
            native_windows: false,
            native_geometry: BTreeMap::new(),
            native_start: BTreeMap::new(),
            native_remembered: BTreeMap::new(),
            cable_focus: None,
            raise: Default::default(),
            inspect: None,
            show_notes: false,
            show_event_list: true,
            show_about: false,
            about_credits: false,
            status: None,
            events: events::EventView::default(),
            complex: None,
            bottom: bottom::BoxState::default(),
            pointer: None,
            tile_drag: None,
            tile_drop: None,
            clipboard: None,
            editing_note: None,
            selected_note: None,
            palette: palette::State::default(),
            recent: Vec::new(),
            lang_changed: false,
            canvas_rect: None,
            image_pending: false,
            pdf_for: None,
            print_choices: None,
            print_job: None,
            shot: Default::default(),
            activity: activity::ActivityView::default(),
            canvas: canvas::CanvasState::default(),
            physical: physical::PhysState::default(),
            prefs: prefs::Prefs::default(),
            prefs_ui: prefs::WindowState::default(),
            env_ui: environments::EnvUi::default(),
        }
    }
}

/// Opens the Create Complex PDU window with `source` picked.
pub fn open_complex_pdu(app: &mut PacketApp, source: Option<DeviceId>) {
    pdu_dialog::open(app, source);
}

/// Opens (or raises) a device's window.
pub fn open_console(app: &mut PacketApp, d: DeviceId) {
    console::open(app, d);
}

/// Opens a host's Desktop: `"prompt"`, `"ip"` (IP Configuration), `"email"`, `"mib"`, `"terminal"`,
/// `"programming"`, `"telnet"` (Telnet/SSH Client), `"editor"` (Text Editor), `"firewall"`, `"firewall6"`,
/// `"traffic"` (Traffic Generator), `"pppoe"` (PPPoE Dialer), `"dialup"` (Dial-up), `"vpn"` (VPN Client), `"netflow"` (Netflow Collector) or the tiles (anything else).
pub fn open_desktop_app(app: &mut PacketApp, d: DeviceId, which: &str) {
    console::open(app, d);
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.tab = console::Tab::Desktop;
        w.desktop = match which {
            "prompt" => Some(console::DesktopApp::Prompt),
            "ip" => Some(console::DesktopApp::IpConfig),
            "email" => Some(console::DesktopApp::Email),
            "mib" => Some(console::DesktopApp::MibBrowser),
            "terminal" => Some(console::DesktopApp::Terminal),
            "programming" => Some(console::DesktopApp::Programming),
            "settings" => Some(console::DesktopApp::Settings),
            "telnet" => Some(console::DesktopApp::TelnetSsh),
            "editor" => Some(console::DesktopApp::TextEditor),
            "firewall" => Some(console::DesktopApp::Firewall),
            "firewall6" => Some(console::DesktopApp::Firewall6),
            "traffic" => Some(console::DesktopApp::TrafficGenerator),
            "pppoe" => Some(console::DesktopApp::PppoeDialer),
            "dialup" => Some(console::DesktopApp::DialUp),
            "vpn" => Some(console::DesktopApp::VpnClient),
            "netflow" => Some(console::DesktopApp::NetflowCollector),
            "softphone" => Some(console::DesktopApp::Softphone),
            _ => None,
        };
    }
}

/// Shows the Connections group of the device box; with a cable id (`"fiber"`), picks that
/// cable's tool.
pub fn show_connections(app: &mut PacketApp, cable: Option<&str>) {
    app.ui.bottom.category = bottom::Category::Connections;
    app.ui.bottom.sub = bottom::Sub::Cables;
    app.ui.bottom.search.clear();
    if let Some(c) = cable.and_then(netlabcraft_model::cable::Cable::parse) {
        app.set_tool(Tool::Cable(c));
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
/// `aaa`, `ftp`, `email`, `pppoe`).
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
        "pppoe" => Page::Pppoe,
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

/// Opens a device window on a tab by name: `physical`, `config`, `cli`, `services`, `desktop`,
/// `phone`, `iot`, `capture` (a sniffer) or `attributes` (anything else keeps the default).
pub fn open_tab(app: &mut PacketApp, d: DeviceId, name: &str) {
    use console::Tab;
    console::open(app, d);
    let tab = match name {
        "physical" => Tab::Physical,
        "config" => Tab::Config,
        "cli" => Tab::Cli,
        "services" => Tab::Services,
        "desktop" => Tab::Desktop,
        "phone" => Tab::Phone,
        "phone-settings" => {
            if let Some(w) = app.ui.windows.get_mut(&d) {
                w.phone.open = true;
            }
            Tab::Phone
        }
        "iot" => Tab::Iot,
        "programming" | "board" => Tab::Board,
        "capture" => Tab::Capture,
        "attributes" => Tab::Attributes,
        _ => return,
    };
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.tab = tab;
    }
}

/// Opens a device's Config tab on one page: `settings`, `home`, `vlans`, `routes`, `wireless`
/// `cellular` (a central office server or a cell tower) or `port:N` (the N-th port, from 0).
pub fn open_config_page(app: &mut PacketApp, d: DeviceId, page: &str) {
    use console::ConfigPage;
    open_config_tab(app, d);
    let p = match page {
        "home" => ConfigPage::Home,
        "vlans" => ConfigPage::Vlans,
        "routes" => ConfigPage::Routes,
        "wireless" => ConfigPage::Wireless,
        "cellular" => ConfigPage::Cellular,
        other => match other.strip_prefix("port:").and_then(|n| n.parse().ok()) {
            Some(n) => ConfigPage::Port(n),
            None => ConfigPage::Settings,
        },
    };
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.page = p;
    }
}

/// Opens a device's Physical tab (power switch, and the bays of a modular router).
pub fn open_physical_tab(app: &mut PacketApp, d: DeviceId) {
    console::open(app, d);
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.tab = console::Tab::Physical;
    }
}

/// Where windows without a place of their own open: below the menu and the toolbars, so they
/// never start over them.
pub(crate) const BELOW_TOOLBARS: [f32; 2] = [300.0, 180.0];

/// The top of the workspace: below the menu, the two toolbars and the Logical/Physical bar, where
/// device windows start so they never cover the view's controls.
pub(crate) const BELOW_VIEW_BAR: f32 = 170.0;

/// The application.
pub struct PacketApp {
    pub session: Session,
    pub ui: UiState,
    requests: Vec<UiRequest>,
}

/// A square toolbar button with a glyph; `label` is its tooltip and accessible name.
/// A menu row that is on or off (highlighted when on) with its shortcut shown at the right; true when clicked.
fn check_item(ui: &mut egui::Ui, on: bool, label: &str, shortcut: &str) -> bool {
    ui.add(egui::Button::new(label).shortcut_text(shortcut).selected(on)).clicked()
}

fn tool_button(ui: &mut egui::Ui, g: Glyph, label: &str, selected: bool, enabled: bool) -> egui::Response {
    let tk = theme::tokens(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(vec2(38.0, 36.0), if enabled { Sense::click() } else { Sense::hover() });
    let label = crate::i18n::tr(label);
    let resp = resp.hint(label);
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

/// How wide [`pill`] draws a tab with this label.
fn pill_width(ui: &egui::Ui, label: &str) -> f32 {
    text_width(ui, crate::i18n::tr(label), egui::FontId::proportional(20.0)) + 38.0
}

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x
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

/// [`navy_button`] that fits the width it is given: the text wraps onto more lines (centred)
/// rather than running past the column.
pub(crate) fn navy_button_fit(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let label = crate::i18n::tr(label);
    let width = ui.available_width().max(40.0);
    let mut job = egui::text::LayoutJob::simple(label.to_string(), egui::FontId::proportional(16.0), Color32::WHITE, width - 16.0);
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let (rect, resp) = ui.allocate_exact_size(vec2((galley.size().x + 16.0).min(width), galley.size().y + 10.0), Sense::click());
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
    // By the text's own box, so centred (wrapped) text lands in the middle too.
    ui.painter().galley(rect.center() - galley.rect.center().to_vec2(), galley, Color32::WHITE);
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
    let label = crate::i18n::tr(label);
    let resp = resp.hint(label);
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
        let mut app = PacketApp { session, ui: UiState::default(), requests: Vec::new() };
        app.ui.canvas.fit_pending = true;
        app
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
        self.ui.status = Some((crate::i18n::tr_any(&e.to_string()), true));
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
        sv.http.files.push(netlabcraft_model::services::WebFile {
            name: name.clone(),
            content: text.chars().take(netlabcraft_model::services::MAX_FILE).collect(),
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
        self.clear_picks();
        self.ui.inspect = None;
        self.ui.complex = None;
        self.ui.events = events::EventView::default();
        self.ui.pan = egui::Vec2::ZERO;
        self.ui.zoom = 1.0;
        self.ui.canvas.fit_pending = true;
        if !self.session.topology.notes.is_empty() {
            self.info(tl("This lab has notes: open them with the notes button."));
        }
    }

    pub fn new_lab(&mut self, sample: Option<&str>) {
        let params = sample.map_or_else(|| json!({}), |s| json!({"sample": s}));
        if self.run("file.new", params).is_some() {
            self.after_replace();
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
        format!("NetLabCraft - {}", self.session.title())
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

    fn clear_picks(&mut self) {
        self.ui.picked.clear();
        self.ui.picked_clusters.clear();
        self.ui.renaming_cluster = None;
        self.ui.canvas.cancel();
    }

    /// Something to group: picked devices or clusters, or the selected device.
    pub fn can_group(&self) -> bool {
        !self.ui.picked.is_empty() || !self.ui.picked_clusters.is_empty() || self.ui.selected.is_some()
    }

    /// Groups the picked devices and clusters (or the selected device) into a new cluster.
    pub fn group_selection(&mut self) {
        let mut devices: Vec<u32> = self.ui.picked.iter().map(|d| d.0).collect();
        if let Some(d) = self.ui.selected
            && !self.ui.picked.contains(&d)
        {
            devices.push(d.0);
        }
        let clusters: Vec<u32> = self.ui.picked_clusters.iter().copied().collect();
        if devices.is_empty() && clusters.is_empty() {
            self.info(tl("Pick devices first: Shift+click them or Shift+drag a box around them."));
            return;
        }
        if let Some(v) = self.run("cluster.create", json!({"devices": devices, "clusters": clusters})) {
            let name = v.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            self.ui.selected = None;
            self.ui.picked.clear();
            self.ui.picked_clusters.clear();
            if let Some(id) = v.get("id").and_then(Value::as_u64).and_then(|i| u32::try_from(i).ok()) {
                self.ui.picked_clusters.insert(id);
            }
            self.info(crate::i18n::trf("Grouped into {}: double-click it to open it.", &[&name]));
        }
    }

    /// Dissolves the picked clusters.
    pub fn ungroup_selection(&mut self) {
        for c in std::mem::take(&mut self.ui.picked_clusters) {
            self.run("cluster.dissolve", json!({"id": c}));
        }
    }

    /// Shows a cluster's inside on the workspace.
    pub fn enter_cluster(&mut self, c: u32) {
        if self.run("view.enter_cluster", json!({"id": c})).is_some() {
            self.ui.selected = None;
            self.clear_picks();
            canvas::fit_view(self);
        }
    }

    /// Back up one cluster (`root`: to the top level).
    pub fn leave_cluster(&mut self, root: bool) {
        if self.run(if root { "view.root" } else { "view.up" }, json!({})).is_some() {
            self.ui.selected = None;
            self.clear_picks();
            canvas::fit_view(self);
        }
    }

    /// Switches the workspace between the logical and the physical view.
    pub fn set_physical(&mut self, on: bool) {
        if self.run(if on { "view.physical" } else { "view.logical" }, json!({})).is_some() {
            self.ui.selected = None;
            self.clear_picks();
            self.ui.canvas.cancel();
            if on {
                physical::fit(self);
            } else {
                canvas::fit_view(self);
            }
        }
    }

    /// Opens a city, building or wiring closet in the physical view.
    pub fn enter_place(&mut self, id: u32) {
        self.ui.selected = None;
        physical::enter(self, id);
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.ui.tool = t;
        self.ui.canvas.cancel();
        self.ui.physical.closet.wire = None;
    }

    /// Tools ▸ Drawing Palette: picks a drawing tool, which shows the Palette window (a drawing tool already
    /// picked stays). The physical view has no drawings.
    pub fn open_drawing_palette(&mut self) {
        if self.session.physical {
            return;
        }
        if !matches!(self.ui.tool, Tool::Draw(s) if s != netlabcraft_model::Shape::Text) {
            self.set_tool(Tool::Draw(netlabcraft_model::Shape::Rectangle));
        }
    }

    fn zoom(&mut self, factor: Option<f32>) {
        let old = self.ui.zoom;
        let new = match factor {
            Some(f) => (old * f).clamp(0.25, 3.0),
            None => 1.0,
        };
        // In the wiring closet the buttons zoom about the middle of the workspace, so what is in view stays in view.
        let mid = self.ui.canvas.size / 2.0;
        if crate::closet::current(self).is_some() && old > 0.0 && mid.x > 0.0 {
            self.ui.pan = mid - (mid - self.ui.pan) * (new / old);
        }
        self.ui.zoom = new;
    }

    /// Zoom Reset: the true size and the start of the workspace (in a wiring closet, the furniture's top left).
    fn reset_view(&mut self) {
        if let Some(place) = crate::closet::current(self) {
            crate::closet::reset(self, place);
        } else {
            self.zoom(None);
            self.ui.pan = egui::Vec2::ZERO;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        shortcuts::dispatch(self, ctx);
    }

    /// Asks for a screenshot of the window, to export as a PNG or (`pdf`) to make the report PDF.
    fn request_image(&mut self, ctx: &egui::Context, pdf: Option<PdfUse>) {
        self.ui.image_pending = true;
        self.ui.pdf_for = pdf;
        let slot = self.ui.shot.clone();
        let ctx2 = ctx.clone();
        ctx.request_screenshot(move |image| {
            *slot.lock() = Some(image);
            ctx2.request_repaint();
        });
    }

    /// Draws the whole app into `ui` (the root of the window).
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let tk = theme::tokens(self.ui.dark);
        self.take_screenshot(&ctx);
        // Esc drops a tile being dragged from the device box before it does anything else.
        let dropped_tile = self.ui.tile_drag.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if dropped_tile {
            self.ui.tile_drag = None;
        } else {
            self.shortcuts(&ctx);
        }
        let chrome = egui::Frame::new().fill(tk.chrome).inner_margin(egui::Margin::symmetric(6, 4));
        let bar = egui::Frame::new().fill(tk.bar).inner_margin(egui::Margin::symmetric(6, 3));
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        if self.ui.toolbars.main {
            egui::Panel::top("toolbar").frame(chrome.stroke(egui::Stroke::new(1.0, tk.chrome_line))).show(ui, |ui| self.main_toolbar(ui));
        }
        if self.ui.toolbars.secondary {
            egui::Panel::top("tools").frame(chrome.stroke(egui::Stroke::new(1.0, tk.chrome_line))).show(ui, |ui| self.tool_bar(ui));
        }
        egui::Panel::top("workspace_bar").frame(bar).show(ui, |ui| self.workspace_bar(ui));
        if self.ui.toolbars.bottom {
            egui::Panel::bottom("device_box").frame(chrome).exact_size(160.0).show(ui, |ui| bottom::show(self, ui));
        }
        egui::Panel::bottom("time_bar").frame(bar).show(ui, |ui| self.time_bar(ui));
        if self.ui.prefs.device_taskbar && !self.ui.windows.is_empty() {
            egui::Panel::bottom("device_taskbar").frame(bar).show(ui, |ui| prefs_misc::taskbar(self, ui));
        }
        if self.ui.mode == Mode::Simulation && self.ui.show_event_list {
            egui::Panel::right("simulation_panel").resizable(true).default_size(420.0).show(ui, |ui| events::panel(self, ui));
        }
        if self.session.physical {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| physical::show(self, ui));
        } else {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| canvas::show(self, ui));
        }
        physical::windows(self, &ctx);
        console::windows(self, &ctx);
        canvas::inspect_window(self, &ctx);
        canvas::note_editor(self, &ctx);
        palette::window(self, &ctx);
        canvas::cluster_rename_window(self, &ctx);
        events::pdu_window(self, &ctx);
        pdu_dialog::window(self, &ctx);
        self.notes_window(&ctx);
        activity::windows(self, &ctx);
        prefs::windows(self, &ctx);
        environments::windows(self, &ctx);
        shortcuts::windows(self, &ctx);
        print::window(self, &ctx);
        self.about_window(&ctx);
        help::window(self, &ctx);
        overview::viewport_window(self, &ctx);
        bottom::tile_ghost(self, &ctx);
        overview::list_window(self, &ctx);
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tl("File"), |ui| {
                if ui.button(tl("New")).clicked() {
                    self.new_lab(None);
                }
                ui.menu_button(tl("New from sample"), |ui| {
                    for s in sample::SAMPLES {
                        if ui.button(crate::i18n::tr(s.title)).hint(crate::i18n::tr(s.description)).clicked() {
                            self.new_lab(Some(s.id));
                        }
                    }
                });
                if ui.add(egui::Button::new(tl("Open...")).shortcut_text(shortcuts::text(shortcuts::Act::Open))).clicked() {
                    self.requests.push(UiRequest::Open);
                }
                if ui.add(egui::Button::new(tl("Open Samples")).shortcut_text(shortcuts::text(shortcuts::Act::OpenSamples))).clicked() {
                    self.ui.show_samples = true;
                    ui.close();
                }
                if !self.ui.recent.is_empty() {
                    ui.menu_button(tl("Open Recent"), |ui| {
                        for p in self.ui.recent.clone() {
                            let name = std::path::Path::new(&p).file_name().map_or(p.clone(), |n| n.to_string_lossy().to_string());
                            if ui.button(name).hint(&p).clicked() {
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
                if ui.button(tl("Export Capture (pcap)...")).hint(tl("The event list as a capture any packet analyser opens")).clicked() {
                    self.requests.push(UiRequest::ExportPcap(self.session.pcap()));
                }
                let png = ui.button(tl("Export Workspace Image (PNG)...")).hint(tl("The workspace as a picture, for a report")).clicked();
                let save_pdf =
                    ui.button(tl("Save as PDF...")).hint(tl("The workspace and a report of the devices on A4 pages, saved as a PDF file")).clicked();
                let print = ui
                    .add(egui::Button::new(tl("Print...")).shortcut_text(shortcuts::text(shortcuts::Act::Print)))
                    .hint(tl("The same PDF: save it, open it in a PDF viewer or print it"))
                    .clicked();
                let pdf = if print {
                    Some(PdfUse::Print)
                } else if save_pdf {
                    Some(PdfUse::Save)
                } else {
                    None
                };
                if png || pdf.is_some() {
                    let ctx = ui.ctx().clone();
                    self.request_image(&ctx, pdf);
                }
                ui.separator();
                if ui.button(tl("Exit")).clicked() {
                    self.requests.push(UiRequest::Quit);
                }
            });
            ui.menu_button(tl("Edit"), |ui| {
                if ui.add_enabled(self.ui.selected.is_some(), egui::Button::new(tl("Copy")).shortcut_text("Ctrl+C")).clicked() {
                    self.copy();
                }
                if ui.add_enabled(self.ui.clipboard.is_some(), egui::Button::new(tl("Paste")).shortcut_text("Ctrl+V")).clicked() {
                    self.paste();
                }
                ui.separator();
                if ui.add_enabled(self.session.can_undo(), egui::Button::new(tl("Undo")).shortcut_text("Ctrl+Z")).clicked() {
                    self.run("edit.undo", json!({}));
                }
                if ui.add_enabled(self.session.can_redo(), egui::Button::new(tl("Redo")).shortcut_text("Ctrl+Shift+Z")).clicked() {
                    self.run("edit.redo", json!({}));
                }
                ui.separator();
                if ui.add_enabled(self.ui.selected.is_some(), egui::Button::new(tl("Delete"))).clicked()
                    && let Some(d) = self.ui.selected.take()
                {
                    self.run("device.remove", json!({"device": d.0}));
                }
                ui.separator();
                if ui.add_enabled(self.can_group(), egui::Button::new(tl("Group into Cluster")).shortcut_text("Ctrl+G")).clicked() {
                    self.group_selection();
                }
                if ui.add_enabled(!self.ui.picked_clusters.is_empty(), egui::Button::new(tl("Uncluster"))).clicked() {
                    self.ungroup_selection();
                }
            });
            ui.menu_button(tl("Options"), |ui| {
                prefs::menu_items(self, ui);
                environments::menu_item(self, ui);
                if ui.checkbox(&mut self.ui.dark, tl("Dark interface")).changed() {
                    self.apply_theme(ui.ctx());
                }
                ui.checkbox(&mut self.ui.show_grid, tl("Show grid")).hint(shortcuts::text(shortcuts::Act::ShowGrid));
                // Native windows need viewports: egui embeds them (the web build, headless runs).
                let embedded = ui.ctx().embed_viewports();
                let native = ui.add_enabled(!embedded, egui::Checkbox::new(&mut self.ui.native_windows, tl("Device windows as separate windows")));
                if embedded {
                    let _ = native.disabled_hint(tl("Not available in the web version"));
                }
                ui.menu_button(tl("Language"), |ui| {
                    for l in i18n::Lang::all() {
                        if ui.radio(i18n::lang() == l, l.name()).clicked() {
                            i18n::set_lang(l);
                            self.ui.lang_changed = true;
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button(tl("View"), |ui| {
                ui.menu_button(tl("Zoom"), |ui| {
                    if ui.add(egui::Button::new(tl("Zoom In")).shortcut_text(shortcuts::text(shortcuts::Act::ZoomIn))).clicked() {
                        self.zoom(Some(1.25));
                    }
                    if ui.add(egui::Button::new(tl("Zoom Reset")).shortcut_text(shortcuts::text(shortcuts::Act::ZoomReset))).clicked() {
                        self.reset_view();
                    }
                    if ui.add(egui::Button::new(tl("Zoom Out")).shortcut_text(shortcuts::text(shortcuts::Act::ZoomOut))).clicked() {
                        self.zoom(Some(0.8));
                    }
                });
                ui.menu_button(tl("Toolbars"), |ui| {
                    if check_item(ui, self.ui.toolbars.main, tl("Main"), "Ctrl+Shift+A") {
                        self.ui.toolbars.main = !self.ui.toolbars.main;
                    }
                    if check_item(ui, self.ui.toolbars.secondary, tl("Secondary"), "Ctrl+Shift+R") {
                        self.ui.toolbars.secondary = !self.ui.toolbars.secondary;
                    }
                    if check_item(ui, self.ui.toolbars.bottom, tl("Bottom"), "Ctrl+Shift+B") {
                        self.ui.toolbars.bottom = !self.ui.toolbars.bottom;
                    }
                });
                ui.separator();
                if check_item(ui, self.ui.mode == Mode::Simulation, tl("Simulation Mode"), "Shift+S") {
                    self.ui.mode = Mode::Simulation;
                }
                if check_item(ui, self.ui.mode == Mode::Realtime, tl("Realtime Mode"), "Shift+R") {
                    self.ui.mode = Mode::Realtime;
                }
                ui.separator();
                let physical = self.session.physical;
                if check_item(ui, !physical, tl("Logical View"), "Shift+L") && physical {
                    self.set_physical(false);
                }
                if check_item(ui, physical, tl("Physical View"), "Shift+P") && !physical {
                    self.set_physical(true);
                }
                ui.separator();
                if check_item(ui, self.ui.show_viewport, tl("Show Viewport"), "Shift+V") {
                    self.ui.show_viewport = !self.ui.show_viewport;
                }
                if check_item(ui, self.ui.show_workspace_list, tl("Show Workspace List"), "Shift+W") {
                    self.ui.show_workspace_list = !self.ui.show_workspace_list;
                }
                if check_item(ui, self.ui.prefs.show_wireless_signals, tl("View Wireless Signals"), &shortcuts::text(shortcuts::Act::WirelessSignals))
                {
                    self.ui.prefs.show_wireless_signals = !self.ui.prefs.show_wireless_signals;
                }
            });
            ui.menu_button(tl("Tools"), |ui| {
                if ui.add_enabled(!self.session.physical, egui::Button::new(tl("Drawing Palette")).shortcut_text("Ctrl+D")).clicked() {
                    self.open_drawing_palette();
                    ui.close();
                }
                ui.separator();
                if ui.button(tl("Inspect")).clicked() {
                    self.set_tool(Tool::Inspect);
                    ui.close();
                }
                if ui.button(tl("Add Simple PDU")).clicked() {
                    self.set_tool(Tool::Ping);
                    ui.close();
                }
                if ui.button(tl("Add Complex PDU")).clicked() {
                    let src = self.ui.selected;
                    pdu_dialog::open(self, src);
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
                if ui.add_enabled(self.session.topology.activity.is_some(), egui::Button::new(tl("Connectivity Tests"))).clicked() {
                    self.ui.activity.tools.show_connectivity = true;
                    ui.close();
                }
                ui.separator();
                ui.label(tl("Agents and scripts drive NetLabCraft through"));
                ui.monospace("netlabcraft-cli mcp");
                ui.monospace("netlabcraft-cli run --cmd ...");
            });
            ui.menu_button(tl("Window"), |ui| {
                ui.checkbox(&mut self.ui.show_notes, tl("Lab Notes"));
                let mut sim = self.ui.mode == Mode::Simulation;
                if ui.checkbox(&mut sim, tl("Simulation Panel")).changed() {
                    self.ui.mode = if sim { Mode::Simulation } else { Mode::Realtime };
                }
            });
            ui.menu_button(tl("Help"), |ui| {
                for (page, shortcut) in [
                    (help::Page::GettingStarted, ""),
                    (help::Page::Contents, "F1"),
                    (help::Page::Tutorials, ""),
                    (help::Page::Accessibility, ""),
                    (help::Page::Shortcuts, ""),
                ] {
                    if ui.add(egui::Button::new(tl(page.title())).shortcut_text(shortcut)).clicked() {
                        self.ui.help = Some(page);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(tl("Report an Issue")).hint(tl("Opens the issue tracker in your browser")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(help::ISSUES_URL));
                    ui.close();
                }
                ui.separator();
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
                self.reset_view();
            }
            if tool_button(ui, Glyph::ZoomOut, "Zoom Out", false, true).clicked() {
                self.zoom(Some(0.8));
            }
            ui.add_space(8.0);
            if tool_button(ui, Glyph::Capture, "Export Capture (pcap)", false, true).clicked() {
                self.requests.push(UiRequest::ExportPcap(self.session.pcap()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if tool_button(ui, Glyph::Help, "About NetLabCraft", false, true).clicked() {
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
            if tool_button(ui, Glyph::Cluster, "Group into Cluster (Ctrl+G): the picked devices", false, self.can_group()).clicked() {
                self.group_selection();
            }
            ui.add_space(8.0);
            if tool_button(ui, Glyph::Note, "Place Note (N)", t == Tool::Note, true).clicked() {
                self.set_tool(Tool::Note);
            }
            for (g, label, shape) in [
                (Glyph::Line, "Draw Line", netlabcraft_model::Shape::Line),
                (Glyph::Rectangle, "Draw Rectangle", netlabcraft_model::Shape::Rectangle),
                (Glyph::Ellipse, "Draw Ellipse", netlabcraft_model::Shape::Ellipse),
            ] {
                if tool_button(ui, g, &crate::i18n::trf("{} (drag on the workspace)", &[&crate::i18n::tr(label)]), t == Tool::Draw(shape), true)
                    .clicked()
                {
                    self.set_tool(Tool::Draw(shape));
                }
            }
            if tool_button(ui, Glyph::Freeform, "Draw Freehand (drag on the workspace)", t == Tool::Draw(netlabcraft_model::Shape::Freehand), true)
                .clicked()
            {
                self.set_tool(Tool::Draw(netlabcraft_model::Shape::Freehand));
            }
            if tool_button(ui, Glyph::Polygon, "Draw Polygon (click the corners)", t == Tool::Draw(netlabcraft_model::Shape::Polygon), true).clicked()
            {
                self.set_tool(Tool::Draw(netlabcraft_model::Shape::Polygon));
            }
            ui.add_space(8.0);
            if tool_button(ui, Glyph::SimplePdu, "Add Simple PDU (P)", t == Tool::Ping, true).clicked() {
                self.set_tool(Tool::Ping);
            }
            if tool_button(ui, Glyph::ComplexPdu, "Add Complex PDU", self.ui.complex.is_some(), true).clicked() {
                let src = self.ui.selected;
                pdu_dialog::open(self, src);
            }
        });
    }

    fn workspace_bar(&mut self, ui: &mut egui::Ui) {
        let tk = theme::tokens(self.ui.dark);
        ui.horizontal(|ui| {
            let physical = self.session.physical;
            if pill(ui, Glyph::Logical, "Logical", !physical).clicked() && physical {
                self.set_physical(false);
            }
            if pill(ui, Glyph::Physical, "Physical", physical).clicked() && !physical {
                self.set_physical(true);
            }
            if physical && let Some(kind) = physical::child_kind(self) {
                let label = physical::new_label(kind);
                let b = ui.add(egui::Button::new(RichText::new(tl(label)).color(tk.bar_text).size(15.0)).fill(tk.pill.gamma_multiply(0.5)));
                if b.hint(tl("Adds a place to the map shown; drag it into position")).clicked() {
                    physical::add_place(self, None);
                }
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
                let back =
                    if physical { "Back to the enclosing place (Intercity is the top)" } else { "Back to the parent cluster (Root is the top)" };
                for (g, label) in [
                    (Glyph::Physical, "Set Background (not available yet)"),
                    (Glyph::ZoomReset, "Navigation: back to the start of the workspace"),
                    (Glyph::Undo, back),
                ] {
                    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::click());
                    let label = crate::i18n::tr(label);
                    let resp = resp.hint(label);
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, label));
                    let p = ui.painter_at(r);
                    if resp.hovered() {
                        p.rect_filled(r.shrink(1.5), 7.0, tk.pill.gamma_multiply(0.5));
                    }
                    p.rect_stroke(r, 8.0, egui::Stroke::new(1.5, tk.bar_text), egui::StrokeKind::Inside);
                    icons::glyph(&p, egui::Rect::from_center_size(r.center(), vec2(20.0, 20.0)), g, tk.pill_text);
                    if resp.clicked() && g == Glyph::ZoomReset {
                        self.reset_view();
                    }
                    if resp.clicked() && g == Glyph::Undo {
                        if physical {
                            physical::leave(self, false);
                        } else {
                            self.leave_cluster(false);
                        }
                    }
                }
                if physical {
                    let on = self.ui.prefs.show_wireless_signals;
                    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::click());
                    let label = tl("View Wireless Signals (Ctrl+Alt+Shift+W): faint coverage areas of the access points and a line from each Wi-Fi host to its access point");
                    let resp = resp.hint(label);
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, on, label));
                    let p = ui.painter_at(r);
                    if on {
                        p.rect_filled(r.shrink(1.5), 7.0, tk.pill);
                    } else if resp.hovered() {
                        p.rect_filled(r.shrink(1.5), 7.0, tk.pill.gamma_multiply(0.5));
                    }
                    p.rect_stroke(r, 8.0, egui::Stroke::new(1.5, tk.bar_text), egui::StrokeKind::Inside);
                    icons::glyph(&p, egui::Rect::from_center_size(r.center(), vec2(20.0, 20.0)), Glyph::Wireless, tk.pill_text);
                    if resp.clicked() {
                        self.ui.prefs.show_wireless_signals = !on;
                    }
                }
                if physical {
                    // Intercity › City › Building › Closet (right to left here); each opens.
                    let path: Vec<(u32, String)> =
                        self.session.topology.place_path(self.session.place_view).into_iter().map(|p| (p.id, p.name)).collect();
                    let mut go: Option<Option<u32>> = None;
                    for (i, (id, name)) in path.iter().enumerate().rev() {
                        let last = i + 1 == path.len();
                        let text = RichText::new(name).color(tk.bar_text).size(17.0);
                        let r = ui.add(egui::Label::new(if last { text.strong() } else { text }).sense(Sense::click()));
                        if r.clicked() && !last {
                            go = Some(Some(*id));
                        }
                        ui.label(RichText::new("›").color(tk.bar_text).size(17.0));
                    }
                    let top = ui.add(egui::Label::new(RichText::new(tl("Intercity")).color(tk.bar_text).size(17.0)).sense(Sense::click()));
                    if top.hint(tl("Back to the Intercity map")).clicked() && self.session.place_view.is_some() {
                        go = Some(None);
                    }
                    match go {
                        Some(Some(p)) => physical::enter(self, p),
                        Some(None) => physical::leave(self, true),
                        None => {}
                    }
                    return;
                }
                // Where the workspace is: Root › Cluster › … (right to left here); each opens.
                let t = &self.session.topology;
                let path: Vec<(u32, String)> =
                    t.cluster_path(self.session.view).iter().filter_map(|c| t.cluster(*c)).map(|c| (c.id, c.name.clone())).collect();
                let mut go: Option<Option<u32>> = None;
                for (i, (id, name)) in path.iter().enumerate().rev() {
                    let last = i + 1 == path.len();
                    let text = RichText::new(name).color(tk.bar_text).size(17.0);
                    let r = ui.add(egui::Label::new(if last { text.strong() } else { text }).sense(Sense::click()));
                    if r.clicked() && !last {
                        go = Some(Some(*id));
                    }
                    ui.label(RichText::new("›").color(tk.bar_text).size(17.0));
                }
                let root = ui.add(egui::Label::new(RichText::new(tl("Root")).color(tk.bar_text).size(17.0)).sense(Sense::click()));
                if root.hint(tl("Back to the top level")).clicked() && self.session.view.is_some() {
                    go = Some(None);
                }
                match go {
                    Some(Some(c)) => self.enter_cluster(c),
                    Some(None) => self.leave_cluster(true),
                    None => {}
                }
            });
        });
    }

    fn time_bar(&mut self, ui: &mut egui::Ui) {
        let tk = theme::tokens(self.ui.dark);
        // Everything on the bar shares one middle line: text and the round buttons.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let now = self.session.sim.now_us();
            let time = if self.ui.mode == Mode::Simulation {
                format!("{}: {}.{:03}", tl("Time"), clock(now), (now / 1000) % 1000)
            } else {
                format!("{}: {}", tl("Time"), clock(now))
            };
            ui.label(RichText::new(time).color(tk.bar_text).size(18.0));
            let white = |ui: &mut egui::Ui, g: Glyph, label: &str| {
                let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
                let label = crate::i18n::tr(label);
                let resp = resp.hint(label);
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
            // The right-hand tabs come first: the hint, then the "PLAY CONTROLS:" caption give way
            // (shrink, then go) when the bar is short, so nothing ever overlaps.
            let sim = self.ui.mode == Mode::Simulation;
            let gap = ui.spacing().item_spacing.x;
            let storm = self.session.sim.storm_detected();
            let mut right = pill_width(ui, "Simulation") + pill_width(ui, "Realtime") + 2.0 * gap;
            if sim {
                right += pill_width(ui, "Event List") + gap;
            }
            if storm {
                right += text_width(ui, tl("Switching loop detected"), egui::TextStyle::Body.resolve(ui.style())) + gap;
            }
            if sim {
                let caption = tl("PLAY CONTROLS:");
                let controls = 3.0 * (30.0 + gap) + 12.0;
                if ui.available_width() - right - controls >= text_width(ui, caption, egui::FontId::proportional(16.0)) + gap {
                    ui.label(RichText::new(caption).color(tk.bar_text).size(16.0));
                }
                events::play_controls(self, ui, 30.0);
            } else if white(ui, Glyph::FastForward, "Fast Forward Time (30 s)").clicked() {
                self.run("sim.advance", json!({"seconds": 30}));
            }
            ui.add_space(12.0);
            let (msg, color) = match &self.ui.status {
                Some((m, true)) => (m.clone(), Color32::from_rgb(0xff, 0x9a, 0x9a)),
                Some((m, false)) => (m.clone(), tk.bar_text),
                None => (canvas::hint(self), tk.bar_text.gamma_multiply(0.75)),
            };
            let room = ui.available_width() - right - 2.0 * gap;
            if room >= 60.0 && !msg.is_empty() {
                let bar_h = ui.available_height();
                ui.allocate_ui_with_layout(vec2(room, bar_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_max_width(room);
                    ui.add(egui::Label::new(RichText::new(&msg).color(color).size(13.0)).truncate()).hint(&msg);
                });
            }
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
                if storm {
                    ui.label(RichText::new(tl("Switching loop detected")).color(Color32::from_rgb(0xff, 0x9a, 0x9a)))
                        .hint(tl("A frame circled the network; there is no spanning tree yet, so avoid redundant switch links"));
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

    /// The screenshot asked for by Export Workspace Image: cropped to the workspace, as PNG.
    fn take_screenshot(&mut self, ctx: &egui::Context) {
        if !self.ui.image_pending {
            return;
        }
        let shot = self.ui.shot.lock().take();
        let Some(image) = shot else { return };
        self.ui.image_pending = false;
        let ppp = ctx.pixels_per_point();
        let [w, h] = image.size;
        let area = self.ui.canvas_rect.unwrap_or(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32 / ppp, h as f32 / ppp)));
        let px = |v: f32, max: usize| ((v * ppp).round().max(0.0) as usize).min(max);
        let (x0, y0, x1, y1) = (px(area.min.x, w), px(area.min.y, h), px(area.max.x, w), px(area.max.y, h));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
        for y in y0..y1 {
            for x in x0..x1 {
                let c = image.pixels.get(y * w + x).copied().unwrap_or_default();
                rgba.extend_from_slice(&[c.r(), c.g(), c.b(), 255]);
            }
        }
        let (w, h) = (u32::try_from(x1 - x0).unwrap_or(0), u32::try_from(y1 - y0).unwrap_or(0));
        if let Some(usage) = self.ui.pdf_for.take() {
            let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect();
            let title = self.session.title();
            let title = title.trim_end_matches(" •");
            let created = netlabcraft_engine::instructor::now_utc();
            let name = print::pdf_file_name(title);
            let bytes = pdf::document(title, &created, &self.session.topology, w, h, &rgb);
            self.pdf_ready(usage, name, bytes);
        } else {
            self.requests.push(UiRequest::ExportImage(png::encode(w, h, &rgba)));
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.ui.show_about {
            return;
        }
        let mut open = true;
        let credits = &mut self.ui.about_credits;
        egui::Window::new(tl("About NetLabCraft")).open(&mut open).collapsible(false).resizable(false).default_pos(BELOW_TOOLBARS).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(!*credits, tl("About")).clicked() {
                    *credits = false;
                }
                if ui.selectable_label(*credits, tl("Contributors")).clicked() {
                    *credits = true;
                }
            });
            ui.separator();
            if *credits {
                // Room for every column of the table view.
                ui.set_min_width(640.0);
                credits::contributors_ui(ui);
                return;
            }
            ui.heading(tl("NetLabCraft"));
            ui.label(crate::i18n::trf("Version {}", &[&env!("CARGO_PKG_VERSION")]));
            ui.label(tl("An open-source network simulator: build labs, configure routers and switches, and watch every frame."));
            ui.label(tl("Licensed under MIT or Apache-2.0."));
            ui.separator();
            ui.label(RichText::new(tl("Independent clean-room project; not affiliated with any network equipment vendor.")).small());
        });
        self.ui.show_about = open;
    }
}
