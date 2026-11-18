// UI harness tests for the `+` menu and the Home button of the physical bar.
#[cfg(test)]
mod t {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use serde_json::json;

    use crate::PacketApp;
    use crate::physical_bar::{Create, items};
    use netlabcraft_model::furniture::FurnitureKind;

    /// The `lan` sample (one city, one building, one closet) in the physical view, Intercity map.
    fn lab() -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "lan"})).unwrap();
        s.execute("view.intercity", json!({})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.run_steps(3);
        h
    }

    fn go(h: &mut Harness<'static, PacketApp>, id: Option<u32>) {
        match id {
            Some(id) => h.state_mut().run("view.enter_place", json!({"id": id})),
            None => h.state_mut().run("view.intercity", json!({})),
        };
        h.run_steps(3);
    }

    fn key(h: &mut Harness<'static, PacketApp>, key: egui::Key, modifiers: egui::Modifiers) {
        h.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        h.run_steps(2);
    }

    fn undo(h: &mut Harness<'static, PacketApp>) {
        h.state_mut().run("edit.undo", json!({}));
        h.run_steps(2);
    }

    fn places(h: &Harness<'static, PacketApp>) -> usize {
        h.state().session.topology.places().len()
    }

    #[test]
    fn the_menu_offers_what_each_level_can_create() {
        use Create::*;
        use FurnitureKind::*;
        let furniture =
            vec![Furniture(Rack), Furniture(Table), Furniture(Shelf), Furniture(InventoryShelf), Furniture(CablePegboard), Furniture(ControlPanel)];
        for dark in [false, true] {
            let mut h = lab();
            h.state_mut().ui.dark = dark;
            // A container to look inside, found by making one in the city.
            go(&mut h, Some(1));
            h.state_mut().run("place.add", json!({"kind": "container", "name": "Store"}));
            let store = h.state().session.topology.places().iter().find(|p| p.name == "Store").map(|p| p.id).unwrap();
            let levels: Vec<(Option<u32>, Vec<Create>)> = vec![
                (None, vec![City]),
                (Some(1), vec![Building, Container]),
                (Some(2), vec![Closet, Container]),
                (Some(3), furniture.clone()),
                (Some(store), furniture.clone()),
            ];
            for (at, want) in levels {
                go(&mut h, at);
                assert_eq!(items(h.state()), want, "at {at:?}");
                h.get_by_label("Create New…").click();
                h.run_steps(2);
                for what in Create::ALL {
                    assert_eq!(h.query_by_label(what.label()).is_some(), want.contains(&what), "{} at {at:?}", what.label());
                }
                key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
                assert!(!h.state().ui.physical.menu.open, "Esc closes the menu");
            }
        }
    }

    #[test]
    fn each_item_is_one_undo_step() {
        let mut h = lab();
        let n = places(&h);
        h.get_by_label("Create New…").click();
        h.run_steps(2);
        h.get_by_label("Create New City").click();
        h.run_steps(2);
        assert_eq!(places(&h), n + 1);
        assert!(!h.state().ui.physical.menu.open, "choosing closes the menu");
        undo(&mut h);
        assert_eq!(places(&h), n);
        go(&mut h, Some(1));
        h.get_by_label("Create New…").click();
        h.run_steps(2);
        h.get_by_label("Create New Generic Container").click();
        h.run_steps(2);
        let made = h.state().session.topology.places().iter().find(|p| p.kind == netlabcraft_model::physical::PlaceKind::Container).cloned().unwrap();
        assert_eq!(made.parent, Some(1));
        undo(&mut h);
        assert_eq!(places(&h), n);
        // Furniture in the closet.
        go(&mut h, Some(3));
        for (label, kind) in [
            ("Create New Inventory Shelf", FurnitureKind::InventoryShelf),
            ("Create New Cable Pegboard", FurnitureKind::CablePegboard),
            ("Create New Industrial Control Panel (ICP)", FurnitureKind::ControlPanel),
        ] {
            let before = h.state().session.topology.physical.furniture.len();
            h.get_by_label("Create New…").click();
            h.run_steps(2);
            h.get_by_label(label).click();
            h.run_steps(2);
            let f = h.state().session.topology.physical.furniture.last().cloned().unwrap();
            assert_eq!((h.state().session.topology.physical.furniture.len(), f.kind), (before + 1, kind));
            undo(&mut h);
            assert_eq!(h.state().session.topology.physical.furniture.len(), before, "{label} is one step");
        }
    }

    #[test]
    fn the_keyboard_opens_moves_chooses_and_closes() {
        let mut h = lab();
        go(&mut h, Some(3));
        h.get_by_label("Create New…").focus();
        h.run_steps(2);
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        assert!(h.state().ui.physical.menu.open, "Enter opens it");
        key(&mut h, egui::Key::ArrowDown, egui::Modifiers::NONE);
        assert_eq!(h.state().ui.physical.menu.sel, 1);
        key(&mut h, egui::Key::ArrowUp, egui::Modifiers::NONE);
        key(&mut h, egui::Key::ArrowUp, egui::Modifiers::NONE);
        assert_eq!(h.state().ui.physical.menu.sel, 5, "it wraps round");
        key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!h.state().ui.physical.menu.open, "Esc closes it");
        assert!(h.state().session.topology.physical.furniture.is_empty());
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        key(&mut h, egui::Key::ArrowDown, egui::Modifiers::NONE);
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        let made = h.state().session.topology.physical.furniture.clone();
        assert_eq!(made.len(), 1, "Enter chooses");
        assert_eq!(made[0].kind, FurnitureKind::Table, "the second item");
        assert!(!h.state().ui.physical.menu.open);
    }

    #[test]
    fn a_shortcut_at_the_wrong_level_does_nothing_and_says_why() {
        let mut h = lab();
        let n = places(&h);
        // Intercity: only a city.
        for k in [egui::Key::B, egui::Key::J, egui::Key::F] {
            key(&mut h, k, egui::Modifiers::SHIFT);
            assert_eq!(places(&h), n, "{k:?}");
            assert!(h.state().ui.status.as_ref().is_some_and(|s| s.1), "{k:?} says why");
        }
        key(&mut h, egui::Key::C, egui::Modifiers::SHIFT);
        assert_eq!(places(&h), n + 1);
        // In a city: a building and a container, never a city or a closet.
        go(&mut h, Some(1));
        let n = places(&h);
        key(&mut h, egui::Key::C, egui::Modifiers::SHIFT);
        key(&mut h, egui::Key::J, egui::Modifiers::SHIFT);
        assert_eq!(places(&h), n);
        key(&mut h, egui::Key::B, egui::Modifiers::SHIFT);
        key(&mut h, egui::Key::F, egui::Modifiers::SHIFT);
        assert_eq!(places(&h), n + 2);
        // In a building: a closet.
        go(&mut h, Some(2));
        let n = places(&h);
        key(&mut h, egui::Key::J, egui::Modifiers::SHIFT);
        assert_eq!(places(&h), n + 1);
        // In the logical view nothing is made.
        h.get_by_label("Logical").click();
        h.run_steps(2);
        let n = places(&h);
        key(&mut h, egui::Key::C, egui::Modifiers::SHIFT);
        assert_eq!(places(&h), n);
    }

    #[test]
    fn home_goes_to_the_working_closet_from_anywhere() {
        let mut h = lab();
        assert_eq!(h.state().session.place_view, None);
        h.get_by_label("Go to Working Closet").click();
        h.run_steps(3);
        assert_eq!(h.state().session.place_view, Some(3), "from the Intercity map");
        // A second closet marked as the working one; the mark is one undo step.
        go(&mut h, Some(2));
        h.state_mut().run("place.add", json!({"kind": "closet", "name": "Lab"}));
        let lab = h.state().session.topology.places().iter().find(|p| p.name == "Lab").map(|p| p.id).unwrap();
        h.state_mut().run("place.working.set", json!({"id": lab}));
        go(&mut h, None);
        key(&mut h, egui::Key::H, egui::Modifiers::SHIFT);
        assert_eq!(h.state().session.place_view, Some(lab), "Shift+H");
        undo(&mut h);
        assert_eq!(h.state().session.topology.working_closet(), Some(3));
        // From the logical workspace it switches to Physical first.
        h.get_by_label("Logical").click();
        h.run_steps(2);
        assert!(!h.state().session.physical);
        key(&mut h, egui::Key::H, egui::Modifiers::SHIFT);
        assert!(h.state().session.physical);
        assert_eq!(h.state().session.place_view, Some(3));
        // Without a closet the button is disabled and the shortcut says so.
        let ids: Vec<u32> =
            h.state().session.topology.places().iter().filter(|p| p.kind == netlabcraft_model::physical::PlaceKind::Closet).map(|p| p.id).collect();
        for id in ids {
            h.state_mut().run("place.remove", json!({"id": id}));
        }
        go(&mut h, None);
        assert_eq!(h.state().session.topology.working_closet(), None);
        h.get_by_label("Go to Working Closet").click();
        h.run_steps(2);
        assert_eq!(h.state().session.place_view, None);
        key(&mut h, egui::Key::H, egui::Modifiers::SHIFT);
        assert_eq!(h.state().session.place_view, None);
        assert!(h.state().ui.status.as_ref().is_some_and(|s| s.0.contains("No wiring closet yet")));
    }
}
