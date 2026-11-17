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
}
