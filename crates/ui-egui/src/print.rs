//! File ▸ Save as PDF… and File ▸ Print…
//!
//! Both build the same report PDF ([`crate::pdf`]) from a screenshot of the workspace. Save as PDF
//! hands the bytes to the shell at once (a save dialog on the desktop, a download on the web).
//! Print… asks the shell what it can do ([`PrintChoices`], set by the desktop app; the web build
//! sets none and so downloads the file) and, when there is a choice, shows a small window with the
//! three ways out: save the file, open it in the system's PDF viewer, or send it to a printer.

use crate::prefs_look::HintExt;
use egui::{Context, RichText};

use crate::i18n::tl;
use crate::{BELOW_TOOLBARS, PacketApp, UiRequest};

/// What a pending screenshot becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfUse {
    /// File ▸ Save as PDF…
    Save,
    /// File ▸ Print…
    Print,
}

/// What the shell can do with a PDF besides saving it (the desktop app fills it in).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrintChoices {
    /// A program can open the file (`xdg-open`, `open`, `start`).
    pub viewer: bool,
    /// Why printing straight to a printer is not possible here, or `None` when it is.
    pub direct_unavailable: Option<String>,
}

/// A PDF waiting for the person's choice in the Print window.
#[derive(Debug, Clone)]
pub struct PrintJob {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A file name for the lab's report: its title with anything a file system dislikes replaced, and
/// `.pdf` (`workspace.pdf` when nothing is left).
pub fn pdf_file_name(title: &str) -> String {
    let stem: String = title.trim().chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect();
    let stem: String = stem.trim_matches(|c| matches!(c, ' ' | '.' | '_')).chars().take(80).collect();
    if stem.is_empty() { "workspace.pdf".to_string() } else { format!("{stem}.pdf") }
}

impl PacketApp {
    /// The report PDF is ready (the screenshot arrived): save it, or ask what to do with it.
    pub(crate) fn pdf_ready(&mut self, usage: PdfUse, name: String, bytes: Vec<u8>) {
        if usage == PdfUse::Print && self.ui.print_choices.is_some() {
            self.ui.print_job = Some(PrintJob { name, bytes });
        } else {
            self.requests.push(UiRequest::SavePdf { name, bytes });
        }
    }
}

#[derive(Clone, Copy)]
enum Choice {
    Save,
    Open,
    Print,
}

/// The Print window (desktop): save, open in the viewer, or print directly.
pub fn window(app: &mut PacketApp, ctx: &Context) {
    let Some(name) = app.ui.print_job.as_ref().map(|j| j.name.clone()) else { return };
    let choices = app.ui.print_choices.clone().unwrap_or_default();
    let mut open = true;
    let mut pick = None;
    egui::Window::new(tl("Print")).open(&mut open).collapsible(false).resizable(false).default_pos(BELOW_TOOLBARS).show(ctx, |ui| {
        ui.label(tl("The report is ready as a PDF. What do you want to do with it?"));
        ui.label(RichText::new(&name).small());
        ui.add_space(4.0);
        if ui.button(tl("Save as PDF...")).hint(tl("Choose where to keep the file")).clicked() {
            pick = Some(Choice::Save);
        }
        let viewer = ui.add_enabled(choices.viewer, egui::Button::new(tl("Open in the PDF viewer")));
        let viewer = if choices.viewer {
            viewer.hint(tl("Opens a temporary copy in the program your system uses for PDF files"))
        } else {
            viewer.disabled_hint(tl("This system has no program to open files with: use Save as PDF"))
        };
        if viewer.clicked() {
            pick = Some(Choice::Open);
        }
        let direct = ui.add_enabled(choices.direct_unavailable.is_none(), egui::Button::new(tl("Print directly")));
        let direct = match &choices.direct_unavailable {
            Some(why) => direct.disabled_hint(crate::i18n::tr(why)),
            None => direct.hint(tl("Sends the file to the default printer")),
        };
        if direct.clicked() {
            pick = Some(Choice::Print);
        }
    });
    if let Some(c) = pick {
        if let Some(job) = app.ui.print_job.take() {
            let PrintJob { name, bytes } = job;
            app.requests.push(match c {
                Choice::Save => UiRequest::SavePdf { name, bytes },
                Choice::Open => UiRequest::OpenPdf { name, bytes },
                Choice::Print => UiRequest::PrintPdf { name, bytes },
            });
        }
    } else if !open {
        app.ui.print_job = None;
    }
}

#[cfg(test)]
mod tests {
    use super::pdf_file_name;

    #[test]
    fn the_file_name_comes_from_the_lab_title() {
        assert_eq!(pdf_file_name("Campus lab"), "Campus lab.pdf");
        assert_eq!(pdf_file_name("a/b\\c:d"), "a_b_c_d.pdf");
        assert_eq!(pdf_file_name("  ..  "), "workspace.pdf");
        assert_eq!(pdf_file_name(""), "workspace.pdf");
        assert!(pdf_file_name(&"x".repeat(500)).len() <= 84);
    }
}
