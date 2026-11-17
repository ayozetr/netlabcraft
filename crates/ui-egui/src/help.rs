//! The Help menu's pages: Getting Started, Contents, Tutorials and Accessibility. The text is ours; the longer
//! reading lives in the repository's `docs/` folder, linked from here.

use crate::PacketApp;
use crate::i18n::tl;

/// Where "Report an Issue" goes.
pub const ISSUES_URL: &str = "https://github.com/ayozetr/netlabcraft/issues/new";

/// The repository's documentation on GitHub.
const DOCS_URL: &str = "https://github.com/ayozetr/netlabcraft/blob/main/docs";

/// The lab book: step-by-step exercises.
const LABS_URL: &str = "https://github.com/ayozetr/netlabcraft/tree/main/docs/labs";

/// A page of the Help window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    GettingStarted,
    Contents,
    Tutorials,
    Accessibility,
    Shortcuts,
}

impl Page {
    pub const ALL: [Page; 5] = [Page::GettingStarted, Page::Contents, Page::Tutorials, Page::Accessibility, Page::Shortcuts];

    /// The English title (the menu entry and the page's tab).
    pub fn title(self) -> &'static str {
        match self {
            Page::GettingStarted => "Getting Started",
            Page::Contents => "Contents",
            Page::Tutorials => "Tutorials",
            Page::Accessibility => "Accessibility",
            Page::Shortcuts => "Keyboard shortcuts",
        }
    }
}

fn doc_link(ui: &mut egui::Ui, label: &'static str, file: &str) {
    ui.hyperlink_to(tl(label), format!("{DOCS_URL}/{file}"));
}

/// The Help window, while a page is chosen.
pub fn window(app: &mut PacketApp, ctx: &egui::Context) {
    let Some(mut page) = app.ui.help else { return };
    let mut open = true;
    egui::Window::new(tl("NetLabCraft Help")).open(&mut open).default_width(520.0).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for p in Page::ALL {
                if ui.selectable_label(page == p, tl(p.title())).clicked() {
                    page = p;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| match page {
            Page::GettingStarted => getting_started(ui),
            Page::Contents => contents(ui, &mut page),
            Page::Tutorials => tutorials(ui),
            Page::Accessibility => accessibility(ui),
            Page::Shortcuts => crate::shortcuts::page(ui),
        });
    });
    app.ui.help = open.then_some(page);
}

fn getting_started(ui: &mut egui::Ui) {
    ui.heading(tl("Getting Started"));
    ui.label(tl("1. Pick a device in the box at the bottom, then click the workspace to place it."));
    ui.label(tl("2. Pick a cable in the same box, click one device, then another, and choose the ports."));
    ui.label(tl("3. Click a device to open its window: routers and switches have a CLI, computers a Desktop."));
    ui.label(tl("4. Test the network with Add Simple PDU in the Tools menu: click a source, then a destination."));
    ui.label(tl("5. Switch to Simulation mode to step through every frame in the event list."));
    ui.label(tl("6. Save with Ctrl+S. New from sample in the File menu opens a ready-made lab to explore."));
}

fn contents(ui: &mut egui::Ui, page: &mut Page) {
    ui.heading(tl("Contents"));
    for p in [Page::GettingStarted, Page::Tutorials, Page::Accessibility, Page::Shortcuts] {
        if ui.link(tl(p.title())).clicked() {
            *page = p;
        }
    }
    ui.add_space(6.0);
    ui.strong(tl("Documentation online"));
    doc_link(ui, "Command reference", "command-reference.md");
    doc_link(ui, "Command line", "cli.md");
    doc_link(ui, "Agent control (MCP)", "mcp.md");
    doc_link(ui, "Lab file format", "file-format.md");
}

fn tutorials(ui: &mut egui::Ui) {
    ui.heading(tl("Tutorials"));
    ui.label(tl("The File menu's New from sample opens labs made to be explored: VLANs, routing, wireless and more."));
    ui.label(tl("The lab book has step-by-step exercises, from first device setup to routing protocols."));
    ui.label(tl("Each lab lists the devices to place, the commands to type and what you should see."));
    ui.hyperlink_to(tl("Open the lab book"), LABS_URL);
}

fn accessibility(ui: &mut egui::Ui) {
    ui.heading(tl("Accessibility"));
    ui.label(tl("Every device on the workspace is announced by a screen reader with its name and model."));
    ui.label(tl("Tab and the arrow keys move between devices and cables; Enter or Space opens the one in focus."));
    ui.label(tl("The Workspace List in the View menu names every device in a table you can move through."));
    ui.label(tl("Everything the menus do can also be typed, from the command line or by an agent."));
    ui.hyperlink_to(tl("Command line"), format!("{DOCS_URL}/cli.md"));
}
