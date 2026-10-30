//! Offscreen render of the app to a PNG, for looking at UI work without a window:
//!
//! `cargo run -p netlabcraft-ui-egui --example snapshot -- --sample vlans [--dark] [--simulation] [--size 1280x820]
//!  [--cmd 'id={json}']... [--console DEVICE]... [--event N] [--physical [--place N]] [--physical-tab]
//!  [--tab physical|config|cli|services|desktop|phone|iot|attributes] [--config-page settings|vlans|routes|wireless|port:N]
//!  [--instructions] [--wizard] [--info N] out.png`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use netlabcraft_engine::Session;
use netlabcraft_ui_egui::PacketApp;
use serde_json::json;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut session = Session::new();
    let (mut dark, mut size, mut out) = (false, (1280.0f32, 820.0f32), String::from("snapshot.png"));
    let mut consoles = Vec::new();
    let mut event: Option<u64> = None;
    let mut info: Option<u64> = None;
    let mut simulation = false;
    let mut desktop: Option<String> = None;
    let mut config = false;
    let mut services = false;
    let mut physical_tab = false;
    let mut about: Option<bool> = None;
    let mut compare = false;
    let mut page: Option<String> = None;
    let mut url: Option<String> = None;
    let mut physical = false;
    let mut place: Option<u32> = None;
    let mut connections: Option<Option<String>> = None;
    let mut tab: Option<String> = None;
    let mut config_page: Option<String> = None;
    let (mut instructions, mut wizard) = (false, false);
    let mut complex: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--sample" => {
                session.execute("file.new", json!({"sample": it.next().expect("sample id")})).expect("sample");
            }
            "--dark" => dark = true,
            "--simulation" => simulation = true,
            "--physical" => physical = true,
            "--place" => place = Some(it.next().and_then(|s| s.parse().ok()).expect("place number")),
            "--size" => {
                let (w, h) = it.next().and_then(|s| s.split_once('x')).expect("--size WxH");
                size = (w.parse().expect("width"), h.parse().expect("height"));
            }
            "--cmd" => {
                let c = it.next().expect("--cmd id={json}");
                let (id, p) =
                    c.split_once('=').map(|(a, b)| (a.to_string(), serde_json::from_str(b).expect("json"))).unwrap_or((c.clone(), json!({})));
                if let Err(e) = session.execute(&id, p) {
                    eprintln!("{id}: {e}");
                }
            }
            "--console" => consoles.push(it.next().expect("device").clone()),
            "--lang" => netlabcraft_ui_egui::i18n::set_lang(netlabcraft_ui_egui::i18n::Lang::from_locale(it.next().map_or("en", String::as_str))),
            "--desktop" => desktop = Some(it.next().expect("prompt|ip|home").clone()),
            "--config" => config = true,
            "--services" => services = true,
            "--physical-tab" => physical_tab = true,
            "--about" => about = Some(false),
            "--credits" => about = Some(true),
            "--compare" => compare = true,
            "--services-page" => page = Some(it.next().expect("page").clone()),
            "--url" => url = Some(it.next().expect("url").clone()),
            "--tab" => tab = Some(it.next().expect("tab name").clone()),
            "--config-page" => config_page = Some(it.next().expect("config page").clone()),
            "--instructions" => instructions = true,
            "--wizard" => wizard = true,
            "--connections" => connections = Some(None),
            "--cable" => connections = Some(Some(it.next().expect("cable id").clone())),
            "--info" => info = Some(it.next().and_then(|s| s.parse().ok()).expect("event id")),
            "--event" => event = Some(it.next().and_then(|s| s.parse().ok()).expect("event id")),
            "--complex-pdu" => complex = Some(it.next().expect("source device").clone()),
            other => out = other.to_string(),
        }
    }
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(size.0, size.1)).with_max_steps(12).wgpu().build_ui_state(
        |ui, app: &mut PacketApp| {
            app.apply_theme(ui.ctx());
            app.ui(ui);
        },
        {
            let mut app = PacketApp::new(session);
            app.ui.dark = dark;
            if let Some(credits) = about {
                app.ui.show_about = true;
                app.ui.about_credits = credits;
            }
            for c in &consoles {
                let id = app.session.topology.resolve(c).expect("device");
                netlabcraft_ui_egui::open_console(&mut app, id);
                if let Some(which) = &desktop {
                    netlabcraft_ui_egui::open_desktop_app(&mut app, id, which.as_str());
                }
                if config {
                    netlabcraft_ui_egui::open_config_tab(&mut app, id);
                }
                if physical_tab {
                    netlabcraft_ui_egui::open_physical_tab(&mut app, id);
                }
                if services {
                    netlabcraft_ui_egui::open_services_tab(&mut app, id);
                }
                if let Some(p) = &page {
                    netlabcraft_ui_egui::open_services_page(&mut app, id, p);
                }
                if let Some(u) = &url {
                    netlabcraft_ui_egui::open_browser(&mut app, id, u);
                }
                if let Some(t) = &tab {
                    netlabcraft_ui_egui::open_tab(&mut app, id, t);
                }
                if let Some(p) = &config_page {
                    netlabcraft_ui_egui::open_config_page(&mut app, id, p);
                }
            }
            if let Some(c) = &connections {
                netlabcraft_ui_egui::show_connections(&mut app, c.as_deref());
            }
            if let Some(c) = &complex {
                let id = app.session.topology.resolve(c).expect("device");
                netlabcraft_ui_egui::open_complex_pdu(&mut app, Some(id));
            }
            app.ui.events.selected = event;
            app.ui.events.info = info;
            if simulation {
                app.ui.mode = netlabcraft_ui_egui::Mode::Simulation;
            }
            app
        },
    );
    harness.run_steps(2);
    if physical {
        // After the first frames, so the view can centre on what it shows.
        let app = harness.state_mut();
        app.set_physical(true);
        if let Some(p) = place {
            app.enter_place(p);
        }
    }
    if compare {
        // After the first frames: opening an activity clears earlier results.
        let app = harness.state_mut();
        app.ui.activity.results = app.run("activity.check", serde_json::json!({}));
        app.ui.activity.show_compare = true;
    }
    if instructions || wizard {
        let app = harness.state_mut();
        app.ui.activity.show_instructions = instructions;
        app.ui.activity.show_wizard = wizard;
    }
    harness.run_steps(6);
    let img = harness.render().expect("render");
    img.save(&out).expect("save png");
    eprintln!("wrote {out} ({}x{})", img.width(), img.height());
}
