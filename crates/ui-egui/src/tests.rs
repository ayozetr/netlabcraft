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

#[test]
fn an_activity_opens_its_instructions_and_checks_results() {
    let mut h = harness(Some("activity-first-router"));
    h.run();
    h.get_by_label("Check Results").click();
    h.run();
    let r = h.state().ui.activity.results.clone().unwrap();
    assert_eq!(r["percent"], 0);
    assert!(r["results"].as_array().unwrap().len() >= 7);
    // Side by side: the answer next to what the lab has.
    h.get_by_label("Compare").click();
    h.run();
    assert!(h.state().ui.activity.show_compare);
    assert!(h.query_by_label("Compare with the Answer").is_some() && h.query_by_label("Your lab").is_some());
    h.state_mut().ui.activity.show_wizard = true;
    h.run();
    h.get_by_label("Capture answers from this lab").click();
    h.run();
    assert!(h.state().session.topology.activity.as_ref().unwrap().checks.iter().any(|c| c.label.starts_with("PC1")));
}

#[test]
fn exporting_the_workspace_crops_the_screenshot_to_a_png() {
    let mut h = harness(Some("lan"));
    h.run_steps(2);
    let area = h.state().ui.canvas_rect.expect("the workspace was drawn");
    // A fake 1200×800 screenshot: the workspace's pixels are green, the rest red.
    let mut img = egui::ColorImage::filled([1200, 800], egui::Color32::RED);
    for y in area.min.y as usize..area.max.y as usize {
        for x in area.min.x as usize..area.max.x as usize {
            if let Some(p) = img.pixels.get_mut(y * 1200 + x) {
                *p = egui::Color32::GREEN;
            }
        }
    }
    h.state_mut().ui.image_pending = true;
    *h.state().ui.shot.lock().unwrap() = Some(std::sync::Arc::new(img));
    h.run_steps(1);
    let png = h.state_mut().take_requests().into_iter().find_map(|r| match r {
        UiRequest::ExportImage(b) => Some(b),
        _ => None,
    });
    let png = png.expect("an image was exported");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let (w, hgt) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    assert_eq!((w, hgt), (area.width().round() as u32, area.height().round() as u32));
    assert!(!h.state().ui.image_pending);
}

#[test]
fn about_lists_the_contributors_as_a_bag_and_a_table() {
    let mut h = harness(None);
    h.state_mut().ui.show_about = true;
    h.run_steps(2);
    h.get_by_label("Contributors").click();
    h.run_steps(2);
    assert!(h.state().ui.about_credits);
    h.get_by_label("Table").click();
    h.run_steps(2);
    h.get_by_label("Real name").click();
    h.get_by_label("Ascending").click();
    h.run_steps(2);
    assert!(h.query_by_label("Descending").is_some());
}

/// The workspace's top-left corner on screen (where workspace 0,0 sits before panning).
fn canvas_origin(h: &Harness<'static, PacketApp>) -> egui::Pos2 {
    h.get_by_label("Logical").rect().left_bottom() + egui::vec2(0.0, 6.0)
}

fn press(h: &mut Harness<'static, PacketApp>, at: egui::Pos2, pressed: bool, modifiers: egui::Modifiers) {
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    h.input_mut().events.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers });
}

fn click(h: &mut Harness<'static, PacketApp>, at: egui::Pos2) {
    press(h, at, true, egui::Modifiers::NONE);
    press(h, at, false, egui::Modifiers::NONE);
    // Far enough apart in time not to count as a double click.
    h.run_steps(2);
}

fn key(h: &mut Harness<'static, PacketApp>, key: egui::Key, modifiers: egui::Modifiers) {
    h.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.run_steps(2);
}

#[test]
fn drawing_a_polygon_and_a_freehand_stroke_with_the_mouse() {
    let mut h = harness(None);
    h.run_steps(2);
    h.get_by_label("Draw Polygon (click the corners)").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.tool, Tool::Draw(packetcraft_model::Shape::Polygon));
    let o = canvas_origin(&h);
    for (x, y) in [(200.0, 100.0), (360.0, 120.0), (300.0, 260.0), (240.0, 200.0)] {
        click(&mut h, o + egui::vec2(x, y));
    }
    assert_eq!(h.state().ui.canvas.poly.len(), 4);
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    let notes = &h.state().session.topology.annotations;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].shape, packetcraft_model::Shape::Polygon);
    assert_eq!(notes[0].points.len(), 4);
    assert!(h.state().ui.canvas.poly.is_empty());
    // Esc drops a polygon half placed.
    click(&mut h, o + egui::vec2(500.0, 100.0));
    key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(h.state().ui.canvas.poly.is_empty());
    assert_eq!(h.state().ui.tool, Tool::Draw(packetcraft_model::Shape::Polygon));

    h.get_by_label("Draw Freehand (drag on the workspace)").click();
    h.run_steps(2);
    let start = o + egui::vec2(500.0, 250.0);
    press(&mut h, start, true, egui::Modifiers::NONE);
    h.run_steps(1);
    for i in 1..=30 {
        let t = i as f32 / 30.0;
        h.input_mut().events.push(egui::Event::PointerMoved(start + egui::vec2(t * 200.0, (t * 9.0).sin() * 40.0)));
        h.run_steps(1);
    }
    press(&mut h, start + egui::vec2(200.0, (9.0f32).sin() * 40.0), false, egui::Modifiers::NONE);
    h.run_steps(2);
    let notes = &h.state().session.topology.annotations;
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[1].shape, packetcraft_model::Shape::Freehand);
    assert!(notes[1].points.len() >= 3 && notes[1].points.len() <= 31, "{:?}", notes[1].points);
    // Both are one undo step each.
    h.state_mut().run("edit.undo", json!({}));
    assert_eq!(h.state().session.topology.annotations.len(), 1);
}

#[test]
fn picking_devices_grouping_them_and_opening_the_cluster() {
    let mut s = Session::new();
    for (x, y) in [(100.0, 100.0), (220.0, 100.0), (500.0, 300.0)] {
        s.execute("device.add", json!({"kind": "pc", "x": x, "y": y})).unwrap();
    }
    s.execute("link.add", json!({"a": "PC1", "b": "PC3"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
    h.run_steps(2);
    let o = canvas_origin(&h);
    // Shift+drag a box around PC1 and PC2.
    let shift = egui::Modifiers::SHIFT;
    h.input_mut().events.push(egui::Event::ModifiersChanged(shift));
    // From below-left, along the bottom, then up: clear of the devices until the drag has begun.
    press(&mut h, o + egui::vec2(40.0, 190.0), true, shift);
    h.run_steps(1);
    for at in [(90.0, 190.0), (190.0, 190.0), (290.0, 190.0), (290.0, 120.0), (290.0, 40.0)] {
        h.input_mut().events.push(egui::Event::PointerMoved(o + egui::vec2(at.0, at.1)));
        h.run_steps(1);
    }
    press(&mut h, o + egui::vec2(290.0, 40.0), false, shift);
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    let (pc1, pc2, pc3) = {
        let t = &h.state().session.topology;
        (t.resolve("PC1").unwrap(), t.resolve("PC2").unwrap(), t.resolve("PC3").unwrap())
    };
    assert_eq!(h.state().ui.picked.iter().copied().collect::<Vec<_>>(), vec![pc1, pc2]);
    h.get_by_label("Group into Cluster (Ctrl+G): the picked devices").click();
    h.run_steps(2);
    let t = &h.state().session.topology;
    assert_eq!(t.clusters.len(), 1);
    let c = t.clusters[0].clone();
    assert_eq!(c.devices, vec![pc1, pc2]);
    assert_eq!((c.x, c.y), (160.0, 100.0));
    assert_eq!(t.shown_as(pc3, None), Some(packetcraft_model::Shown::Device));

    // Into the cluster and back out through the bar.
    h.state_mut().enter_cluster(c.id);
    h.run_steps(2);
    assert_eq!(h.state().session.view, Some(c.id));
    assert!(h.query_by_label(&c.name).is_some());
    h.get_by_label("Root").click();
    h.run_steps(2);
    assert_eq!(h.state().session.view, None);
    h.state_mut().enter_cluster(c.id);
    h.run_steps(1);
    h.get_by_label("Back to the parent cluster (Root is the top)").click();
    h.run_steps(2);
    assert_eq!(h.state().session.view, None);

    // Rename through the dialog, then uncluster from the Edit menu.
    h.state_mut().ui.renaming_cluster = Some((c.id, "Lab bench".into()));
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(h.state().session.topology.clusters[0].name, "Lab bench");
    h.state_mut().ui.picked_clusters.insert(c.id);
    h.get_by_label("Edit").click();
    h.run_steps(3);
    h.get_by_label("Uncluster").click();
    h.run_steps(2);
    assert!(h.state().session.topology.clusters.is_empty());
    // Ctrl+G groups the selected device on its own.
    h.state_mut().ui.selected = Some(pc3);
    key(&mut h, egui::Key::G, egui::Modifiers::COMMAND);
    assert_eq!(h.state().session.topology.clusters.len(), 1);
    assert_eq!(h.state().session.topology.clusters[0].devices, vec![pc3]);
}

#[test]
fn clusters_render_nested_in_both_themes() {
    for dark in [false, true] {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "vlans"})).unwrap();
        let a = s.execute("cluster.create", json!({"devices": ["PC1", "PC2"], "name": "Room A"})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("cluster.create", json!({"devices": ["PC3"], "clusters": [a], "name": "Floor"})).unwrap();
        s.execute("note.add", json!({"shape": "polygon", "points": [[0, 0], [50, 0], [60, 40], [25, 10], [0, 40]]})).unwrap();
        s.execute("net.ping", json!({"from": "PC1", "to": "192.168.20.10", "count": 1})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.dark = dark;
        h.state_mut().ui.mode = Mode::Simulation;
        h.state_mut().ui.events.selected = h.state().session.sim.trace().back().map(|e| e.id);
        h.run_steps(2);
        let floor = h.state().session.topology.clusters[1].id;
        h.state_mut().enter_cluster(floor);
        h.run_steps(2);
        h.state_mut().enter_cluster(a);
        h.run_steps(2);
        assert!(h.query_by_label("Floor").is_some() && h.query_by_label("Room A").is_some());
    }
}

#[test]
fn polygons_triangulate_without_trouble() {
    let p = |v: &[(f32, f32)]| v.iter().map(|(x, y)| egui::pos2(*x, *y)).collect::<Vec<_>>();
    // A concave L: 6 corners, 4 triangles.
    assert_eq!(canvas::triangulate_for_tests(&p(&[(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 10.0), (0.0, 10.0)])).len(), 4);
    // Degenerate and self-crossing shapes end, whatever they give.
    let _ = canvas::triangulate_for_tests(&p(&[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]));
    let _ = canvas::triangulate_for_tests(&p(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]));
    let _ = canvas::triangulate_for_tests(&p(&[(f32::NAN, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 3.0)]));
}
