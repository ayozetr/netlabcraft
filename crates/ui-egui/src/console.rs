//! Device windows. Routers and switches: Physical, Config, CLI, Attributes. Hosts: Physical,
//! Config, Desktop (IP Configuration, Command Prompt), Attributes. Every change goes through
//! engine commands (`device.configure`, `cli.exec`, `device.rename`), so it can be undone.

use egui::{Key, Modifiers, RichText, Sense, TextStyle, vec2};
use packetcraft_core::DeviceId;
use packetcraft_model::{Device, DeviceKind, End, SwitchportMode, ifname};
use packetcraft_sim::port_up;
use serde_json::{Value, json};

use crate::i18n::tl;
use crate::{PacketApp, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    Physical,
    Config,
    #[default]
    Cli,
    Services,
    Desktop,
    Attributes,
}

/// What the Config tab's list has selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfigPage {
    #[default]
    Settings,
    /// A home router's LAN, DHCP, Internet and Wi-Fi.
    Home,
    Vlans,
    Routes,
    /// A wireless LAN controller's WLANs and RADIUS server, or a lightweight AP's controller.
    Wireless,
    Port(usize),
}

/// The Desktop application open on a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopApp {
    IpConfig,
    Prompt,
    Browser,
    Email,
    MibBrowser,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceWindow {
    pub tab: Tab,
    pub page: ConfigPage,
    pub desktop: Option<DesktopApp>,
    pub input: String,
    /// Position while browsing history with ↑/↓ (index into the history).
    history_pos: Option<usize>,
    focus: bool,
    // Form fields.
    name: String,
    label: String,
    ip: String,
    mask: String,
    gateway: String,
    dns: String,
    /// IPv6 address with its length (`2001:db8::10/64`) and gateway.
    ip6: String,
    gw6: String,
    /// "Static" picked for IPv6, before an address is applied.
    v6_static: bool,
    /// Wi-Fi form: SSID, WPA2 passphrase, channel.
    wifi_ssid: String,
    wifi_key: String,
    wifi_channel: String,
    /// WPA2-Enterprise login (hosts).
    wifi_user: String,
    wifi_password: String,
    vlan_id: String,
    vlan_name: String,
    route_net: String,
    route_mask: String,
    route_hop: String,
    port_vlan: String,
    /// The console lines the last form change ran (shown under the form).
    echo: String,
    /// Services tab and browser state.
    pub(crate) svc: crate::services::ServiceForms,
}

impl DeviceWindow {
    /// Shows a page of the Services tab.
    pub fn set_service_page(&mut self, p: crate::services::Page) {
        self.svc.page = p;
    }

    /// Types a URL into the browser and presses Go (on the next frame).
    pub fn set_url(&mut self, url: &str) {
        self.svc.url = url.to_string();
        self.svc.go_pending = true;
    }
}

/// Opens (or raises) a device's window, filling the forms from the device.
pub fn open(app: &mut PacketApp, d: DeviceId) {
    app.session.console(d);
    let Some(dev) = app.session.topology.device(d).cloned() else { return };
    let new = !app.ui.windows.contains_key(&d);
    let w = app.ui.windows.entry(d).or_default();
    // A home router opens on its settings page.
    if new && dev.catalogue_model().home_router() {
        w.tab = Tab::Config;
        w.page = ConfigPage::Home;
    }
    // A controller's (or lightweight AP's) Config tab opens on its wireless page.
    if new && (packetcraft_model::wlan::is_wlc(&dev) || packetcraft_model::wlan::is_lap(&dev)) {
        w.page = ConfigPage::Wireless;
    }
    w.focus = true;
    w.name = dev.name.clone();
    w.label = dev.display_name().to_string();
    if dev.kind.is_host() && w.tab == Tab::Cli {
        w.tab = Tab::Desktop;
    }
    if dev.kind == DeviceKind::Hub {
        w.tab = Tab::Physical;
    }
    fill_host(w, &dev);
}

fn fill_host(w: &mut DeviceWindow, dev: &Device) {
    let p = dev.ports.first();
    w.ip = p.and_then(|p| p.ip).map(|ip| ip.addr.to_string()).unwrap_or_default();
    w.mask = p.and_then(|p| p.ip).map(|ip| ip.mask().to_string()).unwrap_or_else(|| "255.255.255.0".into());
    w.gateway = dev.gateway.map(|g| g.to_string()).unwrap_or_default();
    w.dns = dev.dns.map(|g| g.to_string()).unwrap_or_default();
    w.ip6 = p.and_then(|p| p.ipv6.addresses.first()).map(|a| a.to_string()).unwrap_or_default();
    w.gw6 = dev.gateway6.map(|g| g.to_string()).unwrap_or_default();
}

fn fill_port(w: &mut DeviceWindow, dev: &Device, port: usize) {
    if let Some(p) = dev.ports.get(port) {
        w.ip = p.ip.map(|ip| ip.addr.to_string()).unwrap_or_default();
        w.mask = p.ip.map(|ip| ip.mask().to_string()).unwrap_or_default();
        w.port_vlan = p.switchport.access_vlan.to_string();
        w.ip6 = p.ipv6.addresses.first().map(|a| a.to_string()).unwrap_or_default();
    }
}

pub fn windows(app: &mut PacketApp, ctx: &egui::Context) {
    let ids: Vec<DeviceId> = app.ui.windows.keys().copied().collect();
    for d in ids {
        let Some(dev) = app.session.topology.device(d).cloned() else {
            app.ui.windows.remove(&d);
            continue;
        };
        let mut open = true;
        let pos = [180.0 + 26.0 * (d.0 % 8) as f32, 110.0 + 26.0 * (d.0 % 8) as f32];
        egui::Window::new(dev.display_name())
            .id(egui::Id::unique(("device", d.0)))
            .open(&mut open)
            .default_size([680.0, 470.0])
            .default_pos(pos)
            .show(ctx, |ui| {
                let Some(w) = app.ui.windows.get_mut(&d) else { return };
                ui.horizontal(|ui| {
                    let tabs: &[(Tab, &str)] = if dev.kind == DeviceKind::Hub {
                        &[(Tab::Physical, "Physical"), (Tab::Attributes, "Attributes")]
                    } else if dev.kind == DeviceKind::Server {
                        &[
                            (Tab::Physical, "Physical"),
                            (Tab::Config, "Config"),
                            (Tab::Services, "Services"),
                            (Tab::Desktop, "Desktop"),
                            (Tab::Attributes, "Attributes"),
                        ]
                    } else if dev.kind.is_host() {
                        &[(Tab::Physical, "Physical"), (Tab::Config, "Config"), (Tab::Desktop, "Desktop"), (Tab::Attributes, "Attributes")]
                    } else {
                        &[(Tab::Physical, "Physical"), (Tab::Config, "Config"), (Tab::Cli, "CLI"), (Tab::Attributes, "Attributes")]
                    };
                    for (t, label) in tabs {
                        if ui.selectable_label(w.tab == *t, RichText::new(crate::i18n::tr(label)).size(15.0)).clicked() {
                            w.tab = *t;
                            w.focus = true;
                        }
                    }
                });
                ui.separator();
                let tab = w.tab;
                match tab {
                    Tab::Physical => physical(app, ui, &dev),
                    Tab::Config => config(app, ui, &dev),
                    Tab::Cli => {
                        ui.vertical_centered(|ui| ui.strong("Command Line Interface"));
                        console(app, ui, d, false);
                    }
                    Tab::Desktop => desktop(app, ui, &dev),
                    Tab::Services => {
                        let mut forms = app.ui.windows.get_mut(&d).map(|w| std::mem::take(&mut w.svc)).unwrap_or_default();
                        crate::services::tab(app, ui, &dev, &mut forms);
                        if let Some(w) = app.ui.windows.get_mut(&d) {
                            w.svc = forms;
                        }
                    }
                    Tab::Attributes => attributes(app, ui, &dev),
                }
            });
        if !open {
            app.ui.windows.remove(&d);
        }
    }
}

/// The console: scrollback, prompt and input line. `dark`: the black command-prompt look of a
/// host; otherwise the white device CLI.
fn console(app: &mut PacketApp, ui: &mut egui::Ui, d: DeviceId, dark: bool) {
    let tk = theme::tokens(app.ui.dark);
    let (bg, fg) =
        if dark { (egui::Color32::from_rgb(0x0c, 0x0c, 0x0c), egui::Color32::from_rgb(0xe8, 0xe8, 0xe8)) } else { (tk.console_bg, tk.console_text) };
    let input_id = egui::Id::unique(("console_input", d.0));
    let prompt = {
        let cli = app.session.console(d).cli.clone();
        packetcraft_ios::prompt(&app.session.topology, d, &cli)
    };
    // Keys the console handles before the text field sees them.
    let focused = ui.memory(|m| m.has_focus(input_id));
    let (enter, up, down, tab) = if focused {
        ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Tab),
            )
        })
    } else {
        (false, false, false, false)
    };
    let history = app.session.console(d).cli.history.clone();
    let Some(w) = app.ui.windows.get_mut(&d) else { return };
    let mut move_cursor = false;
    if up && !history.is_empty() {
        let pos = w.history_pos.map_or(history.len().saturating_sub(1), |p| p.saturating_sub(1));
        w.history_pos = Some(pos);
        w.input = history.get(pos).cloned().unwrap_or_default();
        move_cursor = true;
    }
    if down && let Some(p) = w.history_pos {
        if p + 1 < history.len() {
            w.history_pos = Some(p + 1);
            w.input = history.get(p + 1).cloned().unwrap_or_default();
        } else {
            w.history_pos = None;
            w.input.clear();
        }
        move_cursor = true;
    }
    let mut run_line: Option<String> = None;
    let mut help_for: Option<String> = None;
    if tab {
        let line = w.input.clone();
        if let Ok(v) = app.session.execute("cli.complete", json!({"device": d.0, "line": line}))
            && let Some(done) = v.get("line").and_then(Value::as_str)
            && let Some(w) = app.ui.windows.get_mut(&d)
        {
            w.input = done.to_string();
            move_cursor = true;
        }
    }
    let Some(w) = app.ui.windows.get_mut(&d) else { return };
    if enter {
        run_line = Some(std::mem::take(&mut w.input));
        w.history_pos = None;
    }

    let log = app.session.console(d).log.clone();
    let shown = log.strip_suffix(prompt.as_str()).unwrap_or(&log).to_string();
    let input_h = ui.text_style_height(&TextStyle::Monospace) + 12.0;
    let buttons_h = if dark { 0.0 } else { 30.0 };
    egui::Frame::new().fill(bg).stroke(egui::Stroke::new(1.0, tk.chrome_line)).inner_margin(6.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .max_height((ui.available_height() - input_h - buttons_h).max(80.0))
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&shown).monospace().color(fg)).wrap_mode(egui::TextWrapMode::Extend).selectable(true));
            });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let label = ui.label(RichText::new(&prompt).monospace().color(fg));
            let Some(w) = app.ui.windows.get_mut(&d) else { return };
            let resp = ui
                .add(
                    egui::TextEdit::singleline(&mut w.input)
                        .id(input_id)
                        .font(TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .text_color(fg)
                        .password(prompt == packetcraft_ios::PASSWORD_PROMPT),
                )
                .labelled_by(label.id);
            if w.focus || enter || tab {
                resp.request_focus();
                w.focus = false;
            }
            // `?` answers at once, as on real devices, and leaves the line as it was.
            if let Some(stripped) = w.input.strip_suffix('?') {
                help_for = Some(stripped.to_string());
                w.input = stripped.to_string();
                move_cursor = true;
            }
            if move_cursor && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), input_id) {
                let n = w.input.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
                state.store(ui.ctx(), input_id);
            }
        });
    });
    if !dark {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tl("Paste")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    if let Some(w) = app.ui.windows.get_mut(&d) {
                        w.focus = true;
                    }
                }
                if ui.button(tl("Copy")).clicked() {
                    ui.ctx().copy_text(shown.clone());
                }
            });
        });
    }

    if let Some(line) = help_for {
        let help = app
            .session
            .execute("cli.help", json!({"device": d.0, "line": line}))
            .ok()
            .and_then(|v| v.get("help").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        app.session.console(d).append(&format!("{line}?\n{help}\n{prompt}"));
    }
    if let Some(line) = run_line {
        app.run("cli.exec", json!({"device": d.0, "line": line}));
        if let Some(dev) = app.session.topology.device(d).cloned()
            && let Some(w) = app.ui.windows.get_mut(&d)
            && dev.kind.is_host()
        {
            fill_host(w, &dev);
        }
    }
}

/// The front panel: one socket per port, its link light, and what is plugged in.
fn physical(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let tk = theme::tokens(app.ui.dark);
    ui.label(RichText::new(crate::i18n::trf("{}: {} ports", &[&dev.catalogue_model().label, &dev.ports.len()])).strong());
    let per_row = 13usize;
    let rows = dev.ports.len().div_ceil(per_row).max(1);
    let size = vec2(ui.available_width().min(640.0), 40.0 + rows as f32 * 56.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, egui::Color32::from_rgb(0x3a, 0x40, 0x48));
    p.rect_filled(rect.shrink(4.0), 4.0, egui::Color32::from_rgb(0x55, 0x5d, 0x68));
    let led_power = rect.left_top() + vec2(18.0, 18.0);
    p.circle_filled(led_power, 4.0, egui::Color32::from_rgb(0x7c, 0xf0, 0x7c));
    p.text(led_power + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, "PWR", egui::FontId::proportional(10.0), egui::Color32::WHITE);
    let mut hovered = None;
    for (i, port) in dev.ports.iter().enumerate() {
        let (row, col) = (i / per_row, i % per_row);
        let cell = egui::Rect::from_min_size(rect.left_top() + vec2(20.0 + col as f32 * 46.0, 36.0 + row as f32 * 56.0), vec2(38.0, 46.0));
        let socket = egui::Rect::from_min_size(cell.min + vec2(5.0, 12.0), vec2(28.0, 20.0));
        let end = End { device: dev.id, port: i };
        let (cabled, up) = (app.session.topology.peer(end).is_some(), port_up(&app.session.topology, end));
        p.rect_filled(socket, 2.0, egui::Color32::from_rgb(0x1a, 0x1c, 0x20));
        if cabled {
            p.rect_filled(socket.shrink(4.0), 1.0, egui::Color32::from_rgb(0x4a, 0x90, 0xd0));
        }
        let led = if up {
            tk.link_up
        } else if cabled {
            egui::Color32::from_rgb(0xe8, 0xa3, 0x1c)
        } else {
            egui::Color32::from_rgb(0x2a, 0x2a, 0x2a)
        };
        p.circle_filled(socket.center_top() - vec2(0.0, 6.0), 3.5, led);
        p.text(
            cell.center_bottom() - vec2(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            ifname::short(&port.name),
            egui::FontId::proportional(9.5),
            egui::Color32::WHITE,
        );
        if ui.rect_contains_pointer(cell) {
            hovered = Some(i);
        }
    }
    if let Some(i) = hovered
        && let Some(port) = dev.ports.get(i)
    {
        let end = End { device: dev.id, port: i };
        let peer = app.session.topology.peer(end).map_or("nothing".to_string(), |e| app.session.topology.end_label(e));
        ui.label(crate::i18n::trf(
            "{}: {} · cabled to {}",
            &[&port.name, &crate::i18n::tl(if port_up(&app.session.topology, end) { "link up" } else { "link down" }), &peer],
        ));
    } else {
        ui.label(RichText::new(tl("Point at a port to see what is plugged in. Cables are made with the Connections tool.")).color(tk.muted));
    }
}

fn attributes(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    egui::Grid::new(("attributes", dev.id.0)).striped(true).num_columns(2).show(ui, |ui| {
        let cabled = (0..dev.ports.len()).filter(|p| app.session.topology.peer(End { device: dev.id, port: *p }).is_some()).count();
        let rows = [
            ("Name", dev.name.clone()),
            ("Model", dev.catalogue_model().label.to_string()),
            ("Ports", dev.ports.len().to_string()),
            ("Cabled ports", cabled.to_string()),
            ("Base MAC address", dev.ports.first().map_or("-".into(), |p| p.mac.to_string())),
            ("Saved configuration", if dev.startup.is_some() { "yes" } else { "no" }.into()),
            ("Position", format!("{:.0}, {:.0}", dev.x, dev.y)),
        ];
        for (k, v) in rows {
            ui.strong(k);
            ui.label(v);
            ui.end_row();
        }
    });
}

/// Runs configuration lines through the engine and echoes them under the form.
pub(crate) fn configure(app: &mut PacketApp, d: DeviceId, lines: Vec<String>) {
    let echo = lines.join("\n");
    if let Some(v) = app.run("device.configure", json!({"device": d.0, "lines": lines})) {
        let errors: Vec<String> =
            v.get("errors").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        if !errors.is_empty() {
            app.ui.status = Some((errors.join("; "), true));
        }
    }
    if let Some(w) = app.ui.windows.get_mut(&d) {
        w.echo = echo;
    }
}

/// A full-width button of the Config tab's list; headers are bold and inert.
pub(crate) fn list_item(ui: &mut egui::Ui, text: &str, selected: bool, header: bool) -> egui::Response {
    let text = crate::i18n::tr(text);
    let tk = theme::tokens(ui.visuals().dark_mode);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), if header { Sense::hover() } else { Sense::click() });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::Button, !header, selected, text));
    let p = ui.painter_at(rect);
    let fill = if selected {
        tk.pill
    } else if resp.hovered() && !header {
        tk.chrome_line.gamma_multiply(0.6)
    } else {
        tk.chrome
    };
    p.rect_filled(rect.shrink(1.0), 2.0, fill);
    p.rect_stroke(rect.shrink(1.0), 2.0, egui::Stroke::new(1.0, tk.chrome_line), egui::StrokeKind::Inside);
    let font = if header { egui::FontId::proportional(16.0) } else { egui::FontId::proportional(15.0) };
    let color = if selected { tk.pill_text } else { tk.text };
    let text_rt = if header { RichText::new(text).strong() } else { RichText::new(text) };
    let galley = egui::WidgetText::from(text_rt.font(font).color(color)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        rect.width() - 8.0,
        egui::TextStyle::Body,
    );
    p.galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

fn config(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    let tk = theme::tokens(app.ui.dark);
    let Some(w) = app.ui.windows.get_mut(&d) else { return };
    let mut page = w.page;
    let body_h = 380.0;
    ui.horizontal_top(|ui| {
        egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.text)).inner_margin(2.0).show(ui, |ui| {
            ui.set_width(190.0);
            ui.set_height(body_h);
            egui::ScrollArea::vertical().id_salt(("config_list", d.0)).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    list_item(ui, tl("GLOBAL"), false, true);
                    if list_item(ui, tl("Settings"), page == ConfigPage::Settings, false).clicked() {
                        page = ConfigPage::Settings;
                    }
                    if dev.catalogue_model().home_router() && list_item(ui, tl("Home Router"), page == ConfigPage::Home, false).clicked() {
                        page = ConfigPage::Home;
                    }
                    let wlc = packetcraft_model::wlan::is_wlc(dev);
                    if (wlc || packetcraft_model::wlan::is_lap(dev))
                        && list_item(ui, tl(if wlc { "Wireless LANs" } else { "Controller" }), page == ConfigPage::Wireless, false).clicked()
                    {
                        page = ConfigPage::Wireless;
                    }
                    match dev.kind {
                        DeviceKind::Switch => {
                            list_item(ui, tl("SWITCHING"), false, true);
                            if list_item(ui, tl("VLAN Database"), page == ConfigPage::Vlans, false).clicked() {
                                page = ConfigPage::Vlans;
                            }
                        }
                        DeviceKind::Router => {
                            list_item(ui, tl("ROUTING"), false, true);
                            if list_item(ui, tl("Static"), page == ConfigPage::Routes, false).clicked() {
                                page = ConfigPage::Routes;
                            }
                        }
                        _ => {}
                    }
                    list_item(ui, tl("INTERFACE"), false, true);
                    for (i, p) in dev.ports.iter().enumerate() {
                        if list_item(ui, &p.name, page == ConfigPage::Port(i), false).clicked() {
                            page = ConfigPage::Port(i);
                        }
                    }
                })
            });
        });
        egui::Frame::new().stroke(egui::Stroke::new(1.0, tk.chrome_line)).fill(tk.chrome).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(460.0);
            ui.set_min_height(body_h - 20.0);
            ui.vertical(|ui| {
                if let Some(w) = app.ui.windows.get_mut(&d)
                    && w.page != page
                {
                    w.page = page;
                    if let ConfigPage::Port(i) = page {
                        fill_port(w, dev, i);
                    }
                    if dev.kind.is_host() {
                        fill_host(w, dev);
                    }
                }
                match page {
                    ConfigPage::Settings => settings_page(app, ui, dev),
                    ConfigPage::Home => home_page(app, ui, dev),
                    ConfigPage::Vlans => vlan_page(app, ui, dev),
                    ConfigPage::Routes => routes_page(app, ui, dev),
                    ConfigPage::Wireless => crate::wlan_page::show(app, ui, dev),
                    ConfigPage::Port(i) => port_page(app, ui, dev, i),
                }
            });
        });
    });
    if !dev.kind.is_host() {
        // The device's console output, as it scrolls by in the CLI tab.
        ui.add_space(6.0);
        ui.label(tl("Equivalent console commands"));
        let log = app.session.console(d).log.clone();
        egui::Frame::new().fill(tk.console_bg).stroke(egui::Stroke::new(1.5, tk.text)).inner_margin(4.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_width(680.0);
            egui::ScrollArea::vertical()
                .id_salt(("equivalent", d.0))
                .stick_to_bottom(true)
                .max_height(110.0)
                .min_scrolled_height(110.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(RichText::new(log).monospace().color(tk.console_text));
                });
        });
    }
}

pub(crate) fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        let l = ui.add_sized([130.0, 22.0], egui::Label::new(label));
        ui.add(egui::TextEdit::singleline(value).desired_width(ui.available_width().min(320.0))).labelled_by(l.id);
    });
}

/// A centred title with a rule under it, as the Config pages have.
pub(crate) fn page_title(ui: &mut egui::Ui, title: &str) {
    ui.vertical_centered(|ui| ui.label(RichText::new(crate::i18n::tr(title)).size(16.0)));
    ui.separator();
}

/// The form of a home router's settings page, kept between frames.
#[derive(Debug, Clone, Default)]
struct HomeForm {
    lan: String,
    lan_mask: String,
    dhcp: bool,
    dhcp_start: String,
    internet: String,
    internet_mask: String,
    gateway: String,
    internet_dhcp: bool,
    ssid: String,
    key: String,
}

impl HomeForm {
    fn from_device(dev: &Device) -> HomeForm {
        let lan = dev.svis.iter().find(|s| s.vlan.get() == 1).and_then(|s| s.ip);
        let inet = dev.ports.iter().find(|p| p.name.starts_with("Internet")).and_then(|p| p.ip);
        let gw = dev.routes.iter().find(|r| r.net.prefix == 0).and_then(|r| match r.via {
            packetcraft_model::NextHop::Address(a) => Some(a),
            _ => None,
        });
        let start = dev
            .dhcp_excluded
            .iter()
            .map(|(_, y)| std::net::Ipv4Addr::from(u32::from(*y).saturating_add(1)))
            .max()
            .or_else(|| lan.map(|l| std::net::Ipv4Addr::from(u32::from(l.network().addr) + 100)));
        let wifi = dev.ports.iter().find_map(|p| p.wifi.clone());
        HomeForm {
            lan: lan.map(|l| l.addr.to_string()).unwrap_or_default(),
            lan_mask: lan.map(|l| l.mask().to_string()).unwrap_or_default(),
            dhcp: !dev.dhcp_pools.is_empty(),
            dhcp_start: start.map(|a| a.to_string()).unwrap_or_default(),
            internet_dhcp: dev.ports.iter().any(|p| p.name.starts_with("Internet") && p.dhcp),
            internet: inet.map(|n| n.addr.to_string()).unwrap_or_default(),
            internet_mask: inet.map(|n| n.mask().to_string()).unwrap_or_default(),
            gateway: gw.map(|g| g.to_string()).unwrap_or_default(),
            ssid: wifi.as_ref().map(|w| w.ssid.clone()).unwrap_or_default(),
            key: wifi.and_then(|w| w.key).unwrap_or_default(),
        }
    }
}

fn home_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    page_title(ui, "Home Router");
    let id = egui::Id::unique(("home_form", d.0));
    let mut f: HomeForm = ui.data_mut(|m| m.get_temp(id)).unwrap_or_else(|| HomeForm::from_device(dev));
    let mut actions: Vec<(&str, serde_json::Value)> = Vec::new();
    ui.label(RichText::new(tl("LAN")).strong());
    field(ui, tl("Router address"), &mut f.lan);
    field(ui, tl("Subnet Mask"), &mut f.lan_mask);
    ui.checkbox(&mut f.dhcp, tl("Hand out addresses (DHCP)"));
    field(ui, tl("First address"), &mut f.dhcp_start);
    if ui.button(tl("Save LAN")).clicked() {
        let mut p = json!({"device": d.0, "lan": f.lan.trim(), "lan_mask": f.lan_mask.trim(), "dhcp": f.dhcp});
        if f.dhcp && !f.dhcp_start.trim().is_empty() {
            p["dhcp_start"] = json!(f.dhcp_start.trim());
        }
        actions.push(("home.settings", p));
    }
    ui.separator();
    ui.label(RichText::new(tl("Internet")).strong());
    ui.checkbox(&mut f.internet_dhcp, tl("Obtain the address from the provider (DHCP)"));
    if !f.internet_dhcp {
        field(ui, tl("IPv4 Address"), &mut f.internet);
        field(ui, tl("Subnet Mask"), &mut f.internet_mask);
        field(ui, tl("Default Gateway"), &mut f.gateway);
    } else if !f.internet.is_empty() {
        ui.label(crate::i18n::trf("Leased: {}", &[&f.internet]));
    }
    if ui.button(tl("Save Internet")).clicked() {
        let p = if f.internet_dhcp {
            json!({"device": d.0, "internet_dhcp": true})
        } else {
            json!({"device": d.0, "internet": f.internet.trim(), "internet_mask": f.internet_mask.trim(), "gateway": f.gateway.trim()})
        };
        actions.push(("home.settings", p));
    }
    ui.separator();
    ui.label(RichText::new(tl("Wireless")).strong());
    field(ui, tl("SSID"), &mut f.ssid);
    field(ui, tl("WPA2 passphrase (empty: open)"), &mut f.key);
    if ui.button(tl("Save Wireless")).clicked() {
        actions.push(("device.wireless", json!({"device": d.0, "ssid": f.ssid.trim(), "key": f.key})));
    }
    let mut saved = false;
    for (cmd, p) in actions {
        saved |= app.run(cmd, p).is_some();
    }
    if saved {
        // Read back what the router now has.
        ui.data_mut(|m| m.remove::<HomeForm>(id));
    } else {
        ui.data_mut(|m| m.insert_temp(id, f));
    }
}

fn settings_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    page_title(ui, "Global Settings");
    let Some(w) = app.ui.windows.get_mut(&d) else { return };
    let mut label = w.label.clone();
    let mut host = w.name.clone();
    let mut actions: Vec<(&str, serde_json::Value)> = Vec::new();
    let mut requests = Vec::new();
    ui.horizontal(|ui| {
        let l = ui.add_sized([130.0, 22.0], egui::Label::new(tl("Display Name")));
        let r = ui.add(egui::TextEdit::singleline(&mut label).desired_width(ui.available_width().min(320.0))).labelled_by(l.id);
        if r.lost_focus() && label.trim() != dev.display_name() {
            actions.push(("device.label", json!({"device": d.0, "label": label.trim()})));
        }
    });
    ui.horizontal(|ui| {
        let l = ui.add_sized([130.0, 22.0], egui::Label::new(tl("Hostname")));
        let r = ui.add(egui::TextEdit::singleline(&mut host).desired_width(ui.available_width().min(320.0))).labelled_by(l.id);
        if r.lost_focus() && host.trim() != dev.name && !host.trim().is_empty() {
            if dev.kind.is_host() {
                actions.push(("device.rename", json!({"device": d.0, "name": host.trim()})));
            } else {
                actions.push(("device.configure", json!({"device": d.0, "lines": [format!("hostname {}", host.trim())]})));
            }
        }
    });
    w.label = label;
    w.name = host;
    if dev.kind.is_host() {
        ui.add_space(8.0);
        field(ui, tl("Default Gateway"), &mut w.gateway);
        if ui.button(tl("Set Gateway")).clicked() {
            let line = format!("ipconfig {} {} {}", w.ip.trim(), w.mask.trim(), w.gateway.trim());
            actions.push(("cli.exec", json!({"device": d.0, "line": line})));
        }
    } else {
        let row = |ui: &mut egui::Ui, label: &str, a: &str, b: &str| -> (bool, bool) {
            ui.horizontal(|ui| {
                ui.add_sized([130.0, 26.0], egui::Label::new(label));
                let x = ui.add_sized([150.0, 26.0], egui::Button::new(a)).clicked();
                let y = ui.add_sized([150.0, 26.0], egui::Button::new(b)).clicked();
                (x, y)
            })
            .inner
        };
        let (erase, save) = row(ui, "NVRAM", "Erase", "Save");
        if erase {
            actions.push(("device.startup", json!({"device": d.0, "action": "erase"})));
        }
        if save {
            actions.push(("device.startup", json!({"device": d.0, "action": "save"})));
        }
        let (load, export_start) = row(ui, "Startup Config", "Load...", "Export...");
        if load {
            requests.push(crate::UiRequest::OpenText { device: d, merge: false });
        }
        if export_start {
            match &dev.startup {
                Some(cfg) => requests.push(crate::UiRequest::SaveText { name: format!("{}_startup-config.txt", dev.name), text: cfg.clone() }),
                None => app.ui.status = Some((crate::i18n::tl("There is no startup configuration to export yet (Save first).").into(), true)),
            }
        }
        let (export_run, merge) = row(ui, "Running Config", "Export...", "Merge...");
        if export_run {
            requests
                .push(crate::UiRequest::SaveText { name: format!("{}_running-config.txt", dev.name), text: packetcraft_ios::running_config(dev) });
        }
        if merge {
            requests.push(crate::UiRequest::OpenText { device: d, merge: true });
        }
    }
    for (id, params) in actions {
        app.run(id, params);
    }
    app.requests.extend(requests);
}

fn vlan_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    ui.heading(tl("VLAN Database"));
    let mut lines = None;
    if let Some(w) = app.ui.windows.get_mut(&d) {
        field(ui, tl("VLAN Number"), &mut w.vlan_id);
        field(ui, tl("VLAN Name"), &mut w.vlan_name);
        if ui.button(tl("Add")).clicked() {
            let mut l = vec![format!("vlan {}", w.vlan_id.trim())];
            if !w.vlan_name.trim().is_empty() {
                l.push(format!("name {}", w.vlan_name.trim()));
            }
            lines = Some(l);
        }
    }
    egui::Grid::new(("vlans", d.0)).striped(true).num_columns(3).show(ui, |ui| {
        for (v, name) in &dev.vlans {
            ui.label(v.to_string());
            ui.label(name);
            if v.get() != 1 && ui.small_button(tl("Remove")).clicked() {
                lines = Some(vec![format!("no vlan {v}")]);
            }
            ui.end_row();
        }
    });
    if let Some(l) = lines {
        configure(app, d, l);
    }
}

fn routes_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    ui.heading(tl("Static Routes"));
    let mut lines = None;
    if let Some(w) = app.ui.windows.get_mut(&d) {
        field(ui, tl("Network"), &mut w.route_net);
        field(ui, tl("Mask"), &mut w.route_mask);
        field(ui, tl("Next Hop"), &mut w.route_hop);
        if ui.button(tl("Add")).clicked() {
            lines = Some(vec![format!("ip route {} {} {}", w.route_net.trim(), w.route_mask.trim(), w.route_hop.trim())]);
        }
    }
    ui.label(RichText::new(tl("Network Address")).strong());
    for r in &dev.routes {
        ui.horizontal(|ui| {
            let via = match &r.via {
                packetcraft_model::NextHop::Address(a) => a.to_string(),
                packetcraft_model::NextHop::Interface(i) => i.clone(),
            };
            ui.monospace(format!("{} via {via}", r.net));
            if ui.small_button(tl("Remove")).clicked() {
                lines = Some(vec![format!("no ip route {} {} {via}", r.net.addr, r.net.mask())]);
            }
        });
    }
    if let Some(l) = lines {
        configure(app, d, l);
    }
}

fn port_page(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device, i: usize) {
    let d = dev.id;
    let Some(port) = dev.ports.get(i).cloned() else { return };
    ui.heading(&port.name);
    let mut on = !port.shutdown;
    let mut lines: Option<Vec<String>> = None;
    let iface = format!("interface {}", port.name);
    ui.horizontal(|ui| {
        ui.label(tl("Port Status"));
        if ui.checkbox(&mut on, tl("On")).changed() {
            lines = Some(vec![iface.clone(), if on { "no shutdown".into() } else { "shutdown".into() }]);
        }
    });
    ui.label(format!("MAC Address: {}", port.mac));
    let Some(w) = app.ui.windows.get_mut(&d) else { return };
    match dev.kind {
        DeviceKind::Router => {
            field(ui, tl("IPv4 Address"), &mut w.ip);
            field(ui, tl("Subnet Mask"), &mut w.mask);
            if ui.button(tl("Apply")).clicked() {
                lines = Some(if w.ip.trim().is_empty() {
                    vec![iface.clone(), "no ip address".into()]
                } else {
                    vec![iface.clone(), format!("ip address {} {}", w.ip.trim(), w.mask.trim())]
                });
            }
            ui.add_space(6.0);
            field(ui, tl("IPv6 Address"), &mut w.ip6);
            ui.label(format!(
                "Link Local Address: {}",
                packetcraft_sim::v6::l3_ifaces6(dev).iter().find(|x| x.port == i).map_or("(IPv6 off)".to_string(), |x| x.link_local.to_string())
            ));
            if ui.button(tl("Apply IPv6")).clicked() {
                let mut l = vec![iface.clone(), "no ipv6 address".into()];
                if !w.ip6.trim().is_empty() {
                    l.push(format!("ipv6 address {}", w.ip6.trim()));
                }
                lines = Some(l);
            }
        }
        DeviceKind::Switch => {
            let mut trunk = port.switchport.mode != SwitchportMode::Access;
            ui.horizontal(|ui| {
                ui.label(tl("Mode"));
                if ui.radio_value(&mut trunk, false, tl("Access")).changed() || ui.radio_value(&mut trunk, true, tl("Trunk")).changed() {
                    lines = Some(vec![iface.clone(), format!("switchport mode {}", if trunk { "trunk" } else { "access" })]);
                }
            });
            if !trunk {
                field(ui, tl("Access VLAN"), &mut w.port_vlan);
                if ui.button(tl("Apply")).clicked() {
                    lines = Some(vec![iface.clone(), format!("switchport access vlan {}", w.port_vlan.trim())]);
                }
            } else {
                ui.label(format!("Native VLAN {} · allowed {}", port.switchport.native_vlan, port.switchport.allowed));
            }
        }
        DeviceKind::Hub => {
            ui.label(tl("Hub ports have nothing to configure: every frame is repeated out of every other port."));
        }
        DeviceKind::Pc | DeviceKind::Laptop | DeviceKind::Server | DeviceKind::Printer => {
            field(ui, tl("IPv4 Address"), &mut w.ip);
            field(ui, tl("Subnet Mask"), &mut w.mask);
            if ui.button(tl("Apply")).clicked() {
                let line = format!("ipconfig {} {} {}", w.ip.trim(), w.mask.trim(), w.gateway.trim());
                app.run("cli.exec", json!({"device": d.0, "line": line.trim()}));
            }
            if let Some(l) = lines.take() {
                let off = l.last().is_some_and(|x| x == "shutdown");
                app.info(if off { "Host ports are always on." } else { "" });
            }
        }
    }
    if ifname::is_wireless(&port.name)
        && let Some(w) = app.ui.windows.get_mut(&d)
    {
        ui.add_space(8.0);
        ui.label(RichText::new(tl("Wi-Fi")).strong());
        if w.wifi_ssid.is_empty()
            && let Some(wf) = &port.wifi
        {
            w.wifi_ssid = wf.ssid.clone();
            w.wifi_key = wf.key.clone().unwrap_or_default();
            w.wifi_channel = wf.channel.to_string();
            w.wifi_user = wf.login.as_ref().map(|l| l.user.clone()).unwrap_or_default();
            w.wifi_password = wf.login.as_ref().map(|l| l.password.clone()).unwrap_or_default();
        }
        let ap = dev.kind == DeviceKind::Switch;
        if packetcraft_model::wlan::is_lap(dev) {
            ui.label(tl("A lightweight AP offers the WLANs of its controller: see its Controller page."));
            let n = app.session.topology.wifi_clients(packetcraft_model::End { device: d, port: i }).len();
            ui.label(crate::i18n::trf("{} host(s) associated", &[&n]));
            if let Some(l) = lines {
                configure(app, d, l);
            }
            return;
        }
        field(ui, tl("SSID"), &mut w.wifi_ssid);
        field(ui, tl("WPA2 passphrase"), &mut w.wifi_key);
        if ap {
            field(ui, tl("Channel"), &mut w.wifi_channel);
        } else {
            field(ui, tl("Username (WPA2-Enterprise)"), &mut w.wifi_user);
            field(ui, tl("Password (WPA2-Enterprise)"), &mut w.wifi_password);
        }
        let (ssid, key, chan) = (w.wifi_ssid.trim().to_string(), w.wifi_key.trim().to_string(), w.wifi_channel.trim().parse::<u64>().unwrap_or(6));
        let (user, password) = (w.wifi_user.trim().to_string(), w.wifi_password.clone());
        let assoc = if ap {
            crate::i18n::trf("{} host(s) associated", &[&app.session.topology.wifi_clients(packetcraft_model::End { device: d, port: i }).len()])
        } else {
            app.session
                .topology
                .wifi_ap_of(packetcraft_model::End { device: d, port: i })
                .map_or(crate::i18n::tl("Not associated").to_string(), |e| {
                    crate::i18n::trf("Associated with {}", &[&app.session.topology.end_label(e)])
                })
        };
        ui.label(assoc);
        if ui.button(tl("Apply Wi-Fi")).clicked()
            && let Some(v) = app.run(
                "device.wireless",
                json!({"device": d.0, "port": port.name, "ssid": ssid, "key": key, "channel": chan, "user": user, "password": password}),
            )
        {
            let msg = match (v["associated_with"].as_str(), v["problem"].as_str()) {
                (Some(a), _) => crate::i18n::trf("Associated with {}", &[&a]),
                (None, _) if ap => "Wi-Fi settings applied".to_string(),
                (None, Some(why)) => crate::i18n::trf("Not associated: {}", &[&crate::i18n::tr_any(why)]),
                (None, None) => "Not associated: check the SSID, the passphrase and the distance".to_string(),
            };
            app.info(msg);
        }
    }
    if let Some(l) = lines {
        configure(app, d, l);
    }
}

fn desktop(app: &mut PacketApp, ui: &mut egui::Ui, dev: &Device) {
    let d = dev.id;
    let tk = theme::tokens(app.ui.dark);
    let Some(w) = app.ui.windows.get_mut(&d) else { return };
    match w.desktop {
        None => {
            // A navy desktop with large tiles, as learners know it. Only working apps are shown.
            let mut open = None;
            egui::Frame::new().fill(tk.bar).inner_margin(16.0).show(ui, |ui| {
                ui.set_min_size(vec2(640.0, 380.0));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(28.0, 20.0);
                    for (app_id, label) in [
                        (DesktopApp::IpConfig, "IP Configuration"),
                        (DesktopApp::Prompt, "Command Prompt"),
                        (DesktopApp::Browser, "Web Browser"),
                        (DesktopApp::Email, "Email"),
                        (DesktopApp::MibBrowser, "MIB Browser"),
                    ] {
                        let (rect, resp) = ui.allocate_exact_size(vec2(130.0, 150.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, label));
                        let p = ui.painter_at(rect);
                        let tile = egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 52.0), vec2(100.0, 100.0));
                        let teal = egui::Color32::from_rgb(0x58, 0xbf, 0xd2);
                        p.rect_filled(tile, 0.0, if resp.hovered() { theme::shade(teal, 1.15) } else { teal });
                        draw_app_icon(&p, tile.shrink(14.0), app_id);
                        p.text(
                            rect.center_bottom() - vec2(0.0, 22.0),
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(16.0),
                            egui::Color32::WHITE,
                        );
                        if resp.clicked() {
                            open = Some(app_id);
                        }
                    }
                });
            });
            if let Some(a) = open {
                w.desktop = Some(a);
                w.focus = true;
            }
        }
        Some(open) => {
            let title = match open {
                DesktopApp::IpConfig => "IP Configuration",
                DesktopApp::Prompt => "Command Prompt",
                DesktopApp::Browser => "Web Browser",
                DesktopApp::Email => "Email",
                DesktopApp::MibBrowser => "MIB Browser",
            };
            let mut close = false;
            egui::Frame::new().fill(tk.bar).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).color(tk.bar_text).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = ui.button("X").on_hover_text(tl("Close")).clicked();
                    });
                });
            });
            if close {
                w.desktop = None;
                return;
            }
            match open {
                DesktopApp::IpConfig => {
                    ui.add_space(6.0);
                    let port = dev.ports.first().map_or("FastEthernet0".to_string(), |p| p.name.clone());
                    ui.label(RichText::new(crate::i18n::trf("Interface {}", &[&port])).strong());
                    let mut dhcp = dev.dhcp_client;
                    ui.horizontal(|ui| {
                        ui.label(tl("IP Configuration"));
                        if ui.radio_value(&mut dhcp, true, tl("DHCP")).clicked()
                            && let Some(v) = app.run("cli.exec", json!({"device": d.0, "line": "ipconfig /renew"}))
                        {
                            let out = v.get("output").and_then(Value::as_str).unwrap_or("").to_string();
                            let failed = out.starts_with("DHCP failed");
                            app.ui.status = Some((out, failed));
                        }
                        if ui.radio_value(&mut dhcp, false, tl("Static")).clicked() && dev.dhcp_client {
                            app.run("cli.exec", json!({"device": d.0, "line": "ipconfig /release"}));
                        }
                    });
                    if let Some(now) = app.session.topology.device(d).cloned()
                        && let Some(w) = app.ui.windows.get_mut(&d)
                        && now.dhcp_client
                    {
                        fill_host(w, &now);
                    }
                    let Some(w) = app.ui.windows.get_mut(&d) else { return };
                    field(ui, tl("IPv4 Address"), &mut w.ip);
                    field(ui, tl("Subnet Mask"), &mut w.mask);
                    field(ui, tl("Default Gateway"), &mut w.gateway);
                    field(ui, tl("DNS Server"), &mut w.dns);
                    if ui.button(tl("Apply")).clicked() {
                        let gw = if w.gateway.trim().is_empty() && !w.dns.trim().is_empty() { "" } else { w.gateway.trim() };
                        let line = format!("ipconfig {} {} {} {}", w.ip.trim(), w.mask.trim(), gw, w.dns.trim());
                        if let Some(v) = app.run("cli.exec", json!({"device": d.0, "line": line.trim()})) {
                            let out = v.get("output").and_then(Value::as_str).unwrap_or("").to_string();
                            app.info(out);
                        }
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new(tl("IPv6 Configuration")).strong());
                    let conf = dev.ports.first().map(|p| p.ipv6.clone()).unwrap_or_default();
                    #[derive(PartialEq, Clone, Copy)]
                    enum V6 {
                        Off,
                        Auto,
                        Static,
                    }
                    let chosen = app.ui.windows.get(&d).is_some_and(|w| w.v6_static);
                    let now = if conf.autoconfig {
                        V6::Auto
                    } else if conf.active() || chosen {
                        V6::Static
                    } else {
                        V6::Off
                    };
                    let mut pick = now;
                    let mut run: Option<String> = None;
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut pick, V6::Off, tl("Off"));
                        ui.radio_value(&mut pick, V6::Auto, tl("Automatic"));
                        ui.radio_value(&mut pick, V6::Static, tl("Static"));
                    });
                    let Some(w) = app.ui.windows.get_mut(&d) else { return };
                    if pick != now {
                        w.v6_static = pick == V6::Static;
                        match pick {
                            V6::Off => run = Some("ipv6config off".into()),
                            V6::Auto => run = Some("ipv6config auto".into()),
                            V6::Static => {}
                        }
                    }
                    if conf.autoconfig {
                        fill_host(w, dev);
                    }
                    field(ui, tl("IPv6 Address"), &mut w.ip6);
                    let ll = packetcraft_sim::v6::l3_ifaces6(dev).first().map_or("(IPv6 off)".to_string(), |x| x.link_local.to_string());
                    ui.horizontal(|ui| {
                        ui.add_sized([130.0, 22.0], egui::Label::new(tl("Link Local Address")));
                        ui.label(ll);
                    });
                    field(ui, tl("IPv6 Gateway"), &mut w.gw6);
                    if pick == V6::Static && ui.button(tl("Apply IPv6")).clicked() {
                        run = Some(format!("ipv6config {} {}", w.ip6.trim(), w.gw6.trim()));
                    }
                    if let Some(line) = run
                        && let Some(v) = app.run("cli.exec", json!({"device": d.0, "line": line.trim()}))
                    {
                        let out = v.get("output").and_then(Value::as_str).unwrap_or("").to_string();
                        let failed = out.contains("failed") || out.starts_with("Usage") || out.contains("can't") || out.contains("not");
                        app.ui.status = Some((out, failed));
                        if let (Some(now), Some(w)) = (app.session.topology.device(d).cloned(), app.ui.windows.get_mut(&d)) {
                            fill_host(w, &now);
                        }
                    }
                }
                DesktopApp::Prompt => console(app, ui, d, true),
                DesktopApp::Email => crate::mail::app(app, ui, dev),
                DesktopApp::MibBrowser => crate::mib::app(app, ui, dev),
                DesktopApp::Browser => {
                    let mut forms = app.ui.windows.get_mut(&d).map(|w| std::mem::take(&mut w.svc)).unwrap_or_default();
                    crate::services::browser(app, ui, d, &mut forms);
                    if let Some(w) = app.ui.windows.get_mut(&d) {
                        w.svc = forms;
                    }
                }
            }
        }
    }
}

/// The pictures on the Desktop tiles (our own drawings).
fn draw_app_icon(p: &egui::Painter, r: egui::Rect, app: DesktopApp) {
    let dark = egui::Color32::from_rgb(0x2a, 0x33, 0x40);
    let light = egui::Color32::from_rgb(0xf4, 0xf1, 0xe6);
    match app {
        DesktopApp::IpConfig => {
            // A network card: a board with a port and the letters IP.
            let board = egui::Rect::from_min_size(r.min + vec2(0.0, r.height() * 0.2), vec2(r.width(), r.height() * 0.6));
            p.rect_filled(board, 4.0, light);
            p.rect_stroke(board, 4.0, egui::Stroke::new(2.0, dark), egui::StrokeKind::Inside);
            let port =
                egui::Rect::from_min_size(board.right_top() + vec2(-r.width() * 0.32, r.height() * 0.12), vec2(r.width() * 0.22, r.height() * 0.3));
            p.rect_filled(port, 2.0, dark);
            p.text(
                board.left_center() + vec2(r.width() * 0.08, 0.0),
                egui::Align2::LEFT_CENTER,
                "IP",
                egui::FontId::proportional(r.height() * 0.32),
                dark,
            );
            for i in 0..5 {
                let x = board.left() + r.width() * (0.1 + 0.1 * i as f32);
                p.line_segment(
                    [egui::pos2(x, board.bottom()), egui::pos2(x, board.bottom() + r.height() * 0.1)],
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(0xc8, 0xa0, 0x3c)),
                );
            }
        }
        DesktopApp::Browser => {
            // A globe with an address label across it.
            let c = r.center();
            p.circle_filled(c, r.width() * 0.42, egui::Color32::from_rgb(0x2f, 0x7d, 0xc8));
            p.circle_stroke(c, r.width() * 0.42, egui::Stroke::new(2.0, dark));
            for k in [-0.2_f32, 0.0, 0.2] {
                p.line_segment(
                    [egui::pos2(r.left() + r.width() * 0.1, c.y + r.height() * k), egui::pos2(r.right() - r.width() * 0.1, c.y + r.height() * k)],
                    egui::Stroke::new(1.5, light),
                );
            }
            p.line_segment(
                [egui::pos2(c.x, r.top() + r.height() * 0.1), egui::pos2(c.x, r.bottom() - r.height() * 0.1)],
                egui::Stroke::new(1.5, light),
            );
            let tag = egui::Rect::from_center_size(c + vec2(0.0, r.height() * 0.02), vec2(r.width() * 0.8, r.height() * 0.28));
            p.rect_filled(tag, 3.0, light);
            p.text(tag.center(), egui::Align2::CENTER_CENTER, "http:", egui::FontId::proportional(r.height() * 0.2), dark);
        }
        DesktopApp::MibBrowser => {
            // A tree of objects: a root and three branches.
            let s = egui::Stroke::new(2.0, dark);
            let root = r.center_top() + vec2(0.0, r.height() * 0.15);
            p.circle_filled(root, r.width() * 0.1, light);
            p.circle_stroke(root, r.width() * 0.1, s);
            for k in [-0.32_f32, 0.0, 0.32] {
                let leaf = egui::pos2(r.center().x + r.width() * k, r.bottom() - r.height() * 0.2);
                p.line_segment([root, leaf], s);
                let tile = egui::Rect::from_center_size(leaf, vec2(r.width() * 0.24, r.height() * 0.2));
                p.rect_filled(tile, 2.0, light);
                p.rect_stroke(tile, 2.0, s, egui::StrokeKind::Inside);
            }
        }
        DesktopApp::Email => {
            // An envelope with its flap.
            let env = egui::Rect::from_center_size(r.center(), vec2(r.width() * 0.9, r.height() * 0.62));
            p.rect_filled(env, 3.0, light);
            p.rect_stroke(env, 3.0, egui::Stroke::new(2.0, dark), egui::StrokeKind::Inside);
            let stroke = egui::Stroke::new(2.0, dark);
            p.line_segment([env.left_top(), env.center() + vec2(0.0, env.height() * 0.08)], stroke);
            p.line_segment([env.right_top(), env.center() + vec2(0.0, env.height() * 0.08)], stroke);
        }
        DesktopApp::Prompt => {
            // A monitor with a prompt on a black screen.
            let screen = egui::Rect::from_min_size(r.min + vec2(0.0, r.height() * 0.08), vec2(r.width(), r.height() * 0.72));
            p.rect_filled(screen, 4.0, light);
            p.rect_filled(screen.shrink(r.width() * 0.08), 2.0, egui::Color32::BLACK);
            p.text(
                screen.shrink(r.width() * 0.14).left_top(),
                egui::Align2::LEFT_TOP,
                ">_",
                egui::FontId::monospace(r.height() * 0.3),
                egui::Color32::WHITE,
            );
            let base = egui::Rect::from_min_size(
                egui::pos2(r.center().x - r.width() * 0.25, screen.bottom() + 2.0),
                vec2(r.width() * 0.5, r.height() * 0.12),
            );
            p.rect_filled(base, 2.0, light);
        }
    }
}
