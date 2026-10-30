//! The cloud's port pairs in its Physical tab: which port each frame leaves by, a Part button
//! per pair and two port pickers to join two more. Every change is `cloud.map`.

use egui::RichText;
use netlabcraft_model::Device;
use serde_json::json;

use crate::PacketApp;
use crate::i18n::{tl, trf};

pub(crate) fn panel(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = crate::theme::tokens(app.ui.dark);
    ui.label(RichText::new(tl("Port pairs (a frame in one port leaves by the other):")).strong());
    let mut part = None;
    for (a, b) in &dev.bridges {
        ui.horizontal(|ui| {
            ui.label(trf("{} joined to {}", &[a, b]));
            let what = trf("Part {} from {}", &[a, b]);
            let r = ui.button(tl("Part"));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, what.clone()));
            if r.on_hover_text(what.clone()).clicked() {
                part = Some((a.clone(), b.clone()));
            }
        });
    }
    if dev.bridges.is_empty() {
        ui.label(RichText::new(tl("No port is joined: the cloud passes nothing.")).color(tk.muted));
    }
    if let Some((a, b)) = part {
        app.run("cloud.map", json!({"device": dev.id.0, "a": a, "b": b, "remove": true}));
    }
    let names: Vec<String> = dev.ports.iter().map(|p| p.name.clone()).collect();
    let id = ui.make_persistent_id(("cloud-join", dev.id.0));
    let (mut a, mut b): (usize, usize) = ui.data(|d| d.get_temp(id)).unwrap_or((0, 1));
    ui.horizontal(|ui| {
        for (which, pick, label) in [(0, &mut a, tl("First port")), (1, &mut b, tl("Second port"))] {
            egui::ComboBox::from_id_salt(("cloud-port", dev.id.0, which))
                .selected_text(names.get(*pick).cloned().unwrap_or_default())
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(pick, i, n);
                    }
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::ComboBox, true, label));
        }
        if ui.button(tl("Join")).clicked()
            && let (Some(x), Some(y)) = (names.get(a), names.get(b))
        {
            app.run("cloud.map", json!({"device": dev.id.0, "a": x, "b": y}));
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (a, b)));
}
