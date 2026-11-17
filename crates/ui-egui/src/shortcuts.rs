//! Every keyboard shortcut, in one table.
//!
//! A row is a group (for the Help page), the English label, the modifiers and key, and the action
//! it runs. [`dispatch`] reads the table once per frame; the menus, the toolbar tooltips and the
//! Help page show the same rows through [`text`] and [`page`], so a shortcut is added in one
//! place: a line in [`TABLE`] and, for a new action, a branch in [`perform`].
//!
//! Rules the table keeps (and `tests` checks):
//! - A chord with more modifiers is read before one with fewer (egui lets a shortcut without Shift
//!   match when Shift is also down, so `Ctrl+Z` would swallow `Ctrl+Shift+Z`): [`ordered`] sorts by
//!   modifier count, stable, so the table can be written in reading order.
//! - A row that is not [`Always`](When::Always) is skipped while a text field or the console has
//!   the keyboard, so typing a letter never picks a tool.
//! - No two rows share a chord.

use std::sync::OnceLock;

use egui::{Context, Key, KeyboardShortcut, Modifiers};
use netlabcraft_model::Shape;
use serde_json::json;

use crate::bottom::{Category, Sub};
use crate::i18n::{tl, trf};
use crate::{Mode, PacketApp, Tool, UiRequest};

/// Control (Command on a Mac).
pub const C: u8 = 1;
/// Alt.
pub const A: u8 = 2;
/// Shift.
pub const S: u8 = 4;

/// When a row is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Even while a text field has the keyboard (the file chords, Help, Print).
    Always,
    /// Only when no text field has it (bare keys, Shift chords, edits).
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    New,
    Open,
    OpenSamples,
    Save,
    SaveAs,
    ResetActivity,
    Print,
    Copy,
    Paste,
    Undo,
    Redo,
    Group,
    Preferences,
    NetworkInfo,
    UserProfile,
    AlgorithmSettings,
    CommandLog,
    ZoomIn,
    ZoomReset,
    ZoomOut,
    ShowGrid,
    Simulation,
    Realtime,
    Logical,
    Physical,
    Viewport,
    WorkspaceList,
    ToolbarMain,
    ToolbarSecondary,
    ToolbarBottom,
    GoBack,
    NewCity,
    NewBuilding,
    NewCloset,
    ActivityWizard,
    DrawingPalette,
    Help,
    Escape,
    Note,
    DeleteKey,
    Inspect,
    SimplePdu,
    ComplexPdu,
    Line,
    Rectangle,
    Ellipse,
    Freeform,
    PowerCycle,
    FastForward,
    SimBack,
    SimPlay,
    SimForward,
    EventList,
    Routers,
    Switches,
    Hubs,
    Wan,
    EndDevices,
    Connections,
    OpenDevice,
    MoveSelected(Dir),
    SelectToward(Dir),
    NewScenario,
    DeleteScenario,
    PduList,
}

/// One row.
#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
    pub group: &'static str,
    pub label: &'static str,
    pub mods: u8,
    pub key: Key,
    pub act: Act,
    pub when: When,
}

const fn row(group: &'static str, label: &'static str, mods: u8, key: Key, act: Act, when: When) -> Shortcut {
    Shortcut { group, label, mods, key, act, when }
}

use When::{Always, Idle};

/// Every shortcut. Written in reading order (by group); [`ordered`] sorts what is read first.
///
/// Not here, because we have nothing for them: Ctrl+Shift+H (dock the Instruction window),
/// Ctrl+Shift+I (scenario description), Ctrl+Shift+P (Port Monitor), Shift+E (Environment
/// window), Shift+F (generic container), Shift+H (working closet), Shift+M (Move Object),
/// Shift+N (Navigation panel), Alt+R (Resize), Ctrl+Space and Space (rubber-band and select by
/// key), Alt+F/E/O/V/T/X/H (open a menu), Y and N for confirmations, and the device boxes for
/// wireless, security, home, smart city, industrial, power grid, boards, actuators, sensors and
/// miscellaneous (see docs/gaps.md). Ctrl+Insert and Shift+Insert copy and paste in the console
/// and in every text field already (the toolkit does it).
pub static TABLE: &[Shortcut] = &[
    // File
    row("File", "New", C, Key::N, Act::New, Always),
    row("File", "Open...", C, Key::O, Act::Open, Always),
    row("File", "Open Samples", C | S, Key::T, Act::OpenSamples, Idle),
    row("File", "Save", C, Key::S, Act::Save, Always),
    row("File", "Save As...", C | S, Key::S, Act::SaveAs, Always),
    row("File", "Reset Activity", A, Key::N, Act::ResetActivity, Idle),
    row("File", "Print...", C, Key::P, Act::Print, Always),
    // Edit
    row("Edit", "Copy", C, Key::C, Act::Copy, Idle),
    row("Edit", "Paste", C, Key::V, Act::Paste, Idle),
    row("Edit", "Undo", C, Key::Z, Act::Undo, Idle),
    row("Edit", "Redo", C | S, Key::Z, Act::Redo, Idle),
    row("Edit", "Redo", C, Key::Y, Act::Redo, Idle),
    row("Edit", "Group into Cluster", C, Key::G, Act::Group, Idle),
    row("Edit", "New Cluster", S, Key::U, Act::Group, Idle),
    row("Edit", "Delete", 0, Key::Delete, Act::DeleteKey, Idle),
    // Options
    row("Options", "Preferences…", C, Key::R, Act::Preferences, Idle),
    row("Options", "Network Information", C | S, Key::F, Act::NetworkInfo, Idle),
    row("Options", "User Profile…", C | S, Key::U, Act::UserProfile, Always),
    row("Options", "Algorithm Settings", C | S, Key::M, Act::AlgorithmSettings, Always),
    row("Options", "View Command Log", C | S, Key::V, Act::CommandLog, Always),
    row("Options", "Show grid", S, Key::G, Act::ShowGrid, Idle),
    // View
    row("View", "Zoom In", C, Key::I, Act::ZoomIn, Idle),
    row("View", "Zoom Reset", C, Key::T, Act::ZoomReset, Idle),
    row("View", "Zoom Out", C, Key::U, Act::ZoomOut, Idle),
    row("View", "Simulation Mode", S, Key::S, Act::Simulation, Idle),
    row("View", "Realtime Mode", S, Key::R, Act::Realtime, Idle),
    row("View", "Logical View", S, Key::L, Act::Logical, Idle),
    row("View", "Physical View", S, Key::P, Act::Physical, Idle),
    row("View", "Show Viewport", S, Key::V, Act::Viewport, Idle),
    row("View", "Show Workspace List", S, Key::W, Act::WorkspaceList, Idle),
    row("View", "Main Toolbar", C | S, Key::A, Act::ToolbarMain, Idle),
    row("View", "Secondary Toolbar", C | S, Key::R, Act::ToolbarSecondary, Idle),
    row("View", "Bottom Toolbar", C | S, Key::B, Act::ToolbarBottom, Idle),
    row("View", "Go Back One Level", A, Key::ArrowLeft, Act::GoBack, Idle),
    // Physical view
    row("Physical", "New City", S, Key::C, Act::NewCity, Idle),
    row("Physical", "New Building", S, Key::B, Act::NewBuilding, Idle),
    row("Physical", "New Wiring Closet", S, Key::J, Act::NewCloset, Idle),
    // Tools
    row("Tools", "Activity Wizard…", C, Key::W, Act::ActivityWizard, Idle),
    row("Tools", "Drawing Palette", C, Key::D, Act::DrawingPalette, Idle),
    row("Tools", "Select", 0, Key::Escape, Act::Escape, Idle),
    row("Tools", "Place Note", 0, Key::N, Act::Note, Idle),
    row("Tools", "Inspect", 0, Key::I, Act::Inspect, Idle),
    row("Tools", "Add Simple PDU", 0, Key::P, Act::SimplePdu, Idle),
    row("Tools", "Add Complex PDU", 0, Key::C, Act::ComplexPdu, Idle),
    row("Tools", "Draw Line", 0, Key::L, Act::Line, Idle),
    row("Tools", "Draw Rectangle", 0, Key::R, Act::Rectangle, Idle),
    row("Tools", "Draw Ellipse", 0, Key::E, Act::Ellipse, Idle),
    row("Tools", "Draw Freehand", 0, Key::F, Act::Freeform, Idle),
    // Time and simulation
    row("Simulation", "Power Cycle Devices", A, Key::S, Act::PowerCycle, Idle),
    row("Simulation", "Fast Forward Time", A, Key::D, Act::FastForward, Idle),
    row("Simulation", "Simulation Back", A, Key::B, Act::SimBack, Idle),
    row("Simulation", "Auto Capture / Play", A, Key::P, Act::SimPlay, Idle),
    row("Simulation", "Capture / Forward", A, Key::C, Act::SimForward, Idle),
    row("Simulation", "Show Event List", A, Key::I, Act::EventList, Idle),
    // Scenarios
    row("Scenarios", "New Scenario", C | S, Key::N, Act::NewScenario, Idle),
    row("Scenarios", "Delete Scenario", C | S, Key::E, Act::DeleteScenario, Idle),
    row("Scenarios", "Toggle PDU List Window", C | S, Key::O, Act::PduList, Idle),
    row("Scenarios", "Toggle PDU List Window", C, Key::Equals, Act::PduList, Idle),
    // The device box
    row("Device box", "Routers", C | A, Key::R, Act::Routers, Always),
    row("Device box", "Switches", C | A, Key::S, Act::Switches, Always),
    row("Device box", "Hubs", C | A, Key::U, Act::Hubs, Always),
    row("Device box", "WAN Emulation", C | A, Key::N, Act::Wan, Always),
    row("Device box", "End Devices", C | A, Key::V, Act::EndDevices, Always),
    row("Device box", "Connections", C | A, Key::O, Act::Connections, Always),
    // Selection
    row("Selection", "Open Device", 0, Key::Enter, Act::OpenDevice, Idle),
    row("Selection", "Move Device Up", C, Key::ArrowUp, Act::MoveSelected(Dir::Up), Idle),
    row("Selection", "Move Device Down", C, Key::ArrowDown, Act::MoveSelected(Dir::Down), Idle),
    row("Selection", "Move Device Left", C, Key::ArrowLeft, Act::MoveSelected(Dir::Left), Idle),
    row("Selection", "Move Device Right", C, Key::ArrowRight, Act::MoveSelected(Dir::Right), Idle),
    row("Selection", "Select Next Device Up", S, Key::ArrowUp, Act::SelectToward(Dir::Up), Idle),
    row("Selection", "Select Next Device Down", S, Key::ArrowDown, Act::SelectToward(Dir::Down), Idle),
    row("Selection", "Select Next Device Left", S, Key::ArrowLeft, Act::SelectToward(Dir::Left), Idle),
    row("Selection", "Select Next Device Right", S, Key::ArrowRight, Act::SelectToward(Dir::Right), Idle),
    // Help
    row("Help", "Contents", 0, Key::F1, Act::Help, Always),
];

/// The groups, in the order the Help page lists them.
pub const GROUPS: [&str; 11] = ["File", "Edit", "Options", "View", "Physical", "Tools", "Simulation", "Scenarios", "Device box", "Selection", "Help"];

fn modifiers(m: u8) -> Modifiers {
    let mut r = Modifiers::NONE;
    if m & C != 0 {
        r |= Modifiers::COMMAND;
    }
    if m & A != 0 {
        r |= Modifiers::ALT;
    }
    if m & S != 0 {
        r |= Modifiers::SHIFT;
    }
    r
}

impl Shortcut {
    pub fn shortcut(&self) -> KeyboardShortcut {
        KeyboardShortcut::new(modifiers(self.mods), self.key)
    }

    /// `Ctrl+Alt+O`.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (bit, name) in [(C, "Ctrl+"), (A, "Alt+"), (S, "Shift+")] {
            if self.mods & bit != 0 {
                s.push_str(name);
            }
        }
        s.push_str(match self.key {
            Key::Escape => "Esc",
            Key::ArrowLeft => "Left",
            Key::ArrowRight => "Right",
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down",
            Key::Equals => "=",
            k => k.name(),
        });
        s
    }

    fn modifier_count(&self) -> u32 {
        self.mods.count_ones()
    }
}

/// The shortcut text of an action (the first row for it), for menus and tooltips; empty when it has none.
pub fn text(act: Act) -> String {
    TABLE.iter().find(|r| r.act == act).map(Shortcut::text).unwrap_or_default()
}

/// The rows in the order they are read: most modifiers first (the table's order within a count).
pub fn ordered() -> &'static [&'static Shortcut] {
    static ORDER: OnceLock<Vec<&'static Shortcut>> = OnceLock::new();
    ORDER.get_or_init(|| {
        let mut v: Vec<&'static Shortcut> = TABLE.iter().collect();
        v.sort_by_key(|r| std::cmp::Reverse(r.modifier_count()));
        v
    })
}

/// Reads the table: runs the action of every chord pressed this frame.
pub fn dispatch(app: &mut PacketApp, ctx: &Context) {
    let typing = ctx.egui_wants_keyboard_input();
    // A widget that has the focus (a focused device on the workspace, a button) takes Enter itself.
    let focused = ctx.memory(|m| m.focused().is_some());
    for r in ordered() {
        if typing && r.when == When::Idle {
            continue;
        }
        if r.key == Key::Enter && r.mods == 0 && focused {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&r.shortcut())) {
            perform(app, ctx, r.act);
        }
    }
}

fn dir_vec(d: Dir) -> (f32, f32) {
    match d {
        Dir::Up => (0.0, -1.0),
        Dir::Down => (0.0, 1.0),
        Dir::Left => (-1.0, 0.0),
        Dir::Right => (1.0, 0.0),
    }
}

/// Moves the selected device one grid step.
fn move_selected(app: &mut PacketApp, d: Dir) {
    if app.session.physical {
        return;
    }
    let Some(id) = app.ui.selected else { return };
    let Some(dev) = app.session.topology.device(id) else { return };
    let (dx, dy) = dir_vec(d);
    let (x, y) = (dev.x + dx * crate::GRID_STEP, dev.y + dy * crate::GRID_STEP);
    app.run("device.move", json!({"device": id.0, "x": x, "y": y}));
}

/// Selects the nearest device in a direction from the selected one (the first when none is).
fn select_toward(app: &mut PacketApp, d: Dir) {
    let t = &app.session.topology;
    let current = app.ui.selected.and_then(|id| t.device(id));
    let next = match current {
        None => t.devices.first().map(|x| x.id),
        Some(c) => {
            let (ux, uy) = dir_vec(d);
            t.devices
                .iter()
                .filter(|x| x.id != c.id)
                .filter_map(|x| {
                    let (dx, dy) = (x.x - c.x, x.y - c.y);
                    let along = dx * ux + dy * uy;
                    let across = (dx * uy - dy * ux).abs();
                    (along > 0.0).then_some((along + 2.0 * across, x.id))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, id)| id)
        }
    };
    if let Some(id) = next {
        app.ui.selected = Some(id);
    }
}

/// A new place of a kind, when the map shown holds that kind.
fn new_place(app: &mut PacketApp, kind: &str, label: &'static str) {
    if app.session.physical && crate::physical::child_kind(app).is_some_and(|k| k.id() == kind) {
        crate::physical::add_place(app, None);
    } else {
        let msg = trf("{} is not available at this level of the Physical view.", &[&tl(label)]);
        app.ui.status = Some((msg, true));
    }
}

fn show_category(app: &mut PacketApp, cat: Category, sub: Sub) {
    app.ui.toolbars.bottom = true;
    crate::bottom::show_category(app, cat, sub);
}

/// Runs an action.
pub fn perform(app: &mut PacketApp, ctx: &Context, act: Act) {
    match act {
        Act::New => app.new_lab(None),
        Act::Open => app.requests.push(UiRequest::Open),
        Act::OpenSamples => app.ui.show_samples = true,
        Act::Save => app.save(false),
        Act::SaveAs => app.save(true),
        Act::ResetActivity => crate::activity::reset(app, ctx.input(|i| i.time)),
        Act::Print => app.request_image(ctx, true),
        Act::Copy => app.copy(),
        Act::Paste => app.paste(),
        Act::Undo => {
            app.run("edit.undo", json!({}));
        }
        Act::Redo => {
            app.run("edit.redo", json!({}));
        }
        Act::Group => app.group_selection(),
        Act::Preferences => app.open_preferences(),
        Act::NetworkInfo => app.ui.show_notes = !app.ui.show_notes,
        Act::UserProfile => app.open_user_profile(),
        Act::AlgorithmSettings => app.open_algorithm_settings(),
        Act::CommandLog => app.open_command_log(),
        Act::ZoomIn => app.zoom(Some(1.25)),
        Act::ZoomReset => {
            app.zoom(None);
            app.ui.pan = egui::Vec2::ZERO;
        }
        Act::ZoomOut => app.zoom(Some(0.8)),
        Act::ShowGrid => app.ui.show_grid = !app.ui.show_grid,
        Act::Simulation => app.ui.mode = Mode::Simulation,
        Act::Realtime => app.ui.mode = Mode::Realtime,
        Act::Logical => {
            if app.session.physical {
                app.set_physical(false);
            }
        }
        Act::Physical => {
            if !app.session.physical {
                app.set_physical(true);
            }
        }
        Act::Viewport => app.ui.show_viewport = !app.ui.show_viewport,
        Act::WorkspaceList => app.ui.show_workspace_list = !app.ui.show_workspace_list,
        Act::ToolbarMain => app.ui.toolbars.main = !app.ui.toolbars.main,
        Act::ToolbarSecondary => app.ui.toolbars.secondary = !app.ui.toolbars.secondary,
        Act::ToolbarBottom => app.ui.toolbars.bottom = !app.ui.toolbars.bottom,
        Act::GoBack => {
            if app.session.physical {
                crate::physical::leave(app, false);
            } else {
                app.run("view.up", json!({}));
            }
        }
        Act::NewCity => new_place(app, "city", "New City"),
        Act::NewBuilding => new_place(app, "building", "New Building"),
        Act::NewCloset => new_place(app, "closet", "New Wiring Closet"),
        Act::ActivityWizard => app.ui.activity.show_wizard = true,
        Act::DrawingPalette => app.open_drawing_palette(),
        Act::Help => app.ui.help = Some(crate::help::Page::Contents),
        Act::Escape => {
            // Esc first drops a polygon being placed, then the tool.
            if app.ui.canvas.poly.is_empty() {
                app.set_tool(Tool::Select);
            } else {
                app.ui.canvas.poly.clear();
            }
        }
        Act::Note => app.set_tool(Tool::Note),
        Act::DeleteKey => match app.ui.selected.take() {
            Some(d) => {
                app.run("device.remove", json!({"device": d.0}));
            }
            None => app.set_tool(Tool::Delete),
        },
        Act::Inspect => app.set_tool(Tool::Inspect),
        Act::SimplePdu => app.set_tool(Tool::Ping),
        Act::ComplexPdu => {
            let src = app.ui.selected;
            crate::pdu_dialog::open(app, src);
        }
        Act::Line => app.set_tool(Tool::Draw(Shape::Line)),
        Act::Rectangle => app.set_tool(Tool::Draw(Shape::Rectangle)),
        Act::Ellipse => app.set_tool(Tool::Draw(Shape::Ellipse)),
        Act::Freeform => app.set_tool(Tool::Draw(Shape::Freehand)),
        Act::PowerCycle => {
            app.run("sim.reset", json!({}));
            app.info(tl("Devices power-cycled: dynamic tables cleared."));
        }
        Act::FastForward => {
            app.run("sim.advance", json!({"seconds": 30}));
        }
        Act::SimBack => crate::events::step(app, false),
        Act::SimPlay => crate::events::toggle_play(app),
        Act::SimForward => crate::events::step(app, true),
        Act::EventList => {
            // The list lives in Simulation mode: the first press goes there, the next hides or shows it.
            if app.ui.mode == Mode::Simulation {
                app.ui.show_event_list = !app.ui.show_event_list;
            } else {
                app.ui.mode = Mode::Simulation;
                app.ui.show_event_list = true;
            }
        }
        Act::Routers => show_category(app, Category::NetworkDevices, Sub::Routers),
        Act::Switches => show_category(app, Category::NetworkDevices, Sub::Switches),
        Act::Hubs => show_category(app, Category::NetworkDevices, Sub::Hubs),
        Act::Wan => show_category(app, Category::NetworkDevices, Sub::Wan),
        Act::EndDevices => show_category(app, Category::EndDevices, Sub::Hosts),
        Act::Connections => show_category(app, Category::Connections, Sub::Cables),
        Act::OpenDevice => {
            if let Some(d) = app.ui.selected {
                crate::console::open(app, d);
            }
        }
        Act::MoveSelected(d) => move_selected(app, d),
        Act::SelectToward(d) => select_toward(app, d),
        Act::NewScenario => {
            app.run("scenario.new", json!({}));
        }
        Act::DeleteScenario => {
            app.run("scenario.delete", json!({}));
        }
        Act::PduList => app.ui.bottom.hide_pdus = !app.ui.bottom.hide_pdus,
    }
}

/// The Open Samples window (Ctrl+Shift+T): one button per sample lab.
pub fn windows(app: &mut PacketApp, ctx: &Context) {
    if !app.ui.show_samples {
        return;
    }
    let mut open = true;
    let mut pick = None;
    egui::Window::new(tl("Open Samples")).open(&mut open).default_width(360.0).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for s in netlabcraft_engine::sample::SAMPLES {
                if ui.button(crate::i18n::tr(s.title)).on_hover_text(crate::i18n::tr(s.description)).clicked() {
                    pick = Some(s.id);
                }
            }
        });
    });
    if let Some(id) = pick {
        app.new_lab(Some(id));
        open = false;
    }
    app.ui.show_samples = open;
}

/// The Help page: every shortcut by group.
pub fn page(ui: &mut egui::Ui) {
    ui.heading(tl("Keyboard shortcuts"));
    ui.label(tl("Keys that type a letter are ignored while a text field or the console has the keyboard."));
    for g in GROUPS {
        ui.add_space(4.0);
        ui.strong(tl(g));
        egui::Grid::new(format!("shortcuts_{g}")).num_columns(2).spacing([24.0, 2.0]).show(ui, |ui| {
            for r in TABLE.iter().filter(|r| r.group == g) {
                ui.monospace(r.text());
                ui.label(tl(r.label));
                ui.end_row();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_rows_share_a_chord() {
        let mut seen = std::collections::BTreeSet::new();
        for r in TABLE {
            assert!(seen.insert((r.mods, r.key.name())), "{} is bound twice", r.text());
        }
    }

    #[test]
    fn a_chord_with_more_modifiers_is_read_first() {
        let counts: Vec<u32> = ordered().iter().map(|r| r.modifier_count()).collect();
        assert!(counts.windows(2).all(|w| w[0] >= w[1]), "{counts:?}");
        assert_eq!(ordered().len(), TABLE.len());
        // Ctrl+Shift+Z (Redo) comes before Ctrl+Z (Undo), Ctrl+Alt+O before Ctrl+O.
        let at = |a: Act, m: u8| ordered().iter().position(|r| r.act == a && r.mods == m);
        assert!(at(Act::Redo, C | S) < at(Act::Undo, C));
        assert!(at(Act::Connections, C | A) < at(Act::Open, C));
        assert!(at(Act::Realtime, S) < at(Act::Rectangle, 0));
    }

    #[test]
    fn every_row_names_its_group_and_reads_in_text() {
        for r in TABLE {
            assert!(GROUPS.contains(&r.group), "{}", r.label);
            assert!(!r.label.is_empty() && !r.text().is_empty());
        }
        for g in GROUPS {
            assert!(TABLE.iter().any(|r| r.group == g), "{g}");
        }
        assert_eq!(text(Act::Connections), "Ctrl+Alt+O");
        assert_eq!(text(Act::Escape), "Esc");
        assert_eq!(text(Act::SelectToward(Dir::Left)), "Shift+Left");
        assert_eq!(text(Act::PduList), "Ctrl+Shift+O");
    }

    #[test]
    fn every_row_reads_in_spanish() {
        let es = crate::i18n::spanish();
        let mut missing: Vec<&str> = TABLE.iter().map(|r| r.label).chain(GROUPS).filter(|l| !es.contains_key(l)).collect();
        missing.sort_unstable();
        missing.dedup();
        assert!(missing.is_empty(), "no Spanish row for: {missing:#?}");
    }
}
