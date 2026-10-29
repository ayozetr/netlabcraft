use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use packetcraft_engine::Session;
use serde_json::json;

use crate::*;

fn harness(sample: Option<&str>) -> Harness<'static, PacketApp> {
    let mut s = Session::new();
    if let Some(id) = sample {
        s.execute("file.new", json!({"sample": id})).unwrap();
    }
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s))
}

#[test]
fn every_sample_renders_in_both_themes() {
    for s in packetcraft_engine::sample::SAMPLES {
        for dark in [false, true] {
            let mut h = harness(Some(s.id));
            h.state_mut().ui.dark = dark;
            h.run_steps(3);
            assert!(h.query_by_label("File").is_some());
            assert!(h.query_by_label("Realtime").is_some() && h.query_by_label("Simulation").is_some());
            h.state_mut().ui.mode = Mode::Simulation;
            h.run_steps(2);
        }
    }
}

#[test]
fn placing_a_device_with_the_palette() {
    let mut h = harness(None);
    h.run_steps(2);
    h.get_by_label("Router-3").click();
    h.run_steps(1);
    assert_eq!(h.state().ui.tool, Tool::Place("Router-3"));
    // Click the middle of the canvas.
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(600.0, 300.0)));
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(600.0, 300.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(2);
    assert_eq!(h.state().session.topology.devices.len(), 1);
    assert_eq!(h.state().session.topology.devices[0].name, "R1");
}

#[test]
fn typing_in_a_console() {
    let mut h = harness(Some("two-subnets"));
    let r1 = h.state().session.topology.resolve("R1").unwrap();
    open_console(h.state_mut(), r1);
    h.state_mut().ui.show_notes = false;
    h.run_steps(3);
    for line in ["enable", "show ip route"] {
        h.input_mut().events.push(egui::Event::Text(line.into()));
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(2);
    }
    let log = &h.state_mut().session.console(r1).log;
    assert!(log.contains("R1#show ip route"), "{log}");
    assert!(log.contains("C    10.0.1.0/24"), "{log}");
}

#[test]
fn frame_details_decode_every_layer() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "vlans"})).unwrap();
    s.execute("net.ping", json!({"from": "PC1", "to": "192.168.20.10", "count": 2})).unwrap();
    let tagged_icmp = s.sim.trace().iter().find(|e| e.protocol == packetcraft_wire::Protocol::Icmp && e.summary.contains("VLAN")).unwrap();
    let layers: Vec<String> = events::decode(&tagged_icmp.frame).into_iter().map(|(l, _)| l).collect();
    assert_eq!(layers, ["Ethernet II", "IPv4", "ICMP"]);
    assert!(events::decode(&tagged_icmp.frame)[0].1.iter().any(|(k, _)| k == "802.1Q VLAN"));
    assert!(events::hex_dump(&tagged_icmp.frame).starts_with("0000  "));
    assert_eq!(events::decode(&[1, 2, 3])[0].0, "Malformed frame");
}

#[test]
fn a_simple_pdu_lands_in_the_pdu_list() {
    let mut h = harness(Some("lan"));
    h.state_mut().ui.show_notes = false;
    let (a, b) = {
        let t = &h.state().session.topology;
        (t.resolve("PC1").unwrap(), t.resolve("PC3").unwrap())
    };
    h.run_steps(2);
    canvas::simple_pdu_for_tests(h.state_mut(), a, b);
    canvas::fire(h.state_mut(), 0);
    h.run_steps(2);
    let pdus = &h.state().ui.pdus;
    assert_eq!(pdus.len(), 1);
    assert_eq!(pdus[0].ok, Some(true));
    assert!(h.query_by_label("Fire PDU 1").is_some());
}

#[test]
fn device_windows_show_every_tab() {
    for (sample, dev) in [("vlans", "R1"), ("vlans", "S1"), ("vlans", "PC1")] {
        let mut h = harness(Some(sample));
        h.state_mut().ui.show_notes = false;
        let d = h.state().session.topology.resolve(dev).unwrap();
        open_console(h.state_mut(), d);
        for tab in [console::Tab::Physical, console::Tab::Config, console::Tab::Cli, console::Tab::Desktop, console::Tab::Attributes] {
            h.state_mut().ui.windows.get_mut(&d).unwrap().tab = tab;
            h.run_steps(2);
        }
        let port = console::ConfigPage::Port(0);
        let w = h.state_mut().ui.windows.get_mut(&d).unwrap();
        w.tab = console::Tab::Config;
        w.page = port;
        h.run_steps(2);
    }
}

#[test]
fn the_browser_opens_a_page_by_name() {
    let mut h = harness(Some("services"));
    let pc = h.state().session.topology.resolve("PC1").unwrap();
    open_browser(h.state_mut(), pc, "www.packetcraft.lab");
    h.run_steps(3);
    assert!(h.query_by_label_contains("PacketCraft web server").is_some());
    assert!(h.state().session.sim.trace().iter().any(|e| e.protocol == packetcraft_wire::Protocol::Http));
}

#[test]
fn every_services_page_renders() {
    let mut h = harness(Some("services"));
    let srv = h.state().session.topology.resolve("Server0").unwrap();
    open_services_tab(h.state_mut(), srv);
    for page in [services::Page::Http, services::Page::Dhcp, services::Page::Dns] {
        h.state_mut().ui.windows.get_mut(&srv).unwrap().set_service_page(page);
        h.run_steps(2);
    }
    assert!(h.query_by_label("Add").is_some());
}

#[test]
fn one_click_on_a_device_opens_it() {
    let mut s = Session::new();
    s.execute("device.add", json!({"kind": "router", "x": 0, "y": 0})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
    h.run_steps(2);
    // The workspace origin is its top-left corner; find the device on screen.
    let pos = h.get_by_label("Logical").rect().left_bottom() + egui::vec2(0.0, 6.0);
    let at = pos + egui::vec2(0.0, 0.0);
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(2);
    let d = h.state().session.topology.devices[0].id;
    assert!(h.state().ui.windows.contains_key(&d), "one click opens the device window");
}
