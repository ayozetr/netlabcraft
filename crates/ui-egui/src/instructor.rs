//! Instructor tools in the window: the answer network (a read-only drawing), the assessment
//! tree, connectivity tests, results files, the networks, languages and locks parts of the
//! Activity Wizard. Everything acts through the `activity.*` commands.

use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, Rect, RichText, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::i18n::{tl, trf};
use crate::{PacketApp, UiRequest};

#[derive(Debug, Clone, Default)]
pub struct ToolsView {
    pub show_answer: bool,
    pub show_tree: bool,
    pub show_connectivity: bool,
    /// The answer network as last read, and the lab revision it was read at.
    answer: Option<(u64, netlabcraft_model::Topology)>,
    /// The assessment tree as last read, and the revision.
    tree: Option<(u64, Value)>,
    /// Hints being typed in the tree, by item key.
    hints: BTreeMap<String, String>,
    /// The last connectivity run.
    pub connectivity: Option<Value>,
    /// The wizard's language tab, and the instructions per language besides the main one.
    pub tab: String,
    pub texts: BTreeMap<String, String>,
    new_lang: String,
    /// A results file pasted to check, and what the check said.
    verify_text: String,
    verify: Option<Value>,
}

fn ok_color() -> Color32 {
    Color32::from_rgb(0x2e, 0x8b, 0x3e)
}

fn bad_color() -> Color32 {
    Color32::from_rgb(0xc8, 0x3a, 0x2e)
}

pub fn windows(app: &mut PacketApp, ctx: &egui::Context) {
    if app.session.topology.activity.is_none() {
        let t = &mut app.ui.activity.tools;
        t.show_answer = false;
        t.show_tree = false;
        t.show_connectivity = false;
        return;
    }
    answer_window(app, ctx);
    tree_window(app, ctx);
    connectivity_window(app, ctx);
}

/// The language a text for the interface's language is kept under.
pub fn ui_lang() -> &'static str {
    crate::i18n::lang().code()
}

// --- Answer Network ------------------------------------------------------------------------

fn answer_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.activity.tools.show_answer {
        app.ui.activity.tools.answer = None;
        return;
    }
    let rev = app.session.revision;
    if app.ui.activity.tools.answer.as_ref().is_none_or(|(r, _)| *r != rev) {
        let lab = app.run("activity.answer", json!({})).and_then(|v| v["lab"].as_str().and_then(|j| netlabcraft_model::file::from_json(j).ok()));
        match lab {
            Some(t) => app.ui.activity.tools.answer = Some((app.session.revision, t)),
            None => {
                app.ui.activity.tools.show_answer = false;
                return;
            }
        }
    }
    let Some((_, t)) = app.ui.activity.tools.answer.clone() else { return };
    let tk = crate::theme::tokens(app.ui.dark);
    let mut open = true;
    egui::Window::new(tl("Answer Network")).open(&mut open).default_size([560.0, 420.0]).default_pos([420.0, 140.0]).show(ctx, |ui| {
        ui.label(
            RichText::new(trf("{} devices, {} cables. Read only: edit it from the Activity Wizard.", &[&t.devices.len(), &t.links.len()])).small(),
        );
        let size = vec2(ui.available_width().max(200.0), ui.available_height().max(240.0));
        let (resp, painter) = ui.allocate_painter(size, egui::Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, tl("Drawing of the answer network")));
        draw_network(&painter, resp.rect, &t, &tk);
    });
    app.ui.activity.tools.show_answer = open;
}

/// A lab drawn to fit a rectangle: cables, then device icons and names.
fn draw_network(painter: &egui::Painter, rect: Rect, t: &netlabcraft_model::Topology, tk: &crate::theme::Tokens) {
    painter.rect_filled(rect, 6.0, tk.canvas);
    if t.devices.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, tl("(no devices)"), FontId::proportional(14.0), tk.muted);
        return;
    }
    let (mut lo, mut hi) = (pos2(f32::MAX, f32::MAX), pos2(f32::MIN, f32::MIN));
    for d in &t.devices {
        lo = lo.min(pos2(d.x, d.y));
        hi = hi.max(pos2(d.x, d.y));
    }
    let inner = rect.shrink2(vec2(40.0, 34.0));
    let span = (hi - lo).max(vec2(1.0, 1.0));
    let zoom = (inner.width() / span.x).min(inner.height() / span.y).clamp(0.1, 1.5);
    let center = lo + span / 2.0;
    let at = |x: f32, y: f32| inner.center() + (pos2(x, y) - center) * zoom;
    let painter = painter.with_clip_rect(rect);
    for l in &t.links {
        if let (Some(a), Some(b)) = (t.device(l.a.device), t.device(l.b.device)) {
            painter.line_segment([at(a.x, a.y), at(b.x, b.y)], Stroke::new(1.5, tk.cable));
        }
    }
    let icon = (48.0 * zoom).clamp(18.0, 56.0);
    for d in &t.devices {
        let c = at(d.x, d.y);
        let r = Rect::from_center_size(c, vec2(icon, icon));
        let model = d.catalogue_model();
        crate::icons_families::model_icon(&painter, r, d.kind, model.id, model.multilayer(), tk);
        painter.text(pos2(c.x, r.bottom() + 2.0), Align2::CENTER_TOP, d.display_name(), FontId::proportional(12.0), tk.text);
    }
}

// --- Assessment Tree -----------------------------------------------------------------------

fn tree_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.activity.tools.show_tree {
        app.ui.activity.tools.tree = None;
        return;
    }
    let rev = app.session.revision;
    if app.ui.activity.tools.tree.as_ref().is_none_or(|(r, _)| *r != rev) {
        match app.run("activity.tree", json!({})) {
            Some(v) => app.ui.activity.tools.tree = Some((app.session.revision, v)),
            None => {
                app.ui.activity.tools.show_tree = false;
                return;
            }
        }
    }
    let Some((_, tree)) = app.ui.activity.tools.tree.clone() else { return };
    let items = tree["items"].as_array().cloned().unwrap_or_default();
    let mut open = true;
    let mut run: Vec<(&'static str, Value)> = Vec::new();
    let hints = &mut app.ui.activity.tools.hints;
    egui::Window::new(tl("Assessment Tree")).open(&mut open).default_size([620.0, 480.0]).default_pos([380.0, 120.0]).show(ctx, |ui| {
        ui.label(tl("What the answer network holds, device by device. Untick what isn't graded, set the points and a hint, then make the checks."));
        let graded = items.iter().filter(|i| i["points"].as_u64().unwrap_or(0) > 0).count();
        ui.label(RichText::new(trf("{} of {} items graded, {} points", &[&graded, &items.len(), &tree["points"].as_u64().unwrap_or(0)])).strong());
        ui.horizontal(|ui| {
            if ui
                .button(tl("Make checks from the tree"))
                .on_hover_text(tl("The graded items replace the configuration, address and cable checks"))
                .clicked()
            {
                run.push(("activity.tree_checks", json!({})));
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("activity_tree").show(ui, |ui| {
            let mut devices: Vec<&str> = Vec::new();
            for i in &items {
                let d = i["device"].as_str().unwrap_or("");
                if !devices.contains(&d) {
                    devices.push(d);
                }
            }
            for dev in devices {
                let mine: Vec<&Value> = items.iter().filter(|i| i["device"].as_str() == Some(dev)).collect();
                let points: u64 = mine.iter().map(|i| i["points"].as_u64().unwrap_or(0)).sum();
                egui::CollapsingHeader::new(trf("{}: {} items, {} points", &[&dev, &mine.len(), &points]))
                    .id_salt(("tree", dev))
                    .default_open(true)
                    .show(ui, |ui| {
                        for i in mine {
                            let key = i["key"].as_str().unwrap_or("").to_string();
                            let stored = i["points"].as_u64().unwrap_or(0) as u32;
                            let mut pts = stored.max(1);
                            let mut graded = stored > 0;
                            let kind = match i["kind"].as_str().unwrap_or("") {
                                "config" => tl("configuration"),
                                "address" => tl("address"),
                                "cable" => tl("cable"),
                                _ => "",
                            };
                            let text = i["text"].as_str().unwrap_or("");
                            ui.horizontal(|ui| {
                                let cr = ui.checkbox(&mut graded, tl("graded"));
                                if cr.changed() {
                                    run.push(("activity.weight", json!({"key": key, "points": u32::from(graded)})));
                                }
                                let l = ui.label(RichText::new(kind).weak());
                                if ui.add_enabled(graded, egui::DragValue::new(&mut pts).range(1..=1000).suffix(" pt")).labelled_by(l.id).changed()
                                    && graded
                                    && pts != stored
                                {
                                    run.push(("activity.weight", json!({"key": key, "points": pts})));
                                }
                                ui.label(RichText::new(text).monospace());
                            });
                            let hint = hints.entry(key.clone()).or_insert_with(|| i["feedback"].as_str().unwrap_or("").to_string());
                            ui.horizontal(|ui| {
                                ui.add_space(28.0);
                                let l = ui.label(RichText::new(tl("Hint")).small());
                                let r = ui
                                    .add(egui::TextEdit::singleline(hint).desired_width(320.0).hint_text(tl("shown when it fails")))
                                    .labelled_by(l.id);
                                if r.lost_focus() && hint.as_str() != i["feedback"].as_str().unwrap_or("") {
                                    run.push(("activity.weight", json!({"key": key, "feedback": hint.clone()})));
                                }
                            });
                        }
                    });
            }
        });
    });
    app.ui.activity.tools.show_tree = open;
    for (id, params) in run {
        if let Some(r) = app.run(id, params)
            && id == "activity.tree_checks"
        {
            app.info(trf("{} checks made, {} points.", &[&r["checks"].as_u64().unwrap_or(0), &r["points"].as_u64().unwrap_or(0)]));
        }
    }
}

// --- Connectivity Tests --------------------------------------------------------------------

fn connectivity_window(app: &mut PacketApp, ctx: &egui::Context) {
    if !app.ui.activity.tools.show_connectivity {
        return;
    }
    let mut open = true;
    let mut run = false;
    let r = app.ui.activity.tools.connectivity.clone();
    egui::Window::new(tl("Connectivity Tests")).open(&mut open).default_size([560.0, 300.0]).default_pos([420.0, 200.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            run = ui.button(tl("Run the tests")).clicked();
            if let Some(r) = &r {
                if r["detail"] == true {
                    ui.label(RichText::new(trf("{} of {} pass", &[&r["passed"].as_u64().unwrap_or(0), &r["total"].as_u64().unwrap_or(0)])).strong());
                } else {
                    ui.label(RichText::new(tl("The instructor shows the score only.")).italics());
                }
            }
        });
        ui.separator();
        let Some(r) = &r else {
            ui.label(tl("Each test pings from a device to an address; it passes when the replies come (or don't, when blocking is the point)."));
            return;
        };
        let tests = r["tests"].as_array().cloned().unwrap_or_default();
        if tests.is_empty() {
            ui.label(RichText::new(tl("This activity has no ping checks.")).italics());
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("activity_connectivity").show(ui, |ui| {
            egui::Grid::new("connectivity_grid").striped(true).num_columns(5).spacing([12.0, 6.0]).show(ui, |ui| {
                for h in ["Result", "From", "To", "Expected", "Got"] {
                    ui.label(RichText::new(tl(h)).strong());
                }
                ui.end_row();
                for t in &tests {
                    match t["passed"].as_bool() {
                        Some(true) => ui.label(RichText::new(tl("pass")).color(ok_color()).strong()),
                        Some(false) => ui.label(RichText::new(tl("fail")).color(bad_color()).strong()),
                        None => ui.label(RichText::new(tl("(hidden)")).italics().weak()),
                    };
                    ui.label(t["from"].as_str().unwrap_or(""));
                    ui.label(RichText::new(t["to"].as_str().unwrap_or("")).monospace());
                    ui.label(crate::i18n::tr(t["expect"].as_str().unwrap_or("")));
                    ui.label(crate::i18n::tr(t["actual"].as_str().unwrap_or("")));
                    ui.end_row();
                }
            });
        });
    });
    app.ui.activity.tools.show_connectivity = open;
    if run {
        app.ui.activity.tools.connectivity = app.run("activity.connectivity", json!({}));
    }
}

// --- Parts of the Instructions window and the Activity Wizard ------------------------------

/// The Instructions window's second row: connectivity tests and the results file.
pub fn student_row(app: &mut PacketApp, ui: &mut egui::Ui, elapsed: u64) {
    let mut export = None;
    ui.horizontal(|ui| {
        if ui.button(tl("Connectivity Tests")).clicked() {
            app.ui.activity.tools.show_connectivity = true;
        }
        if ui.button(tl("Save results (JSON)")).on_hover_text(tl("A signed file with your name, the time and your score, to hand in")).clicked() {
            export = Some("json");
        }
        if ui.button(tl("Save results (CSV)")).clicked() {
            export = Some("csv");
        }
    });
    if let Some(format) = export
        && let Some(r) = app.run("activity.export_results", json!({"format": format, "elapsed_s": elapsed}))
    {
        let name = r["file_name"].as_str().unwrap_or("results.json").to_string();
        let text = r["text"].as_str().unwrap_or("").to_string();
        app.requests.push(UiRequest::SaveText { name, text });
    }
}

/// The language tabs above the wizard's instructions box; returns the text being edited.
pub fn language_tabs<'a>(ui: &mut egui::Ui, tools: &'a mut ToolsView, main: &'a mut String, main_lang: &str) -> &'a mut String {
    let mut langs: Vec<String> = vec![main_lang.to_string()];
    for l in ["en", "es"].iter().map(|s| s.to_string()).chain(tools.texts.keys().cloned()) {
        if !langs.contains(&l) {
            langs.push(l);
        }
    }
    if tools.tab.is_empty() || !langs.contains(&tools.tab) {
        tools.tab = main_lang.to_string();
    }
    ui.horizontal(|ui| {
        ui.label(tl("Instructions in"));
        for lang in &langs {
            let name = match lang.as_str() {
                "en" => "English".to_string(),
                "es" => "Español".to_string(),
                other => other.to_string(),
            };
            let shown = if lang == main_lang { trf("{} (main)", &[&name]) } else { name };
            if ui.selectable_label(tools.tab == *lang, shown).clicked() {
                tools.tab = lang.clone();
            }
        }
        let l = ui.label(tl("Other language code"));
        ui.add(egui::TextEdit::singleline(&mut tools.new_lang).desired_width(48.0)).labelled_by(l.id);
        let code = tools.new_lang.trim().to_lowercase();
        if ui.add_enabled(!code.is_empty() && code.len() <= 16, egui::Button::new(tl("Add"))).clicked() {
            tools.texts.entry(code.clone()).or_default();
            tools.tab = code;
            tools.new_lang.clear();
        }
    });
    if tools.tab == main_lang { main } else { tools.texts.entry(tools.tab.clone()).or_default() }
}

/// The commands that keep the translations typed in the wizard.
pub fn translation_commands(tools: &ToolsView, main_lang: &str) -> Vec<(&'static str, Value)> {
    tools.texts.iter().filter(|(l, _)| l.as_str() != main_lang).map(|(l, t)| ("activity.translate", json!({"lang": l, "text": t}))).collect()
}

/// The wizard's networks, locks and results-file parts.
pub fn wizard_sections(
    ui: &mut egui::Ui,
    app_tools: &mut ToolsView,
    a: &netlabcraft_model::activity::Activity,
    devices: &[(u32, String)],
    run: &mut Vec<(&'static str, Value)>,
) {
    ui.collapsing(tl("Answer and initial networks"), |ui| {
        let answer = match a.answer.as_ref().map(|s| s.scheme.as_str()) {
            None => tl("none yet"),
            Some("plain") => tl("kept in the clear"),
            Some("salt") => tl("hidden from a casual look (not encryption: the key is in the file)"),
            Some(_) => tl("encrypted with the password"),
        };
        ui.label(trf("Answer network: {}", &[&answer]));
        ui.horizontal_wrapped(|ui| {
            if ui.button(tl("Save this lab as the answer")).clicked() {
                run.push(("activity.save_answer", json!({})));
            }
            if ui.add_enabled(a.answer.is_some(), egui::Button::new(tl("View the answer network"))).clicked() {
                app_tools.show_answer = true;
            }
            if ui.add_enabled(a.answer.is_some(), egui::Button::new(tl("Edit it in the workspace"))).clicked() {
                run.push(("activity.load_answer", json!({})));
            }
            if ui.add_enabled(a.answer.is_some(), egui::Button::new(tl("Assessment Tree"))).clicked() {
                app_tools.show_tree = true;
            }
        });
        ui.label(trf(
            "Initial network: {}",
            &[&if a.initial.is_some() {
                tl("saved: Reset Activity returns to it")
            } else {
                tl("none: Reset Activity returns to the lab as it was opened")
            }],
        ));
        ui.horizontal_wrapped(|ui| {
            if ui.button(tl("Save this lab as the initial network")).clicked() {
                run.push(("activity.save_initial", json!({})));
            }
            if ui
                .add_enabled(a.initial.is_some(), egui::Button::new(tl("Edit the initial network")))
                .on_hover_text(tl("Puts it in the workspace; save it again when done"))
                .clicked()
            {
                run.push(("activity.load_initial", json!({})));
            }
            if ui.add_enabled(a.initial.is_some(), egui::Button::new(tl("Remove the initial network"))).clicked() {
                run.push(("activity.clear_initial", json!({})));
            }
        });
        ui.label(
            RichText::new(tl("Lock the activity so students can't open the answer network; with a password it is encrypted in the file.")).small(),
        );
    });
    ui.collapsing(tl("What the student may change"), |ui| {
        ui.label(RichText::new(tl("These locks hold while the activity is locked with the password.")).small());
        let mut dev = a.locks.devices;
        if ui.checkbox(&mut dev, tl("No adding or removing devices")).changed() {
            run.push(("activity.locks", json!({"devices": dev})));
        }
        let mut cab = a.locks.cables;
        if ui.checkbox(&mut cab, tl("No cable changes")).changed() {
            run.push(("activity.locks", json!({"cables": cab})));
        }
        ui.label(tl("Devices whose configuration is locked:"));
        ui.horizontal_wrapped(|ui| {
            for (id, name) in devices {
                let mut on = a.locks.config.iter().any(|d| d.0 == *id);
                if ui.checkbox(&mut on, name).changed() {
                    let mut ids: Vec<u32> = a.locks.config.iter().map(|d| d.0).filter(|d| d != id).collect();
                    if on {
                        ids.push(*id);
                    }
                    run.push(("activity.locks", json!({"config": ids})));
                }
            }
        });
    });
    ui.collapsing(tl("Check a student's results file"), |ui| {
        let l = ui.label(tl("Paste the results file"));
        ui.add(egui::TextEdit::multiline(&mut app_tools.verify_text).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace))
            .labelled_by(l.id);
        if ui.add_enabled(!app_tools.verify_text.trim().is_empty(), egui::Button::new(tl("Verify"))).clicked() {
            run.push(("activity.verify_results", json!({"text": app_tools.verify_text})));
        }
        if let Some(v) = &app_tools.verify {
            let (text, color) = if v["valid"] == true {
                (
                    trf(
                        "Genuine: {} scored {}% ({} of {} points).",
                        &[&v["student"].as_str().unwrap_or(""), &v["percent"], &v["score"], &v["total"]],
                    ),
                    ok_color(),
                )
            } else if v["same_activity"] == false {
                (tl("This results file is from another activity.").to_string(), bad_color())
            } else {
                (tl("This results file was changed after it was saved.").to_string(), bad_color())
            };
            ui.label(RichText::new(text).color(color).strong());
        }
    });
}

/// Keeps what a wizard command answered that the tools show.
pub fn after_command(tools: &mut ToolsView, id: &str, r: &Value) {
    if id == "activity.verify_results" {
        tools.verify = Some(r.clone());
    }
}
