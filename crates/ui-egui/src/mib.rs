//! The MIB Browser on a host's Desktop: SNMP get, get-next, set and a walk, through the
//! `snmp.request` command.

use egui::RichText;
use serde_json::json;

use crate::PacketApp;
use crate::i18n::tl;
use packetcraft_model::Device;

/// Objects every PacketCraft agent serves, by name.
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
    community: String,
    oid: String,
    op: &'static str,
    value: String,
    results: Vec<(String, String)>,
}

impl Default for Form {
    fn default() -> Form {
        Form {
            agent: String::new(),
            community: "public".into(),
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
            let l = ui.add_sized([120.0, 22.0], egui::Label::new(label));
            ui.add(egui::TextEdit::singleline(v).desired_width(240.0)).labelled_by(l.id);
        });
    };
    row(ui, tl("Agent address"), &mut f.agent);
    row(ui, tl("Community"), &mut f.community);
    ui.horizontal(|ui| {
        let l = ui.add_sized([120.0, 22.0], egui::Label::new(tl("OID")));
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
            let mut p =
                json!({"device": dev.id.0, "target": f.agent.trim(), "community": f.community.trim(), "operation": f.op, "oid": f.oid.trim()});
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
                let Some(r) = app.run(
                    "snmp.request",
                    json!({"device": dev.id.0, "target": f.agent.trim(), "community": f.community.trim(), "operation": "get-next", "oid": at}),
                ) else {
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
    ui.data_mut(|m| m.insert_temp(id, f));
}
