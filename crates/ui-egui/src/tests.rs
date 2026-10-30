use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use netlabcraft_engine::Session;
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
    for s in netlabcraft_engine::sample::SAMPLES {
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
    h.get_by_label("Router 3GE").click();
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
    let tagged_icmp = s.sim.trace().iter().find(|e| e.protocol == netlabcraft_wire::Protocol::Icmp && e.summary.contains("VLAN")).unwrap();
    let layers: Vec<String> = events::decode(&tagged_icmp.frame).into_iter().map(|(l, _)| l).collect();
    assert_eq!(layers, ["Ethernet II", "IPv4", "ICMP"]);
    assert!(events::decode(&tagged_icmp.frame)[0].1.iter().any(|(k, _)| k == "802.1Q VLAN"));
    assert!(events::hex_dump(&tagged_icmp.frame).starts_with("0000  "));
    assert_eq!(events::decode(&[1, 2, 3])[0].0, "Malformed frame");
}

#[test]
fn air_hops_show_their_802_11_layer_first() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wlc"})).unwrap();
    s.execute("sim.reset", json!({})).unwrap();
    s.execute("net.ping", json!({"from": "Laptop2", "to": "192.168.20.1", "count": 1})).unwrap();
    let probe = s.sim.trace().iter().find(|e| e.summary.contains("probe request: \"Guest\"")).unwrap();
    let layers = events::decode_event(&probe.frame, probe.air.as_deref());
    assert_eq!(layers.len(), 1);
    assert!(layers[0].1.iter().any(|(k, v)| k == "SSID" && v == "Guest"));
    let key = s.sim.trace().iter().find(|e| e.summary.contains("message 2 of 4")).unwrap();
    let names: Vec<String> = events::decode_event(&key.frame, key.air.as_deref()).into_iter().map(|(l, _)| l).collect();
    assert_eq!(names, ["IEEE 802.11", "Ethernet II (equivalent)", "IEEE 802.1X (EAPOL)"]);
    assert!(events::decode_event(&[], Some(&[1, 2])).first().is_some_and(|l| l.1.iter().any(|(k, _)| k == "Error")));
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
    open_browser(h.state_mut(), pc, "www.netlabcraft.lab");
    h.run_steps(3);
    assert!(h.query_by_label_contains("NetLabCraft web server").is_some());
    assert!(h.state().session.sim.trace().iter().any(|e| e.protocol == netlabcraft_wire::Protocol::Http));
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
    // The lab opened fitted to the workspace: find the device on screen.
    let at = on_screen(&h, 0.0, 0.0);
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
fn printing_makes_a_one_page_pdf_of_the_workspace() {
    let mut h = harness(Some("lan"));
    h.run_steps(2);
    let img = egui::ColorImage::filled([1200, 800], egui::Color32::LIGHT_BLUE);
    h.state_mut().ui.image_pending = true;
    h.state_mut().ui.print_pending = true;
    *h.state().ui.shot.lock().unwrap() = Some(std::sync::Arc::new(img));
    h.run_steps(1);
    let pdf = h.state_mut().take_requests().into_iter().find_map(|r| match r {
        UiRequest::Print(b) => Some(b),
        _ => None,
    });
    let pdf = pdf.expect("a page to print");
    assert!(pdf.starts_with(b"%PDF-1.4") && pdf.windows(9).any(|w| w == b"/Count 1 "));
    assert!(!h.state().ui.print_pending && !h.state().ui.image_pending);
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

/// Where a workspace point is on screen, with the view's pan and zoom.
fn on_screen(h: &Harness<'static, PacketApp>, x: f32, y: f32) -> egui::Pos2 {
    let app = h.state();
    app.ui.canvas_rect.unwrap().min + app.ui.pan + egui::vec2(x, y) * app.ui.zoom
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
    assert_eq!(h.state().ui.tool, Tool::Draw(netlabcraft_model::Shape::Polygon));
    let o = canvas_origin(&h);
    for (x, y) in [(200.0, 100.0), (360.0, 120.0), (300.0, 260.0), (240.0, 200.0)] {
        click(&mut h, o + egui::vec2(x, y));
    }
    assert_eq!(h.state().ui.canvas.poly.len(), 4);
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    let notes = &h.state().session.topology.annotations;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].shape, netlabcraft_model::Shape::Polygon);
    assert_eq!(notes[0].points.len(), 4);
    assert!(h.state().ui.canvas.poly.is_empty());
    // Esc drops a polygon half placed.
    click(&mut h, o + egui::vec2(500.0, 100.0));
    key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(h.state().ui.canvas.poly.is_empty());
    assert_eq!(h.state().ui.tool, Tool::Draw(netlabcraft_model::Shape::Polygon));

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
    assert_eq!(notes[1].shape, netlabcraft_model::Shape::Freehand);
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
    // Shift+drag a box around PC1 and PC2.
    let shift = egui::Modifiers::SHIFT;
    h.input_mut().events.push(egui::Event::ModifiersChanged(shift));
    // From below-left, along the bottom, then up: clear of the devices until the drag has begun.
    let start = on_screen(&h, 40.0, 190.0);
    press(&mut h, start, true, shift);
    h.run_steps(1);
    for at in [(90.0, 190.0), (190.0, 190.0), (290.0, 190.0), (290.0, 120.0), (290.0, 40.0)] {
        let p = on_screen(&h, at.0, at.1);
        h.input_mut().events.push(egui::Event::PointerMoved(p));
        h.run_steps(1);
    }
    let end = on_screen(&h, 290.0, 40.0);
    press(&mut h, end, false, shift);
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
    assert_eq!(t.shown_as(pc3, None), Some(netlabcraft_model::Shown::Device));

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

#[test]
fn a_drawing_being_dragged_out_stays_under_the_cables() {
    let mut h = harness(Some("two-subnets"));
    h.run_steps(2);
    let t = &h.state().session.topology;
    let (r1, s2) = (t.find("R1").map(|d| (d.x, d.y)).unwrap(), t.find("S2").map(|d| (d.x, d.y)).unwrap());
    // The cable's middle, then an ellipse dragged out over it (not released yet).
    let mid = on_screen(&h, (r1.0 + s2.0) / 2.0, (r1.1 + s2.1) / 2.0);
    h.state_mut().ui.tool = Tool::Draw(netlabcraft_model::Shape::Ellipse);
    let from = mid - egui::vec2(80.0, 60.0);
    press(&mut h, from, true, egui::Modifiers::NONE);
    h.run_steps(1);
    for i in 1..=8 {
        h.input_mut().events.push(egui::Event::PointerMoved(from + egui::vec2(20.0, 15.0) * i as f32));
        h.run_steps(1);
    }
    let img = h.render().unwrap();
    let dark = |p: egui::Pos2| {
        (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (dx, dy))).any(|(dx, dy)| {
            let px = img.get_pixel((p.x as i32 + dx) as u32, (p.y as i32 + dy) as u32);
            u32::from(px[0]) + u32::from(px[1]) + u32::from(px[2]) < 300
        })
    };
    assert!(dark(mid), "the cable shows through the drawing in progress");
    press(&mut h, from + egui::vec2(160.0, 120.0), false, egui::Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(h.state().session.topology.annotations.len(), 1);
}

/// Where a place's icon is on screen.
fn place_on_screen(h: &Harness<'static, PacketApp>, id: u32) -> egui::Pos2 {
    let app = h.state();
    let p = app.session.topology.place(id).unwrap();
    app.ui.canvas_rect.unwrap().min + app.ui.pan + egui::vec2(p.x, p.y) * app.ui.zoom
}

#[test]
fn the_physical_view_opens_places_and_goes_back() {
    for dark in [false, true] {
        let mut h = harness(Some("lan"));
        h.state_mut().ui.dark = dark;
        h.run_steps(2);
        // Toggle to Physical: the Intercity map with the lab's one city.
        h.get_by_label("Physical").click();
        h.run_steps(2);
        assert!(h.state().session.physical);
        assert_eq!(h.state().session.place_view, None);
        assert!(h.query_by_label("Home City").is_some() || h.query_by_label("Intercity").is_some());
        // Into the city (double-click its icon), then into its building.
        for (id, kind) in [(1, "city"), (2, "building")] {
            let at = place_on_screen(&h, id);
            press(&mut h, at, true, egui::Modifiers::NONE);
            press(&mut h, at, false, egui::Modifiers::NONE);
            h.run_steps(1);
            press(&mut h, at, true, egui::Modifiers::NONE);
            press(&mut h, at, false, egui::Modifiers::NONE);
            h.run_steps(2);
            assert_eq!(h.state().session.place_view, Some(id), "into the {kind}");
        }
        assert!(h.query_by_label("Main Building").is_some());
        // A new closet from the bar, renamed through the dialog.
        h.get_by_label("New Wiring Closet").click();
        h.run_steps(2);
        let closet = h.state().session.topology.places().iter().map(|p| p.id).max().unwrap();
        assert_eq!(h.state().session.topology.place(closet).unwrap().parent, Some(2));
        h.state_mut().ui.physical.renaming = Some((closet, "Lab rack".into()));
        h.run_steps(2);
        h.get_by_label("OK").click();
        h.run_steps(2);
        assert_eq!(h.state().session.topology.place(closet).unwrap().name, "Lab rack");
        // A cable length through its dialog.
        let pc1 = h.state().session.topology.resolve("PC1").unwrap();
        h.state_mut().ui.physical.length = Some((netlabcraft_model::End { device: pc1, port: 0 }, "150".into()));
        h.run_steps(2);
        assert!(h.query_by_label("Length (m):").is_some());
        h.get_by_label("OK").click();
        h.run_steps(2);
        let l = *h.state().session.topology.link_at(netlabcraft_model::End { device: pc1, port: 0 }).unwrap();
        assert_eq!(h.state().session.topology.cable_length(&l).meters, 150);
        // Back up through the bar: one level, then the whole way.
        h.get_by_label("Back to the enclosing place (Intercity is the top)").click();
        h.run_steps(2);
        assert_eq!(h.state().session.place_view, Some(1));
        h.get_by_label("Intercity").click();
        h.run_steps(2);
        assert_eq!(h.state().session.place_view, None);
        h.get_by_label("Logical").click();
        h.run_steps(2);
        assert!(!h.state().session.physical);
    }
}

fn click_at(h: &mut Harness<'static, PacketApp>, at: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(2);
}

#[test]
fn every_cable_tile_picks_its_cable_and_cables_ports_of_its_kind() {
    use netlabcraft_model::cable::Cable;
    let mut h = harness(None);
    h.run_steps(2);
    let (fiber_sw, sw, pc, r) = (egui::pos2(500.0, 300.0), egui::pos2(800.0, 300.0), egui::pos2(500.0, 460.0), egui::pos2(800.0, 460.0));
    for (model, at) in [("Switch-24-SFP", fiber_sw), ("Switch-24", sw), ("PC", pc), ("Router-3", r)] {
        h.state_mut().set_tool(Tool::Place(model));
        click_at(&mut h, at);
    }
    assert_eq!(h.state().session.topology.devices.len(), 4);
    h.get_by_label("Connections").click();
    h.run_steps(2);
    for c in Cable::ALL {
        h.get_by_label(c.label()).scroll_to_me();
        h.run_steps(2);
        h.get_by_label(c.label()).click();
        h.run_steps(1);
        assert_eq!(h.state().ui.tool, Tool::Cable(c), "{}", c.label());
    }
    // Fiber: the port menu offers the fiber and combo ports.
    h.get_by_label("Fiber").scroll_to_me();
    h.run_steps(12);
    h.get_by_label("Fiber").click();
    h.run_steps(1);
    h.get_by_label("Fiber").scroll_to_me();
    h.run_steps(12);
    h.get_by_label("Fiber").click();
    h.run_steps(1);
    assert_eq!(h.state().ui.tool, Tool::Select);
    h.get_by_label("Fiber").scroll_to_me();
    h.run_steps(12);
    h.get_by_label("Fiber").click();
    h.run_steps(1);
    click_at(&mut h, fiber_sw);
    assert!(h.query_by_label("FastEthernet0/1  (other cable type)").is_some());
    h.get_by_label("GigabitEthernet0/1").scroll_to_me();
    h.run_steps(12);
    h.get_by_label("GigabitEthernet0/1").click();
    h.run_steps(12);
    click_at(&mut h, sw);
    h.get_by_label("GigabitEthernet0/2").scroll_to_me();
    h.run_steps(12);
    h.get_by_label("GigabitEthernet0/2").click();
    h.run_steps(12);
    let t = &h.state().session.topology;
    assert_eq!(t.links.len(), 1);
    assert_eq!(t.links[0].cable, Cable::Fiber);
    // Console: a click on each device, then the PC's Terminal types into the router.
    h.get_by_label("Console").scroll_to_me();
    h.run_steps(12);
    h.get_by_label("Console").click();
    h.run_steps(1);
    click_at(&mut h, pc);
    click_at(&mut h, r);
    let (pc_id, r_id) = {
        let t = &h.state().session.topology;
        (t.resolve("PC1").unwrap(), t.resolve("R1").unwrap())
    };
    assert_eq!(h.state().session.topology.console_peer(pc_id), Some(r_id));
    h.state_mut().set_tool(Tool::Select);
    open_desktop_app(h.state_mut(), pc_id, "terminal");
    h.run_steps(3);
    assert!(h.query_by_label_contains("Console of R1").is_some());
    h.input_mut().events.push(egui::Event::Text("enable".into()));
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
    let log = &h.state_mut().session.console(r_id).log;
    assert!(log.contains("R1>enable"), "{log}");
}

/// Renders this test's harnesses in Spanish while it lives. The language is global and tests
/// run in parallel, so this sets it for the test's own thread (kittest draws on it).
struct InSpanish;

impl InSpanish {
    fn new() -> InSpanish {
        i18n::set_thread_lang(Some(i18n::Lang::Spanish));
        InSpanish
    }
}

impl Drop for InSpanish {
    fn drop(&mut self) {
        i18n::set_thread_lang(None);
    }
}

/// Every text the harness shows (labels and values of every widget), but what people type.
fn shown_texts(h: &Harness<'static, PacketApp>, into: &mut std::collections::BTreeSet<String>) {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT as _;
    let typed = |r: Role| matches!(r, Role::TextInput | Role::MultilineTextInput);
    for n in h.query_all_by(move |n| !typed(n.role()) && !n.parent().is_some_and(|p| typed(p.role()))) {
        let n = n.accesskit_node();
        into.extend(n.label());
        into.extend(n.value());
    }
}

/// The time bar keeps its widgets apart in both languages and both modes, at the default window
/// size and a narrow one: the hint and the play caption give way, nothing overlaps.
#[test]
fn the_time_bar_never_overlaps() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT as _;
    for spanish in [false, true] {
        let _lang = spanish.then(InSpanish::new);
        for sim in [false, true] {
            for width in [1280.0, 1000.0] {
                let mut s = Session::new();
                s.execute("file.new", json!({"sample": "static-routing"})).unwrap();
                let mut h =
                    Harness::builder().with_size(egui::vec2(width, 820.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
                h.state_mut().ui.mode = if sim { Mode::Simulation } else { Mode::Realtime };
                h.run_steps(3);
                let bar = h.get_by_role_and_label(Role::Tab, i18n::tr("Simulation")).rect();
                let widgets: Vec<(String, egui::Rect)> = h
                    .query_all_by(|n| !matches!(n.role(), Role::TextRun | Role::GenericContainer) && n.bounding_box().is_some())
                    .map(|n| (format!("{:?} {:?}", n.accesskit_node().role(), n.accesskit_node().label().or(n.accesskit_node().value())), n.rect()))
                    .filter(|(_, r)| r.height() < 40.0 && r.center().y > bar.top() && r.center().y < bar.bottom())
                    .collect();
                assert!(widgets.len() >= 5, "{widgets:?}");
                for (i, (a, ra)) in widgets.iter().enumerate() {
                    assert!(ra.right() <= width + 0.5, "{a} runs off the window ({ra:?}), es={spanish} sim={sim} width={width}");
                    for (b, rb) in widgets.iter().skip(i + 1) {
                        assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "{a} {ra:?} overlaps {b} {rb:?} (es={spanish} sim={sim} width={width})");
                    }
                }
            }
        }
    }
}

/// Renders the interface in Spanish through every window, tab, page and mode, and fails on any
/// text shown in English that has a Spanish row: text that skips `tl`/`tr`/`trf`.
#[test]
fn spanish_shows_no_text_that_has_a_translation_in_english() {
    let _es = InSpanish::new();
    let mut seen = std::collections::BTreeSet::new();
    for sample in ["services", "vlans", "wlc", "iot", "ip-phones", "wifi", "dhcp", "serial-wan", "firewall", "modular"] {
        let mut h = harness(Some(sample));
        h.state_mut().ui.show_notes = true;
        h.run_steps(2);
        shown_texts(&h, &mut seen);
        h.state_mut().ui.show_notes = false;
        let ids: Vec<_> = h.state().session.topology.devices.iter().map(|d| d.id).collect();
        for d in ids {
            open_console(h.state_mut(), d);
            for tab in [
                console::Tab::Physical,
                console::Tab::Config,
                console::Tab::Cli,
                console::Tab::Services,
                console::Tab::Desktop,
                console::Tab::Phone,
                console::Tab::Iot,
                console::Tab::Attributes,
            ] {
                h.state_mut().ui.windows.get_mut(&d).unwrap().tab = tab;
                h.run_steps(2);
                shown_texts(&h, &mut seen);
            }
            let ports = h.state().session.topology.device(d).unwrap().ports.len().min(3);
            let pages = [
                console::ConfigPage::Settings,
                console::ConfigPage::Home,
                console::ConfigPage::Vlans,
                console::ConfigPage::Routes,
                console::ConfigPage::Wireless,
            ]
            .into_iter()
            .chain((0..ports).map(console::ConfigPage::Port));
            for page in pages {
                let w = h.state_mut().ui.windows.get_mut(&d).unwrap();
                w.tab = console::Tab::Config;
                w.page = page;
                h.run_steps(2);
                shown_texts(&h, &mut seen);
            }
            let kind = h.state().session.topology.device(d).unwrap().kind;
            if kind == netlabcraft_model::DeviceKind::Server {
                for page in [
                    services::Page::Http,
                    services::Page::Dhcp,
                    services::Page::Dns,
                    services::Page::Syslog,
                    services::Page::Ntp,
                    services::Page::Tftp,
                    services::Page::Aaa,
                    services::Page::Ftp,
                    services::Page::Email,
                ] {
                    let w = h.state_mut().ui.windows.get_mut(&d).unwrap();
                    w.tab = console::Tab::Services;
                    w.set_service_page(page);
                    h.run_steps(2);
                    shown_texts(&h, &mut seen);
                }
            }
            if kind.is_host() {
                for which in ["home", "ip", "prompt", "email", "mib", "terminal", "browser"] {
                    if which == "browser" {
                        open_browser(h.state_mut(), d, "www.netlabcraft.lab");
                    } else {
                        open_desktop_app(h.state_mut(), d, which);
                    }
                    h.run_steps(3);
                    shown_texts(&h, &mut seen);
                }
            }
            h.state_mut().ui.windows.clear();
        }
        // Simulation mode with a frame picked, then the physical view.
        let hosts: Vec<_> = h.state().session.topology.devices.iter().filter(|d| d.kind.is_host()).map(|d| d.id).take(2).collect();
        if let [a, b] = hosts[..] {
            canvas::simple_pdu_for_tests(h.state_mut(), a, b);
            canvas::fire(h.state_mut(), 0);
        }
        h.state_mut().ui.mode = Mode::Simulation;
        let first = h.state().session.sim.trace().front().map(|e| e.id);
        h.state_mut().ui.events.selected = first;
        h.run_steps(3);
        shown_texts(&h, &mut seen);
        h.state_mut().ui.mode = Mode::Realtime;
        h.state_mut().set_physical(true);
        h.run_steps(3);
        shown_texts(&h, &mut seen);
    }
    // The activity windows, About and its credits.
    let mut h = harness(Some("activity-first-router"));
    h.run_steps(2);
    let results = h.state_mut().run("activity.check", json!({}));
    let a = &mut h.state_mut().ui.activity;
    a.show_instructions = true;
    a.results = results;
    a.show_compare = true;
    a.show_wizard = true;
    h.run_steps(3);
    shown_texts(&h, &mut seen);
    for credits in [false, true] {
        h.state_mut().ui.show_about = true;
        h.state_mut().ui.about_credits = credits;
        h.run_steps(3);
        shown_texts(&h, &mut seen);
    }
    let es = i18n::spanish();
    // A Spanish word can be an English key too ("Red" is the Spanish for "Network").
    let spanish_words: std::collections::BTreeSet<&str> = es.values().copied().collect();
    let english: Vec<&String> = seen.iter().filter(|t| !spanish_words.contains(t.as_str()) && es.get(t.as_str()).is_some_and(|s| s != t)).collect();
    assert!(english.is_empty(), "shown in English despite a Spanish row: {english:#?}");
    // And text of this crate shown as it is written: it needs a row, unless it reads the same in
    // both languages (names of protocols, products and commands).
    let literals = source_literals();
    let missing: Vec<&String> = seen
        .iter()
        .filter(|t| literals.contains(t.as_str()) && !es.contains_key(t.as_str()) && !spanish_words.contains(t.as_str()))
        .filter(|t| !SAME_IN_SPANISH.contains(&t.as_str()) && t.chars().any(|c| c.is_ascii_lowercase()))
        .collect();
    assert!(missing.is_empty(), "shown in English with no Spanish row: {missing:#?}");
}

/// Interface text that reads the same in Spanish.
const SAME_IN_SPANISH: &[&str] = &[
    "AAA",
    "DHCP",
    "DNS",
    "FTP",
    "FastEthernet0",
    "HTTP",
    "HTTPS",
    "ICMP",
    "Internet",
    "NTP",
    "SNMPv2c",
    "SNMPv3",
    "Syslog",
    "TFTP",
    "URL",
    "WPA2-Enterprise",
    "Wi-Fi",
    "get",
    "get-next",
    "no",
    "set",
];

/// Every literal handed to `tl("…")` or `trf("…", …)` has a Spanish row (the sweep above only
/// sees what its windows show; this covers every message, menu and tooltip in the code).
#[test]
fn every_translated_literal_has_a_spanish_row() {
    let es = i18n::spanish();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = Vec::new();
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == "tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        // Calls split over lines: join each statement's lines first.
        let flat = code.lines().map(str::trim).filter(|l| !l.starts_with("//")).collect::<Vec<_>>().join(" ");
        for call in ["tl(\"", "trf(\""] {
            for (at, _) in flat.match_indices(call) {
                let before = flat.get(..at).and_then(|b| b.chars().next_back());
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let Some(lit) = flat.get(at + call.len() - 1..).and_then(|rest| string_literals(rest).into_iter().next()) else { continue };
                seen += 1;
                if !es.contains_key(lit.as_str()) && !SAME_IN_SPANISH.contains(&lit.as_str()) {
                    missing.push(format!("{}: {lit}", path.file_name().unwrap().to_string_lossy()));
                }
            }
        }
    }
    assert!(seen > 300, "the scan found only {seen} calls");
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "no Spanish row for: {missing:#?}");
}

/// The string literals of this crate's code (not its tests), with `\n` escapes resolved.
fn source_literals() -> std::collections::BTreeSet<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == "tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        out.extend(string_literals(code));
    }
    out
}

/// The `"…"` literals of Rust code (comments and char literals skipped; raw strings aren't used
/// for interface text).
fn string_literals(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in code.lines() {
        let line = line.trim_start();
        if line.starts_with("//") {
            continue;
        }
        let mut chars = line.chars().peekable();
        let mut cur: Option<String> = None;
        let mut prev = ' ';
        while let Some(c) = chars.next() {
            match (&mut cur, c) {
                (None, '/') if chars.peek() == Some(&'/') => break,
                (None, '\'') => {
                    // A char literal such as '"' (but not a lifetime).
                    let rest: String = chars.clone().take(3).collect();
                    if rest.starts_with("\\") || rest.chars().nth(1) == Some('\'') {
                        while let Some(x) = chars.next() {
                            if x == '\'' {
                                break;
                            }
                            if x == '\\' {
                                chars.next();
                            }
                        }
                    }
                }
                (None, '"') if prev != 'r' => cur = Some(String::new()),
                (Some(s), '\\') => match chars.next() {
                    Some('n') => s.push('\n'),
                    Some(x) => s.push(x),
                    None => {}
                },
                (Some(s), '"') => {
                    out.push(std::mem::take(s));
                    cur = None;
                }
                (Some(s), x) => s.push(x),
                (None, _) => {}
            }
            prev = c;
        }
    }
    out
}

/// The left edge of the text field a label names (a form's field, through `labelled_by`).
fn field_left(h: &Harness<'static, PacketApp>, label: &str) -> f32 {
    use egui::accesskit::Role;
    h.get_by_role_and_label(Role::TextInput, i18n::tr(label)).rect().left()
}

/// The one-line forms keep their fields in one column in both languages, however long a label
/// grows in Spanish (it wraps instead of pushing its field out of line).
#[test]
fn form_fields_line_up_whatever_the_label_length() {
    for spanish in [false, true] {
        let _lang = spanish.then(InSpanish::new);
        // A server's DHCP page.
        let mut h = harness(Some("services"));
        h.state_mut().ui.show_notes = false;
        let srv = h.state().session.topology.resolve("Server0").unwrap();
        open_services_page(h.state_mut(), srv, "dhcp");
        h.run_steps(3);
        let lefts: Vec<f32> = ["Pool Name", "Default Gateway", "DNS Server", "Maximum Number of Users"].iter().map(|l| field_left(&h, l)).collect();
        assert!(lefts.iter().all(|x| (x - lefts[0]).abs() < 0.5), "DHCP fields out of line (es={spanish}): {lefts:?}");
        // A wireless laptop's Wi-Fi settings.
        let mut h = harness(Some("wlc"));
        h.state_mut().ui.show_notes = false;
        let laptop = h.state().session.topology.resolve("Laptop1").unwrap();
        open_config_page(h.state_mut(), laptop, "port:0");
        h.run_steps(3);
        let lefts: Vec<f32> =
            ["SSID", "WPA2 passphrase", "Username (WPA2-Enterprise)", "Password (WPA2-Enterprise)"].iter().map(|l| field_left(&h, l)).collect();
        assert!(lefts.iter().all(|x| (x - lefts[0]).abs() < 0.5), "Wi-Fi fields out of line (es={spanish}): {lefts:?}");
    }
}

/// The simulation panel shows its filter buttons whole above the time bar at the usual window
/// size, in both languages.
#[test]
fn the_simulation_panel_fits_above_the_time_bar() {
    use egui::accesskit::Role;
    for spanish in [false, true] {
        let _lang = spanish.then(InSpanish::new);
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "static-routing"})).unwrap();
        s.execute("net.ping", json!({"from": "PC1", "to": "172.16.3.10"})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1280.0, 820.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.mode = Mode::Simulation;
        h.run_steps(3);
        let bar = h.get_by_role_and_label(Role::Tab, i18n::tr("Simulation")).rect();
        for b in ["Edit Filters", "Show All/None"] {
            let r = h.get_by_role_and_label(Role::Button, i18n::tr(b)).rect();
            assert!(r.bottom() <= bar.top() - 2.0, "{b} {r:?} reaches the time bar {bar:?} (es={spanish})");
        }
    }
}

/// A device window opens below the view bar (Logical, Physical, zoom), never over it, even in a
/// small window.
#[test]
fn device_windows_open_below_the_view_bar() {
    use egui::accesskit::Role;
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wlc"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1024.0, 700.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
    h.state_mut().ui.show_notes = false;
    h.run_steps(2);
    let view_bar = h.get_by_role_and_label(Role::Tab, i18n::tr("Logical")).rect();
    let ids: Vec<DeviceId> = h.state().session.topology.devices.iter().map(|d| d.id).collect();
    for d in ids {
        open_console(h.state_mut(), d);
        h.run_steps(2);
        let area = h.ctx.memory(|m| m.area_rect(egui::Id::unique(("device", d.0)))).unwrap();
        assert!(area.top() >= view_bar.bottom(), "the window of {d:?} {area:?} covers the view bar {view_bar:?}");
        h.state_mut().ui.windows.clear();
        h.run_steps(1);
    }
}
