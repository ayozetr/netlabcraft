//! The IoT tab: a smart thing's reading or state and its server; a server's IoT service, the
//! things registered with it and its rules. Every change is an engine command (`iot.configure`,
//! `iot.sync`, `iot.server`, `iot.switch`).

use egui::{RichText, Sense, vec2};
use netlabcraft_model::Device;
use netlabcraft_model::iot::is_sensor;
use serde_json::{Value, json};

use crate::i18n::{tl, tr_any, trf};
use crate::{PacketApp, theme};

/// The form fields of the IoT tab.
#[derive(Debug, Clone, Default)]
pub(crate) struct IotForm {
    server: String,
    filled: bool,
    rule_sensor: String,
    rule_above: String,
    rule_actuator: String,
}

pub(crate) fn tab(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, form: &mut IotForm) {
    if dev.kind == netlabcraft_model::DeviceKind::Server { server_page(app, ui, dev, form) } else { thing_page(app, ui, dev, form) }
}

fn thing_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, form: &mut IotForm) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    if !form.filled {
        form.server = dev.iot.server.map(|a| a.to_string()).unwrap_or_default();
        form.filled = true;
    }
    let st = app.session.sim.iot_thing(d).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        let l = ui.label(tl("IoT server:"));
        ui.add(egui::TextEdit::singleline(&mut form.server).desired_width(140.0)).labelled_by(l.id);
        if ui.button(tl("Apply")).clicked() {
            app.run("iot.configure", json!({"device": d.0, "server": form.server.trim()}));
        }
    });
    if is_sensor(dev) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(trf("Reading: {}", &[&dev.iot.value])).size(18.0).strong());
            for (label, step) in [("-5", -5), ("-1", -1), ("+1", 1), ("+5", 5)] {
                if ui.button(label).on_hover_text(tl("Change the reading")).clicked() {
                    app.run("iot.configure", json!({"device": d.0, "value": dev.iot.value.saturating_add(step)}));
                }
            }
        });
    } else {
        // The fan: its blades drawn in code, coloured while it runs.
        let (rect, _) = ui.allocate_exact_size(vec2(70.0, 70.0), Sense::hover());
        let p = ui.painter_at(rect);
        let c = rect.center();
        let col = if st.on { tk.link_up } else { tk.chrome_line };
        p.circle_stroke(c, 30.0, egui::Stroke::new(2.0, tk.muted));
        for k in 0..3 {
            let a = k as f32 * std::f32::consts::TAU / 3.0;
            let tip = c + vec2(a.cos(), a.sin()) * 26.0;
            let side = c + vec2((a + 0.6).cos(), (a + 0.6).sin()) * 20.0;
            p.add(egui::Shape::convex_polygon(vec![c, tip, side], col, egui::Stroke::NONE));
        }
        p.circle_filled(c, 5.0, tk.muted);
        ui.label(RichText::new(if st.on { tl("Running (switched on by the server)") } else { tl("Stopped") }).strong());
    }
    match (&st.registered, &st.message) {
        (Some(a), _) => ui.label(trf("Registered with {}", &[a])),
        (None, Some(m)) => ui.label(RichText::new(tr_any(m)).color(tk.error)),
        (None, None) => ui.label(tl("Not registered yet")),
    };
    if ui.button(tl("Report now")).on_hover_text(tl("Every smart thing reports to its server over HTTP")).clicked() {
        app.run("iot.sync", json!({}));
    }
}

fn server_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, form: &mut IotForm) {
    let d = dev.id;
    let mut on = dev.iot.service;
    if ui.checkbox(&mut on, tl("IoT service")).changed() {
        app.run("iot.server", json!({"device": d.0, "service": on}));
    }
    if ui.button(tl("Refresh")).on_hover_text(tl("Let the smart things report, then list them")).clicked() {
        app.run("iot.server", json!({"device": d.0}));
    }
    ui.add_space(4.0);
    ui.label(RichText::new(tl("Registered things")).strong());
    let things = app.session.sim.iot_registry(d).to_vec();
    if things.is_empty() {
        ui.label(tl("None yet: smart things register when they report (Refresh)."));
    }
    let mut switch: Option<(String, bool)> = None;
    egui::Grid::new(("iot-things", d.0)).striped(true).num_columns(4).show(ui, |ui| {
        for t in &things {
            ui.label(&t.name);
            ui.label(t.address.to_string());
            if t.kind == "sensor" {
                ui.label(trf("reading {}", &[&t.value.map_or("-".to_string(), |v| v.to_string())]));
                ui.label("");
            } else {
                ui.label(if t.on { tl("on") } else { tl("off") });
                if ui.button(if t.on { tl("Switch off") } else { tl("Switch on") }).clicked() {
                    switch = Some((t.name.clone(), !t.on));
                }
            }
            ui.end_row();
        }
    });
    if let Some((name, on)) = switch {
        app.run("iot.switch", json!({"device": d.0, "name": name, "on": on}));
    }
    ui.add_space(6.0);
    ui.label(RichText::new(tl("Rules")).strong());
    let rules: Vec<Value> = dev.iot.rules.iter().map(|r| json!({"sensor": r.sensor, "above": r.above, "actuator": r.actuator})).collect();
    let mut remove = None;
    for (i, r) in dev.iot.rules.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(trf("If {} is above {}, switch {} on", &[&r.sensor, &r.above, &r.actuator]));
            if ui.button(tl("Remove")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        let mut left = rules.clone();
        if i < left.len() {
            left.remove(i);
        }
        app.run("iot.server", json!({"device": d.0, "rules": left}));
    }
    ui.horizontal(|ui| {
        let l = ui.label(tl("If"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_sensor).desired_width(90.0).hint_text(tl("sensor"))).labelled_by(l.id);
        let l = ui.label(tl("is above"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_above).desired_width(50.0).hint_text("30")).labelled_by(l.id);
        let l = ui.label(tl("switch on"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_actuator).desired_width(90.0).hint_text(tl("device"))).labelled_by(l.id);
        if ui.button(tl("Add rule")).clicked() {
            let mut all = rules.clone();
            all.push(json!({"sensor": form.rule_sensor.trim(), "above": form.rule_above.trim().parse::<i64>().unwrap_or(0), "actuator": form.rule_actuator.trim()}));
            if app.run("iot.server", json!({"device": d.0, "rules": all})).is_some() {
                *form = IotForm { filled: form.filled, ..IotForm::default() };
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use serde_json::json;

    use crate::PacketApp;

    #[test]
    fn raising_the_reading_runs_the_fan_and_the_server_page_switches_it() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "iot"})).unwrap();
        let (sensor, server) = (s.topology.resolve("Sensor1").unwrap(), s.topology.resolve("IoT-Server").unwrap());
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.show_notes = false;
        crate::open_console(h.state_mut(), sensor);
        h.run_steps(3);
        assert!(h.query_by_label("IoT server:").is_some());
        h.get_by_label("+5").click();
        h.run_steps(2);
        h.get_by_label("+5").click();
        h.run_steps(2);
        assert_eq!(h.state().session.topology.device(sensor).unwrap().iot.value, 35);
        h.state_mut().ui.windows.clear();
        crate::open_console(h.state_mut(), server);
        if let Some(w) = h.state_mut().ui.windows.get_mut(&server) {
            w.tab = crate::console::Tab::Iot;
        }
        h.run_steps(3);
        h.get_by_label("Refresh").click();
        h.run_steps(2);
        let fan = |h: &Harness<'_, PacketApp>| h.state().session.sim.iot_registry(server).iter().find(|e| e.name == "Fan1").map(|e| e.on);
        assert_eq!(fan(&h), Some(true));
        h.get_by_label("Switch off").click();
        h.run_steps(2);
        assert_eq!(fan(&h), Some(false));
        assert!(h.query_by_label("If Sensor1 is above 30, switch Fan1 on").is_some());
    }
}
