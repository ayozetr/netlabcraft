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
}
