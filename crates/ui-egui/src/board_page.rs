//! Programmable boards: the Programming tab (the program's editor, Run and Stop, what it
//! printed, its variables and pins, and the IoT custom cables of each pin) and the board drawn
//! in code for the Physical tab. Every change is an engine command (`board.program`,
//! `board.wire`, `board.status`).

use egui::{Color32, RichText, Sense, vec2};
use netlabcraft_model::Device;
use serde_json::{Value, json};

use crate::i18n::{tl, tr_any, trf};
use crate::{PacketApp, theme};

/// The Programming tab's fields.
#[derive(Debug, Clone, Default)]
pub(crate) struct BoardForm {
    source: String,
    server: String,
    filled: bool,
}

/// A program to start from: reads a sensor on A0, switches what is on D0.
pub(crate) const EXAMPLE: &str = "# D0 on while A0 reads above 30\nlet limit = 30\nt = read(A0)\nif t > limit\n  write(D0, 1)\nelse\n  write(D0, 0)\nend\nprint \"reading\", t\nsleep 2\n";

fn status(app: &mut PacketApp, dev: &Device) -> Value {
    app.session.execute("board.status", json!({"device": dev.id.0})).unwrap_or(Value::Null)
}

pub(crate) fn tab(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, form: &mut BoardForm) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    if !form.filled {
        form.source = if dev.board.program.is_empty() { EXAMPLE.to_string() } else { dev.board.program.clone() };
        form.server = dev.iot.server.map(|a| a.to_string()).unwrap_or_default();
        form.filled = true;
    }
    let st = status(app, dev);
    ui.horizontal(|ui| {
        let state = st["state"].as_str().unwrap_or("not started");
        let words = match state {
            "running" => tl("Running"),
            "stopped" => tl("Stopped"),
            _ => tl("Not started"),
        };
        ui.label(RichText::new(words).strong().color(if state == "running" { tk.ok } else { tk.text }));
        if let Some(line) = st["line"].as_u64().filter(|_| state == "running") {
            ui.label(RichText::new(trf("at line {}", &[&line])).color(tk.muted));
        }
        if ui.button(tl("Run")).on_hover_text(tl("Save the program and run it from the top")).clicked() {
            let src = form.source.clone();
            app.run("board.program", json!({"device": d.0, "source": src, "run": true}));
        }
        if ui.add_enabled(state == "running", egui::Button::new(tl("Stop"))).clicked() {
            app.run("board.program", json!({"device": d.0, "run": false}));
        }
        if ui.button(tl("Save")).on_hover_text(tl("Keep the program without running it")).clicked() {
            let src = form.source.clone();
            app.run("board.program", json!({"device": d.0, "source": src}));
        }
        if ui.button(tl("Example")).on_hover_text(tl("Put a short example program in the editor")).clicked() {
            form.source = EXAMPLE.to_string();
        }
    });
    if let Some(m) = st["message"].as_str() {
        ui.label(RichText::new(tr_any(m)).color(tk.error));
    }
    ui.horizontal(|ui| {
        let l = ui.label(tl("IoT server:"));
        ui.add(egui::TextEdit::singleline(&mut form.server).desired_width(140.0)).labelled_by(l.id);
        if ui.button(tl("Apply")).clicked() {
            app.run("board.program", json!({"device": d.0, "server": form.server.trim()}));
        }
        ui.label(RichText::new(tl("for get(...) and report")).color(tk.muted));
    });
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        ui.label(RichText::new(tl("Program (BoardScript)")).strong());
        egui::ScrollArea::vertical().id_salt(("board_src", d.0)).max_height(260.0).show(ui, |ui| {
            let l = ui.label(RichText::new(tl("Program text")).small().color(tk.muted));
            ui.add(egui::TextEdit::multiline(&mut form.source).code_editor().desired_rows(14).desired_width(f32::INFINITY)).labelled_by(l.id);
        });
        ui.label(
            RichText::new(tl(
                "let x = 1 · x = read(A0) · write(D0, 1) · if / elif / else / end · while / end · sleep 2 · print x · report x · get(path) · stop",
            ))
            .small()
            .color(tk.muted),
        );
        let ui = &mut cols[1];
        ui.label(RichText::new(tl("Pins")).strong());
        pins_table(app, ui, dev, &st);
        ui.add_space(6.0);
        ui.label(RichText::new(tl("Variables")).strong());
        let vars = st["vars"].as_object().cloned().unwrap_or_default();
        if vars.is_empty() {
            ui.label(RichText::new(tl("(none yet)")).color(tk.muted));
        }
        for (k, v) in vars {
            ui.label(RichText::new(format!("{k} = {}", v.as_str().map_or_else(|| v.to_string(), str::to_string))).monospace());
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tl("Output")).strong());
        egui::Frame::new().fill(tk.console_bg).inner_margin(6.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt(("board_log", d.0)).max_height(120.0).stick_to_bottom(true).show(ui, |ui| {
                for l in st["log"].as_array().cloned().unwrap_or_default() {
                    ui.label(RichText::new(l.as_str().unwrap_or_default()).monospace().color(tk.console_text));
                }
            });
        });
    });
}

/// Each pin: what is wired to it (chosen from the smart things) and its level.
fn pins_table(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, st: &Value) {
    let things: Vec<String> =
        app.session.topology.devices.iter().filter(|x| netlabcraft_model::iot::thing(x).is_some()).map(|x| x.name.clone()).collect();
    let mut change: Option<(String, String)> = None;
    egui::Grid::new(("board_pins", dev.id.0)).striped(true).num_columns(3).show(ui, |ui| {
        for p in st["pins"].as_array().cloned().unwrap_or_default() {
            let pin = p["pin"].as_str().unwrap_or_default().to_string();
            ui.label(RichText::new(&pin).monospace());
            let wired = p["wired"].as_str().unwrap_or_default().to_string();
            let shown = if wired.is_empty() { tl("(nothing)").to_string() } else { wired.clone() };
            let label = trf("Pin {} wired to", &[&pin]);
            egui::ComboBox::from_id_salt(("pin", dev.id.0, pin.clone()))
                .selected_text(shown)
                .width(130.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(wired.is_empty(), tl("(nothing)")).clicked() && !wired.is_empty() {
                        change = Some((pin.clone(), String::new()));
                    }
                    for t in &things {
                        if ui.selectable_label(*t == wired, t).clicked() && *t != wired {
                            change = Some((pin.clone(), t.clone()));
                        }
                    }
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::ComboBox, true, label.clone()));
            ui.label(RichText::new(p["level"].to_string()).monospace());
            ui.end_row();
        }
    });
    if let Some((pin, thing)) = change {
        // The pin's old cable comes out, and a thing moved from another pin is unplugged there.
        if app.session.topology.device(dev.id).is_some_and(|x| x.board.wires.iter().any(|w| w.pin == pin)) {
            app.run("board.wire", json!({"device": dev.id.0, "pin": pin}));
        }
        if let Some((b, old)) = app.session.topology.resolve(&thing).ok().and_then(|id| app.session.topology.board_of(id)) {
            app.run("board.wire", json!({"device": b.0, "pin": old}));
        }
        if !thing.is_empty() {
            app.run("board.wire", json!({"device": dev.id.0, "pin": pin, "thing": thing}));
        }
    }
}

/// The Physical tab: the board seen from above, its pin header with the cables plugged in.
pub(crate) fn physical(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    crate::hardware::panel(app, ui, dev);
    let st = status(app, dev);
    let pins = st["pins"].as_array().cloned().unwrap_or_default();
    let sbc = dev.catalogue_model().id == "Board-SBC";
    ui.label(RichText::new(trf("{}: {} pins", &[&tl(dev.catalogue_model().label), &pins.len()])).strong());
    let size = vec2(ui.available_width().min(560.0), 190.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let picture = if sbc { tl("A single-board computer seen from above") } else { tl("A microcontroller board seen from above") };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, picture));
    let p = ui.painter_at(rect);
    let green = if sbc { Color32::from_rgb(0x1f, 0x6e, 0x43) } else { Color32::from_rgb(0x1d, 0x4f, 0x8a) };
    p.rect_filled(rect, 8.0, green);
    // Mounting holes, the chip and (SBC) the Ethernet socket; (MCU) the radio's antenna trace.
    for c in [
        rect.left_top() + vec2(12.0, 12.0),
        rect.right_top() + vec2(-12.0, 12.0),
        rect.left_bottom() + vec2(12.0, -12.0),
        rect.right_bottom() + vec2(-12.0, -12.0),
    ] {
        p.circle_filled(c, 5.0, Color32::from_rgb(0xd8, 0xc8, 0x8a));
        p.circle_filled(c, 2.5, tk.canvas);
    }
    let chip = egui::Rect::from_center_size(rect.center() + vec2(-40.0, 10.0), vec2(70.0, 70.0));
    p.rect_filled(chip, 3.0, Color32::from_rgb(0x22, 0x22, 0x26));
    for k in 0..7 {
        let o = 8.0 + k as f32 * 9.0;
        for (a, b) in [
            (chip.left_top() + vec2(o, 0.0), chip.left_top() + vec2(o, -5.0)),
            (chip.left_bottom() + vec2(o, 0.0), chip.left_bottom() + vec2(o, 5.0)),
        ] {
            p.line_segment([a, b], egui::Stroke::new(2.0, Color32::from_rgb(0xc0, 0xc0, 0xc8)));
        }
    }
    let lit = !dev.powered_off;
    p.circle_filled(chip.right_top() + vec2(24.0, 4.0), 5.0, if lit { tk.ok } else { Color32::from_rgb(0x2a, 0x2a, 0x2a) });
    p.text(chip.right_top() + vec2(34.0, 4.0), egui::Align2::LEFT_CENTER, "PWR", egui::FontId::proportional(10.0), Color32::WHITE);
    if sbc {
        let jack = egui::Rect::from_min_size(rect.right_center() + vec2(-58.0, -10.0), vec2(50.0, 40.0));
        p.rect_filled(jack, 2.0, Color32::from_rgb(0xb8, 0xbc, 0xc4));
        p.rect_filled(jack.shrink(8.0), 1.0, Color32::from_rgb(0x1a, 0x1c, 0x20));
    } else {
        let at = rect.right_center() + vec2(-60.0, 0.0);
        let pts: Vec<egui::Pos2> = (0..8).map(|i| at + vec2((i / 2) as f32 * 12.0, if (i + 1) % 4 < 2 { -16.0 } else { 16.0 })).collect();
        p.add(egui::Shape::line(pts, egui::Stroke::new(2.5, Color32::from_rgb(0xd8, 0xc8, 0x8a))));
    }
    // The pin header along the top edge: each pin's name, a socket (filled when a cable is in)
    // and its level.
    let n = pins.len().max(1) as f32;
    let room = rect.width() - 40.0;
    for (i, pin) in pins.iter().enumerate() {
        let x = rect.left() + 20.0 + (i as f32 + 0.5) * room / n;
        let socket = egui::Rect::from_center_size(egui::pos2(x, rect.top() + 38.0), vec2(14.0, 14.0));
        let wired = pin["wired"].as_str();
        p.rect_filled(socket, 1.0, Color32::from_rgb(0x15, 0x15, 0x18));
        if wired.is_some() {
            p.rect_filled(socket.shrink(3.0), 1.0, Color32::from_rgb(0xf2, 0x9a, 0x2e));
        }
        let level = pin["level"].as_i64().unwrap_or(0);
        p.circle_filled(
            socket.center_bottom() + vec2(0.0, 8.0),
            3.0,
            if lit && level != 0 { Color32::from_rgb(0xf6, 0xd3, 0x4a) } else { Color32::from_rgb(0x2a, 0x2a, 0x2a) },
        );
        p.text(
            socket.center_top() - vec2(0.0, 8.0),
            egui::Align2::CENTER_CENTER,
            pin["pin"].as_str().unwrap_or_default(),
            egui::FontId::proportional(10.0),
            Color32::WHITE,
        );
        if let Some(w) = wired {
            p.text(socket.center_bottom() + vec2(0.0, 22.0), egui::Align2::CENTER_CENTER, w, egui::FontId::proportional(9.0), Color32::WHITE);
        }
    }
    ui.label(
        RichText::new(tl(
            "Join a pin to a smart thing with the IoT Custom cable (Connections), or pick the thing for each pin in the Programming tab.",
        ))
        .color(tk.muted),
    );
}
