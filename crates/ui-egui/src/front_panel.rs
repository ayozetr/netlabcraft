//! The front panel of a device window's Physical tab: the device's face with its model name, power
//! light, screws and vents, and one real connector for every port (an RJ45 jack with its latch and
//! link and activity lights, an SFP cage with its labelled tab, a serial D-shell, a coaxial F
//! connector, a phone jack, a radio mark for a port that takes no cable), so ports can be told apart
//! by kind. The connectors are the ones the wiring closet draws ([`crate::icons_ports`]), at the
//! larger size of this view. Pointing at a port, or clicking or activating it from the keyboard,
//! shows what is plugged in.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use netlabcraft_model::{Device, DeviceKind, End, Topology, ifname};
use netlabcraft_sim::port_up;

use crate::PacketApp;
use crate::i18n::{tl, trf};
use crate::icons_ports::{self, Connector};
use crate::theme::{self, Tokens, shade};

/// The room left of the first port and right of the last one.
const MARGIN: f32 = 24.0;
/// The header band: power light, model name, vents and the power inlet.
const HEADER: f32 = 44.0;
/// A port's cell: its lights, its connector, the cable and PoE marks and its label.
const CELL_H: f32 = 80.0;
const CELL_W: f32 = 52.0;
/// The extra room between two groups of different connectors on a row.
const GAP: f32 = 14.0;

/// Where each port's cell sits (relative to the panel's top left) in a panel `width` wide, and the
/// panel's height. Ports keep their order, wrap to a new row when a row is full, and a change of
/// connector leaves a gap.
pub(crate) fn layout(conns: &[Connector], width: f32) -> (Vec<Rect>, f32) {
    let right = width - MARGIN;
    let (mut x, mut y) = (MARGIN, HEADER);
    let mut prev: Option<Connector> = None;
    let mut cells = Vec::with_capacity(conns.len());
    for c in conns {
        let w = CELL_W.max(icons_ports::panel_size(*c).x + 8.0);
        let gap = if x > MARGIN && prev.is_some_and(|p| p != *c) { GAP } else { 0.0 };
        if x > MARGIN && x + gap + w > right {
            x = MARGIN;
            y += CELL_H;
        } else {
            x += gap;
        }
        cells.push(Rect::from_min_size(pos2(x, y), vec2(w, CELL_H)));
        x += w;
        prev = Some(*c);
    }
    let rows_end = if conns.is_empty() { HEADER } else { y + CELL_H };
    (cells, rows_end + 14.0)
}

/// The connector of each of a device's ports.
pub(crate) fn connectors(dev: &Device) -> Vec<Connector> {
    dev.ports.iter().enumerate().map(|(i, p)| icons_ports::panel_connector(&p.name, dev.port_medium(i))).collect()
}

/// The short label under a port (`Gi0/1`, `Se0/0/0`, `Ce2` for a cellular radio).
fn short_name(name: &str) -> String {
    match name.strip_prefix("Cellular") {
        Some(rest) => format!("Ce{rest}"),
        None => ifname::short(name),
    }
}

/// What a port says about itself, in words: its name, its link and what it is cabled to.
fn port_line(t: &Topology, dev: &Device, i: usize) -> String {
    let Some(port) = dev.ports.get(i) else { return String::new() };
    let end = End { device: dev.id, port: i };
    let peer = t.peer(end).map_or("nothing".to_string(), |e| t.end_label(e));
    trf("{}: {} · cabled to {}", &[&port.name, &tl(if port_up(t, end) { "link up" } else { "link down" }), &peer])
}

/// Draws the front panel and handles pointing, clicking and the keyboard on its ports.
pub(crate) fn show(app: &PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    let t = &app.session.topology;
    let conns = connectors(dev);
    // As wide as the ports need (a PC with one port is not a rack), up to the window's width.
    let needed = conns.iter().map(|c| CELL_W.max(icons_ports::panel_size(*c).x + 8.0) + GAP).sum::<f32>() + 2.0 * MARGIN;
    let width = ui.available_width().clamp(340.0, 760.0).min(needed.max(440.0));
    let (cells, height) = layout(&conns, width);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let p = ui.painter_at(rect);
    let body = crate::closet::body_color(dev);
    let ink = theme::readable_on(body);
    chassis(&p, dev, rect, body, ink, &tk);

    let sel_id = ui.make_persistent_id(("front_panel_port", dev.id.0));
    let mut selected: Option<usize> = ui.data(|d| d.get_temp::<usize>(sel_id)).filter(|i| *i < dev.ports.len());
    // PoE switches: a bar under each port that can give power, lit while it powers a device.
    let poe = netlabcraft_model::poe::budget_mw(dev.catalogue_model()).map(|_| t.power_inline(dev.id));
    let (mut hovered, mut clicked) = (None, None);
    for (i, (port, conn)) in dev.ports.iter().zip(&conns).enumerate() {
        let Some(rel) = cells.get(i) else { continue };
        let cell = rel.translate(rect.min.to_vec2());
        let end = End { device: dev.id, port: i };
        let cabled = t.peer(end).is_some();
        let up = port_up(t, end);
        let cable = t.link_at(end).map(|l| crate::closet::cable_color(t.resolved_cable(l), &tk));
        let resp = ui.interact(cell, ui.make_persistent_id(("front_port", dev.id.0, i)), Sense::click());
        let label = port_line(t, dev, i);
        let is_selected = selected == Some(i);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, is_selected, label.clone()));
        if resp.hovered() {
            hovered = Some(i);
        }
        if resp.clicked() {
            clicked = Some(i);
        }

        // The connector in a recessed bezel, the lights over it.
        let size = icons_ports::panel_size(*conn);
        let jack = Rect::from_center_size(pos2(cell.center().x, cell.top() + 31.0), size);
        let bezel =
            Rect::from_min_max(pos2(jack.left() - 5.0, (jack.top() - 12.0).max(cell.top() + 2.0)), pos2(jack.right() + 5.0, jack.bottom() + 6.0));
        p.rect_filled(bezel, 3.0, shade(body, 0.42));
        p.rect_stroke(bezel, 3.0, Stroke::new(1.0, shade(body, 0.3)), StrokeKind::Inside);
        icons_ports::draw(&p, *conn, jack, cabled, &tk);
        let off = icons_ports::off_light(&tk);
        let link = if up {
            tk.link_up
        } else if cabled {
            tk.port_activity
        } else {
            off
        };
        if matches!(conn, Connector::Rj45 | Connector::Combo | Connector::Sfp) {
            icons_ports::lights(&p, jack, link, if up { tk.port_activity } else { off }, 3.0);
        } else {
            p.circle_filled(jack.center_top() - vec2(0.0, 7.0), 2.8, link);
        }
        // The cable plugged in: a bar in its kind's colour; PoE: a bar that is lit while it powers a device.
        if let Some(colour) = cable {
            p.rect_filled(Rect::from_center_size(pos2(cell.center().x, cell.top() + 57.0), vec2(28.0, 3.5)), 1.5, colour);
        }
        if let Some(rows) = &poe
            && port.name.starts_with("FastEthernet")
        {
            let on = rows.iter().any(|r| r.port == i && r.device.is_some());
            let colour = if on { Color32::from_rgb(0xf2, 0xc2, 0x3a) } else { Color32::from_rgb(0x6a, 0x5a, 0x2a) };
            p.rect_filled(Rect::from_center_size(pos2(cell.center().x, cell.top() + 62.0), vec2(28.0, 3.0)), 1.0, colour);
        }
        p.text(pos2(cell.center().x, cell.top() + 72.0), Align2::CENTER_CENTER, short_name(&port.name), FontId::proportional(10.5), ink);
        // The ring: the picked port, the one with the keyboard focus, a lighter one under the pointer.
        let ring = bezel.expand(2.0);
        if is_selected || resp.has_focus() {
            p.rect_stroke(ring, 4.0, Stroke::new(2.0, tk.selection), StrokeKind::Outside);
        } else if resp.hovered() {
            p.rect_stroke(ring, 4.0, Stroke::new(1.5, tk.selection.gamma_multiply(0.6)), StrokeKind::Outside);
        }
    }
    if dev.powered_off {
        p.rect_filled(rect, 6.0, Color32::from_black_alpha(80));
    }
    if let Some(i) = clicked {
        selected = if selected == Some(i) { None } else { Some(i) };
        ui.data_mut(|d| {
            if let Some(v) = selected {
                d.insert_temp(sel_id, v);
            } else {
                d.remove_temp::<usize>(sel_id);
            }
        });
        ui.ctx().request_repaint();
    }
    match hovered.or(selected) {
        Some(i) => {
            ui.label(port_line(t, dev, i));
        }
        None => {
            ui.label(RichText::new(tl("Point at a port to see what is plugged in. Cables are made with the Connections tool.")).color(tk.muted));
        }
    }
}

/// The device's face: body, lit top edge, foot, screws, the power light and the model's name, vents
/// and the power inlet on the kinds that have one.
fn chassis(p: &egui::Painter, dev: &Device, rect: Rect, body: Color32, ink: Color32, tk: &Tokens) {
    let round = 6.0;
    let edge = shade(body, 0.5);
    p.rect_filled(rect, round, body);
    p.line_segment([pos2(rect.left() + round, rect.top() + 1.5), pos2(rect.right() - round, rect.top() + 1.5)], Stroke::new(1.5, shade(body, 1.3)));
    p.rect_filled(
        Rect::from_min_max(pos2(rect.left() + 1.0, rect.bottom() - 6.0), pos2(rect.right() - 1.0, rect.bottom() - 1.0)),
        0.0,
        shade(body, 0.82),
    );
    p.rect_stroke(rect, round, Stroke::new(1.0, edge), StrokeKind::Inside);
    if dev.catalogue_model().id.starts_with("Firewall") {
        p.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 4.0)), 0.0, Color32::from_rgb(0xc8, 0x3a, 0x2e));
    }
    // Four screws.
    for c in [
        rect.left_top() + vec2(10.0, 12.0),
        rect.right_top() + vec2(-10.0, 12.0),
        rect.left_bottom() + vec2(10.0, -14.0),
        rect.right_bottom() + vec2(-10.0, -14.0),
    ] {
        p.circle_filled(c, 3.8, shade(body, 1.2));
        p.circle_stroke(c, 3.8, Stroke::new(0.8, edge));
        p.line_segment([c + vec2(-2.3, 1.1), c + vec2(2.3, -1.1)], Stroke::new(1.0, edge));
    }
    // The power light and the model's name.
    let led = rect.left_top() + vec2(36.0, 24.0);
    p.circle_filled(led, 4.5, if dev.powered_off { Color32::from_rgb(0x2a, 0x2a, 0x2a) } else { tk.link_up });
    p.circle_stroke(led, 4.5, Stroke::new(0.8, edge));
    p.text(led + vec2(10.0, 0.0), Align2::LEFT_CENTER, "PWR", FontId::proportional(10.0), ink);
    p.text(led + vec2(46.0, 0.0), Align2::LEFT_CENTER, tl(dev.catalogue_model().label), FontId::proportional(13.0), ink);
    // Vents and the power inlet.
    let inlet = icons_ports::has_inlet(dev.kind);
    let mut right = rect.right() - MARGIN;
    if inlet {
        let r = Rect::from_center_size(pos2(right - 17.0, rect.top() + 24.0), icons_ports::panel_size(Connector::Inlet));
        p.rect_filled(r.expand(4.0), 3.0, shade(body, 0.42));
        icons_ports::draw(p, Connector::Inlet, r, false, tk);
        right = r.left() - 12.0;
    }
    let vented = inlet || dev.catalogue_model().id.starts_with("Firewall") || dev.kind == DeviceKind::Hub;
    if vented && rect.width() >= 460.0 {
        for k in 0..7 {
            let slit = Rect::from_center_size(pos2(right - k as f32 * 7.0, rect.top() + 24.0), vec2(2.6, 16.0));
            p.rect_filled(slit, 1.0, shade(body, 0.45));
        }
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use netlabcraft_model::models::MODELS;
    use serde_json::json;

    use super::*;

    /// A session with one device of every catalogue model, and their ids in order.
    fn every_model() -> Session {
        let mut s = Session::new();
        for (n, m) in MODELS.iter().enumerate() {
            let _ = s.execute("device.add", json!({"model": m.id, "name": format!("D{n}"), "x": 0.0, "y": 0.0}));
        }
        s
    }

    #[test]
    fn every_port_of_every_model_has_a_cell_inside_the_panel_at_every_width() {
        let s = every_model();
        assert!(s.topology.devices.len() > 100, "the catalogue was added");
        for dev in &s.topology.devices {
            let conns = connectors(dev);
            assert_eq!(conns.len(), dev.ports.len());
            for width in [340.0, 520.0, 640.0, 760.0] {
                let (cells, height) = layout(&conns, width);
                assert_eq!(cells.len(), conns.len());
                for (i, c) in cells.iter().enumerate() {
                    assert!(
                        c.left() >= 0.0 && c.right() <= width && c.top() >= 0.0 && c.bottom() <= height,
                        "{} port {i} leaves the panel",
                        dev.name
                    );
                    assert!(c.width() >= icons_ports::panel_size(conns[i]).x, "the cell holds its connector");
                    for other in cells.iter().skip(i + 1) {
                        assert!(!c.shrink(0.5).intersects(other.shrink(0.5)), "{} has two ports on top of each other", dev.name);
                    }
                }
            }
        }
    }

    #[test]
    fn a_port_is_drawn_as_the_connector_its_name_and_medium_say_in_every_model() {
        let s = every_model();
        for dev in &s.topology.devices {
            for (i, port) in dev.ports.iter().enumerate() {
                let conn = icons_ports::panel_connector(&port.name, dev.port_medium(i));
                let medium = dev.port_medium(i).unwrap_or_else(|| netlabcraft_model::models::medium_by_name(&port.name));
                use netlabcraft_model::cable::Medium as M;
                let expected = match medium {
                    _ if netlabcraft_model::cable::is_usb_port_name(&port.name) => Connector::Usb,
                    M::Wireless => Connector::Radio,
                    M::Copper => Connector::Rj45,
                    M::Combo => Connector::Combo,
                    M::Fiber => Connector::Sfp,
                    M::Console => Connector::Console,
                    M::Rs232 => Connector::Rs232,
                    M::Serial => Connector::Serial,
                    M::Coaxial => Connector::Coax,
                    M::Phone => Connector::Rj11,
                };
                assert_eq!(conn, expected, "{} {}", dev.name, port.name);
            }
        }
        // A few families, by what they must show.
        let kinds = |model: &str| -> Vec<Connector> {
            let d = s.topology.devices.iter().find(|d| d.catalogue_model().id == model).unwrap();
            connectors(d)
        };
        assert!(kinds("Router-WAN").contains(&Connector::Serial));
        assert!(kinds("Switch-24-SFP").iter().any(|c| matches!(c, Connector::Sfp | Connector::Combo)));
        assert!(kinds("DSL-Modem").contains(&Connector::Rj11));
        assert!(kinds("Cable-Modem").contains(&Connector::Coax));
        assert!(kinds("Hub-8").iter().all(|c| *c == Connector::Rj45));
        assert!(kinds("AccessPoint").contains(&Connector::Radio));
    }

    #[test]
    fn the_hub_window_reports_clicks_on_its_ports() {
        let mut s = Session::new();
        s.execute("device.add", json!({"model": "Hub-8", "name": "HUB1", "x": 0.0, "y": 0.0})).unwrap();
        let hub = s.topology.resolve("HUB1").unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        crate::open_physical_tab(h.state_mut(), hub);
        h.state_mut().ui.show_notes = false;
        h.run_steps(3);
        let line = "Ethernet0: link down · cabled to nothing";
        assert_eq!(h.query_all_by_label(line).count(), 1, "the port is a button and nothing is reported yet");
        assert!(h.query_by_label("Point at a port to see what is plugged in. Cables are made with the Connections tool.").is_some());
        h.get_by_label(line).click();
        h.run_steps(3);
        assert!(h.query_all_by_label(line).count() >= 2, "the click reports the port under the panel");
        assert!(h.query_by_label("Point at a port to see what is plugged in. Cables are made with the Connections tool.").is_none());
    }
}
