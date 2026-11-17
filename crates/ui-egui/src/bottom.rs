//! The bottom box: device-type selection (categories, search, model tiles) on the left, the
//! scenario and its PDU list on the right.

use egui::{RichText, Sense, Ui, vec2};
use netlabcraft_model::DeviceKind;
use netlabcraft_model::cable::Cable;
use netlabcraft_model::furniture::FittingKind;
use netlabcraft_model::models::{self, Group};

use crate::i18n::tl;
use crate::icons::{self, Glyph};
use crate::{PacketApp, Tool, canvas, theme};

/// The categories of the device box. The reference app also has Multiuser (out of scope here) and
/// Legacy (nothing of ours belongs in it), so neither is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    NetworkDevices,
    EndDevices,
    Components,
    Connections,
    /// The devices placed most recently.
    Miscellaneous,
}

impl Category {
    /// The name the interface shows (and its tooltip).
    pub fn label(self) -> &'static str {
        match self {
            Category::NetworkDevices => "Network Devices",
            Category::EndDevices => "End Devices",
            Category::Components => "Components",
            Category::Connections => "Connections",
            Category::Miscellaneous => "Miscellaneous",
        }
    }

    pub const ALL: [Category; 5] =
        [Category::NetworkDevices, Category::EndDevices, Category::Components, Category::Connections, Category::Miscellaneous];

    /// The subcategories, in the order the box lists them.
    pub fn subs(self) -> &'static [Sub] {
        match self {
            Category::NetworkDevices => &[Sub::Routers, Sub::Switches, Sub::Hubs, Sub::Wireless, Sub::Security, Sub::Wan],
            Category::EndDevices => &[Sub::Hosts, Sub::Home, Sub::SmartCity, Sub::Industrial, Sub::PowerGrid],
            Category::Components => &[Sub::Boards, Sub::Actuators, Sub::Sensors],
            Category::Connections => &[Sub::Cables, Sub::Cabling],
            Category::Miscellaneous => &[Sub::Recent],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sub {
    #[default]
    Routers,
    Switches,
    Hubs,
    /// Access points, controllers, home routers, the cell tower and the central office.
    Wireless,
    Security,
    /// The cloud and the modems.
    Wan,
    /// PCs, laptops, servers, printers, phones, tablets, smartphones, a sniffer.
    Hosts,
    Home,
    SmartCity,
    Industrial,
    PowerGrid,
    Boards,
    Actuators,
    Sensors,
    Cables,
    /// Patch panels and wall mounts: the passive devices of a structured cabling run.
    Cabling,
    /// The devices placed most recently.
    Recent,
}

impl Sub {
    /// The name the interface shows (and its tooltip).
    pub fn label(self) -> &'static str {
        match self {
            Sub::Routers => "Routers",
            Sub::Switches => "Switches",
            Sub::Hubs => "Hubs",
            Sub::Wireless => "Wireless Devices",
            Sub::Security => "Security",
            Sub::Wan => "WAN Emulation",
            Sub::Hosts => "End Devices",
            Sub::Home => "Home",
            Sub::SmartCity => "Smart City",
            Sub::Industrial => "Industrial",
            Sub::PowerGrid => "Power Grid",
            Sub::Boards => "Boards",
            Sub::Actuators => "Actuators",
            Sub::Sensors => "Sensors",
            Sub::Cables => "Connections",
            Sub::Cabling => "Structured Cabling",
            Sub::Recent => "Recently Used",
        }
    }

    /// The subcategory a model group is listed under.
    pub fn of_group(g: Group) -> Sub {
        match g {
            Group::Routers => Sub::Routers,
            Group::Switches => Sub::Switches,
            Group::Hubs => Sub::Hubs,
            Group::Wireless => Sub::Wireless,
            Group::Security => Sub::Security,
            Group::Wan => Sub::Wan,
            Group::EndDevices => Sub::Hosts,
            Group::Home => Sub::Home,
            Group::SmartCity => Sub::SmartCity,
            Group::Industrial => Sub::Industrial,
            Group::PowerGrid => Sub::PowerGrid,
            Group::Boards => Sub::Boards,
            Group::Actuators => Sub::Actuators,
            Group::Sensors => Sub::Sensors,
        }
    }
}

/// The most devices the Miscellaneous list remembers.
pub const RECENT_MAX: usize = 16;

#[derive(Debug, Clone, Default)]
pub struct BoxState {
    pub category: Category,
    pub sub: Sub,
    pub search: String,
    /// The PDU list is shown (Toggle PDU List Window).
    pub hide_pdus: bool,
    /// Model ids of the devices placed most recently, newest first (the Miscellaneous list).
    pub recent: Vec<&'static str>,
}

impl BoxState {
    /// Notes that a device of a model was placed (picked to place, or dropped).
    pub fn remember(&mut self, id: &'static str) {
        if models::find(id).is_none() {
            return;
        }
        self.recent.retain(|r| *r != id);
        self.recent.insert(0, id);
        self.recent.truncate(RECENT_MAX);
    }
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

/// Opens the device box on its Structured Cabling items (Ctrl+Alt+K).
pub fn open_cabling(app: &mut PacketApp) {
    let b = &mut app.ui.bottom;
    b.category = Category::Connections;
    b.sub = Sub::Cabling;
    b.search.clear();
}

/// Every model of the catalogue, plus one tile per cable kind.
fn tiles() -> Vec<Tile> {
    let mut t: Vec<Tile> = models::MODELS
        .iter()
        .map(|m| Tile {
            sub: Sub::of_group(m.group()),
            kind: Some(m.kind),
            tool: Tool::Place(m.id),
            short: m.label,
            long: m.description,
            multilayer: m.multilayer(),
        })
        .collect();
    t.extend(FittingKind::CABLING.iter().map(|&k| Tile {
        sub: Sub::Cabling,
        kind: None,
        tool: Tool::Place(k.id()),
        short: k.label(),
        multilayer: false,
        long: k.description(),
    }));
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

/// Shows a category and subcategory of the device box (the Ctrl+Alt+letter shortcuts).
pub fn show_category(app: &mut PacketApp, cat: Category, sub: Sub) {
    app.ui.bottom.category = cat;
    app.ui.bottom.sub = sub;
    app.ui.bottom.search.clear();
}

/// A category or subcategory button: a picture, with the name as tooltip.
fn kind_button(ui: &mut Ui, label: &str, selected: bool, draw: impl FnOnce(&egui::Painter, egui::Rect)) -> egui::Response {
    let tk = theme::tokens(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 36.0), Sense::click());
    let label = crate::i18n::tr(label);
    let resp = resp.on_hover_text(label);
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
        ui.allocate_ui_with_layout(vec2(292.0, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                let b = &mut app.ui.bottom;
                for cat in Category::ALL {
                    let first = cat.subs().first().copied().unwrap_or_default();
                    if kind_button(ui, cat.label(), b.category == cat && b.search.is_empty(), |p, r| crate::icons_box::category_icon(p, r, cat, &tk))
                        .clicked()
                    {
                        b.category = cat;
                        b.sub = first;
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
                    .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::TextInput, true, tl("Search for device")));
                });
            });
            ui.horizontal(|ui| {
                let b = &mut app.ui.bottom;
                for &sub in b.category.subs() {
                    let sel = b.sub == sub && b.search.is_empty();
                    if kind_button(ui, sub.label(), sel, |p, r| crate::icons_box::sub_icon(p, r, sub, &tk)).clicked() {
                        b.sub = sub;
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
            let hide_auto = app.ui.prefs.disable_auto_cable;
            let all: Vec<Tile> = tiles().into_iter().filter(|t| !(hide_auto && t.tool == Tool::Cable(Cable::Auto))).collect();
            let shown: Vec<&Tile> = if q.is_empty() && app.ui.bottom.sub == Sub::Recent {
                // Most recent first, one tile per model.
                app.ui.bottom.recent.iter().filter_map(|&id| all.iter().find(|t| t.tool == Tool::Place(id))).collect()
            } else {
                all.iter()
                    .filter(|m| {
                        if q.is_empty() {
                            m.sub == app.ui.bottom.sub
                        } else {
                            [m.short, m.long, crate::i18n::tl(m.short), crate::i18n::tl(m.long)].iter().any(|x| x.to_lowercase().contains(&q))
                        }
                    })
                    .collect()
            };
            // Rows of tiles that wrap, scrolled up and down, so every model of a group is in reach.
            let room = (ui.available_height() - 30.0).max(60.0);
            egui::ScrollArea::vertical().id_salt("model_tiles").max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if shown.is_empty() {
                        let none = if q.is_empty() && app.ui.bottom.sub == Sub::Recent { "No device placed yet." } else { "No device matches." };
                        ui.label(RichText::new(tl(none)).color(tk.muted));
                    }
                    for m in &shown {
                        let tool = m.tool;
                        let selected = app.ui.tool == tool;
                        // Room for a name on two lines (`Modular Router 3GE`).
                        let (rect, resp) = ui.allocate_exact_size(vec2(76.0, 56.0), Sense::click_and_drag());
                        // Cable names, model names and every description are interface text.
                        let (short, long) = (crate::i18n::tl(m.short), crate::i18n::tl(m.long));
                        let resp = resp.on_hover_text(long);
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, selected, short));
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
                            Some(k) => match tool {
                                Tool::Place(id) => crate::icons_families::model_icon(&p, icon, k, id, m.multilayer, &tk),
                                _ => icons::device(&p, icon, k, &tk),
                            },
                            None => {
                                if let Tool::Cable(c) = tool {
                                    icons::cable(&p, icon, c, &tk);
                                } else if let Tool::Place(id) = tool
                                    && let Some(k) = FittingKind::from_id(id)
                                {
                                    crate::closet::fitting_icon(&p, icon, k, &tk);
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
                            if let (false, Tool::Place(id)) = (selected, tool) {
                                app.ui.bottom.remember(id);
                            }
                        }
                        // A model or a passive device can also be dragged onto the workspace; a cable tile cannot.
                        if let Tool::Place(id) = tool {
                            if resp.drag_started() {
                                app.ui.tile_drag = Some((id, short));
                            }
                            if resp.drag_stopped() && app.ui.tile_drag.take().is_some() {
                                let at = ui.input(|i| i.pointer.latest_pos());
                                // Released outside the workspace: nothing is placed.
                                if let (Some(at), Some(area)) = (at, app.ui.canvas_rect)
                                    && area.contains(at)
                                {
                                    app.ui.tile_drop = Some((id, at));
                                    app.ui.bottom.remember(id);
                                }
                            }
                        }
                    }
                });
            });
            let current = all.iter().find(|m| m.tool == app.ui.tool).map_or("", |m| crate::i18n::tl(m.long));
            egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.canvas).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.vertical_centered(|ui| ui.add(egui::Label::new(RichText::new(current).small()).truncate()));
            });
        });
        ui.separator();

        // Scenario and PDU list.
        ui.allocate_ui_with_layout(vec2(196.0, ui.available_height()), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (r, info) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                let info = info.on_hover_text(tl("Scenarios hold the PDUs you send, so a lab can be re-tested"));
                info.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, tl("Scenario information")));
                ui.painter().circle_filled(r.center(), 15.0, tk.bar);
                ui.painter().circle_filled(r.center(), 10.0, egui::Color32::WHITE);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(15.0), tk.bar);
                // The scenario in use; the menu switches to another.
                let sc = &app.session.topology.scenarios;
                let (current, count) = (sc.current, sc.count());
                let mut pick = None;
                egui::ComboBox::from_id_salt("scenario")
                    .width(130.0)
                    .selected_text(RichText::new(crate::i18n::trf("Scenario {}", &[&current])).size(16.0))
                    .show_ui(ui, |ui| {
                        for i in 0..count {
                            if ui.selectable_label(i == current, crate::i18n::trf("Scenario {}", &[&i])).clicked() {
                                pick = Some(i);
                            }
                        }
                    })
                    .response
                    .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::ComboBox, true, tl("Scenario")));
                if let Some(i) = pick.filter(|i| *i != current) {
                    app.run("scenario.select", serde_json::json!({"index": i}));
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                if crate::navy_button(ui, "New").on_hover_text(tl("Start a new scenario with an empty PDU list")).clicked() {
                    app.run("scenario.new", serde_json::json!({}));
                }
                ui.add_space(18.0);
                if crate::navy_button(ui, "Delete").on_hover_text(tl("Delete this scenario and its PDUs")).clicked() {
                    app.run("scenario.delete", serde_json::json!({}));
                }
            });
            ui.add_space(6.0);
            if crate::navy_button_fit(ui, "Toggle PDU List Window").clicked() {
                app.ui.bottom.hide_pdus = !app.ui.bottom.hide_pdus;
            }
        });
        ui.separator();
        if app.ui.bottom.hide_pdus {
            if crate::navy_button(ui, "Fire All").on_hover_text(tl("Send every PDU in the list again")).clicked() {
                for i in 0..app.session.topology.scenarios.pdus().len() {
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
        // Tight columns so the header fits the box at the usual window width (it scrolls when not).
        ui.spacing_mut().item_spacing.x = 4.0;
        egui::Grid::new("pdu_grid").striped(true).num_columns(10).min_col_width(18.0).show(ui, |ui| {
            for h in ["Fire", "Last Status", "Source", "Destination", "Type", "Color", "Time(sec)", "Periodic", "Num", "Delete"] {
                ui.label(RichText::new(crate::i18n::tr(h)).size(12.0).strong());
            }
            ui.end_row();
            let t = &app.session.topology;
            for (i, pdu) in t.scenarios.pdus().iter().enumerate() {
                let name = |d| t.device(d).map_or("?".to_string(), |x| x.name.clone());
                let (status, color) = match pdu.status {
                    netlabcraft_model::scenario::PduStatus::Successful => ("Successful", tk.ok),
                    netlabcraft_model::scenario::PduStatus::Failed => ("Failed", tk.error),
                    netlabcraft_model::scenario::PduStatus::Waiting => ("Waiting", tk.muted),
                };
                let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, crate::i18n::trf("Fire PDU {}", &[&pdu.num])));
                ui.painter().circle_filled(r.center(), 6.0, color);
                if resp.on_hover_text(tl("Fire again")).clicked() {
                    fire = Some(i);
                }
                let status = ui.label(RichText::new(tl(status)).color(color));
                if !pdu.note.is_empty() {
                    status.on_hover_text(crate::i18n::tr_any(&pdu.note));
                }
                ui.label(name(pdu.source));
                let dest = match pdu.target {
                    Some(d) => format!("{} ({})", name(d), pdu.destination),
                    None if pdu.destination.is_empty() => tl("(broadcast)").to_string(),
                    None => pdu.destination.clone(),
                };
                ui.label(dest);
                ui.label(pdu.type_name());
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 14.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, canvas::pdu_color(pdu.num));
                ui.label(pdu.last_s.map_or("-".to_string(), |s| format!("{s:.3}")));
                ui.label(pdu.period_s.map_or_else(|| tl("No").to_string(), |p| crate::i18n::trf("every {} s", &[&p])));
                ui.label(pdu.num.to_string());
                let del = ui.small_button(tl("delete"));
                if del.clicked() {
                    delete = Some(pdu.num);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = fire {
        canvas::fire(app, i);
    }
    if let Some(num) = delete {
        app.run("pdu.delete", serde_json::json!({"num": num}));
    }
}

/// The tile being dragged from the device box, drawn under the pointer.
pub fn tile_ghost(app: &PacketApp, ctx: &egui::Context) {
    let Some((_, name)) = &app.ui.tile_drag else { return };
    let Some(at) = ctx.input(|i| i.pointer.latest_pos()) else { return };
    let tk = theme::tokens(app.ui.dark);
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("tile_ghost")));
    let rect = egui::Rect::from_center_size(at, vec2(96.0, 28.0));
    p.rect_filled(rect, 8.0, tk.pill.gamma_multiply(0.35));
    p.rect_stroke(rect, 8.0, egui::Stroke::new(1.5, tk.pill), egui::StrokeKind::Inside);
    p.text(rect.center(), egui::Align2::CENTER_CENTER, *name, egui::FontId::proportional(11.0), tk.text);
}
