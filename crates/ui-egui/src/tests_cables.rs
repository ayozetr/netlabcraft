// UI harness tests for cables as objects: select, hover, the right-click menu, the Properties window,
// the Delete tool and the keyboard.
#[cfg(test)]
mod t {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use netlabcraft_model::End;
    use netlabcraft_model::cable_ref::CableRef;
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
        assert_eq!(h.state().ui.hover_cable, Some(CableRef::Link(end)));
        left_click(&mut h, at);
        assert_eq!(h.state().ui.selected_cable, Some(CableRef::Link(end)));
        assert_eq!(h.state().ui.selected, None);
        assert!(h.state().ui.cable_window.is_none(), "a click picks the cable; Properties opens its window");
    }

    #[test]
    fn the_right_click_menu_opens_properties_and_the_window_deletes_the_cable() {
        let mut h = lab();
        let (end, at) = on_a_cable(&h);
        let before = h.state().session.topology.links.len();
        right_click(&mut h, at);
        assert_eq!(h.state().ui.canvas.context_cable_for_tests(), Some(CableRef::Link(end)));
        for label in ["Properties…", "Open First Device", "Open Second Device", "Cable Length…", "Delete"] {
            assert!(h.query_all_by_label(label).next().is_some(), "the menu lacks {label}");
        }
        assert!(h.query_all_by_label("Show Info").next().is_none(), "Show Info is for the physical workspace");
        h.get_by_label("Properties…").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.cable_window, Some(CableRef::Link(end)));
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
        h.state_mut().ui.selected_cable = Some(CableRef::Link(end));
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
        h.state_mut().ui.cable_window = Some(CableRef::Link(l.a));
        h.run_steps(3);
        assert!(h.query_all_by_label("Down").next().is_some());
        assert!(h.query_all_by_label_contains("is shut down").next().is_some());
    }

    #[test]
    fn a_fiber_cable_shows_its_loss_budget_and_the_splice_buttons_change_it() {
        let mut s = Session::new();
        for n in ["F1", "F2"] {
            s.execute("device.add", json!({"model": "Switch-24-SFP", "name": n})).unwrap();
        }
        s.execute("link.add", json!({"a": "F1", "b": "F2", "cable": "multi-mode"})).unwrap();
        s.execute("link.length", json!({"device": "F1", "port": "g0/1", "meters": 550})).unwrap();
        let end = s.topology.links[0].a;
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.cable_window = Some(CableRef::Link(end));
        h.run_steps(3);
        assert!(h.query_all_by_label("Loss budget").next().is_some());
        assert!(h.query_all_by_label_contains("degraded").next().is_some(), "550 m of multi-mode leaves under a decibel");
        h.get_by_label("More splices").click();
        h.run_steps(3);
        assert_eq!(h.state().session.topology.splices_on(&h.state().session.topology.links[0]), 1);
        h.get_by_label("More splices").click();
        h.run_steps(3);
        assert!(h.query_all_by_label("over the budget").next().is_some());
        assert!(h.query_all_by_label("Down").next().is_some());
        h.get_by_label("Fewer splices").click();
        h.run_steps(3);
        assert_eq!(h.state().session.topology.splices_on(&h.state().session.topology.links[0]), 1);
    }

    /// A PC and a router a long way apart, joined by a console cable: its name and the middle of its line.
    fn console_lab() -> (Harness<'static, PacketApp>, CableRef, egui::Pos2) {
        let mut s = Session::new();
        let pc = s.topology.add_device(netlabcraft_model::DeviceKind::Pc, None, 100.0, 200.0).unwrap();
        let r = s.topology.add_device(netlabcraft_model::DeviceKind::Router, None, 500.0, 200.0).unwrap();
        s.topology.connect_console(pc, r).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.run_steps(3);
        let app = h.state();
        let rect = app.ui.canvas_rect.unwrap();
        let (origin, zoom) = (rect.min.to_vec2() + app.ui.pan, app.ui.zoom);
        let scr = |id| {
            let d = app.session.topology.device(id).unwrap();
            egui::pos2(d.x * zoom, d.y * zoom) + origin
        };
        let mid = scr(pc) + (scr(r) - scr(pc)) * 0.5;
        (h, CableRef::Console(r), mid)
    }

    #[test]
    fn a_console_cable_is_hovered_picked_and_has_a_menu_and_a_window_like_any_cable() {
        let (mut h, r, at) = console_lab();
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        h.run_steps(3);
        assert_eq!(h.state().ui.hover_cable, Some(r));
        left_click(&mut h, at);
        assert_eq!(h.state().ui.selected_cable, Some(r));
        right_click(&mut h, at);
        assert_eq!(h.state().ui.canvas.context_cable_for_tests(), Some(r));
        for label in ["Properties…", "Open First Device", "Open Second Device", "Delete"] {
            assert!(h.query_all_by_label(label).next().is_some(), "the menu lacks {label}");
        }
        assert!(h.query_all_by_label("Cable Length…").next().is_none(), "a console cable has no length of its own");
        h.get_by_label("Properties…").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.cable_window, Some(r));
        assert!(h.query_all_by_label("Console").next().is_some(), "the window names the kind");
        h.get_by_label("Delete Cable").click();
        h.run_steps(3);
        assert!(h.state().session.topology.consoles.is_empty());
        assert!(h.state().ui.cable_window.is_none() && h.state().ui.selected_cable.is_none());
    }

    #[test]
    fn the_delete_tool_and_the_delete_key_remove_a_console_cable() {
        let (mut h, _, at) = console_lab();
        h.state_mut().set_tool(Tool::Delete);
        h.run_steps(2);
        left_click(&mut h, at);
        assert!(h.state().session.topology.consoles.is_empty());
        let (mut h, r, _) = console_lab();
        h.state_mut().ui.selected = None;
        h.state_mut().ui.selected_cable = Some(r);
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(3);
        assert!(h.state().session.topology.consoles.is_empty());
        assert_eq!(h.state().ui.selected_cable, None);
    }

    #[test]
    fn a_console_cable_is_an_accessible_node_that_opens_its_window() {
        let (mut h, r, _) = console_lab();
        let info = h.state().session.topology.cable_info(r).unwrap();
        let label = format!("Cable, {}, {} - {}", info.kind.label(), info.a_label, info.b_label);
        assert!(h.query_by_label(&label).is_some(), "no node labelled {label}");
        h.get_by_label(&label).click();
        h.run_steps(3);
        assert_eq!(h.state().ui.cable_window, Some(r));
        assert_eq!(h.state().ui.selected_cable, Some(r));
    }
}
