// Set Background Image (Shift+I): the window, its options, Apply and Reset; Image Cleanup.
#[cfg(test)]
mod t {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use netlabcraft_engine::Session;
    use netlabcraft_engine::images::pattern_png;
    use netlabcraft_model::backdrop::{BgMode, INTERCITY};
    use serde_json::json;

    use crate::shortcuts::{Act, text};
    use crate::{PacketApp, UiRequest};

    fn harness() -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1280.0, 900.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.show_notes = false;
        h
    }

    fn has(h: &Harness<'static, PacketApp>, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    fn click(h: &mut Harness<'static, PacketApp>, label: &str) {
        h.get_by_role_and_label(Role::Button, label).click();
        h.run_steps(3);
    }

    /// Browse, and the shell answers with a generated picture.
    fn browse(h: &mut Harness<'static, PacketApp>, w: u32, ht: u32) {
        click(h, "Browse…");
        assert!(h.state_mut().take_requests().iter().any(|r| matches!(r, UiRequest::PickImage)));
        h.state_mut().image_picked(pattern_png(w, ht));
        h.run_steps(3);
    }

    fn closet(h: &Harness<'static, PacketApp>) -> u32 {
        h.state().session.topology.default_place()
    }

    #[test]
    fn shift_i_opens_the_window_and_the_logical_one_has_no_place_options() {
        assert_eq!(text(Act::SetBackground), "Shift+I");
        let mut h = harness();
        assert!(!h.state().ui.pictures.window.open);
        h.state_mut().open_background_window();
        h.run_steps(3);
        for label in
            ["Browse…", "Apply", "Reset", "Sizes", "Layout", "Original size", "Tiled", "Stretch to the map area", "Custom width", "Custom height"]
        {
            assert!(has(&h, label), "{label}");
        }
        assert!(!has(&h, "Size of the place") && !has(&h, "Place Icon"), "the logical workspace has no size and no icon");
        assert!(has(&h, "No picture"));
    }

    #[test]
    fn a_place_window_has_the_size_of_the_place_and_an_icon_tab() {
        let mut h = harness();
        h.state_mut().set_physical(true);
        let c = closet(&h);
        h.state_mut().open_pictures(false, c);
        h.run_steps(3);
        for label in ["Size of the place", "Place Icon", "Background Image", "Width (m):", "Length (m):", "Height (m):", "Volume:", "Scale:"] {
            assert!(has(&h, label), "{label}");
        }
        // The Intercity map has a size and no icon.
        h.state_mut().open_pictures(false, INTERCITY);
        h.run_steps(3);
        assert!(has(&h, "Size of the place") && !has(&h, "Place Icon"));
    }

    #[test]
    fn a_picked_picture_fills_the_sizes_and_the_scale_follows_the_layout() {
        let mut h = harness();
        h.state_mut().set_physical(true);
        let c = closet(&h);
        h.state_mut().open_pictures(false, c);
        h.run_steps(3);
        browse(&mut h, 200, 100);
        assert!(has(&h, "200 × 100 px"), "the picture's size");
        assert!(has(&h, "1000 × 700 px"), "the map area");
        // A metre a unit to start with: 200 m wide, 100 m long, 3 m high.
        assert!(has(&h, "1 m per pixel") && has(&h, "60000 m³"));
        // Stretched over the map area (1000 units wide), the same 200 m make 0.2 m a unit.
        h.get_by_role_and_label(Role::RadioButton, "Stretch to the map area").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.pictures.window.mode, BgMode::Fit);
        h.state_mut().ui.pictures.window.touched = true;
        h.state_mut().ui.pictures.window.width_m = 500.0;
        h.run_steps(3);
        assert!(has(&h, "0.5 m per pixel"), "500 m over 1000 units");
    }

    #[test]
    fn apply_is_one_undo_step_and_reset_goes_back_to_the_lab() {
        let mut h = harness();
        h.state_mut().set_physical(true);
        let c = closet(&h);
        h.state_mut().open_pictures(false, c);
        h.run_steps(3);
        browse(&mut h, 400, 300);
        h.get_by_role_and_label(Role::RadioButton, "Tiled").click();
        h.run_steps(3);
        // Reset forgets the draft.
        click(&mut h, "Reset");
        assert_eq!(h.state().ui.pictures.window.mode, BgMode::Original);
        assert!(has(&h, "No picture"));
        browse(&mut h, 400, 300);
        h.get_by_role_and_label(Role::RadioButton, "Tiled").click();
        h.run_steps(3);
        h.state_mut().ui.pictures.window.touched = true;
        h.state_mut().ui.pictures.window.width_m = 2000.0;
        h.run_steps(3);
        let steps_before = h.state().session.topology.clone();
        click(&mut h, "Apply");
        let t = h.state().session.topology.clone();
        let scene = t.scene(c).expect("a scene");
        assert_eq!(scene.background.as_ref().map(|b| b.mode), Some(BgMode::Tiled));
        assert_eq!(scene.size.map(|z| z.scale), Some(5.0), "2000 m over 400 units");
        assert_eq!(t.images.0.len(), 1);
        assert_eq!(t.map_scale(c), 5.0);
        // One undo step takes all of it back.
        h.state_mut().run("edit.undo", json!({}));
        assert_eq!(h.state().session.topology, steps_before);
        // The window shows what the lab has after Apply.
        h.state_mut().run("edit.redo", json!({}));
        h.state_mut().open_pictures(false, c);
        h.run_steps(3);
        assert!(has(&h, "400 × 300 px") && has(&h, "5 m per pixel"));
    }

    #[test]
    fn the_logical_workspace_takes_a_tiled_background_and_remove_picture_clears_it() {
        let mut h = harness();
        h.state_mut().open_background_window();
        h.run_steps(3);
        browse(&mut h, 120, 80);
        h.get_by_role_and_label(Role::RadioButton, "Tiled").click();
        h.run_steps(3);
        click(&mut h, "Apply");
        let t = h.state().session.topology.clone();
        assert_eq!(t.background.as_ref().map(|b| b.mode), Some(BgMode::Tiled));
        assert!(t.physical.scenes.is_empty());
        click(&mut h, "Remove Picture");
        click(&mut h, "Apply");
        assert!(h.state().session.topology.background.is_none() && h.state().session.topology.images.is_default());
    }

    #[test]
    fn a_picture_that_is_not_one_is_refused_in_the_status_line() {
        let mut h = harness();
        h.state_mut().open_background_window();
        h.run_steps(3);
        click(&mut h, "Browse…");
        h.state_mut().take_requests();
        h.state_mut().image_picked(b"not a picture".to_vec());
        h.run_steps(2);
        assert!(h.state().ui.status.as_ref().is_some_and(|s| s.1));
        assert!(has(&h, "No picture"));
        // Bytes nobody asked for are ignored.
        h.state_mut().image_picked(pattern_png(10, 10));
        h.run_steps(2);
        assert!(has(&h, "No picture"));
    }

    #[test]
    fn image_cleanup_lists_the_pictures_and_removing_one_is_one_undo_step() {
        let mut h = harness();
        h.state_mut().set_physical(true);
        let c = closet(&h);
        let data = netlabcraft_model::backdrop::base64_encode(&pattern_png(64, 48));
        h.state_mut().run("place.scene.set", json!({"place": c, "background": {"data": data, "mode": "fit"}, "icon": {"data": data}}));
        h.state_mut().run("workspace.background.set", json!({"data": data, "mode": "tiled"}));
        h.state_mut().open_preferences();
        h.run_steps(2);
        h.get_by_label("Image Cleanup").click();
        h.run_steps(3);
        assert!(has(&h, "Workspace background"));
        assert!(h.query_by_label_contains("Background of").is_some() && h.query_by_label_contains("Icon of").is_some());
        assert_eq!(h.query_all_by_role_and_label(Role::Button, "Remove").count(), 3);
        assert!(h.query_by_label_contains("The lab holds no images").is_none());
        let before = h.state().session.topology.clone();
        h.query_all_by_role_and_label(Role::Button, "Remove").next().unwrap().click();
        h.run_steps(3);
        assert_eq!(h.state().session.topology.image_uses().len(), 2);
        assert_eq!(h.state().session.topology.images.0.len(), 1, "the picture stays while two places use it");
        h.state_mut().run("edit.undo", json!({}));
        assert_eq!(h.state().session.topology, before);
    }
}
