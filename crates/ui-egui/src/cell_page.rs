//! The cellular network in the Config tab: a smartphone's or tablet's Cellular0 page (Cellular
//! Data, the APN and what the radio sees), and the cellular service page of a central office
//! server (address pool, APN, data plan, the devices admitted) or a cell tower (its sectors).
//! Every change is an engine command (`cell.settings`, `cell.core`).

use egui::{RichText, Sense, vec2};
use netlabcraft_model::cellular::{CELL_RANGE, NoLink};
use netlabcraft_model::{Device, End, ifname};
use serde_json::json;

use crate::PacketApp;
use crate::console::{field, page_title};
use crate::i18n::{tl, tr_any, trf};
use crate::icons_families::{CELL_ORANGE, signal_bars};
use crate::theme;

/// What the page keeps between frames.
#[derive(Debug, Clone, Default)]
struct Form {
    apn: String,
    pool: String,
    loaded: bool,
}

fn form(ui: &egui::Ui, dev: &Device) -> (egui::Id, Form) {
    let fid = ui.make_persistent_id(("cell_form", dev.id.0));
    let mut f: Form = ui.data_mut(|m| m.get_temp(fid)).unwrap_or_default();
    if !f.loaded {
        f.loaded = true;
        f.apn = dev.cell.apn.clone();
        f.pool = dev.cell.pool.map(|p| format!("{}/{}", p.addr, p.prefix)).unwrap_or_default();
    }
    (fid, f)
}

/// Which connection a phone is using now: Wi-Fi when associated, else cellular when attached.
fn active(app: &PacketApp, dev: &Device) -> &'static str {
    let t = &app.session.topology;
    let wifi = dev.ports.iter().position(|p| ifname::is_wireless(&p.name)).is_some_and(|port| t.wifi_ap_of(End { device: dev.id, port }).is_some());
    if wifi {
        "Wi-Fi"
    } else if t.cell_link(dev.id).is_some_and(|l| l.outcome.is_ok()) {
        "Cellular"
    } else {
        "None"
    }
}

/// The status lines and signal bars of a phone's radio.
fn radio_status(app: &PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    let t = &app.session.topology;
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::hover());
        let bars = t.cell_link(dev.id).map_or(0, |l| l.bars);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, trf("Signal {} of 4", &[&bars])));
        signal_bars(&ui.painter_at(rect), rect.left_bottom(), 18.0, bars, CELL_ORANGE, tk.muted);
        match t.cell_link(dev.id) {
            Some(l) => {
                let tower = t.device(l.tower).map_or("?", |x| x.name.as_str());
                ui.label(trf("Signal {} of 4 from {}", &[&l.bars, &tower]));
            }
            None => {
                let why = t.cell_no_link(dev.id).unwrap_or(NoLink::Unavailable);
                ui.label(RichText::new(tr_any(why.text())).color(tk.muted));
            }
        }
    });
    if let Some(l) = t.cell_link(dev.id) {
        match &l.outcome {
            Ok(lease) => {
                ui.label(trf("Address {} · gateway {}", &[&format!("{}/{}", lease.address.addr, lease.address.prefix), &lease.gateway]));
            }
            Err(r) => {
                ui.label(RichText::new(trf("Refused by the central office: {}", &[&tr_any(r.text())])).color(tk.error));
            }
        }
    }
    ui.label(trf("Active connection: {}", &[&tl(active(app, dev))]));
}

/// A smartphone's or tablet's Cellular0 page.
pub(crate) fn phone_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    let mut on = !dev.cell.data_off;
    if ui.checkbox(&mut on, tl("Cellular Data")).on_hover_text(tl("Switch the cellular radio's data connection on or off")).changed() {
        app.run("cell.settings", json!({"device": d.0, "data": on}));
    }
    let (fid, mut f) = form(ui, dev);
    field(ui, tl("APN (access point name)"), &mut f.apn);
    if ui.button(tl("Apply APN")).clicked() {
        app.run("cell.settings", json!({"device": d.0, "apn": f.apn.trim()}));
    }
    ui.separator();
    radio_status(app, ui, dev);
    ui.label(RichText::new(tl("The radio attaches to the nearest cell tower in range; the central office server behind it gives the address. Wi-Fi is preferred when it is connected.")).color(theme::tokens(app.ui.dark).muted));
    ui.data_mut(|m| m.insert_temp(fid, f));
}

/// A central office server's or a cell tower's cellular service page.
pub(crate) fn service_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    let tk = theme::tokens(app.ui.dark);
    let t = &app.session.topology;
    if dev.is_tower() {
        page_title(ui, "Cell tower");
        ui.label(trf(
            "Coverage: {} workspace units around it (a circle on the workspace); {} sectors",
            &[&CELL_RANGE, &dev.ports.iter().filter(|p| ifname::is_cellular(&p.name)).count()],
        ));
        let clients = t.tower_clients(d);
        if clients.is_empty() {
            ui.label(RichText::new(tl("No device is attached to this tower.")).color(tk.muted));
        }
        egui::Grid::new(("tower_clients", d.0)).striped(true).num_columns(4).show(ui, |ui| {
            for (id, l) in clients {
                ui.label(dev.ports.get(l.sector).map_or("?", |p| p.name.as_str()));
                ui.label(t.device(id).map_or("?", |x| x.name.as_str()));
                let (rect, _) = ui.allocate_exact_size(vec2(26.0, 18.0), Sense::hover());
                signal_bars(&ui.painter_at(rect), rect.left_bottom(), 16.0, l.bars, CELL_ORANGE, tk.muted);
                match &l.outcome {
                    Ok(x) => ui.label(x.address.addr.to_string()),
                    Err(r) => ui.label(RichText::new(tr_any(r.text())).color(tk.error)),
                };
                ui.end_row();
            }
        });
        return;
    }
    page_title(ui, "Cellular service");
    let (fid, mut f) = form(ui, dev);
    ui.label(tl("The network the devices' addresses come from; this server's own interface in it is their gateway."));
    field(ui, tl("Address pool (network/prefix)"), &mut f.pool);
    if ui.button(tl("Apply pool")).clicked() {
        let pool = if f.pool.trim().is_empty() { json!(null) } else { json!(f.pool.trim()) };
        app.run("cell.core", json!({"device": d.0, "pool": pool}));
    }
    field(ui, tl("APN accepted (empty: any)"), &mut f.apn);
    if ui.button(tl("Apply APN")).clicked() {
        app.run("cell.core", json!({"device": d.0, "apn": f.apn.trim()}));
    }
    let mut plan = !dev.cell.plan_off;
    if ui.checkbox(&mut plan, tl("Data plan on")).on_hover_text(tl("With the plan off the server refuses every attach")).changed() {
        app.run("cell.core", json!({"device": d.0, "plan": plan}));
    }
    ui.separator();
    ui.label(RichText::new(tl("Devices admitted")).strong());
    let t = &app.session.topology;
    let admitted = t.core_clients(d);
    if admitted.is_empty() {
        ui.label(RichText::new(tl("None yet.")).color(tk.muted));
    }
    egui::Grid::new(("core_clients", d.0)).striped(true).num_columns(3).show(ui, |ui| {
        for (id, l, lease) in admitted {
            ui.label(t.device(id).map_or("?", |x| x.name.as_str()));
            ui.label(lease.address.addr.to_string());
            ui.label(trf("via {}", &[&t.device(l.tower).map_or("?", |x| x.name.as_str())]));
            ui.end_row();
        }
    });
    for (id, l) in &t.cell_links {
        if let (Some(core), Err(r)) = (l.core, &l.outcome)
            && core.device == d
        {
            ui.label(RichText::new(trf("{}: refused: {}", &[&t.device(*id).map_or("?", |x| x.name.as_str()), &tr_any(r.text())])).color(tk.error));
        }
    }
    ui.data_mut(|m| m.insert_temp(fid, f));
}
