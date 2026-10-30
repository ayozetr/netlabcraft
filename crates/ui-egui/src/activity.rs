//! Activities in the window: the instructions with the score (students) and the Activity
//! Wizard (instructors). Both act through the `activity.*` commands.

use egui::{Color32, RichText};
use serde_json::{Value, json};

use crate::PacketApp;
use crate::i18n::tl;

#[derive(Debug, Clone, Default)]
pub struct ActivityView {
    pub show_instructions: bool,
    pub show_wizard: bool,
    /// The last Check Results answer.
    pub results: Option<Value>,
    /// Whether the lab had an activity last frame (to open the instructions when one appears).
    had: bool,
    title: String,
    instructions: String,
    ping_from: String,
    ping_to: String,
    ping_fail: bool,
    cfg_device: String,
    cfg_line: String,
    out_device: String,
    out_command: String,
    out_contains: String,
    /// The wizard's text fields were filled from the lab.
    loaded: bool,
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32) {
    ui.horizontal(|ui| {
        let l = ui.add_sized([110.0, 20.0], egui::Label::new(label));
        ui.add(egui::TextEdit::singleline(value).desired_width(width)).labelled_by(l.id);
    });
}

pub fn windows(app: &mut PacketApp, ctx: &egui::Context) {
    let has = app.session.topology.activity.is_some();
    if has && !app.ui.activity.had {
        app.ui.activity.show_instructions = true;
        app.ui.activity.results = None;
    }
    app.ui.activity.had = has;
    instructions(app, ctx);
    wizard(app, ctx);
}

fn instructions(app: &mut PacketApp, ctx: &egui::Context) {
    let Some(a) = app.session.topology.activity.clone() else { return };
    if !app.ui.activity.show_instructions {
        return;
    }
    let mut open = true;
    let mut check = false;
    let mut reset = false;
    egui::Window::new(tl("Instructions")).open(&mut open).default_size([380.0, 320.0]).anchor(egui::Align2::LEFT_TOP, [12.0, 190.0]).show(
        ctx,
        |ui| {
            ui.heading(&a.title);
            egui::ScrollArea::vertical().max_height(160.0).id_salt("activity_text").show(ui, |ui| {
                ui.label(if a.instructions.is_empty() { "(no instructions)" } else { a.instructions.as_str() });
            });
            ui.separator();
            let (score, total, percent) = match &app.ui.activity.results {
                Some(r) => (r["score"].as_u64().unwrap_or(0), r["total"].as_u64().unwrap_or(0), r["percent"].as_u64().unwrap_or(0)),
                None => (0, u64::from(a.total_points()), 0),
            };
            ui.label(RichText::new(format!("{}: {percent}%  ({score} {} {total} {})", tl("Completion"), tl("of"), tl("points"))).strong());
            ui.horizontal(|ui| {
                check = ui.button(tl("Check Results")).clicked();
                reset = ui.add_enabled(app.session.activity_start.is_some(), egui::Button::new(tl("Reset Activity"))).clicked();
            });
            if let Some(r) = &app.ui.activity.results {
                egui::ScrollArea::vertical().max_height(180.0).id_salt("activity_results").show(ui, |ui| {
                    for item in r["results"].as_array().into_iter().flatten() {
                        let passed = item["passed"].as_bool().unwrap_or(false);
                        let (mark, color) =
                            if passed { ("done", Color32::from_rgb(0x2e, 0x8b, 0x3e)) } else { ("missing", Color32::from_rgb(0xc8, 0x3a, 0x2e)) };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(mark).color(color).strong());
                            ui.label(item["label"].as_str().unwrap_or(""));
                        });
                    }
                });
            }
        },
    );
    app.ui.activity.show_instructions = open;
    if check {
        app.ui.activity.results = app.run("activity.check", json!({}));
    }
    if reset && app.run("activity.reset", json!({})).is_some() {
        app.ui.activity.results = None;
        app.info("Activity reset to how it was opened.");
    }
}

fn wizard(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.activity.show_wizard {
        app.ui.activity.loaded = false;
        return;
    }
    if !app.ui.activity.loaded {
        let a = app.session.topology.activity.clone().unwrap_or_default();
        app.ui.activity.title = if a.title.is_empty() { "Activity".into() } else { a.title };
        app.ui.activity.instructions = a.instructions;
        app.ui.activity.loaded = true;
    }
    let activity = app.session.topology.activity.clone();
    let mut open = true;
    let mut run: Vec<(&'static str, Value)> = Vec::new();
    let v = &mut app.ui.activity;
    egui::Window::new(tl("Activity Wizard")).open(&mut open).default_size([520.0, 520.0]).show(ctx, |ui| {
        ui.label(tl(
            "Build the finished network, capture its configuration as the answers, then put the lab back in its starting state and save it.",
        ));
        ui.separator();
        field(ui, tl("Title"), &mut v.title, 360.0);
        let l = ui.label(tl("Instructions"));
        ui.add(egui::TextEdit::multiline(&mut v.instructions).desired_rows(4).desired_width(f32::INFINITY)).labelled_by(l.id);
        let mut hidden = activity.as_ref().is_some_and(|a| a.hidden);
        ui.horizontal(|ui| {
            if ui.button(tl("Save title and instructions")).clicked() {
                run.push(("activity.set", json!({"title": v.title, "instructions": v.instructions})));
            }
            if ui.checkbox(&mut hidden, tl("Hide answers in the file")).changed() {
                run.push(("activity.set", json!({"hidden": hidden})));
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(tl("Capture answers from this lab"))
                .on_hover_text(tl("Every configuration line of the routers and switches, and every PC address, becomes a check"))
                .clicked()
            {
                if activity.is_none() {
                    run.push(("activity.set", json!({"title": v.title, "instructions": v.instructions})));
                }
                run.push(("activity.capture", json!({})));
            }
            if activity.is_some() && ui.button(tl("Remove the activity")).clicked() {
                run.push(("activity.clear", json!({})));
            }
        });
        ui.collapsing(tl("Add a ping check"), |ui| {
            field(ui, tl("From device"), &mut v.ping_from, 140.0);
            field(ui, tl("To address"), &mut v.ping_to, 140.0);
            ui.checkbox(&mut v.ping_fail, tl("Must fail (an access list should block it)"));
            if ui.button(tl("Add")).clicked() {
                run.push((
                    "activity.add_check",
                    json!({"type": "ping", "from": v.ping_from.trim(), "to": v.ping_to.trim(), "succeed": !v.ping_fail}),
                ));
            }
        });
        ui.collapsing(tl("Add a configuration check"), |ui| {
            field(ui, tl("Device"), &mut v.cfg_device, 140.0);
            field(ui, tl("Line"), &mut v.cfg_line, 300.0);
            ui.label(RichText::new(tl("Inside a block: interface g0/0 | ip address 10.0.0.1 255.255.255.0")).small());
            if ui.button(tl("Add")).clicked() {
                run.push(("activity.add_check", json!({"type": "config", "device": v.cfg_device.trim(), "line": v.cfg_line.trim()})));
            }
        });
        ui.collapsing(tl("Add a command check"), |ui| {
            field(ui, tl("Device"), &mut v.out_device, 140.0);
            field(ui, tl("Command"), &mut v.out_command, 300.0);
            field(ui, tl("Output shows"), &mut v.out_contains, 300.0);
            if ui.button(tl("Add")).clicked() {
                run.push((
                    "activity.add_check",
                    json!({"type": "output", "device": v.out_device.trim(), "command": v.out_command.trim(), "contains": v.out_contains.trim()}),
                ));
            }
        });
        ui.separator();
        let checks = activity.as_ref().map(|a| a.checks.clone()).unwrap_or_default();
        ui.label(
            RichText::new(format!(
                "{} checks, {} points",
                checks.len(),
                activity.as_ref().map_or(0, packetcraft_model::activity::Activity::total_points)
            ))
            .strong(),
        );
        egui::ScrollArea::vertical().max_height(200.0).id_salt("activity_checks").show(ui, |ui| {
            for (i, c) in checks.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button(tl("Remove")).clicked() {
                        run.push(("activity.remove_check", json!({"index": i})));
                    }
                    ui.label(format!("{} pt  {}", c.points, c.label));
                });
            }
        });
    });
    app.ui.activity.show_wizard = open;
    for (id, params) in run {
        if let Some(r) = app.run(id, params)
            && id == "activity.capture"
        {
            let n = r["checks"].as_u64().unwrap_or(0);
            app.info(format!("{n} checks captured. Now put the lab in its starting state and save it."));
        }
    }
}
