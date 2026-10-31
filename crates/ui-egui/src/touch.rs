//! Tablets and smartphones: their touch home screen (rounded app icons on the device's own
//! screen, drawn in code) and their Physical tab (the device seen from the front, with its
//! Wi-Fi radio). The apps are the Desktop's, reached the same way; every change is an engine
//! command.

use egui::{Color32, RichText, Sense, vec2};
use netlabcraft_model::{Device, End};

use crate::console::DesktopApp;
use crate::i18n::{tl, trf};
use crate::{PacketApp, theme};

/// The apps a touch device has, in the order its home screen shows them.
pub(crate) const APPS: [(DesktopApp, &str); 5] = [
    (DesktopApp::Browser, "Web Browser"),
    (DesktopApp::Email, "Email"),
    (DesktopApp::IpConfig, "IP Configuration"),
    (DesktopApp::Prompt, "Command Prompt"),
    (DesktopApp::Programming, "Programming"),
];

/// Whether a device is a tablet (else a smartphone): the tablet lies wide, the phone stands.
fn wide(dev: &Device) -> bool {
    dev.catalogue_model().id == "Tablet"
}

/// The home screen: the device body, its screen with a status bar and the app icons. Returns
/// the app tapped.
pub(crate) fn home(app: &PacketApp, ui: &mut egui::Ui, dev: &Device) -> Option<DesktopApp> {
    let tk = theme::tokens(app.ui.dark);
    let size = if wide(dev) { vec2(560.0, 380.0) } else { vec2(300.0, 460.0) };
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(outer);
    let body = Color32::from_rgb(0x26, 0x2b, 0x33);
    p.rect_filled(outer, 26.0, body);
    p.rect_stroke(outer, 26.0, egui::Stroke::new(2.0, Color32::from_rgb(0x55, 0x5d, 0x68)), egui::StrokeKind::Inside);
    let screen = outer.shrink2(if wide(dev) { vec2(34.0, 22.0) } else { vec2(14.0, 40.0) });
    p.rect_filled(screen, 10.0, tk.bar);
    // The status bar: the network it is on and its address.
    let wifi = wifi_line(app, dev);
    let status = p.layout(wifi, egui::FontId::proportional(12.0), tk.bar_text, screen.width() - 60.0);
    p.galley(screen.left_top() + vec2(12.0, 10.0), status, tk.bar_text);
    signal_bars(&p, screen.right_top() + vec2(-30.0, 10.0), associated(app, dev), &tk);
    let mut tapped = None;
    let cols = if wide(dev) { 5 } else { 3 };
    let cell = vec2((screen.width() - 24.0) / cols as f32, 110.0);
    for (i, (id, label)) in APPS.iter().enumerate() {
        let (row, col) = (i / cols, i % cols);
        let at = screen.left_top() + vec2(12.0 + col as f32 * cell.x, 50.0 + row as f32 * cell.y);
        let rect = egui::Rect::from_min_size(at, cell);
        let resp = ui.interact(rect, ui.scope_id().with(("touch_app", dev.id.0, i)), Sense::click());
        let name = tl(label);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, name));
        let icon = egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 38.0), vec2(64.0, 64.0));
        let teal = Color32::from_rgb(0x58, 0xbf, 0xd2);
        p.rect_filled(icon, 16.0, if resp.hovered() { theme::shade(teal, 1.15) } else { teal });
        crate::console::draw_app_icon(&p, icon.shrink(12.0), *id);
        let galley = p.layout(name.to_string(), egui::FontId::proportional(if wide(dev) { 12.5 } else { 11.5 }), Color32::WHITE, cell.x - 2.0);
        let pos = egui::Align2::CENTER_TOP.anchor_size(icon.center_bottom() + vec2(0.0, 6.0), galley.size());
        p.galley(pos.min, galley, Color32::WHITE);
        if resp.clicked() {
            tapped = Some(*id);
        }
    }
    // The home button (phones) or the camera dot (tablets).
    if wide(dev) {
        p.circle_filled(egui::pos2(outer.left() + 17.0, outer.center().y), 4.0, Color32::from_rgb(0x44, 0x4b, 0x55));
    } else {
        p.circle_stroke(egui::pos2(outer.center().x, outer.bottom() - 20.0), 11.0, egui::Stroke::new(2.0, Color32::from_rgb(0x55, 0x5d, 0x68)));
        p.circle_filled(egui::pos2(outer.center().x, outer.top() + 20.0), 3.5, Color32::from_rgb(0x44, 0x4b, 0x55));
    }
    tapped
}

/// Whether the radio is associated with an access point.
fn associated(app: &PacketApp, dev: &Device) -> bool {
    app.session.topology.wifi_ap_of(End { device: dev.id, port: 0 }).is_some()
}

/// The status bar's text: the network and the address.
fn wifi_line(app: &PacketApp, dev: &Device) -> String {
    let ssid = dev.ports.first().and_then(|p| p.wifi.as_ref()).map(|w| w.ssid.clone()).filter(|s| !s.is_empty());
    let addr = dev.ports.first().and_then(|p| p.ip).map(|n| n.addr.to_string());
    match (ssid, associated(app, dev), addr) {
        (Some(s), true, Some(a)) => trf("Wi-Fi {} · {}", &[&s, &a]),
        (Some(s), true, None) => trf("Wi-Fi {} · no address yet", &[&s]),
        (Some(s), false, _) => trf("Looking for {}", &[&s]),
        (None, _, _) => tl("Wi-Fi off: pick a network in Config").to_string(),
    }
}

/// Four rising bars, lit when associated.
fn signal_bars(p: &egui::Painter, at: egui::Pos2, on: bool, tk: &theme::Tokens) {
    for k in 0..4 {
        let h = 3.0 + k as f32 * 3.0;
        let r = egui::Rect::from_min_max(at + vec2(k as f32 * 5.0, 12.0 - h), at + vec2(k as f32 * 5.0 + 3.0, 12.0));
        p.rect_filled(r, 0.5, if on { tk.bar_text } else { tk.muted });
    }
}

/// The Physical tab: the device from the front, its radio and what it is associated with.
pub(crate) fn physical(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    crate::hardware::panel(app, ui, dev);
    ui.label(RichText::new(trf("{}: Wi-Fi radio (Wireless0)", &[&tl(dev.catalogue_model().label)])).strong());
    let size = if wide(dev) { vec2(300.0, 200.0) } else { vec2(130.0, 240.0) };
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        let picture = if wide(dev) { tl("A tablet seen from the front") } else { tl("A smartphone seen from the front") };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, picture));
        let p = ui.painter_at(rect);
        let body = Color32::from_rgb(0x26, 0x2b, 0x33);
        p.rect_filled(rect, 18.0, body);
        let screen = rect.shrink2(if wide(dev) { vec2(22.0, 14.0) } else { vec2(10.0, 26.0) });
        let lit = !dev.powered_off;
        p.rect_filled(screen, 6.0, if lit { tk.bar } else { Color32::from_rgb(0x10, 0x12, 0x16) });
        if lit {
            for (i, _) in APPS.iter().enumerate() {
                let (cols, s) = if wide(dev) { (5, 30.0) } else { (3, 24.0) };
                let at = screen.left_top() + vec2(12.0 + (i % cols) as f32 * (s + 10.0), 14.0 + (i / cols) as f32 * (s + 12.0));
                p.rect_filled(egui::Rect::from_min_size(at, vec2(s, s)), 7.0, Color32::from_rgb(0x58, 0xbf, 0xd2));
            }
        }
        // The radio: an antenna mark with its waves, green while associated.
        let on = lit && associated(app, dev);
        let c = rect.right_top() + vec2(-16.0, 12.0);
        let col = if on { tk.link_up } else { tk.muted };
        for r in [4.0f32, 8.0, 12.0] {
            let pts: Vec<egui::Pos2> = (0..=8)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 - 0.7 + i as f32 * 1.4 / 8.0;
                    c + vec2(a.cos(), a.sin()) * r + vec2(0.0, 8.0)
                })
                .collect();
            p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, col)));
        }
        ui.vertical(|ui| {
            let end = End { device: dev.id, port: 0 };
            let ap = app.session.topology.wifi_ap_of(end).and_then(|e| app.session.topology.device(e.device)).map(|d| d.name.clone());
            match ap {
                Some(name) => ui.label(trf("Associated with {}", &[&name])),
                None => ui.label(RichText::new(tl("Not associated with any access point")).color(tk.muted)),
            };
            ui.label(RichText::new(tl("Always charged here: there is no battery to look after.")).color(tk.muted));
            ui.label(RichText::new(tl("Set its Wi-Fi network in Config > Wireless0; move it near an access point to join.")).color(tk.muted));
        });
    });
}
