//! Preferences ▸ Administrative: a local password that locks tabs of the Preferences window,
//! and the user folder.
//!
//! The lock keeps people out of the Preferences window, nothing more. The record of the password
//! (a salted PBKDF2 hash, never the password) lives in the preferences file, which the person at
//! the keyboard can edit or delete, so this stops a student clicking through the window, not one
//! who reads the file. Locked tabs open read-only until the password is entered; the menus and
//! shortcuts that toggle the same settings are not locked.

use egui::RichText;

use crate::i18n::tl;
use crate::prefs::{Prefs, Tab, WindowState};
use crate::{PacketApp, UiRequest};

/// Fewest characters a password may have.
pub const MIN_PASSWORD: usize = 4;

/// Whether a tab is read-only now: the password is enabled, was not entered this session, and
/// the tab is one the Interface Locking boxes cover (the Administrative tab always is).
pub(crate) fn tab_locked(prefs: &Prefs, ui: &WindowState, tab: Tab) -> bool {
    if prefs.lock_record.is_empty() || ui.unlocked {
        return false;
    }
    match tab {
        Tab::Interface => prefs.lock_interface,
        Tab::ShowHide => prefs.lock_showhide,
        Tab::CustomInterfaces => prefs.lock_custom,
        Tab::Miscellaneous => prefs.lock_misc,
        Tab::Administrative => true,
        _ => false,
    }
}

/// What makes each salt different: the time on the wall clock and the frame clock, and a counter.
fn entropy(app: &PacketApp, ui: &egui::Ui) -> String {
    format!("{}|{}|{}", netlabcraft_engine::instructor::now_utc(), ui.input(|i| i.time).to_bits(), app.session.revision)
}

/// The strip above the tabs when a password is enabled: enter it, or lock again.
pub(crate) fn lock_bar(app: &mut PacketApp, ui: &mut egui::Ui) {
    if app.ui.prefs.lock_record.is_empty() {
        return;
    }
    if app.ui.prefs_ui.unlocked {
        ui.horizontal(|ui| {
            ui.label(tl("Unlocked until you lock it again or close the program."));
            if ui.button(tl("Lock")).clicked() {
                app.ui.prefs_ui.unlocked = false;
                app.ui.prefs_ui.pw_entry.clear();
            }
        });
        return;
    }
    let mut unlock = false;
    ui.horizontal(|ui| {
        let l = ui.label(tl("Locked. Enter the password:"));
        let r = ui.add(egui::TextEdit::singleline(&mut app.ui.prefs_ui.pw_entry).password(true).desired_width(160.0)).labelled_by(l.id);
        unlock = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        unlock |= ui.button(tl("Unlock")).clicked();
    });
    if unlock {
        let entry = std::mem::take(&mut app.ui.prefs_ui.pw_entry);
        if netlabcraft_engine::settings::lock_check(&app.ui.prefs.lock_record, &entry) {
            app.ui.prefs_ui.unlocked = true;
            app.ui.prefs_ui.pw_note = None;
        } else {
            app.ui.prefs_ui.pw_note = Some((tl("That is not the password.").to_string(), true));
        }
    }
    if let Some((note, true)) = &app.ui.prefs_ui.pw_note {
        ui.colored_label(crate::theme::tokens(app.ui.dark).error, note);
    }
}

pub(crate) fn tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    let locked = tab_locked(&app.ui.prefs, &app.ui.prefs_ui, Tab::Administrative);
    ui.add_enabled_ui(!locked, |ui| {
        password_section(app, ui);
        ui.add_space(6.0);
        locking_section(app, ui);
    });
    ui.add_space(6.0);
    ui.add_enabled_ui(!locked, |ui| folder_section(app, ui));
}

fn password_section(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Choose Password")).strong());
    let (mut enable, mut disable) = (false, false);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let state = &mut app.ui.prefs_ui;
        egui::Grid::new("pw_fields").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            let l = ui.label(tl("Password:"));
            ui.add(egui::TextEdit::singleline(&mut state.pw_draft).password(true).desired_width(200.0)).labelled_by(l.id);
            ui.end_row();
            let l = ui.label(tl("Confirm:"));
            ui.add(egui::TextEdit::singleline(&mut state.pw_confirm).password(true).desired_width(200.0)).labelled_by(l.id);
            ui.end_row();
        });
        let ok = state.pw_draft.chars().count() >= MIN_PASSWORD && state.pw_draft == state.pw_confirm;
        let enabled = !app.ui.prefs.lock_record.is_empty();
        ui.horizontal(|ui| {
            let b = ui.add_enabled(ok, egui::Button::new(tl("Enable Password")));
            enable = b.clicked();
            let d = ui.add_enabled(enabled, egui::Button::new(tl("Disable Password")));
            disable = d.clicked();
        });
        ui.label(
            RichText::new(crate::i18n::trf("At least {} characters; both fields must match. Enabling again changes the password.", &[&MIN_PASSWORD]))
                .small(),
        );
        ui.label(
            RichText::new(tl(
                "Only a salted hash is kept, never the password. The lock keeps people out of this window, not out of the preferences file.",
            ))
            .small(),
        );
        ui.label(RichText::new(if enabled { tl("A password is enabled.") } else { tl("No password is set: nothing is locked.") }).strong());
    });
    if enable {
        let record = netlabcraft_engine::settings::lock_record(&app.ui.prefs_ui.pw_draft, &entropy(app, ui));
        app.ui.prefs.lock_record = record;
        app.ui.prefs_ui.unlocked = true;
        app.ui.prefs_ui.pw_draft.clear();
        app.ui.prefs_ui.pw_confirm.clear();
        app.info(tl("Password enabled."));
    }
    if disable {
        app.ui.prefs.lock_record.clear();
        app.ui.prefs_ui.unlocked = false;
        app.ui.prefs_ui.pw_draft.clear();
        app.ui.prefs_ui.pw_confirm.clear();
        app.info(tl("Password disabled."));
    }
}

fn locking_section(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Interface Locking")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new(tl("With a password enabled, the ticked tabs open read-only until it is entered.")).small());
        let p = &mut app.ui.prefs;
        crate::prefs::pref(ui, &mut p.lock_interface, "Lock the Interface tab", None);
        crate::prefs::pref(ui, &mut p.lock_showhide, "Lock the Show/Hide tab", None);
        crate::prefs::pref(ui, &mut p.lock_custom, "Lock the Custom Interfaces tab", None);
        crate::prefs::pref(ui, &mut p.lock_misc, "Lock the Miscellaneous tab", None);
        ui.label(RichText::new(tl("There are no Multiuser, IPC or scripting menus to lock.")).small());
    });
}

fn folder_section(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("User Folder")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let desktop = app.ui.prefs_ui.desktop;
        ui.add_enabled_ui(desktop, |ui| {
            ui.horizontal(|ui| {
                let l = ui.label(tl("Folder:"));
                ui.add(egui::TextEdit::singleline(&mut app.ui.prefs.user_folder).desired_width(320.0)).labelled_by(l.id);
                if ui.button(tl("Browse…")).clicked() {
                    app.requests.push(UiRequest::PickFolder);
                }
                if ui.button(tl("Clear")).clicked() {
                    app.ui.prefs.user_folder.clear();
                }
            });
        });
        if !desktop {
            ui.label(RichText::new(tl("Not available in the web version: the browser chooses where files go.")).small());
        }
        ui.label(RichText::new(tl("The Open and Save dialogs start here, and automatic backups are written in a backups folder inside it.")).small());
    });
}

impl PacketApp {
    /// The shell picked a folder for Browse.
    pub fn folder_picked(&mut self, path: &str) {
        if path.len() <= 1024 && !path.chars().any(char::is_control) {
            self.ui.prefs.user_folder = path.to_string();
        }
    }

    /// The folder the Open and Save dialogs start in, when one is set.
    pub fn user_folder(&self) -> Option<&str> {
        Some(self.ui.prefs.user_folder.as_str()).filter(|f| !f.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn administrative_is_always_covered_and_others_only_when_ticked() {
        let mut p = Prefs { lock_record: netlabcraft_engine::settings::lock_record("abcd", "t"), lock_misc: true, ..Prefs::default() };
        let mut w = WindowState::default();
        assert!(tab_locked(&p, &w, Tab::Miscellaneous) && tab_locked(&p, &w, Tab::Administrative));
        assert!(!tab_locked(&p, &w, Tab::Interface) && !tab_locked(&p, &w, Tab::Font));
        w.unlocked = true;
        assert!(!tab_locked(&p, &w, Tab::Miscellaneous));
        w.unlocked = false;
        p.lock_record.clear();
        assert!(!tab_locked(&p, &w, Tab::Miscellaneous), "no password, no lock");
    }
}
