//! An IP phone's Phone tab: the phone drawn in code, its screen (registration, number, call)
//! and a dial pad. Every action is an engine command (`phone.dial`, `phone.answer`,
//! `phone.hangup`, `phone.status`).

use egui::{RichText, Sense, vec2};
use netlabcraft_model::Device;
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
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
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
                        if ui.add(b).on_hover_text(trf("Key {}", &[&k])).clicked() {
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
                            app.run("phone.answer", json!({"device": d.0}));
                        }
                        if ui.button(tl("Decline")).clicked() {
                            app.run("phone.hangup", json!({"device": d.0}));
                        }
                    }
                    Some(_) => {
                        if ui.button(tl("Hang up")).clicked() {
                            app.run("phone.hangup", json!({"device": d.0}));
                        }
                    }
                    None => {
                        if ui.add_enabled(!dialled.is_empty(), egui::Button::new(tl("Call"))).clicked() {
                            app.run("phone.dial", json!({"device": d.0, "number": dialled}));
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
                if ui.button(tl("Refresh")).on_hover_text(tl("Let the phone register and update its screen")).clicked() {
                    app.run("phone.status", json!({"device": d.0}));
                }
            });
            let mut sip = dev.voice.sip;
            if ui
                .checkbox(&mut sip, tl("Signal with SIP"))
                .on_hover_text(tl("Use SIP (UDP 5060) instead of NetLabCraft's own call-control protocol"))
                .changed()
            {
                app.run("phone.configure", json!({"device": d.0, "protocol": if sip { "sip" } else { "own" }}));
            }
        });
    });
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
}
