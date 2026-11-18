//! Contact sheets for looking at the code-drawn art without a window (no GPU needed beyond a software
//! Vulkan driver). Writes PNGs into a directory:
//!
//! `cargo run -p netlabcraft-ui-egui --example sheets -- OUT_DIR`
//!
//! * `icons-light.png`, `icons-dark.png`: every device model, labelled with its id, large and small;
//! * `cables-light.png`, `cables-dark.png`: every cable tile;
//! * `closet-light.png`, `closet-dark.png`: a wiring closet with a rack, a shelf, a table, patch panels, a
//!   power strip and cables, built through engine commands;
//! * `closet-zoom-light.png`, `closet-zoom-dark.png`: the same closet zoomed on the rack, for the connectors;
//! * `closet-z050-*.png`, `closet-z100-*.png`, `closet-z150-*.png`: the closet at zoom 0.5, 1.0 and 1.5;
//! * `device-physical-<NAME>-light.png`, `device-physical-<NAME>-dark.png`: the Physical tab of one device of every
//!   family (hub, PoE and SFP switches, routers with serial ports and a module card, firewall, access point, home
//!   router, server, PC, laptop, printer, IP phone, modems, clouds, cell tower, central office, an IoT thing and the
//!   boards), built through engine commands, with the real connector drawn for every port;
//! * `grid-window-*.png`: the Intercity map with the Grid Preferences window open (the four-squares button on the
//!   bar), every level's grid ticked; `grid-intercity-*.png`, `grid-city-*.png`, `grid-building-*.png`: the grid of each
//!   level with its spacing at the corner; `grid-colour-*.png`: the window with the colour palette open;
//! * `create-menu-<LEVEL>-light.png`, `create-menu-<LEVEL>-dark.png`: the physical bar with the `+` menu open at the
//!   Intercity map, a city, a building, a wiring closet and a container, the Home button at the right end;
//! * `create-kinds-light.png`, `create-kinds-dark.png`: a closet with every kind of furniture (a rack, a shelf, a
//!   table, an inventory shelf with devices in storage, a cable pegboard with hung cables and a control panel);
//! * `logical-light.png`, `logical-dark.png`: the logical workspace with a few devices.
//!
//! The `snapshots` workflow runs it and uploads the PNGs; nothing here is committed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use netlabcraft_engine::Session;
use netlabcraft_model::cable::Cable;
use netlabcraft_model::models::MODELS;
use netlabcraft_ui_egui::{GridLevel, PacketApp, theme};
use serde_json::{Value, json};

/// Runs a command; a refusal is printed (the sheet is still rendered) and gives `Null`.
fn cmd(s: &mut Session, id: &str, params: Value) -> Value {
    match s.execute(id, params) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{id}: {e}");
            Value::Null
        }
    }
}

/// The name of port number `i` of a device.
fn port(s: &Session, dev: &str, i: usize) -> String {
    s.topology.find(dev).and_then(|d| d.ports.get(i)).map(|p| p.name.clone()).unwrap_or_default()
}

/// Cables port `ai` of `a` to port `bi` of `b`.
fn link(s: &mut Session, a: &str, ai: usize, b: &str, bi: usize, cable: &str) {
    let (pa, pb) = (port(s, a, ai), port(s, b, bi));
    cmd(s, "link.add", json!({"a": a, "a_port": pa, "b": b, "b_port": pb, "cable": cable}));
}

fn add(s: &mut Session, model: &str, name: &str, x: f32, y: f32) {
    cmd(s, "device.add", json!({"model": model, "name": name, "x": x, "y": y}));
}

/// Draws `draw` on a blank canvas of `size` and saves the render as `dir/name.png`.
fn painted(dir: &str, name: &str, size: (f32, f32), dark: bool, draw: impl Fn(&egui::Painter, Pos2, &theme::Tokens) + 'static) {
    let tk = theme::tokens(dark);
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(size.0 + 40.0, size.1 + 40.0)).wgpu().build_ui(move |ui| {
        netlabcraft_ui_egui::fonts::install(ui.ctx());
        ui.style_mut().visuals.panel_fill = tk.canvas;
        let (resp, painter) = ui.allocate_painter(vec2(size.0, size.1), Sense::hover());
        painter.rect_filled(resp.rect.expand(40.0), 0.0, tk.canvas);
        draw(&painter, resp.rect.min, &tk);
    });
    harness.run_steps(3);
    let img = harness.render().expect("render");
    let path = format!("{dir}/{name}.png");
    img.save(&path).expect("save png");
    eprintln!("wrote {path} ({}x{})", img.width(), img.height());
}

/// A cell frame with a caption under it.
fn cell(p: &egui::Painter, r: Rect, caption: &str, tk: &theme::Tokens) {
    p.rect_stroke(r, 4.0, Stroke::new(1.0, tk.grid), StrokeKind::Inside);
    p.text(pos2(r.center().x, r.bottom() - 7.0), Align2::CENTER_CENTER, caption, FontId::proportional(10.5), tk.muted);
}

fn icons_sheet(dir: &str, dark: bool) {
    const COLS: usize = 8;
    let (cw, ch) = (150.0, 118.0);
    let rows = MODELS.len().div_ceil(COLS);
    let size = (cw * COLS as f32, ch * rows as f32);
    let name = if dark { "icons-dark" } else { "icons-light" };
    painted(dir, name, size, dark, move |p, o, tk| {
        for (i, m) in MODELS.iter().enumerate() {
            let (c, r) = (i % COLS, i / COLS);
            let cellr = Rect::from_min_size(o + vec2(c as f32 * cw, r as f32 * ch), vec2(cw, ch)).shrink(2.0);
            cell(p, cellr, m.id, tk);
            let big = Rect::from_center_size(pos2(cellr.left() + 50.0, cellr.top() + 48.0), vec2(64.0, 64.0));
            netlabcraft_ui_egui::icons_families::model_icon(p, big, m.kind, m.id, m.multilayer(), tk);
            let small = Rect::from_center_size(pos2(cellr.right() - 30.0, cellr.top() + 36.0), vec2(30.0, 30.0));
            netlabcraft_ui_egui::icons_families::model_icon(p, small, m.kind, m.id, m.multilayer(), tk);
        }
    });
}

fn cables_sheet(dir: &str, dark: bool) {
    let (cw, ch) = (130.0, 140.0);
    let size = (cw * 7.0, ch * 2.0);
    let name = if dark { "cables-dark" } else { "cables-light" };
    painted(dir, name, size, dark, move |p, o, tk| {
        for (i, kind) in Cable::ALL.iter().enumerate() {
            let (c, r) = (i % 7, i / 7);
            let cellr = Rect::from_min_size(o + vec2(c as f32 * cw, r as f32 * ch), vec2(cw, ch)).shrink(2.0);
            cell(p, cellr, kind.id(), tk);
            let big = Rect::from_center_size(pos2(cellr.center().x - 20.0, cellr.top() + 52.0), vec2(80.0, 80.0));
            netlabcraft_ui_egui::icons::cable(p, big, *kind, tk);
            let small = Rect::from_center_size(pos2(cellr.right() - 22.0, cellr.top() + 30.0), vec2(30.0, 30.0));
            netlabcraft_ui_egui::icons::cable(p, small, *kind, tk);
        }
    });
}

/// A rendered app (a harness over `PacketApp`) saved as `dir/name.png`; `setup` runs after the first frames.
fn app_sheet(dir: &str, name: &str, session: Session, size: (f32, f32), dark: bool, setup: impl FnOnce(&mut PacketApp)) {
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(size.0, size.1)).with_max_steps(12).wgpu().build_ui_state(
        |ui, app: &mut PacketApp| {
            netlabcraft_ui_egui::fonts::install(ui.ctx());
            app.apply_theme(ui.ctx());
            app.ui(ui);
        },
        {
            let mut app = PacketApp::new(session);
            app.ui.dark = dark;
            app
        },
    );
    harness.run_steps(2);
    setup(harness.state_mut());
    harness.run_steps(6);
    let img = harness.render().expect("render");
    let path = format!("{dir}/{name}.png");
    img.save(&path).expect("save png");
    eprintln!("wrote {path} ({}x{})", img.width(), img.height());
}

/// A closet with a rack (a patch panel, routers, switches, a firewall, a fiber panel and a strip), a
/// shelf, a table and a loose phone, cabled, patched and plugged in.
fn closet_lab() -> (Session, u32) {
    let mut s = Session::new();
    add(&mut s, "Router-WAN", "R1", 0.0, 0.0);
    add(&mut s, "Router-4", "R2", 0.0, 0.0);
    add(&mut s, "Switch-24", "SW1", 0.0, 0.0);
    add(&mut s, "Switch-24-SFP", "SW2", 0.0, 0.0);
    add(&mut s, "Switch-48", "SW3", 0.0, 0.0);
    add(&mut s, "Firewall-5", "FW1", 0.0, 0.0);
    add(&mut s, "Hub-8", "H1", 0.0, 0.0);
    add(&mut s, "Server", "SRV1", 0.0, 0.0);
    add(&mut s, "AccessPoint", "AP1", 0.0, 0.0);
    add(&mut s, "Cable-Modem", "CM1", 0.0, 0.0);
    add(&mut s, "DSL-Modem", "DSL1", 0.0, 0.0);
    add(&mut s, "Home-Router", "HR1", 0.0, 0.0);
    add(&mut s, "PC-Modem", "PC1", 0.0, 0.0);
    add(&mut s, "Laptop", "LT1", 0.0, 0.0);
    add(&mut s, "Printer", "PR1", 0.0, 0.0);
    add(&mut s, "IP-Phone", "PH1", 0.0, 0.0);
    let places = cmd(&mut s, "place.list", json!({}));
    let closet = places["places"]
        .as_array()
        .and_then(|v| v.iter().find(|p| p["kind"] == "closet"))
        .and_then(|p| p["id"].as_u64())
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(3);
    cmd(&mut s, "furniture.add", json!({"kind": "rack", "name": "Rack0", "size": 14, "place": closet, "x": 60.0, "y": 40.0}));
    cmd(&mut s, "furniture.add", json!({"kind": "shelf", "name": "Shelf0", "size": 2, "place": closet, "x": 460.0, "y": 40.0}));
    cmd(&mut s, "furniture.add", json!({"kind": "table", "name": "Table0", "place": closet, "x": 460.0, "y": 360.0}));
    let rack = ["R1", "R2", "SW1", "SW2", "SW3", "FW1", "H1"];
    for (i, d) in rack.iter().enumerate() {
        cmd(&mut s, "furniture.mount", json!({"device": d, "furniture": "Rack0", "slot": i + 1}));
    }
    cmd(&mut s, "passive.add", json!({"kind": "copper-patch-panel", "ports": 24, "furniture": "Rack0", "slot": 8}));
    cmd(&mut s, "passive.add", json!({"kind": "fiber-patch-panel", "ports": 12, "furniture": "Rack0", "slot": 9}));
    cmd(&mut s, "passive.add", json!({"kind": "copper-wall-mount", "ports": 2, "furniture": "Rack0", "slot": 10, "units": 2}));
    cmd(&mut s, "passive.add", json!({"kind": "power-strip", "ports": 8, "furniture": "Rack0", "slot": 12}));
    for (i, d) in ["SRV1", "AP1", "CM1", "DSL1", "HR1"].iter().enumerate() {
        cmd(&mut s, "furniture.mount", json!({"device": d, "furniture": "Shelf0", "slot": i / 3 + 1}));
    }
    for d in ["PC1", "LT1", "PR1"] {
        cmd(&mut s, "furniture.mount", json!({"device": d, "furniture": "Table0"}));
    }
    link(&mut s, "R1", 0, "SW1", 0, "straight-through");
    link(&mut s, "R2", 0, "SW1", 1, "straight-through");
    link(&mut s, "SW1", 2, "SW2", 2, "cross-over");
    link(&mut s, "SW2", 24, "SW3", 0, "fiber-multi-mode");
    link(&mut s, "FW1", 0, "R2", 1, "straight-through");
    link(&mut s, "SRV1", 0, "SW3", 1, "straight-through");
    link(&mut s, "PC1", 0, "SW3", 2, "straight-through");
    link(&mut s, "R1", 2, "R2", 2, "serial-dce");
    cmd(&mut s, "link.add", json!({"a": "PC1", "b": "R1", "cable": "console"}));
    let (p1, p2) = (port(&s, "SW1", 5), port(&s, "SW3", 5));
    cmd(&mut s, "patch.wire", json!({"a": {"panel": "Copper Patch Panel0", "port": 1}, "b": {"device": "SW1", "port": p1}}));
    cmd(&mut s, "patch.wire", json!({"a": {"panel": "Copper Patch Panel0", "port": 2}, "b": {"device": "SW3", "port": p2}}));
    for d in ["R1", "SW1", "SW2", "FW1"] {
        cmd(&mut s, "power.plug", json!({"device": d, "strip": "Power Distribution Device0"}));
    }
    (s, closet)
}

fn closet_sheet(dir: &str, dark: bool) {
    let suffix = if dark { "dark" } else { "light" };
    let (s, closet) = closet_lab();
    app_sheet(dir, &format!("closet-{suffix}"), s, (1500.0, 900.0), dark, move |app| {
        app.set_physical(true);
        app.enter_place(closet);
    });
    // Zoomed on the rack (its first frame): the top units (devices and their jacks), then the lower ones
    // (patch panels, wall mount, power strip).
    for (name, skip_units) in [("closet-zoom-top", 0.0f32), ("closet-zoom-bottom", 8.0)] {
        let (s, closet) = closet_lab();
        app_sheet(dir, &format!("{name}-{suffix}"), s, (1500.0, 900.0), dark, move |app| {
            app.set_physical(true);
            app.enter_place(closet);
            let frame = app.session.topology.closet_layout(closet).frames.first().map(|(_, c)| (c.x, c.y));
            let (x, y) = frame.unwrap_or((0.0, 0.0));
            app.ui.zoom = 2.2;
            app.ui.pan = vec2(30.0, 10.0) - vec2(x, y + 36.0 + skip_units * 44.0) * 2.2;
        });
    }
}

/// The same closet at 0.5, 1.0 and 1.5 (the sizes between the fit and the connector close-ups), the furniture's top
/// left a little inside the view: what a person sees when they zoom the closet with the buttons.
fn closet_zoom_steps(dir: &str, dark: bool) {
    let suffix = if dark { "dark" } else { "light" };
    for (name, zoom) in [("closet-z050", 0.5f32), ("closet-z100", 1.0), ("closet-z150", 1.5)] {
        let (s, closet) = closet_lab();
        app_sheet(dir, &format!("{name}-{suffix}"), s, (1500.0, 900.0), dark, move |app| {
            app.set_physical(true);
            app.enter_place(closet);
            let frame = app.session.topology.closet_layout(closet).frames.first().map(|(_, c)| (c.x, c.y));
            let (x, y) = frame.unwrap_or((0.0, 0.0));
            app.ui.zoom = zoom;
            app.ui.pan = vec2(30.0, 60.0) - vec2(x, y) * zoom;
        });
    }
}

/// The index of the first port of `dev` whose name starts with `prefix`.
fn first_port(s: &Session, dev: &str, prefix: &str) -> usize {
    s.topology.find(dev).and_then(|d| d.ports.iter().position(|p| p.name.starts_with(prefix))).unwrap_or(0)
}

/// One device of every family, cabled so the lights and cable marks show: what the device windows are rendered from.
fn physical_lab() -> Session {
    let mut s = Session::new();
    for (model, name) in [
        ("Hub-8", "HUB1"),
        ("PC", "PC1"),
        ("Laptop", "LT1"),
        ("Switch-24-SFP", "SW1"),
        ("Switch-24-SFP", "SW1B"),
        ("Switch-8-PoE", "SWP1"),
        ("Router-WAN", "R1"),
        ("Router-M2", "RM1"),
        ("Firewall-5", "FW1"),
        ("AccessPoint", "AP1"),
        ("Home-Router", "HR1"),
        ("Server", "SRV1"),
        ("Printer", "PR1"),
        ("IP-Phone", "PH1"),
        ("DSL-Modem", "DSL1"),
        ("Cable-Modem", "CM1"),
        ("Cloud", "NET1"),
        ("PSTN", "TEL1"),
        ("Cell-Tower", "TOWER1"),
        ("Central-Office", "CO1"),
        ("IoT-Light", "LIGHT1"),
        ("Board-SBC", "SBC1"),
        ("Board-MCU", "MCU1"),
        ("Smartphone", "SP1"),
    ] {
        add(&mut s, model, name, 0.0, 0.0);
    }
    // A card in the modular router's first bay (cards go in with the power off).
    cmd(&mut s, "device.power", json!({"device": "RM1", "on": false}));
    cmd(&mut s, "device.module.add", json!({"device": "RM1", "slot": "0/0", "module": "Card-2S"}));
    cmd(&mut s, "device.power", json!({"device": "RM1", "on": true}));
    link(&mut s, "HUB1", 0, "PC1", 0, "straight-through");
    link(&mut s, "HUB1", 1, "LT1", 0, "straight-through");
    link(&mut s, "SW1", 0, "SRV1", 0, "straight-through");
    link(&mut s, "SW1", 1, "PR1", 0, "straight-through");
    link(&mut s, "SW1", 24, "SW1B", 24, "fiber-multi-mode");
    link(&mut s, "SWP1", 0, "AP1", 0, "straight-through");
    link(&mut s, "SWP1", 1, "PH1", 0, "straight-through");
    link(&mut s, "R1", 0, "SW1", 2, "straight-through");
    link(&mut s, "FW1", 0, "R1", 1, "straight-through");
    let (a, b) = (first_port(&s, "R1", "Serial"), first_port(&s, "RM1", "Serial"));
    link(&mut s, "R1", a, "RM1", b, "serial-dce");
    let (a, b) = (first_port(&s, "DSL1", "Phone"), first_port(&s, "TEL1", "Phone"));
    link(&mut s, "DSL1", a, "TEL1", b, "phone");
    let (a, b) = (first_port(&s, "CM1", "Coaxial"), first_port(&s, "NET1", "Coaxial"));
    link(&mut s, "CM1", a, "NET1", b, "coaxial");
    s
}

/// The Physical tab of every family's device, in both themes: `device-physical-<NAME>-<theme>.png`.
fn device_physical_sheets(dir: &str, dark: bool) {
    let suffix = if dark { "dark" } else { "light" };
    let names = [
        "HUB1", "PC1", "LT1", "SW1", "SWP1", "R1", "RM1", "FW1", "AP1", "HR1", "SRV1", "PR1", "PH1", "DSL1", "CM1", "NET1", "TEL1", "TOWER1", "CO1",
        "LIGHT1", "SBC1", "MCU1", "SP1",
    ];
    for name in names {
        let s = physical_lab();
        app_sheet(dir, &format!("device-physical-{name}-{suffix}"), s, (900.0, 760.0), dark, move |app| {
            if let Ok(id) = app.session.topology.resolve(name) {
                app.ui.show_notes = false;
                netlabcraft_ui_egui::open_physical_tab(app, id);
            }
        });
    }
}

fn logical_sheet(dir: &str, dark: bool) {
    let suffix = if dark { "dark" } else { "light" };
    let mut s = Session::new();
    add(&mut s, "Router-WAN", "R1", 420.0, 90.0);
    add(&mut s, "Switch-ML-24", "CORE", 420.0, 230.0);
    add(&mut s, "Switch-24", "SW1", 240.0, 370.0);
    add(&mut s, "Switch-8-PoE", "SW2", 600.0, 370.0);
    add(&mut s, "Firewall-5", "FW1", 640.0, 90.0);
    add(&mut s, "Cloud", "NET", 860.0, 90.0);
    add(&mut s, "Server", "SRV1", 90.0, 230.0);
    add(&mut s, "PC-Modem", "PC1", 160.0, 520.0);
    add(&mut s, "Laptop", "LT1", 320.0, 520.0);
    add(&mut s, "Printer", "PR1", 480.0, 520.0);
    add(&mut s, "AccessPoint", "AP1", 640.0, 520.0);
    add(&mut s, "IP-Phone", "PH1", 790.0, 520.0);
    add(&mut s, "Smartphone", "SP1", 760.0, 640.0);
    add(&mut s, "IoT-Light", "LIGHT1", 880.0, 370.0);
    add(&mut s, "IoT-Thermostat", "THERM1", 900.0, 230.0);
    add(&mut s, "IoT-Plug", "PLUG1", 1010.0, 370.0);
    add(&mut s, "IoT-Door", "DOOR1", 1010.0, 520.0);
    add(&mut s, "IoT-Webcam", "CAM1", 900.0, 520.0);
    link(&mut s, "R1", 0, "CORE", 0, "straight-through");
    link(&mut s, "CORE", 1, "SW1", 0, "straight-through");
    link(&mut s, "CORE", 2, "SW2", 0, "straight-through");
    link(&mut s, "SRV1", 0, "CORE", 3, "straight-through");
    link(&mut s, "PC1", 0, "SW1", 1, "straight-through");
    link(&mut s, "LT1", 0, "SW1", 2, "straight-through");
    link(&mut s, "PR1", 0, "SW1", 3, "straight-through");
    link(&mut s, "AP1", 0, "SW2", 1, "straight-through");
    link(&mut s, "PH1", 0, "SW2", 2, "straight-through");
    link(&mut s, "FW1", 0, "R1", 1, "straight-through");
    link(&mut s, "R1", 2, "NET", 0, "serial-dce");
    app_sheet(dir, &format!("logical-{suffix}"), s, (1200.0, 760.0), dark, |_| {});
}

/// The physical workspace with the grid of every level ticked: the window, the colour palette and each level.
fn grid_sheets(dir: &str, dark: bool) {
    let suffix = if dark { "dark" } else { "light" };
    let lab = || {
        let mut s = Session::new();
        cmd(&mut s, "file.new", json!({"sample": "lan"}));
        s
    };
    let on = |app: &mut PacketApp| {
        for l in GridLevel::ALL {
            app.ui.prefs.grid.set_on(l, true);
        }
        app.set_physical(true);
    };
    app_sheet(dir, &format!("grid-window-{suffix}"), lab(), (1200.0, 760.0), dark, move |app| {
        on(app);
        app.open_grid_preferences();
    });
    app_sheet(dir, &format!("grid-colour-{suffix}"), lab(), (1200.0, 760.0), dark, move |app| {
        on(app);
        app.ui.prefs.grid.color = [0x3c, 0x78, 0xc8];
        app.open_grid_preferences();
        app.open_grid_colours();
    });
    for (name, place) in [("grid-intercity", None), ("grid-city", Some(1)), ("grid-building", Some(2))] {
        app_sheet(dir, &format!("{name}-{suffix}"), lab(), (1200.0, 760.0), dark, move |app| {
            on(app);
            if let Some(p) = place {
                app.enter_place(p);
            }
        });
    }
}

/// A small lab with a container in the city, for the menu sheets; the container's number.
fn create_lab() -> (Session, u32) {
    let mut s = Session::new();
    add(&mut s, "Switch-24", "SW1", 0.0, 0.0);
    add(&mut s, "Router-4", "R1", 0.0, 0.0);
    let city = 1;
    let made = cmd(&mut s, "place.add", json!({"kind": "container", "name": "Storage", "parent": city}));
    let container = made["id"].as_u64().and_then(|i| u32::try_from(i).ok()).unwrap_or(4);
    (s, container)
}

/// The physical bar with the `+` menu open at each level.
fn create_menu_sheets(dir: &str, dark: bool) {
    let suffix = if dark { "dark" } else { "light" };
    for (name, level) in [("intercity", None), ("city", Some(1)), ("building", Some(2)), ("closet", Some(3)), ("container", Some(0))] {
        let (s, container) = create_lab();
        app_sheet(dir, &format!("create-menu-{name}-{suffix}"), s, (1200.0, 520.0), dark, move |app| {
            app.set_physical(true);
            if let Some(id) = level {
                app.enter_place(if id == 0 { container } else { id });
            }
            app.open_create_menu();
        });
    }
}

/// A closet with one of every kind of furniture, devices in storage, cables on pegs and a control panel in use.
fn create_kinds_sheet(dir: &str, dark: bool) {
    let suffix = if dark { "dark" } else { "light" };
    let mut s = Session::new();
    for (model, name) in [
        ("Switch-24", "SW1"),
        ("Router-4", "R1"),
        ("Switch-24", "SW2"),
        ("PC", "PC1"),
        ("PLC", "PLC1"),
        ("HMI-Panel", "HMI1"),
        ("Switch-IND-8", "IND1"),
    ] {
        add(&mut s, model, name, 0.0, 0.0);
    }
    let closet = 3;
    cmd(&mut s, "furniture.add", json!({"kind": "rack", "size": 6, "place": closet, "x": 40.0, "y": 30.0}));
    cmd(&mut s, "furniture.add", json!({"kind": "shelf", "size": 2, "place": closet, "x": 400.0, "y": 30.0}));
    cmd(&mut s, "furniture.add", json!({"kind": "table", "place": closet, "x": 400.0, "y": 360.0}));
    cmd(&mut s, "furniture.add", json!({"kind": "inventory-shelf", "size": 2, "place": closet, "x": 40.0, "y": 700.0}));
    cmd(&mut s, "furniture.add", json!({"kind": "pegboard", "size": 2, "place": closet, "x": 900.0, "y": 700.0}));
    cmd(&mut s, "furniture.add", json!({"kind": "icp", "size": 8, "place": closet, "x": 1250.0, "y": 30.0}));
    cmd(&mut s, "furniture.mount", json!({"device": "SW1", "furniture": "Rack0", "slot": 1}));
    cmd(&mut s, "furniture.mount", json!({"device": "R1", "furniture": "Rack0", "slot": 2}));
    cmd(&mut s, "furniture.mount", json!({"device": "PC1", "furniture": "Table0"}));
    cmd(&mut s, "furniture.mount", json!({"device": "SW2", "furniture": "Inventory Shelf0", "slot": 1}));
    cmd(&mut s, "furniture.mount", json!({"device": "PLC1", "furniture": "Industrial Control Panel0", "slot": 1, "units": 2}));
    cmd(&mut s, "furniture.mount", json!({"device": "HMI1", "furniture": "Industrial Control Panel0", "slot": 3, "units": 2}));
    cmd(&mut s, "furniture.mount", json!({"device": "IND1", "furniture": "Industrial Control Panel0", "slot": 5, "units": 2}));
    link(&mut s, "SW1", 0, "R1", 0, "straight-through");
    link(&mut s, "PLC1", 0, "IND1", 0, "straight-through");
    let (p_sw, p_plc) = (port(&s, "SW1", 0), port(&s, "PLC1", 0));
    cmd(&mut s, "furniture.hang", json!({"furniture": "Cable Pegboard0", "device": "SW1", "port": p_sw}));
    cmd(&mut s, "furniture.hang", json!({"furniture": "Cable Pegboard0", "device": "PLC1", "port": p_plc, "peg": 4}));
    app_sheet(dir, &format!("create-kinds-{suffix}"), s, (1500.0, 1000.0), dark, move |app| {
        app.set_physical(true);
        app.enter_place(closet);
        // Everything in view: the furniture is 1560 by 970 closet units.
        let frame = app.session.topology.closet_layout(closet).frames.first().map(|(_, c)| (c.x, c.y));
        let (x, y) = frame.unwrap_or((0.0, 0.0));
        app.ui.zoom = 0.62;
        app.ui.pan = vec2(20.0, 50.0) - vec2(x, y) * 0.62;
    });
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "sheets".to_string());
    std::fs::create_dir_all(&dir).expect("output directory");
    for dark in [false, true] {
        icons_sheet(&dir, dark);
        cables_sheet(&dir, dark);
        closet_sheet(&dir, dark);
        closet_zoom_steps(&dir, dark);
        device_physical_sheets(&dir, dark);
        logical_sheet(&dir, dark);
        grid_sheets(&dir, dark);
        create_menu_sheets(&dir, dark);
        create_kinds_sheet(&dir, dark);
    }
}
