//! The computer apps of an end device's Desktop beside the older ones: the Telnet/SSH Client's
//! form, the Text Editor, the Firewall and IPv6 Firewall, the Traffic Generator, the PPPoE
//! Dialer, the VPN Client and the Netflow Collector. They act through engine commands (`cli.exec` for the client,
//! `editor.*`, `host.firewall`, `traffic.*`, `pppoe.*`, `vpnclient.*`), like every other panel.

use egui::RichText;
use netlabcraft_model::hostfw::{FwProto, HostFwRule};
use netlabcraft_model::services::MAX_HOST_FILE_BYTES;
use netlabcraft_model::{Device, End};
use serde_json::{Value, json};

use crate::console::{LABEL_W, form_grid, form_label, grid_field};
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

/// The Traffic Generator's fields and the last burst it sent, kept between frames.
#[derive(Debug, Clone)]
struct TrafficForm {
    /// 0 ICMP, 1 UDP, 2 TCP.
    proto: usize,
    to: String,
    port: String,
    size: String,
    count: String,
    interval: String,
    /// What `traffic.send` or `traffic.status` last answered.
    last: Option<Value>,
    /// The engine's last burst was looked up (once, when the app opened).
    seeded: bool,
}

impl Default for TrafficForm {
    fn default() -> Self {
        TrafficForm {
            proto: 0,
            to: String::new(),
            port: "80".into(),
            size: "32".into(),
            count: "4".into(),
            interval: "1".into(),
            last: None,
            seeded: false,
        }
    }
}

/// The parameters of `traffic.send` for the form, or none when a number is not a number.
fn traffic_params(f: &TrafficForm, device: u32) -> Option<Value> {
    let number = |t: &str| t.trim().parse::<u64>().ok();
    let protocol = ["icmp", "udp", "tcp"].get(f.proto).copied()?;
    let mut v = json!({
        "device": device,
        "to": f.to.trim(),
        "protocol": protocol,
        "size": number(&f.size)?,
        "count": number(&f.count)?,
        "interval": f.interval.trim().parse::<f64>().ok()?,
    });
    if f.proto != 0 {
        v["port"] = json!(number(&f.port)?);
    }
    Some(v)
}

/// The Traffic Generator: where to, which protocol, how many packets of what size and how far
/// apart, then the counts and the result of each packet.
pub(crate) fn traffic_generator(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("traffic_form", dev.id.0));
    let mut f: TrafficForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    if !f.seeded {
        f.seeded = true;
        if dev.has_desktop_apps()
            && let Some(r) = app.run("traffic.status", json!({"device": dev.id.0}))
            && r.get("ran").and_then(Value::as_bool) == Some(true)
        {
            f.last = Some(r);
        }
    }
    let mut send = false;
    ui.add_space(6.0);
    form_grid(ui, ("traffic_fields", dev.id.0), |ui| {
        grid_field(ui, tl("Destination address or name"), &mut f.to);
        ui.label(tl("Protocol"));
        ui.horizontal(|ui| {
            for (i, name) in ["ICMP", "UDP", "TCP"].into_iter().enumerate() {
                ui.radio_value(&mut f.proto, i, name);
            }
        });
        ui.end_row();
        let l = ui.label(tl("Port"));
        ui.add_enabled(f.proto != 0, egui::TextEdit::singleline(&mut f.port).desired_width(120.0)).labelled_by(l.id);
        ui.end_row();
        let l = ui.label(tl("Packet size (bytes)"));
        ui.add(egui::TextEdit::singleline(&mut f.size).desired_width(120.0)).labelled_by(l.id);
        ui.end_row();
        let l = ui.label(tl("Number of packets"));
        ui.add(egui::TextEdit::singleline(&mut f.count).desired_width(120.0)).labelled_by(l.id);
        ui.end_row();
        let l = ui.label(tl("Interval (seconds)"));
        ui.add(egui::TextEdit::singleline(&mut f.interval).desired_width(120.0)).labelled_by(l.id);
        ui.end_row();
        ui.label("");
        send = ui.button(tl("Send")).clicked();
        ui.end_row();
    });
    ui.label(RichText::new(tl("Each packet is sent like a PDU: the same path, firewalls and access lists apply.")).color(tk.muted));
    if send {
        if f.to.trim().is_empty() {
            app.ui.status = Some((tl("Enter a destination address or device name.").to_string(), true));
        } else if let Some(params) = traffic_params(&f, dev.id.0) {
            if let Some(r) = app.run("traffic.send", params) {
                f.last = Some(r);
            }
        } else {
            app.ui.status =
                Some((tl("Enter whole numbers for the port, size and number of packets, and seconds for the interval.").to_string(), true));
        }
    }
    ui.separator();
    match &f.last {
        None => {
            ui.label(RichText::new(tl("Nothing sent yet. Fill in the destination and press Send.")).color(tk.muted));
        }
        Some(r) => {
            let count = |key: &str| r.get(key).and_then(Value::as_u64).unwrap_or(0);
            ui.label(RichText::new(trf("Sent {}, received {}, lost {}", &[&count("sent"), &count("received"), &count("lost")])).strong());
            let results = r.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
            egui::ScrollArea::vertical().id_salt(("traffic_results", dev.id.0)).max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new(("traffic_table", dev.id.0)).striped(true).spacing(egui::vec2(14.0, 4.0)).show(ui, |ui| {
                    for h in ["No.", "Result", "Time", "Detail"] {
                        ui.strong(tl(h));
                    }
                    ui.end_row();
                    for x in &results {
                        ui.label(x.get("n").and_then(Value::as_u64).unwrap_or(0).to_string());
                        if x.get("ok").and_then(Value::as_bool) == Some(true) {
                            ui.label(RichText::new(tl("Answered")).color(tk.ok));
                        } else {
                            ui.label(RichText::new(tl("Lost")).color(tk.error));
                        }
                        ui.label(format!("{:.3}", x.get("at").and_then(Value::as_f64).unwrap_or(0.0)));
                        ui.label(crate::i18n::tr_any(x.get("note").and_then(Value::as_str).unwrap_or_default()));
                        ui.end_row();
                    }
                });
            });
        }
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}

/// The PPPoE Dialer's account fields, kept between frames.
#[derive(Debug, Clone, Default)]
struct PppoeForm {
    user: String,
    password: String,
    /// The fields were filled from the device's saved account.
    loaded: bool,
}

/// The PPPoE Dialer: the account, Connect and Disconnect, and what the session gave the device.
pub(crate) fn pppoe_dialer(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("pppoe_form", dev.id.0));
    let mut f: PppoeForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    if !f.loaded {
        f.loaded = true;
        f.user.clone_from(&dev.pppoe.username);
        f.password.clone_from(&dev.pppoe.password);
    }
    let session = dev.pppoe.session.clone();
    let (mut connect, mut disconnect) = (false, false);
    ui.add_space(6.0);
    form_grid(ui, ("pppoe_fields", dev.id.0), |ui| {
        grid_field(ui, tl("Username"), &mut f.user);
        let l = ui.label(tl("Password"));
        ui.add(egui::TextEdit::singleline(&mut f.password).password(true).desired_width(300.0)).labelled_by(l.id);
        ui.end_row();
        ui.label("");
        ui.horizontal(|ui| {
            connect = ui.add_enabled(session.is_none(), egui::Button::new(tl("Connect"))).clicked();
            disconnect = ui.add_enabled(session.is_some(), egui::Button::new(tl("Disconnect"))).clicked();
        });
        ui.end_row();
    });
    ui.label(RichText::new(tl("The first network port must be cabled straight to a server whose PPPoE service is on.")).color(tk.muted));
    ui.separator();
    // The cable still reaches the server the session came from?
    let live = session.as_ref().is_some_and(|x| app.session.topology.peer(End { device: dev.id, port: 0 }).is_some_and(|p| p.device == x.server));
    match &session {
        None => {
            ui.label(RichText::new(tl("Disconnected")).strong());
            ui.label(RichText::new(tl("Enter the account and press Connect.")).color(tk.muted));
        }
        Some(x) => {
            let server = app.session.topology.device(x.server).map_or_else(|| "?".to_string(), |s| s.name.clone());
            if live {
                ui.label(RichText::new(tl("Connected")).strong().color(tk.ok));
            } else {
                ui.label(RichText::new(tl("Connection lost: the cable or the server is gone.")).strong().color(tk.error));
            }
            form_grid(ui, ("pppoe_session", dev.id.0), |ui| {
                ui.label(tl("Server"));
                ui.label(server);
                ui.end_row();
                ui.label(tl("Address"));
                ui.label(x.address.to_string());
                ui.end_row();
                ui.label(tl("Default Gateway"));
                ui.label(x.gateway.to_string());
                ui.end_row();
                ui.label(tl("Authentication"));
                ui.label(x.auth.name().to_ascii_uppercase());
                ui.end_row();
            });
        }
    }
    let device = dev.id.0;
    if connect {
        if f.user.trim().is_empty() {
            app.ui.status = Some((tl("Enter a user name.").to_string(), true));
        } else if let Some(r) = app.run("pppoe.connect", json!({"device": device, "username": f.user.trim(), "password": f.password})) {
            let server = r.get("server").and_then(Value::as_str).unwrap_or("?").to_string();
            app.info(trf("Connected to {}", &[&server]));
        }
    }
    if disconnect && app.run("pppoe.disconnect", json!({"device": device})).is_some() {
        app.info(tl("Disconnected"));
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}

/// The VPN Client's fields, kept between frames.
#[derive(Debug, Clone, Default)]
struct VpnForm {
    group: String,
    key: String,
    host: String,
    user: String,
    password: String,
    /// The fields were filled from the device's saved settings.
    loaded: bool,
}

/// The VPN Client: group, key, the concentrator's address and the account, Connect and
/// Disconnect, and what the session gave the computer.
pub(crate) fn vpn_client(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("vpn_form", dev.id.0));
    let mut f: VpnForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    if !f.loaded {
        f.loaded = true;
        f.group.clone_from(&dev.vpn_client.group);
        f.key.clone_from(&dev.vpn_client.key);
        f.host.clone_from(&dev.vpn_client.host);
        f.user.clone_from(&dev.vpn_client.username);
        f.password.clone_from(&dev.vpn_client.password);
    }
    let session = app.session.sim.ra_client(dev.id).cloned();
    let (mut connect, mut disconnect) = (false, false);
    ui.add_space(6.0);
    form_grid(ui, ("vpn_fields", dev.id.0), |ui| {
        grid_field(ui, tl("Group name"), &mut f.group);
        let l = ui.label(tl("Group key"));
        ui.add(egui::TextEdit::singleline(&mut f.key).password(true).desired_width(300.0)).labelled_by(l.id);
        ui.end_row();
        grid_field(ui, tl("Host IP address"), &mut f.host);
        grid_field(ui, tl("Username"), &mut f.user);
        let l = ui.label(tl("Password"));
        ui.add(egui::TextEdit::singleline(&mut f.password).password(true).desired_width(300.0)).labelled_by(l.id);
        ui.end_row();
        ui.label("");
        ui.horizontal(|ui| {
            connect = ui.add_enabled(session.is_none(), egui::Button::new(tl("Connect"))).clicked();
            disconnect = ui.add_enabled(session.is_some(), egui::Button::new(tl("Disconnect"))).clicked();
        });
        ui.end_row();
    });
    ui.label(
        RichText::new(tl("The host is a router set up as a remote-access VPN server; type the address this computer reaches it at.")).color(tk.muted),
    );
    ui.separator();
    match &session {
        None => {
            ui.label(RichText::new(tl("Disconnected")).strong());
            ui.label(RichText::new(tl("Fill in the form and press Connect.")).color(tk.muted));
        }
        Some(x) => {
            ui.label(RichText::new(tl("Connected")).strong().color(tk.ok));
            form_grid(ui, ("vpn_session", dev.id.0), |ui| {
                ui.label(tl("Server"));
                ui.label(x.peer.to_string());
                ui.end_row();
                ui.label(tl("VPN address"));
                ui.label(x.assigned.to_string());
                ui.end_row();
                ui.label(tl("Encryption"));
                ui.label(x.suite.map_or_else(|| x.transforms.clone(), |su| su.describe()));
                ui.end_row();
            });
            ui.label(RichText::new(trf("Tunnel packets: {} sent, {} received", &[&x.encaps, &x.decaps])).color(tk.muted));
        }
    }
    let device = dev.id.0;
    if connect {
        if f.host.trim().is_empty() || f.group.trim().is_empty() || f.key.is_empty() {
            app.ui.status = Some((tl("Enter the host address, the group name and the group key.").to_string(), true));
        } else if let Some(r) = app.run(
            "vpnclient.connect",
            json!({"device": device, "host": f.host.trim(), "group": f.group.trim(), "key": f.key, "username": f.user.trim(), "password": f.password}),
        ) {
            let server = r.get("server").and_then(Value::as_str).unwrap_or("?").to_string();
            app.info(trf("Connected to {}", &[&server]));
        }
    }
    if disconnect && app.run("vpnclient.disconnect", json!({"device": device})).is_some() {
        app.info(tl("Disconnected"));
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}

/// The Netflow Collector's port field, kept between frames.
#[derive(Debug, Clone, Default)]
struct NetflowForm {
    port: String,
    /// The field was filled from the device's saved port.
    loaded: bool,
}

/// Most flows the Netflow Collector's list shows (the newest).
const NETFLOW_SHOWN: usize = 100;

/// The Netflow Collector: the port it listens on, what it has received (top talkers by bytes,
/// protocols, then the newest flows) and a button that clears it.
pub(crate) fn netflow_collector(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("netflow_form", dev.id.0));
    let mut f: NetflowForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    if !f.loaded {
        f.loaded = true;
        f.port = dev.netflow.collector_port.unwrap_or(netlabcraft_wire::netflow::NETFLOW_PORT).to_string();
    }
    let listening = dev.netflow.collector_port;
    let (mut start, mut stop, mut clear) = (false, false, false);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let l = ui.label(tl("Port"));
        ui.add(egui::TextEdit::singleline(&mut f.port).desired_width(80.0)).labelled_by(l.id);
        start = ui.add_enabled(listening.is_none(), egui::Button::new(tl("Start"))).clicked();
        stop = ui.add_enabled(listening.is_some(), egui::Button::new(tl("Stop"))).clicked();
        clear = ui.button(tl("Clear")).clicked();
    });
    match listening {
        Some(p) => ui.label(RichText::new(trf("Listening on UDP port {}", &[&p])).strong().color(tk.ok)),
        None => ui.label(RichText::new(tl("Not listening")).strong()),
    };
    ui.label(RichText::new(tl("Set routers to export here with: ip flow-export destination ADDRESS PORT")).color(tk.muted));
    ui.separator();
    let got = app.session.sim.netflow_collected(dev.id);
    let datagrams = got.map_or(0, |c| c.datagrams);
    let sum = got.map(netlabcraft_sim::netflow::Collector::summary).unwrap_or_default();
    let newest: Vec<netlabcraft_sim::netflow::Collected> =
        got.map(|c| c.records.iter().rev().take(NETFLOW_SHOWN).cloned().collect()).unwrap_or_default();
    ui.label(
        RichText::new(trf("Datagrams: {}, flows: {}, packets: {}, bytes: {}", &[&datagrams, &sum.total.flows, &sum.total.packets, &sum.total.bytes]))
            .strong(),
    );
    if newest.is_empty() {
        ui.label(RichText::new(tl("No flows received yet. Routers send a flow once it has been quiet for 15 seconds.")).color(tk.muted));
    } else {
        egui::ScrollArea::vertical().id_salt(("netflow_scroll", dev.id.0)).max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.strong(tl("Top talkers"));
            egui::Grid::new(("netflow_talkers", dev.id.0)).striped(true).spacing(egui::vec2(14.0, 4.0)).show(ui, |ui| {
                for h in ["Address", "Flows", "Packets", "Bytes"] {
                    ui.strong(tl(h));
                }
                ui.end_row();
                for (a, t) in sum.talkers.iter().take(10) {
                    ui.label(a.to_string());
                    ui.label(t.flows.to_string());
                    ui.label(t.packets.to_string());
                    ui.label(t.bytes.to_string());
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.strong(tl("Protocols"));
            egui::Grid::new(("netflow_protocols", dev.id.0)).striped(true).spacing(egui::vec2(14.0, 4.0)).show(ui, |ui| {
                for h in ["Protocol", "Flows", "Packets", "Bytes"] {
                    ui.strong(tl(h));
                }
                ui.end_row();
                for (n, t) in &sum.protocols {
                    ui.label(netlabcraft_sim::netflow::protocol_name(*n));
                    ui.label(t.flows.to_string());
                    ui.label(t.packets.to_string());
                    ui.label(t.bytes.to_string());
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.strong(tl("Flows"));
            egui::Grid::new(("netflow_flows", dev.id.0)).striped(true).spacing(egui::vec2(14.0, 4.0)).show(ui, |ui| {
                for h in ["Time", "Exporter", "Source", "Destination", "Protocol", "Ports", "Packets", "Bytes"] {
                    ui.strong(tl(h));
                }
                ui.end_row();
                for c in &newest {
                    let r = &c.rec;
                    ui.label(format!("{:.1}", c.at_us as f64 / 1_000_000.0));
                    ui.label(c.exporter.to_string());
                    ui.label(r.src.to_string());
                    ui.label(r.dst.to_string());
                    ui.label(netlabcraft_sim::netflow::protocol_name(r.protocol));
                    ui.label(format!("{} > {}", r.src_port, r.dst_port));
                    ui.label(r.packets.to_string());
                    ui.label(r.octets.to_string());
                    ui.end_row();
                }
            });
        });
    }
    let device = dev.id.0;
    if start {
        match f.port.trim().parse::<u16>() {
            Ok(p) if p >= 1 => {
                app.run("netflow.listen", json!({"device": device, "port": p}));
            }
            _ => app.ui.status = Some((tl("Enter a port from 1 to 65535.").to_string(), true)),
        }
    }
    if stop {
        app.run("netflow.stop", json!({"device": device}));
    }
    if clear {
        app.run("netflow.clear", json!({"device": device}));
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}
