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
    /// The side-by-side window: each check's answer next to what the lab holds.
    pub show_compare: bool,
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
    /// When the activity was opened (egui time, seconds), for the time limit.
    started: f64,
    /// The time ran out: the results were taken then and Check Results is off.
    time_up: bool,
    feedback: String,
    password: String,
    /// The student's name, to start an activity with variables.
    student: String,
    /// The wizard's variables, one per line: `name = value, value, …`.
    variables: String,
    section: String,
}

/// `name = a, b, c` lines as the `activity.set` variables object.
fn parse_variables(text: &str) -> Value {
    let mut map = serde_json::Map::new();
    for line in text.lines() {
        if let Some((name, values)) = line.split_once('=') {
            let values: Vec<Value> = values.split(',').map(str::trim).filter(|v| !v.is_empty()).map(|v| json!(v)).collect();
            if !name.trim().is_empty() && !values.is_empty() {
                map.insert(name.trim().to_string(), Value::Array(values));
            }
        }
    }
    Value::Object(map)
}

/// The time left as `m:ss` (or `h:mm:ss`).
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
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
        app.ui.activity.started = ctx.input(|i| i.time);
        app.ui.activity.time_up = false;
    }
    app.ui.activity.had = has;
    instructions(app, ctx);
    compare(app, ctx);
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
    let mut start = None;
    let now = ctx.input(|i| i.time);
    let left = (a.time_limit > 0).then(|| {
        let elapsed = (now - app.ui.activity.started).max(0.0) as u64;
        u64::from(a.time_limit).saturating_mul(60).saturating_sub(elapsed)
    });
    if left == Some(0) && !app.ui.activity.time_up {
        app.ui.activity.time_up = true;
        app.ui.activity.results = app.run("activity.check", json!({}));
        app.info(tl("Time is up: these are your results."));
    }
    if left.is_some_and(|l| l > 0) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    let time_up = app.ui.activity.time_up;
    egui::Window::new(tl("Instructions")).open(&mut open).default_size([380.0, 400.0]).anchor(egui::Align2::LEFT_TOP, [12.0, 178.0]).show(
        ctx,
        |ui| {
            ui.heading(a.resolve(crate::i18n::tr(&a.title)));
            // An activity with values per student starts with the student's name.
            if a.needs_student() {
                ui.label(tl("This activity gives each student their own values. Type your name to start."));
                ui.horizontal(|ui| {
                    let l = ui.label(tl("Your name"));
                    ui.add(egui::TextEdit::singleline(&mut app.ui.activity.student).desired_width(180.0)).labelled_by(l.id);
                    if ui.add_enabled(!app.ui.activity.student.trim().is_empty(), egui::Button::new(tl("Start"))).clicked() {
                        start = Some(app.ui.activity.student.trim().to_string());
                    }
                });
                return;
            }
            if let Some(who) = &a.student {
                ui.label(RichText::new(crate::i18n::trf("Student: {}", &[who])).small());
            }
            egui::ScrollArea::vertical().max_height(160.0).id_salt("activity_text").show(ui, |ui| {
                ui.label(if a.instructions.is_empty() { tl("(no instructions)").to_string() } else { a.resolve(crate::i18n::tr(&a.instructions)) });
            });
            ui.separator();
            let (score, total, percent) = match &app.ui.activity.results {
                Some(r) => (r["score"].as_u64().unwrap_or(0), r["total"].as_u64().unwrap_or(0), r["percent"].as_u64().unwrap_or(0)),
                None => (0, u64::from(a.total_points()), 0),
            };
            ui.label(RichText::new(format!("{}: {percent}%  ({score} {} {total} {})", tl("Completion"), tl("of"), tl("points"))).strong());
            if let Some(l) = left {
                let text = if l == 0 { tl("Time is up").to_string() } else { format!("{}: {}", tl("Time left"), clock(l)) };
                let color = if l < 60 { Color32::from_rgb(0xc8, 0x3a, 0x2e) } else { ui.visuals().text_color() };
                ui.label(RichText::new(text).color(color).strong());
            }
            ui.horizontal(|ui| {
                check = ui.add_enabled(!time_up, egui::Button::new(tl("Check Results"))).clicked();
                reset = ui.add_enabled(app.session.activity_start.is_some(), egui::Button::new(tl("Reset Activity"))).clicked();
                let detail = app.ui.activity.results.as_ref().is_some_and(|r| r["detail"] != false);
                if ui
                    .add_enabled(detail, egui::Button::new(tl("Compare")))
                    .on_hover_text(tl("Each check's answer next to what your lab has"))
                    .clicked()
                {
                    app.ui.activity.show_compare = true;
                }
            });
            if let Some(r) = &app.ui.activity.results {
                // At least a few rows tall, whatever room the window has left.
                egui::ScrollArea::vertical().min_scrolled_height(120.0).max_height(170.0).id_salt("activity_results").show(ui, |ui| {
                    if r["detail"] == false {
                        ui.label(RichText::new(tl("The instructor shows the score only.")).italics());
                    }
                    let sections = r["sections"].as_array().cloned().unwrap_or_default();
                    if sections.len() > 1 {
                        for sec in &sections {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}%  ({}/{})",
                                    sec["name"].as_str().unwrap_or(""),
                                    sec["percent"].as_u64().unwrap_or(0),
                                    sec["score"].as_u64().unwrap_or(0),
                                    sec["total"].as_u64().unwrap_or(0)
                                ))
                                .strong(),
                            );
                        }
                        ui.separator();
                    }
                    for item in r["results"].as_array().into_iter().flatten() {
                        let passed = item["passed"].as_bool().unwrap_or(false);
                        let (mark, color) =
                            if passed { ("done", Color32::from_rgb(0x2e, 0x8b, 0x3e)) } else { ("missing", Color32::from_rgb(0xc8, 0x3a, 0x2e)) };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tl(mark)).color(color).strong());
                            ui.label(crate::i18n::tr_any(item["label"].as_str().unwrap_or("")));
                        });
                        if let Some(hint) = item["feedback"].as_str().filter(|h| !passed && !h.is_empty()) {
                            ui.label(RichText::new(format!("    {}", crate::i18n::tr(hint))).italics().small());
                        }
                    }
                });
            }
        },
    );
    app.ui.activity.show_instructions = open;
    if let Some(who) = start
        && app.run("activity.start", json!({"student": who})).is_some()
    {
        app.ui.activity.started = now;
    }
    if check {
        app.ui.activity.results = app.run("activity.check", json!({}));
    }
    if reset && app.run("activity.reset", json!({})).is_some() {
        app.ui.activity.results = None;
        app.ui.activity.started = now;
        app.ui.activity.time_up = false;
        app.info(tl("Activity reset to how it was opened."));
    }
}

/// The answer next to the lab, check by check, grouped by section. A hidden activity shows only
/// the lab's side (the engine leaves `expected` empty).
fn compare(app: &mut PacketApp, ctx: &egui::Context) {
    let Some(r) = app.ui.activity.results.as_ref().filter(|r| r["detail"] != false) else {
        app.ui.activity.show_compare = false;
        return;
    };
    if !app.ui.activity.show_compare {
        return;
    }
    let mut open = true;
    let items = r["results"].as_array().cloned().unwrap_or_default();
    egui::Window::new(tl("Compare with the Answer")).open(&mut open).default_size([720.0, 420.0]).default_pos([410.0, 180.0]).show(ctx, |ui| {
        let failed = items.iter().filter(|i| i["passed"] != true).count();
        ui.label(crate::i18n::trf("{} of {} checks differ from the answer", &[&failed, &items.len()]));
        ui.separator();
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("activity_compare").striped(true).num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                for h in ["Result", "Check", "Answer", "Your lab"] {
                    ui.label(RichText::new(tl(h)).strong());
                }
                ui.end_row();
                let mut section = "";
                for item in &items {
                    let sec = item["section"].as_str().unwrap_or("");
                    if sec != section && !sec.is_empty() {
                        section = sec;
                        ui.allocate_space(egui::Vec2::ZERO);
                        ui.label(RichText::new(sec).strong().underline());
                        ui.end_row();
                    }
                    let passed = item["passed"].as_bool().unwrap_or(false);
                    let color = if passed { Color32::from_rgb(0x2e, 0x8b, 0x3e) } else { Color32::from_rgb(0xc8, 0x3a, 0x2e) };
                    ui.label(RichText::new(if passed { tl("same") } else { tl("differs") }).color(color).strong());
                    ui.label(crate::i18n::tr_any(item["label"].as_str().unwrap_or("")));
                    let expected = item["expected"].as_str().unwrap_or("");
                    if expected.is_empty() {
                        ui.label(RichText::new(tl("(hidden)")).italics().weak());
                    } else {
                        ui.label(RichText::new(crate::i18n::tr_any(expected)).monospace());
                    }
                    let actual = item["actual"].as_str().unwrap_or("");
                    if actual.is_empty() {
                        ui.label(RichText::new(tl("(nothing)")).italics().weak());
                    } else {
                        ui.label(RichText::new(crate::i18n::tr_any(actual)).monospace().color(if passed {
                            ui.visuals().text_color()
                        } else {
                            color
                        }));
                    }
                    ui.end_row();
                }
            });
        });
    });
    app.ui.activity.show_compare = open;
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
        app.ui.activity.variables = a.variables.iter().map(|v| format!("{} = {}", v.name, v.values.join(", "))).collect::<Vec<_>>().join("\n");
        app.ui.activity.loaded = true;
    }
    let activity = app.session.topology.activity.clone();
    let mut open = true;
    let mut run: Vec<(&'static str, Value)> = Vec::new();
    let v = &mut app.ui.activity;
    if activity.as_ref().is_some_and(netlabcraft_model::activity::Activity::locked) {
        egui::Window::new(tl("Activity Wizard")).open(&mut open).default_size([360.0, 120.0]).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
            ui.label(tl("This activity is locked. The instructor's password unlocks it."));
            ui.horizontal(|ui| {
                let l = ui.label(tl("Password"));
                ui.add(egui::TextEdit::singleline(&mut v.password).password(true).desired_width(160.0)).labelled_by(l.id);
                if ui.button(tl("Unlock")).clicked() {
                    run.push(("activity.unlock", json!({"password": v.password})));
                    v.password.clear();
                }
            });
        });
        app.ui.activity.show_wizard = open;
        for (id, params) in run {
            app.run(id, params);
        }
        return;
    }
    egui::Window::new(tl("Activity Wizard")).open(&mut open).default_size([520.0, 520.0]).default_pos(crate::BELOW_TOOLBARS).show(ctx, |ui| {
        ui.label(tl(
            "Build the finished network, capture its configuration as the answers, then put the lab back in its starting state and save it.",
        ));
        ui.separator();
        field(ui, tl("Title"), &mut v.title, 360.0);
        let l = ui.label(tl("Instructions"));
        ui.add(egui::TextEdit::multiline(&mut v.instructions).desired_rows(4).desired_width(f32::INFINITY)).labelled_by(l.id);
        let l = ui.label(tl("Values per student, one variable per line (net = 10, 20, 30); write {net} in the instructions and checks"));
        ui.add(egui::TextEdit::multiline(&mut v.variables).desired_rows(2).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace)).labelled_by(l.id);
        let mut hidden = activity.as_ref().is_some_and(|a| a.hidden);
        ui.horizontal(|ui| {
            if ui.button(tl("Save title and instructions")).clicked() {
                run.push(("activity.set", json!({"title": v.title, "instructions": v.instructions, "variables": parse_variables(&v.variables)})));
            }
                        if ui.checkbox(&mut hidden, tl("Hide answers in the file")).changed() {
                run.push(("activity.set", json!({"hidden": hidden})));
            }
        });
        if let Some(a) = &activity {
            ui.horizontal(|ui| {
                let mut minutes = a.time_limit;
                let l = ui.label(tl("Time limit (minutes, 0: none)"));
                if ui.add(egui::DragValue::new(&mut minutes).range(0..=netlabcraft_model::activity::MAX_TIME_LIMIT)).labelled_by(l.id).changed() {
                    run.push(("activity.set", json!({"time_limit": minutes})));
                }
                let mut score_only = a.score_only;
                if ui
                    .checkbox(&mut score_only, tl("Students see the score only"))
                    .on_hover_text(tl("While locked, Check Results doesn't say which checks passed"))
                    .changed()
                {
                    run.push(("activity.set", json!({"score_only": score_only})));
                }
            });
            ui.horizontal(|ui| {
                let l = ui.label(tl("Password"));
                ui.add(egui::TextEdit::singleline(&mut v.password).password(true).desired_width(140.0)).labelled_by(l.id);
                if ui
                    .add_enabled(!v.password.is_empty(), egui::Button::new(tl("Lock the activity")))
                    .on_hover_text(tl("Students can't change the activity or see its checks until it is unlocked"))
                    .clicked()
                {
                    run.push(("activity.lock", json!({"password": v.password})));
                    v.password.clear();
                }
            });
        }
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
                field(ui, tl("Hint when it fails"), &mut v.feedback, 300.0);
                field(ui, tl("Section"), &mut v.section, 200.0);
        ui.collapsing(tl("Add a ping check"), |ui| {
            field(ui, tl("From device"), &mut v.ping_from, 140.0);
            field(ui, tl("To address"), &mut v.ping_to, 140.0);
            ui.checkbox(&mut v.ping_fail, tl("Must fail (an access list should block it)"));
            if ui.button(tl("Add")).clicked() {
                run.push((
                    "activity.add_check",
                    json!({"type": "ping", "from": v.ping_from.trim(), "to": v.ping_to.trim(), "succeed": !v.ping_fail, "feedback": v.feedback.trim(), "section": v.section.trim()}),
                ));
            }
        });
        ui.collapsing(tl("Add a configuration check"), |ui| {
            field(ui, tl("Device"), &mut v.cfg_device, 140.0);
            field(ui, tl("Line"), &mut v.cfg_line, 300.0);
            ui.label(RichText::new(tl("Inside a block: interface g0/0 | ip address 10.0.0.1 255.255.255.0")).small());
            if ui.button(tl("Add")).clicked() {
                run.push(("activity.add_check", json!({"type": "config", "device": v.cfg_device.trim(), "line": v.cfg_line.trim(), "feedback": v.feedback.trim(), "section": v.section.trim()})));
            }
        });
        ui.collapsing(tl("Add a command check"), |ui| {
            field(ui, tl("Device"), &mut v.out_device, 140.0);
            field(ui, tl("Command"), &mut v.out_command, 300.0);
            field(ui, tl("Output shows"), &mut v.out_contains, 300.0);
            if ui.button(tl("Add")).clicked() {
                run.push((
                    "activity.add_check",
                    json!({"type": "output", "device": v.out_device.trim(), "command": v.out_command.trim(), "contains": v.out_contains.trim(), "feedback": v.feedback.trim(), "section": v.section.trim()}),
                ));
            }
        });
        ui.separator();
        let checks = activity.as_ref().map(|a| a.checks.clone()).unwrap_or_default();
        ui.label(
            RichText::new(crate::i18n::trf(
                "{} checks, {} points",
                &[&checks.len(), &activity.as_ref().map_or(0, netlabcraft_model::activity::Activity::total_points)],
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
            app.info(crate::i18n::trf("{} checks captured. Now put the lab in its starting state and save it.", &[&n]));
        }
    }
}
