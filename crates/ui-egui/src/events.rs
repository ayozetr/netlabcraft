//! The Simulation Panel: the event list (every frame that crossed a cable), play controls,
//! filters, and the PDU information window (layer by layer, plus the bytes).

use egui::{RichText, Ui};
use packetcraft_wire::{Arp, ArpOp, ETHERTYPE_ARP, ETHERTYPE_IPV4, Frame, Icmp, Ipv4, PROTO_ICMP, PROTO_UDP, Protocol, Udp};
use serde_json::json;

use crate::i18n::tl;
use crate::{PacketApp, theme};

const FILTERS: [Protocol; 30] = [
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
    Protocol::Other,
];

#[derive(Debug, Clone)]
pub struct EventView {
    pub selected: Option<u64>,
    pub playing: bool,
    step_started: f64,
    /// Seconds per hop while playing.
    pub step_seconds: f64,
    pub filter: [bool; 9],
    start_at: Option<u64>,
    scroll_to_selected: bool,
    /// The event whose PDU information window is open.
    pub info: Option<u64>,
    show_filters: bool,
}

impl Default for EventView {
    fn default() -> Self {
        EventView {
            selected: None,
            playing: false,
            step_started: 0.0,
            step_seconds: 0.8,
            filter: [true; 9],
            start_at: None,
            scroll_to_selected: false,
            info: None,
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
    let list_h = (ui.available_height() - 250.0).max(140.0);
    let row_h = 26.0;
    let cols = [("Vis.", 70.0), ("Time(sec)", 80.0), ("Last Device", 105.0), ("At Device", 105.0), ("Type", 55.0), ("Info", 40.0)];
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
                        ui.painter().text(
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
                        let summary = e.summary.clone();
                        let last = t.device(e.from.device).map_or("--".to_string(), |d| d.display_name().to_string());
                        let at = t.device(e.to.device).map_or("--".to_string(), |d| d.display_name().to_string());
                        let cells = [
                            if shown { "Visible".to_string() } else { String::new() },
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
                        for (c, (_, w)) in cells.iter().zip(cols.iter()) {
                            let align =
                                if c.parse::<f64>().is_ok() || c == "Visible" { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER };
                            let pos = if align == egui::Align2::RIGHT_CENTER {
                                egui::pos2(x + w - 6.0, rect.center().y)
                            } else {
                                egui::pos2(x + 6.0, rect.center().y)
                            };
                            p.text(pos, align, c, egui::FontId::proportional(15.0), text);
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
    ui.separator();
    ui.label(RichText::new(tl("Event List Filters - Visible Events")).size(16.0));
    let visible: Vec<&str> =
        FILTERS.iter().enumerate().filter(|(i, _)| app.ui.events.filter.get(*i).copied().unwrap_or(true)).map(|(_, p)| p.name()).collect();
    ui.label(if visible.is_empty() { "(none)".to_string() } else { visible.join(", ") });
    if app.ui.events.show_filters {
        ui.horizontal_wrapped(|ui| {
            for (i, p) in FILTERS.iter().enumerate() {
                if let Some(f) = app.ui.events.filter.get_mut(i) {
                    ui.checkbox(f, RichText::new(p.name()).color(theme::protocol_color(*p)));
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
            app.ui.events.filter = [!all; 9];
        }
    });
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
    let tk = theme::tokens(app.ui.dark);
    let mut open = true;
    egui::Window::new(format!("PDU Information at Device: {at}")).id(egui::Id::unique("pdu_info")).open(&mut open).default_size([460.0, 420.0]).show(
        ctx,
        |ui| {
            ui.label(crate::i18n::trf(
                "Event {} at {} s, from {} to {}",
                &[&e.id, &format!("{:.6}", e.at_us as f64 / 1e6), &t.end_label(e.from), &t.end_label(e.to)],
            ));
            match &e.dropped {
                Some(why) => ui.label(RichText::new(crate::i18n::trf("The device dropped it: {}", &[&crate::i18n::tr_any(why)])).color(tk.error)),
                None => ui.label(RichText::new(tl("The device accepted it.")).color(tk.ok)),
            };
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (layer, fields) in decode(&e.frame) {
                    egui::CollapsingHeader::new(RichText::new(layer.clone()).strong()).id_salt(("layer", id, layer)).default_open(true).show(
                        ui,
                        |ui| {
                            egui::Grid::new(("fields", id, fields.first().map(|f| f.0.clone()))).num_columns(2).striped(true).show(ui, |ui| {
                                for (k, v) in fields {
                                    ui.label(k);
                                    ui.monospace(v);
                                    ui.end_row();
                                }
                            });
                        },
                    );
                }
                egui::CollapsingHeader::new("Bytes").id_salt(("hex", id)).default_open(false).show(ui, |ui| {
                    ui.monospace(hex_dump(&e.frame));
                });
            });
        },
    );
    if !open {
        app.ui.events.info = None;
    }
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
