//! Preferences ▸ Miscellaneous: automatic backups, what happens when the event list is full,
//! buffering only the events the filter lets through, and the device window taskbar.
//!
//! Backups are written by the desktop shell (`UiRequest::Backup`, through the engine's
//! `file.backup`, which never replaces a file and leaves the lab's path and unsaved mark alone).
//! The event list is the simulator's trace; it holds at most `TRACE_CAP` events and forgets the
//! oldest, so "full" means it has reached that many.

use crate::prefs_look::HintExt;
use egui::RichText;
use serde_json::json;

use crate::i18n::{tl, trf};
use crate::{PacketApp, UiRequest};

/// The minutes between automatic backups the tab offers (0 is off).
pub const BACKUP_MINUTES: [u16; 5] = [0, 1, 5, 10, 30];

/// What to do when the event list is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BufferFull {
    /// Ask.
    #[default]
    Prompt,
    /// Clear the list and go on.
    AutoClear,
    /// Keep the full list aside, clear it, and show the kept events.
    AutoView,
}

impl BufferFull {
    pub const ALL: [BufferFull; 3] = [BufferFull::Prompt, BufferFull::AutoClear, BufferFull::AutoView];

    pub fn key(self) -> &'static str {
        match self {
            BufferFull::Prompt => "prompt",
            BufferFull::AutoClear => "clear",
            BufferFull::AutoView => "view",
        }
    }

    pub fn from_key(key: &str) -> Option<BufferFull> {
        Self::ALL.into_iter().find(|b| b.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            BufferFull::Prompt => "Prompt",
            BufferFull::AutoClear => "Clear the event list",
            BufferFull::AutoView => "View the previous events",
        }
    }
}

/// One event kept aside when the list was full.
#[derive(Debug, Clone, PartialEq)]
pub struct Past {
    pub id: u64,
    pub at_us: u64,
    pub from: String,
    pub to: String,
    pub protocol: String,
    pub summary: String,
}

/// What the tab and its windows remember between frames.
#[derive(Debug, Clone, Default)]
pub struct MiscState {
    /// The "event list is full" question is open.
    pub(crate) full_prompt: bool,
    /// The list has been full since it was last handled (it is asked about once per filling).
    pub(crate) full_seen: bool,
    /// The events kept aside by "View the previous events".
    pub previous: Vec<Past>,
    pub(crate) show_previous: bool,
    /// When the next backup is due (frame clock, seconds).
    pub(crate) next_backup: Option<f64>,
    /// The lab's revision when the last backup was asked for.
    pub(crate) last_backup_rev: Option<u64>,
    /// The event list and the filter as they were when it was last pruned.
    prune_mark: Option<((usize, u64), Vec<bool>)>,
}

fn minutes_text(m: u16) -> String {
    match m {
        0 => tl("Off").to_string(),
        1 => tl("1 minute").to_string(),
        n => trf("{} minutes", &[&n]),
    }
}

pub(crate) fn tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Backups")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let desktop = app.ui.prefs_ui.desktop;
        ui.add_enabled_ui(desktop, |ui| {
            ui.horizontal(|ui| {
                let l = ui.label(tl("Auto File Backup Interval:"));
                let p = &mut app.ui.prefs;
                egui::ComboBox::from_id_salt("backup_minutes").selected_text(minutes_text(p.backup_minutes)).show_ui(ui, |ui| {
                    for m in BACKUP_MINUTES {
                        ui.selectable_value(&mut p.backup_minutes, m, minutes_text(m));
                    }
                });
                let _ = l;
            });
        });
        let note = if desktop {
            tl("Writes a copy of the open lab in the backups folder of the user folder (or of the settings folder), only when the lab changed. Your own file is never touched, and a backup never replaces another.")
        } else {
            tl("Not available in the web version: the browser keeps no files for the page.")
        };
        ui.label(RichText::new(note).small());
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Simulation")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            let l = ui.label(tl("When the event list is full:"));
            let p = &mut app.ui.prefs;
            egui::ComboBox::from_id_salt("buffer_full").selected_text(tl(p.buffer_full.label())).show_ui(ui, |ui| {
                for b in BufferFull::ALL {
                    ui.selectable_value(&mut p.buffer_full, b, tl(b.label()));
                }
            });
            let _ = l;
        });
        ui.label(RichText::new(trf("The list holds {} events and forgets the oldest ones when it is full.", &[&netlabcraft_sim::TRACE_CAP])).small());
        crate::prefs::pref(
            ui,
            &mut app.ui.prefs.buffer_filtered_only,
            "Buffer only the events that pass the filter",
            Some("Events the Edit Filters list hides are dropped instead of being kept."),
        );
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Windows")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        crate::prefs::pref(
            ui,
            &mut app.ui.prefs.device_taskbar,
            "Show the device window taskbar",
            Some("A bar under the workspace lists the open device windows: click one to bring it to the front."),
        );
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Not applicable")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let mut off = false;
        ui.add_enabled(false, egui::Checkbox::new(&mut off, tl("Reload local image files")));
        ui.label(RichText::new(tl("Pictures are stored inside the lab, so there is nothing to reload.")).small());
        ui.add_enabled(false, egui::Checkbox::new(&mut off, tl("External network access")));
        ui.add_enabled(false, egui::Checkbox::new(&mut off, tl("Use a proxy")));
        ui.label(RichText::new(tl("NetLabCraft makes no network connections of its own, so it has no network access or proxy to set.")).small());
    });
}

/// What the settings ask for, once per frame.
pub(crate) fn run(app: &mut PacketApp, ctx: &egui::Context) {
    // The device windows were drawn before this: a taskbar click has had its effect.
    app.ui.prefs_ui.raise = None;
    let now = ctx.input(|i| i.time);
    let full = app.session.sim.trace().len() >= netlabcraft_sim::TRACE_CAP;
    buffer_check(app, full);
    prune_filtered(app);
    backup_tick(app, ctx, now);
    full_window(app, ctx);
    previous_window(app, ctx);
}

/// Applies "when the event list is full" once each time it fills.
pub(crate) fn buffer_check(app: &mut PacketApp, full: bool) {
    if !full {
        app.ui.prefs_ui.misc.full_seen = false;
        return;
    }
    if app.ui.prefs_ui.misc.full_seen {
        return;
    }
    app.ui.prefs_ui.misc.full_seen = true;
    match app.ui.prefs.buffer_full {
        BufferFull::Prompt => app.ui.prefs_ui.misc.full_prompt = true,
        BufferFull::AutoClear => clear_events(app),
        BufferFull::AutoView => view_previous(app),
    }
}

fn clear_events(app: &mut PacketApp) {
    app.run("sim.clear_events", json!({}));
    app.ui.events.selected = None;
    app.ui.events.playing = false;
    app.ui.prefs_ui.misc.full_seen = false;
}

/// Keeps what the list holds aside, clears it and opens the window that shows it.
fn view_previous(app: &mut PacketApp) {
    let t = &app.session.topology;
    let name = |d: netlabcraft_core::DeviceId| t.device(d).map_or_else(|| "--".to_string(), |x| x.display_name().to_string());
    let kept: Vec<Past> = app
        .session
        .sim
        .trace()
        .iter()
        .map(|e| Past {
            id: e.id,
            at_us: e.at_us,
            from: name(e.from.device),
            to: name(e.to.device),
            protocol: e.protocol.name().to_string(),
            summary: e.summary.clone(),
        })
        .collect();
    app.ui.prefs_ui.misc.previous = kept;
    app.ui.prefs_ui.misc.show_previous = true;
    clear_events(app);
}

/// With "buffer only filtered events", drops what the filter hides, when the list or the filter changed.
fn prune_filtered(app: &mut PacketApp) {
    if !app.ui.prefs.buffer_filtered_only {
        app.ui.prefs_ui.misc.prune_mark = None;
        return;
    }
    let mark = |app: &PacketApp| {
        let trace = app.session.sim.trace();
        ((trace.len(), trace.back().map_or(0, |e| e.id)), app.ui.events.filter.to_vec())
    };
    let now = mark(app);
    if app.ui.prefs_ui.misc.prune_mark.as_ref() == Some(&now) {
        return;
    }
    let hides = app.session.sim.trace().iter().any(|e| !app.ui.events.visible(e.protocol));
    if hides {
        let keep = app.ui.events.kept_protocols();
        app.run("sim.prune_events", json!({"keep": keep}));
    }
    let after = mark(app);
    app.ui.prefs_ui.misc.prune_mark = Some(after);
}

/// Asks the shell for a backup when one is due and the lab changed since the last one.
pub(crate) fn backup_tick(app: &mut PacketApp, ctx: &egui::Context, now: f64) {
    let minutes = app.ui.prefs.backup_minutes;
    if minutes == 0 || !app.ui.prefs_ui.desktop {
        app.ui.prefs_ui.misc.next_backup = None;
        return;
    }
    let every = f64::from(minutes) * 60.0;
    ctx.request_repaint_after(std::time::Duration::from_secs(30));
    let due = *app.ui.prefs_ui.misc.next_backup.get_or_insert(now + every);
    if now < due {
        return;
    }
    app.ui.prefs_ui.misc.next_backup = Some(now + every);
    if app.ui.prefs_ui.misc.last_backup_rev == Some(app.session.revision) {
        return;
    }
    app.ui.prefs_ui.misc.last_backup_rev = Some(app.session.revision);
    let stem = backup_stem(app.session.path.as_deref());
    let folder = app.user_folder().map(str::to_string);
    app.requests.push(UiRequest::Backup { stem, folder });
}

/// A file-name stem for the backups of a lab: its file's name, made of letters, digits, `-` and `_`.
pub(crate) fn backup_stem(path: Option<&str>) -> String {
    let raw = path.and_then(|p| std::path::Path::new(p).file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let clean: String = raw.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(40).collect();
    if clean.is_empty() { "lab".to_string() } else { clean }
}

fn full_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.prefs_ui.misc.full_prompt {
        return;
    }
    let mut choice: Option<BufferFull> = None;
    let mut keep = false;
    egui::Window::new(tl("Event List Full")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(trf("The event list holds {} events, its limit. What now?", &[&netlabcraft_sim::TRACE_CAP]));
        ui.horizontal(|ui| {
            if ui.button(tl("Clear the event list")).clicked() {
                choice = Some(BufferFull::AutoClear);
            }
            if ui.button(tl("View the previous events")).clicked() {
                choice = Some(BufferFull::AutoView);
            }
            if ui.button(tl("Keep going")).hint(tl("The oldest events are forgotten as new ones arrive.")).clicked() {
                keep = true;
            }
        });
    });
    if choice.is_some() || keep {
        app.ui.prefs_ui.misc.full_prompt = false;
    }
    match choice {
        Some(BufferFull::AutoClear) => clear_events(app),
        Some(BufferFull::AutoView) => view_previous(app),
        _ => {}
    }
}

fn previous_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.prefs_ui.misc.show_previous {
        return;
    }
    let mut open = true;
    egui::Window::new(tl("Previous Events")).open(&mut open).default_size([640.0, 360.0]).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        let list = &app.ui.prefs_ui.misc.previous;
        ui.label(trf("{} events kept from the list that filled up.", &[&list.len()]));
        ui.separator();
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both().id_salt("previous_events").auto_shrink([false, false]).show_rows(ui, row, list.len(), |ui, range| {
            for e in list.iter().skip(range.start).take(range.len()) {
                let line = format!(
                    "{:>6}  {:>10.3}  {} -> {}  {}  {}",
                    e.id,
                    e.at_us as f64 / 1e6,
                    e.from,
                    e.to,
                    e.protocol,
                    crate::i18n::tr_event(&e.summary)
                );
                ui.add(egui::Label::new(RichText::new(line).monospace()).wrap_mode(egui::TextWrapMode::Extend));
            }
        });
    });
    if !open {
        app.ui.prefs_ui.misc.show_previous = false;
        app.ui.prefs_ui.misc.previous.clear();
    }
}

/// The device window taskbar: one button per open device window.
pub(crate) fn taskbar(app: &mut PacketApp, ui: &mut egui::Ui) {
    let ids: Vec<netlabcraft_core::DeviceId> = app.ui.windows.keys().copied().collect();
    ui.horizontal_wrapped(|ui| {
        for d in ids {
            let name = app.session.topology.device(d).map_or_else(|| format!("#{}", d.0), |x| x.display_name().to_string());
            if ui.button(name).clicked() {
                crate::console::open(app, d);
                app.ui.prefs_ui.raise = Some(d);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_stems_are_file_safe() {
        for b in BufferFull::ALL {
            assert_eq!(BufferFull::from_key(b.key()), Some(b));
        }
        assert_eq!(BufferFull::from_key("nope"), None);
        assert_eq!(backup_stem(Some("some/dir/My Lab (1).nlcraft")), "My_Lab__1_");
        assert_eq!(backup_stem(None), "lab");
        assert_eq!(backup_stem(Some("")), "lab");
        assert_eq!(backup_stem(Some(&"x".repeat(200))).chars().count(), 40);
    }
}
