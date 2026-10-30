//! The MIB Browser on a host's Desktop: SNMP get, get-next, set and a walk (SNMPv2c or
//! SNMPv3), through the `snmp.request` command, and the traps agents sent the host.

use egui::RichText;
use serde_json::json;

use crate::PacketApp;
use crate::i18n::tl;
use netlabcraft_model::Device;

/// Objects every NetLabCraft agent serves, by name.
const NAMED: [(&str, &str); 8] = [
    ("sysDescr", "1.3.6.1.2.1.1.1.0"),
    ("sysUpTime", "1.3.6.1.2.1.1.3.0"),
    ("sysContact", "1.3.6.1.2.1.1.4.0"),
    ("sysName", "1.3.6.1.2.1.1.5.0"),
    ("sysLocation", "1.3.6.1.2.1.1.6.0"),
    ("ifNumber", "1.3.6.1.2.1.2.1.0"),
    ("ifDescr", "1.3.6.1.2.1.2.2.1.2"),
    ("ifOperStatus", "1.3.6.1.2.1.2.2.1.8"),
];

#[derive(Debug, Clone)]
struct Form {
    agent: String,
    /// "2c" or "3".
    version: &'static str,
    community: String,
    user: String,
    /// "none", "md5" or "sha".
    auth: &'static str,
    password: String,
    oid: String,
    op: &'static str,
    value: String,
    results: Vec<(String, String)>,
}

impl Default for Form {
    fn default() -> Form {
        Form {
            agent: String::new(),
            version: "2c",
            community: "public".into(),
            user: String::new(),
            auth: "none",
            password: String::new(),
            oid: "1.3.6.1.2.1.1.5.0".into(),
            op: "get",
            value: String::new(),
            results: Vec::new(),
        }
    }
}

pub fn app(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("mib_form", dev.id.0));
    let mut f: Form = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let row = |ui: &mut egui::Ui, label: &str, v: &mut String| {
        ui.horizontal(|ui| {
            let l = crate::console::form_label(ui, label, crate::console::LABEL_W);
            ui.add(egui::TextEdit::singleline(v).desired_width(240.0)).labelled_by(l.id);
        });
    };
    row(ui, tl("Agent address"), &mut f.agent);
    ui.horizontal(|ui| {
        crate::console::form_label(ui, tl("Version"), crate::console::LABEL_W);
        ui.radio_value(&mut f.version, "2c", "SNMPv2c");
        ui.radio_value(&mut f.version, "3", "SNMPv3");
    });
    if f.version == "3" {
        row(ui, tl("User"), &mut f.user);
        ui.horizontal(|ui| {
            crate::console::form_label(ui, tl("Authentication"), crate::console::LABEL_W);
            ui.radio_value(&mut f.auth, "none", tl("None"));
            ui.radio_value(&mut f.auth, "md5", "MD5");
            ui.radio_value(&mut f.auth, "sha", "SHA");
        });
        if f.auth != "none" {
            ui.horizontal(|ui| {
                let l = crate::console::form_label(ui, tl("Password"), crate::console::LABEL_W);
                ui.add(egui::TextEdit::singleline(&mut f.password).password(true).desired_width(240.0)).labelled_by(l.id);
            });
        }
    } else {
        row(ui, tl("Community"), &mut f.community);
    }
    // What every request carries besides the operation and OID.
    let login = json!({
        "device": dev.id.0, "target": f.agent.trim(), "version": f.version, "community": f.community.trim(),
        "user": f.user.trim(), "auth": f.auth, "password": f.password,
    });
    let with = |extra: serde_json::Value| {
        let mut p = login.clone();
        if let (Some(m), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            m.extend(e.clone());
        }
        p
    };
    ui.horizontal(|ui| {
        let l = crate::console::form_label(ui, tl("OID"), crate::console::LABEL_W);
        ui.add(egui::TextEdit::singleline(&mut f.oid).desired_width(200.0)).labelled_by(l.id);
        ui.menu_button(tl("Objects"), |ui| {
            for (name, oid) in NAMED {
                if ui.selectable_label(f.oid == oid, name).clicked() {
                    f.oid = oid.to_string();
                    ui.close();
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        for op in ["get", "get-next", "set"] {
            ui.radio_value(&mut f.op, op, op);
        }
    });
    if f.op == "set" {
        row(ui, tl("Value"), &mut f.value);
    }
    ui.horizontal(|ui| {
        if ui.button(tl("Go")).clicked() {
            let mut p = with(json!({"operation": f.op, "oid": f.oid.trim()}));
            if f.op == "set" {
                p["value"] = json!(f.value);
            }
            if let Some(r) = app.run("snmp.request", p) {
                if let Some(e) = r["error"].as_str() {
                    f.results.push((f.oid.clone(), format!("({e})")));
                }
                for b in r["bindings"].as_array().into_iter().flatten() {
                    let (o, v) = (b["oid"].as_str().unwrap_or("").to_string(), b["value"].as_str().unwrap_or("").to_string());
                    if f.op == "get-next" {
                        f.oid.clone_from(&o);
                    }
                    f.results.push((o, v));
                }
            }
        }
        if ui.button(tl("Walk")).on_hover_text(tl("get-next from the OID until its branch ends")).clicked() {
            let prefix = f.oid.trim().trim_end_matches(".0").to_string();
            let mut at = prefix.clone();
            for _ in 0..64 {
                let Some(r) = app.run("snmp.request", with(json!({"operation": "get-next", "oid": at}))) else {
                    break;
                };
                let Some(b) = r["bindings"].as_array().and_then(|a| a.first()).cloned() else { break };
                let o = b["oid"].as_str().unwrap_or("").to_string();
                if !o.starts_with(&format!("{prefix}.")) || b["value"] == "end of the MIB" {
                    break;
                }
                f.results.push((o.clone(), b["value"].as_str().unwrap_or("").to_string()));
                at = o;
            }
        }
        if ui.button(tl("Clear")).clicked() {
            f.results.clear();
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().max_height(180.0).id_salt(("mib_results", dev.id.0)).stick_to_bottom(true).show(ui, |ui| {
        egui::Grid::new(("mib_grid", dev.id.0)).striped(true).show(ui, |ui| {
            if !f.results.is_empty() {
                ui.label(RichText::new(tl("OID")).strong());
                ui.label(RichText::new(tl("Value")).strong());
                ui.end_row();
            }
            for (o, v) in &f.results {
                let name = NAMED.iter().find(|(_, x)| o == *x || o.starts_with(&format!("{x}."))).map_or(String::new(), |(n, _)| format!("  {n}"));
                ui.monospace(format!("{o}{name}"));
                ui.label(v);
                ui.end_row();
            }
        });
    });
    traps(app, ui, dev);
    ui.data_mut(|m| m.insert_temp(id, f));
}

/// The traps agents sent this host (UDP port 162), newest first.
fn traps(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let list = app.session.sim.traps(dev.id);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} ({})", tl("Traps"), list.len())).strong());
        if !list.is_empty() && ui.button(tl("Clear")).on_hover_text(tl("Forget the traps received")).clicked() {
            app.run("snmp.traps", json!({"device": dev.id.0, "clear": true}));
        }
    });
    if list.is_empty() {
        ui.weak(tl("No traps yet. Routers and switches send them with snmp-server host and snmp-server enable traps."));
        return;
    }
    egui::ScrollArea::vertical().max_height(140.0).id_salt(("mib_traps", dev.id.0)).show(ui, |ui| {
        egui::Grid::new(("mib_trap_grid", dev.id.0)).striped(true).show(ui, |ui| {
            for h in [tl("Time"), tl("From"), tl("Trap"), tl("Details")] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for x in list.iter().rev() {
                let secs = x.at_us / 1_000_000;
                ui.monospace(format!("{:02}:{:02}:{:02}", secs / 3_600, secs / 60 % 60, secs % 60));
                ui.label(format!("{} ({})", x.from, x.community));
                ui.label(&x.name);
                // The port's name, when the trap names one (ifDescr), else the values.
                let detail = x
                    .bindings
                    .iter()
                    .find(|(o, _)| o.starts_with("1.3.6.1.2.1.2.2.1.2."))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| x.bindings.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(", "));
                ui.label(detail);
                ui.end_row();
            }
        });
    });
}
