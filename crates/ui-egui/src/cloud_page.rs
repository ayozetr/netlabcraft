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
    if dev.catalogue_model().pstn() {
        numbers(app, ui, dev);
    }
}

/// A telephone cloud's numbers: one field per phone port, set with `cloud.number`.
fn numbers(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    ui.separator();
    ui.label(RichText::new(tl("Telephone numbers (one per port):")).strong());
    let id = ui.make_persistent_id(("cloud-numbers", dev.id.0));
    let mut edits: std::collections::BTreeMap<String, String> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let mut set = None;
    egui::Grid::new(("cloud-number-grid", dev.id.0)).num_columns(3).show(ui, |ui| {
        for port in dev.ports.iter().map(|p| p.name.clone()) {
            let saved = dev.phone_numbers.get(&port).cloned().unwrap_or_default();
            let text = edits.entry(port.clone()).or_insert_with(|| saved.clone());
            let label = ui.label(&port);
            ui.add(egui::TextEdit::singleline(text).desired_width(140.0)).labelled_by(label.id);
            if ui.button(tl("Set")).clicked() {
                set = Some((port.clone(), text.trim().to_string()));
            }
            ui.end_row();
        }
    });
    if let Some((port, text)) = set {
        let number = if text.is_empty() { serde_json::Value::Null } else { json!(text) };
        app.run("cloud.number", json!({"device": dev.id.0, "port": port, "number": number}));
    }
    ui.data_mut(|d| d.insert_temp(id, edits));
}
