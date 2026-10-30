//! A server's Services tab (HTTP, DHCP, DNS) and the PC's Web Browser. Changes are applied with
//! the `server.services` command (undoable); pages are fetched with `browser.get`.

use std::net::Ipv4Addr;

use egui::{RichText, Ui, vec2};
use netlabcraft_core::DeviceId;
use netlabcraft_model::Device;
use netlabcraft_model::services::{DnsRecord, ServerPool, Services, WebFile};
use serde_json::{Value, json};

use crate::i18n::tl;
use crate::{PacketApp, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Http,
    Dhcp,
    Dns,
    Syslog,
    Ntp,
    Tftp,
    Aaa,
    Ftp,
    Email,
}

/// Form state of the Services tab and the browser, per device window.
#[derive(Debug, Clone, Default)]
pub struct ServiceForms {
    pub page: Page,
    /// The web file being edited: (original name, name, content).
    editing: Option<(Option<String>, String, String)>,
    pool_name: String,
    pool_gw: String,
    pool_dns: String,
    pool_start: [String; 4],
    pool_mask: [String; 4],
    pool_max: String,
    pool_sel: Option<usize>,
    dns_name: String,
    dns_addr: String,
    dns_sel: Option<usize>,
    aaa_client: String,
    aaa_client_ip: String,
    aaa_secret: String,
    aaa_user: String,
    aaa_password: String,
    ftp_user: String,
    ftp_password: String,
    ftp_perms: [bool; 4],
    mail_user: String,
    mail_password: String,
    // Browser.
    pub url: String,
    /// Press Go on the next frame.
    pub go_pending: bool,
    history: Vec<String>,
    pos: usize,
    page_body: Option<(String, String)>,
    error: Option<String>,
}

fn apply(app: &mut PacketApp, d: DeviceId, sv: &Services) {
    if let Ok(v) = serde_json::to_value(sv) {
        app.run("server.services", json!({"device": d.0, "services": v}));
    }
}

fn on_off(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        crate::console::form_label(ui, label, crate::console::LABEL_W);
        changed |= ui.radio_value(on, true, tl("On")).changed();
        ui.add_space(60.0);
        changed |= ui.radio_value(on, false, tl("Off")).changed();
    });
    changed
}

fn octets(ui: &mut Ui, label: &str, boxes: &mut [String; 4]) {
    ui.horizontal(|ui| {
        let l = crate::console::form_label(ui, label, crate::console::LABEL_W);
        for b in boxes.iter_mut() {
            ui.add(egui::TextEdit::singleline(b).desired_width(56.0)).labelled_by(l.id);
        }
    });
}

fn split(a: Ipv4Addr) -> [String; 4] {
    let o = a.octets();
    [o[0].to_string(), o[1].to_string(), o[2].to_string(), o[3].to_string()]
}

fn join(b: &[String; 4]) -> Option<Ipv4Addr> {
    let mut o = [0u8; 4];
    for (i, s) in b.iter().enumerate() {
        *o.get_mut(i)? = s.trim().parse().ok()?;
    }
    Some(Ipv4Addr::from(o))
}

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        let l = crate::console::form_label(ui, label, crate::console::LABEL_W);
        ui.add(egui::TextEdit::singleline(value).desired_width(260.0)).labelled_by(l.id);
    });
}

/// The Services tab of a server.
pub fn tab(app: &mut PacketApp, ui: &mut Ui, dev: &Device, forms: &mut ServiceForms) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    ui.horizontal_top(|ui| {
        egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.text)).inner_margin(2.0).show(ui, |ui| {
            ui.set_width(170.0);
            ui.set_height(400.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                crate::console::list_item(ui, tl("SERVICES"), false, true);
                for (p, name) in [
                    (Page::Http, "HTTP"),
                    (Page::Dhcp, "DHCP"),
                    (Page::Dns, "DNS"),
                    (Page::Syslog, "SYSLOG"),
                    (Page::Ntp, "NTP"),
                    (Page::Tftp, "TFTP"),
                    (Page::Aaa, "AAA"),
                    (Page::Ftp, "FTP"),
                    (Page::Email, "EMAIL"),
                ] {
                    if crate::console::list_item(ui, name, forms.page == p, false).clicked() {
                        forms.page = p;
                    }
                }
            });
        });
        egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.chrome).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(520.0);
            ui.set_min_height(380.0);
            ui.vertical(|ui| match forms.page {
                Page::Http => http(app, ui, d, &dev.services, forms),
                Page::Dhcp => dhcp(app, ui, dev, forms),
                Page::Dns => dns(app, ui, d, &dev.services, forms),
                Page::Syslog => syslog(app, ui, d, &dev.services),
                Page::Ntp => ntp(app, ui, d, &dev.services),
                Page::Tftp => tftp(app, ui, d, &dev.services),
                Page::Aaa => aaa(app, ui, d, &dev.services, forms),
                Page::Ftp => ftp(app, ui, d, &dev.services, forms),
                Page::Email => email(app, ui, d, &dev.services, forms),
            });
        });
    });
}

fn http(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services, forms: &mut ServiceForms) {
    let tk = theme::tokens(app.ui.dark);
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("HTTP")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    // Two boxes of one height, their text on one middle line.
    let boxed = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(230.0);
            ui.allocate_ui_with_layout(vec2(230.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| add(ui));
        });
    };
    ui.horizontal(|ui| {
        boxed(ui, &mut |ui| {
            ui.label(tl("HTTP"));
            if ui.radio_value(&mut new.http.on, true, tl("On")).changed() | ui.radio_value(&mut new.http.on, false, tl("Off")).changed() {
                apply(app, d, &new);
            }
        });
        boxed(ui, &mut |ui| {
            ui.label(tl("HTTPS"));
            ui.label(RichText::new(tl("Not simulated yet")).color(tk.muted));
        });
    });
    if let Some((orig, name, content)) = &mut forms.editing {
        ui.label(RichText::new(tl("Edit file")).strong());
        field(ui, tl("File Name"), name);
        egui::ScrollArea::vertical().id_salt("web_edit").max_height(230.0).show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(content).code_editor().desired_width(f32::INFINITY).desired_rows(12));
        });
        let mut done = None;
        ui.horizontal(|ui| {
            if ui.button(tl("Save")).clicked() {
                done = Some(true);
            }
            if ui.button(tl("Cancel")).clicked() {
                done = Some(false);
            }
        });
        if let Some(save) = done {
            if save {
                let n = name.trim().to_string();
                new.http.files.retain(|f| Some(&f.name) != orig.as_ref() && f.name != n);
                new.http.files.push(WebFile { name: n, content: content.clone() });
                new.http.files.sort_by(|a, b| a.name.cmp(&b.name));
                apply(app, d, &new);
            }
            forms.editing = None;
        }
        return;
    }
    ui.label(tl("File Manager"));
    let mut edit = None;
    let mut delete = None;
    egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.canvas).inner_margin(4.0).show(ui, |ui| {
        ui.set_min_height(220.0);
        ui.set_width(ui.available_width());
        egui::Grid::new(("web_files", d.0)).striped(true).num_columns(4).min_col_width(40.0).show(ui, |ui| {
            for h in ["", "File Name", "Edit", "Delete"] {
                ui.strong(crate::i18n::tr(h));
            }
            ui.end_row();
            for (i, f) in sv.http.files.iter().enumerate() {
                ui.label((i + 1).to_string());
                ui.label(&f.name);
                if ui.link(tl("(edit)")).clicked() {
                    edit = Some(f.clone());
                }
                if ui.link(tl("(delete)")).clicked() {
                    delete = Some(f.name.clone());
                }
                ui.end_row();
            }
        });
    });
    if let Some(f) = edit {
        forms.editing = Some((Some(f.name.clone()), f.name, f.content));
    }
    if let Some(n) = delete {
        new.http.files.retain(|f| f.name != n);
        apply(app, d, &new);
    }
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tl("Import")).on_hover_text(tl("Add an .html file from your computer")).clicked() {
                app.requests.push(crate::UiRequest::ImportWebFile { device: d });
            }
            if ui.button(tl("New File")).clicked() {
                forms.editing = Some((None, "newpage.html".into(), "<html>\n<body>\n<h1>New page</h1>\n</body>\n</html>\n".into()));
            }
        });
    });
}

fn dhcp(app: &mut PacketApp, ui: &mut Ui, dev: &Device, forms: &mut ServiceForms) {
    let tk = theme::tokens(app.ui.dark);
    let d = dev.id;
    let sv = &dev.services;
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("DHCP")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    ui.horizontal(|ui| {
        crate::console::form_label(ui, tl("Interface"), crate::console::LABEL_W);
        ui.label(dev.ports.first().map_or("-", |p| p.name.as_str()));
        ui.add_space(12.0);
        ui.label(tl("Service"));
        if ui.radio_value(&mut new.dhcp.on, true, tl("On")).changed() | ui.radio_value(&mut new.dhcp.on, false, tl("Off")).changed() {
            apply(app, d, &new);
        }
    });
    if forms.pool_max.is_empty() {
        forms.pool_max = "50".into();
        forms.pool_start = split(Ipv4Addr::new(192, 168, 0, 10));
        forms.pool_mask = split(Ipv4Addr::new(255, 255, 255, 0));
        forms.pool_gw = "0.0.0.0".into();
        forms.pool_dns = "0.0.0.0".into();
    }
    field(ui, tl("Pool Name"), &mut forms.pool_name);
    field(ui, tl("Default Gateway"), &mut forms.pool_gw);
    field(ui, tl("DNS Server"), &mut forms.pool_dns);
    octets(ui, tl("Start IP Address"), &mut forms.pool_start);
    octets(ui, tl("Subnet Mask"), &mut forms.pool_mask);
    field(ui, tl("Maximum Number of Users"), &mut forms.pool_max);
    let pool = || -> Result<ServerPool, String> {
        let ip = |s: &str, what: &'static str| {
            s.trim().parse::<Ipv4Addr>().map_err(|_| crate::i18n::trf("{}: `{}` is not an address", &[&tl(what), &s.trim()]))
        };
        let name = forms.pool_name.trim();
        if name.is_empty() {
            return Err(tl("Give the pool a name").into());
        }
        Ok(ServerPool {
            name: name.into(),
            gateway: ip(&forms.pool_gw, "Default Gateway")?,
            dns: ip(&forms.pool_dns, "DNS Server")?,
            start: join(&forms.pool_start).ok_or(tl("Start IP Address: four numbers 0-255"))?,
            mask: join(&forms.pool_mask).ok_or(tl("Subnet Mask: four numbers 0-255"))?,
            max_users: forms.pool_max.trim().parse().map_err(|_| tl("Maximum Number of Users: a number"))?,
        })
    };
    let mut action = None;
    ui.columns(3, |c| {
        if let Some(c0) = c.get_mut(0)
            && c0.add_sized([c0.available_width(), 26.0], egui::Button::new(tl("Add"))).clicked()
        {
            action = Some(0);
        }
        if let Some(c1) = c.get_mut(1)
            && c1.add_sized([c1.available_width(), 26.0], egui::Button::new(tl("Save"))).clicked()
        {
            action = Some(1);
        }
        if let Some(c2) = c.get_mut(2)
            && c2.add_enabled(forms.pool_sel.is_some(), egui::Button::new(tl("Remove")).min_size(vec2(c2.available_width(), 26.0))).clicked()
        {
            action = Some(2);
        }
    });
    match action {
        Some(0 | 1) => match pool() {
            Ok(p) => {
                match (action, forms.pool_sel) {
                    (Some(1), Some(i)) if i < new.dhcp.pools.len() => {
                        if let Some(slot) = new.dhcp.pools.get_mut(i) {
                            *slot = p;
                        }
                    }
                    _ => {
                        new.dhcp.pools.retain(|x| x.name != p.name);
                        new.dhcp.pools.push(p);
                    }
                }
                apply(app, d, &new);
            }
            Err(e) => app.ui.status = Some((e, true)),
        },
        Some(_) => {
            if let Some(i) = forms.pool_sel.take()
                && i < new.dhcp.pools.len()
            {
                new.dhcp.pools.remove(i);
                apply(app, d, &new);
            }
        }
        None => {}
    }
    egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.canvas).inner_margin(4.0).show(ui, |ui| {
        ui.set_min_height(120.0);
        ui.set_width(ui.available_width());
        egui::ScrollArea::horizontal().id_salt("pools").show(ui, |ui| {
            egui::Grid::new(("pools", d.0)).striped(true).num_columns(6).show(ui, |ui| {
                for h in ["Pool Name", "Default Gateway", "DNS Server", "Start IP Address", "Subnet Mask", "Max User"] {
                    ui.strong(crate::i18n::tr(h));
                }
                ui.end_row();
                for (i, p) in sv.dhcp.pools.iter().enumerate() {
                    if ui.selectable_label(forms.pool_sel == Some(i), &p.name).clicked() {
                        forms.pool_sel = Some(i);
                        forms.pool_name = p.name.clone();
                        forms.pool_gw = p.gateway.to_string();
                        forms.pool_dns = p.dns.to_string();
                        forms.pool_start = split(p.start);
                        forms.pool_mask = split(p.mask);
                        forms.pool_max = p.max_users.to_string();
                    }
                    ui.label(p.gateway.to_string());
                    ui.label(p.dns.to_string());
                    ui.label(p.start.to_string());
                    ui.label(p.mask.to_string());
                    ui.label(p.max_users.to_string());
                    ui.end_row();
                }
            });
        });
    });
}

fn dns(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services, forms: &mut ServiceForms) {
    let tk = theme::tokens(app.ui.dark);
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("DNS")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    if on_off(ui, tl("DNS Service"), &mut new.dns.on) {
        apply(app, d, &new);
    }
    ui.separator();
    ui.label(tl("Resource Records"));
    ui.horizontal(|ui| {
        let l = crate::console::form_label(ui, tl("Name"), crate::console::LABEL_W);
        ui.add(egui::TextEdit::singleline(&mut forms.dns_name).desired_width(170.0)).labelled_by(l.id);
        ui.label(tl("Type"));
        ui.label(RichText::new(tl("A Record")).strong());
    });
    field(ui, tl("Address"), &mut forms.dns_addr);
    let mut action = None;
    ui.columns(3, |c| {
        for (i, label) in ["Add", "Save", "Remove"].into_iter().enumerate() {
            if let Some(col) = c.get_mut(i) {
                let enabled = i == 0 || forms.dns_sel.is_some();
                if col.add_enabled(enabled, egui::Button::new(tl(label)).min_size(vec2(col.available_width(), 26.0))).clicked() {
                    action = Some(i);
                }
            }
        }
    });
    if let Some(a) = action {
        let rec =
            forms.dns_addr.trim().parse::<Ipv4Addr>().ok().map(|address| DnsRecord { name: forms.dns_name.trim().to_ascii_lowercase(), address });
        match (a, rec, forms.dns_sel) {
            (2, _, Some(i)) => {
                if i < new.dns.records.len() {
                    new.dns.records.remove(i);
                }
                forms.dns_sel = None;
                apply(app, d, &new);
            }
            (_, Some(r), _) if r.name.is_empty() => app.ui.status = Some((tl("Give the record a name, such as www.example.lab").into(), true)),
            (1, Some(r), Some(i)) => {
                if let Some(slot) = new.dns.records.get_mut(i) {
                    *slot = r;
                }
                apply(app, d, &new);
            }
            (_, Some(r), _) => {
                new.dns.records.retain(|x| x.name != r.name);
                new.dns.records.push(r);
                apply(app, d, &new);
            }
            (_, None, _) => app.ui.status = Some((crate::i18n::trf("`{}` is not an IPv4 address", &[&forms.dns_addr.trim()]), true)),
        }
    }
    egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.canvas).inner_margin(4.0).show(ui, |ui| {
        ui.set_min_height(160.0);
        ui.set_width(ui.available_width());
        egui::Grid::new(("dns_records", d.0)).striped(true).num_columns(4).min_col_width(60.0).show(ui, |ui| {
            for h in ["No.", "Name", "Type", "Detail"] {
                ui.strong(crate::i18n::tr(h));
            }
            ui.end_row();
            for (i, r) in sv.dns.records.iter().enumerate() {
                ui.label(i.to_string());
                if ui.selectable_label(forms.dns_sel == Some(i), &r.name).clicked() {
                    forms.dns_sel = Some(i);
                    forms.dns_name = r.name.clone();
                    forms.dns_addr = r.address.to_string();
                }
                ui.label(tl("A Record"));
                ui.label(r.address.to_string());
                ui.end_row();
            }
        });
    });
}

/// The Web Browser desktop app.
pub fn browser(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, forms: &mut ServiceForms) {
    let tk = theme::tokens(app.ui.dark);
    let mut go: Option<String> = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(forms.pos > 0, egui::Button::new("<")).on_hover_text(tl("Back")).clicked() {
            forms.pos -= 1;
            go = forms.history.get(forms.pos).cloned();
        }
        if ui.add_enabled(forms.pos + 1 < forms.history.len(), egui::Button::new(">")).on_hover_text(tl("Forward")).clicked() {
            forms.pos += 1;
            go = forms.history.get(forms.pos).cloned();
        }
        let l = ui.label(tl("URL"));
        let r = ui.add(egui::TextEdit::singleline(&mut forms.url).desired_width(ui.available_width() - 110.0)).labelled_by(l.id);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let pending = std::mem::take(&mut forms.go_pending);
        if ui.button(tl("Go")).clicked() || enter || pending {
            let u = forms.url.trim().to_string();
            if !u.is_empty() {
                forms.history.truncate(forms.pos + usize::from(!forms.history.is_empty()));
                forms.history.push(u.clone());
                forms.pos = forms.history.len() - 1;
                go = Some(u);
            }
        }
        if ui.button(tl("Stop")).clicked() {
            forms.error = Some(tl("Stopped.").into());
        }
    });
    if let Some(u) = go {
        let url = if u.contains("://") { u } else { format!("http://{u}") };
        forms.url = url.clone();
        match app.session.execute("browser.get", json!({"device": d.0, "url": url})) {
            Ok(v) => {
                forms.error = None;
                forms.page_body = Some((
                    v.get("status").and_then(Value::as_str).unwrap_or("").to_string(),
                    v.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
                ));
                if let Some(u) = v.get("url").and_then(Value::as_str) {
                    forms.url = u.to_string();
                }
            }
            Err(e) => {
                forms.page_body = None;
                forms.error = Some(crate::i18n::trf("Request timed out or failed: {}", &[&e]));
            }
        }
    }
    let mut follow: Option<String> = None;
    egui::Frame::new().fill(egui::Color32::WHITE).stroke(egui::Stroke::new(1.0, tk.chrome_line)).inner_margin(10.0).show(ui, |ui| {
        ui.set_min_size(vec2(ui.available_width(), 300.0));
        egui::ScrollArea::vertical().id_salt(("page", d.0)).show(ui, |ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(0x10, 0x10, 0x10));
            if let Some(e) = &forms.error {
                ui.label(e);
            } else if let Some((status, body)) = &forms.page_body {
                if !status.starts_with("200") {
                    ui.label(RichText::new(status).strong());
                }
                follow = render_html(ui, body);
            }
        });
    });
    if let Some(href) = follow {
        let base = forms.url.trim_start_matches("http://").split('/').next().unwrap_or("").to_string();
        let target = if href.contains("://") { href } else { format!("http://{base}/{}", href.trim_start_matches('/')) };
        forms.url = target;
        // Navigate on the next frame through the same path as Go.
        forms.history.truncate(forms.pos + 1);
        forms.history.push(forms.url.clone());
        forms.pos = forms.history.len() - 1;
        if let Ok(v) = app.session.execute("browser.get", json!({"device": d.0, "url": forms.url})) {
            forms.error = None;
            forms.page_body = Some((
                v.get("status").and_then(Value::as_str).unwrap_or("").to_string(),
                v.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
            ));
        } else {
            forms.error = Some(tl("Request timed out.").into());
        }
    }
}

/// One piece of inline text.
#[derive(Debug, Clone, PartialEq)]
struct Span {
    text: String,
    size: f32,
    bold: bool,
    link: Option<String>,
}

fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&nbsp;", " ").replace("&#39;", "'").replace("&amp;", "&")
}

/// Lays out simple HTML (headings, paragraphs, line breaks, bold, links, list items) as blocks
/// of spans. Scripts, styles and unknown tags are ignored.
fn layout_html(html: &str) -> Vec<Vec<Span>> {
    let mut blocks: Vec<Vec<Span>> = Vec::new();
    let mut cur: Vec<Span> = Vec::new();
    let (mut size, mut bold, mut link, mut skip) = (14.0_f32, 0u32, None::<String>, false);
    let mut rest = html;
    let flush = |cur: &mut Vec<Span>, blocks: &mut Vec<Vec<Span>>| {
        if cur.iter().any(|s| !s.text.trim().is_empty()) {
            blocks.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    let mut guard = 0;
    while !rest.is_empty() && guard < 100_000 {
        guard += 1;
        match rest.find('<') {
            Some(0) => {
                let end = rest.find('>').map_or(rest.len(), |e| e + 1);
                let tag = rest.get(1..end.saturating_sub(1)).unwrap_or("").trim().to_string();
                rest = rest.get(end..).unwrap_or("");
                let closing = tag.starts_with('/');
                let name = tag.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                match name.as_str() {
                    "script" | "style" | "head" | "title" => skip = !closing,
                    "h1" | "h2" | "h3" | "h4" => {
                        flush(&mut cur, &mut blocks);
                        size = if closing {
                            14.0
                        } else {
                            match name.as_str() {
                                "h1" => 26.0,
                                "h2" => 21.0,
                                "h3" => 18.0,
                                _ => 16.0,
                            }
                        };
                        bold = if closing { bold.saturating_sub(1) } else { bold + 1 };
                    }
                    "p" | "div" | "br" | "li" | "tr" | "ul" | "ol" | "table" | "hr" => {
                        flush(&mut cur, &mut blocks);
                        if name == "li" && !closing {
                            cur.push(Span { text: "• ".into(), size, bold: bold > 0, link: None });
                        }
                    }
                    "b" | "strong" => bold = if closing { bold.saturating_sub(1) } else { bold + 1 },
                    "a" => {
                        link = if closing {
                            None
                        } else {
                            tag.split("href=")
                                .nth(1)
                                .map(|h| h.trim_start_matches(['"', '\'']).split(['"', '\'', ' ']).next().unwrap_or("").to_string())
                        }
                    }
                    "img" => cur.push(Span { text: "[image]".into(), size, bold: false, link: None }),
                    _ => {}
                }
            }
            Some(i) => {
                let text = rest.get(..i).unwrap_or("");
                rest = rest.get(i..).unwrap_or("");
                if !skip {
                    let t = decode_entities(&text.split_whitespace().collect::<Vec<_>>().join(" "));
                    if !t.is_empty() {
                        cur.push(Span { text: t + " ", size, bold: bold > 0, link: link.clone() });
                    }
                }
            }
            None => {
                if !skip {
                    let t = decode_entities(&rest.split_whitespace().collect::<Vec<_>>().join(" "));
                    if !t.is_empty() {
                        cur.push(Span { text: t, size, bold: bold > 0, link: link.clone() });
                    }
                }
                rest = "";
            }
        }
    }
    flush(&mut cur, &mut blocks);
    blocks
}

/// Draws a page; returns the link the person clicked, if any.
fn render_html(ui: &mut Ui, html: &str) -> Option<String> {
    let mut clicked = None;
    for block in layout_html(html) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for s in block {
                let mut rt = RichText::new(&s.text).size(s.size);
                if s.bold {
                    rt = rt.strong();
                }
                match &s.link {
                    Some(href) => {
                        if ui.link(rt.color(egui::Color32::from_rgb(0x1a, 0x4f, 0xd6))).clicked() {
                            clicked = Some(href.clone());
                        }
                    }
                    None => {
                        ui.label(rt);
                    }
                }
            }
        });
        ui.add_space(6.0);
    }
    clicked
}

fn syslog(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services) {
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("Syslog")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    if on_off(ui, tl("Syslog Service"), &mut new.syslog.on) {
        apply(app, d, &new);
    }
    ui.separator();
    let msgs = app.session.sim.state(d).map(|s| s.syslog.clone()).unwrap_or_default();
    ui.label(crate::i18n::trf("Messages received: {}", &[&msgs.len()]));
    egui::ScrollArea::vertical().max_height(260.0).id_salt("syslog_list").show(ui, |ui| {
        egui::Grid::new("syslog_grid").striped(true).show(ui, |ui| {
            ui.label(RichText::new(tl("Time (s)")).strong());
            ui.label(RichText::new(tl("From")).strong());
            ui.label(RichText::new(tl("Message")).strong());
            ui.end_row();
            for (at, from, m) in msgs.iter().rev() {
                ui.label(format!("{:.1}", *at as f64 / 1e6));
                ui.label(from.to_string());
                ui.label(m);
                ui.end_row();
            }
        });
    });
}

fn aaa(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services, forms: &mut ServiceForms) {
    use netlabcraft_model::services::{AaaClient, AaaUser};
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("AAA")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    if on_off(ui, tl("AAA Service (RADIUS, TACACS+)"), &mut new.aaa.on) {
        apply(app, d, &new);
    }
    ui.separator();
    ui.label(RichText::new(tl("Clients")).strong());
    field(ui, tl("Client name"), &mut forms.aaa_client);
    field(ui, tl("Client address"), &mut forms.aaa_client_ip);
    field(ui, tl("Secret"), &mut forms.aaa_secret);
    if ui.button(tl("Add client")).clicked() {
        match forms.aaa_client_ip.trim().parse::<Ipv4Addr>() {
            Ok(address) if !forms.aaa_secret.trim().is_empty() => {
                new.aaa.clients.retain(|c| c.address != address);
                new.aaa.clients.push(AaaClient { name: forms.aaa_client.trim().to_string(), address, secret: forms.aaa_secret.trim().to_string() });
                apply(app, d, &new);
            }
            Ok(_) => app.ui.status = Some((tl("Give the secret the router has in `radius-server host ... key`").into(), true)),
            Err(_) => app.ui.status = Some((crate::i18n::trf("`{}` is not an IPv4 address", &[&forms.aaa_client_ip.trim()]), true)),
        }
    }
    let mut changed = false;
    egui::Grid::new(("aaa_clients", d.0)).striped(true).show(ui, |ui| {
        for h in ["Name", "Address", "Secret", ""] {
            ui.strong(crate::i18n::tr(h));
        }
        ui.end_row();
        let mut remove = None;
        for (i, c) in new.aaa.clients.iter().enumerate() {
            ui.label(&c.name);
            ui.label(c.address.to_string());
            ui.label(&c.secret);
            if ui.small_button(tl("Remove")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
        if let Some(i) = remove {
            new.aaa.clients.remove(i);
            changed = true;
        }
    });
    ui.separator();
    ui.label(RichText::new(tl("Users")).strong());
    field(ui, tl("Username"), &mut forms.aaa_user);
    field(ui, tl("Password"), &mut forms.aaa_password);
    if ui.button(tl("Add user")).clicked() && !forms.aaa_user.trim().is_empty() {
        let name = forms.aaa_user.trim().to_string();
        new.aaa.users.retain(|u| u.name != name);
        new.aaa.users.push(AaaUser { name, password: forms.aaa_password.clone() });
        changed = true;
    }
    egui::Grid::new(("aaa_users", d.0)).striped(true).show(ui, |ui| {
        for h in ["Username", "Password", ""] {
            ui.strong(crate::i18n::tr(h));
        }
        ui.end_row();
        let mut remove = None;
        for (i, u) in new.aaa.users.iter().enumerate() {
            ui.label(&u.name);
            ui.label(&u.password);
            if ui.small_button(tl("Remove")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
        if let Some(i) = remove {
            new.aaa.users.remove(i);
            changed = true;
        }
    });
    if changed {
        apply(app, d, &new);
    }
}

fn ftp(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services, forms: &mut ServiceForms) {
    use netlabcraft_model::services::FtpUser;
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("FTP")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    let mut changed = on_off(ui, tl("FTP Service"), &mut new.ftp.on);
    ui.separator();
    field(ui, tl("Username"), &mut forms.ftp_user);
    field(ui, tl("Password"), &mut forms.ftp_password);
    ui.horizontal(|ui| {
        for (i, label) in ["Read", "Write", "Delete", "List"].into_iter().enumerate() {
            if let Some(p) = forms.ftp_perms.get_mut(i) {
                ui.checkbox(p, crate::i18n::tr(label));
            }
        }
    });
    if ui.button(tl("Add user")).clicked() && !forms.ftp_user.trim().is_empty() {
        let name = forms.ftp_user.trim().to_string();
        let [read, write, delete, list] = forms.ftp_perms;
        new.ftp.users.retain(|u| u.name != name);
        new.ftp.users.push(FtpUser { name, password: forms.ftp_password.clone(), read, write, delete, list });
        changed = true;
    }
    egui::Grid::new(("ftp_users", d.0)).striped(true).show(ui, |ui| {
        for h in ["Username", "Password", "Permissions", ""] {
            ui.strong(crate::i18n::tr(h));
        }
        ui.end_row();
        let mut remove = None;
        for (i, u) in new.ftp.users.iter().enumerate() {
            ui.label(&u.name);
            ui.label(&u.password);
            let perms: String =
                [(u.read, 'R'), (u.write, 'W'), (u.delete, 'D'), (u.list, 'L')].iter().map(|(on, c)| if *on { *c } else { '-' }).collect();
            ui.monospace(perms);
            if ui.small_button(tl("Remove")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
        if let Some(i) = remove {
            new.ftp.users.remove(i);
            changed = true;
        }
    });
    ui.separator();
    ui.label(RichText::new(tl("Files")).strong());
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(140.0).id_salt("ftp_files").show(ui, |ui| {
        egui::Grid::new(("ftp_files_grid", d.0)).striped(true).show(ui, |ui| {
            for (i, f) in new.ftp.files.iter().enumerate() {
                ui.label(&f.name);
                ui.label(format!("{} B", f.content.len()));
                if ui.small_button(tl("Remove")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = remove {
        new.ftp.files.remove(i);
        changed = true;
    }
    if changed {
        apply(app, d, &new);
    }
}

fn email(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services, forms: &mut ServiceForms) {
    use netlabcraft_model::services::AaaUser;
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("EMAIL")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    let mut changed = on_off(ui, tl("SMTP and POP3 Service"), &mut new.email.on);
    let mut domain = new.email.domain.clone();
    ui.horizontal(|ui| {
        let l = crate::console::form_label(ui, tl("Domain Name"), crate::console::LABEL_W);
        let r = ui.add(egui::TextEdit::singleline(&mut domain).desired_width(200.0)).labelled_by(l.id);
        if r.lost_focus() && domain.trim() != new.email.domain {
            new.email.domain = domain.trim().to_string();
            changed = true;
        }
    });
    ui.separator();
    ui.label(RichText::new(tl("Users")).strong());
    field(ui, tl("Username"), &mut forms.mail_user);
    field(ui, tl("Password"), &mut forms.mail_password);
    if ui.button(tl("Add user")).clicked() && !forms.mail_user.trim().is_empty() {
        let name = forms.mail_user.trim().to_string();
        new.email.users.retain(|u| u.name != name);
        new.email.users.push(AaaUser { name, password: forms.mail_password.clone() });
        changed = true;
    }
    egui::Grid::new(("mail_users", d.0)).striped(true).show(ui, |ui| {
        let mut remove = None;
        for (i, u) in new.email.users.iter().enumerate() {
            let waiting = new.email.mailboxes.iter().filter(|(x, _)| x.eq_ignore_ascii_case(&u.name)).count();
            ui.label(format!("{}@{}", u.name, new.email.domain));
            ui.label(&u.password);
            ui.label(crate::i18n::trf("{} waiting", &[&waiting]));
            if ui.small_button(tl("Remove")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
        if let Some(i) = remove {
            new.email.users.remove(i);
            changed = true;
        }
    });
    if changed {
        apply(app, d, &new);
    }
}

fn ntp(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services) {
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("NTP")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    if on_off(ui, tl("NTP Service"), &mut new.ntp.on) {
        apply(app, d, &new);
    }
    ui.separator();
    let (now, _) = app.session.sim.clock(d);
    let secs = now.rem_euclid(86_400);
    let clock = format!("{:02}:{:02}:{:02}", secs / 3_600, secs / 60 % 60, secs % 60);
    ui.label(crate::i18n::trf("This server's time: {} UTC (day {} of the Unix era)", &[&clock, &now.div_euclid(86_400)]));
    ui.label(tl("Routers and switches with `ntp server <this address>` take this time."));
}

fn tftp(app: &mut PacketApp, ui: &mut Ui, d: DeviceId, sv: &Services) {
    ui.vertical_centered(|ui| ui.label(RichText::new(tl("TFTP")).size(16.0)));
    ui.separator();
    let mut new = sv.clone();
    if on_off(ui, tl("TFTP Service"), &mut new.tftp.on) {
        apply(app, d, &new);
    }
    ui.separator();
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(260.0).id_salt("tftp_files").show(ui, |ui| {
        egui::Grid::new("tftp_grid").striped(true).show(ui, |ui| {
            ui.label(RichText::new(tl("File")).strong());
            ui.label(RichText::new(tl("Bytes")).strong());
            ui.label("");
            ui.end_row();
            for (i, f) in sv.tftp.files.iter().enumerate() {
                ui.label(&f.name);
                ui.label(f.content.len().to_string());
                if ui.small_button(tl("Remove")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if sv.tftp.files.is_empty() {
        ui.label(tl("No files yet: `copy running-config tftp: <this address>` on a router saves one here."));
    }
    if let Some(i) = remove {
        new.tftp.files.remove(i);
        apply(app, d, &new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_layout() {
        let b = layout_html(
            "<html><head><title>x</title></head><body><h1>Hi &amp; bye</h1><p>One <b>two</b> <a href=\"about.html\">link</a></p><script>bad()</script><ul><li>a</li></ul></body></html>",
        );
        assert_eq!(b.len(), 3);
        assert_eq!(b[0][0].text.trim(), "Hi & bye");
        assert!(b[0][0].size > 20.0 && b[0][0].bold);
        assert!(b[1].iter().any(|s| s.bold && s.text.trim() == "two"));
        assert_eq!(b[1].iter().find(|s| s.link.is_some()).and_then(|s| s.link.clone()).as_deref(), Some("about.html"));
        assert!(!b.iter().flatten().any(|s| s.text.contains("bad")));
        assert_eq!(b[2][0].text, "• ");
        let _ = layout_html("<<<a href=");
        let _ = layout_html("<");
    }
}
