//! An IP phone's Phone tab: the phone drawn in code, its screen (registration, number, call)
//! and a dial pad. Every action is an engine command (`phone.dial`, `phone.answer`,
//! `phone.hangup`, `phone.status`). The same screen and dial pad serve the Softphone app of a
//! PC's Desktop (`softphone.*` commands).

use crate::prefs_look::HintExt;
use egui::{RichText, Sense, vec2};
use netlabcraft_model::{Device, DeviceKind};
use netlabcraft_sim::voice::CallState;
use serde_json::json;

use crate::i18n::{tl, tr_any, trf};
use crate::{PacketApp, theme};

/// The lines on the phone's screen.
fn screen_lines(app: &PacketApp, dev: &Device) -> Vec<String> {
    let st = app.session.sim.phone(dev.id).cloned().unwrap_or_default();
    let mut lines = Vec::new();
    match (&st.number, st.agent) {
        (Some(n), Some(a)) => lines.push(trf("Line {} (registered with {})", &[n, &a])),
        _ if dev.ports.first().and_then(|p| p.ip).is_none() => lines.push(tl("No address: take one by DHCP (Config tab)").to_string()),
        _ if dev.voice_server().is_none() && dev.kind != DeviceKind::Phone => {
            lines.push(tl("No call server: type its address above, or give option 150 in DHCP").to_string());
        }
        _ if dev.voice.tftp.is_none() => lines.push(tl("No call server: DHCP gave no TFTP server (option 150)").to_string()),
        _ => lines.push(tl("Not registered").to_string()),
    }
    if let Some(c) = &st.call {
        lines.push(match c.state {
            CallState::Dialing => trf("Dialing {}", &[&c.peer]),
            CallState::Ringback => trf("Ringing {}", &[&c.peer]),
            CallState::Ringing => trf("Incoming call from {}", &[&c.peer]),
            CallState::Connected => trf("Talking to {} (voice packets sent {}, received {})", &[&c.peer, &c.rtp_sent, &c.rtp_received]),
        });
    } else if let Some(m) = &st.message {
        lines.push(tr_any(m));
    }
    lines
}

pub(crate) fn tab(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    let mut form = app.ui.windows.get_mut(&d).map(|w| std::mem::take(&mut w.phone)).unwrap_or_default();
    if form.open {
        settings(app, ui, dev, &mut form);
        if let Some(w) = app.ui.windows.get_mut(&d) {
            w.phone = form;
        }
        return;
    }
    if ui.button(tl("Settings")).hint(tl("The phone's own settings: protocol, call server, extension, address and voice VLAN")).clicked() {
        form = PhoneForm { open: true, ..PhoneForm::default() };
    }
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.phone = form;
    }
    call_screen(app, ui, dev);
}

/// The phone's screen and dial pad, for an IP phone and for a PC's Softphone alike.
pub(crate) fn call_screen(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    let soft = dev.kind != DeviceKind::Phone;
    let cmd = |verb: &str| format!("{}.{verb}", if soft { "softphone" } else { "phone" });
    let call = app.session.sim.phone(d).and_then(|s| s.call.clone());
    let dialled = app.ui.windows.get(&d).map(|w| w.dialled.clone()).unwrap_or_default();
    ui.horizontal_top(|ui| {
        // The handset and body, drawn in code.
        let (rect, _) = ui.allocate_exact_size(vec2(90.0, 200.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect.shrink2(vec2(6.0, 4.0)), 10.0, tk.bar);
        let handset = egui::Rect::from_min_size(rect.left_top() + vec2(14.0, 12.0), vec2(28.0, 176.0));
        p.rect_filled(handset, 12.0, tk.chrome_line);
        p.rect_stroke(handset, 12.0, egui::Stroke::new(1.0, tk.muted), egui::StrokeKind::Inside);
        let ringing = call.as_ref().is_some_and(|c| c.state == CallState::Ringing);
        let led = rect.right_top() + vec2(-22.0, 18.0);
        p.circle_filled(
            led,
            5.0,
            if ringing {
                tk.link_down
            } else if call.is_some() {
                tk.link_up
            } else {
                tk.chrome_line
            },
        );
        ui.vertical(|ui| {
            egui::Frame::new().fill(tk.console_bg).inner_margin(8.0).corner_radius(4.0).show(ui, |ui| {
                ui.set_min_width(300.0);
                for l in screen_lines(app, dev) {
                    ui.label(RichText::new(l).color(tk.console_text).monospace());
                }
                ui.label(
                    RichText::new(if dialled.is_empty() { " ".to_string() } else { dialled.clone() }).color(tk.console_text).monospace().size(18.0),
                );
            });
            ui.add_space(6.0);
            let mut typed: Option<char> = None;
            egui::Grid::new(("dialpad", d.0)).spacing(vec2(6.0, 6.0)).show(ui, |ui| {
                for row in [['1', '2', '3'], ['4', '5', '6'], ['7', '8', '9'], ['*', '0', '#']] {
                    for k in row {
                        let b = egui::Button::new(RichText::new(k.to_string()).size(18.0)).min_size(vec2(48.0, 34.0));
                        if ui.add(b).hint(trf("Key {}", &[&k])).clicked() {
                            typed = Some(k);
                        }
                    }
                    ui.end_row();
                }
            });
            if let (Some(k), Some(w)) = (typed, app.ui.windows.get_mut(&d))
                && w.dialled.len() < 16
                && k.is_ascii_digit()
            {
                w.dialled.push(k);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                match call.as_ref().map(|c| c.state) {
                    Some(CallState::Ringing) => {
                        if ui.button(tl("Pick up")).clicked() {
                            app.run(&cmd("answer"), json!({"device": d.0}));
                        }
                        if ui.button(tl("Decline")).clicked() {
                            app.run(&cmd("hangup"), json!({"device": d.0}));
                        }
                    }
                    Some(_) => {
                        if ui.button(tl("Hang up")).clicked() {
                            app.run(&cmd("hangup"), json!({"device": d.0}));
                        }
                    }
                    None => {
                        if ui.add_enabled(!dialled.is_empty(), egui::Button::new(tl("Call"))).clicked() {
                            app.run(&cmd("dial"), json!({"device": d.0, "number": dialled}));
                            app.play(crate::sound::Sound::Ring);
                            if let Some(w) = app.ui.windows.get_mut(&d) {
                                w.dialled.clear();
                            }
                        }
                        if ui.add_enabled(!dialled.is_empty(), egui::Button::new(tl("Clear"))).clicked()
                            && let Some(w) = app.ui.windows.get_mut(&d)
                        {
                            w.dialled.clear();
                        }
                    }
                }
                if ui.button(tl("Refresh")).hint(tl("Let the phone register and update its screen")).clicked() {
                    app.run(&cmd("status"), json!({"device": d.0}));
                }
            });
            let mut sip = dev.voice_sip();
            if ui
                .checkbox(&mut sip, tl("Signal with SIP"))
                .hint(tl("Use SIP (UDP 5060) instead of NetLabCraft's own call-control protocol"))
                .changed()
            {
                let id = if soft { "softphone.config" } else { "phone.configure" };
                app.run(id, json!({"device": d.0, "protocol": if sip { "sip" } else { "own" }}));
            }
        });
    });
}

/// The Softphone app of a PC's Desktop: the switch, the call server, and the phone's own screen
/// and dial pad once it is on.
pub(crate) fn softphone(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    let on = dev.voice.softphone.is_some();
    ui.add_space(6.0);
    let mut want = on;
    if ui.checkbox(&mut want, tl("Switch the Softphone on")).changed() {
        app.run("softphone.config", json!({"device": d.0, "enabled": want}));
        return;
    }
    if !on {
        ui.label(RichText::new(tl("The Softphone is off. Switch it on to register with a call server and make calls.")).color(tk.muted));
        return;
    }
    let id = egui::Id::unique(("softphone_server", d.0));
    let mut server: Option<String> = ui.data_mut(|m| m.get_temp::<String>(id));
    let text = server.get_or_insert_with(|| dev.voice.softphone.as_ref().and_then(|x| x.server).map(|a| a.to_string()).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.label(tl("Call server"));
        ui.add(egui::TextEdit::singleline(text).desired_width(130.0).hint_text(tl("from DHCP")))
            .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::TextInput, true, tl("Call server")));
        if ui.button(tl("Apply")).hint(tl("Empty: the TFTP server DHCP gave (option 150)")).clicked() {
            app.run("softphone.config", json!({"device": d.0, "server": text.trim()}));
        }
        let mac = dev.ports.first().map_or_else(String::new, |p| p.mac.to_string());
        ui.label(RichText::new(format!("{}: {mac}", tl("MAC address"))).color(tk.muted));
    });
    if let Some(v) = server {
        ui.data_mut(|m| m.insert_temp(id, v));
    }
    ui.add_space(4.0);
    call_screen(app, ui, dev);
}

/// The phone's own settings screen: its fields.
#[derive(Debug, Clone, Default)]
pub(crate) struct PhoneForm {
    /// The settings screen is showing (else the call screen).
    pub(crate) open: bool,
    filled: bool,
    server: String,
    ip: String,
    mask: String,
    gateway: String,
}

/// The settings screen: protocol, call server, the extension and voice VLAN it learned, and
/// its address (DHCP or static). Every change is `phone.configure`.
pub(crate) fn settings(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, form: &mut PhoneForm) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    // What the phone learned, read without letting it register again on every frame.
    let number = app.session.sim.phone(d).and_then(|x| x.number.clone());
    let vlan = netlabcraft_sim::voice::voice_vlan(&app.session.topology, d);
    if !form.filled {
        form.server = dev.voice.sip_server.map(|a| a.to_string()).unwrap_or_default();
        let ip = dev.ports.first().and_then(|p| p.ip);
        form.ip = ip.map(|n| n.addr.to_string()).unwrap_or_default();
        form.mask = ip.map(|n| n.mask().to_string()).unwrap_or_else(|| "255.255.255.0".into());
        form.gateway = dev.gateway.map(|g| g.to_string()).unwrap_or_default();
        form.filled = true;
    }
    egui::Frame::new().fill(tk.console_bg).inner_margin(10.0).corner_radius(4.0).show(ui, |ui| {
        ui.set_min_width(420.0);
        ui.label(RichText::new(tl("Phone settings")).color(tk.console_text).strong().size(16.0));
        ui.add_space(4.0);
        let text = |s: String| RichText::new(s).color(tk.console_text);
        crate::console::form_grid(ui, ("phone_settings", d.0), |ui| {
            ui.label(text(tl("Protocol").into()));
            ui.horizontal(|ui| {
                let mut sip = dev.voice.sip;
                let a = ui.radio_value(&mut sip, false, tl("Own call control")).clicked();
                let b = ui.radio_value(&mut sip, true, tl("SIP")).clicked();
                if (a || b) && sip != dev.voice.sip {
                    app.run("phone.configure", json!({"device": d.0, "protocol": if sip { "sip" } else { "own" }}));
                }
            });
            ui.end_row();
            ui.label(text(tl("Call server").into()));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut form.server).desired_width(130.0).hint_text(tl("from DHCP")))
                    .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::TextInput, true, tl("Call server")));
                if ui.button(tl("Apply")).hint(tl("Empty: the TFTP server DHCP gave (option 150)")).clicked() {
                    app.run("phone.configure", json!({"device": d.0, "server": form.server.trim()}));
                }
            });
            ui.end_row();
            ui.label(text(tl("Extension").into()));
            ui.label(text(number.clone().unwrap_or_else(|| tl("none yet (not registered)").to_string())));
            ui.end_row();
            ui.label(text(tl("MAC address").into()));
            ui.label(text(dev.ports.first().map_or_else(String::new, |p| p.mac.to_string())));
            ui.end_row();
            ui.label(text(tl("Voice VLAN").into()));
            ui.label(text(vlan.map_or_else(|| tl("none learned (untagged)").to_string(), |v| trf("{} (learned from the switch)", &[&v]))));
            ui.end_row();
            ui.label(text(tl("TFTP server").into()));
            ui.label(text(dev.voice.tftp.map_or_else(|| "-".to_string(), |a| a.to_string())));
            ui.end_row();
            ui.label(text(tl("Address").into()));
            ui.horizontal(|ui| {
                let mut dhcp = dev.dhcp_client;
                if ui.radio_value(&mut dhcp, true, tl("DHCP")).clicked() {
                    app.run("phone.configure", json!({"device": d.0, "address": "dhcp"}));
                    form.filled = false;
                }
                ui.radio_value(&mut dhcp, false, tl("Static"));
            });
            ui.end_row();
            crate::console::grid_field(ui, tl("IPv4 Address"), &mut form.ip);
            crate::console::grid_field(ui, tl("Subnet Mask"), &mut form.mask);
            crate::console::grid_field(ui, tl("Default Gateway"), &mut form.gateway);
            ui.label("");
            if ui.button(tl("Apply address")).clicked() {
                app.run(
                    "phone.configure",
                    json!({"device": d.0, "address": form.ip.trim(), "mask": form.mask.trim(), "gateway": form.gateway.trim()}),
                );
            }
            ui.end_row();
        });
    });
    if ui.button(tl("Back to the call screen")).clicked() {
        form.open = false;
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
    fn dialling_from_the_phone_tab_rings_the_other_phone() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
        s.execute("phone.status", json!({"device": "Phone2"})).unwrap();
        let n2 = s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["number"].as_str().unwrap().to_string();
        let (p1, p2) = (s.topology.resolve("Phone1").unwrap(), s.topology.resolve("Phone2").unwrap());
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.show_notes = false;
        crate::open_console(h.state_mut(), p1);
        h.run_steps(3);
        h.get_by_label("Refresh").click();
        h.run_steps(2);
        for k in n2.chars() {
            h.get_by_label(&k.to_string()).click();
            h.run_steps(1);
        }
        assert_eq!(h.state().ui.windows.get(&p1).unwrap().dialled, n2);
        h.get_by_label("Call").click();
        h.run_steps(2);
        assert_eq!(h.state().session.sim.phone(p2).and_then(|x| x.call.as_ref()).map(|c| c.state), Some(netlabcraft_sim::voice::CallState::Ringing));
        h.state_mut().ui.windows.clear();
        crate::open_console(h.state_mut(), p2);
        h.run_steps(3);
        h.get_by_label("Pick up").click();
        h.run_steps(2);
        assert_eq!(
            h.state().session.sim.phone(p2).and_then(|x| x.call.as_ref()).map(|c| c.state),
            Some(netlabcraft_sim::voice::CallState::Connected)
        );
        h.get_by_label("Hang up").click();
        h.run_steps(2);
        assert!(h.state().session.sim.phone(p1).is_some_and(|x| x.call.is_none()));
    }

    #[test]
    fn the_softphone_app_on_a_pcs_desktop_calls_an_ip_phone() {
        use egui::accesskit::Role;
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
        for l in ["enable", "configure terminal", "ephone-dn 3", "number 1003", "end"] {
            s.execute("cli.exec", json!({"device": "R1", "line": l})).unwrap();
        }
        s.execute("phone.status", json!({"device": "Phone2"})).unwrap();
        let n2 = s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["number"].as_str().unwrap().to_string();
        let (pc1, p2) = (s.topology.resolve("PC1").unwrap(), s.topology.resolve("Phone2").unwrap());
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.show_notes = false;
        crate::open_desktop_app(h.state_mut(), pc1, "softphone");
        h.run_steps(3);
        assert!(h.query_by_label("The Softphone is off. Switch it on to register with a call server and make calls.").is_some());
        h.get_by_label("Switch the Softphone on").click();
        h.run_steps(3);
        assert!(h.state().session.topology.device(pc1).unwrap().voice.softphone.is_some());
        assert!(h.query_by_label("No call server: type its address above, or give option 150 in DHCP").is_some());
        h.state_mut().session.execute("softphone.config", json!({"device": "PC1", "server": "192.168.100.1"})).unwrap();
        h.run_steps(2);
        h.get_by_role_and_label(Role::Button, "Refresh").click();
        h.run_steps(2);
        for k in n2.chars() {
            h.get_by_label(&k.to_string()).click();
            h.run_steps(1);
        }
        h.get_by_role_and_label(Role::Button, "Call").click();
        h.run_steps(2);
        let state = |h: &Harness<'_, PacketApp>, d| h.state().session.sim.phone(d).and_then(|x| x.call.as_ref()).map(|c| c.state);
        assert_eq!(state(&h, p2), Some(netlabcraft_sim::voice::CallState::Ringing));
        assert_eq!(state(&h, pc1), Some(netlabcraft_sim::voice::CallState::Ringback));
        h.state_mut().session.execute("phone.answer", json!({"device": "Phone2"})).unwrap();
        // The call manager's CONNECT reaches the softphone a moment later.
        h.state_mut().session.execute("softphone.status", json!({"device": "PC1"})).unwrap();
        h.run_steps(3);
        assert_eq!(state(&h, pc1), Some(netlabcraft_sim::voice::CallState::Connected));
        h.get_by_role_and_label(Role::Button, "Hang up").click();
        h.run_steps(2);
        assert_eq!(state(&h, pc1), None);
        assert_eq!(state(&h, p2), None);
    }
}
