//! The Programming app on a host's Desktop: an HTTP client for RESTCONF (method, URL, headers,
//! a JSON body, a Basic login, saved requests), a NETCONF session runner and a small controller
//! (managed routers, their inventory, a template pushed to all of them), with a JSON viewer for
//! the answers. It acts through `pc.http_request`, `pc.http_saved`, `netconf.session`,
//! `controller.devices` and `controller.push`.

use egui::RichText;
use serde_json::{Value, json};

use crate::PacketApp;
use crate::console::{LABEL_W, form_label};
use crate::i18n::{tl, trf};
use netlabcraft_model::Device;

const METHODS: [&str; 7] = ["GET", "PUT", "PATCH", "POST", "DELETE", "HEAD", "OPTIONS"];
/// Most JSON nodes the viewer draws (a huge answer stays usable).
const MAX_NODES: usize = 1_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Http,
    Netconf,
    Controller,
}

#[derive(Debug, Clone, PartialEq)]
struct Form {
    mode: Mode,
    netconf: bool,
    ctl_address: String,
    ctl_path: String,
    ctl_method: &'static str,
    ctl_template: String,
    ctl_devices: Option<Value>,
    ctl_results: Option<Value>,
    method: &'static str,
    url: String,
    headers: String,
    body: String,
    user: String,
    password: String,
    save_name: String,
    /// The answer: `pc.http_request`'s or `netconf.session`'s result.
    answer: Option<Value>,
    error: Option<String>,
    raw: bool,
    target: String,
    rpcs: String,
    chunked: bool,
}

impl Default for Form {
    fn default() -> Form {
        Form {
            mode: Mode::Http,
            netconf: false,
            ctl_address: String::new(),
            ctl_path: "/restconf/data/ietf-interfaces:interfaces".into(),
            ctl_method: "PATCH",
            ctl_template:
                "{\"ietf-interfaces:interfaces\": {\"interface\": [{\"name\": \"Loopback100\", \"description\": \"Pushed by the controller\"}]}}"
                    .into(),
            ctl_devices: None,
            ctl_results: None,
            method: "GET",
            url: "https://192.168.1.1/restconf/data/ietf-interfaces:interfaces".into(),
            headers: "Accept: application/yang-data+json".into(),
            body: String::new(),
            user: String::new(),
            password: String::new(),
            save_name: String::new(),
            answer: None,
            error: None,
            raw: false,
            target: "192.168.1.1".into(),
            rpcs: "<get-config><source><running/></source></get-config>".into(),
            chunked: true,
        }
    }
}

/// `Name: value` lines as header pairs (lines without a colon are skipped).
fn header_pairs(text: &str) -> Vec<Value> {
    text.lines().filter_map(|l| l.split_once(':')).filter(|(k, _)| !k.trim().is_empty()).map(|(k, v)| json!([k.trim(), v.trim()])).collect()
}

pub fn app(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("programming_form", dev.id.0));
    let mut f: Form = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.radio_value(&mut f.mode, Mode::Http, tl("RESTCONF (HTTP)"));
        ui.radio_value(&mut f.mode, Mode::Netconf, "NETCONF");
        ui.radio_value(&mut f.mode, Mode::Controller, tl("Controller"));
    });
    ui.separator();
    egui::ScrollArea::vertical().id_salt(("programming_scroll", dev.id.0)).auto_shrink([false, true]).show(ui, |ui| match f.mode {
        Mode::Controller => controller(app, ui, dev, &mut f),
        mode => {
            if mode == Mode::Netconf {
                netconf_form(app, ui, dev, &mut f);
            } else {
                http_form(app, ui, dev, &mut f);
            }
            answer(ui, dev, &mut f);
        }
    });
    ui.data_mut(|m| m.insert_temp(id, f));
}

fn login(ui: &mut egui::Ui, f: &mut Form) {
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("User"), LABEL_W);
        ui.add(egui::TextEdit::singleline(&mut f.user).desired_width(140.0)).labelled_by(l.id);
        let l = ui.label(tl("Password"));
        ui.add(egui::TextEdit::singleline(&mut f.password).password(true).desired_width(140.0)).labelled_by(l.id);
    });
}

fn failed(app: &PacketApp) -> Option<String> {
    app.ui.status.as_ref().filter(|(_, err)| *err).map(|(m, _)| m.clone())
}

fn http_form(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, f: &mut Form) {
    // Saved requests: pick one to load it; save the form under a name.
    let saved = dev.automation.saved.clone();
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Saved requests"), LABEL_W);
        let pick = egui::ComboBox::from_id_salt(("programming_saved", dev.id.0))
            .selected_text(if saved.is_empty() { tl("(none)") } else { tl("Load…") })
            .width(220.0)
            .show_ui(ui, |ui| {
                let mut chosen = None;
                for r in &saved {
                    if ui.selectable_label(false, format!("{}  {} {}", r.name, r.method, short(&r.url))).clicked() {
                        chosen = Some(r.clone());
                    }
                }
                chosen
            });
        let _ = pick.response.labelled_by(l.id);
        if let Some(Some(r)) = pick.inner {
            f.method = METHODS.iter().copied().find(|m| *m == r.method).unwrap_or("GET");
            f.url = r.url;
            f.headers = r.headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n");
            f.body = r.body;
            f.save_name = r.name;
        }
    });
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Save as"), LABEL_W);
        ui.add(egui::TextEdit::singleline(&mut f.save_name).desired_width(160.0)).labelled_by(l.id);
        if ui.button(tl("Save")).clicked() {
            let save =
                json!({"name": f.save_name.trim(), "method": f.method, "url": f.url.trim(), "headers": header_pairs(&f.headers), "body": f.body});
            if app.run("pc.http_saved", json!({"pc": dev.id.0, "save": save})).is_some() {
                app.info(trf("Saved the request {}", &[&f.save_name.trim()]));
            }
        }
        let exists = saved.iter().any(|r| r.name == f.save_name.trim());
        if ui.add_enabled(exists, egui::Button::new(tl("Delete"))).clicked() {
            app.run("pc.http_saved", json!({"pc": dev.id.0, "remove": f.save_name.trim()}));
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Request"), LABEL_W);
        let combo = egui::ComboBox::from_id_salt(("programming_method", dev.id.0)).selected_text(f.method).width(90.0).show_ui(ui, |ui| {
            for m in METHODS {
                ui.selectable_value(&mut f.method, m, m);
            }
        });
        let _ = combo.response.labelled_by(l.id);
        ui.add(egui::TextEdit::singleline(&mut f.url).desired_width(ui.available_width().min(420.0)).hint_text("https://…")).labelled_by(l.id);
    });
    login(ui, f);
    ui.horizontal_top(|ui| {
        let l = form_label(ui, tl("Headers (Name: value)"), LABEL_W);
        ui.add(egui::TextEdit::multiline(&mut f.headers).desired_rows(2).desired_width(420.0).code_editor()).labelled_by(l.id);
    });
    ui.horizontal_top(|ui| {
        let l = form_label(ui, tl("Body (JSON or XML)"), LABEL_W);
        ui.vertical(|ui| {
            ui.add(egui::TextEdit::multiline(&mut f.body).desired_rows(5).desired_width(420.0).code_editor()).labelled_by(l.id);
            if ui.button(tl("Format JSON")).clicked() && !f.body.trim().is_empty() {
                match serde_json::from_str::<Value>(&f.body) {
                    Ok(v) => f.body = serde_json::to_string_pretty(&v).unwrap_or_default(),
                    Err(e) => f.error = Some(trf("The body is not valid JSON: {}", &[&e])),
                }
            }
        });
    });
    if ui.button(RichText::new(tl("Send")).strong()).clicked() {
        let mut p = json!({"pc": dev.id.0, "method": f.method, "url": f.url.trim(), "headers": header_pairs(&f.headers)});
        if !f.body.trim().is_empty() {
            p["body"] = json!(f.body);
        }
        if !f.user.trim().is_empty() {
            p["user"] = json!(f.user.trim());
            p["password"] = json!(f.password);
        }
        f.answer = app.run("pc.http_request", p);
        f.netconf = false;
        f.error = if f.answer.is_some() { None } else { failed(app) };
    }
}

fn netconf_form(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, f: &mut Form) {
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Router (port 830)"), LABEL_W);
        ui.add(egui::TextEdit::singleline(&mut f.target).desired_width(200.0)).labelled_by(l.id);
    });
    login(ui, f);
    ui.horizontal(|ui| {
        form_label(ui, tl("Framing"), LABEL_W);
        ui.radio_value(&mut f.chunked, true, tl("1.1 (chunked)"));
        ui.radio_value(&mut f.chunked, false, tl("1.0 (end-of-message)"));
    });
    ui.horizontal_top(|ui| {
        let l = form_label(ui, tl("Operations (one per blank-line block)"), LABEL_W);
        ui.add(egui::TextEdit::multiline(&mut f.rpcs).desired_rows(6).desired_width(420.0).code_editor()).labelled_by(l.id);
    });
    if ui.button(RichText::new(tl("Run session")).strong()).clicked() {
        let rpcs: Vec<String> = f.rpcs.split("\n\n").map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect();
        let p = json!({"pc": dev.id.0, "target": f.target.trim(), "user": f.user.trim(), "password": f.password, "framing": if f.chunked { "1.1" } else { "1.0" }, "rpcs": rpcs});
        f.answer = app.run("netconf.session", p);
        f.netconf = true;
        f.error = if f.answer.is_some() { None } else { failed(app) };
    }
}

/// The controller: the routers this host manages, their inventory, and a template for all.
fn controller(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, f: &mut Form) {
    ui.label(RichText::new(tl("Managed routers")).strong());
    let list = dev.automation.managed.clone();
    let shown = f.ctl_devices.as_ref().and_then(|v| v["devices"].as_array().cloned()).unwrap_or_default();
    egui::Grid::new(("ctl_grid", dev.id.0)).striped(true).show(ui, |ui| {
        for h in [tl("Address"), tl("User"), tl("Hostname"), tl("Interfaces up"), ""] {
            ui.label(RichText::new(h).strong());
        }
        ui.end_row();
        for m in &list {
            let info = shown.iter().find(|x| x["address"] == m.address.as_str());
            ui.monospace(&m.address);
            ui.label(&m.user);
            match info {
                Some(x) if x["error"].is_string() => {
                    ui.colored_label(egui::Color32::from_rgb(0xc0, 0x39, 0x2b), x["error"].as_str().unwrap_or(""));
                    ui.label("");
                }
                Some(x) if x["hostname"].is_string() => {
                    ui.label(x["hostname"].as_str().unwrap_or(""));
                    ui.label(format!("{} / {}", x["up"].as_u64().unwrap_or(0), x["interfaces"].as_u64().unwrap_or(0)));
                }
                _ => {
                    ui.weak(tl("(not read yet)"));
                    ui.label("");
                }
            }
            if ui.button(tl("Remove")).clicked() {
                app.run("controller.devices", json!({"host": dev.id.0, "remove": m.address}));
            }
            ui.end_row();
        }
    });
    if list.is_empty() {
        ui.weak(tl("No routers yet: add one with its address and an admin login."));
    }
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Router address"), LABEL_W);
        ui.add(egui::TextEdit::singleline(&mut f.ctl_address).desired_width(140.0)).labelled_by(l.id);
        if ui.button(tl("Add")).clicked() {
            let add = json!({"address": f.ctl_address.trim(), "user": f.user.trim(), "password": f.password});
            f.error = app.run("controller.devices", json!({"host": dev.id.0, "add": add})).map_or_else(|| failed(app), |_| None);
        }
    });
    login(ui, f);
    if ui.button(tl("Read the inventory")).on_hover_text(tl("Ask every managed router for its hostname and interfaces over RESTCONF")).clicked() {
        f.ctl_devices = app.run("controller.devices", json!({"host": dev.id.0, "refresh": true}));
    }
    ui.separator();
    ui.label(RichText::new(tl("Push a template to every router")).strong());
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Request"), LABEL_W);
        let combo = egui::ComboBox::from_id_salt(("ctl_method", dev.id.0)).selected_text(f.ctl_method).width(90.0).show_ui(ui, |ui| {
            for m in ["PATCH", "PUT", "POST", "DELETE"] {
                ui.selectable_value(&mut f.ctl_method, m, m);
            }
        });
        let _ = combo.response.labelled_by(l.id);
        ui.add(egui::TextEdit::singleline(&mut f.ctl_path).desired_width(360.0)).labelled_by(l.id);
    });
    ui.horizontal_top(|ui| {
        let l = form_label(ui, tl("Template (JSON)"), LABEL_W);
        ui.add(egui::TextEdit::multiline(&mut f.ctl_template).desired_rows(5).desired_width(420.0).code_editor()).labelled_by(l.id);
    });
    if ui.button(RichText::new(tl("Push")).strong()).clicked() {
        let p = json!({"host": dev.id.0, "method": f.ctl_method, "path": f.ctl_path.trim(), "body": f.ctl_template});
        f.ctl_results = app.run("controller.push", p);
        f.error = if f.ctl_results.is_some() { None } else { failed(app) };
    }
    if let Some(e) = &f.error {
        ui.colored_label(egui::Color32::from_rgb(0xc0, 0x39, 0x2b), e);
    }
    for r in f.ctl_results.as_ref().and_then(|v| v["results"].as_array()).into_iter().flatten() {
        let ok = r["status"].as_u64().is_some_and(|s| s < 300);
        let colour = if ok { egui::Color32::from_rgb(0x2e, 0x8b, 0x57) } else { egui::Color32::from_rgb(0xc0, 0x39, 0x2b) };
        let what = r["error"].as_str().map_or_else(|| r["status"].to_string(), |e| format!("{} {e}", r["status"]));
        ui.colored_label(colour, format!("{}: {what}", r["address"].as_str().unwrap_or("")));
    }
}

fn short(url: &str) -> String {
    let path = url.split_once("://").map_or(url, |(_, r)| r);
    let path = path.find('/').and_then(|i| path.get(i..)).unwrap_or(path);
    if path.chars().count() > 48 {
        format!("…{}", path.chars().rev().take(46).collect::<Vec<_>>().into_iter().rev().collect::<String>())
    } else {
        path.to_string()
    }
}

fn answer(ui: &mut egui::Ui, dev: &Device, f: &mut Form) {
    if let Some(e) = &f.error {
        ui.separator();
        ui.colored_label(egui::Color32::from_rgb(0xc0, 0x39, 0x2b), e);
    }
    let Some(a) = f.answer.clone() else { return };
    ui.separator();
    if f.netconf {
        if a["connected"] == false {
            ui.label(trf("The router ended the session: {}", &[&a["error"].as_str().unwrap_or("")]));
            return;
        }
        for (i, m) in a["replies"].as_array().into_iter().flatten().enumerate() {
            egui::CollapsingHeader::new(trf("Reply {}", &[&(i + 1)])).id_salt(("nc_reply", dev.id.0, i)).default_open(true).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(m.as_str().unwrap_or("")).monospace()).wrap());
            });
        }
        egui::CollapsingHeader::new(tl("The router's hello")).id_salt(("nc_hello", dev.id.0)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(a["hello"].as_str().unwrap_or("")).monospace()).wrap());
        });
        applied(ui, &a);
        return;
    }
    let status = a["status"].as_u64().unwrap_or(0);
    let colour = if status < 300 { egui::Color32::from_rgb(0x2e, 0x8b, 0x57) } else { egui::Color32::from_rgb(0xc0, 0x39, 0x2b) };
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{status} {}", a["reason"].as_str().unwrap_or(""))).color(colour).strong());
        ui.label(RichText::new(a["url"].as_str().unwrap_or("")).weak());
    });
    egui::CollapsingHeader::new(tl("Response headers")).id_salt(("http_headers", dev.id.0)).show(ui, |ui| {
        for h in a["headers"].as_array().into_iter().flatten() {
            ui.monospace(format!("{}: {}", h[0].as_str().unwrap_or(""), h[1].as_str().unwrap_or("")));
        }
    });
    let body = a["body"].as_str().unwrap_or("");
    if body.is_empty() {
        ui.weak(tl("(no body)"));
    } else if a["json"].is_null() || f.raw {
        if !a["json"].is_null() {
            ui.checkbox(&mut f.raw, tl("Show as text"));
        }
        ui.add(egui::Label::new(RichText::new(body).monospace()).wrap());
    } else {
        ui.checkbox(&mut f.raw, tl("Show as text"));
        let mut budget = MAX_NODES;
        json_tree(ui, None, &a["json"], &mut vec![dev.id.0.to_string()], 0, &mut budget);
        if budget == 0 {
            ui.weak(tl("(the rest is hidden: show it as text)"));
        }
    }
    applied(ui, &a);
}

fn applied(ui: &mut egui::Ui, a: &Value) {
    for x in a["applied"].as_array().into_iter().flatten() {
        let lines: Vec<&str> = x["lines"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|l| *l != "exit").collect();
        if lines.is_empty() {
            continue;
        }
        ui.label(RichText::new(trf("Configuration applied on {}", &[&x["device"].as_str().unwrap_or("")])).strong());
        ui.monospace(lines.join("\n"));
    }
}

/// A JSON value as a tree: objects and lists fold, leaves show their value.
fn json_tree(ui: &mut egui::Ui, key: Option<&str>, v: &Value, path: &mut Vec<String>, depth: usize, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let name = key.unwrap_or("");
    match v {
        Value::Object(m) => {
            let title = if name.is_empty() { trf("{} members", &[&m.len()]) } else { format!("{name}  ({})", trf("{} members", &[&m.len()])) };
            egui::CollapsingHeader::new(title).id_salt(path.join("/")).default_open(depth < 4).show(ui, |ui| {
                for (k, x) in m {
                    path.push(k.clone());
                    json_tree(ui, Some(k), x, path, depth + 1, budget);
                    path.pop();
                }
            });
        }
        Value::Array(a) => {
            let title = if name.is_empty() { trf("{} entries", &[&a.len()]) } else { format!("{name}  ({})", trf("{} entries", &[&a.len()])) };
            egui::CollapsingHeader::new(title).id_salt(path.join("/")).default_open(depth < 4).show(ui, |ui| {
                for (i, x) in a.iter().enumerate() {
                    path.push(i.to_string());
                    let label = x
                        .get("name")
                        .and_then(Value::as_str)
                        .or_else(|| x.get("prefix").and_then(Value::as_str))
                        .map_or_else(|| format!("[{i}]"), |n| format!("[{i}] {n}"));
                    json_tree(ui, Some(&label), x, path, depth + 1, budget);
                    path.pop();
                }
            });
        }
        leaf => {
            ui.horizontal(|ui| {
                if !name.is_empty() {
                    ui.label(RichText::new(format!("{name}:")).monospace().strong());
                }
                ui.label(RichText::new(leaf.to_string()).monospace());
            });
        }
    }
}
