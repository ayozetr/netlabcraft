//! The Config tab's wireless page: a controller's WLANs and RADIUS server, or the controller a
//! lightweight access point joins. Every change goes through engine commands.

use egui::RichText;
use packetcraft_model::Device;
use packetcraft_model::wlan::{WlanSecurity, is_wlc, lap_channel, mgmt_addr};
use serde_json::json;

use crate::PacketApp;
use crate::console::{configure, field, page_title};
use crate::i18n::tl;

/// The page's form, kept between frames.
#[derive(Debug, Clone, Default)]
struct Form {
    id: String,
    ssid: String,
    /// 0 open, 1 WPA2-PSK, 2 WPA2-Enterprise.
    security: usize,
    key: String,
    /// The VLAN its hosts land in (empty: the access points' own).
    vlan: String,
    radius: String,
    radius_key: String,
    controller: String,
}

const SECURITY: [&str; 3] = ["Open", "WPA2-PSK", "WPA2-Enterprise"];

pub fn show(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    let fid = egui::Id::unique(("wlan_form", d.0));
    let mut f: Form = ui.data_mut(|m| m.get_temp(fid)).unwrap_or_else(|| Form {
        id: (1..=16u8).find(|n| !dev.wlan.wlans.iter().any(|w| w.id == *n)).unwrap_or(1).to_string(),
        controller: dev.wlan.controller.map(|a| a.to_string()).unwrap_or_default(),
        ..Form::default()
    });
    let mut done = false;
    let addr = mgmt_addr(dev).map(|(v, n)| format!("{} (Vlan{v})", n.addr));
    if is_wlc(dev) {
        page_title(ui, "Wireless LANs");
        ui.label(match &addr {
            Some(a) => crate::i18n::trf("Management address: {}", &[a]),
            None => tl("No management address yet: give interface Vlan1 an address.").to_string(),
        });
        let joined = app.session.topology.joined_aps(d);
        let names: Vec<String> =
            joined.iter().filter_map(|a| app.session.topology.device(*a)).map(|a| format!("{} (ch {})", a.name, lap_channel(a.id))).collect();
        ui.label(crate::i18n::trf("Access points joined: {}", &[&if names.is_empty() { "-".to_string() } else { names.join(", ") }]));
        ui.separator();
        egui::Grid::new(("wlans", d.0)).striped(true).num_columns(5).show(ui, |ui| {
            for h in ["WLAN", "SSID", "Security", "VLAN", ""] {
                ui.label(RichText::new(crate::i18n::tr(h)).strong());
            }
            ui.end_row();
            for w in &dev.wlan.wlans {
                ui.label(w.id.to_string());
                ui.label(&w.ssid);
                ui.label(w.security.name());
                ui.label(w.vlan.map_or("-".to_string(), |v| v.to_string()));
                if ui.small_button(tl("Remove")).clicked() {
                    done |= app.run("wlc.wlan", json!({"device": d.0, "id": w.id, "remove": true})).is_some();
                }
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(tl("Add or change a WLAN")).strong());
        field(ui, tl("WLAN number"), &mut f.id);
        field(ui, tl("SSID"), &mut f.ssid);
        ui.horizontal(|ui| {
            let l = ui.add_sized([130.0, 22.0], egui::Label::new(tl("Security")));
            egui::ComboBox::from_id_salt(("wlan_sec", d.0))
                .selected_text(SECURITY.get(f.security).copied().unwrap_or("Open"))
                .show_ui(ui, |ui| {
                    for (i, s) in SECURITY.iter().enumerate() {
                        ui.selectable_value(&mut f.security, i, *s);
                    }
                })
                .response
                .labelled_by(l.id);
        });
        if f.security == 1 {
            field(ui, tl("WPA2 passphrase"), &mut f.key);
        }
        field(ui, tl("VLAN (optional)"), &mut f.vlan);
        if ui.button(tl("Save WLAN")).clicked() {
            let sec = ["open", "psk", "enterprise"].get(f.security).copied().unwrap_or("open");
            let id = f.id.trim().parse::<u64>().unwrap_or(0);
            let mut p = json!({"device": d.0, "id": id, "ssid": f.ssid.trim(), "security": sec, "key": f.key.trim()});
            if let Ok(v) = f.vlan.trim().parse::<u64>() {
                p["vlan"] = json!(v);
            }
            done |= app.run("wlc.wlan", p).is_some();
        }
        ui.separator();
        ui.label(RichText::new(tl("RADIUS server (WPA2-Enterprise logins)")).strong());
        for (a, _) in &dev.access.radius_servers {
            ui.label(format!("• {a}"));
        }
        field(ui, tl("Server address"), &mut f.radius);
        field(ui, tl("Shared secret"), &mut f.radius_key);
        if ui.button(tl("Add RADIUS server")).clicked() && !f.radius.trim().is_empty() {
            configure(app, d, vec![format!("radius-server host {} key {}", f.radius.trim(), f.radius_key.trim())]);
            done = true;
        }
        if dev.wlan.wlans.iter().any(|w| w.security == WlanSecurity::Enterprise) && dev.access.radius_servers.is_empty() {
            ui.label(RichText::new(tl("A WPA2-Enterprise WLAN needs a RADIUS server.")).color(crate::theme::tokens(app.ui.dark).error));
        }
    } else {
        page_title(ui, "Controller");
        ui.label(match &addr {
            Some(a) => crate::i18n::trf("This access point's address: {}", &[a]),
            None => tl("No address yet: give interface Vlan1 an address.").to_string(),
        });
        field(ui, tl("Controller address (empty: any in this subnet)"), &mut f.controller);
        if ui.button(tl("Apply")).clicked() {
            done |= app.run("lap.controller", json!({"device": d.0, "address": f.controller.trim()})).is_some();
        }
        let joined = app.session.topology.lap_controller(d).and_then(|w| app.session.topology.device(w));
        ui.label(match joined {
            Some(w) => crate::i18n::trf("Joined to {}: offers {} WLAN(s) on channel {}", &[&w.name, &w.wlan.wlans.len(), &lap_channel(d)]),
            None => tl("Not joined: no controller answers (check both addresses, the cabling and the default gateways).").to_string(),
        });
    }
    if done {
        ui.data_mut(|m| m.remove::<Form>(fid));
    } else {
        ui.data_mut(|m| m.insert_temp(fid, f));
    }
}
