//! The bottom box: device-type selection (categories, search, model tiles) on the left, the
//! scenario and its PDU list on the right.

use egui::{RichText, Sense, Ui, vec2};
use packetcraft_model::cable::Cable;
use packetcraft_model::{DeviceKind, models};

use crate::i18n::tl;
use crate::icons::{self, Glyph};
use crate::{PacketApp, Tool, canvas, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    NetworkDevices,
    EndDevices,
    Connections,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sub {
    #[default]
    Routers,
    Switches,
    Hubs,
    Hosts,
    Cables,
}

#[derive(Debug, Clone, Default)]
pub struct BoxState {
    pub category: Category,
    pub sub: Sub,
    pub search: String,
    /// The PDU list is shown (Toggle PDU List Window).
    pub hide_pdus: bool,
}

/// A tile in the model list.
struct Tile {
    sub: Sub,
    /// `None`: a cable.
    kind: Option<DeviceKind>,
    tool: Tool,
    short: &'static str,
    multilayer: bool,
    long: &'static str,
}

fn sub_of(kind: DeviceKind) -> Sub {
    match kind {
        DeviceKind::Router => Sub::Routers,
        DeviceKind::Switch => Sub::Switches,
        DeviceKind::Hub => Sub::Hubs,
        _ => Sub::Hosts,
    }
}

/// Every model of the catalogue, plus one tile per cable kind.
fn tiles() -> Vec<Tile> {
    let mut t: Vec<Tile> = models::MODELS
        .iter()
        .map(|m| Tile {
            sub: sub_of(m.kind),
            kind: Some(m.kind),
            tool: Tool::Place(m.id),
            short: m.label,
            long: m.description,
            multilayer: m.multilayer(),
        })
        .collect();
    t.extend(Cable::ALL.iter().map(|&c| Tile {
        sub: Sub::Cables,
        kind: None,
        tool: Tool::Cable(c),
        short: c.label(),
        multilayer: false,
        long: c.description(),
    }));
    t
}

/// A category or subcategory button: a picture, with the name as tooltip.
fn kind_button(ui: &mut Ui, label: &str, selected: bool, draw: impl FnOnce(&egui::Painter, egui::Rect)) -> egui::Response {
    let tk = theme::tokens(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 36.0), Sense::click());
    let resp = resp.on_hover_text(crate::i18n::tr(label));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, selected, label));
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect.shrink(1.0), 3.0, tk.pill.gamma_multiply(0.2));
        p.rect_stroke(rect.shrink(1.0), 3.0, egui::Stroke::new(1.0, tk.pill), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect.shrink(1.0), 3.0, tk.chrome_line.gamma_multiply(0.6));
    }
    draw(&p, egui::Rect::from_center_size(rect.center(), vec2(30.0, 30.0)));
    resp
}

pub fn show(app: &mut PacketApp, ui: &mut Ui) {
    let tk = theme::tokens(app.ui.dark);
    ui.horizontal_top(|ui| {
        // Categories, search, subcategories.
        ui.allocate_ui_with_layout(vec2(250.0, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                let b = &mut app.ui.bottom;
                for (cat, g, label, sub) in [
                    (Category::NetworkDevices, Glyph::NetworkDevices, "Network Devices", Sub::Routers),
                    (Category::EndDevices, Glyph::EndDevices, "End Devices", Sub::Hosts),
                    (Category::Connections, Glyph::Connections, "Connections", Sub::Cables),
                ] {
                    if kind_button(ui, label, b.category == cat && b.search.is_empty(), |p, r| icons::glyph(p, r, g, tk.glyph)).clicked() {
                        b.category = cat;
                        b.sub = sub;
                        b.search.clear();
                    }
                }
            });
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                icons::glyph(ui.painter(), r, Glyph::Search, tk.glyph);
                egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut app.ui.bottom.search)
                            .hint_text(tl("Search for device"))
                            .frame(egui::Frame::NONE)
                            .desired_width(200.0),
                    )
                    .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::TextInput, true, "Search for device"));
                });
            });
            ui.horizontal(|ui| {
                let b = &mut app.ui.bottom;
                let subs: &[(Sub, &str)] = match b.category {
                    Category::NetworkDevices => &[(Sub::Routers, "Routers"), (Sub::Switches, "Switches"), (Sub::Hubs, "Hubs")],
                    Category::EndDevices => &[(Sub::Hosts, "End Devices")],
                    Category::Connections => &[(Sub::Cables, "Connections")],
                };
                for (sub, label) in subs {
                    let sel = b.sub == *sub && b.search.is_empty();
                    let draw = |p: &egui::Painter, r: egui::Rect| match sub {
                        Sub::Routers => icons::device(p, r, DeviceKind::Router, &tk),
                        Sub::Switches => icons::device(p, r, DeviceKind::Switch, &tk),
                        Sub::Hubs => icons::device(p, r, DeviceKind::Hub, &tk),
                        Sub::Hosts => icons::device(p, r, DeviceKind::Pc, &tk),
                        Sub::Cables => icons::glyph(p, r, Glyph::Cable, tk.glyph),
                    };
                    if kind_button(ui, label, sel, draw).clicked() {
                        b.sub = *sub;
                        b.search.clear();
                    }
                }
            });
        });
        ui.separator();

        // Model tiles.
        let tiles_w = (ui.available_width() - 640.0).max(220.0);
        ui.allocate_ui_with_layout(vec2(tiles_w, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
            let q = app.ui.bottom.search.trim().to_lowercase();
            let all = tiles();
            let shown: Vec<&Tile> = all
                .iter()
                .filter(|m| {
                    if q.is_empty() {
                        m.sub == app.ui.bottom.sub
                    } else {
                        [m.short, m.long, crate::i18n::tl(m.short), crate::i18n::tl(m.long)].iter().any(|x| x.to_lowercase().contains(&q))
                    }
                })
                .collect();
            // Rows of tiles that wrap, scrolled up and down, so every model of a group is in reach.
            let room = (ui.available_height() - 30.0).max(60.0);
            egui::ScrollArea::vertical().id_salt("model_tiles").max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if shown.is_empty() {
                        ui.label(RichText::new(tl("No device matches.")).color(tk.muted));
                    }
                    for m in &shown {
                        let tool = m.tool;
                        let selected = app.ui.tool == tool;
                        // Room for a name on two lines (`Modular Router 3GE`).
                        let (rect, resp) = ui.allocate_exact_size(vec2(76.0, 56.0), Sense::click());
                        // Cable names and descriptions are interface text; model names stay as they are.
                        let (short, long) = if m.kind.is_none() { (crate::i18n::tl(m.short), crate::i18n::tl(m.long)) } else { (m.short, m.long) };
                        let resp = resp.on_hover_text(long);
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, selected, m.short));
                        let p = ui.painter_at(rect);
                        let fill = if selected {
                            tk.pill.gamma_multiply(0.18)
                        } else if resp.hovered() {
                            tk.chrome_line.gamma_multiply(0.5)
                        } else {
                            tk.chrome
                        };
                        p.rect_filled(rect.shrink(2.0), 8.0, fill);
                        p.rect_stroke(
                            rect.shrink(2.0),
                            8.0,
                            egui::Stroke::new(1.2, if selected { tk.pill } else { tk.chrome_line }),
                            egui::StrokeKind::Inside,
                        );
                        let icon = egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 17.0), vec2(28.0, 28.0));
                        match m.kind {
                            Some(k) => {
                                icons::device(&p, icon, k, &tk);
                                if let Tool::Place(id) = tool {
                                    icons::model_marks(&p, icon, id, m.multilayer);
                                }
                            }
                            None => {
                                if let Tool::Cable(c) = tool {
                                    icons::cable(&p, icon, c, &tk);
                                }
                            }
                        }
                        // The name wraps onto a second line under the icon rather than being cut.
                        // A long word (`Straight-Through`) gets a smaller size rather than an ellipsis.
                        let longest = short.split_whitespace().max_by_key(|w| w.chars().count()).unwrap_or("").to_string();
                        let size = [10.0, 9.0, 8.0]
                            .into_iter()
                            .find(|&s| {
                                ui.fonts_mut(|f| f.layout_no_wrap(longest.clone(), egui::FontId::proportional(s), tk.text)).size().x
                                    <= rect.width() - 8.0
                            })
                            .unwrap_or(8.0);
                        let mut job = egui::text::LayoutJob::simple(short.to_string(), egui::FontId::proportional(size), tk.text, rect.width() - 6.0);
                        job.halign = egui::Align::Center;
                        job.wrap.max_rows = 2;
                        let galley = ui.fonts_mut(|f| f.layout_job(job));
                        let top = rect.top() + 31.0;
                        p.galley(egui::pos2(rect.center().x, top), galley, tk.text);
                        if resp.clicked() {
                            app.set_tool(if selected { Tool::Select } else { tool });
                        }
                    }
                });
            });
            let current = all.iter().find(|m| m.tool == app.ui.tool).map_or("", |m| if m.kind.is_none() { crate::i18n::tl(m.long) } else { m.long });
            egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.canvas).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.vertical_centered(|ui| ui.label(RichText::new(current).small()));
            });
        });
        ui.separator();

        // Scenario and PDU list.
        ui.allocate_ui_with_layout(vec2(210.0, ui.available_height()), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (r, info) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                let info = info.on_hover_text(tl("Scenarios hold the PDUs you send, so a lab can be re-tested"));
                info.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, "Scenario information"));
                ui.painter().circle_filled(r.center(), 15.0, tk.bar);
                ui.painter().circle_filled(r.center(), 10.0, egui::Color32::WHITE);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(15.0), tk.bar);
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0, tk.chrome_line))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .corner_radius(3.0)
                    .show(ui, |ui| {
                        ui.set_min_width(130.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tl("Scenario 0")).size(16.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                ui.painter().add(egui::Shape::convex_polygon(
                                    vec![r.left_top() + vec2(1.0, 3.0), r.right_top() + vec2(-1.0, 3.0), r.center_bottom() - vec2(0.0, 2.0)],
                                    tk.glyph,
                                    egui::Stroke::NONE,
                                ));
                            });
                        });
                    });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                if crate::navy_button(ui, "New").on_hover_text(tl("Start an empty PDU list")).clicked() {
                    app.ui.pdus.clear();
                }
                ui.add_space(18.0);
                if crate::navy_button(ui, "Delete").on_hover_text(tl("Delete every PDU in the list")).clicked() {
                    app.ui.pdus.clear();
                }
            });
            ui.add_space(6.0);
            if crate::navy_button(ui, "Toggle PDU List Window").clicked() {
                app.ui.bottom.hide_pdus = !app.ui.bottom.hide_pdus;
            }
        });
        ui.separator();
        if app.ui.bottom.hide_pdus {
            if crate::navy_button(ui, "Fire All").on_hover_text(tl("Send every PDU in the list again")).clicked() {
                for i in 0..app.ui.pdus.len() {
                    canvas::fire(app, i);
                }
            }
            return;
        }
        pdu_list(app, ui);
    });
}

fn pdu_list(app: &mut PacketApp, ui: &mut Ui) {
    let tk = theme::tokens(app.ui.dark);
    let mut fire = None;
    let mut delete = None;
    egui::ScrollArea::both().id_salt("pdu_list").auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("pdu_grid").striped(true).num_columns(9).min_col_width(30.0).show(ui, |ui| {
            for h in ["Fire", "Last Status", "Source", "Destination", "Type", "Color", "Time(sec)", "Num", "Delete"] {
                ui.strong(crate::i18n::tr(h));
            }
            ui.end_row();
            for (i, pdu) in app.ui.pdus.iter().enumerate() {
                let name = |d| app.session.topology.device(d).map_or("?".to_string(), |x| x.name.clone());
                let (status, color) = match pdu.ok {
                    Some(true) => ("Successful", tk.ok),
                    Some(false) => ("Failed", tk.error),
                    None => ("In Progress", tk.muted),
                };
                let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, format!("Fire PDU {}", pdu.num)));
                ui.painter().circle_filled(r.center(), 6.0, color);
                if resp.on_hover_text(tl("Fire again")).clicked() {
                    fire = Some(i);
                }
                ui.label(RichText::new(status).color(color));
                ui.label(name(pdu.source));
                ui.label(format!("{} ({})", name(pdu.destination), pdu.address));
                ui.label(tl("ICMP"));
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 14.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, pdu.color);
                ui.label(format!("{:.3}", pdu.time_s));
                ui.label(pdu.num.to_string());
                if ui.small_button(tl("delete")).clicked() {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = fire {
        canvas::fire(app, i);
    }
    if let Some(i) = delete
        && i < app.ui.pdus.len()
    {
        app.ui.pdus.remove(i);
    }
}
