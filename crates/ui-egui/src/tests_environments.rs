// Options > Environments (Shift+E): View mode, Edit mode and its two tabs.
#[cfg(test)]
mod t {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use netlabcraft_engine::Session;
    use netlabcraft_model::environments::Key;
    use serde_json::json;

    use crate::environments::{Tab, current_place, fmt, graph_point, near_key, place_items};
    use crate::{PacketApp, UiRequest};

    fn harness(sample: &str) -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1280.0, 900.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.show_notes = false;
        h
    }

    fn has(h: &Harness<'static, PacketApp>, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    fn click(h: &mut Harness<'static, PacketApp>, role: Role, label: &str) {
        h.get_by_role_and_label(role, label).click();
        h.run_steps(3);
    }

    fn own(h: &Harness<'static, PacketApp>, id: &str) -> Vec<Key> {
        let t = &h.state().session.topology;
        t.place_env(current_place(h.state())).and_then(|e| e.setting(id)).map(|s| s.keys.clone()).unwrap_or_default()
    }

    #[test]
    fn shift_e_is_in_the_table_and_the_window_opens_on_view_mode() {
        assert_eq!(crate::shortcuts::text(crate::shortcuts::Act::Environments), "Shift+E");
        let mut h = harness("iot");
        assert!(!h.state().ui.env_ui.open);
        h.state_mut().open_environments();
        h.run_steps(3);
        for label in ["Location:", "Current Time:", "Filter:", "08:00:00", "Gravity", "Always on Top"] {
            assert!(has(&h, label), "{label}");
        }
        assert!(!has(&h, "Simulation Time Scale:"), "that is Edit mode");
        // The defaults are ordinary constants.
        assert!(has(&h, "9.80665 m/s2"));
        assert!(has(&h, "101.325 kPa"));
        assert_eq!(fmt(0.000_055), "0.000055");
        assert_eq!(fmt(12_345.6), "12346");
    }

    #[test]
    fn view_mode_shows_what_the_place_sets_and_what_it_inherits() {
        let mut h = harness("iot");
        h.state_mut().run("environment.set", json!({"place": "intercity", "variable": "gravity", "value": 1.62}));
        h.state_mut().run("environment.set", json!({"variable": "humidity", "value": 72}));
        h.state_mut().open_environments();
        h.run_steps(3);
        assert!(has(&h, "1.62 m/s2"));
        assert!(has(&h, "72 %"));
        assert!(has(&h, "set here"));
        assert!(h.query_all_by_label("from Intercity").next().is_some());
        // Another place reads its own surroundings.
        h.state_mut().ui.env_ui.place = Some(0);
        h.run_steps(3);
        assert!(!has(&h, "72 %"));
        // A place that is gone falls back to the default one.
        h.state_mut().ui.env_ui.place = Some(9_999);
        assert_eq!(current_place(h.state()), h.state().session.topology.default_place());
    }

    #[test]
    fn pause_and_resume_run_the_clock_command_and_the_time_follows_the_simulation() {
        let mut h = harness("iot");
        h.state_mut().open_environments();
        h.run_steps(3);
        click(&mut h, Role::Button, "Pause");
        assert!(h.state().session.topology.environments.clock.paused());
        assert!(has(&h, "Resume"));
        click(&mut h, Role::Button, "Resume");
        assert!(!h.state().session.topology.environments.clock.paused());
        h.state_mut().run("environment.clock", json!({"time": "13:45:10"}));
        h.run_steps(3);
        assert!(has(&h, "13:45:10"));
        assert!(h.state_mut().session.can_undo());
    }

    #[test]
    fn search_filters_the_values_and_reset_brings_them_back() {
        let mut h = harness("iot");
        h.state_mut().open_environments();
        h.state_mut().ui.env_ui.filter = "wind".into();
        h.run_steps(2);
        assert!(has(&h, "Gravity"), "nothing is applied until Search");
        click(&mut h, Role::Button, "Search");
        assert!(has(&h, "Wind speed") && !has(&h, "Gravity"));
        h.state_mut().ui.env_ui.filter = "zzz".into();
        click(&mut h, Role::Button, "Search");
        assert!(has(&h, "No variables match the filter."));
        click(&mut h, Role::Button, "Reset");
        assert!(has(&h, "Gravity"));
    }

    #[test]
    fn the_edit_button_opens_edit_mode_with_the_time_scale_and_its_two_tabs() {
        let mut h = harness("iot");
        h.state_mut().open_environments();
        h.run_steps(3);
        assert_eq!(h.query_all_by_label("Edit").count(), 3, "the menu's, the place's and the clock's");
        h.query_all_by_label("Edit").nth(1).unwrap().click();
        h.run_steps(3);
        assert!(h.state().ui.env_ui.edit);
        for label in
            ["Simulation Time Scale:", "minute(s) real equals", "simulated minute(s)", "Keyframe Graph", "Advanced", "Time Snap:", "Value Snap:"]
        {
            assert!(has(&h, label), "{label}");
        }
        h.get_by_label("Advanced").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.env_ui.tab, Tab::Advanced);
        for label in
            ["Time Line:", "Time Increment:", "Add Keyframe", "Export File…", "Import File…", "Show Notes in View Mode", "Remove Custom Environment"]
        {
            assert!(has(&h, label), "{label}");
        }
        click(&mut h, Role::Button, "View Mode");
        assert!(!h.state().ui.env_ui.edit);
    }

    #[test]
    fn clicking_a_legend_name_shows_and_selects_the_variable() {
        let mut h = harness("iot");
        h.state_mut().open_environments();
        h.state_mut().ui.env_ui.edit = true;
        h.run_steps(3);
        assert!(!has(&h, "Pick a variable on the left"), "painted text is not a node");
        h.get_by_label("Elevation").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.env_ui.selected, Some("elevation"));
        assert_eq!(h.state().ui.env_ui.shown, ["elevation"]);
        h.get_by_label("Elevation").click();
        h.run_steps(3);
        assert!(h.state().ui.env_ui.shown.is_empty() && h.state().ui.env_ui.selected.is_none());
        // Drawn in both themes without trouble, with keyframes on it.
        h.state_mut().run("environment.keyframe.add", json!({"variable": "humidity", "time": "06:00", "value": 20}));
        h.state_mut().run("environment.keyframe.add", json!({"variable": "humidity", "time": "18:00", "value": 90}));
        h.state_mut().ui.env_ui.shown = vec!["humidity", "ambient_temperature"];
        h.state_mut().ui.env_ui.selected = Some("humidity");
        for dark in [false, true] {
            h.state_mut().ui.dark = dark;
            h.run_steps(3);
        }
    }

    #[test]
    fn graph_clicks_become_snapped_keyframes_and_land_on_existing_ones() {
        let v = netlabcraft_model::environments::var_by_id("humidity").unwrap();
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 20.0), egui::pos2(100.0 + 864.0, 120.0));
        // Halfway across and halfway up, snapped to a quarter of an hour and to whole units.
        assert_eq!(graph_point(rect, egui::pos2(100.0 + 432.0, 70.0), v, 900, 1.0, &[]), (43_200, 50.0));
        // A click at 12:07 snaps to 12:00; one at 12:08 to 12:15.
        let at = |minutes: f32| egui::pos2(100.0 + 864.0 * minutes / 1_440.0, 70.0);
        assert_eq!(graph_point(rect, at(727.0), v, 900, 0.0, &[]).0, 43_200);
        assert_eq!(graph_point(rect, at(728.0), v, 900, 0.0, &[]).0, 44_100);
        // Without snaps the value is the pixel's; outside the graph it clamps to the range.
        let (_, value) = graph_point(rect, egui::pos2(100.0, 20.0 + 25.0), v, 0, 0.0, &[]);
        assert!((value - 75.0).abs() < 1e-6);
        assert_eq!(graph_point(rect, egui::pos2(5_000.0, -50.0), v, 0, 0.0, &[]), (86_399, 100.0));
        // Near an existing keyframe the click is that keyframe's time (to edit its value).
        let keys = [Key { t: 21_600, v: 10.0 }];
        let x = 100.0 + 864.0 * 0.25 + 3.0;
        assert_eq!(near_key(rect, &keys, x), Some(21_600));
        assert_eq!(near_key(rect, &keys, x + 30.0), None);
        assert_eq!(graph_point(rect, egui::pos2(x, 70.0), v, 900, 1.0, &keys).0, 21_600);
    }

    #[test]
    fn the_advanced_tab_adds_moves_and_removes_keyframes_and_exports_and_imports() {
        let mut h = harness("iot");
        h.state_mut().open_environments();
        h.state_mut().ui.env_ui.edit = true;
        h.state_mut().ui.env_ui.tab = Tab::Advanced;
        h.state_mut().ui.env_ui.selected = Some("humidity");
        h.run_steps(3);
        click(&mut h, Role::Button, "Add Keyframe");
        assert_eq!(own(&h, "humidity"), [Key { t: 0, v: 50.0 }], "the value there now, at the time line");
        // The time line moves by its increment (one minute at first).
        click(&mut h, Role::Button, "+");
        assert_eq!(h.state().ui.env_ui.key_time, 60);
        h.state_mut().ui.env_ui.key_value = Some(65.5);
        click(&mut h, Role::Button, "Add Keyframe");
        assert_eq!(own(&h, "humidity").len(), 2);
        assert_eq!(own(&h, "humidity")[1].v, 65.5);
        click(&mut h, Role::Button, "Previous");
        assert_eq!(h.state().ui.env_ui.key_time, 0);
        click(&mut h, Role::Button, "Next");
        assert_eq!(h.state().ui.env_ui.key_time, 60);
        click(&mut h, Role::Button, "Remove");
        assert_eq!(own(&h, "humidity").len(), 1);
        // Export hands the file to the shell; Import asks it to pick one.
        click(&mut h, Role::Button, "Export File…");
        let asked = h.state_mut().take_requests();
        let text = asked
            .iter()
            .find_map(|r| match r {
                UiRequest::SaveText { name, text } if name.ends_with(".environment.json") => Some(text.clone()),
                _ => None,
            })
            .expect("a file to save");
        assert!(text.contains("netlabcraft-environment") && text.contains("humidity"));
        click(&mut h, Role::Button, "Import File…");
        let place = current_place(h.state());
        assert!(h.state_mut().take_requests().iter().any(|r| matches!(r, UiRequest::ImportEnvironment { place: p } if *p == place)));
        // The shell comes back with the file's text, here for another place.
        h.state_mut().environment_imported(0, &text);
        assert!(h.state().session.topology.place_env(0).is_some_and(|e| e.setting("humidity").is_some()));
        h.state_mut().environment_imported(0, "not an environment");
        assert!(h.state().ui.status.as_ref().is_some_and(|s| s.1), "a bad file is an error in the status line");
        click(&mut h, Role::Button, "Remove Custom Environment");
        assert!(own(&h, "humidity").is_empty());
    }

    #[test]
    fn notes_and_the_active_variables_have_their_controls() {
        let mut h = harness("iot");
        h.state_mut().open_environments();
        h.state_mut().ui.env_ui.edit = true;
        h.state_mut().ui.env_ui.tab = Tab::Advanced;
        h.run_steps(3);
        click(&mut h, Role::CheckBox, "Show Notes in View Mode");
        let place = current_place(h.state());
        assert!(h.state().session.topology.place_env(place).is_some_and(|e| e.show_notes));
        h.state_mut().run("environment.set", json!({"notes": "Door left open", "show_notes": true}));
        h.state_mut().ui.env_ui.edit = false;
        h.run_steps(3);
        assert!(has(&h, "Door left open"));
        // Hide All empties View mode; Show All brings it back.
        h.state_mut().ui.env_ui.edit = true;
        h.run_steps(3);
        click(&mut h, Role::Button, "Hide All");
        assert_eq!(h.state().ui.env_ui.hidden.len(), netlabcraft_model::environments::VARS.len());
        h.state_mut().ui.env_ui.edit = false;
        h.run_steps(3);
        assert!(!has(&h, "Gravity"));
        h.state_mut().ui.env_ui.edit = true;
        h.run_steps(3);
        click(&mut h, Role::Button, "Show All");
        assert!(h.state().ui.env_ui.hidden.is_empty());
        h.state_mut().ui.env_ui.edit = false;
        h.run_steps(3);
        assert!(has(&h, "Gravity"));
        click(&mut h, Role::CheckBox, "Always on Top");
        assert!(h.state().ui.env_ui.on_top);
    }

    #[test]
    fn the_location_list_is_the_tree_of_places() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "iot"})).unwrap();
        let city = s.execute("place.add", json!({"kind": "city", "name": "Harbor"})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("place.add", json!({"kind": "building", "name": "Annex", "parent": city})).unwrap()["id"].as_u64().unwrap();
        s.execute("place.add", json!({"kind": "closet", "name": "Cold room", "parent": b})).unwrap();
        let items = place_items(&s.topology);
        assert_eq!(items[0], (0, "Intercity".to_string(), 0));
        let tail: Vec<(&str, usize)> = items.iter().rev().take(3).rev().map(|i| (i.1.as_str(), i.2)).collect();
        assert_eq!(tail, [("Harbor", 1), ("Annex", 2), ("Cold room", 3)]);
        assert!(items.iter().any(|i| i.2 == 3), "the default closet is in the tree too");
    }
}
