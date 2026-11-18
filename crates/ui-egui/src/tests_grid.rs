// Tests for the physical workspace's grid: the settings, the lines, snapping and the window.
#[cfg(test)]
mod t {
    use egui::{Rect, Vec2, pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use serde_json::json;

    use crate::PacketApp;
    use crate::physical_grid::{self as g, GridLevel, GridPrefs};

    fn harness() -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "lan"})).unwrap();
        Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s))
    }

    fn physical() -> Harness<'static, PacketApp> {
        let mut h = harness();
        h.run_steps(2);
        h.get_by_label("Physical").click();
        h.run_steps(2);
        h
    }

    fn key(h: &mut Harness<'static, PacketApp>, key: egui::Key, modifiers: egui::Modifiers) {
        h.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        h.run_steps(2);
    }

    #[test]
    fn the_defaults_are_off_with_the_level_spacings() {
        let p = GridPrefs::default();
        for l in GridLevel::ALL {
            assert!(!p.on(l));
        }
        assert_eq!(p.spacing(GridLevel::Intercity), (2000.0, 2000.0));
        assert_eq!(p.spacing(GridLevel::City), (200.0, 200.0));
        assert_eq!(p.spacing(GridLevel::Building), (20.0, 20.0));
        assert_eq!(p.spacing(GridLevel::Container), (1.0, 1.0));
        assert_eq!(p.color, g::DEFAULT_COLOR);
    }

    #[test]
    fn a_spacing_must_be_positive_and_finite_and_is_kept_in_bounds() {
        let mut p = GridPrefs::default();
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -3.0] {
            p.set_spacing(GridLevel::City, bad, bad);
            assert_eq!(p.spacing(GridLevel::City), (200.0, 200.0), "{bad} is refused");
        }
        p.set_spacing(GridLevel::City, 1e-9, 1e30);
        assert_eq!(p.spacing(GridLevel::City), (g::MIN_SPACING_M, g::MAX_SPACING_M));
        p.set_spacing(GridLevel::City, 12.5, f32::NAN);
        assert_eq!(p.spacing(GridLevel::City), (12.5, g::MAX_SPACING_M));
    }

    #[test]
    fn lines_stand_a_spacing_apart_inside_the_view() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0));
        let l = g::lines(rect, Vec2::ZERO, 1.0, (200.0, 100.0), 1.0);
        assert!(!l.dense);
        assert_eq!(l.xs, vec![0.0, 200.0, 400.0, 600.0, 800.0]);
        assert_eq!(l.ys.len(), 8);
        // The origin moves the lines; the zoom and the metres per unit scale them.
        let l = g::lines(rect, vec2(50.0, 0.0), 2.0, (100.0, 100.0), 1.0);
        assert_eq!(l.xs, vec![50.0, 250.0, 450.0, 650.0, 850.0]);
        let l = g::lines(rect, Vec2::ZERO, 2.0, (100.0, 100.0), 100.0);
        assert!(l.dense, "2 px apart is dust");
        assert!(l.xs.is_empty() && l.ys.is_empty());
    }

    #[test]
    fn a_tiny_spacing_draws_no_lines_and_every_input_is_survived() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(4000.0, 3000.0));
        let l = g::lines(rect, Vec2::ZERO, 1.0, (g::MIN_SPACING_M, g::MIN_SPACING_M), 1.0);
        assert!(l.dense && l.xs.is_empty() && l.ys.is_empty());
        // Just coarse enough, but still more than the cap across a huge view.
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(100_000.0, 100_000.0));
        let l = g::lines(wide, Vec2::ZERO, 1.0, (10.0, 10.0), 1.0);
        assert!(l.dense && l.xs.len() <= g::MAX_LINES && l.ys.len() <= g::MAX_LINES);
        for (zoom, scale, s) in
            [(0.0, 1.0, 5.0), (f32::NAN, 1.0, 5.0), (1.0, 0.0, 5.0), (1.0, -1.0, 5.0), (1.0, 1.0, f32::NAN), (1e-30, 1e-30, 1e30), (1e30, 1.0, 1e-30)]
        {
            let l = g::lines(rect, vec2(f32::NAN, 0.0), zoom, (s, s), scale);
            assert!(l.xs.len() <= g::MAX_LINES && l.ys.len() <= g::MAX_LINES);
            let l = g::lines(rect, vec2(1e30, -1e30), zoom, (s, s), scale);
            assert!(l.xs.len() <= g::MAX_LINES && l.ys.len() <= g::MAX_LINES);
        }
        // A view that is not there.
        let none = Rect::from_min_size(pos2(0.0, 0.0), Vec2::ZERO);
        assert_eq!(g::lines(none, Vec2::ZERO, 1.0, (20.0, 20.0), 1.0), g::Lines::default());
    }

    #[test]
    fn snapping_goes_to_the_nearest_multiple() {
        assert_eq!(g::snap_to(130.0, 200.0, 1.0), 200.0);
        assert_eq!(g::snap_to(90.0, 200.0, 1.0), 0.0);
        assert_eq!(g::snap_to(-130.0, 200.0, 1.0), -200.0);
        // 2000 m on a map of 100 m units is 20 units.
        assert_eq!(g::snap_to(33.0, 2000.0, 100.0), 40.0);
        assert_eq!(g::snap_to(5.0, 0.0, 1.0), 5.0);
        assert!(g::snap_to(f32::NAN, 20.0, 1.0).is_nan());
    }

    #[test]
    fn aligned_objects_snap_to_the_grid_of_the_level_shown() {
        let mut h = physical();
        let app = h.state_mut();
        // Not aligned: where it is.
        assert_eq!(g::snap_xy(app, 33.0, 47.0), (33.0, 47.0));
        app.ui.prefs.align_physical = true;
        // Aligned, grid not ticked: the old half-grid.
        assert_eq!(g::snap_xy(app, 33.0, 47.0), (40.0, 40.0));
        app.ui.prefs.grid.set_on(GridLevel::Intercity, true);
        assert_eq!(g::snap_xy(app, 33.0, 47.0), (40.0, 40.0));
        app.ui.prefs.grid.set_spacing(GridLevel::Intercity, 1000.0, 500.0);
        assert_eq!(g::snap_xy(app, 33.0, 47.0), (30.0, 45.0));
        // A city shows its buildings in metres of one unit each.
        app.run("view.enter_place", json!({"id": 1}));
        app.ui.prefs.grid.set_on(GridLevel::City, true);
        assert_eq!(g::snap_xy(app, 330.0, 470.0), (400.0, 400.0));
    }

    #[test]
    fn the_button_opens_the_window_and_ok_closes_it() {
        let mut h = physical();
        assert!(!h.state().ui.physical.grid.open);
        h.get_by_label("Grid Preferences").click();
        h.run_steps(3);
        assert!(h.state().ui.physical.grid.open);
        for label in
            ["Grid Scale", "Generic Container", "Grid Line Color", "Select Grid Color", "OK", "A wiring closet has no scale, so it has no grid."]
        {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
        assert!(h.query_all_by_label("Grid-X (m)").count() >= 1);
        for level in ["City Grid-X (m)", "Building Grid-Y (m)", "Generic Container Grid-X (m)"] {
            assert!(h.query_by_label(level).is_some(), "{level}");
        }
        h.get_by_label("OK").click();
        h.run_steps(3);
        assert!(!h.state().ui.physical.grid.open);
    }

    #[test]
    fn ticking_a_level_draws_its_lines_and_unticking_stops_them() {
        let mut h = physical();
        h.get_by_label("Grid Preferences").click();
        h.run_steps(3);
        let boxes: Vec<_> = h.get_all_by_role(egui::accesskit::Role::CheckBox).collect();
        assert_eq!(boxes.len(), 4, "one per level");
        boxes[0].click();
        h.run_steps(3);
        assert!(h.state().ui.prefs.grid.on(GridLevel::Intercity));
        assert!(!h.state().ui.prefs.grid.on(GridLevel::City));
        let app = h.state();
        let rect = app.ui.canvas_rect.unwrap();
        let l = g::lines(
            rect,
            rect.min.to_vec2() + app.ui.pan,
            app.ui.zoom,
            app.ui.prefs.grid.spacing(GridLevel::Intercity),
            g::metres_per_unit(app, GridLevel::Intercity),
        );
        assert!(!l.xs.is_empty() || l.dense, "the Intercity grid has lines to draw or says it is too dense");
        let boxes: Vec<_> = h.get_all_by_role(egui::accesskit::Role::CheckBox).collect();
        boxes[0].click();
        h.run_steps(3);
        assert!(!h.state().ui.prefs.grid.on(GridLevel::Intercity));
    }

    #[test]
    fn the_colour_is_picked_from_the_palette() {
        let mut h = physical();
        h.get_by_label("Grid Preferences").click();
        h.run_steps(3);
        assert!(h.query_by_label("Grid color #c8ccd2").is_none(), "the palette waits for the button");
        h.get_by_label("Select Grid Color").click();
        h.run_steps(3);
        h.get_by_label("Grid color #c8ccd2").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.prefs.grid.color, [0xc8, 0xcc, 0xd2]);
        assert!(h.query_by_label("Red").is_some() && h.query_by_label("Green").is_some() && h.query_by_label("Blue").is_some());
    }

    #[test]
    fn the_grid_renders_at_every_level_in_both_themes() {
        for dark in [false, true] {
            let mut h = physical();
            h.state_mut().ui.dark = dark;
            for l in GridLevel::ALL {
                h.state_mut().ui.prefs.grid.set_on(l, true);
            }
            h.run_steps(2);
            h.state_mut().run("view.enter_place", json!({"id": 1}));
            h.run_steps(2);
            h.state_mut().run("view.enter_place", json!({"id": 2}));
            h.run_steps(2);
            h.state_mut().run("view.enter_place", json!({"id": 3}));
            h.run_steps(2);
        }
    }

    #[test]
    fn shift_g_toggles_the_level_shown_and_keeps_the_logical_grid_apart() {
        let mut h = physical();
        key(&mut h, egui::Key::G, egui::Modifiers::SHIFT);
        assert!(h.state().ui.prefs.grid.on(GridLevel::Intercity));
        assert!(!h.state().ui.show_grid, "the logical grid is not touched");
        key(&mut h, egui::Key::G, egui::Modifiers::SHIFT);
        assert!(!h.state().ui.prefs.grid.on(GridLevel::Intercity));
        // Inside a closet there is no scale: nothing changes and the status says why.
        h.state_mut().run("view.enter_place", json!({"id": 3}));
        h.run_steps(2);
        key(&mut h, egui::Key::G, egui::Modifiers::SHIFT);
        assert_eq!(h.state().ui.prefs.grid, GridPrefs::default());
        let status = h.state().ui.status.clone().map(|s| s.0);
        assert_eq!(status.as_deref(), Some("A wiring closet has no scale, so it has no grid."));
        // On the logical workspace the old grid toggles, the physical settings stay.
        h.state_mut().set_physical(false);
        h.run_steps(2);
        key(&mut h, egui::Key::G, egui::Modifiers::SHIFT);
        assert!(h.state().ui.show_grid);
        assert_eq!(h.state().ui.prefs.grid, GridPrefs::default());
    }

    #[test]
    fn the_settings_survive_a_restart_as_text() {
        let mut p = GridPrefs::default();
        p.set_on(GridLevel::City, true);
        p.set_spacing(GridLevel::Building, 12.5, 7.0);
        p.color = [1, 2, 3];
        let mut q = GridPrefs::default();
        for line in p.to_text().lines() {
            let (k, v) = line.split_once('=').unwrap();
            assert!(q.apply(k, v), "{k}");
        }
        assert_eq!(p, q);
        // Through the whole preferences file, and an old file without grid lines still loads.
        let mut h = harness();
        h.state_mut().ui.prefs.grid = p.clone();
        let text = h.state().preferences_text();
        let mut fresh = harness();
        fresh.state_mut().load_preferences(&text);
        assert_eq!(fresh.state().ui.prefs.grid, p);
        assert_eq!(fresh.state().preferences_text(), text);
        let mut old = harness();
        old.state_mut().load_preferences("align-physical=on\nfont=110\n");
        assert_eq!(old.state().ui.prefs.grid, GridPrefs::default());
        assert!(old.state().ui.prefs.align_physical);
    }

    #[test]
    fn hostile_values_change_nothing_or_are_brought_in_bounds() {
        let mut p = GridPrefs::default();
        for (k, v) in [
            ("grid-city-x", "NaN"),
            ("grid-city-x", "inf"),
            ("grid-city-y", "-5"),
            ("grid-city-y", "0"),
            ("grid-city-y", "abc"),
            ("grid-city", "maybe"),
            ("grid-color", "zzzzzz"),
            ("grid-color", "#12345"),
            ("grid-color", "#1234567"),
            ("grid-color", "#é2345"),
            ("grid-city-xx", "3"),
            ("grid-citys", "on"),
            ("grid-", "1"),
            ("grid-bogus", "1"),
        ] {
            p.apply(k, v);
            assert_eq!(p, GridPrefs::default(), "{k}={v}");
        }
        assert!(p.apply("grid-building-x", "1e30"));
        assert_eq!(p.spacing(GridLevel::Building).0, g::MAX_SPACING_M);
        assert!(p.apply("grid-building-x", "0.000001"));
        assert_eq!(p.spacing(GridLevel::Building).0, g::MIN_SPACING_M);
        assert!(!p.apply("grid", "on"));
        assert!(!p.apply("align", "on"));
    }

    #[test]
    fn the_button_sits_on_the_physical_bar_only() {
        let mut h = harness();
        h.run_steps(2);
        assert!(h.query_by_label("Grid Preferences").is_none(), "not on the logical bar");
        h.get_by_label("Physical").click();
        h.run_steps(2);
        let b = h.get_by_label("Grid Preferences").rect();
        assert!(b.center().x > h.get_by_label("Physical").rect().right() && b.center().x > 600.0, "{b:?}");
    }

    fn sized(scale: f32) -> Option<netlabcraft_model::backdrop::Size> {
        Some(netlabcraft_model::backdrop::Size { width: 100.0, length: 100.0, height: 3.0, scale })
    }

    fn enter(h: &mut Harness<'static, PacketApp>, id: Option<u32>) {
        match id {
            Some(id) => h.state_mut().run("view.enter_place", json!({"id": id})),
            None => h.state_mut().run("view.intercity", json!({})),
        };
        h.run_steps(3);
    }

    #[test]
    fn the_metres_of_a_unit_follow_the_scale_set_on_the_place() {
        let mut h = physical();
        enter(&mut h, None);
        assert_eq!(g::metres_per_unit(h.state(), GridLevel::Intercity), 100.0, "the built-in unit");
        h.state_mut().session.topology.set_place_size(netlabcraft_model::backdrop::INTERCITY, sized(250.0)).unwrap();
        assert_eq!(g::metres_per_unit(h.state(), GridLevel::Intercity), 250.0);
        enter(&mut h, Some(1));
        assert_eq!(g::current_level(h.state()), Some(GridLevel::City));
        assert_eq!(g::metres_per_unit(h.state(), GridLevel::City), 1.0, "a city without a size keeps 1 m a unit");
        h.state_mut().session.topology.set_place_size(1, sized(10.0)).unwrap();
        assert_eq!(g::metres_per_unit(h.state(), GridLevel::City), 10.0);
        // The same spacing is ten times closer on screen once a unit stands for ten metres.
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
        let near = g::lines(rect, Vec2::ZERO, 1.0, (200.0, 200.0), 1.0);
        let far = g::lines(rect, Vec2::ZERO, 1.0, (200.0, 200.0), 10.0);
        assert!(far.xs.len() > near.xs.len());
        enter(&mut h, Some(2));
        assert_eq!(g::metres_per_unit(h.state(), GridLevel::Building), 1.0, "another place keeps its own scale");
    }

    #[test]
    fn a_container_has_a_level_and_a_closet_has_none() {
        let mut h = physical();
        let c = h.state_mut().session.topology.add_place(netlabcraft_model::physical::PlaceKind::Container, None, Some(1), 0.0, 0.0).unwrap();
        enter(&mut h, Some(3));
        assert_eq!(g::current_level(h.state()), None, "a wiring closet has no scale");
        enter(&mut h, Some(c));
        assert_eq!(g::current_level(h.state()), Some(GridLevel::Container));
        assert_eq!(g::metres_per_unit(h.state(), GridLevel::Container), 1.0);
        h.state_mut().session.topology.set_place_size(c, sized(0.5)).unwrap();
        assert_eq!(g::metres_per_unit(h.state(), GridLevel::Container), 0.5);
        // Shift+G turns the container's grid on, and the grid is drawn in its view.
        key(&mut h, egui::Key::G, egui::Modifiers::SHIFT);
        assert!(h.state().ui.prefs.grid.on(GridLevel::Container));
        assert!(g::showing(h.state()));
        assert!(!h.state().ui.prefs.grid.on(GridLevel::City), "only its own level");
        key(&mut h, egui::Key::G, egui::Modifiers::SHIFT);
        assert!(!g::showing(h.state()));
    }

    #[test]
    fn creating_and_going_home_still_work_inside_a_container() {
        use crate::physical_bar::{Create, reason};
        use netlabcraft_model::furniture::FurnitureKind;
        let mut h = physical();
        let c = h.state_mut().session.topology.add_place(netlabcraft_model::physical::PlaceKind::Container, None, Some(1), 0.0, 0.0).unwrap();
        enter(&mut h, Some(c));
        assert!(reason(h.state(), Create::Furniture(FurnitureKind::Rack)).is_none(), "furniture stands in a container");
        assert!(reason(h.state(), Create::Container).is_some(), "a container holds no places");
        h.get_by_label("Go to Working Closet").click();
        h.run_steps(3);
        assert_eq!(h.state().session.place_view, h.state().session.topology.working_closet());
    }
}
