//! The Physical tab's hardware panel: the power switch and, on modular routers, the chassis
//! with its bays and the card list. Everything acts through engine commands (`device.power`,
//! `device.module.add`, `device.module.remove`).

use egui::{RichText, Sense, vec2};
use netlabcraft_model::{Device, models};
use serde_json::json;

use crate::i18n::{tl, tr, trf};
use crate::{PacketApp, theme};

/// The power switch, then (modular routers) the chassis and the cards.
pub(crate) fn panel(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    ui.horizontal(|ui| {
        let on = !dev.powered_off;
        if power_switch(ui, on, &tk).clicked() {
            app.run("device.power", json!({"device": dev.id.0, "on": !on}));
        }
        ui.label(if on { tl("Switched on") } else { tl("Switched off") });
        if !on && dev.has_running_config() {
            ui.label(RichText::new(tl("Switching on boots from the saved configuration.")).color(tk.muted));
        }
    });
    power_supply(app, ui, dev, &tk);
    let model = dev.catalogue_model();
    if model.slots().is_empty() {
        return;
    }
    let selected = app.ui.windows.get(&dev.id).and_then(|w| w.card);
    let mut clicked_bay = None;
    let up: Vec<bool> =
        (0..dev.ports.len()).map(|i| netlabcraft_sim::port_up(&app.session.topology, netlabcraft_model::End { device: dev.id, port: i })).collect();
    chassis(ui, dev, &up, selected.is_some(), &tk, &mut clicked_bay);
    if let Some(bay) = clicked_bay {
        match (dev.module_in(bay), selected) {
            (Some(_), _) => {
                app.run("device.module.remove", json!({"device": dev.id.0, "slot": bay}));
            }
            (None, Some(card)) => {
                app.run("device.module.add", json!({"device": dev.id.0, "slot": bay, "module": card}));
            }
            (None, None) => app.info(tl("Pick a card in the list first, then click an empty bay.")),
        }
    }
    let hint = if dev.powered_off {
        tl("Pick a card, then click an empty bay to fit it; click a fitted card to take it out.")
    } else {
        tl("Cards are fitted and removed with the device switched off.")
    };
    ui.label(RichText::new(hint).color(tk.muted));
    ui.label(RichText::new(tl("Cards:")).strong());
    ui.horizontal_wrapped(|ui| {
        for m in models::MODULES {
            let picked = selected == Some(m.id);
            if ui.selectable_label(picked, tr(m.label)).on_hover_text(tr(m.description)).clicked()
                && let Some(w) = app.ui.windows.get_mut(&dev.id)
            {
                w.card = if picked { None } else { Some(m.id) };
            }
        }
    });
}

/// A powered device's supply: its adapter (a plug drawn in code, clicked to plug in or
/// unplug) and where its power comes from; a PoE switch's budget.
fn power_supply(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, tk: &theme::Tokens) {
    if let Some(budget) = netlabcraft_model::poe::budget_mw(dev.catalogue_model()) {
        let used: u32 = app.session.topology.power_inline(dev.id).iter().filter_map(|r| r.device.map(|(_, mw)| mw)).sum();
        let w = |mw: u32| format!("{}.{}", mw / 1000, (mw % 1000) / 100);
        ui.label(trf("Power over Ethernet: {} W of {} W in use", &[&w(used), &w(budget)]));
        return;
    }
    if netlabcraft_model::poe::draw_mw(dev).is_none() {
        return;
    }
    let plugged = dev.adapter != Some(false);
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(vec2(46.0, 24.0), Sense::click());
        let label = if plugged { tl("Power adapter: plugged in (click to unplug)") } else { tl("Power adapter: unplugged (click to plug in)") };
        resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, plugged, label));
        let resp = resp.on_hover_text(label);
        let p = ui.painter_at(rect);
        // A plug with two prongs; its cord reaches the socket when plugged in.
        let body = egui::Rect::from_center_size(rect.center() + vec2(if plugged { 6.0 } else { -6.0 }, 0.0), vec2(14.0, 14.0));
        p.rect_filled(body, 3.0, if plugged { tk.ok } else { tk.chrome_line });
        p.rect_stroke(body, 3.0, egui::Stroke::new(1.0, tk.muted), egui::StrokeKind::Inside);
        for dy in [-3.0, 3.0] {
            p.line_segment([body.right_center() + vec2(0.0, dy), body.right_center() + vec2(5.0, dy)], egui::Stroke::new(1.5, tk.muted));
        }
        p.line_segment([rect.left_center(), body.left_center()], egui::Stroke::new(2.0, tk.muted));
        p.rect_filled(egui::Rect::from_min_size(rect.right_top() + vec2(-4.0, 4.0), vec2(4.0, 16.0)), 1.0, tk.muted);
        if resp.clicked() {
            app.run("device.adapter", json!({"device": dev.id.0, "on": !plugged}));
        }
        ui.label(if plugged { tl("Power adapter plugged in") } else { tl("No power adapter") });
    });
    let src = power_line(&app.session.topology, dev);
    let color = if src.1 { tk.text } else { tk.error };
    ui.label(RichText::new(src.0).color(color));
}

/// What powers a powered device, in words, and whether it has power.
fn power_line(t: &netlabcraft_model::Topology, dev: &Device) -> (String, bool) {
    use netlabcraft_model::poe::Power;
    match t.power(dev.id) {
        Power::Inline { switch, port, mw } => {
            let at = t.device(switch).map_or_else(String::new, |x| {
                format!("{} {}", x.name, x.ports.get(port).map_or_else(String::new, |p| netlabcraft_model::ifname::short(&p.name)))
            });
            (trf("Powered over Ethernet by {} ({} W)", &[&at, &format!("{}.{}", mw / 1000, (mw % 1000) / 100)]), true)
        }
        Power::Adapter | Power::Mains => (tl("Powered by its adapter").to_string(), true),
        Power::Dark(why) => (crate::i18n::tr_any(&why), false),
    }
}

/// A rocker switch drawn in code: green when on.
fn power_switch(ui: &mut egui::Ui, on: bool, tk: &theme::Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(46.0, 24.0), Sense::click());
    let label = if on { tl("Power: on (click to switch off)") } else { tl("Power: off (click to switch on)") };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, on, label));
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 12.0, if on { tk.ok } else { tk.chrome_line });
    p.rect_stroke(rect, 12.0, egui::Stroke::new(1.0, tk.muted), egui::StrokeKind::Inside);
    let knob = if on { rect.right_center() - vec2(12.0, 0.0) } else { rect.left_center() + vec2(12.0, 0.0) };
    p.circle_filled(knob, 9.0, tk.canvas);
    // The power symbol on the knob: a broken ring and a bar.
    let s = egui::Stroke::new(1.5, tk.muted);
    p.line_segment([knob - vec2(0.0, 6.0), knob - vec2(0.0, 1.0)], s);
    let pts: Vec<egui::Pos2> = (0..=10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + 0.6 + i as f32 * (std::f32::consts::TAU - 1.2) / 10.0;
            knob + vec2(a.cos(), a.sin()) * 4.5
        })
        .collect();
    p.add(egui::Shape::line(pts, s));
    resp
}

/// The chassis seen from the back: the bays in a row, each empty (a dark blank plate) or
/// holding a card with its sockets.
fn chassis(ui: &mut egui::Ui, dev: &Device, up: &[bool], armed: bool, tk: &theme::Tokens, clicked: &mut Option<&'static str>) {
    let slots = dev.catalogue_model().slots();
    let bay_w = 130.0;
    let size = vec2((slots.len() as f32 * (bay_w + 10.0) + 30.0).min(ui.available_width().max(200.0)), 92.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, tk.chrome_line);
    p.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, tk.muted), egui::StrokeKind::Inside);
    for (i, slot) in slots.iter().enumerate() {
        let bay = egui::Rect::from_min_size(rect.left_top() + vec2(15.0 + i as f32 * (bay_w + 10.0), 18.0), vec2(bay_w, 60.0));
        let resp = ui.interact(bay, ui.scope_id().with(("bay", dev.id.0, i)), Sense::click());
        let card = dev.module_in(slot);
        let label = match card {
            Some(c) => trf("Bay {}: {} (click to take it out)", &[slot, &tr(c.label)]),
            None => trf("Bay {}: empty", &[slot]),
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, label.clone()));
        let resp = resp.on_hover_text(label);
        if resp.clicked() {
            *clicked = Some(slot);
        }
        let hot = resp.hovered() && !dev.powered_off;
        match card {
            None => {
                p.rect_filled(bay, 3.0, tk.chrome);
                for k in 0..2 {
                    p.circle_filled(bay.left_center() + vec2(8.0 + k as f32 * (bay_w - 16.0), 0.0), 2.5, tk.muted);
                }
                let txt = if armed && dev.powered_off { tl("empty: click to fit") } else { tl("empty") };
                p.text(bay.center(), egui::Align2::CENTER_CENTER, txt, egui::FontId::proportional(11.0), tk.muted);
            }
            Some(c) => {
                p.rect_filled(bay, 3.0, tk.bar);
                let n = c.port_count.max(1) as f32;
                let serial = c.port_type == "Serial";
                for k in 0..c.port_count {
                    let cx = bay.left() + (k as f32 + 0.5) * bay.width() / n;
                    let socket =
                        egui::Rect::from_center_size(egui::pos2(cx, bay.center().y - 6.0), if serial { vec2(22.0, 12.0) } else { vec2(16.0, 14.0) });
                    p.rect_filled(socket, if serial { 5.0 } else { 1.5 }, tk.console_bg);
                    // A switch card's ports: a link light over each socket.
                    if c.switched {
                        let name = format!("{}{slot}/{k}", c.port_type);
                        let lit = dev.ports.iter().position(|x| x.name == name).is_some_and(|i| up.get(i).copied().unwrap_or(false));
                        p.circle_filled(socket.center_top() - vec2(0.0, 5.0), 2.5, if lit { tk.ok } else { tk.muted });
                    }
                }
                p.text(bay.center_bottom() - vec2(0.0, 9.0), egui::Align2::CENTER_CENTER, tr(c.label), egui::FontId::proportional(10.0), tk.bar_text);
            }
        }
        p.rect_stroke(bay, 3.0, egui::Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { tk.selection } else { tk.muted }), egui::StrokeKind::Inside);
        p.text(bay.center_top() - vec2(0.0, 9.0), egui::Align2::CENTER_CENTER, *slot, egui::FontId::proportional(10.0), tk.text);
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
    fn the_physical_tab_switches_power_and_fits_cards() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "modular"})).unwrap();
        let hq = s.topology.resolve("HQ").unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        crate::open_physical_tab(h.state_mut(), hq);
        h.state_mut().ui.show_notes = false;
        h.run_steps(3);
        assert!(h.query_by_label("Bay 0/1: 2-port Serial card (click to take it out)").is_some());
        h.get_by_label("Power: on (click to switch off)").click();
        h.run_steps(2);
        assert!(h.state().session.topology.device(hq).unwrap().powered_off);
        h.get_by_label("2-port Gigabit card").click();
        h.run_steps(2);
        h.get_by_label("Bay 0/2: empty").click();
        h.run_steps(2);
        assert_eq!(h.state().session.topology.device(hq).unwrap().modules.get("0/2").map(String::as_str), Some("Card-2GE"));
        h.get_by_label("Bay 0/0: 1-port Gigabit card (click to take it out)").click();
        h.run_steps(2);
        assert!(!h.state().session.topology.device(hq).unwrap().modules.contains_key("0/0"));
        h.get_by_label("Power: off (click to switch on)").click();
        h.run_steps(2);
        let d = h.state().session.topology.device(hq).unwrap();
        assert!(!d.powered_off && d.ports.iter().any(|p| p.name == "GigabitEthernet0/2/1"));
        // A fixed model shows the power switch only.
        let s1 = h.state().session.topology.resolve("S1").unwrap();
        h.state_mut().ui.windows.clear();
        crate::open_physical_tab(h.state_mut(), s1);
        h.run_steps(3);
        assert!(h.query_by_label("Bay 0/0: empty").is_none());
    }
}
