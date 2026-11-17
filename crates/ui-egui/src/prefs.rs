//! Options ▸ Preferences, Algorithm Settings, User Profile and View Command Log.
//!
//! The settings in [`Prefs`] are view and behaviour choices of this program, not part of a lab:
//! the desktop shell keeps them between sessions as text ([`PacketApp::preferences_text`] and
//! [`PacketApp::load_preferences`]); the web build starts from the defaults every time. The ones
//! that are state of the lab or of the session go through engine commands instead (cable length
//! effects: `physical.length_effects`; logging: `log.enable`; the display name: `profile.set`).
//!
//! Defaults keep what NetLabCraft did before these settings existed (aligned logical workspace,
//! cable lengths counted, devices opening on their command line), so nothing changes until a
//! person unchecks a box.

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use serde_json::json;

use crate::i18n::{self, tl, trn};
use crate::sound::Sound;
use crate::{PacketApp, Tool, UiRequest};

/// Most seconds a tooltip may wait.
const MAX_TOOLTIP_SECS: f32 = 10.0;
/// The wait egui uses when the setting is 0.
const DEFAULT_TOOLTIP_SECS: f32 = 0.5;
/// Smallest and largest interface text size, in percent.
const FONT_RANGE: std::ops::RangeInclusive<u16> = 70..=160;

/// The Interface, Font and Show/Hide choices.
#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    /// The packet being replayed moves along its cable (otherwise it sits in the middle).
    pub show_animation: bool,
    /// A short tone when a PDU arrives or fails.
    pub play_sound: bool,
    pub show_model_labels: bool,
    pub show_name_labels: bool,
    /// Port names are written by every cable (otherwise only on mouse over, if that is on).
    pub always_port_labels: bool,
    /// The Automatic cable is left out of the device box.
    pub disable_auto_cable: bool,
    /// Lengths in the physical workspace in metres (otherwise feet).
    pub metric: bool,
    /// Devices snap to the grid in the logical workspace.
    pub align_logical: bool,
    /// Seconds before a tooltip shows (0: the default).
    pub tooltip_secs: f32,
    pub show_link_lights: bool,
    /// Tones for IP phone calls and the dial-up modem.
    pub play_telephony: bool,
    pub show_qos_stamps: bool,
    pub port_labels_on_hover: bool,
    /// Devices with a command line open on its tab (otherwise on Config).
    pub cli_default_tab: bool,
    pub cable_popup: bool,
    /// Places and devices snap to the grid in the physical workspace.
    pub align_physical: bool,
    /// Interface text size in percent.
    pub font_percent: u16,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            show_animation: true,
            play_sound: false,
            show_model_labels: true,
            show_name_labels: true,
            always_port_labels: true,
            disable_auto_cable: false,
            metric: true,
            align_logical: true,
            tooltip_secs: 0.0,
            show_link_lights: true,
            play_telephony: true,
            show_qos_stamps: true,
            port_labels_on_hover: true,
            cli_default_tab: true,
            cable_popup: true,
            align_physical: false,
            font_percent: 100,
        }
    }
}

impl Prefs {
    /// Whether the model and the name are written under each device.
    pub fn labels(&self) -> (bool, bool) {
        (self.show_model_labels, self.show_name_labels)
    }

    /// The yes-or-no settings with the name each has in the saved text.
    fn flags(&mut self) -> [(&'static str, &mut bool); 15] {
        [
            ("animation", &mut self.show_animation),
            ("sound", &mut self.play_sound),
            ("model-labels", &mut self.show_model_labels),
            ("name-labels", &mut self.show_name_labels),
            ("port-labels-always", &mut self.always_port_labels),
            ("no-auto-cable", &mut self.disable_auto_cable),
            ("metric", &mut self.metric),
            ("align-logical", &mut self.align_logical),
            ("link-lights", &mut self.show_link_lights),
            ("telephony-sound", &mut self.play_telephony),
            ("qos-stamps", &mut self.show_qos_stamps),
            ("port-labels-hover", &mut self.port_labels_on_hover),
            ("cli-tab", &mut self.cli_default_tab),
            ("cable-popup", &mut self.cable_popup),
            ("align-physical", &mut self.align_physical),
        ]
    }

    /// The settings as `key=on|off` lines (and the two numbers).
    pub fn to_text(&self) -> String {
        let mut copy = self.clone();
        let mut out = String::new();
        for (key, value) in copy.flags() {
            out.push_str(&format!("{key}={}\n", if *value { "on" } else { "off" }));
        }
        out.push_str(&format!("tooltip={}\nfont={}\n", self.tooltip_secs, self.font_percent));
        out
    }

    /// Reads one saved line. `false` when the key is not one of these; a value that does not read
    /// leaves the setting as it was.
    pub fn apply(&mut self, key: &str, value: &str) -> bool {
        match key {
            "tooltip" => {
                if let Some(v) = value.parse::<f32>().ok().filter(|v| v.is_finite()) {
                    self.tooltip_secs = (v.clamp(0.0, MAX_TOOLTIP_SECS) * 10.0).round() / 10.0;
                }
                return true;
            }
            "font" => {
                if let Some(v) = value.parse::<u16>().ok().filter(|v| FONT_RANGE.contains(v)) {
                    self.font_percent = v;
                }
                return true;
            }
            _ => {}
        }
        for (name, flag) in self.flags() {
            if name == key {
                if matches!(value, "on" | "off") {
                    *flag = value == "on";
                }
                return true;
            }
        }
        false
    }
}

/// The tabs of the Preferences window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Interface,
    Administrative,
    ShowHide,
    Font,
    Miscellaneous,
    AnswerTree,
    CustomInterfaces,
    Publishers,
    ImageCleanup,
}

impl Tab {
    const ALL: [Tab; 9] = [
        Tab::Interface,
        Tab::Administrative,
        Tab::ShowHide,
        Tab::Font,
        Tab::Miscellaneous,
        Tab::AnswerTree,
        Tab::CustomInterfaces,
        Tab::Publishers,
        Tab::ImageCleanup,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Interface => "Interface",
            Tab::Administrative => "Administrative",
            Tab::ShowHide => "Show/Hide",
            Tab::Font => "Font",
            Tab::Miscellaneous => "Miscellaneous",
            Tab::AnswerTree => "Answer Tree",
            Tab::CustomInterfaces => "Custom Interfaces",
            Tab::Publishers => "Publishers",
            Tab::ImageCleanup => "Image Cleanup",
        }
    }
}

/// Which of these windows are open, and what they are showing.
#[derive(Debug, Clone, Default)]
pub struct WindowState {
    pub show_prefs: bool,
    pub tab: Tab,
    pub show_profile: bool,
    pub show_log: bool,
    pub show_algorithm: bool,
    pub(crate) name_draft: String,
    /// The style the interface was last set to (tooltip wait in tenths of a second, text size).
    applied: Option<(u32, u16)>,
}

impl PacketApp {
    /// What the desktop shell keeps between sessions: the settings, the theme, the grid, whether
    /// commands are logged and the profile's display name, as `key=value` lines.
    pub fn preferences_text(&self) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        let mut out = self.ui.prefs.to_text();
        out.push_str(&format!(
            "dark={}\ngrid={}\nlogging={}\nname={}\n",
            on(self.ui.dark),
            on(self.ui.show_grid),
            on(self.session.cmd_log.enabled),
            self.session.profile.display_name
        ));
        out
    }

    /// Applies what [`PacketApp::preferences_text`] wrote. Hostile or damaged text changes less,
    /// never fails.
    pub fn load_preferences(&mut self, text: &str) {
        for line in text.lines().take(64) {
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "dark" => self.ui.dark = value == "on",
                "grid" => self.ui.show_grid = value == "on",
                "logging" => self.session.cmd_log.enabled = value != "off",
                "name" => self.session.profile.display_name = netlabcraft_engine::settings::clean_name(value),
                _ => {
                    self.ui.prefs.apply(key, value);
                }
            }
        }
    }

    /// Plays a tone when its preference is on: the shell does the playing.
    pub fn play(&mut self, sound: Sound) {
        let wanted = if sound.is_telephony() { self.ui.prefs.play_telephony } else { self.ui.prefs.play_sound };
        if wanted {
            self.requests.push(UiRequest::PlaySound(crate::sound::wav(sound)));
        }
    }

    /// Asks the shell to save the command log as a text file.
    pub fn export_log(&mut self) {
        if let Some(v) = self.run("log.export", json!({})) {
            let text = v.get("text").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
            self.requests.push(UiRequest::SaveText { name: netlabcraft_engine::settings::LOG_FILE_NAME.to_string(), text });
        }
    }

    pub fn open_preferences(&mut self) {
        self.ui.prefs_ui.show_prefs = true;
    }

    pub fn open_user_profile(&mut self) {
        self.ui.prefs_ui.name_draft = self.session.profile.display_name.clone();
        self.ui.prefs_ui.show_profile = true;
    }

    pub fn open_command_log(&mut self) {
        self.ui.prefs_ui.show_log = true;
    }

    pub fn open_algorithm_settings(&mut self) {
        self.ui.prefs_ui.show_algorithm = true;
    }
}

/// Preferences (Ctrl+R), User Profile (Ctrl+Shift+U), View Command Log (Ctrl+Shift+V) and
/// Algorithm Settings (Ctrl+Shift+M).
pub fn shortcuts(app: &mut PacketApp, ctx: &egui::Context) {
    let cmd = Modifiers::COMMAND;
    let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
    if pressed(cmd | Modifiers::SHIFT, Key::V) {
        app.open_command_log();
    }
    if pressed(cmd | Modifiers::SHIFT, Key::U) {
        app.open_user_profile();
    }
    if pressed(cmd | Modifiers::SHIFT, Key::M) {
        app.open_algorithm_settings();
    }
    if pressed(cmd, Key::R) {
        app.open_preferences();
    }
}

/// The four items at the top of the Options menu.
pub fn menu_items(app: &mut PacketApp, ui: &mut egui::Ui) {
    if ui.add(egui::Button::new(tl("Preferences…")).shortcut_text("Ctrl+R")).clicked() {
        app.open_preferences();
        ui.close();
    }
    if ui.add(egui::Button::new(tl("Algorithm Settings")).shortcut_text("Ctrl+Shift+M")).clicked() {
        app.open_algorithm_settings();
        ui.close();
    }
    if ui.add(egui::Button::new(tl("User Profile…")).shortcut_text("Ctrl+Shift+U")).clicked() {
        app.open_user_profile();
        ui.close();
    }
    if ui.add(egui::Button::new(tl("View Command Log")).shortcut_text("Ctrl+Shift+V")).clicked() {
        app.open_command_log();
        ui.close();
    }
    ui.separator();
}

/// Sets the tooltip wait and the text size of the whole interface when they change.
fn apply_style(app: &mut PacketApp, ctx: &egui::Context) {
    let tooltip = (app.ui.prefs.tooltip_secs * 10.0).round() as u32;
    let want = (tooltip, app.ui.prefs.font_percent);
    if app.ui.prefs_ui.applied == Some(want) {
        return;
    }
    app.ui.prefs_ui.applied = Some(want);
    let wait = if tooltip == 0 { DEFAULT_TOOLTIP_SECS } else { tooltip as f32 / 10.0 };
    let scale = f32::from(want.1) / 100.0;
    ctx.global_style_mut(|style| {
        style.interaction.tooltip_delay = wait;
        for (kind, base) in &egui::Style::default().text_styles {
            if let Some(font) = style.text_styles.get_mut(kind) {
                font.size = base.size * scale;
            }
        }
    });
}

/// The windows of this module, and the style the settings ask for.
pub fn windows(app: &mut PacketApp, ctx: &egui::Context) {
    apply_style(app, ctx);
    preferences_window(app, ctx);
    profile_window(app, ctx);
    log_window(app, ctx);
    algorithm_window(app, ctx);
}

/// A checkbox that says what it does when the pointer rests on it.
fn pref(ui: &mut egui::Ui, value: &mut bool, label: &'static str, hint: Option<&'static str>) -> egui::Response {
    let r = ui.checkbox(value, tl(label));
    match hint {
        Some(h) => r.on_hover_text(tl(h)),
        None => r,
    }
}

fn preferences_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.prefs_ui.show_prefs {
        return;
    }
    let mut open = true;
    egui::Window::new(tl("Preferences")).open(&mut open).default_size([700.0, 480.0]).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            for t in Tab::ALL {
                if ui.selectable_label(app.ui.prefs_ui.tab == t, tl(t.label())).clicked() {
                    app.ui.prefs_ui.tab = t;
                }
            }
        });
        ui.separator();
        match app.ui.prefs_ui.tab {
            Tab::Interface => interface_tab(app, ui),
            Tab::ShowHide => show_hide_tab(app, ui),
            Tab::Font => font_tab(app, ui),
            _ => {
                ui.label(tl("Not available yet: NetLabCraft has nothing to set in this tab so far."));
            }
        }
        ui.add_space(8.0);
        if ui.button(tl("Restore Defaults")).clicked() {
            app.ui.prefs = Prefs::default();
            app.ui.dark = false;
            app.ui.show_grid = false;
            app.apply_theme(ui.ctx());
        }
    });
    app.ui.prefs_ui.show_prefs = open;
}

fn interface_tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Customize User Experience")).strong());
    let mut effects = !app.session.topology.physical.ignore_lengths;
    let (mut effects_changed, mut auto_off) = (false, false);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let p = &mut app.ui.prefs;
        ui.columns(2, |cols| {
            if let [left, right] = cols {
                pref(left, &mut p.show_animation, "Show Animation", None);
                pref(left, &mut p.play_sound, "Play Sound", Some("A short tone when a PDU arrives or fails."));
                pref(left, &mut p.show_model_labels, "Show Device Model Labels", None);
                pref(left, &mut p.show_name_labels, "Show Device Name Labels", None);
                pref(left, &mut p.always_port_labels, "Always Show Port Labels in Logical Workspace", None);
                auto_off = pref(
                    left,
                    &mut p.disable_auto_cable,
                    "Disable Auto Cable",
                    Some("Leaves the Automatic cable out of the device box: pick the cable type for each connection."),
                )
                .changed();
                pref(
                    left,
                    &mut p.metric,
                    "Use Metric System (uncheck to use imperial)",
                    Some("Cable lengths in the physical workspace show in metres and kilometres; unchecked, in feet and miles."),
                );
                pref(
                    left,
                    &mut p.align_logical,
                    "Align logical workspace objects",
                    Some("Devices snap to the grid when they are placed or dropped."),
                );
                left.horizontal(|ui| {
                    let l = ui.label(tl("Tooltip Duration:"));
                    ui.add(egui::DragValue::new(&mut p.tooltip_secs).range(0.0..=MAX_TOOLTIP_SECS).speed(0.1).fixed_decimals(1).suffix(" s"))
                        .labelled_by(l.id)
                        .on_hover_text(tl("Seconds the pointer rests on something before its tooltip shows. 0 keeps the default (0.5 s)."));
                });

                pref(right, &mut p.show_link_lights, "Show Link Lights", None);
                pref(right, &mut p.play_telephony, "Play Telephony Sound", Some("Tones for IP phone calls and the Dial-up modem."));
                pref(
                    right,
                    &mut p.show_qos_stamps,
                    "Show QoS Stamps on Packets",
                    Some("The replayed packet carries its DSCP and 802.1p priority when they are not 0."),
                );
                pref(
                    right,
                    &mut p.port_labels_on_hover,
                    "Show Port Labels When Mouse Over in Logical Workspace",
                    Some("Used when port labels are not always shown: the cables of the device under the pointer show theirs."),
                );
                effects_changed = pref(
                    right,
                    &mut effects,
                    "Enable Cable Length Effects",
                    Some("A cable longer than its medium reaches (copper 100 m) carries nothing. Saved with the lab."),
                )
                .changed();
                pref(
                    right,
                    &mut p.cli_default_tab,
                    "Use CLI as Device Default Tab",
                    Some("Routers, switches and other devices with a command line open on it; unchecked, they open on Config."),
                );
                pref(right, &mut p.cable_popup, "Show Cable Info Popup in Physical Workspace", None);
                pref(
                    right,
                    &mut p.align_physical,
                    "Align physical workspace objects",
                    Some("Places and devices snap to the grid when they are dragged."),
                );
            }
        });
    });
    if effects_changed {
        app.run("physical.length_effects", json!({"on": effects}));
    }
    if auto_off && app.ui.prefs.disable_auto_cable && app.ui.tool == Tool::Cable(netlabcraft_model::cable::Cable::Auto) {
        app.set_tool(Tool::Select);
    }

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Logging")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let mut logging = app.session.cmd_log.enabled;
        if ui
            .checkbox(&mut logging, tl("Enable Logging"))
            .on_hover_text(tl("Records every command in the command log that View Command Log shows."))
            .changed()
        {
            app.run("log.enable", json!({"on": logging}));
        }
        if ui.button(tl("Export Log…")).clicked() {
            app.export_log();
        }
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Application")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        if ui.checkbox(&mut app.ui.dark, tl("Dark interface")).changed() {
            app.apply_theme(ui.ctx());
        }
        ui.checkbox(&mut app.ui.show_grid, tl("Show grid"));
        let embedded = ui.ctx().embed_viewports();
        let native = ui.add_enabled(!embedded, egui::Checkbox::new(&mut app.ui.native_windows, tl("Device windows as separate windows")));
        if embedded {
            let _ = native.on_disabled_hover_text(tl("Not available in the web version"));
        }
        egui::ComboBox::from_label(tl("Language")).selected_text(i18n::lang().name()).show_ui(ui, |ui| {
            for l in i18n::Lang::all() {
                if ui.selectable_label(i18n::lang() == l, l.name()).clicked() {
                    i18n::set_lang(l);
                    app.ui.lang_changed = true;
                }
            }
        });
    });
}

fn show_hide_tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Toolbars")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let tb = &mut app.ui.toolbars;
        pref(ui, &mut tb.main, "Main", None);
        pref(ui, &mut tb.secondary, "Secondary", None);
        pref(ui, &mut tb.bottom, "Bottom", Some("The device box with the models and the PDU list."));
    });
}

fn font_tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Interface text")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            let l = ui.label(tl("Text size:"));
            let mut pct = app.ui.prefs.font_percent;
            if ui.add(egui::Slider::new(&mut pct, FONT_RANGE).suffix(" %")).labelled_by(l.id).changed() {
                app.ui.prefs.font_percent = pct;
            }
            if ui.button(tl("Reset")).clicked() {
                app.ui.prefs.font_percent = 100;
            }
        });
        ui.label(RichText::new(tl("Sets the text of menus, buttons and forms. Names drawn on the workspace follow its zoom instead.")).small());
    });
}

fn profile_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.prefs_ui.show_profile {
        return;
    }
    let mut open = true;
    let mut save = false;
    egui::Window::new(tl("User Profile")).open(&mut open).collapsible(false).resizable(false).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        ui.label(tl("Kept on this computer only. It names you in activity results when an activity was not started with a name."));
        ui.horizontal(|ui| {
            let l = ui.label(tl("Display name:"));
            let r = ui.add(egui::TextEdit::singleline(&mut app.ui.prefs_ui.name_draft).desired_width(220.0)).labelled_by(l.id);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                save = true;
            }
        });
        ui.label(RichText::new(tl("Use a nickname if you like: no real name or e-mail address is needed.")).small());
        if ui.button(tl("Save")).clicked() {
            save = true;
        }
    });
    if save {
        let name = app.ui.prefs_ui.name_draft.clone();
        if app.run("profile.set", json!({"name": name})).is_some() {
            app.ui.prefs_ui.name_draft = app.session.profile.display_name.clone();
            app.info(tl("Profile saved."));
        }
    }
    app.ui.prefs_ui.show_profile = open;
}

fn log_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.prefs_ui.show_log {
        return;
    }
    let mut open = true;
    egui::Window::new(tl("Command Log")).open(&mut open).default_size([640.0, 360.0]).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        let total = app.session.cmd_log.len();
        ui.horizontal(|ui| {
            ui.label(trn("{} command", "{} commands", total as u64));
            if ui.button(tl("Copy")).clicked() {
                ui.ctx().copy_text(app.session.cmd_log.text());
            }
            if ui.button(tl("Export Log…")).clicked() {
                app.export_log();
            }
            if ui.button(tl("Clear")).clicked() {
                app.run("log.clear", json!({}));
            }
            let mut logging = app.session.cmd_log.enabled;
            if ui.checkbox(&mut logging, tl("Enable Logging")).changed() {
                app.run("log.enable", json!({"on": logging}));
            }
        });
        ui.separator();
        if total == 0 {
            ui.label(tl("No commands have been recorded yet."));
            return;
        }
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both().id_salt("command_log").auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, row, total, |ui, range| {
            for e in app.session.cmd_log.entries().skip(range.start).take(range.len()) {
                let line = netlabcraft_engine::settings::CommandLog::line(e);
                let text = RichText::new(line).monospace();
                ui.add(egui::Label::new(if e.ok { text } else { text.color(theme_error(ui)) }).wrap_mode(egui::TextWrapMode::Extend));
            }
        });
    });
    app.ui.prefs_ui.show_log = open;
}

fn theme_error(ui: &egui::Ui) -> egui::Color32 {
    crate::theme::tokens(ui.visuals().dark_mode).error
}

fn algorithm_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.prefs_ui.show_algorithm {
        return;
    }
    let mut open = true;
    egui::Window::new(tl("Algorithm Settings")).open(&mut open).collapsible(false).resizable(false).default_pos(crate::BELOW_TOOLBARS).show(
        ctx,
        |ui| {
            ui.label(tl("Not available yet: NetLabCraft has no simulator-wide algorithm settings."));
            ui.label(tl("The timing options it has are Constant Delay in the Simulation Panel and the timers each device's console sets."));
        },
    );
    app.ui.prefs_ui.show_algorithm = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_as_text() {
        let mut p = Prefs { play_sound: true, align_logical: false, tooltip_secs: 1.5, font_percent: 120, ..Prefs::default() };
        let text = p.to_text();
        let mut q = Prefs::default();
        for line in text.lines() {
            let (k, v) = line.split_once('=').unwrap();
            assert!(q.apply(k, v), "{k}");
        }
        assert_eq!(p, q);
        p.metric = false;
        assert_ne!(p, q);
    }

    #[test]
    fn damaged_text_leaves_the_defaults() {
        let mut p = Prefs::default();
        assert!(!p.apply("nothing", "on"));
        assert!(p.apply("sound", "maybe"));
        assert!(p.apply("tooltip", "NaN"));
        assert!(p.apply("font", "5000"));
        assert!(p.apply("tooltip", "99"));
        assert_eq!(p, Prefs { tooltip_secs: MAX_TOOLTIP_SECS, ..Prefs::default() });
    }
}
