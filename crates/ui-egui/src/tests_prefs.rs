// Options > Preferences, User Profile, View Command Log and Algorithm Settings.
#[cfg(test)]
mod t {

    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use netlabcraft_engine::Session;
    use serde_json::json;

    use crate::{PacketApp, UiRequest};

    fn harness(sample: &str) -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1280.0, 900.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.state_mut().ui.show_notes = false;
        h
    }

    fn check(h: &mut Harness<'static, PacketApp>, label: &str) {
        h.get_by_role_and_label(Role::CheckBox, label).click();
        h.run_steps(2);
    }

    #[test]
    fn the_interface_tab_changes_the_settings_it_names() {
        let mut h = harness("two-subnets");
        h.state_mut().open_preferences();
        h.run_steps(3);
        assert!(h.state().ui.prefs.show_animation && !h.state().ui.prefs.play_sound);
        check(&mut h, "Show Animation");
        check(&mut h, "Play Sound");
        check(&mut h, "Show Device Model Labels");
        check(&mut h, "Disable Auto Cable");
        check(&mut h, "Use Metric System (uncheck to use imperial)");
        check(&mut h, "Align physical workspace objects");
        check(&mut h, "Use CLI as Device Default Tab");
        let p = &h.state().ui.prefs;
        assert!(
            !p.show_animation && p.play_sound && !p.show_model_labels && p.disable_auto_cable && !p.metric && p.align_physical && !p.cli_default_tab
        );
        // The ones the engine holds go through its commands.
        assert!(!h.state().session.topology.physical.ignore_lengths);
        check(&mut h, "Enable Cable Length Effects");
        assert!(h.state().session.topology.physical.ignore_lengths);
        assert!(h.state().session.cmd_log.enabled);
        check(&mut h, "Enable Logging");
        assert!(!h.state().session.cmd_log.enabled);
        let logged: Vec<String> = h.state().session.cmd_log.entries().map(|e| e.command.clone()).collect();
        assert!(logged.contains(&"physical.length_effects".to_string()), "{logged:?}");
    }

    #[test]
    fn the_other_tabs_are_there_and_say_when_they_have_nothing() {
        let mut h = harness("two-subnets");
        h.state_mut().open_preferences();
        h.run_steps(3);
        for tab in ["Administrative", "Miscellaneous", "Answer Tree", "Custom Interfaces", "Publishers", "Image Cleanup"] {
            h.get_by_label(tab).click();
            h.run_steps(2);
            assert!(h.query_by_label_contains("Not available yet").is_some(), "{tab}");
        }
        h.get_by_label("Show/Hide").click();
        h.run_steps(2);
        check(&mut h, "Bottom");
        assert!(!h.state().ui.toolbars.bottom);
        h.get_by_label("Font").click();
        h.run_steps(2);
        assert!(h.query_all_by_label("Text size:").next().is_some());
    }

    #[test]
    fn unchecked_labels_and_hidden_bars_leave_the_screen() {
        let mut h = harness("two-subnets");
        h.run_steps(3);
        assert!(h.query_by_label("Delete (Del)").is_some());
        h.state_mut().ui.toolbars.secondary = false;
        h.run_steps(3);
        assert!(h.query_by_label("Delete (Del)").is_none());
        h.state_mut().ui.toolbars.secondary = true;
        // The device box: the Automatic cable is among the cables until Disable Auto Cable.
        crate::show_connections(h.state_mut(), None);
        h.run_steps(3);
        assert!(h.query_by_label("Automatically Choose Connection Type").is_some());
        h.state_mut().ui.prefs.disable_auto_cable = true;
        h.run_steps(3);
        assert!(h.query_by_label("Automatically Choose Connection Type").is_none());
    }

    #[test]
    fn a_device_opens_on_config_without_the_cli_default() {
        let mut h = harness("two-subnets");
        let r1 = h.state().session.topology.resolve("R1").unwrap();
        crate::open_console(h.state_mut(), r1);
        assert_eq!(h.state().ui.windows.get(&r1).unwrap().tab, crate::console::Tab::Cli);
        h.state_mut().ui.windows.clear();
        h.state_mut().ui.prefs.cli_default_tab = false;
        crate::open_console(h.state_mut(), r1);
        assert_eq!(h.state().ui.windows.get(&r1).unwrap().tab, crate::console::Tab::Config);
    }

    #[test]
    fn sounds_are_asked_for_only_when_their_setting_is_on() {
        let mut h = harness("two-subnets");
        let app = h.state_mut();
        app.play(crate::sound::Sound::Delivered);
        app.play(crate::sound::Sound::Ring);
        let first = app.take_requests();
        // Telephony is on by default, the PDU sound is not.
        assert_eq!(first.len(), 1);
        assert!(matches!(&first[0], UiRequest::PlaySound(w) if w.starts_with(b"RIFF")));
        app.ui.prefs.play_sound = true;
        app.ui.prefs.play_telephony = false;
        app.play(crate::sound::Sound::Dropped);
        app.play(crate::sound::Sound::Dial);
        assert_eq!(app.take_requests().len(), 1);
    }

    #[test]
    fn the_profile_window_saves_its_fields_with_ok_and_the_name_is_what_the_activity_offers() {
        let mut h = harness("activity-first-router");
        h.state_mut().open_user_profile();
        h.run_steps(3);
        // The window says in one line that the fields stay on this computer.
        assert!(h.query_by_label_contains("stay on this computer").is_some());
        for label in ["Name:", "E-Mail:", "Additional Info:"] {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
        let ui = &mut h.state_mut().ui.prefs_ui;
        ui.name_draft = "  Test User  ".into();
        ui.email_draft = "user@example.test".into();
        ui.info_draft = "Group A\nBench 3".into();
        h.get_by_label("OK").click();
        h.run_steps(2);
        let p = &h.state().session.profile;
        assert_eq!((p.display_name.as_str(), p.email.as_str(), p.info.as_str()), ("Test User", "user@example.test", "Group A\nBench 3"));
        assert!(!h.state().ui.prefs_ui.show_profile, "OK closes the window");
        let text = h.state().preferences_text();
        assert!(text.contains("name=Test User") && text.contains("email=user@example.test") && text.contains("info=Group A\\nBench 3"), "{text}");
        // Only the name reaches the activity.
        let a = h.state_mut().run("activity.start", json!({}));
        assert!(a.is_some());
        let student = h.state().session.topology.activity.as_ref().and_then(|a| a.student.clone());
        assert_eq!(student.as_deref(), Some("Test User"));
    }

    #[test]
    fn cancel_in_the_profile_window_drops_what_was_typed() {
        let mut h = harness("two-subnets");
        h.state_mut().session.profile.display_name = "Test User".into();
        h.state_mut().open_user_profile();
        h.run_steps(3);
        assert_eq!(h.state().ui.prefs_ui.name_draft, "Test User");
        let ui = &mut h.state_mut().ui.prefs_ui;
        ui.name_draft = "Someone Else".into();
        ui.email_draft = "other@example.test".into();
        ui.info_draft = "changed".into();
        h.get_by_label("Cancel").click();
        h.run_steps(2);
        assert!(!h.state().ui.prefs_ui.show_profile);
        let p = &h.state().session.profile;
        assert_eq!((p.display_name.as_str(), p.email.as_str(), p.info.as_str()), ("Test User", "", ""));
        // Opening it again shows what is kept, not the dropped draft.
        h.state_mut().open_user_profile();
        assert_eq!(h.state().ui.prefs_ui.email_draft, "");
    }

    #[test]
    fn an_old_preferences_file_without_the_new_profile_fields_still_loads() {
        let mut h = harness("two-subnets");
        h.state_mut().load_preferences("dark=on\nname=Test User\nfont=120\n");
        let app = h.state();
        assert!(app.ui.dark && app.ui.prefs.font_percent == 120);
        assert_eq!(app.session.profile.display_name, "Test User");
        assert_eq!((app.session.profile.email.as_str(), app.session.profile.info.as_str()), ("", ""));
        assert!(!app.ui.prefs.show_wireless_signals);
    }

    #[test]
    fn view_wireless_signals_is_a_ui_setting_kept_in_the_preferences() {
        let mut h = harness("wifi");
        h.run_steps(2);
        assert!(!h.state().ui.prefs.show_wireless_signals);
        for dark in [false, true] {
            h.state_mut().ui.dark = dark;
            h.state_mut().ui.prefs.show_wireless_signals = true;
            h.run_steps(3);
            h.state_mut().ui.prefs.show_wireless_signals = false;
        }
        // The shortcut toggles it, in the logical view too.
        let chord = |h: &mut Harness<'static, PacketApp>| {
            h.input_mut().events.push(egui::Event::Key {
                key: egui::Key::W,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND | egui::Modifiers::ALT,
            });
            h.run_steps(2);
        };
        chord(&mut h);
        assert!(h.state().ui.prefs.show_wireless_signals);
        let text = h.state().preferences_text();
        assert!(text.contains("wireless-signals=on"), "{text}");
        let mut fresh = harness("wifi");
        fresh.state_mut().load_preferences(&text);
        assert!(fresh.state().ui.prefs.show_wireless_signals);
        // In the physical view the bar has the button; clicking it turns the signals off.
        h.state_mut().set_physical(true);
        h.run_steps(3);
        h.get_by_label_contains("View Wireless Signals").click();
        h.run_steps(2);
        assert!(!h.state().ui.prefs.show_wireless_signals);
        h.get_by_label_contains("View Wireless Signals").click();
        h.run_steps(2);
        assert!(h.state().ui.prefs.show_wireless_signals);
    }

    #[test]
    fn the_command_log_window_lists_what_ran_and_exports_it() {
        let mut h = harness("two-subnets");
        h.state_mut().run("sim.advance", json!({"seconds": 1}));
        h.state_mut().open_command_log();
        h.run_steps(3);
        assert!(h.query_by_label_contains("sim.advance").is_some());
        assert!(h.query_by_label_contains("file.new").is_some());
        h.get_by_label("Export Log…").click();
        h.run_steps(2);
        let requests = h.state_mut().take_requests();
        assert!(
            requests.iter().any(|r| matches!(r, UiRequest::SaveText { name, text } if name.ends_with(".txt") && text.contains("sim.advance"))),
            "{requests:?}"
        );
        h.get_by_label("Clear").click();
        h.run_steps(2);
        assert!(h.state().session.cmd_log.is_empty());
        assert!(h.query_by_label("No commands have been recorded yet.").is_some());
    }

    #[test]
    fn preferences_survive_a_restart_as_text() {
        let mut h = harness("two-subnets");
        let app = h.state_mut();
        app.ui.prefs.align_physical = true;
        app.ui.prefs.font_percent = 130;
        app.ui.dark = true;
        app.session.cmd_log.enabled = false;
        app.session.profile.display_name = "Ana".into();
        app.session.profile.email = "user@example.test".into();
        app.session.profile.info = "line one\nline two \\ end".into();
        app.ui.prefs.show_wireless_signals = true;
        let text = app.preferences_text();
        let mut fresh = harness("two-subnets");
        fresh.state_mut().load_preferences(&text);
        let b = fresh.state();
        assert!(b.ui.prefs.align_physical && b.ui.prefs.font_percent == 130 && b.ui.dark);
        assert!(!b.session.cmd_log.enabled);
        assert_eq!(b.session.profile.display_name, "Ana");
        assert_eq!(b.session.profile.email, "user@example.test");
        assert_eq!(b.session.profile.info, "line one\nline two \\ end");
        assert!(b.ui.prefs.show_wireless_signals);
        assert_eq!(b.preferences_text(), text);
        // Hostile text changes nothing and does not panic.
        let mut junk = harness("two-subnets");
        junk.state_mut().load_preferences("=\n\u{0}=x\nfont=-1\nname=\u{7}\nemail=\u{7}\ninfo=\\\ndark");
        assert_eq!(junk.state().ui.prefs, crate::prefs::Prefs::default());
    }

    #[test]
    fn qos_stamps_read_the_marks_of_a_frame() {
        use netlabcraft_core::MacAddr;
        use netlabcraft_wire::{ETHERTYPE_IPV4, Frame};
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        ip[1] = 46 << 2;
        let frame = Frame { dst: MacAddr([1; 6]), src: MacAddr([2; 6]), vlan: None, ethertype: ETHERTYPE_IPV4, payload: ip }.encode();
        assert_eq!(crate::canvas::qos_stamp(&frame).as_deref(), Some("DSCP ef"));
        let plain = Frame { dst: MacAddr([1; 6]), src: MacAddr([2; 6]), vlan: None, ethertype: 0x0806, payload: vec![0; 28] }.encode();
        assert_eq!(crate::canvas::qos_stamp(&plain), None);
    }
}
