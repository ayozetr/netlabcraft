//! The smaller Preferences tabs: Show/Hide, Answer Tree, Custom Interfaces, Publishers and
//! Image Cleanup. (Font, Miscellaneous and Administrative have their own files.)

use egui::RichText;

use crate::i18n::tl;
use crate::prefs::pref;
use crate::prefs_look::HintExt;
use crate::{PacketApp, theme};

/// Which items of the Assessment Tree the instructor sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnswerFilter {
    /// Every item.
    #[default]
    ShowAll,
    /// Configuration and address items: what most labs grade (cables are left out).
    Default,
    Router,
    Switch,
    EndDevice,
}

impl AnswerFilter {
    pub const ALL: [AnswerFilter; 5] =
        [AnswerFilter::ShowAll, AnswerFilter::Default, AnswerFilter::Router, AnswerFilter::Switch, AnswerFilter::EndDevice];

    pub fn key(self) -> &'static str {
        match self {
            AnswerFilter::ShowAll => "all",
            AnswerFilter::Default => "standard",
            AnswerFilter::Router => "router",
            AnswerFilter::Switch => "switch",
            AnswerFilter::EndDevice => "end",
        }
    }

    pub fn from_key(key: &str) -> Option<AnswerFilter> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            AnswerFilter::ShowAll => "Show All",
            AnswerFilter::Default => "Default",
            AnswerFilter::Router => "Router",
            AnswerFilter::Switch => "Switch",
            AnswerFilter::EndDevice => "End Device",
        }
    }

    /// Whether a tree item shows: `device_kind` is `router`, `switch`, `end` or `other`, `kind`
    /// is `config`, `address` or `cable` (as `activity.tree` reports them).
    pub fn shows(self, device_kind: &str, kind: &str) -> bool {
        match self {
            AnswerFilter::ShowAll => true,
            AnswerFilter::Default => kind != "cable",
            AnswerFilter::Router => device_kind == "router",
            AnswerFilter::Switch => device_kind == "switch",
            AnswerFilter::EndDevice => device_kind == "end",
        }
    }
}

pub(crate) fn show_hide_tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Toolbars")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let tb = &mut app.ui.toolbars;
        pref(ui, &mut tb.main, "Main", None);
        pref(ui, &mut tb.secondary, "Secondary", None);
        pref(ui, &mut tb.bottom, "Bottom", Some("The device box with the models and the PDU list."));
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Device window tabs")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let p = &mut app.ui.prefs;
        pref(ui, &mut p.hide_physical_tab, "Hide the Physical tab", None);
        pref(ui, &mut p.hide_config_tab, "Hide the Config tab of every device", None);
        pref(ui, &mut p.hide_cli_tab, "Hide the CLI tab of every device", None);
        pref(ui, &mut p.hide_rs_config_tab, "Hide the Config tab of routers and switches", None);
        pref(ui, &mut p.hide_rs_cli_tab, "Hide the CLI tab of routers and switches", None);
        pref(ui, &mut p.hide_services_tab, "Hide the Services tab", None);
        pref(ui, &mut p.hide_desktop_tab, "Hide the Desktop tab", None);
        pref(ui, &mut p.hide_gui_tab, "Hide the screen tabs of phones and smart things", Some("The Phone tab and the IoT tab of a smart thing."));
        pref(ui, &mut p.hide_attributes_tab, "Hide the Attributes tab", None);
        ui.label(RichText::new(tl("A window never loses all its tabs: if every one were hidden it keeps them.")).small());
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Wireless and other things")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let p = &mut app.ui.prefs;
        pref(ui, &mut p.hide_wireless_cell, "Hide wireless and cellular pages", Some("The Wi-Fi and cellular pages and ports in the Config tab."));
        let mut legacy = false;
        let r = ui.add_enabled(false, egui::Checkbox::new(&mut legacy, tl("Hide legacy equipment")));
        let _ = r.disabled_hint(tl("NetLabCraft has no legacy equipment to hide."));
        pref(
            ui,
            &mut p.wireless_boundary,
            "Show the wireless boundary",
            Some("View Wireless Signals draws an outline around each radio's coverage."),
        );
        pref(
            ui,
            &mut p.wireless_gradient,
            "Fade the wireless colour toward the edge",
            Some("View Wireless Signals fills each coverage area with a colour that fades outwards; unchecked, a flat disc."),
        );
        pref(ui, &mut p.hide_coverage_spin, "Hide the coverage range field of Wi-Fi forms", None);
        pref(
            ui,
            &mut p.advanced_things,
            "Open smart things with their advanced settings shown",
            Some("The IoT tab of a smart thing opens its Advanced section."),
        );
    });
}

pub(crate) fn answer_tree_tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Answer Tree Filter")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for f in AnswerFilter::ALL {
                if ui.selectable_label(app.ui.prefs.answer_filter == f, tl(f.label())).clicked() {
                    app.ui.prefs.answer_filter = f;
                }
            }
        });
        let words = match app.ui.prefs.answer_filter {
            AnswerFilter::ShowAll => "Every item of the Assessment Tree.",
            AnswerFilter::Default => "Configuration and address items; the cables are left out.",
            AnswerFilter::Router => "Only the items about routers.",
            AnswerFilter::Switch => "Only the items about switches.",
            AnswerFilter::EndDevice => "Only the items about end devices (PCs, servers, phones and the like).",
        };
        ui.label(tl(words));
        ui.label(
            RichText::new(tl("The filter applies to the Assessment Tree window of the Activity Wizard. What is hidden is still graded.")).small(),
        );
    });
}

pub(crate) fn custom_interfaces_tab(ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Custom Interfaces")).strong());
    ui.label(tl("Not available: NetLabCraft has no scripted device pages, so a device cannot be given a custom page."));
}

pub(crate) fn publishers_tab(ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Publishers")).strong());
    ui.label(tl("Not applicable: there are no scripted modules to sign. Activity results carry their own signature, checked by the instructor's Verify Results."));
}

pub(crate) fn image_cleanup_tab(app: &PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Images held by the lab")).strong());
    let tk = theme::tokens(app.ui.dark);
    egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.0, tk.chrome_line)).show(ui, |ui| {
        egui::Grid::new("image_cleanup").num_columns(4).spacing([24.0, 4.0]).show(ui, |ui| {
            for h in ["Remove", "Type", "Size", "Image"] {
                ui.label(RichText::new(tl(h)).strong());
            }
            ui.end_row();
        });
        ui.separator();
        ui.label(tl("The lab holds no images: a lab file is only its devices, cables, places and settings."));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_filter_shows_what_it_names() {
        use AnswerFilter::*;
        assert!(ShowAll.shows("other", "cable"));
        assert!(Default.shows("router", "config") && Default.shows("end", "address") && !Default.shows("router", "cable"));
        assert!(Router.shows("router", "cable") && !Router.shows("switch", "config"));
        assert!(Switch.shows("switch", "config") && !Switch.shows("end", "address"));
        assert!(EndDevice.shows("end", "address") && !EndDevice.shows("other", "address"));
        for f in AnswerFilter::ALL {
            assert_eq!(AnswerFilter::from_key(f.key()), Some(f));
        }
    }
}
