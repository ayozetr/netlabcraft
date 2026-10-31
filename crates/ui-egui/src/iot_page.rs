//! The IoT tab: a smart thing's reading or state, its server and its report timer; an IoT
//! service's page (a server's, or a home gateway's built-in one): the things registered with
//! it, live, and its rules. Every change is an engine command (`iot.configure`, `iot.sync`,
//! `iot.server`, `iot.switch`).

use egui::{RichText, Sense, vec2};
use netlabcraft_model::Device;
use netlabcraft_model::iot::{can_serve, is_sensor, thing};
use serde_json::{Value, json};

use crate::i18n::{tl, tr_any, trf};
use crate::{PacketApp, theme};

/// The form fields of the IoT tab.
#[derive(Debug, Clone, Default)]
pub(crate) struct IotForm {
    server: String,
    interval: String,
    filled: bool,
    rule_sensor: String,
    rule_op: usize,
    rule_above: String,
    /// A second condition (optional) and how it joins the first.
    rule_or: bool,
    rule_sensor2: String,
    rule_op2: usize,
    rule_value2: String,
    rule_off: bool,
    rule_actuator: String,
}

const OPS: [&str; 3] = [">", "<", "="];

pub(crate) fn tab(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, form: &mut IotForm) {
    if can_serve(dev) { server_page(app, ui, dev, form) } else { thing_page(app, ui, dev, form) }
}

/// The thing drawn in code: a lamp, a lock, a siren… lit while on.
fn draw_state(ui: &mut egui::Ui, what: &str, on: bool, tk: &theme::Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(70.0, 70.0), Sense::hover());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let col = if on { tk.link_up } else { tk.chrome_line };
    let s = egui::Stroke::new(2.0, tk.muted);
    match what {
        "fan" => {
            p.circle_stroke(c, 30.0, s);
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3.0;
                let tip = c + vec2(a.cos(), a.sin()) * 26.0;
                let side = c + vec2((a + 0.6).cos(), (a + 0.6).sin()) * 20.0;
                p.add(egui::Shape::convex_polygon(vec![c, tip, side], col, egui::Stroke::NONE));
            }
            p.circle_filled(c, 5.0, tk.muted);
        }
        "light" => {
            p.circle_filled(c - vec2(0.0, 6.0), 20.0, if on { egui::Color32::from_rgb(0xf6, 0xd3, 0x4a) } else { tk.chrome_line });
            p.circle_stroke(c - vec2(0.0, 6.0), 20.0, s);
            p.rect_filled(egui::Rect::from_center_size(c + vec2(0.0, 22.0), vec2(16.0, 10.0)), 2.0, tk.muted);
        }
        "door" => {
            // A padlock: the shackle is open when unlocked.
            let body = egui::Rect::from_center_size(c + vec2(0.0, 10.0), vec2(36.0, 28.0));
            p.rect_filled(body, 4.0, col);
            p.rect_stroke(body, 4.0, s, egui::StrokeKind::Inside);
            let lift = if on { 10.0 } else { 0.0 };
            let pts: Vec<egui::Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 * std::f32::consts::PI / 12.0;
                    c + vec2(a.cos() * 12.0, a.sin() * 14.0 - 4.0 - lift)
                })
                .collect();
            p.add(egui::Shape::line(pts, egui::Stroke::new(4.0, tk.muted)));
        }
        "siren" => {
            p.add(egui::Shape::convex_polygon(
                vec![c + vec2(-18.0, 20.0), c + vec2(18.0, 20.0), c + vec2(12.0, -14.0), c + vec2(-12.0, -14.0)],
                col,
                s,
            ));
            if on {
                for r in [26.0f32, 32.0] {
                    p.circle_stroke(c - vec2(0.0, 6.0), r, egui::Stroke::new(1.5, egui::Color32::from_rgb(0xd6, 0x45, 0x3a)));
                }
            }
        }
        "heater" | "ac" => {
            // A unit with fins; warm red or cool blue lines while it runs.
            let unit = egui::Rect::from_center_size(c + vec2(0.0, 8.0), vec2(54.0, 30.0));
            p.rect_filled(unit, 4.0, tk.chrome_line);
            p.rect_stroke(unit, 4.0, s, egui::StrokeKind::Inside);
            for k in 0..5 {
                let x = unit.left() + 8.0 + k as f32 * 9.5;
                p.line_segment([egui::pos2(x, unit.top() + 5.0), egui::pos2(x, unit.bottom() - 5.0)], egui::Stroke::new(1.5, tk.muted));
            }
            if on {
                let col = if what == "heater" { egui::Color32::from_rgb(0xd6, 0x45, 0x3a) } else { egui::Color32::from_rgb(0x3a, 0x8f, 0xd6) };
                for k in -1..=1 {
                    let x = c.x + k as f32 * 14.0;
                    let pts: Vec<egui::Pos2> =
                        (0..=8).map(|i| egui::pos2(x + (i as f32 * 1.4).sin() * 3.0, unit.top() - 4.0 - i as f32 * 2.2)).collect();
                    p.add(egui::Shape::line(pts, egui::Stroke::new(2.0, col)));
                }
            }
        }
        "window" | "garage" => {
            // A frame whose pane (or door) slides up when open.
            let frame = egui::Rect::from_center_size(c, vec2(50.0, 54.0));
            p.rect_stroke(frame, 2.0, egui::Stroke::new(3.0, tk.muted), egui::StrokeKind::Inside);
            let shut = if on { 0.3 } else { 1.0 };
            let pane = egui::Rect::from_min_size(frame.min + vec2(3.0, 3.0), vec2(frame.width() - 6.0, (frame.height() - 6.0) * shut));
            p.rect_filled(pane, 1.0, if what == "window" { egui::Color32::from_rgb(0xcf, 0xe8, 0xf7) } else { tk.chrome_line });
            if what == "garage" {
                for k in 1..4 {
                    let y = pane.top() + k as f32 * pane.height() / 4.0;
                    p.line_segment([egui::pos2(pane.left(), y), egui::pos2(pane.right(), y)], egui::Stroke::new(1.0, tk.muted));
                }
            }
        }
        "sprinkler" => {
            p.rect_filled(egui::Rect::from_center_size(c + vec2(0.0, 22.0), vec2(10.0, 20.0)), 2.0, tk.muted);
            if on {
                for k in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
                    let tip = c + vec2(k * 28.0, -20.0 + k.abs() * 10.0);
                    p.line_segment([c + vec2(0.0, 12.0), tip], egui::Stroke::new(1.5, egui::Color32::from_rgb(0x3a, 0x8f, 0xd6)));
                    p.circle_filled(tip, 2.5, egui::Color32::from_rgb(0x3a, 0x8f, 0xd6));
                }
            }
        }
        "plug" => {
            let face = egui::Rect::from_center_size(c, vec2(44.0, 44.0));
            p.rect_filled(face, 10.0, tk.chrome_line);
            p.rect_stroke(face, 10.0, s, egui::StrokeKind::Inside);
            for dx in [-8.0f32, 8.0] {
                p.circle_filled(c + vec2(dx, 0.0), 4.0, tk.muted);
            }
            p.circle_filled(face.right_top() + vec2(-8.0, 8.0), 4.0, if on { tk.link_up } else { tk.muted });
        }
        _ => {
            // A camera: body and lens.
            p.rect_filled(egui::Rect::from_center_size(c, vec2(44.0, 30.0)), 5.0, col);
            p.circle_filled(c, 10.0, tk.muted);
            p.circle_filled(c, 5.0, if on { egui::Color32::from_rgb(0xd6, 0x45, 0x3a) } else { tk.chrome });
        }
    }
}

fn thing_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, form: &mut IotForm) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    if !form.filled {
        form.server = dev.iot.server.map(|a| a.to_string()).unwrap_or_default();
        form.interval = dev.iot.interval.to_string();
        form.filled = true;
    }
    let st = app.session.sim.iot_thing(d).cloned().unwrap_or_default();
    let what = thing(dev);
    ui.horizontal(|ui| {
        let l = ui.label(tl("IoT server:"));
        ui.add(egui::TextEdit::singleline(&mut form.server).desired_width(140.0)).labelled_by(l.id);
        let l = ui.label(tl("Report every (s):"));
        ui.add(egui::TextEdit::singleline(&mut form.interval).desired_width(50.0)).labelled_by(l.id);
        if ui.button(tl("Apply")).clicked() {
            let interval = form.interval.trim().parse::<u64>().unwrap_or(0);
            app.run("iot.configure", json!({"device": d.0, "server": form.server.trim(), "interval": interval}));
        }
    });
    if is_sensor(dev) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(trf("Reading: {}", &[&dev.iot.value])).size(18.0).strong());
            if let Some(t) = what {
                ui.label(RichText::new(tl(t.unit)).color(tk.muted));
            }
            for (label, step) in [("-5", -5), ("-1", -1), ("+1", 1), ("+5", 5)] {
                if ui.button(label).on_hover_text(tl("Change the reading")).clicked() {
                    app.run("iot.configure", json!({"device": d.0, "value": dev.iot.value.saturating_add(step)}));
                }
            }
        });
    } else {
        let w = what.map_or("fan", |t| t.what);
        draw_state(ui, w, st.on, &tk);
        let words = what.map_or(("Running (switched on by the server)", "Stopped"), |t| (t.on_word, t.off_word));
        ui.label(RichText::new(tl(if st.on { words.0 } else { words.1 })).strong());
        if let Some(img) = &st.image {
            ui.label(RichText::new(tr_any(img)).monospace());
        }
    }
    if let Some(at) = app.session.sim.wired_to(&app.session.topology, d) {
        let words = if is_sensor(dev) { "Wired to {} (its program reads it)" } else { "Wired to {} (its program switches it)" };
        ui.label(RichText::new(crate::i18n::trf(words, &[&at])).color(tk.muted));
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
    egui::Grid::new(("iot-things", d.0)).striped(true).num_columns(5).show(ui, |ui| {
        for t in &things {
            ui.label(&t.name);
            ui.label(crate::i18n::tr(&t.what));
            ui.label(t.address.to_string());
            if t.kind == "sensor" {
                ui.label(trf("reading {}", &[&t.value.map_or("-".to_string(), |v| v.to_string())]));
                ui.label("");
            } else {
                let state = if t.on { tl("on") } else { tl("off") };
                match &t.image {
                    Some(img) => ui.label(format!("{state} · {}", tr_any(img))),
                    None => ui.label(state),
                };
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
    let rules: Vec<Value> = dev
        .iot
        .rules
        .iter()
        .map(|r| {
            let conds: Vec<Value> = r.all_conditions().iter().map(|c| json!({"sensor": c.sensor, "op": c.op.symbol(), "value": c.value})).collect();
            json!({"conditions": conds, "join": if r.any { "or" } else { "and" }, "actuator": r.actuator, "on": r.on})
        })
        .collect();
    let mut remove = None;
    for (i, r) in dev.iot.rules.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(rule_words(r));
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
    ui.horizontal_wrapped(|ui| {
        let l = ui.label(tl("If"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_sensor).desired_width(80.0).hint_text(tl("sensor"))).labelled_by(l.id);
        op_box(ui, ("op1", d.0), &mut form.rule_op);
        let l = ui.label(tl("value"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_above).desired_width(40.0).hint_text("30")).labelled_by(l.id);
        egui::ComboBox::from_id_salt(("join", d.0))
            .selected_text(if form.rule_or { tl("or") } else { tl("and") })
            .width(50.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut form.rule_or, false, tl("and"));
                ui.selectable_value(&mut form.rule_or, true, tl("or"));
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::ComboBox, true, tl("Join the conditions with")));
        let l = ui.label(tl("second sensor"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_sensor2).desired_width(80.0).hint_text(tl("optional"))).labelled_by(l.id);
        op_box(ui, ("op2", d.0), &mut form.rule_op2);
        let l = ui.label(tl("second value"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_value2).desired_width(40.0)).labelled_by(l.id);
    });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(("action", d.0))
            .selected_text(if form.rule_off { tl("switch off") } else { tl("switch on") })
            .width(90.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut form.rule_off, false, tl("switch on"));
                ui.selectable_value(&mut form.rule_off, true, tl("switch off"));
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::ComboBox, true, tl("What the rule does")));
        let l = ui.label(tl("device"));
        ui.add(egui::TextEdit::singleline(&mut form.rule_actuator).desired_width(90.0).hint_text(tl("device"))).labelled_by(l.id);
        if ui.button(tl("Add rule")).clicked() {
            let num = |s: &str| s.trim().parse::<i64>().unwrap_or(0);
            let mut conds = vec![json!({"sensor": form.rule_sensor.trim(), "op": OPS.get(form.rule_op).copied().unwrap_or(">"), "value": num(&form.rule_above)})];
            if !form.rule_sensor2.trim().is_empty() {
                conds.push(json!({"sensor": form.rule_sensor2.trim(), "op": OPS.get(form.rule_op2).copied().unwrap_or(">"), "value": num(&form.rule_value2)}));
            }
            let mut all = rules.clone();
            all.push(json!({"conditions": conds, "join": if form.rule_or { "or" } else { "and" }, "actuator": form.rule_actuator.trim(), "on": !form.rule_off}));
            if app.run("iot.server", json!({"device": d.0, "rules": all})).is_some() {
                *form = IotForm { filled: form.filled, ..IotForm::default() };
            }
        }
    });
}

/// `>`, `<` or `=`.
fn op_box(ui: &mut egui::Ui, id: (&str, u32), op: &mut usize) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(OPS.get(*op).copied().unwrap_or(">"))
        .width(36.0)
        .show_ui(ui, |ui| {
            for (i, o) in OPS.iter().enumerate() {
                ui.selectable_value(op, i, *o);
            }
        })
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::ComboBox, true, tl("Comparison")));
}

/// A rule in the interface language.
fn rule_words(r: &netlabcraft_model::iot::IotRule) -> String {
    let conds: Vec<String> = r.all_conditions().iter().map(|c| format!("{} {} {}", c.sensor, c.op.symbol(), c.value)).collect();
    let join = format!(" {} ", if r.any { tl("or") } else { tl("and") });
    if r.on {
        trf("If {}, switch {} on", &[&conds.join(&join), &r.actuator])
    } else {
        trf("If {}, switch {} off", &[&conds.join(&join), &r.actuator])
    }
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
        assert!(h.query_by_label("If Sensor1 > 30, switch Fan1 on").is_some());
    }
}
