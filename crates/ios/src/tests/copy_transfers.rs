//! `copy` between flash, the configurations and FTP, HTTP and flash files.

use netlabcraft_model::DeviceKind;
use netlabcraft_model::services::{FtpUser, WebFile};

use super::Lab;

/// R1 and a server on one subnet: the server has an FTP account and a web page that holds a
/// small configuration.
fn lab() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(r, "g0/0", srv, "g0");
    lab.run(srv, "ipconfig 192.168.1.5 255.255.255.0 192.168.1.1");
    lab.run(r, "en\nconf t\nint g0/0\nip add 192.168.1.1 255.255.255.0\nno shut\nexit\nip ftp username student\nip ftp password ftp123\nend");
    let s = &mut lab.t.device_mut(srv).unwrap().services;
    s.ftp.users.push(FtpUser { name: "student".into(), password: "ftp123".into(), read: true, write: true, delete: false, list: true });
    s.http.files.push(WebFile { name: "base.cfg".into(), content: "hostname FromWeb\n".into() });
    (lab, r, srv)
}

#[test]
fn a_configuration_goes_to_flash_and_back() {
    let (mut lab, r, _) = lab();
    let out = lab.run(r, "copy running-config flash: backup.cfg");
    assert!(out.contains("Wrote") && out.contains("flash:backup.cfg"), "{out}");
    assert!(lab.run(r, "show flash").contains("backup.cfg"));
    lab.run(r, "conf t\nhostname Changed\nend");
    assert!(lab.run(r, "copy flash: running-config backup.cfg").contains("Merged"));
    assert!(lab.try_run(r, "copy flash: running-config missing.cfg").starts_with("% "));
}

#[test]
fn a_flash_file_travels_over_ftp_and_back() {
    let (mut lab, r, srv) = lab();
    lab.run(r, "copy running-config flash: backup.cfg");
    let out = lab.run(r, "copy flash: ftp: backup.cfg 192.168.1.5 saved.cfg");
    assert!(out.contains("ftp://192.168.1.5/saved.cfg"), "{out}");
    assert!(lab.t.device(srv).unwrap().services.ftp.files.iter().any(|f| f.name == "saved.cfg"));
    let out = lab.run(r, "copy ftp: flash: 192.168.1.5 saved.cfg again.cfg");
    assert!(out.contains("flash:again.cfg"), "{out}");
    assert!(lab.run(r, "show flash").contains("again.cfg"));
    lab.run(r, "conf t\nno ip ftp username\nend");
    assert!(lab.try_run(r, "copy flash: ftp: backup.cfg 192.168.1.5").contains("ip ftp username"));
}

#[test]
fn a_page_of_a_web_server_becomes_a_configuration_or_a_flash_file() {
    let (mut lab, r, _) = lab();
    let out = lab.run(r, "copy http: running-config 192.168.1.5 base.cfg");
    assert!(out.contains("http://192.168.1.5/base.cfg"), "{out}");
    assert!(lab.run(r, "show running-config").contains("hostname FromWeb"));
    assert!(lab.run(r, "copy http: flash: 192.168.1.5 base.cfg").contains("flash:base.cfg"));
    assert!(lab.run(r, "show flash").contains("base.cfg"));
    assert!(lab.run(r, "copy http: startup-config 192.168.1.5 base.cfg").contains("startup-config"));
    assert!(lab.try_run(r, "copy http: flash: 192.168.1.5 nothing.here").starts_with("% "));
}
