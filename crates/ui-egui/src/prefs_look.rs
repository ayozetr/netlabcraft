//! Preferences ▸ Font and what it changes: the interface text size, the command-line font, the
//! tooltip text, and the named colours of the consoles and tooltips.
//!
//! Sizes are shown in points. One point is 1.5 of egui's size units and the default of every
//! size is 8 pt, so the interface slider reads 8 pt at 100% (the range 70 to 160% is 5.6 to
//! 12.8 pt). The file keeps the interface size as the percentage it always was.
//!
//! The tooltip choices cannot be set once for egui's popups without changing every window, so
//! tooltips written through [`HintExt::hint`] (all the ones this program writes) read them.

use std::cell::Cell;

use egui::{Color32, FontFamily, FontId, RichText};

use crate::i18n::tl;
use crate::prefs::Prefs;
use crate::{PacketApp, theme};

/// One point in egui's size units.
pub const UNIT_PER_PT: f32 = 1.5;
/// The default of every size, in points.
pub const DEFAULT_PT: u16 = 8;
/// Smallest and largest command-line and tooltip size, in points.
pub const TEXT_PT: std::ops::RangeInclusive<u16> = 6..=24;
/// Interface text size in percent per point: 8 pt is 100%.
const PERCENT_PER_PT: f32 = 12.5;
/// Smallest contrast between a text colour and its background that Apply accepts.
const MIN_CONTRAST: f32 = 2.0;

/// The interface size in points for a percentage.
pub fn percent_to_pt(percent: u16) -> f32 {
    f32::from(percent) / PERCENT_PER_PT
}

/// The percentage for a size in points (rounded).
pub fn pt_to_percent(pt: f32) -> u16 {
    (pt * PERCENT_PER_PT).round().clamp(0.0, 1000.0) as u16
}

/// The family of the command-line text: the bundled monospace face, or the interface's face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CliFont {
    #[default]
    Monospace,
    Proportional,
}

impl CliFont {
    pub const ALL: [CliFont; 2] = [CliFont::Monospace, CliFont::Proportional];

    pub fn key(self) -> &'static str {
        match self {
            CliFont::Monospace => "monospace",
            CliFont::Proportional => "proportional",
        }
    }

    pub fn from_key(key: &str) -> Option<CliFont> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            CliFont::Monospace => "Monospace (Hack)",
            CliFont::Proportional => "Proportional (as the interface)",
        }
    }
}

/// A colour chosen by name. `Default` keeps the theme's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NamedColor {
    #[default]
    Default,
    Black,
    White,
    Green,
    Amber,
    Blue,
    Red,
    Grey,
    Yellow,
    Cyan,
    Magenta,
}

impl NamedColor {
    pub const ALL: [NamedColor; 11] = [
        NamedColor::Default,
        NamedColor::Black,
        NamedColor::White,
        NamedColor::Green,
        NamedColor::Amber,
        NamedColor::Blue,
        NamedColor::Red,
        NamedColor::Grey,
        NamedColor::Yellow,
        NamedColor::Cyan,
        NamedColor::Magenta,
    ];

    pub fn key(self) -> &'static str {
        match self {
            NamedColor::Default => "theme",
            NamedColor::Black => "black",
            NamedColor::White => "white",
            NamedColor::Green => "green",
            NamedColor::Amber => "amber",
            NamedColor::Blue => "blue",
            NamedColor::Red => "red",
            NamedColor::Grey => "grey",
            NamedColor::Yellow => "yellow",
            NamedColor::Cyan => "cyan",
            NamedColor::Magenta => "magenta",
        }
    }

    pub fn from_key(key: &str) -> Option<NamedColor> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    /// The name the interface shows.
    pub fn label(self) -> &'static str {
        match self {
            NamedColor::Default => "Default",
            NamedColor::Black => "Black",
            NamedColor::White => "White",
            NamedColor::Green => "Green",
            NamedColor::Amber => "Amber",
            NamedColor::Blue => "Blue",
            NamedColor::Red => "Red",
            NamedColor::Grey => "Grey",
            NamedColor::Yellow => "Yellow",
            NamedColor::Cyan => "Cyan",
            NamedColor::Magenta => "Magenta",
        }
    }

    /// The colour, or `None` for `Default`.
    pub fn rgb(self) -> Option<Color32> {
        let c = |r, g, b| Some(Color32::from_rgb(r, g, b));
        match self {
            NamedColor::Default => None,
            NamedColor::Black => c(0x00, 0x00, 0x00),
            NamedColor::White => c(0xff, 0xff, 0xff),
            NamedColor::Green => c(0x33, 0xcc, 0x55),
            NamedColor::Amber => c(0xff, 0xb0, 0x00),
            NamedColor::Blue => c(0x1f, 0x4f, 0xd8),
            NamedColor::Red => c(0xd8, 0x30, 0x30),
            NamedColor::Grey => c(0x80, 0x80, 0x80),
            NamedColor::Yellow => c(0xf5, 0xe0, 0x40),
            NamedColor::Cyan => c(0x20, 0xc8, 0xd8),
            NamedColor::Magenta => c(0xd0, 0x40, 0xc8),
        }
    }

    /// This colour, or `fallback` when it is `Default`.
    pub fn or(self, fallback: Color32) -> Color32 {
        self.rgb().unwrap_or(fallback)
    }
}

/// The six colours of the Colors table: router console, PC console and tooltip, each a text
/// colour and a background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Colors {
    pub ios_text: NamedColor,
    pub ios_bg: NamedColor,
    pub pc_text: NamedColor,
    pub pc_bg: NamedColor,
    pub tip_text: NamedColor,
    pub tip_bg: NamedColor,
}

impl Colors {
    /// The rows of the table: (saved key, label, colour).
    pub fn slots(&mut self) -> [(&'static str, &'static str, &mut NamedColor); 6] {
        [
            ("color-ios-text", "Router console text", &mut self.ios_text),
            ("color-ios-bg", "Router console background", &mut self.ios_bg),
            ("color-pc-text", "PC console text", &mut self.pc_text),
            ("color-pc-bg", "PC console background", &mut self.pc_bg),
            ("color-tip-text", "Tooltip text", &mut self.tip_text),
            ("color-tip-bg", "Tooltip background", &mut self.tip_bg),
        ]
    }

    /// Which pairs (text, background) are too close in colour to read, as the table's labels.
    pub fn unreadable(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (name, text, bg) in
            [("Router console", self.ios_text, self.ios_bg), ("PC console", self.pc_text, self.pc_bg), ("Tooltip", self.tip_text, self.tip_bg)]
        {
            if let (Some(t), Some(b)) = (text.rgb(), bg.rgb())
                && theme::contrast(t, b) < MIN_CONTRAST
            {
                out.push(name);
            }
        }
        out
    }
}

/// The font of the command line: its family at its size.
pub fn cli_font(prefs: &Prefs) -> FontId {
    let family = match prefs.cli_font {
        CliFont::Monospace => FontFamily::Monospace,
        CliFont::Proportional => FontFamily::Proportional,
    };
    FontId::new(f32::from(prefs.cli_pt) * UNIT_PER_PT, family)
}

/// What a tooltip looks like; `None` keeps egui's.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TipStyle {
    size: Option<f32>,
    text: Option<Color32>,
    bg: Option<Color32>,
}

thread_local! {
    static TIP: Cell<TipStyle> = const { Cell::new(TipStyle { size: None, text: None, bg: None }) };
}

/// Tells the tooltips what the settings ask for (each frame, before anything is drawn).
pub fn set_tip_style(prefs: &Prefs) {
    let size = (prefs.tooltip_pt != DEFAULT_PT).then(|| f32::from(prefs.tooltip_pt) * UNIT_PER_PT);
    TIP.with(|t| t.set(TipStyle { size, text: prefs.colors.tip_text.rgb(), bg: prefs.colors.tip_bg.rgb() }));
}

/// The tooltip style now in force: size, text colour, background (for the tests).
#[cfg(test)]
pub(crate) fn current_tip() -> (Option<f32>, Option<Color32>, Option<Color32>) {
    let t = TIP.with(Cell::get);
    (t.size, t.text, t.bg)
}

/// A tooltip's text, in the size and colours chosen. The background, when chosen, fills a box
/// inside the popup; text left at the default is made readable over it.
fn tip_label(ui: &mut egui::Ui, text: &str) {
    let style = TIP.with(Cell::get);
    let mut rich = RichText::new(text);
    if let Some(size) = style.size {
        rich = rich.size(size);
    }
    match (style.text, style.bg) {
        (Some(color), _) => rich = rich.color(color),
        (None, Some(bg)) => rich = rich.color(theme::readable_on(bg)),
        (None, None) => {}
    }
    match style.bg {
        Some(bg) => {
            egui::Frame::new().fill(bg).inner_margin(4.0).corner_radius(3.0).show(ui, |ui| {
                ui.label(rich);
            });
        }
        None => {
            ui.label(rich);
        }
    }
}

/// `on_hover_text` and `on_disabled_hover_text` that read the tooltip settings.
pub trait HintExt {
    fn hint(self, text: impl Into<egui::WidgetText>) -> egui::Response;
    fn disabled_hint(self, text: impl Into<egui::WidgetText>) -> egui::Response;
}

impl HintExt for egui::Response {
    fn hint(self, text: impl Into<egui::WidgetText>) -> egui::Response {
        let text: egui::WidgetText = text.into();
        let text = text.text().to_string();
        self.on_hover_ui(move |ui| tip_label(ui, &text))
    }

    fn disabled_hint(self, text: impl Into<egui::WidgetText>) -> egui::Response {
        let text: egui::WidgetText = text.into();
        let text = text.text().to_string();
        self.on_disabled_hover_ui(move |ui| tip_label(ui, &text))
    }
}

/// Preferences ▸ Font.
pub(crate) fn font_tab(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Interface text")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            let l = ui.label(tl("Application size:"));
            let mut pt = percent_to_pt(app.ui.prefs.font_percent);
            if ui.add(egui::Slider::new(&mut pt, 5.6..=12.8).step_by(0.1).fixed_decimals(1).suffix(" pt")).labelled_by(l.id).changed() {
                app.ui.prefs.font_percent = pt_to_percent(pt).clamp(*crate::prefs::FONT_RANGE.start(), *crate::prefs::FONT_RANGE.end());
            }
            if ui.button(tl("Reset")).clicked() {
                app.ui.prefs.font_percent = 100;
            }
        });
        ui.label(
            RichText::new(tl(
                "8 pt is the default (100%). Sets the text of menus, buttons and forms. Names drawn on the workspace follow its zoom instead.",
            ))
            .small(),
        );
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Command line")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let p = &mut app.ui.prefs;
        ui.horizontal(|ui| {
            let l = ui.label(tl("CLI font:"));
            egui::ComboBox::from_id_salt("cli_font").selected_text(tl(p.cli_font.label())).show_ui(ui, |ui| {
                for f in CliFont::ALL {
                    ui.selectable_value(&mut p.cli_font, f, tl(f.label()));
                }
            });
            let _ = l;
        });
        ui.horizontal(|ui| {
            let l = ui.label(tl("CLI font size:"));
            ui.add(egui::DragValue::new(&mut p.cli_pt).range(TEXT_PT).suffix(" pt")).labelled_by(l.id);
        });
        ui.label(
            RichText::new(tl(
                "Used by the CLI tab, the Command Prompt and the Terminal. These are the faces the program carries: it adds no others.",
            ))
            .small(),
        );
        ui.label(RichText::new(tl("Sample: Router#show ip route")).font(cli_font(p)));
    });

    ui.add_space(6.0);
    ui.label(RichText::new(tl("Tooltips")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let p = &mut app.ui.prefs;
        ui.horizontal(|ui| {
            let l = ui.label(tl("Tooltip font size:"));
            ui.add(egui::DragValue::new(&mut p.tooltip_pt).range(TEXT_PT).suffix(" pt")).labelled_by(l.id);
        });
    });

    ui.add_space(6.0);
    colors_table(app, ui);
}

/// The Colors table with its Apply and Reset buttons: the choices are kept as a draft until Apply.
fn colors_table(app: &mut PacketApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tl("Colors")).strong());
    let mut draft = app.ui.prefs_ui.colors_draft.unwrap_or(app.ui.prefs.colors);
    let (mut apply, mut reset) = (false, false);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::Grid::new("colors_table").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            for (key, label, slot) in draft.slots() {
                let l = ui.label(tl(label));
                egui::ComboBox::from_id_salt(key).selected_text(tl(slot.label())).show_ui(ui, |ui| {
                    for c in NamedColor::ALL {
                        ui.selectable_value(slot, c, tl(c.label()));
                    }
                });
                let _ = l;
                ui.end_row();
            }
        });
        let bad = draft.unreadable();
        for name in &bad {
            ui.colored_label(
                theme::tokens(app.ui.dark).error,
                crate::i18n::trf("{}: the text and the background are too close in colour to read.", &[&tl(name)]),
            );
        }
        ui.horizontal(|ui| {
            apply = ui.add_enabled(bad.is_empty() && draft != app.ui.prefs.colors, egui::Button::new(tl("Apply"))).clicked();
            reset = ui.button(tl("Reset Colors")).clicked();
        });
    });
    if apply {
        app.ui.prefs.colors = draft;
        app.ui.prefs_ui.colors_draft = None;
    } else if reset {
        app.ui.prefs.colors = Colors::default();
        app.ui.prefs_ui.colors_draft = None;
    } else {
        app.ui.prefs_ui.colors_draft = (draft != app.ui.prefs.colors).then_some(draft);
    }
}

/// The device-window tabs a device shows once the Show/Hide choices are applied. Never empty: if
/// every tab were hidden, the device keeps its tabs.
pub(crate) fn visible_tabs<'a>(
    prefs: &Prefs,
    dev: &netlabcraft_model::Device,
    tabs: &[(crate::console::Tab, &'a str)],
) -> Vec<(crate::console::Tab, &'a str)> {
    use crate::console::Tab;
    use netlabcraft_model::DeviceKind;
    let router_or_switch = matches!(dev.kind, DeviceKind::Router | DeviceKind::Switch);
    let hidden = |tab: Tab| match tab {
        Tab::Physical => prefs.hide_physical_tab,
        Tab::Config => prefs.hide_config_tab || (router_or_switch && prefs.hide_rs_config_tab),
        Tab::Cli => prefs.hide_cli_tab || (router_or_switch && prefs.hide_rs_cli_tab),
        Tab::Services => prefs.hide_services_tab,
        Tab::Desktop => prefs.hide_desktop_tab,
        Tab::Phone => prefs.hide_gui_tab,
        Tab::Iot => prefs.hide_gui_tab && !netlabcraft_model::iot::can_serve(dev),
        Tab::Attributes => prefs.hide_attributes_tab,
        Tab::Board | Tab::Capture => false,
    };
    let shown: Vec<_> = tabs.iter().copied().filter(|(t, _)| !hidden(*t)).collect();
    if shown.is_empty() { tabs.to_vec() } else { shown }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_mapping_is_8_pt_at_100_percent_and_covers_the_old_range() {
        assert_eq!(pt_to_percent(8.0), 100);
        assert!((percent_to_pt(100) - 8.0).abs() < 1e-6);
        assert_eq!((pt_to_percent(5.6), pt_to_percent(12.8)), (70, 160));
        for pct in 70..=160u16 {
            assert_eq!(pt_to_percent(percent_to_pt(pct)), pct, "{pct}");
        }
    }

    #[test]
    fn colour_and_font_names_round_trip() {
        for c in NamedColor::ALL {
            assert_eq!(NamedColor::from_key(c.key()), Some(c));
        }
        for f in CliFont::ALL {
            assert_eq!(CliFont::from_key(f.key()), Some(f));
        }
        assert_eq!(NamedColor::from_key("chartreuse"), None);
        assert_eq!(NamedColor::Default.rgb(), None);
        assert!(NamedColor::ALL.iter().skip(1).all(|c| c.rgb().is_some()));
    }

    #[test]
    fn a_text_colour_too_close_to_its_background_is_refused() {
        let mut c = Colors { ios_text: NamedColor::Green, ios_bg: NamedColor::Black, ..Colors::default() };
        assert!(c.unreadable().is_empty());
        c.pc_text = NamedColor::Red;
        c.pc_bg = NamedColor::Red;
        c.tip_text = NamedColor::Yellow;
        c.tip_bg = NamedColor::White;
        assert_eq!(c.unreadable(), ["PC console", "Tooltip"]);
        // A colour left at the default is never compared.
        c.pc_bg = NamedColor::Default;
        assert_eq!(c.unreadable(), ["Tooltip"]);
    }
}
