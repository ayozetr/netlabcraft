// Options > Preferences: Administrative, Show/Hide, Font, Miscellaneous, Answer Tree and what they change.
#[cfg(test)]
mod t {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable};
    use netlabcraft_engine::Session;
    use serde_json::json;

    use crate::console::Tab as DevTab;
    use crate::prefs::Prefs;
    use crate::prefs_look::{Colors, NamedColor};
    use crate::{PacketApp, UiRequest};

    fn harness(sample: &str) -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1280.0, 900.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.show_notes = false;
        h
    }

    fn check(h: &mut Harness<'static, PacketApp>, label: &str) {
        h.get_by_role_and_label(Role::CheckBox, label).click_accesskit();
        h.run_steps(2);
    }

    fn open_tab(h: &mut Harness<'static, PacketApp>, tab: &str) {
        h.state_mut().open_preferences();
        h.run_steps(2);
        h.get_by_label(tab).click();
        h.run_steps(2);
    }

    #[test]
    fn the_password_flow_locks_unlocks_and_never_keeps_the_password() {
        let mut h = harness("two-subnets");
        open_tab(&mut h, "Administrative");
        // Too short or not matching: Enable stays off.
        h.state_mut().ui.prefs_ui.pw_draft = "abc".into();
        h.state_mut().ui.prefs_ui.pw_confirm = "abc".into();
        h.run_steps(2);
        assert!(h.get_by_label("Enable Password").accesskit_node().is_disabled());
        h.state_mut().ui.prefs_ui.pw_draft = "test-pass".into();
        h.state_mut().ui.prefs_ui.pw_confirm = "test-pass".into();
        h.run_steps(2);
        h.get_by_label("Enable Password").click_accesskit();
        h.run_steps(2);
        let record = h.state().ui.prefs.lock_record.clone();
        assert!(netlabcraft_engine::settings::lock_valid(&record) && !record.contains("test-pass"));
        assert!(h.state().ui.prefs_ui.unlocked && h.state().ui.prefs_ui.pw_draft.is_empty());
        check(&mut h, "Lock the Miscellaneous tab");
        assert!(h.state().ui.prefs.lock_misc);
        // The saved text has the record and not the password.
        let text = h.state().preferences_text();
        assert!(text.contains(&format!("lock={record}")) && !text.contains("test-pass"), "{text}");
        // A new session: locked until the password is entered.
        let mut fresh = harness("two-subnets");
        fresh.state_mut().load_preferences(&text);
        assert!(!fresh.state().ui.prefs_ui.unlocked);
        open_tab(&mut fresh, "Miscellaneous");
        assert!(fresh.query_by_label("Locked. Enter the password:").is_some());
        assert!(fresh.get_by_role_and_label(Role::CheckBox, "Show the device window taskbar").accesskit_node().is_disabled());
        assert!(!fresh.state().ui.prefs.device_taskbar);
        // The Interface tab is not covered, so it still works.
        fresh.get_by_label("Interface").click();
        fresh.run_steps(2);
        assert!(!fresh.get_by_role_and_label(Role::CheckBox, "Show Animation").accesskit_node().is_disabled());
        fresh.get_by_label("Miscellaneous").click();
        fresh.run_steps(2);
        // Wrong, then right.
        fresh.state_mut().ui.prefs_ui.pw_entry = "nope".into();
        fresh.get_by_label("Unlock").click_accesskit();
        fresh.run_steps(2);
        assert!(!fresh.state().ui.prefs_ui.unlocked && fresh.query_by_label("That is not the password.").is_some());
        fresh.state_mut().ui.prefs_ui.pw_entry = "test-pass".into();
        fresh.get_by_label("Unlock").click_accesskit();
        fresh.run_steps(2);
        assert!(fresh.state().ui.prefs_ui.unlocked);
        check(&mut fresh, "Show the device window taskbar");
        assert!(fresh.state().ui.prefs.device_taskbar);
        // Disabling the password removes the lock.
        fresh.get_by_label("Administrative").click();
        fresh.run_steps(2);
        fresh.get_by_label("Disable Password").click_accesskit();
        fresh.run_steps(2);
        assert!(fresh.state().ui.prefs.lock_record.is_empty() && !fresh.state().preferences_text().contains("lock="));
    }

    #[test]
    fn restore_defaults_keeps_the_password_and_waits_for_it_when_locked() {
        let mut h = harness("two-subnets");
        h.state_mut().ui.prefs.lock_record = netlabcraft_engine::settings::lock_record("test-pass", "t");
        h.state_mut().ui.prefs.lock_interface = true;
        h.state_mut().ui.prefs.metric = false;
        open_tab(&mut h, "Font");
        assert!(h.get_by_label("Restore Defaults").accesskit_node().is_disabled());
        assert!(!h.state().ui.prefs.metric);
        h.state_mut().ui.prefs_ui.unlocked = true;
        h.run_steps(2);
        h.get_by_label("Restore Defaults").click();
        h.run_steps(2);
        let p = &h.state().ui.prefs;
        assert!(p.metric && !p.lock_record.is_empty() && p.lock_interface);
    }

    #[test]
    fn the_user_folder_is_set_by_the_shell_and_never_holds_control_characters() {
        let mut h = harness("two-subnets");
        h.state_mut().ui.prefs_ui.desktop = true;
        open_tab(&mut h, "Administrative");
        h.get_by_label("Browse…").click_accesskit();
        h.run_steps(2);
        assert!(h.state_mut().take_requests().iter().any(|r| matches!(r, UiRequest::PickFolder)));
        assert_eq!(h.state().user_folder(), None);
        h.state_mut().folder_picked("some-folder");
        assert_eq!(h.state().user_folder(), Some("some-folder"));
        h.state_mut().folder_picked("bad\nfolder");
        assert_eq!(h.state().user_folder(), Some("some-folder"));
        let text = h.state().preferences_text();
        assert!(text.contains("user-folder=some-folder"));
        let mut fresh = harness("two-subnets");
        fresh.state_mut().load_preferences(&text);
        assert_eq!(fresh.state().user_folder(), Some("some-folder"));
        // On the web the field is off.
        let mut web = harness("two-subnets");
        open_tab(&mut web, "Administrative");
        assert!(web.get_by_label("Browse…").accesskit_node().is_disabled());
    }

    #[test]
    fn every_show_hide_box_sets_its_setting() {
        let mut h = harness("two-subnets");
        open_tab(&mut h, "Show/Hide");
        let boxes: [(&str, fn(&Prefs) -> bool); 12] = [
            ("Hide the Physical tab", |p| p.hide_physical_tab),
            ("Hide the Config tab of every device", |p| p.hide_config_tab),
            ("Hide the CLI tab of every device", |p| p.hide_cli_tab),
            ("Hide the Config tab of routers and switches", |p| p.hide_rs_config_tab),
            ("Hide the CLI tab of routers and switches", |p| p.hide_rs_cli_tab),
            ("Hide the Services tab", |p| p.hide_services_tab),
            ("Hide the Desktop tab", |p| p.hide_desktop_tab),
            ("Hide the screen tabs of phones and smart things", |p| p.hide_gui_tab),
            ("Hide the Attributes tab", |p| p.hide_attributes_tab),
            ("Hide wireless and cellular pages", |p| p.hide_wireless_cell),
            ("Hide the coverage range field of Wi-Fi forms", |p| p.hide_coverage_spin),
            ("Open smart things with their advanced settings shown", |p| p.advanced_things),
        ];
        for (label, get) in boxes {
            assert!(!get(&h.state().ui.prefs), "{label}");
            check(&mut h, label);
            assert!(get(&h.state().ui.prefs), "{label}");
        }
        // The wireless look starts on, and the boxes turn each part off.
        assert!(h.state().ui.prefs.wireless_boundary && h.state().ui.prefs.wireless_gradient);
        check(&mut h, "Show the wireless boundary");
        check(&mut h, "Fade the wireless colour toward the edge");
        assert!(!h.state().ui.prefs.wireless_boundary && !h.state().ui.prefs.wireless_gradient);
        // Legacy equipment: there is none, so the box is off for good.
        assert!(h.get_by_role_and_label(Role::CheckBox, "Hide legacy equipment").accesskit_node().is_disabled());
        // All of them are kept in the text, and old text without them loads.
        let text = h.state().preferences_text();
        let mut fresh = harness("two-subnets");
        fresh.state_mut().load_preferences(&text);
        assert_eq!(fresh.state().ui.prefs, h.state().ui.prefs);
        // An old file without any of them leaves the defaults (the wireless look on).
        let mut old = harness("two-subnets");
        old.state_mut().load_preferences("dark=off\nfont=110\n");
        assert!(old.state().ui.prefs.wireless_boundary && old.state().ui.prefs.wireless_gradient && !old.state().ui.prefs.hide_cli_tab);
    }

    #[test]
    fn hidden_tabs_leave_the_device_window_and_it_moves_to_one_that_is_shown() {
        let mut h = harness("two-subnets");
        let (r1, pc) = {
            let t = &h.state().session.topology;
            (t.resolve("R1").unwrap(), t.resolve("PC1").unwrap())
        };
        let dev = |h: &Harness<'static, PacketApp>, d| h.state().session.topology.device(d).unwrap().clone();
        let all = [(DevTab::Physical, "Physical"), (DevTab::Config, "Config"), (DevTab::Cli, "CLI"), (DevTab::Attributes, "Attributes")];
        let names = |p: &Prefs, d: &netlabcraft_model::Device| -> Vec<&'static str> {
            crate::prefs_look::visible_tabs(p, d, &all).into_iter().map(|(_, n)| n).collect()
        };
        let router = dev(&h, r1);
        let mut p = Prefs::default();
        assert_eq!(names(&p, &router), ["Physical", "Config", "CLI", "Attributes"]);
        p.hide_rs_cli_tab = true;
        assert_eq!(names(&p, &router), ["Physical", "Config", "Attributes"]);
        assert_eq!(names(&p, &dev(&h, pc)).len(), 4, "the router and switch box leaves a PC alone");
        p.hide_config_tab = true;
        p.hide_physical_tab = true;
        assert_eq!(names(&p, &router), ["Attributes"]);
        p.hide_attributes_tab = true;
        assert_eq!(names(&p, &router).len(), 4, "a window never loses every tab");
        // In the window: a router opens on CLI; with CLI hidden it is on a shown tab.
        h.state_mut().ui.prefs.hide_cli_tab = true;
        crate::open_console(h.state_mut(), r1);
        h.run_steps(3);
        assert_ne!(h.state().ui.windows.get(&r1).unwrap().tab, DevTab::Cli);
        assert!(h.query_by_label("CLI").is_none());
        h.state_mut().ui.prefs.hide_cli_tab = false;
        h.run_steps(2);
        assert!(h.query_by_label("CLI").is_some());
    }

    #[test]
    fn wireless_pages_hide_and_the_coverage_field_goes() {
        let mut h = harness("wifi");
        let ap = h.state().session.topology.resolve("AP1").unwrap();
        let wireless_port =
            h.state().session.topology.device(ap).unwrap().ports.iter().position(|p| netlabcraft_model::ifname::is_wireless(&p.name)).unwrap();
        crate::open_console(h.state_mut(), ap);
        h.state_mut().ui.windows.get_mut(&ap).unwrap().tab = DevTab::Config;
        h.state_mut().ui.windows.get_mut(&ap).unwrap().page = crate::console::ConfigPage::Port(wireless_port);
        h.run_steps(3);
        assert!(h.query_by_label("Coverage Range (meters)").is_some());
        h.state_mut().ui.prefs.hide_coverage_spin = true;
        h.run_steps(2);
        assert!(h.query_by_label("Coverage Range (meters)").is_none());
        h.state_mut().ui.prefs.hide_wireless_cell = true;
        h.run_steps(3);
        assert_eq!(h.state().ui.windows.get(&ap).unwrap().page, crate::console::ConfigPage::Settings);
    }

    #[test]
    fn the_wireless_look_draws_in_all_four_combinations_and_defaults_to_both() {
        let p = Prefs::default();
        assert_eq!(crate::wireless::Look::of(&p), crate::wireless::Look { boundary: true, gradient: true });
        let mut h = harness("wifi");
        h.state_mut().ui.prefs.show_wireless_signals = true;
        for (boundary, gradient) in [(true, true), (true, false), (false, true), (false, false)] {
            h.state_mut().ui.prefs.wireless_boundary = boundary;
            h.state_mut().ui.prefs.wireless_gradient = gradient;
            for physical in [false, true] {
                h.state_mut().set_physical(physical);
                h.run_steps(2);
            }
        }
    }

    #[test]
    fn smart_things_open_their_advanced_section_only_when_asked() {
        for asked in [false, true] {
            let mut h = harness("smart-home");
            h.state_mut().ui.prefs.advanced_things = asked;
            let cam = h
                .state()
                .session
                .topology
                .devices
                .iter()
                .find(|d| d.kind == netlabcraft_model::DeviceKind::Iot && !netlabcraft_model::iot::can_serve(d))
                .map(|d| d.id)
                .unwrap();
            crate::open_console(h.state_mut(), cam);
            h.run_steps(3);
            assert!(h.query_by_label("Advanced").is_some());
            assert_eq!(h.query_by_label_contains("MAC address:").is_some(), asked);
        }
    }

    #[test]
    fn the_font_tab_maps_points_and_applies_the_colour_table_only_on_apply() {
        let mut h = harness("two-subnets");
        open_tab(&mut h, "Font");
        for label in ["Application size:", "CLI font size:", "Tooltip font size:"] {
            assert!(h.query_all_by_label(label).next().is_some(), "{label}");
        }
        h.state_mut().ui.prefs.font_percent = 125;
        h.state_mut().ui.prefs.cli_pt = 10;
        h.state_mut().ui.prefs.tooltip_pt = 12;
        h.run_steps(3);
        assert!((crate::prefs_look::percent_to_pt(125) - 10.0).abs() < 1e-4);
        assert_eq!(crate::prefs_look::cli_font(&h.state().ui.prefs).size, 15.0);
        assert_eq!(crate::prefs_look::current_tip().0, Some(18.0));
        // Colours: a draft is not applied until Apply.
        let good = Colors { ios_text: NamedColor::Green, ios_bg: NamedColor::Black, ..Colors::default() };
        h.state_mut().ui.prefs_ui.colors_draft = Some(good);
        h.run_steps(2);
        assert_eq!(h.state().ui.prefs.colors, Colors::default());
        h.get_by_label("Apply").click_accesskit();
        h.run_steps(2);
        assert_eq!(h.state().ui.prefs.colors, good);
        // The text is saved by name.
        assert!(h.state().preferences_text().contains("color-ios-text=green"));
        // A pair too close to read cannot be applied.
        let bad = Colors { pc_text: NamedColor::Red, pc_bg: NamedColor::Red, ..good };
        h.state_mut().ui.prefs_ui.colors_draft = Some(bad);
        h.run_steps(2);
        assert!(h.get_by_label("Apply").accesskit_node().is_disabled());
        assert_eq!(h.state().ui.prefs.colors, good);
        h.get_by_label("Reset Colors").click_accesskit();
        h.run_steps(2);
        assert_eq!(h.state().ui.prefs.colors, Colors::default());
        // Tooltips take the chosen colours and size.
        h.state_mut().ui.prefs.colors.tip_text = NamedColor::Yellow;
        h.state_mut().ui.prefs.colors.tip_bg = NamedColor::Black;
        h.run_steps(2);
        let tip = crate::prefs_look::current_tip();
        assert_eq!((tip.1, tip.2), (NamedColor::Yellow.rgb(), NamedColor::Black.rgb()));
    }

    #[test]
    fn the_cli_and_the_consoles_follow_the_font_and_colours() {
        let mut h = harness("two-subnets");
        let r1 = h.state().session.topology.resolve("R1").unwrap();
        h.state_mut().ui.prefs.cli_font = crate::prefs_look::CliFont::Proportional;
        h.state_mut().ui.prefs.cli_pt = 14;
        h.state_mut().ui.prefs.colors = Colors {
            ios_text: NamedColor::Amber,
            ios_bg: NamedColor::Black,
            pc_text: NamedColor::Green,
            pc_bg: NamedColor::Black,
            ..Colors::default()
        };
        crate::open_console(h.state_mut(), r1);
        h.run_steps(3);
        let pc = h.state().session.topology.resolve("PC1").unwrap();
        crate::open_console(h.state_mut(), pc);
        h.state_mut().ui.windows.get_mut(&pc).unwrap().tab = DevTab::Desktop;
        h.run_steps(3);
        let f = crate::prefs_look::cli_font(&h.state().ui.prefs);
        assert_eq!((f.size, f.family.clone()), (21.0, egui::FontFamily::Proportional));
    }

    #[test]
    fn the_event_list_full_action_does_what_it_says() {
        let setup = |action| {
            let mut h = harness("two-subnets");
            h.state_mut().run("net.ping", json!({"from": "PC1", "to": "10.0.2.10"}));
            h.state_mut().ui.prefs.buffer_full = action;
            assert!(!h.state().session.sim.trace().is_empty());
            h
        };
        use crate::prefs_misc::{BufferFull, buffer_check};
        // Not full: nothing happens.
        let mut h = setup(BufferFull::AutoClear);
        buffer_check(h.state_mut(), false);
        assert!(!h.state().session.sim.trace().is_empty());
        // Auto clear.
        buffer_check(h.state_mut(), true);
        assert!(h.state().session.sim.trace().is_empty());
        // View previous: the events are kept aside and shown.
        let mut h = setup(BufferFull::AutoView);
        let n = h.state().session.sim.trace().len();
        buffer_check(h.state_mut(), true);
        h.run_steps(2);
        assert!(h.state().session.sim.trace().is_empty());
        assert_eq!(h.state().ui.prefs_ui.misc.previous.len(), n);
        assert!(h.query_by_label_contains("events kept from the list").is_some());
        // Prompt: it asks once per filling, and Keep going leaves the list alone.
        let mut h = setup(BufferFull::Prompt);
        buffer_check(h.state_mut(), true);
        h.run_steps(2);
        assert!(h.query_by_label("Event List Full").is_some());
        h.get_by_label("Keep going").click_accesskit();
        h.run_steps(2);
        assert!(h.query_by_label("Event List Full").is_none() && !h.state().session.sim.trace().is_empty());
        buffer_check(h.state_mut(), true);
        assert!(h.state().ui.prefs_ui.misc.full_prompt);
        h.state_mut().ui.prefs_ui.misc.full_prompt = false;
        buffer_check(h.state_mut(), true);
        assert!(!h.state().ui.prefs_ui.misc.full_prompt, "asked once per filling");
        buffer_check(h.state_mut(), false);
        buffer_check(h.state_mut(), true);
        h.run_steps(2);
        h.get_by_label("Clear the event list").click_accesskit();
        h.run_steps(2);
        assert!(h.state().session.sim.trace().is_empty());
    }

    #[test]
    fn buffering_only_filtered_events_drops_the_ones_the_filter_hides() {
        let mut h = harness("two-subnets");
        h.state_mut().run("net.ping", json!({"from": "PC1", "to": "10.0.2.10"}));
        let has_arp = |h: &Harness<'static, PacketApp>| h.state().session.sim.trace().iter().any(|e| e.protocol.name() == "ARP");
        assert!(has_arp(&h));
        h.state_mut().ui.events.filter[0] = false; // ARP is the first filter.
        h.run_steps(2);
        assert!(has_arp(&h), "off: everything is kept");
        h.state_mut().ui.prefs.buffer_filtered_only = true;
        h.run_steps(2);
        assert!(!has_arp(&h) && !h.state().session.sim.trace().is_empty());
        // Later events are held to the same filter.
        h.state_mut().run("net.ping", json!({"from": "PC1", "to": "10.0.2.10"}));
        h.run_steps(2);
        assert!(!has_arp(&h));
    }

    #[test]
    fn the_taskbar_lists_open_device_windows_and_a_click_raises_one() {
        let mut h = harness("two-subnets");
        let (r1, pc) = {
            let t = &h.state().session.topology;
            (t.resolve("R1").unwrap(), t.resolve("PC1").unwrap())
        };
        crate::open_console(h.state_mut(), r1);
        crate::open_console(h.state_mut(), pc);
        h.run_steps(3);
        assert!(h.query_by_role_and_label(Role::Button, "R1").is_none(), "off by default");
        h.state_mut().ui.prefs.device_taskbar = true;
        h.run_steps(3);
        assert!(h.query_by_role_and_label(Role::Button, "R1").is_some() && h.query_by_role_and_label(Role::Button, "PC1").is_some());
        h.get_by_role_and_label(Role::Button, "R1").click_accesskit();
        h.run_steps(2);
        assert!(h.state().ui.windows.contains_key(&r1));
        assert_eq!(h.state().ui.prefs_ui.raise, None, "the request is dropped once the windows were drawn");
        h.state_mut().ui.windows.clear();
        h.run_steps(2);
        assert!(h.query_by_role_and_label(Role::Button, "R1").is_none(), "no windows, no bar");
    }

    #[test]
    fn backups_are_asked_for_on_a_timer_only_when_the_lab_changed_and_only_on_the_desktop() {
        let mut h = harness("two-subnets");
        let ctx = egui::Context::default();
        let tick = |h: &mut Harness<'static, PacketApp>, now: f64| -> Vec<UiRequest> {
            crate::prefs_misc::backup_tick(h.state_mut(), &ctx, now);
            h.state_mut().take_requests().into_iter().filter(|r| matches!(r, UiRequest::Backup { .. })).collect()
        };
        h.state_mut().ui.prefs.backup_minutes = 1;
        assert!(tick(&mut h, 0.0).is_empty() && tick(&mut h, 500.0).is_empty(), "the web build keeps no files");
        h.state_mut().ui.prefs_ui.desktop = true;
        h.state_mut().ui.prefs.user_folder = "some-folder".into();
        assert!(tick(&mut h, 1000.0).is_empty(), "the first tick only sets the timer");
        assert!(tick(&mut h, 1030.0).is_empty(), "not due yet");
        let due = tick(&mut h, 1061.0);
        assert!(matches!(&due[..], [UiRequest::Backup { stem, folder }] if stem == "lab" && folder.as_deref() == Some("some-folder")), "{due:?}");
        assert!(tick(&mut h, 1125.0).is_empty(), "unchanged since the last copy");
        h.state_mut().session.revision += 1;
        assert_eq!(tick(&mut h, 1190.0).len(), 1);
        h.state_mut().ui.prefs.backup_minutes = 0;
        h.state_mut().session.revision += 1;
        assert!(tick(&mut h, 5000.0).is_empty(), "off");
    }

    #[test]
    fn the_miscellaneous_tab_offers_the_choices_and_says_what_is_not_applicable() {
        let mut h = harness("two-subnets");
        h.state_mut().ui.prefs_ui.desktop = true;
        open_tab(&mut h, "Miscellaneous");
        for label in ["Auto File Backup Interval:", "When the event list is full:", "Not applicable"] {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
        for label in ["Reload local image files", "External network access", "Use a proxy"] {
            assert!(h.get_by_role_and_label(Role::CheckBox, label).accesskit_node().is_disabled(), "{label}");
        }
        check(&mut h, "Buffer only the events that pass the filter");
        assert!(h.state().ui.prefs.buffer_filtered_only);
        // Saved and read back.
        h.state_mut().ui.prefs.backup_minutes = 10;
        h.state_mut().ui.prefs.buffer_full = crate::prefs_misc::BufferFull::AutoView;
        let text = h.state().preferences_text();
        let mut fresh = harness("two-subnets");
        fresh.state_mut().load_preferences(&text);
        assert_eq!(fresh.state().ui.prefs, h.state().ui.prefs);
        // A value the tab does not offer is ignored.
        fresh.state_mut().load_preferences("backup=7\nbuffer-full=nonsense\ncli-size=500\nlock=garbage\n");
        assert_eq!(fresh.state().ui.prefs.backup_minutes, 10);
        assert!(fresh.state().ui.prefs.lock_record.is_empty());
    }

    #[test]
    fn the_answer_tree_filter_shows_only_the_items_it_names() {
        let mut h = harness("activity-first-router");
        h.state_mut().ui.activity.tools.show_tree = true;
        h.run_steps(3);
        let items = h.state_mut().run("activity.tree", json!({})).unwrap()["items"].as_array().unwrap().clone();
        let count = |h: &mut Harness<'static, PacketApp>| h.query_all_by_label("graded").count();
        let kind = |k: &str| items.iter().filter(|i| i["device_kind"] == k).count();
        let cables = items.iter().filter(|i| i["kind"] == "cable").count();
        assert_eq!(count(&mut h), items.len());
        for (filter, want) in [
            (crate::prefs_tabs::AnswerFilter::Router, kind("router")),
            (crate::prefs_tabs::AnswerFilter::Switch, kind("switch")),
            (crate::prefs_tabs::AnswerFilter::EndDevice, kind("end")),
            (crate::prefs_tabs::AnswerFilter::Default, items.len() - cables),
            (crate::prefs_tabs::AnswerFilter::ShowAll, items.len()),
        ] {
            h.state_mut().ui.prefs.answer_filter = filter;
            h.run_steps(3);
            assert_eq!(count(&mut h), want, "{filter:?}");
        }
        // The tab sets it.
        h.state_mut().ui.activity.tools.show_tree = false;
        open_tab(&mut h, "Answer Tree");
        h.get_by_label("Router").click_accesskit();
        h.run_steps(2);
        assert_eq!(h.state().ui.prefs.answer_filter, crate::prefs_tabs::AnswerFilter::Router);
        assert!(h.state().preferences_text().contains("answer-filter=router"));
    }

    #[test]
    fn the_image_cleanup_table_is_empty_and_says_why() {
        let mut h = harness("two-subnets");
        open_tab(&mut h, "Image Cleanup");
        for col in ["Remove", "Type", "Size", "Image"] {
            assert!(h.query_all_by_label(col).next().is_some(), "{col}");
        }
        assert!(h.query_by_label_contains("The lab holds no images").is_some());
    }
}
