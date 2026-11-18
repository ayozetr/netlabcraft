//! The USB windows: a stick's Files page, and the USB app of a host (PC, laptop, server) that lists
//! what is plugged into its ports and their state. Both act through the `usb.*` and
//! `printer.print` commands.

use egui::RichText;
use netlabcraft_model::usb::STICK_CAPACITY;
use netlabcraft_model::{Device, DeviceKind};
use serde_json::json;

use crate::PacketApp;
use crate::i18n::{tl, tr_any, trf};
use crate::theme;

/// The Files page's form, kept between frames.
#[derive(Debug, Clone, Default)]
struct StickForm {
    name: String,
    text: String,
    current: Option<String>,
}

/// Why the cable of a device carries nothing, if it does not.
fn state_word(c: &netlabcraft_model::usb::UsbCable, problem: Option<String>) -> String {
    match problem {
        Some(why) => tr_any(&why),
        None if c.ejected => tl("ejected").to_string(),
        None => tl("mounted").to_string(),
    }
}

/// A stick's Files page: where it is plugged in, whether it is mounted, its files and a form to
/// put one more on it.
pub(crate) fn stick(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::new(("usb_form", dev.id.0));
    let mut f: StickForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    let tk = theme::tokens(app.ui.dark);
    let t = &app.session.topology;
    let cable = t.usb_of_device(dev.id).copied();
    let status = cable.map(|c| {
        let host = t.device(c.host).map_or(String::new(), |h| h.name.clone());
        trf("Plugged into {} (USB {}): {}", &[&host, &(u16::from(c.host_port) + 1), &state_word(&c, t.usb_problem(&c))])
    });
    let usable = cable.is_some_and(|c| !c.ejected && t.usb_problem(&c).is_none());
    let (mut toggle, mut save, mut delete) = (false, false, false);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        match &status {
            Some(text) => ui.label(text),
            None => ui.label(RichText::new(tl("Not plugged in: join it to a host with a USB cable.")).color(tk.muted)),
        };
        if let Some(c) = cable {
            toggle = ui.button(if c.ejected { tl("Mount") } else { tl("Eject") }).clicked();
        }
    });
    ui.label(RichText::new(trf("{} of {} bytes used", &[&dev.usb.used(), &STICK_CAPACITY])).color(tk.muted));
    ui.separator();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(200.0);
            ui.label(RichText::new(tl("Files")).strong());
            egui::ScrollArea::vertical().id_salt(("usb_files", dev.id.0)).max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                for file in &dev.usb.files {
                    let on = f.current.as_deref() == Some(file.name.as_str());
                    if ui.selectable_label(on, &file.name).clicked() {
                        f.name.clone_from(&file.name);
                        f.text.clone_from(&file.text);
                        f.current = Some(file.name.clone());
                    }
                }
            });
            if dev.usb.files.is_empty() {
                ui.label(RichText::new(tl("No files on the stick.")).color(tk.muted));
            }
            delete = ui.add_enabled(usable && f.current.is_some(), egui::Button::new(tl("Delete"))).clicked();
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let l = ui.label(tl("File name"));
                ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(260.0)).labelled_by(l.id);
            });
            ui.add(egui::TextEdit::multiline(&mut f.text).font(egui::TextStyle::Monospace).desired_rows(10).desired_width(f32::INFINITY));
            save = ui.add_enabled(usable, egui::Button::new(tl("Save to stick"))).clicked();
        });
    });
    if toggle && let Some(c) = cable {
        app.run(if c.ejected { "usb.mount" } else { "usb.eject" }, json!({"device": dev.id.0}));
    }
    if save {
        let name = f.name.trim().to_string();
        if app.run("usb.put", json!({"device": dev.id.0, "name": name, "text": f.text})).is_some() {
            app.info(trf("Saved {}", &[&name]));
            f.current = Some(name);
        }
    }
    if delete
        && let Some(name) = f.current.clone()
        && app.run("usb.delete", json!({"device": dev.id.0, "name": name})).is_some()
    {
        app.info(trf("Deleted {}", &[&name]));
        f = StickForm::default();
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}

/// The USB app of a host: one row for each USB port, empty or with what is plugged into it.
pub(crate) fn host_app(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    let ports = dev.usb_host_ports();
    ui.add_space(6.0);
    if ports == 0 {
        ui.label(RichText::new(tl("This device has no USB port.")).color(tk.muted));
        return;
    }
    let (mut eject, mut open, mut print) = (None, None, None);
    let t = &app.session.topology;
    egui::Grid::new(("usb_app", dev.id.0)).num_columns(4).spacing([14.0, 8.0]).show(ui, |ui| {
        for port in 0..ports {
            ui.label(RichText::new(trf("USB {}", &[&(u16::from(port) + 1)])).strong());
            let Some(c) = t.usb_in_port(dev.id, port).copied() else {
                ui.label(RichText::new(tl("empty")).color(tk.muted));
                ui.end_row();
                continue;
            };
            let Some(x) = t.device(c.device) else {
                ui.end_row();
                continue;
            };
            ui.label(format!("{} ({})", x.name, tr_any(x.catalogue_model().label)));
            let problem = t.usb_problem(&c);
            let word = state_word(&c, problem.clone());
            ui.label(if problem.is_some() { RichText::new(word).color(tk.error) } else { RichText::new(word).color(tk.muted) });
            ui.horizontal(|ui| {
                if x.is_usb_stick() && ui.button(if c.ejected { tl("Mount") } else { tl("Eject") }).clicked() {
                    eject = Some((c.device, c.ejected));
                }
                if x.kind == DeviceKind::Printer && ui.button(tl("Print test page")).clicked() {
                    print = Some(c.device);
                }
                if ui.button(tl("Open")).clicked() {
                    open = Some(c.device);
                }
            });
            ui.end_row();
        }
    });
    if let Some((d, ejected)) = eject {
        app.run(if ejected { "usb.mount" } else { "usb.eject" }, json!({"device": d.0}));
    }
    if let Some(p) = print {
        let text = trf("Test page from {}", &[&dev.name]);
        if app.run("printer.print", json!({"from": dev.id.0, "printer": p.0, "text": text, "via": "usb"})).is_some() {
            app.info(tl("Sent a test page over USB."));
        }
    }
    if let Some(d) = open {
        crate::open_console(app, d);
    }
}
