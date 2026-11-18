//! A cable as an object: the window that shows one, and the menu entries every workspace shares.
//!
//! The logical workspace, the physical workspace and the wiring closet each hit-test their own
//! drawing, then agree on what a cable can do: select it, open its window (Properties), open the
//! device at either end, set its length, show its information and delete it. Everything acts
//! through engine commands (`link.remove`, `link.length`, `port.transceiver`).

use egui::Ui;
use netlabcraft_model::End;
use netlabcraft_model::cable_ref::CableRef;
use serde_json::json;

use crate::i18n::{tl, tr, tr_any};
use crate::theme;
use crate::{PacketApp, physical};

/// Selects a cable of any kind and opens its window.
pub(crate) fn open_ref(app: &mut PacketApp, r: CableRef) {
    app.ui.selected_cable = Some(r);
    app.ui.cable_window = Some(r);
}

/// Selects the cable plugged into `end` (the window stays as it is).
pub(crate) fn select(app: &mut PacketApp, end: End) {
    app.ui.selected_cable = Some(CableRef::Link(app.session.topology.link_at(end).map_or(end, |l| l.a)));
}

/// The name of the port at `end`.
fn port_name(app: &PacketApp, end: End) -> String {
    app.session.topology.device(end.device).and_then(|d| d.ports.get(end.port)).map(|p| p.name.clone()).unwrap_or_default()
}

/// Removes a cable of any kind and forgets it was selected.
pub(crate) fn delete_ref(app: &mut PacketApp, r: CableRef) {
    let Some((device, port)) = app.session.topology.cable_removal(r) else { return };
    if app.run("link.remove", json!({"device": device.0, "port": port})).is_some() {
        forget_ref(app, r);
    }
}

/// Drops the selection and the window of a cable that is gone.
pub(crate) fn forget_ref(app: &mut PacketApp, r: CableRef) {
    if app.ui.selected_cable == Some(r) {
        app.ui.selected_cable = None;
    }
    if app.ui.cable_window == Some(r) {
        app.ui.cable_window = None;
    }
}

/// Whether a cable carries a signal, and why not. A console, USB or octal cable is up when
/// both devices are on and the run is sound; an IoT custom cable when both are on.
fn state(app: &PacketApp, r: CableRef) -> Option<(bool, Option<String>)> {
    let t = &app.session.topology;
    if let CableRef::Link(end) = r {
        let l = t.link_at(end)?;
        let up = netlabcraft_sim::port_up(t, l.a) && netlabcraft_sim::port_up(t, l.b);
        return Some((up, if up { None } else { netlabcraft_sim::link_why_down(t, l) }));
    }
    let info = t.cable_info(r)?;
    let off = [info.a, info.b].into_iter().any(|d| t.device(d).is_some_and(|d| d.powered_off));
    let problem = if info.kind == netlabcraft_model::cable::Cable::IotCustom { None } else { t.run_problem(info.kind, info.a, info.b) };
    let why = if off { Some("a device at one end is powered off".to_string()) } else { problem };
    Some((why.is_none(), why))
}

/// `Kind: A port - B port, length, up or down`, for the status line (Show Info).
pub(crate) fn summary(app: &PacketApp, r: CableRef) -> Option<String> {
    let t = &app.session.topology;
    let info = t.cable_info(r)?;
    let (up, _) = state(app, r)?;
    let state_text = if up { tl("Up") } else { tl("Down") };
    Some(match r {
        CableRef::Link(end) => {
            let l = t.link_at(end)?;
            format!(
                "{}: {} - {}, {}, {}",
                tr(info.kind.label()),
                info.a_label,
                info.b_label,
                physical::length_text(t.cable_length(l).meters, app.ui.prefs.metric),
                state_text
            )
        }
        _ => format!("{}: {} - {}, {}", tr(info.kind.label()), info.a_label, info.b_label, state_text),
    })
}

/// The entries of a cable's right-click menu, after its title: Properties, Open First Device and
/// Open Second Device, Cable Length (network cables), Show Info (the physical workspace only) and
/// Delete.
pub(crate) fn menu_items(app: &mut PacketApp, ui: &mut Ui, r: CableRef, physical: bool) {
    let Some(info) = app.session.topology.cable_info(r) else {
        ui.close();
        return;
    };
    if ui.button(tl("Properties…")).clicked() {
        open_ref(app, r);
        ui.close();
    }
    if ui.button(tl("Open First Device")).clicked() {
        crate::open_console(app, info.a);
        ui.close();
    }
    if ui.button(tl("Open Second Device")).clicked() {
        crate::open_console(app, info.b);
        ui.close();
    }
    if let CableRef::Link(end) = r
        && let Some(l) = app.session.topology.link_at(end).copied()
        && ui.button(tl("Cable Length…")).clicked()
    {
        let m = app.session.topology.cable_length(&l).meters;
        app.ui.physical.length = Some((l.a, m.to_string()));
        ui.close();
    }
    if physical && ui.button(tl("Show Info")).clicked() {
        if let Some(text) = summary(app, r) {
            app.info(text);
        }
        ui.close();
    }
    if ui.button(tl("Delete")).clicked() {
        delete_ref(app, r);
        ui.close();
    }
}

/// What the window shows, read once a frame.
struct Facts {
    kind: String,
    a: String,
    b: String,
    /// A network cable's length in metres, and whether it was set by hand.
    length: Option<(u32, bool)>,
    up: bool,
    why: Option<String>,
    /// For a fiber cable: whether each end's transceiver is single-mode.
    fiber: Option<(bool, bool)>,
    /// For a fiber cable: its loss budget.
    budget: Option<netlabcraft_model::physical::FiberBudget>,
    /// How far this kind of cable carries a signal, and the standard behind it.
    reach: Option<netlabcraft_model::cable::Reach>,
    /// A cable that runs through a backbone: where it leaves and where it arrives.
    backbone: Option<String>,
}

fn facts(app: &PacketApp, r: CableRef) -> Option<Facts> {
    let t = &app.session.topology;
    let info = t.cable_info(r)?;
    let (up, why) = state(app, r)?;
    let link = r.link().and_then(|end| t.link_at(end));
    Some(Facts {
        kind: tr(info.kind.label()).to_string(),
        a: info.a_label,
        b: info.b_label,
        length: link.map(|l| {
            let len = t.cable_length(l);
            (len.meters, len.manual)
        }),
        up,
        why,
        fiber: link.filter(|_| info.kind.is_fiber()).map(|l| (t.single_mode_at(l.a), t.single_mode_at(l.b))),
        budget: link.and_then(|l| t.fiber_budget(l)),
        reach: info.kind.reach(),
        backbone: link.and_then(|l| t.backbone_of_link(l)).map(|(a, b)| {
            let place = |p| t.pole_place(p).and_then(|q| t.place(q)).map(|q| tr(&q.name).to_string()).unwrap_or_default();
            crate::i18n::trf("{} in {} to {} in {}", &[&t.pole_label(a), &place(a), &t.pole_label(b), &place(b)])
        }),
    })
}

/// The wording of a transceiver, by wavelength.
fn mode_text(single: bool) -> &'static str {
    if single { tl("single-mode (1310 nm)") } else { tl("multi-mode (850 nm)") }
}

/// The Cable Properties window: kind, both ends, length, status and why it is down, the
/// transceivers of a fiber cable, and Delete.
pub fn window(app: &mut PacketApp, ctx: &egui::Context) {
    let Some(r) = app.ui.cable_window else { return };
    let Some(f) = facts(app, r) else {
        app.ui.cable_window = None;
        return;
    };
    let Some(info) = app.session.topology.cable_info(r) else { return };
    let link = r.link().and_then(|end| app.session.topology.link_at(end).copied());
    let tk = theme::tokens(app.ui.dark);
    let metric = app.ui.prefs.metric;
    let (mut open, mut delete_it, mut set_length) = (true, false, false);
    let mut open_device = None;
    let mut transceivers = None;
    let mut splices = None;
    egui::Window::new(tl("Cable Properties")).id(egui::Id::unique("cable_window")).open(&mut open).collapsible(false).resizable(false).show(
        ctx,
        |ui| {
            egui::Grid::new("cable_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(tl("Kind"));
                ui.label(&f.kind);
                ui.end_row();
                ui.label(tl("First end"));
                ui.label(&f.a);
                ui.end_row();
                ui.label(tl("Second end"));
                ui.label(&f.b);
                ui.end_row();
                if let Some((meters, manual)) = f.length {
                    ui.label(tl("Length"));
                    let how = if manual { tl("set by hand") } else { tl("from where the devices stand") };
                    ui.label(format!("{} ({how})", physical::length_text(meters, metric)));
                    ui.end_row();
                }
                if let Some(r) = &f.reach {
                    ui.label(tl("Reach"));
                    ui.label(format!("{} ({})", physical::length_text(r.meters, metric), r.standard));
                    ui.end_row();
                }
                if let Some(text) = &f.backbone {
                    ui.label(tl("Backbone"));
                    ui.label(text);
                    ui.end_row();
                }
                ui.label(tl("Status"));
                if f.up {
                    ui.colored_label(tk.link_up, tl("Up"));
                } else {
                    ui.colored_label(tk.link_down, tl("Down"));
                }
                ui.end_row();
                if let Some(why) = &f.why {
                    ui.label(tl("Why it is down"));
                    ui.colored_label(tk.error, tr_any(why));
                    ui.end_row();
                }
                if let Some((sa, sb)) = f.fiber {
                    ui.label(tl("Transceivers"));
                    ui.vertical(|ui| {
                        ui.label(format!("{}: {}", f.a, mode_text(sa)));
                        ui.label(format!("{}: {}", f.b, mode_text(sb)));
                    });
                    ui.end_row();
                }
                if let Some(b) = &f.budget {
                    use netlabcraft_model::physical::BudgetState;
                    ui.label(tl("Loss budget"));
                    let (word, color) = match b.state() {
                        BudgetState::Ok => (tl("within the budget"), tk.link_up),
                        BudgetState::Degraded => (tl("degraded: little margin left"), tk.link_blocked),
                        BudgetState::Over => (tl("over the budget"), tk.link_down),
                    };
                    ui.vertical(|ui| {
                        ui.label(crate::i18n::trf(
                            "{} dB lost of {} dB ({} connectors, {} splices)",
                            &[&format!("{:.1}", b.loss_db), &format!("{:.1}", b.budget_db), &b.connectors, &b.splices],
                        ));
                        ui.colored_label(color, word);
                    });
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button(tl("Open First Device")).clicked() {
                    open_device = Some(info.a);
                }
                if ui.button(tl("Open Second Device")).clicked() {
                    open_device = Some(info.b);
                }
                if f.length.is_some() && ui.button(tl("Cable Length…")).clicked() {
                    set_length = true;
                }
                if let Some(b) = &f.budget {
                    if ui.button(tl("More splices")).clicked() {
                        splices = Some(b.splices.saturating_add(1));
                    }
                    if ui.add_enabled(b.splices > 0, egui::Button::new(tl("Fewer splices"))).clicked() {
                        splices = Some(b.splices.saturating_sub(1));
                    }
                }
                if f.fiber.is_some() {
                    if ui.button(tl("Both ends single-mode")).clicked() {
                        transceivers = Some("single-mode");
                    }
                    if ui.button(tl("Both ends multi-mode")).clicked() {
                        transceivers = Some("multi-mode");
                    }
                }
                if ui.button(tl("Delete Cable")).clicked() {
                    delete_it = true;
                }
            });
        },
    );
    if let Some(d) = open_device {
        crate::open_console(app, d);
    }
    if set_length && let (Some(l), Some((meters, _))) = (link, f.length) {
        app.ui.physical.length = Some((l.a, meters.to_string()));
    }
    if let (Some(mode), Some(l)) = (transceivers, link) {
        for e in [l.a, l.b] {
            let port = port_name(app, e);
            app.run("port.transceiver", json!({"device": e.device.0, "port": port, "mode": mode}));
        }
    }
    if let (Some(count), Some(l)) = (splices, link) {
        let port = port_name(app, l.a);
        app.run("link.splices", json!({"device": l.a.device.0, "port": port, "count": count}));
    }
    if delete_it {
        delete_ref(app, r);
    } else if !open {
        app.ui.cable_window = None;
    }
}
