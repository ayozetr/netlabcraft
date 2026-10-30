//! The Email app on a host's Desktop: set up the account, read the inbox, write and send.
//! Everything goes through the `mail.*` commands.

use egui::RichText;
use serde_json::json;

use crate::PacketApp;
use crate::i18n::tl;
use packetcraft_model::Device;

/// The app's state between frames.
#[derive(Debug, Clone, Default)]
struct MailForm {
    view: View,
    selected: Option<usize>,
    name: String,
    address: String,
    incoming: String,
    outgoing: String,
    user: String,
    password: String,
    to: String,
    subject: String,
    body: String,
    loaded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Inbox,
    Compose,
    Settings,
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, password: bool) {
    ui.horizontal(|ui| {
        let l = ui.add_sized([150.0, 22.0], egui::Label::new(label));
        ui.add(egui::TextEdit::singleline(value).password(password).desired_width(260.0)).labelled_by(l.id);
    });
}

pub fn app(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let id = egui::Id::unique(("mail_form", dev.id.0));
    let mut f: MailForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    if !f.loaded {
        if let Some(a) = &dev.mail {
            f.name.clone_from(&a.name);
            f.address.clone_from(&a.address);
            f.incoming.clone_from(&a.incoming);
            f.outgoing.clone_from(&a.outgoing);
            f.user.clone_from(&a.user);
            f.password.clone_from(&a.password);
        } else {
            f.view = View::Settings;
        }
        f.loaded = true;
    }
    ui.horizontal(|ui| {
        for (v, label) in [(View::Inbox, "Inbox"), (View::Compose, "Compose"), (View::Settings, "Account")] {
            if ui.selectable_label(f.view == v, tl(label)).clicked() {
                f.view = v;
            }
        }
        if ui.button(tl("Receive")).clicked()
            && let Some(r) = app.run("mail.receive", json!({"device": dev.id.0}))
        {
            let n = r["received"].as_u64().unwrap_or(0);
            app.info(crate::i18n::trf("{} new message(s)", &[&n]));
            f.view = View::Inbox;
        }
    });
    ui.separator();
    match f.view {
        View::Settings => {
            ui.label(RichText::new(tl("Your account")).strong());
            field(ui, tl("Your name"), &mut f.name, false);
            field(ui, tl("Email address"), &mut f.address, false);
            field(ui, tl("Incoming (POP3) server"), &mut f.incoming, false);
            field(ui, tl("Outgoing (SMTP) server"), &mut f.outgoing, false);
            field(ui, tl("User name"), &mut f.user, false);
            field(ui, tl("Password"), &mut f.password, true);
            if ui.button(tl("Save")).clicked()
                && app
                    .run(
                        "mail.configure",
                        json!({"device": dev.id.0, "name": f.name, "address": f.address, "incoming": f.incoming, "outgoing": f.outgoing, "user": f.user, "password": f.password}),
                    )
                    .is_some()
            {
                f.view = View::Inbox;
            }
        }
        View::Compose => {
            field(ui, tl("To"), &mut f.to, false);
            field(ui, tl("Subject"), &mut f.subject, false);
            ui.add(egui::TextEdit::multiline(&mut f.body).desired_rows(8).desired_width(f32::INFINITY));
            if ui.button(tl("Send")).clicked()
                && app.run("mail.send", json!({"device": dev.id.0, "to": f.to.trim(), "subject": f.subject, "body": f.body})).is_some()
            {
                app.info(crate::i18n::trf("Sent to {}", &[&f.to.trim()]));
                f.to.clear();
                f.subject.clear();
                f.body.clear();
                f.view = View::Inbox;
            }
        }
        View::Inbox => {
            if dev.inbox.is_empty() {
                ui.label(tl("No messages. Press Receive to fetch new mail."));
            }
            egui::ScrollArea::vertical().max_height(140.0).id_salt(("mail_list", dev.id.0)).show(ui, |ui| {
                egui::Grid::new(("mail_grid", dev.id.0)).striped(true).show(ui, |ui| {
                    if !dev.inbox.is_empty() {
                        ui.strong(tl("From"));
                        ui.strong(tl("Subject"));
                        ui.end_row();
                    }
                    for (i, m) in dev.inbox.iter().enumerate().rev() {
                        if ui.selectable_label(f.selected == Some(i), &m.from).clicked() {
                            f.selected = Some(i);
                        }
                        ui.label(&m.subject);
                        ui.end_row();
                    }
                });
            });
            if let Some(m) = f.selected.and_then(|i| dev.inbox.get(i)) {
                ui.separator();
                ui.label(RichText::new(&m.subject).strong());
                ui.label(RichText::new(crate::i18n::trf("From {} to {}", &[&m.from, &m.to])).small());
                egui::ScrollArea::vertical().max_height(140.0).id_salt(("mail_body", dev.id.0)).show(ui, |ui| {
                    ui.label(&m.body);
                });
            }
        }
    }
    ui.data_mut(|m| m.insert_temp(id, f));
}
