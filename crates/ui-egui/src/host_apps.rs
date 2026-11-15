//! The computer apps of an end device's Desktop beside the older ones: the Telnet/SSH Client's
//! form, the Text Editor, and the Firewall and IPv6 Firewall. They act through engine commands
//! (`cli.exec` for the client, `editor.*`, `host.firewall`), like every other panel.

use egui::RichText;
use netlabcraft_model::Device;
use netlabcraft_model::hostfw::{FwProto, HostFwRule};
use netlabcraft_model::services::MAX_HOST_FILE_BYTES;
use serde_json::{Value, json};

use crate::console::{LABEL_W, form_grid, form_label};
use crate::i18n::{tl, trf};
use crate::{PacketApp, theme};

/// The Telnet/SSH Client's fields, kept between frames.
#[derive(Debug, Clone, Default)]
struct TelnetForm {
    ssh: bool,
    host: String,
    user: String,
}

/// The form on top of the Telnet/SSH Client. Open types `telnet HOST` or `ssh -l USER HOST` into
/// the device's Command Prompt, so the session is the prompt's own (the console below the form).
/// Returns whether a session was just opened.
pub(crate) fn telnet_form(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) -> bool {
    let id = egui::Id::unique(("telnet_form", dev.id.0));
    let mut f: TelnetForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    let in_session = app.session.console(dev.id).cli.remote.is_some();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        form_label(ui, tl("Protocol"), LABEL_W);
        ui.radio_value(&mut f.ssh, false, tl("Telnet"));
        ui.radio_value(&mut f.ssh, true, tl("SSH"));
    });
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Host address or name"), LABEL_W);
        ui.add(egui::TextEdit::singleline(&mut f.host).desired_width(260.0)).labelled_by(l.id);
    });
    ui.horizontal(|ui| {
        let l = form_label(ui, tl("Username"), LABEL_W);
        ui.add_enabled(f.ssh, egui::TextEdit::singleline(&mut f.user).desired_width(260.0)).labelled_by(l.id);
    });
    let mut go = false;
    ui.horizontal(|ui| {
        go = ui.add_enabled(!in_session, egui::Button::new(tl("Open"))).clicked();
        if in_session {
            ui.label(RichText::new(tl("A session is open: type exit in the window below to close it.")).color(tk.muted));
        }
    });
    ui.separator();
    let (host, user) = (f.host.trim().to_string(), f.user.trim().to_string());
    let mut opened = false;
    if go {
        if host.is_empty() {
            app.ui.status = Some((tl("Enter a host address or name.").to_string(), true));
        } else if f.ssh && user.is_empty() {
            app.ui.status = Some((tl("Enter a user name for SSH.").to_string(), true));
        } else {
            let line = if f.ssh { format!("ssh -l {user} {host}") } else { format!("telnet {host}") };
            opened = app.run("cli.exec", json!({"device": dev.id.0, "line": line})).is_some();
        }
    }
    ui.data_mut(|m| m.insert_temp(id, f));
    opened
}

/// The Text Editor's state: the name and text in the editor, and the saved file they came from.
#[derive(Debug, Clone, Default)]
struct EditorForm {
    name: String,
    text: String,
    current: Option<String>,
}

/// The Text Editor: the device's files on the left, one open on the right.
pub(crate) fn text_editor(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("editor_form", dev.id.0));
    let mut f: EditorForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    let (mut save, mut delete, mut new) = (false, false, false);
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(180.0);
            ui.label(RichText::new(tl("Files")).strong());
            egui::ScrollArea::vertical().id_salt(("editor_files", dev.id.0)).max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                for file in &dev.files {
                    let on = f.current.as_deref() == Some(file.name.as_str());
                    if ui.selectable_label(on, &file.name).clicked() {
                        f.name.clone_from(&file.name);
                        f.text.clone_from(&file.content);
                        f.current = Some(file.name.clone());
                    }
                }
            });
            if dev.files.is_empty() {
                ui.label(RichText::new(tl("No files yet. Type a name and some text, then press Save.")).color(tk.muted));
            }
            ui.horizontal(|ui| {
                new = ui.button(tl("New")).clicked();
                delete = ui.add_enabled(f.current.is_some(), egui::Button::new(tl("Delete"))).clicked();
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let l = ui.label(tl("File name"));
                ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(260.0)).labelled_by(l.id);
            });
            ui.add(egui::TextEdit::multiline(&mut f.text).font(egui::TextStyle::Monospace).desired_rows(12).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                save = ui.button(tl("Save")).clicked();
                let size = f.text.len();
                let note = RichText::new(trf("{} of {} bytes", &[&size, &MAX_HOST_FILE_BYTES]));
                ui.label(if size > MAX_HOST_FILE_BYTES { note.color(tk.error) } else { note.color(tk.muted) });
            });
        });
    });
    if new {
        f = EditorForm::default();
    }
    if save {
        let name = f.name.trim().to_string();
        if app.run("editor.write", json!({"device": dev.id.0, "name": name, "text": f.text})).is_some() {
            app.info(trf("Saved {}", &[&name]));
            f.name.clone_from(&name);
            f.current = Some(name);
        }
    }
    if delete
        && let Some(name) = f.current.clone()
        && app.run("editor.delete", json!({"device": dev.id.0, "name": name})).is_some()
    {
        app.info(trf("Deleted {}", &[&name]));
        f = EditorForm::default();
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}

/// The new-rule form of the Firewall, kept between frames.
#[derive(Debug, Clone)]
struct FirewallForm {
    allow: bool,
    proto: FwProto,
    remote: String,
    port: String,
}

impl Default for FirewallForm {
    fn default() -> Self {
        FirewallForm { allow: true, proto: FwProto::Any, remote: String::new(), port: String::new() }
    }
}

fn proto_label(p: FwProto, v6: bool) -> &'static str {
    match p {
        FwProto::Any => tl("Any"),
        FwProto::Tcp => "TCP",
        FwProto::Udp => "UDP",
        FwProto::Icmp if v6 => "ICMPv6",
        FwProto::Icmp => "ICMP",
    }
}

/// A rule as `host.firewall` takes it in `add`.
fn rule_params(r: &HostFwRule) -> Value {
    let mut v = json!({"action": if r.allow { "allow" } else { "deny" }, "protocol": r.proto.name()});
    if let Some((a, l)) = r.remote {
        v["remote"] = json!(format!("{a}/{l}"));
    }
    if let Some((lo, hi)) = r.ports {
        v["port"] = json!(if lo == hi { lo.to_string() } else { format!("{lo}-{hi}") });
    }
    v
}

/// What a click in the rule table asks for.
enum RuleAction {
    Up(usize),
    Down(usize),
    Remove(usize),
}

/// The Firewall (IPv4) or IPv6 Firewall of an end device: the switch, the rule form and the rules.
pub(crate) fn firewall(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, v6: bool) {
    let id = egui::Id::unique((if v6 { "firewall6_form" } else { "firewall_form" }, dev.id.0));
    let mut f: FirewallForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    let list = dev.host_fw.list(v6).clone();
    let version = if v6 { 6 } else { 4 };
    let mut on = list.on;
    let mut add = false;
    let mut act: Option<RuleAction> = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tl(if v6 { "IPv6 Firewall" } else { "Firewall" })).strong());
        ui.radio_value(&mut on, true, tl("On"));
        ui.radio_value(&mut on, false, tl("Off"));
    });
    ui.label(
        RichText::new(tl(
            "With the firewall on, the first rule that matches decides and what no rule matches is blocked. Replies to what this computer sent always get through.",
        ))
        .color(tk.muted),
    );
    ui.separator();
    form_grid(ui, ("firewall_new", v6, dev.id.0), |ui| {
        ui.label(tl("Action"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut f.allow, true, tl("Allow"));
            ui.radio_value(&mut f.allow, false, tl("Deny"));
        });
        ui.end_row();
        ui.label(tl("Protocol"));
        ui.horizontal(|ui| {
            for p in [FwProto::Any, FwProto::Tcp, FwProto::Udp, FwProto::Icmp] {
                ui.radio_value(&mut f.proto, p, proto_label(p, v6));
            }
        });
        ui.end_row();
        let l = ui.label(tl("Remote address"));
        let hint = if v6 { tl("Any, or an address such as 2001:db8::/32") } else { tl("Any, or an address such as 10.0.0.0/24") };
        ui.add(egui::TextEdit::singleline(&mut f.remote).hint_text(hint).desired_width(300.0)).labelled_by(l.id);
        ui.end_row();
        let l = ui.label(tl("Port"));
        ui.add_enabled(
            f.proto != FwProto::Icmp,
            egui::TextEdit::singleline(&mut f.port).hint_text(tl("Any, one port or a range such as 1000-2000")).desired_width(300.0),
        )
        .labelled_by(l.id);
        ui.end_row();
        ui.label(tl("Direction"));
        ui.label(tl("Inbound"));
        ui.end_row();
        ui.label("");
        add = ui.button(tl("Add rule")).clicked();
        ui.end_row();
    });
    ui.separator();
    ui.label(RichText::new(tl("Rules")).strong());
    if list.rules.is_empty() {
        ui.label(RichText::new(tl("No rules yet. While the firewall is on, everything that is not allowed is blocked.")).color(tk.muted));
    } else {
        let count = list.rules.len();
        egui::ScrollArea::vertical().id_salt(("firewall_rules", v6, dev.id.0)).max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new(("firewall_table", v6, dev.id.0)).striped(true).spacing(egui::vec2(14.0, 4.0)).show(ui, |ui| {
                for h in ["No.", "Action", "Protocol", "Remote address", "Port", "Direction"] {
                    ui.strong(tl(h));
                }
                ui.end_row();
                for (i, r) in list.rules.iter().enumerate() {
                    ui.label((i + 1).to_string());
                    ui.label(if r.allow { tl("Allow") } else { tl("Deny") });
                    ui.label(proto_label(r.proto, v6));
                    ui.label(r.remote.map_or_else(|| tl("Any").to_string(), |(a, l)| format!("{a}/{l}")));
                    ui.label(r.ports.map_or_else(|| tl("Any").to_string(), |(lo, hi)| if lo == hi { lo.to_string() } else { format!("{lo}-{hi}") }));
                    ui.label(tl("Inbound"));
                    ui.horizontal(|ui| {
                        if ui.add_enabled(i > 0, egui::Button::new(tl("Move up")).small()).clicked() {
                            act = Some(RuleAction::Up(i));
                        }
                        if ui.add_enabled(i + 1 < count, egui::Button::new(tl("Move down")).small()).clicked() {
                            act = Some(RuleAction::Down(i));
                        }
                        if ui.add(egui::Button::new(tl("Remove")).small()).clicked() {
                            act = Some(RuleAction::Remove(i));
                        }
                    });
                    ui.end_row();
                }
            });
        });
    }
    let device = dev.id.0;
    if on != list.on {
        app.run("host.firewall", json!({"device": device, "version": version, "on": on}));
    }
    if add {
        let mut rule = json!({"action": if f.allow { "allow" } else { "deny" }, "protocol": f.proto.name()});
        if !f.remote.trim().is_empty() {
            rule["remote"] = json!(f.remote.trim());
        }
        if f.proto != FwProto::Icmp && !f.port.trim().is_empty() {
            rule["port"] = json!(f.port.trim());
        }
        if app.run("host.firewall", json!({"device": device, "version": version, "add": rule})).is_some() {
            f.remote.clear();
            f.port.clear();
        }
    }
    // A rule moves by being taken out and put back at its new place.
    let moved = match act {
        Some(RuleAction::Up(i)) => i.checked_sub(1).map(|to| (i, to)),
        Some(RuleAction::Down(i)) => Some((i, i + 1)),
        Some(RuleAction::Remove(i)) => {
            app.run("host.firewall", json!({"device": device, "version": version, "remove": i + 1}));
            None
        }
        None => None,
    };
    if let Some((from, to)) = moved
        && let Some(rule) = list.rules.get(from)
        && app.run("host.firewall", json!({"device": device, "version": version, "remove": from + 1})).is_some()
    {
        let mut params = rule_params(rule);
        params["at"] = json!(to + 1);
        app.run("host.firewall", json!({"device": device, "version": version, "add": params}));
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}
