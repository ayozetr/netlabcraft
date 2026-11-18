//! A USB stick plugged into a router or a switch: `show usb`, `dir usbflash0:`, `copy` and
//! `show file systems`.

use netlabcraft_model::DeviceKind;
use netlabcraft_model::models;
use netlabcraft_model::usb::STICK_MODEL;

use super::Lab;

/// R1 with a stick plugged into its USB port.
fn lab() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let stick = lab.t.add_model(models::find(STICK_MODEL).unwrap(), None, 0.0, 0.0).unwrap();
    lab.t.connect_usb_data(r, stick, None).unwrap();
    lab.run(r, "en");
    (lab, r, stick)
}

#[test]
fn show_usb_names_the_port_and_what_is_plugged_into_it() {
    let (mut lab, r, stick) = lab();
    let out = lab.run(r, "show usb");
    assert!(out.contains("USB ports: 1") && out.contains("Port 1: Stick1") && out.contains("mounted") && out.contains("1 files"), "{out}");
    lab.t.usbs[0].ejected = true;
    assert!(lab.run(r, "show usb").contains("ejected"));
    lab.t.usbs[0].ejected = false;
    lab.t.device_mut(stick).unwrap().powered_off = true;
    assert!(lab.run(r, "show usb").contains("powered off"));
    lab.t.disconnect_usb_data(stick);
    assert!(lab.run(r, "show usb").contains("Port 1: empty"));
}

#[test]
fn a_file_goes_to_the_stick_and_back_and_the_file_systems_list_it() {
    let (mut lab, r, stick) = lab();
    assert!(lab.run(r, "dir usbflash0:").contains("readme.txt"));
    assert!(!lab.run(r, "show file systems").is_empty());
    assert!(lab.run(r, "show file systems").contains("usbflash0:"));
    lab.run(r, "copy running-config flash: backup.cfg");
    let out = lab.run(r, "copy flash: usbflash0: backup.cfg saved.cfg");
    assert!(out.contains("usbflash0:saved.cfg"), "{out}");
    assert!(lab.run(r, "dir usbflash0:").contains("saved.cfg"));
    assert!(lab.t.device(stick).unwrap().usb.find("saved.cfg").is_some_and(|f| f.text.contains("hostname")));
    let out = lab.run(r, "copy usbflash0: flash: saved.cfg again.cfg");
    assert!(out.contains("flash:again.cfg"), "{out}");
    assert!(lab.run(r, "show flash").contains("again.cfg"));
    // The readme is a text file too.
    lab.run(r, "copy usbflash0: flash: readme.txt");
    assert!(lab.run(r, "show flash").contains("readme.txt"));
    assert!(lab.try_run(r, "copy usbflash0: flash: nothing.txt").contains("no such file"));
    assert!(lab.try_run(r, "copy flash: usbflash0: missing.cfg").starts_with("% "));
    assert!(lab.try_run(r, "copy flash: usbflash0:").starts_with("% "));
    assert!(lab.try_run(r, "copy flash: usbflash0: backup.cfg ../x").starts_with("% "));
}

#[test]
fn an_ejected_or_missing_stick_answers_with_the_reason() {
    let (mut lab, r, _) = lab();
    lab.t.usbs[0].ejected = true;
    for line in ["dir usbflash0:", "copy flash: usbflash0: x", "copy usbflash0: flash: readme.txt"] {
        let out = lab.try_run(r, line);
        assert!(out.starts_with("% "), "{line}: {out}");
    }
    assert!(lab.try_run(r, "dir usbflash0:").contains("ejected"));
    lab.t.usbs.clear();
    assert!(lab.try_run(r, "dir usbflash0:").contains("No USB stick"));
    assert!(!lab.run(r, "show file systems").contains("usbflash0:"));
}
