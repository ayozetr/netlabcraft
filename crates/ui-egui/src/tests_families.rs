// UI harness tests for the device families: the switch card in the Physical tab, power over
// Ethernet, SIP phones, IoT things, the cloud and the modems.
#[cfg(test)]
mod t {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use netlabcraft_model::cable::Cable;
    use netlabcraft_model::models;
    use serde_json::json;

    use crate::{PacketApp, i18n};

    fn harness(sample: &str) -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s))
    }

    /// Model names that read the same in Spanish.
    const SAME_IN_SPANISH: &[&str] = &[
        "PC",
        "WLC",
        "Router 2GE",
        "Router 3GE",
        "Router 4GE",
        "Router 2GE+2S",
        "Firewall 5GE",
        "Switch 8FE",
        "Switch 24FE",
        "Switch 48FE",
        "Switch 24FE+2SFP",
        "Hub 8",
    ];

    /// Every model, card and cable reads in Spanish: names and descriptions.
    #[test]
    fn every_model_card_and_cable_has_spanish_rows() {
        let es = i18n::spanish();
        let mut missing = Vec::new();
        for m in models::MODELS {
            for text in [m.label, m.description] {
                if !es.contains_key(text) && !SAME_IN_SPANISH.contains(&text) {
                    missing.push(text);
                }
            }
        }
        for c in models::MODULES {
            for text in [c.label, c.description] {
                if !es.contains_key(text) {
                    missing.push(text);
                }
            }
        }
        for c in Cable::ALL {
            for text in [c.label(), c.description()] {
                if !es.contains_key(text) {
                    missing.push(text);
                }
            }
        }
        assert!(missing.is_empty(), "no Spanish row for: {missing:#?}");
    }

    #[test]
    fn the_physical_tab_shows_a_switch_card() {
        let mut h = harness("switch-card");
        let r1 = h.state().session.topology.resolve("R1").unwrap();
        crate::open_physical_tab(h.state_mut(), r1);
        h.state_mut().ui.show_notes = false;
        h.run_steps(3);
        assert!(h.query_by_label("Bay 0/1: 4-port switch card (click to take it out)").is_some());
        assert!(h.query_by_label("4-port switch card").is_some(), "the card list offers it");
    }

    #[test]
    fn the_physical_tab_plugs_a_phones_adapter_and_says_where_its_power_comes_from() {
        let mut h = harness("ip-phones");
        let ph = h.state().session.topology.resolve("Phone1").unwrap();
        crate::open_physical_tab(h.state_mut(), ph);
        h.state_mut().ui.show_notes = false;
        h.run_steps(3);
        assert!(h.query_by_label_contains("Powered over Ethernet by S1 Fa0/1").is_some());
        h.get_by_label("Power adapter: unplugged (click to plug in)").click();
        h.run_steps(2);
        assert_eq!(h.state().session.topology.device(ph).unwrap().adapter, Some(true));
        assert!(h.query_by_label("Powered by its adapter").is_some());
        // The switch's tab shows its budget.
        let s1 = h.state().session.topology.resolve("S1").unwrap();
        h.state_mut().ui.windows.clear();
        crate::open_physical_tab(h.state_mut(), s1);
        h.run_steps(3);
        assert!(h.query_by_label_contains("W of 370.0 W in use").is_some());
    }

    #[test]
    fn the_phone_tab_switches_to_sip_and_calls() {
        let mut h = harness("sip-phones");
        let (p1, p2) = (h.state().session.topology.resolve("Phone1").unwrap(), h.state().session.topology.resolve("Phone2").unwrap());
        h.state_mut().ui.show_notes = false;
        crate::open_console(h.state_mut(), p1);
        h.run_steps(3);
        h.get_by_label("Refresh").click();
        h.run_steps(2);
        assert!(h.query_by_label_contains("Line 2001").is_some());
        for k in "2002".chars() {
            h.get_by_label(&k.to_string()).click();
            h.run_steps(1);
        }
        h.get_by_label("Call").click();
        h.run_steps(2);
        let state = |h: &Harness<'_, PacketApp>, p| h.state().session.sim.phone(p).and_then(|x| x.call.as_ref()).map(|c| c.state);
        assert_eq!(state(&h, p2), Some(netlabcraft_sim::voice::CallState::Ringing));
        // Back to the own signalling from the checkbox.
        h.state_mut().session.execute("phone.hangup", json!({"device": "Phone1"})).unwrap();
        h.get_by_label("Signal with SIP").click();
        h.run_steps(2);
        assert!(!h.state().session.topology.device(p1).unwrap().voice.sip);
    }

    #[test]
    fn the_gateways_iot_tab_lists_things_live_and_adds_an_and_rule() {
        let mut h = harness("smart-home");
        let (gw, motion, cam) = {
            let t = &h.state().session.topology;
            (t.resolve("Home").unwrap(), t.resolve("Motion1").unwrap(), t.resolve("Cam1").unwrap())
        };
        h.state_mut().ui.show_notes = false;
        crate::open_console(h.state_mut(), gw);
        if let Some(w) = h.state_mut().ui.windows.get_mut(&gw) {
            w.tab = crate::console::Tab::Iot;
        }
        h.run_steps(3);
        h.get_by_label("Refresh").click();
        h.run_steps(2);
        assert!(h.query_by_label("If Smoke1 > 20 or Temp1 > 60, switch Door1 on").is_some());
        assert!(h.query_by_label("Motion1").is_some() && h.query_by_label("webcam").is_some());
        // Movement: the camera's still image shows on the page.
        h.state_mut().session.execute("iot.configure", serde_json::json!({"device": motion.0, "value": 1})).unwrap();
        h.state_mut().session.execute("sim.advance", serde_json::json!({"seconds": 6})).unwrap();
        h.run_steps(2);
        assert!(h.query_by_label_contains("someone in view").is_some());
        let rules = h.state().session.topology.device(gw).unwrap().iot.rules.len();
        h.get_by_label("If").click();
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::Text("Temp1".into()));
        h.run_steps(1);
        h.get_by_label("second sensor").click();
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::Text("Motion1".into()));
        h.run_steps(1);
        h.get_by_label("device").click();
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::Text("Light1".into()));
        h.run_steps(1);
        h.get_by_label("Add rule").click();
        h.run_steps(2);
        let d = h.state().session.topology.device(gw).unwrap();
        assert_eq!(d.iot.rules.len(), rules + 1);
        assert_eq!(d.iot.rules.last().unwrap().all_conditions().len(), 2);
        // A thing's own tab: its timer and its state.
        h.state_mut().ui.windows.clear();
        crate::open_console(h.state_mut(), cam);
        h.run_steps(3);
        assert!(h.query_by_label("Report every (s):").is_some());
        assert!(h.query_by_label("recording").is_some());
    }
}
