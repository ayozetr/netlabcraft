// UI harness tests for the wiring-closet view and the Structured Cabling items of the device box.
#[cfg(test)]
mod t {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use serde_json::json;

    use crate::{PacketApp, Tool};

    /// The `lan` sample with a rack, a shelf, a patch panel and a power strip in its closet, the
    /// physical view open on the closet.
    fn closet() -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "lan"})).unwrap();
        let names: Vec<String> = s.topology.devices.iter().map(|d| d.name.clone()).collect();
        s.execute("furniture.add", json!({"kind": "rack"})).unwrap();
        s.execute("furniture.add", json!({"kind": "shelf"})).unwrap();
        s.execute("furniture.mount", json!({"device": names[0], "furniture": "Rack0", "slot": 2})).unwrap();
        s.execute("furniture.mount", json!({"device": names[1], "furniture": "Shelf0", "slot": 1})).unwrap();
        s.execute("passive.add", json!({"kind": "copper-patch-panel", "furniture": "Rack0", "slot": 5})).unwrap();
        s.execute("passive.add", json!({"kind": "power-strip", "furniture": "Rack0", "slot": 8})).unwrap();
        s.execute("view.enter_place", json!({"id": 3})).unwrap();
        Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s))
    }

    #[test]
    fn the_closet_draws_furniture_devices_and_cables_in_both_themes() {
        for dark in [false, true] {
            let mut h = closet();
            h.state_mut().ui.dark = dark;
            h.run_steps(3);
            crate::physical::fit(h.state_mut());
            h.run_steps(2);
            let names: Vec<String> = h.state().session.topology.devices.iter().map(|d| d.name.clone()).collect();
            assert!(h.query_by_label("Rack0, 12 units").is_some(), "the rack is a node");
            assert!(h.query_by_label("Shelf0, 4 levels").is_some(), "the shelf is a node");
            assert!(h.query_by_label(&format!("{}, on Rack0", names[0])).is_some());
            assert!(h.query_by_label(&format!("{}, on Shelf0", names[1])).is_some());
            assert!(h.query_by_label("Copper Patch Panel0, on Rack0").is_some());
            assert!(h.query_by_label("Power Distribution Device0, on Rack0").is_some());
            // A device that stands on nothing is a node too, and says nothing of furniture.
            assert!(h.query_by_label(&names[2]).is_some());
        }
    }

    #[test]
    fn the_fit_keeps_the_furniture_below_the_title_band() {
        let mut h = closet();
        h.run_steps(3);
        crate::physical::fit(h.state_mut());
        h.run_steps(2);
        let app = h.state();
        let layout = app.session.topology.closet_layout(3);
        assert!(!layout.frames.is_empty());
        for (_, c) in &layout.frames {
            let top = app.ui.pan.y + c.y * app.ui.zoom;
            assert!(top >= crate::closet::TITLE_BAND - 0.5, "a frame starts at {top}, inside the title band");
        }
    }

    #[test]
    fn the_zoom_buttons_zoom_about_the_middle_and_reset_returns_to_true_size() {
        let mut h = closet();
        h.run_steps(3);
        crate::physical::fit(h.state_mut());
        h.run_steps(2);
        let mid = h.state().ui.canvas.size / 2.0;
        let under = |app: &PacketApp| (mid - app.ui.pan) / app.ui.zoom;
        let before = under(h.state());
        let zoom = h.state().ui.zoom;
        h.state_mut().zoom(Some(1.25));
        h.run_steps(2);
        assert!(h.state().ui.zoom > zoom);
        assert!((under(h.state()) - before).length() < 0.5, "the point in the middle of the view stays there");
        h.state_mut().zoom(Some(0.8));
        h.state_mut().zoom(Some(0.8));
        assert!(h.state().ui.zoom < zoom);
        h.state_mut().reset_view();
        h.run_steps(2);
        assert_eq!(h.state().ui.zoom, 1.0);
        let app = h.state();
        for (_, c) in &app.session.topology.closet_layout(3).frames {
            assert!(app.ui.pan.y + c.y >= crate::closet::TITLE_BAND - 0.5, "the furniture starts below the title band at true size");
        }
    }

    #[test]
    fn the_closet_follows_the_engine_after_every_change() {
        let mut h = closet();
        h.run_steps(3);
        let first = h.state().session.topology.devices[2].name.clone();
        h.state_mut().session.execute("furniture.mount", json!({"device": first, "furniture": "Shelf0", "slot": 3})).unwrap();
        h.run_steps(2);
        assert!(h.query_by_label(&format!("{first}, on Shelf0")).is_some());
        h.state_mut().session.execute("furniture.remove", json!({"furniture": "Shelf0"})).unwrap();
        h.run_steps(2);
        assert!(h.query_by_label("Shelf0, 4 levels").is_none());
        assert!(h.query_by_label(&first).is_some(), "what stood on it stays in the closet");
        // Undo and redo go through the same commands.
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        h.run_steps(2);
        assert!(h.query_by_label("Shelf0, 4 levels").is_some());
    }

    #[test]
    fn a_patch_tile_clicked_on_the_closet_adds_a_passive_device() {
        let mut h = closet();
        h.run_steps(3);
        h.state_mut().set_tool(Tool::Place("copper-wall-mount"));
        let r = h.state().ui.canvas_rect.unwrap();
        let at = r.max - egui::vec2(40.0, 40.0);
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run_steps(3);
        let t = &h.state().session.topology;
        assert!(t.physical.fittings.iter().any(|f| f.kind.id() == "copper-wall-mount"), "a wall mount stands in the closet");
    }

    #[test]
    fn structured_cabling_has_four_tiles_and_ctrl_alt_k_opens_them() {
        let mut h = closet();
        h.run_steps(2);
        h.get_by_label("Connections").click();
        h.run_steps(2);
        h.get_by_label("Structured Cabling").click();
        h.run_steps(2);
        for label in ["Copper Patch Panel", "Fiber Patch Panel", "Copper Wall Mount", "Fiber Wall Mount"] {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
        h.get_by_label("Fiber Wall Mount").click();
        h.run_steps(1);
        assert_eq!(h.state().ui.tool, Tool::Place("fiber-wall-mount"));
        // From another category, the shortcut brings the tiles back.
        h.state_mut().ui.bottom.category = crate::bottom::Category::NetworkDevices;
        h.state_mut().ui.bottom.sub = crate::bottom::Sub::Routers;
        h.run_steps(2);
        assert!(h.query_by_label("Copper Wall Mount").is_none());
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::K,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND | egui::Modifiers::ALT,
        });
        h.run_steps(2);
        assert_eq!(h.state().ui.bottom.sub, crate::bottom::Sub::Cabling);
        assert!(h.query_by_label("Copper Wall Mount").is_some());
    }

    /// Drags with `button` from `from` to `to` in a few steps.
    fn drag_button(h: &mut Harness<'static, PacketApp>, from: egui::Pos2, to: egui::Pos2, button: egui::PointerButton) {
        let ev = |pos, pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE };
        h.input_mut().events.push(egui::Event::PointerMoved(from));
        h.input_mut().events.push(ev(from, true));
        h.run_steps(1);
        for i in 1..=8 {
            h.input_mut().events.push(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
            h.run_steps(1);
        }
        h.input_mut().events.push(ev(to, false));
        h.run_steps(2);
    }

    #[test]
    fn the_right_and_middle_buttons_pan_the_closet_and_the_left_one_does_not() {
        let mut h = bare();
        let r = h.state().ui.canvas_rect.unwrap();
        let empty = r.min + egui::vec2(r.width() * 0.9, r.height() * 0.8);
        for button in [egui::PointerButton::Secondary, egui::PointerButton::Middle] {
            let pan = h.state().ui.pan;
            drag_button(&mut h, empty, empty - egui::vec2(40.0, 30.0), button);
            assert!((h.state().ui.pan - (pan - egui::vec2(40.0, 30.0))).length() < 12.0, "{button:?}: {pan:?} -> {:?}", h.state().ui.pan);
        }
        // The left button on empty ground pans nothing.
        let pan = h.state().ui.pan;
        drag_button(&mut h, empty, empty - egui::vec2(40.0, 30.0), egui::PointerButton::Primary);
        assert_eq!(h.state().ui.pan, pan);
        // A right click without movement still opens the context menu.
        let ev =
            |pressed| egui::Event::PointerButton { pos: empty, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
        h.input_mut().events.push(egui::Event::PointerMoved(empty));
        h.input_mut().events.push(ev(true));
        h.input_mut().events.push(ev(false));
        h.run_steps(3);
        assert!(h.query_all_by_label("Add to this closet").count() >= 1, "the empty-ground menu is open");
    }

    fn tap(h: &mut Harness<'static, PacketApp>, at: egui::Pos2) {
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run_steps(3);
    }

    /// Three bare devices in a closet: two switches and a router, none cabled.
    fn bare() -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        for m in ["Switch 24FE", "Switch 24FE", "Router 2GE"] {
            s.execute("device.add", json!({"model": m})).unwrap();
        }
        s.execute("furniture.add", json!({"kind": "rack"})).unwrap();
        let closet = s.topology.places().iter().find(|p| p.kind == netlabcraft_model::physical::PlaceKind::Closet).map(|p| p.id).unwrap();
        s.execute("view.enter_place", json!({"id": closet})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.run_steps(3);
        let place = h.state().session.place_view.unwrap();
        crate::closet::fit_all(h.state_mut(), place);
        h.run_steps(3);
        h
    }

    /// Where a device's port is drawn.
    fn socket(h: &Harness<'static, PacketApp>, device: usize, port: usize) -> egui::Pos2 {
        let id = h.state().session.topology.devices[device].id;
        let pole = netlabcraft_model::furniture::Pole::Device(netlabcraft_model::End { device: id, port });
        h.state().ui.physical.closet.seen_ports.iter().find(|(p, _)| *p == pole).map(|(_, r)| r.center()).expect("the socket is drawn")
    }

    #[test]
    fn two_clicks_on_ports_cable_them_and_the_tool_stays_picked() {
        let mut h = bare();
        h.state_mut().set_tool(Tool::Cable(netlabcraft_model::cable::Cable::Auto));
        h.run_steps(2);
        let (a, b) = (socket(&h, 0, 0), socket(&h, 1, 0));
        tap(&mut h, a);
        assert!(h.state().ui.physical.closet.wire.is_some(), "the first port is picked");
        assert!(h.state().session.topology.links.is_empty());
        tap(&mut h, b);
        assert_eq!(h.state().session.topology.links.len(), 1, "the cable joins the two ports");
        assert!(h.state().ui.physical.closet.wire.is_none());
        assert_eq!(h.state().ui.tool, Tool::Cable(netlabcraft_model::cable::Cable::Auto), "the tool stays for the next cable");
        let l = &h.state().session.topology.links[0];
        assert_eq!(
            (l.a.device, l.a.port, l.b.device, l.b.port),
            (h.state().session.topology.devices[0].id, 0, h.state().session.topology.devices[1].id, 0)
        );
    }

    #[test]
    fn tab_to_a_port_and_enter_cable_two_ports_from_the_keyboard() {
        let mut h = bare();
        h.run_steps(2);
        let label = |h: &Harness<'static, PacketApp>, device: usize| {
            let t = &h.state().session.topology;
            let e = netlabcraft_model::End { device: t.devices[device].id, port: 0 };
            format!("Port, {}", t.end_label(e))
        };
        let enter = |h: &mut Harness<'static, PacketApp>| {
            h.input_mut().events.push(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(3);
        };
        let (first, second) = (label(&h, 0), label(&h, 1));
        h.get_by_label(&first).focus();
        h.run_steps(2);
        enter(&mut h);
        assert!(h.state().ui.physical.closet.wire.is_some(), "Enter on the first port starts the cable");
        assert!(h.state().session.topology.links.is_empty());
        h.get_by_label(&second).focus();
        h.run_steps(2);
        enter(&mut h);
        assert_eq!(h.state().session.topology.links.len(), 1, "Enter on the second port finishes it");
        assert!(h.state().ui.physical.closet.wire.is_none());
    }

    #[test]
    fn esc_and_right_click_drop_a_cable_being_made() {
        let mut h = bare();
        h.state_mut().set_tool(Tool::Cable(netlabcraft_model::cable::Cable::Auto));
        h.run_steps(2);
        let at = socket(&h, 0, 0);
        tap(&mut h, at);
        assert!(h.state().ui.physical.closet.wire.is_some());
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(3);
        assert!(h.state().ui.physical.closet.wire.is_none());
        assert_eq!(h.state().ui.tool, Tool::Cable(netlabcraft_model::cable::Cable::Auto), "the first Esc drops the cable, not the tool");
        // Right-click does the same.
        let at = socket(&h, 0, 0);
        tap(&mut h, at);
        let at = socket(&h, 2, 0);
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run_steps(3);
        assert!(h.state().ui.physical.closet.wire.is_none());
        assert!(h.state().session.topology.links.is_empty());
    }

    #[test]
    fn a_busy_port_is_refused_with_a_reason_and_no_cable_appears() {
        let mut h = bare();
        let names: Vec<String> = h.state().session.topology.devices.iter().map(|d| d.name.clone()).collect();
        let (a_name, b_name) = (names[0].clone(), names[1].clone());
        h.state_mut().session.execute("link.add", json!({"a": a_name, "a_port": "fa0/1", "b": b_name, "b_port": "fa0/1"})).unwrap();
        h.run_steps(2);
        let busy = h.state().session.topology.links[0].a;
        let index = h.state().session.topology.devices.iter().position(|d| d.id == busy.device).unwrap();
        h.state_mut().set_tool(Tool::Cable(netlabcraft_model::cable::Cable::Auto));
        h.run_steps(2);
        let at = socket(&h, index, busy.port);
        tap(&mut h, at);
        assert!(h.state().ui.physical.closet.wire.is_none(), "a busy port starts no cable");
        let (msg, error) = h.state().ui.status.clone().expect("the refusal is said");
        assert!(error && msg.contains("already cabled"), "{msg}");
        // As the second end it is refused too, and the first stays lit.
        let at = socket(&h, 2, 0);
        tap(&mut h, at);
        assert!(h.state().ui.physical.closet.wire.is_some());
        let at = socket(&h, index, busy.port);
        tap(&mut h, at);
        assert_eq!(h.state().session.topology.links.len(), 1, "no second cable");
        assert!(h.state().ui.physical.closet.wire.is_some());
    }

    /// Drags a device-box tile by its label to `to`, in steps, and releases it.
    fn drag_tile(h: &mut Harness<'static, PacketApp>, label: &str, to: egui::Pos2, esc: bool) {
        let from = h.get_by_label(label).rect().center();
        h.input_mut().events.push(egui::Event::PointerMoved(from));
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(2);
        for i in 1..=8 {
            h.input_mut().events.push(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
            h.run_steps(1);
        }
        assert!(h.state().ui.tile_drag.is_some(), "the tile is being dragged");
        if esc {
            h.input_mut().events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(2);
            assert!(h.state().ui.tile_drag.is_none(), "Esc drops the tile");
        }
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(3);
    }

    #[test]
    fn a_tile_dragged_onto_the_closet_adds_the_passive_device_there() {
        let mut h = closet();
        h.run_steps(2);
        h.get_by_label("Connections").click();
        h.run_steps(2);
        h.get_by_label("Structured Cabling").click();
        h.run_steps(2);
        let area = h.state().ui.canvas_rect.unwrap();
        let on_floor = area.max - egui::vec2(60.0, 60.0);
        // Esc, or letting go outside the workspace, places nothing.
        drag_tile(&mut h, "Copper Wall Mount", on_floor, true);
        assert!(h.state().session.topology.physical.fittings.iter().all(|f| f.kind.id() != "copper-wall-mount"));
        drag_tile(&mut h, "Copper Wall Mount", egui::pos2(300.0, area.max.y + 30.0), false);
        assert!(h.state().session.topology.physical.fittings.iter().all(|f| f.kind.id() != "copper-wall-mount"));
        drag_tile(&mut h, "Copper Wall Mount", on_floor, false);
        assert!(h.state().session.topology.physical.fittings.iter().any(|f| f.kind.id() == "copper-wall-mount"), "dropped on the floor");
        assert_eq!(h.state().ui.tool, Tool::Select, "dragging does not pick the tile");
    }

    #[test]
    fn a_model_tile_dragged_onto_the_logical_workspace_places_the_device() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(Session::new()));
        h.run_steps(3);
        let area = h.state().ui.canvas_rect.unwrap();
        drag_tile(&mut h, "Router 2GE", area.center(), false);
        assert_eq!(h.state().session.topology.devices.len(), 1, "the router stands where it was released");
    }
}
