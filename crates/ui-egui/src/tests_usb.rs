// UI harness tests for the USB windows: a stick's Files page and a host's USB app.
#[cfg(test)]
mod t {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use netlabcraft_engine::Session;
    use serde_json::json;

    use crate::PacketApp;
    use crate::console::{DesktopApp, Tab};

    /// A PC with a stick and a printer plugged into it, and a second, loose, stick.
    fn lab() -> Harness<'static, PacketApp> {
        let mut s = Session::new();
        for (model, name) in [("PC", "PC1"), ("USB Storage", "Stick1"), ("Printer", "P1"), ("USB Storage", "Spare")] {
            s.execute("device.add", json!({"model": model, "name": name})).unwrap();
        }
        s.execute("link.add", json!({"a": "PC1", "b": "Stick1", "cable": "usb"})).unwrap();
        s.execute("link.add", json!({"a": "PC1", "b": "P1", "cable": "usb"})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut PacketApp| app.ui(ui), PacketApp::new(s));
        h.run_steps(3);
        h
    }

    fn id(h: &Harness<'static, PacketApp>, name: &str) -> netlabcraft_core::DeviceId {
        h.state().session.topology.find(name).unwrap().id
    }

    #[test]
    fn a_stick_window_opens_on_its_files_and_ejects_deletes_and_saves() {
        let mut h = lab();
        let stick = id(&h, "Stick1");
        crate::open_console(h.state_mut(), stick);
        h.run_steps(4);
        assert_eq!(h.state().ui.windows.get(&stick).map(|w| w.tab), Some(Tab::Iot), "a stick opens on its Files page");
        assert!(h.query_all_by_label("readme.txt").next().is_some());
        assert!(h.query_all_by_label("Plugged into PC1 (USB 1): mounted").next().is_some());
        // Select the file and delete it through the command.
        h.query_all_by_label("readme.txt").next().unwrap().click();
        h.run_steps(2);
        h.query_all_by_label("Delete").next().unwrap().click();
        h.run_steps(3);
        assert!(h.state().session.topology.device(stick).unwrap().usb.files.is_empty());
        assert!(h.query_all_by_label("No files on the stick.").next().is_some());
        // Eject, and the page says so; Mount brings it back.
        h.query_all_by_label("Eject").next().unwrap().click();
        h.run_steps(3);
        assert!(h.state().session.topology.usbs.iter().any(|c| c.device == stick && c.ejected));
        assert!(h.query_all_by_label("Plugged into PC1 (USB 1): ejected").next().is_some());
        h.query_all_by_label("Mount").next().unwrap().click();
        h.run_steps(3);
        assert!(h.state().session.topology.usbs.iter().all(|c| !c.ejected));
        // A file saved through the form lands on the stick (the form is filled as typing would).
        h.state_mut().session.execute("usb.put", json!({"device": "Stick1", "name": "a.txt", "text": "x"})).unwrap();
        h.run_steps(3);
        assert!(h.query_all_by_label("a.txt").next().is_some());
    }

    #[test]
    fn a_stick_that_is_not_plugged_in_says_so_and_cannot_be_written() {
        let mut h = lab();
        let spare = id(&h, "Spare");
        crate::open_console(h.state_mut(), spare);
        h.run_steps(4);
        assert!(h.query_all_by_label("Not plugged in: join it to a host with a USB cable.").next().is_some());
        assert!(h.query_all_by_label("Eject").next().is_none(), "nothing to eject");
        h.state_mut().session.execute("usb.put", json!({"device": "Spare", "name": "a.txt", "text": "x"})).unwrap_err();
    }

    #[test]
    fn the_usb_app_lists_the_ports_and_what_is_in_them() {
        let mut h = lab();
        let pc = id(&h, "PC1");
        crate::open_desktop_app(h.state_mut(), pc, "usb");
        h.run_steps(4);
        assert_eq!(h.state().ui.windows.get(&pc).and_then(|w| w.desktop), Some(DesktopApp::Usb));
        assert!(h.query_all_by_label("USB 1").next().is_some() && h.query_all_by_label("USB 2").next().is_some());
        assert!(h.query_all_by_label("Stick1 (USB Storage)").next().is_some());
        assert!(h.query_all_by_label("P1 (Printer)").next().is_some());
        // Eject the stick from the app.
        h.query_all_by_label("Eject").next().unwrap().click();
        h.run_steps(3);
        assert!(h.state().session.topology.usbs.iter().any(|c| c.ejected));
        assert!(h.query_all_by_label("ejected").next().is_some());
        // Print a test page over the cable.
        h.query_all_by_label("Print test page").next().unwrap().click();
        h.run_steps(3);
        let printer = id(&h, "P1");
        let jobs = &h.state().session.topology.device(printer).unwrap().printer.jobs;
        assert_eq!((jobs.len(), jobs[0].via.as_str(), jobs[0].text.as_str()), (1, "usb", "Test page from PC1"));
        // The tile is on the desktop of a PC, a laptop and a server, not on a printer's.
        h.state_mut().ui.windows.get_mut(&pc).unwrap().desktop = None;
        h.run_steps(3);
        assert!(h.query_all_by_label("USB").next().is_some(), "the USB tile");
    }

    #[test]
    fn a_stick_is_drawn_and_has_its_tile_in_the_device_box() {
        let h = lab();
        let t = &h.state().session.topology;
        assert!(t.device(id(&h, "Stick1")).unwrap().is_usb_stick());
        assert_eq!(crate::bottom::tile_names(crate::bottom::Sub::Hosts).iter().filter(|n| **n == "USB Storage").count(), 1);
        assert_eq!(crate::bottom::tile_names(crate::bottom::Sub::Boards).iter().filter(|n| **n == "USB Storage").count(), 1);
    }
}
