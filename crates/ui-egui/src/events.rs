//! The Simulation Panel: the event list (every frame that crossed a cable), play controls,
//! filters, and the PDU information window (layer by layer, plus the bytes).

use egui::{RichText, Ui};
use netlabcraft_wire::{Arp, ArpOp, ETHERTYPE_ARP, ETHERTYPE_IPV4, Frame, Icmp, Ipv4, PROTO_ICMP, PROTO_UDP, Protocol, Udp};
use serde_json::json;

use crate::i18n::tl;
use crate::{PacketApp, theme};

const FILTERS: [Protocol; 39] = [
    Protocol::Arp,
    Protocol::Icmp,
    Protocol::Dhcp,
    Protocol::Dns,
    Protocol::Http,
    Protocol::Tcp,
    Protocol::Udp,
    Protocol::Ipv4,
    Protocol::Stp,
    Protocol::Ospf,
    Protocol::Rip,
    Protocol::Eigrp,
    Protocol::Hsrp,
    Protocol::Lacp,
    Protocol::Radius,
    Protocol::Ftp,
    Protocol::Smtp,
    Protocol::Pop3,
    Protocol::Tacacs,
    Protocol::Snmp,
    Protocol::Lldp,
    Protocol::Ntp,
    Protocol::Syslog,
    Protocol::Tftp,
    Protocol::Icmpv6,
    Protocol::Ndp,
    Protocol::Ipv6,
    Protocol::Gre,
    Protocol::Esp,
    Protocol::Ah,
    Protocol::Isakmp,
    Protocol::Wlan,
    Protocol::Eapol,
    Protocol::Capwap,
    Protocol::Sip,
    Protocol::Tls,
    Protocol::Ssh,
    Protocol::Cell,
    Protocol::Other,
];

#[derive(Debug, Clone)]
pub struct EventView {
    pub selected: Option<u64>,
    pub playing: bool,
    step_started: f64,
    /// Seconds per hop while playing.
    pub step_seconds: f64,
    pub filter: [bool; FILTERS.len()],
    start_at: Option<u64>,
    scroll_to_selected: bool,
    /// The event whose PDU information window is open.
    pub info: Option<u64>,
    /// Its view: the OSI model, the inbound PDU or the outbound one.
    pub info_tab: InfoTab,
    show_filters: bool,
}

/// The views of the PDU information window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InfoTab {
    #[default]
    Osi,
    Inbound,
    Outbound,
}

impl Default for EventView {
    fn default() -> Self {
        EventView {
            selected: None,
            playing: false,
            step_started: 0.0,
            step_seconds: 0.8,
            filter: [true; FILTERS.len()],
            start_at: None,
            scroll_to_selected: false,
            info: None,
            info_tab: InfoTab::Osi,
            show_filters: false,
        }
    }
}

impl EventView {
    fn visible(&self, p: Protocol) -> bool {
        FILTERS.iter().position(|f| *f == p).and_then(|i| self.filter.get(i)).copied().unwrap_or(true)
    }

    /// How far along its cable the selected frame is drawn (0 = sender, 1 = receiver).
    pub fn progress(&self, now: f64) -> f32 {
        if self.playing { (((now - self.step_started) / self.step_seconds.max(0.05)) as f32).clamp(0.0, 1.0) } else { 0.5 }
    }

    /// Plays from the first event with an id ≥ `id`.
    pub fn start_from(&mut self, id: u64) {
        self.start_at = Some(id);
        self.playing = true;
    }
}

fn advance(app: &mut PacketApp, now: f64) {
    let trace = app.session.sim.trace();
    let v = &mut app.ui.events;
    if let Some(start) = v.start_at.take() {
        v.selected = trace.iter().find(|e| e.id >= start && v.visible(e.protocol)).map(|e| e.id);
        v.step_started = now;
        v.scroll_to_selected = true;
        v.playing = v.selected.is_some();
    }
    if !v.playing || now - v.step_started < v.step_seconds {
        return;
    }
    let cur = v.selected.unwrap_or(0);
    match trace.iter().find(|e| e.id > cur && v.visible(e.protocol)) {
        Some(e) => {
            v.selected = Some(e.id);
            v.step_started = now;
            v.scroll_to_selected = true;
        }
        None => v.playing = false,
    }
}

fn step(app: &mut PacketApp, forward: bool) {
    app.ui.events.playing = false;
    let cur = app.ui.events.selected;
    let v = &app.ui.events;
    let trace = app.session.sim.trace();
    let next = if forward {
        trace.iter().find(|e| cur.is_none_or(|c| e.id > c) && v.visible(e.protocol))
    } else {
        trace.iter().rev().find(|e| cur.is_some_and(|c| e.id < c) && v.visible(e.protocol))
    };
    if let Some(e) = next {
        app.ui.events.selected = Some(e.id);
        app.ui.events.scroll_to_selected = true;
    }
}

/// The three play buttons (back, play/pause, forward), shared by the panel and the time bar.
pub fn play_controls(app: &mut PacketApp, ui: &mut Ui, size: f32) {
    let playing = app.ui.events.playing;
    if crate::play_button(ui, crate::PlayGlyph::Back, "Back", size).clicked() {
        step(app, false);
    }
    let (g, label) = if playing { (crate::PlayGlyph::Pause, "Pause") } else { (crate::PlayGlyph::Play, "Play: replay the frames on the workspace") };
    if crate::play_button(ui, g, label, size).clicked() {
        if playing {
            app.ui.events.playing = false;
        } else {
            let from = app.ui.events.selected.map_or(0, |s| s + 1);
            let from = if app.session.sim.trace().iter().any(|e| e.id >= from) { from } else { 0 };
            app.ui.events.start_from(from);
        }
    }
    if crate::play_button(ui, crate::PlayGlyph::Forward, "Forward", size).clicked() {
        step(app, true);
    }
}

/// The event list's columns and their widths: each as wide as its heading, the two device
/// columns sharing what is left of `width` (the list scrolls sideways only when even their
/// least doesn't fit).
fn columns(ui: &Ui, width: f32) -> [(&'static str, f32); 6] {
    let text = |s: &str| ui.painter().layout_no_wrap(crate::i18n::tr(s).to_string(), egui::FontId::proportional(15.0), egui::Color32::WHITE).size().x;
    let fit = |s: &str, least: f32| (text(s) + 14.0).max(least);
    let vis = fit("Vis.", text("Visible") + 14.0);
    let time = fit("Time(sec)", 64.0);
    let kind = fit("Type", text("ICMPv6") + 14.0);
    let info = fit("Info", 36.0);
    let device = ((width - vis - time - kind - info) / 2.0).max(fit("Last Device", 0.0).min(fit("At Device", 0.0)).min(90.0));
    [("Vis.", vis), ("Time(sec)", time), ("Last Device", device), ("At Device", device), ("Type", kind), ("Info", info)]
}

/// The height the simulation panel keeps under the event list for its controls and filters.
const BELOW_LIST: f32 = 316.0;

pub fn panel(app: &mut PacketApp, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    advance(app, now);
    if app.ui.events.playing {
        ui.ctx().request_repaint();
    }
    let tk = theme::tokens(app.ui.dark);
    ui.add_space(2.0);
    ui.label(RichText::new(tl("Simulation Panel")).size(16.0));
    ui.separator();
    ui.label(RichText::new(tl("Event List")).size(16.0));
    let rows: Vec<usize> = app.session.sim.trace().iter().enumerate().filter(|(_, e)| app.ui.events.visible(e.protocol)).map(|(i, _)| i).collect();
    let list_h = (ui.available_height() - BELOW_LIST).max(120.0);
    let row_h = 26.0;
    let cols = columns(ui, ui.available_width() - 2.0);
    egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.canvas).show(ui, |ui| {
        ui.set_min_height(list_h);
        ui.set_max_height(list_h);
        ui.set_min_width(ui.available_width());
        egui::ScrollArea::horizontal().id_salt("event_cols").show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (h, w) in cols {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::hover());
                        ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.0, tk.chrome_line), egui::StrokeKind::Inside);
                        ui.painter().with_clip_rect(r.shrink(1.0)).text(
                            r.left_center() + egui::vec2(6.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            crate::i18n::tr(h),
                            egui::FontId::proportional(15.0),
                            tk.text,
                        );
                    }
                });
                if rows.is_empty() {
                    ui.label(RichText::new(tl("No events yet. Add a simple PDU (envelope tool) or ping from a console.")).color(tk.muted));
                    return;
                }
                let scroll_to = app.ui.events.scroll_to_selected;
                app.ui.events.scroll_to_selected = false;
                let selected_id = app.ui.events.selected;
                egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("event_rows").show_rows(ui, row_h, rows.len(), |ui, range| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    for &i in rows.get(range).unwrap_or_default() {
                        let Some(e) = app.session.sim.trace().get(i) else { continue };
                        let t = &app.session.topology;
                        let selected = selected_id == Some(e.id);
                        let shown = selected_id.is_some_and(|s| e.id <= s);
                        let id = e.id;
                        let color = theme::protocol_color(e.protocol);
                        let dropped = e.dropped.clone();
                        let summary = crate::i18n::tr_event(&e.summary);
                        let last = t.device(e.from.device).map_or("--".to_string(), |d| d.display_name().to_string());
                        let at = t.device(e.to.device).map_or("--".to_string(), |d| d.display_name().to_string());
                        let cells = [
                            if shown { tl("Visible").to_string() } else { String::new() },
                            format!("{:.3}", e.at_us as f64 / 1e6),
                            last,
                            at,
                            e.protocol.name().to_string(),
                        ];
                        let width: f32 = cols.iter().map(|c| c.1).sum();
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::click());
                        let resp = resp.on_hover_text(match &dropped {
                            Some(why) => format!("{summary}\n{}", crate::i18n::trf("Dropped: {}", &[&crate::i18n::tr_any(why)])),
                            None => summary,
                        });
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Row, true, selected, format!("Event {id}")));
                        let p = ui.painter_at(rect);
                        let (fill, text) = if selected {
                            (tk.pill, egui::Color32::WHITE)
                        } else if i % 2 == 1 {
                            (tk.chrome_line.gamma_multiply(0.25), if dropped.is_some() { tk.error } else { tk.text })
                        } else {
                            (tk.canvas, if dropped.is_some() { tk.error } else { tk.text })
                        };
                        p.rect_filled(rect, 0.0, fill);
                        let mut x = rect.left();
                        for (k, (c, (_, w))) in cells.iter().zip(cols.iter()).enumerate() {
                            // Visible and the time sit right, the names and the type left; each
                            // clipped to its column.
                            let align = if k < 2 { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER };
                            let pos = if align == egui::Align2::RIGHT_CENTER {
                                egui::pos2(x + w - 6.0, rect.center().y)
                            } else {
                                egui::pos2(x + 6.0, rect.center().y)
                            };
                            let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, row_h)).shrink2(egui::vec2(2.0, 0.0));
                            p.with_clip_rect(cell.intersect(rect)).text(pos, align, c, egui::FontId::proportional(15.0), text);
                            x += w;
                        }
                        let info = egui::Rect::from_center_size(egui::pos2(x + 20.0, rect.center().y), egui::vec2(18.0, 14.0));
                        p.rect_filled(info, 1.0, color);
                        if resp.clicked() {
                            app.ui.events.selected = Some(id);
                            app.ui.events.playing = false;
                            if resp.interact_pointer_pos().is_some_and(|pt| info.expand(4.0).contains(pt)) {
                                app.ui.events.info = Some(id);
                            }
                        }
                        if resp.double_clicked() {
                            app.ui.events.info = Some(id);
                        }
                        if selected && scroll_to {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                });
            });
        });
    });
    egui::ScrollArea::vertical().id_salt("sim_controls").auto_shrink([false, true]).show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if crate::navy_button(ui, "Reset Simulation").on_hover_text(tl("Clear the event list")).clicked() {
                app.run("sim.clear_events", json!({}));
                app.ui.events.selected = None;
                app.ui.events.playing = false;
            }
            let state = if app.ui.events.playing {
                crate::i18n::tl("Playing...").to_string()
            } else {
                format!("{} {}", app.session.sim.trace().len(), crate::i18n::tl("events"))
            };
            ui.label(RichText::new(state).color(tk.muted));
        });
        ui.separator();
        ui.label(RichText::new(tl("Play Controls")).size(16.0));
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                let w = 3.0 * 44.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                play_controls(app, ui, 38.0);
            });
        });
        // Slow on the left, fast on the right, as a speed slider reads.
        let mut speed = 2.1 - app.ui.events.step_seconds;
        ui.spacing_mut().slider_width = ui.available_width() - 10.0;
        if ui.add(egui::Slider::new(&mut speed, 0.1..=2.0).show_value(false)).on_hover_text(tl("Play speed")).changed() {
            app.ui.events.step_seconds = 2.1 - speed;
        }
        // Constant delay: every hop takes the same simulated time (the engine keeps the option).
        let mut constant = app.session.sim.constant_delay().is_some();
        if ui
            .checkbox(&mut constant, tl("Constant Delay"))
            .on_hover_text(tl("Every hop takes the same simulated time (1 ms), whatever queues would add"))
            .changed()
        {
            app.run("sim.constant_delay", json!({"on": constant}));
        }
        ui.separator();
        ui.label(RichText::new(tl("Event List Filters - Visible Events")).size(16.0));
        let visible: Vec<&str> =
            FILTERS.iter().enumerate().filter(|(i, _)| app.ui.events.filter.get(*i).copied().unwrap_or(true)).map(|(_, p)| p.name()).collect();
        // Every protocol on is the usual case: one line rather than the whole list.
        ui.label(if visible.is_empty() {
            tl("(none)").to_string()
        } else if visible.len() == FILTERS.len() {
            tl("All protocols").to_string()
        } else {
            visible.join(", ")
        });
        let mut filters_changed = false;
        if app.ui.events.show_filters {
            ui.horizontal_wrapped(|ui| {
                for (i, p) in FILTERS.iter().enumerate() {
                    if let Some(f) = app.ui.events.filter.get_mut(i) {
                        filters_changed |= ui.checkbox(f, RichText::new(p.name()).color(theme::protocol_color(*p))).changed();
                    }
                }
            });
        }
        ui.columns(2, |cols| {
            if let Some(c) = cols.get_mut(0)
                && crate::navy_button_wide(c, "Edit Filters").clicked()
            {
                app.ui.events.show_filters = !app.ui.events.show_filters;
            }
            if let Some(c) = cols.get_mut(1)
                && crate::navy_button_wide(c, "Show All/None").clicked()
            {
                let all = app.ui.events.filter.iter().all(|f| *f);
                app.ui.events.filter = [!all; FILTERS.len()];
                filters_changed = true;
            }
        });
        // The filters can also decide what is recorded (and exported to pcap).
        let mut only = app.session.sim.capture_filter().is_some();
        if ui
            .checkbox(&mut only, tl("Capture only the visible events"))
            .on_hover_text(tl("Events of hidden protocols are neither recorded nor exported to pcap; turning it on also removes them from the list"))
            .changed()
        {
            set_capture(app, only, true);
        } else if filters_changed && only {
            set_capture(app, true, false);
        }
    });
}

/// Records only the visible protocols (`on`), or everything; `prune` also drops what the list
/// holds of the others.
fn set_capture(app: &mut PacketApp, on: bool, prune: bool) {
    let names: Vec<&str> =
        FILTERS.iter().enumerate().filter(|(i, _)| app.ui.events.filter.get(*i).copied().unwrap_or(true)).map(|(_, p)| p.name()).collect();
    let p = if on { json!({"on": true, "protocols": names, "prune": prune}) } else { json!({"on": false}) };
    app.run("sim.capture_filter", p);
    if prune && app.ui.events.selected.is_some_and(|s| !app.session.sim.trace().iter().any(|e| e.id == s)) {
        app.ui.events.selected = None;
    }
}

/// The PDU information window of one event.
pub fn pdu_window(app: &mut PacketApp, ctx: &egui::Context) {
    let Some(id) = app.ui.events.info else { return };
    let Some(e) = app.session.sim.trace().iter().find(|e| e.id == id).cloned() else {
        app.ui.events.info = None;
        return;
    };
    let t = &app.session.topology;
    let at = t.device(e.to.device).map_or("?".to_string(), |d| d.name.clone());
    let osi = netlabcraft_sim::explain::osi(t, &app.session.sim, &e);
    let out = osi.sent.first().and_then(|id| app.session.sim.trace().iter().find(|x| x.id == *id)).cloned();
    let tk = theme::tokens(app.ui.dark);
    let mut open = true;
    let mut tab = app.ui.events.info_tab;
    egui::Window::new(crate::i18n::trf("PDU Information at Device: {}", &[&at]))
        .id(egui::Id::unique("pdu_info"))
        .open(&mut open)
        .default_size([640.0, 480.0])
        .default_pos(crate::BELOW_TOOLBARS)
        .show(ctx, |ui| {
            ui.label(crate::i18n::trf(
                "Event {} at {} s, from {} to {}",
                &[&e.id, &format!("{:.6}", e.at_us as f64 / 1e6), &t.end_label(e.from), &t.end_label(e.to)],
            ));
            match &e.dropped {
                Some(why) => ui.label(RichText::new(crate::i18n::trf("The device dropped it: {}", &[&crate::i18n::tr_any(why)])).color(tk.error)),
                None => ui.label(RichText::new(tl("The device accepted it.")).color(tk.ok)),
            };
            ui.horizontal(|ui| {
                for (which, name) in
                    [(InfoTab::Osi, "OSI Model"), (InfoTab::Inbound, "Inbound PDU Details"), (InfoTab::Outbound, "Outbound PDU Details")]
                {
                    if ui.selectable_label(tab == which, tl(name)).clicked() {
                        tab = which;
                    }
                }
            });
            ui.separator();
            let sender = t.device(e.from.device).map_or("?".to_string(), |d| d.name.clone());
            egui::ScrollArea::vertical().show(ui, |ui| match tab {
                InfoTab::Osi => {
                    osi_view(ui, &osi, &tk);
                    origin_view(ui, &osi, &sender);
                }
                InfoTab::Inbound => layer_details(ui, id, &e.frame, e.air.as_deref()),
                InfoTab::Outbound => match &out {
                    Some(o) => {
                        ui.label(crate::i18n::trf("Event {}, from {} to {}", &[&o.id, &t.end_label(o.from), &t.end_label(o.to)]));
                        layer_details(ui, o.id, &o.frame, o.air.as_deref());
                    }
                    None => {
                        ui.label(RichText::new(tl("The device sent nothing on because of this frame.")).color(tk.muted));
                    }
                },
            });
        });
    app.ui.events.info_tab = tab;
    if !open {
        app.ui.events.info = None;
    }
}

/// The OSI model view: what the device did at each layer with the frame coming in and with
/// what it sent on, layer 7 at the top.
fn osi_view(ui: &mut Ui, osi: &netlabcraft_sim::explain::Osi, tk: &theme::Tokens) {
    let w = ((ui.available_width() - 24.0) / 2.0).max(160.0);
    egui::Grid::new("osi_grid").num_columns(2).striped(true).spacing(egui::vec2(12.0, 8.0)).show(ui, |ui| {
        for h in ["In Layers", "Out Layers"] {
            ui.label(RichText::new(tl(h)).strong());
        }
        ui.end_row();
        for i in (0..7).rev() {
            let name = netlabcraft_sim::explain::LAYER_NAMES.get(i).copied().unwrap_or_default();
            for (side, inbound) in [(&osi.inbound, true), (&osi.outbound, false)] {
                ui.vertical(|ui| {
                    ui.set_width(w);
                    let lines = side.get(i).map(Vec::as_slice).unwrap_or_default();
                    let drop = osi.dropped.as_ref().filter(|(l, _)| inbound && *l == i);
                    let muted = lines.is_empty() && drop.is_none();
                    ui.label(RichText::new(tl(name)).strong().color(if muted { tk.muted } else { tk.text }));
                    for l in lines {
                        ui.add(egui::Label::new(crate::i18n::tr_any(l)).wrap());
                    }
                    if let Some((_, why)) = drop {
                        ui.add(
                            egui::Label::new(
                                RichText::new(crate::i18n::trf("The device drops the frame: {}", &[&crate::i18n::tr_any(why)])).color(tk.error),
                            )
                            .wrap(),
                        );
                    }
                });
            }
            ui.end_row();
        }
    });
}

/// What the sender decided before putting the frame on the wire, when it raised the packet
/// itself (layer 7 at the top; only the layers that say something).
fn origin_view(ui: &mut Ui, osi: &netlabcraft_sim::explain::Osi, sender: &str) {
    if osi.origin.iter().all(Vec::is_empty) {
        return;
    }
    ui.separator();
    ui.label(RichText::new(crate::i18n::trf("What {} decided before sending it", &[&sender])).strong());
    for i in (0..7).rev() {
        let lines = osi.origin.get(i).map(Vec::as_slice).unwrap_or_default();
        if lines.is_empty() {
            continue;
        }
        let name = netlabcraft_sim::explain::LAYER_NAMES.get(i).copied().unwrap_or_default();
        ui.label(RichText::new(tl(name)).strong());
        for l in lines {
            ui.add(egui::Label::new(crate::i18n::tr_any(l)).wrap());
        }
    }
}

/// A frame layer by layer, plus its bytes.
fn layer_details(ui: &mut Ui, id: u64, frame: &[u8], air: Option<&[u8]>) {
    for (layer, fields) in decode_event(frame, air) {
        egui::CollapsingHeader::new(RichText::new(crate::i18n::tr(&layer)).strong()).id_salt(("layer", id, layer)).default_open(true).show(
            ui,
            |ui| {
                egui::Grid::new(("fields", id, fields.first().map(|f| f.0.clone()))).num_columns(2).striped(true).show(ui, |ui| {
                    for (k, v) in fields {
                        ui.label(crate::i18n::tr(&k));
                        ui.monospace(crate::i18n::tr_event(&v));
                        ui.end_row();
                    }
                });
            },
        );
    }
    egui::CollapsingHeader::new(tl("Bytes")).id_salt(("hex", id)).default_open(false).show(ui, |ui| {
        ui.monospace(hex_dump(air.unwrap_or(frame)));
    });
}

/// Layers of an event: the 802.11 frame first when it crossed the air, then what it carries.
pub fn decode_event(frame: &[u8], air: Option<&[u8]>) -> Vec<(String, Vec<(String, String)>)> {
    let Some(air) = air else { return decode(frame) };
    let mut out = match netlabcraft_wire::wlan::Dot11::decode(air) {
        Ok(f) => vec![("IEEE 802.11".to_string(), f.fields())],
        Err(e) => vec![("IEEE 802.11".to_string(), vec![("Error".into(), e.to_string()), ("Length".into(), air.len().to_string())])],
    };
    if !frame.is_empty() {
        let mut inner = decode(frame);
        if let Some((title, _)) = inner.first_mut() {
            *title = "Ethernet II (equivalent)".to_string();
        }
        out.extend(inner);
    }
    out
}

/// Layers of a frame as (title, fields).
pub fn decode(bytes: &[u8]) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let Ok(f) = Frame::decode(bytes) else {
        out.push(("Malformed frame".to_string(), vec![("Length".into(), bytes.len().to_string())]));
        return out;
    };
    let mut eth = vec![("Destination".to_string(), f.dst.to_string()), ("Source".into(), f.src.to_string())];
    if let Some(v) = f.vlan {
        eth.push(("802.1Q VLAN".into(), v.to_string()));
    }
    eth.push(("Type".into(), format!("0x{:04x}", f.ethertype)));
    eth.push(("Length".into(), format!("{} bytes", bytes.len())));
    out.push(("Ethernet II".to_string(), eth));
    match f.ethertype {
        ETHERTYPE_ARP => {
            if let Ok(a) = Arp::decode(&f.payload) {
                out.push((
                    "ARP".into(),
                    vec![
                        ("Operation".into(), if a.op == ArpOp::Request { "request (1)" } else { "reply (2)" }.into()),
                        ("Sender MAC".into(), a.sender_mac.to_string()),
                        ("Sender IP".into(), a.sender_ip.to_string()),
                        ("Target MAC".into(), a.target_mac.to_string()),
                        ("Target IP".into(), a.target_ip.to_string()),
                    ],
                ));
            }
        }
        ETHERTYPE_IPV4 => {
            if let Ok(ip) = Ipv4::decode(&f.payload) {
                out.push((
                    "IPv4".into(),
                    vec![
                        ("Source".into(), ip.src.to_string()),
                        ("Destination".into(), ip.dst.to_string()),
                        ("TTL".into(), ip.ttl.to_string()),
                        ("Protocol".into(), ip.protocol.to_string()),
                        ("Identification".into(), format!("0x{:04x}", ip.id)),
                        ("Payload".into(), format!("{} bytes", ip.payload.len())),
                    ],
                ));
                if ip.protocol == PROTO_ICMP
                    && let Ok(icmp) = Icmp::decode(&ip.payload)
                {
                    let fields = match icmp {
                        Icmp::EchoRequest { id, seq, data } | Icmp::EchoReply { id, seq, data } => {
                            let kind = if ip.payload.first() == Some(&8) { "echo request (8)" } else { "echo reply (0)" };
                            vec![
                                ("Type".into(), kind.to_string()),
                                ("Identifier".into(), id.to_string()),
                                ("Sequence".into(), seq.to_string()),
                                ("Data".into(), format!("{} bytes", data.len())),
                            ]
                        }
                        Icmp::Unreachable { code, original } => vec![
                            ("Type".into(), "destination unreachable (3)".into()),
                            ("Code".into(), code.to_string()),
                            ("Quoted".into(), format!("{} bytes", original.len())),
                        ],
                        Icmp::TimeExceeded { original } => {
                            vec![("Type".into(), "time exceeded (11)".into()), ("Quoted".into(), format!("{} bytes", original.len()))]
                        }
                    };
                    out.push(("ICMP".into(), fields));
                }
                if ip.protocol == PROTO_UDP
                    && let Ok(u) = Udp::decode(&ip.payload, ip.src, ip.dst)
                {
                    out.push((
                        "UDP".into(),
                        vec![
                            ("Source port".into(), u.src_port.to_string()),
                            ("Destination port".into(), u.dst_port.to_string()),
                            ("Payload".into(), format!("{} bytes", u.payload.len())),
                        ],
                    ));
                }
            }
        }
        netlabcraft_wire::wlan::ETHERTYPE_EAPOL => {
            use netlabcraft_wire::wlan::Eapol;
            let fields = match Eapol::decode(&f.payload) {
                Ok(Eapol::Key(k)) => vec![
                    ("Packet".to_string(), format!("EAPOL-Key, handshake message {} of 4", k.message())),
                    ("Key information".into(), format!("0x{:04x}", k.info)),
                    ("Replay counter".into(), k.replay.to_string()),
                    ("Nonce".into(), k.nonce.iter().take(8).map(|b| format!("{b:02x}")).collect::<String>() + "…"),
                    ("MIC".into(), k.mic.iter().map(|b| format!("{b:02x}")).collect()),
                ],
                Ok(e) => vec![("Packet".to_string(), e.describe())],
                Err(e) => vec![("Error".to_string(), e.to_string())],
            };
            out.push(("IEEE 802.1X (EAPOL)".into(), fields));
        }
        _ => {}
    }
    out
}

pub fn hex_dump(b: &[u8]) -> String {
    b.chunks(16)
        .enumerate()
        .map(|(i, c)| {
            let hex: Vec<String> = c.iter().map(|x| format!("{x:02x}")).collect();
            let ascii: String = c.iter().map(|&x| if x.is_ascii_graphic() { x as char } else { '.' }).collect();
            format!("{:04x}  {:<47}  {ascii}", i * 16, hex.join(" "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
