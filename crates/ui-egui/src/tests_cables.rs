// UI harness tests for cables as objects: select, hover, the right-click menu, the Properties window,
// the Delete tool and the keyboard.
#[cfg(test)]
mod t {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use netlabcraft_model::End;
    use serde_json::json;

    use crate::{PacketApp, Tool};

    fn lab() -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "vlans"})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.run_steps(3);
        h
    }

    fn distance(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
        let ab = b - a;
        let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
        (p - (a + ab * t)).length()
    }

    /// A point on a cable that is clear of every device and of every other cable, and the cable's first end.
    fn on_a_cable(h: &Harness<'static, PacketApp>) -> (End, egui::Pos2) {
        let app = h.state();
        let t = &app.session.topology;
        let rect = app.ui.canvas_rect.unwrap();
        let (origin, zoom) = (rect.min.to_vec2() + app.ui.pan, app.ui.zoom);
        let scr = |id| {
            let d = t.device(id).unwrap();
            egui::pos2(d.x * zoom, d.y * zoom) + origin
        };
        let segs: Vec<(End, egui::Pos2, egui::Pos2)> = t.links.iter().map(|l| (l.a, scr(l.a.device), scr(l.b.device))).collect();
        for (end, a, b) in &segs {
            let mid = *a + (*b - *a) * 0.5;
            let clear_of_devices = t.devices.iter().all(|d| (scr(d.id) - mid).length() > 48.0);
            let clear_of_cables = segs.iter().all(|(e, x, y)| e == end || distance(mid, *x, *y) > 14.0);
            if clear_of_devices && clear_of_cables && rect.contains(mid) {
                return (*end, mid);
            }
        }
        panic!("no cable with a clear middle in the lab");
    }

    fn right_click(h: &mut Harness<'static, PacketApp>, at: egui::Pos2) {
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
    }

    fn left_click(h: &mut Harness<'static, PacketApp>, at: egui::Pos2) {
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

    #[test]
    fn hovering_a_cable_lights_it_and_clicking_with_the_select_tool_picks_it() {
        let mut h = lab();
        let (end, at) = on_a_cable(&h);
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        h.run_steps(3);
        assert_eq!(h.state().ui.hover_cable, Some(end));
        left_click(&mut h, at);
        assert_eq!(h.state().ui.selected_cable, Some(end));
        assert_eq!(h.state().ui.selected, None);
        assert!(h.state().ui.cable_window.is_none(), "a click picks the cable; Properties opens its window");
    }

    #[test]
    fn the_right_click_menu_opens_properties_and_the_window_deletes_the_cable() {
        let mut h = lab();
        let (end, at) = on_a_cable(&h);
        let before = h.state().session.topology.links.len();
        right_click(&mut h, at);
        assert_eq!(h.state().ui.canvas.context_cable_for_tests(), Some(end));
        for label in ["Properties…", "Open First Device", "Open Second Device", "Cable Length…", "Delete"] {
            assert!(h.query_all_by_label(label).next().is_some(), "the menu lacks {label}");
        }
        assert!(h.query_all_by_label("Show Info").next().is_none(), "Show Info is for the physical workspace");
        h.get_by_label("Properties…").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.cable_window, Some(end));
        assert!(h.query_by_label("Delete Cable").is_some());
        h.get_by_label("Delete Cable").click();
        h.run_steps(3);
        assert_eq!(h.state().session.topology.links.len(), before - 1);
        assert!(h.state().ui.cable_window.is_none() && h.state().ui.selected_cable.is_none());
    }

    #[test]
    fn the_delete_tool_removes_the_cable_under_the_pointer() {
        let mut h = lab();
        let (_, at) = on_a_cable(&h);
        let before = h.state().session.topology.links.len();
        h.state_mut().set_tool(Tool::Delete);
        h.run_steps(2);
        left_click(&mut h, at);
        assert_eq!(h.state().session.topology.links.len(), before - 1);
    }

    #[test]
    fn the_delete_key_removes_the_picked_cable() {
        let mut h = lab();
        let (end, _) = on_a_cable(&h);
        let before = h.state().session.topology.links.len();
        h.state_mut().ui.selected = None;
        h.state_mut().ui.selected_cable = Some(end);
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(3);
        assert_eq!(h.state().session.topology.links.len(), before - 1);
        assert_eq!(h.state().ui.selected_cable, None);
    }

    #[test]
    fn a_down_cable_says_why_in_its_window() {
        let mut h = lab();
        let l = h.state().session.topology.links[0];
        if let Some(p) = h.state_mut().session.topology.device_mut(l.a.device).and_then(|d| d.ports.get_mut(l.a.port)) {
            p.shutdown = true;
        }
        h.state_mut().ui.cable_window = Some(l.a);
        h.run_steps(3);
        assert!(h.query_all_by_label("Down").next().is_some());
        assert!(h.query_all_by_label_contains("is shut down").next().is_some());
    }
}
