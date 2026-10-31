//! The Create Complex PDU window: a PDU built field by field (source, destination,
//! application, ports, TTL, type of service, sequence number, size) and when to send it (once
//! or every so many seconds). It only gathers the fields: `pdu.complex` does the work.

use egui::RichText;
use netlabcraft_core::DeviceId;
use netlabcraft_model::scenario::PduApp;
use serde_json::{Value, json};

use crate::console::{LABEL_W, form_label};
use crate::i18n::tl;
use crate::{Mode, PacketApp, theme};

/// What the window holds while it is open.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub source: Option<DeviceId>,
    /// The source interface's name (empty: the one the route picks).
    pub port: String,
    pub destination: String,
    pub app: PduApp,
    pub src_port: String,
    pub dst_port: String,
    pub ttl: String,
    pub tos: String,
    pub seq: String,
    pub size: String,
    pub text: String,
    pub periodic: bool,
    pub period: String,
    pub start: String,
    /// Start at an absolute simulated time (`at`) rather than after a delay (`start`).
    pub absolute: bool,
    pub at: String,
    /// Why the last Create failed.
    pub problem: Option<String>,
}

impl Draft {
    pub fn new(source: Option<DeviceId>) -> Draft {
        Draft { source, size: "32".into(), period: "5".into(), start: "0".into(), ..Draft::default() }
    }
}

/// How each application is named in the window.
fn app_label(a: PduApp) -> &'static str {
    match a {
        PduApp::Ping => "Ping (ICMP echo)",
        PduApp::Dns => "DNS query",
        PduApp::Http => "HTTP GET",
        PduApp::Telnet => "Telnet",
        PduApp::Ssh => "SSH",
        PduApp::Ftp => "FTP",
        PduApp::Smtp => "SMTP",
        PduApp::Pop3 => "POP3",
        PduApp::Dhcp => "DHCP discover",
        PduApp::Tftp => "TFTP read",
        PduApp::Snmp => "SNMP get",
        PduApp::Arp => "ARP request",
        PduApp::Udp => "Raw UDP packet",
        PduApp::Tcp => "Raw TCP connection",
    }
}

/// The label of the free-text field, for the applications that have one.
fn text_label(a: PduApp) -> Option<&'static str> {
    match a {
        PduApp::Dns => Some("Name to look up"),
        PduApp::Http => Some("Page"),
        PduApp::Tftp => Some("File"),
        PduApp::Snmp => Some("Community"),
        _ => None,
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        let l = form_label(ui, label, LABEL_W);
        ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(200.0)).labelled_by(l.id);
    });
}

/// A number field: empty leaves it out; anything else must read as a number.
fn number(p: &mut Value, key: &str, text: &str, label: &str) -> Result<(), String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(());
    }
    let v: f64 = t.parse().map_err(|_| crate::i18n::trf("{} is not a number", &[&crate::i18n::tr(label)]))?;
    if !v.is_finite() || v < 0.0 {
        return Err(crate::i18n::trf("{} is not a number", &[&crate::i18n::tr(label)]));
    }
    p[key] = if v.fract() == 0.0 && !matches!(key, "period" | "start" | "at") { json!(v as u64) } else { json!(v) };
    Ok(())
}

/// The command's parameters for a draft.
pub fn params(d: &Draft) -> Result<Value, String> {
    let src = d.source.ok_or_else(|| tl("Choose the source device").to_string())?;
    let mut p = json!({"from": src.0, "app": d.app.id()});
    if !d.port.is_empty() {
        p["port"] = json!(d.port);
    }
    if !d.app.broadcast() {
        if d.destination.trim().is_empty() {
            return Err(tl("Type the destination address or device").to_string());
        }
        p["to"] = json!(d.destination.trim());
    }
    if d.app.default_port().is_some() {
        number(&mut p, "src_port", &d.src_port, "Source port")?;
        number(&mut p, "dst_port", &d.dst_port, "Destination port")?;
    }
    number(&mut p, "ttl", &d.ttl, "TTL")?;
    number(&mut p, "tos", &d.tos, "TOS")?;
    number(&mut p, "seq", &d.seq, "Sequence number")?;
    number(&mut p, "size", &d.size, "Size")?;
    if text_label(d.app).is_some() && !d.text.trim().is_empty() {
        p["text"] = json!(d.text.trim());
    }
    if d.periodic {
        number(&mut p, "period", &d.period, "Interval")?;
        if p.get("period").is_none() {
            return Err(tl("A periodic PDU needs its interval").to_string());
        }
    }
    if d.absolute {
        number(&mut p, "at", &d.at, "At time")?;
        if p.get("at").is_none() {
            return Err(tl("Type the simulated time to send it at").to_string());
        }
    } else {
        number(&mut p, "start", &d.start, "Start time")?;
    }
    Ok(p)
}

/// Opens the window (or brings it back) with `source` picked.
pub fn open(app: &mut PacketApp, source: Option<DeviceId>) {
    let keep = app.ui.complex.as_ref().map(|d| Draft { source: source.or(d.source), problem: None, ..d.clone() });
    app.ui.complex = Some(keep.unwrap_or_else(|| Draft::new(source)));
}

pub fn window(app: &mut PacketApp, ctx: &egui::Context) {
    let Some(mut d) = app.ui.complex.take() else { return };
    let tk = theme::tokens(app.ui.dark);
    let mut open = true;
    let mut create = false;
    let mut cancel = false;
    let t = &app.session.topology;
    let devices: Vec<(DeviceId, String)> =
        t.devices.iter().filter(|x| !netlabcraft_sim::l3_ifaces(x).is_empty() || x.kind.is_host()).map(|x| (x.id, x.name.clone())).collect();
    let ports: Vec<String> =
        d.source.and_then(|s| t.device(s)).map(|x| netlabcraft_sim::l3_ifaces(x).into_iter().map(|i| i.name).collect()).unwrap_or_default();
    egui::Window::new(tl("Create Complex PDU"))
        .id(egui::Id::unique("complex_pdu"))
        .open(&mut open)
        .default_pos(crate::BELOW_TOOLBARS)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(RichText::new(tl("Source")).strong());
            ui.horizontal(|ui| {
                let l = form_label(ui, tl("Device"), LABEL_W);
                let name = d.source.and_then(|s| devices.iter().find(|x| x.0 == s)).map_or(tl("(choose)").to_string(), |x| x.1.clone());
                egui::ComboBox::from_id_salt("complex_src")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (id, n) in &devices {
                            if ui.selectable_label(d.source == Some(*id), n).clicked() {
                                d.source = Some(*id);
                                d.port.clear();
                            }
                        }
                    })
                    .response
                    .labelled_by(l.id);
            });
            ui.horizontal(|ui| {
                let l = form_label(ui, tl("Outgoing port"), LABEL_W);
                let shown = if d.port.is_empty() { tl("Auto (by route)").to_string() } else { d.port.clone() };
                egui::ComboBox::from_id_salt("complex_port")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.port, String::new(), tl("Auto (by route)"));
                        for p in &ports {
                            ui.selectable_value(&mut d.port, p.clone(), p);
                        }
                    })
                    .response
                    .labelled_by(l.id);
            });
            ui.separator();
            ui.label(RichText::new(tl("Packet")).strong());
            ui.horizontal(|ui| {
                let l = form_label(ui, tl("Application"), LABEL_W);
                egui::ComboBox::from_id_salt("complex_app")
                    .selected_text(tl(app_label(d.app)))
                    .show_ui(ui, |ui| {
                        for a in PduApp::ALL {
                            ui.selectable_value(&mut d.app, a, tl(app_label(a)));
                        }
                    })
                    .response
                    .labelled_by(l.id);
            });
            if !d.app.broadcast() {
                row(ui, tl("Destination"), &mut d.destination, tl("address or device name"));
            }
            if let Some(label) = text_label(d.app) {
                row(ui, tl(label), &mut d.text, "");
            }
            if let Some(port) = d.app.default_port() {
                row(ui, tl("Source port"), &mut d.src_port, tl("any"));
                row(ui, tl("Destination port"), &mut d.dst_port, &port.to_string());
            }
            row(ui, tl("TTL"), &mut d.ttl, tl("device default"));
            row(ui, tl("TOS"), &mut d.tos, "0");
            row(ui, tl("Sequence number"), &mut d.seq, "1");
            row(ui, tl("Size"), &mut d.size, tl("bytes"));
            ui.separator();
            ui.label(RichText::new(tl("When to send it")).strong());
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.periodic, false, tl("Once"));
                ui.radio_value(&mut d.periodic, true, tl("Periodically"));
            });
            if d.periodic {
                row(ui, tl("Interval"), &mut d.period, tl("seconds"));
            }
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.absolute, false, tl("After a delay"));
                ui.radio_value(&mut d.absolute, true, tl("At a simulated time"));
            });
            if d.absolute {
                row(ui, tl("At time"), &mut d.at, tl("seconds of simulated time"));
            } else {
                row(ui, tl("Start time"), &mut d.start, tl("seconds from now"));
            }
            if let Some(p) = &d.problem {
                ui.label(RichText::new(p).color(tk.error));
            }
            ui.horizontal(|ui| {
                if ui.button(tl("Create PDU")).clicked() {
                    create = true;
                }
                if ui.button(tl("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    if create {
        match params(&d) {
            Err(e) => d.problem = Some(e),
            Ok(p) => {
                let first = app.session.sim.trace().back().map_or(0, |e| e.id);
                match app.run("pdu.complex", p) {
                    Some(v) => {
                        let ok = v.get("status").and_then(Value::as_str) == Some("successful");
                        let note = v.get("note").and_then(Value::as_str).unwrap_or_default();
                        let what =
                            crate::i18n::trf("Complex PDU {}: {}", &[&v.get("num").and_then(Value::as_u64).unwrap_or(0), &crate::i18n::tr_any(note)]);
                        app.ui.status = Some((what, !ok && v.get("status").and_then(Value::as_str) == Some("failed")));
                        if app.ui.mode == Mode::Simulation {
                            app.ui.events.start_from(first + 1);
                        }
                        return;
                    }
                    // (`run` put the engine's reason in the status bar: show it here too.)
                    None => d.problem = app.ui.status.as_ref().map(|s| s.0.clone()),
                }
            }
        }
    }
    if open && !cancel {
        app.ui.complex = Some(d);
    }
}
