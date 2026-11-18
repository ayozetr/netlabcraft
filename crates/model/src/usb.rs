//! USB data cables and the USB storage stick.
//!
//! A **host** has USB ports a cable plugs into: a PC, laptop or server (two each, the other end
//! of a USB console cable takes one of a PC's) and a router or switch with a command line (one,
//! for a stick). A **USB device** has one port: a printer, a programmable board, a USB stick, a
//! tablet or a smartphone (a USB-C port). A USB data cable joins one of a host's ports to a USB
//! device, so a stick plugs into a PC or a router, a printer into a PC, a board into a PC. These
//! cables are kept apart from the network links in [`Topology::usbs`], like console cables: they
//! carry no Ethernet frames, only the file transfer and the programming the host's apps do.
//!
//! The stick keeps a small list of files (text) in the device itself. A host sees it as mounted
//! while it is plugged in and has not been ejected.

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, ModelError, Result, Topology};

/// The model id of the USB storage stick.
pub const STICK_MODEL: &str = "USB-Storage";
/// What a stick holds (bytes; shown by `dir` and the stick's window).
pub const STICK_CAPACITY: u64 = 134_217_728;
/// Most files on a stick.
pub const MAX_FILES: usize = 32;
/// Longest file name.
pub const MAX_NAME: usize = 64;
/// Most text in one file (bytes).
pub const MAX_FILE_TEXT: usize = 16 * 1024;
/// Most text on a stick in all (bytes).
pub const MAX_STICK_TEXT: usize = 128 * 1024;
/// Most USB data cables in a lab.
pub const MAX_USB_CABLES: usize = 2_000;

/// A file on a stick: a name and its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StickFile {
    pub name: String,
    #[serde(default)]
    pub text: String,
}

impl StickFile {
    /// Its size in bytes.
    pub fn size(&self) -> u64 {
        self.text.len() as u64
    }
}

/// What a USB stick holds. Empty on every other device, and then not saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stick {
    /// The files, in the order they were put there.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<StickFile>,
}

/// Whether a name can be a file name: 1 to [`MAX_NAME`] characters, none of them a path
/// separator, a colon, a wildcard or a control character, no space at either end and not
/// starting with a dot.
pub fn valid_file_name(name: &str) -> bool {
    let n = name.chars().count();
    (1..=MAX_NAME).contains(&n) && !name.starts_with('.') && name.trim() == name && !name.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
}

impl Stick {
    /// A stick as it comes out of the box: one short text file.
    pub fn fresh() -> Stick {
        Stick { files: vec![StickFile { name: "readme.txt".into(), text: "A NetLabCraft USB stick.\n".into() }] }
    }

    /// The bytes the files take.
    pub fn used(&self) -> u64 {
        self.files.iter().map(StickFile::size).sum()
    }

    pub fn find(&self, name: &str) -> Option<&StickFile> {
        self.files.iter().find(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// Puts a file on the stick, replacing one of the same name (names ignore case).
    pub fn put(&mut self, name: &str, text: &str) -> Result<()> {
        if !valid_file_name(name) {
            return Err(ModelError::Invalid(format!(
                "`{name}` is not a file name: 1 to {MAX_NAME} characters, no / \\ : * ? \" < > |, not starting with a dot"
            )));
        }
        if text.len() > MAX_FILE_TEXT {
            return Err(ModelError::Invalid(format!("a file holds at most {MAX_FILE_TEXT} bytes")));
        }
        let others: usize = self.files.iter().filter(|f| !f.name.eq_ignore_ascii_case(name)).map(|f| f.text.len()).sum();
        if others.saturating_add(text.len()) > MAX_STICK_TEXT {
            return Err(ModelError::Invalid(format!("the stick is full: {MAX_STICK_TEXT} bytes of files at most")));
        }
        match self.files.iter_mut().find(|f| f.name.eq_ignore_ascii_case(name)) {
            Some(f) => f.text = text.to_string(),
            None => {
                if self.files.len() >= MAX_FILES {
                    return Err(ModelError::Invalid(format!("a stick holds at most {MAX_FILES} files")));
                }
                self.files.push(StickFile { name: name.to_string(), text: text.to_string() });
            }
        }
        Ok(())
    }

    /// Takes a file off the stick; whether there was one.
    pub fn delete(&mut self, name: &str) -> bool {
        let before = self.files.len();
        self.files.retain(|f| !f.name.eq_ignore_ascii_case(name));
        self.files.len() != before
    }

    /// Makes a stick from a hostile file hold only what a stick can.
    pub(crate) fn sanitize(&mut self) {
        let mut kept: Vec<StickFile> = Vec::new();
        let mut total = 0usize;
        for f in std::mem::take(&mut self.files) {
            let ok = kept.len() < MAX_FILES
                && valid_file_name(&f.name)
                && f.text.len() <= MAX_FILE_TEXT
                && total.saturating_add(f.text.len()) <= MAX_STICK_TEXT
                && !kept.iter().any(|k| k.name.eq_ignore_ascii_case(&f.name));
            if ok {
                total = total.saturating_add(f.text.len());
                kept.push(f);
            }
        }
        self.files = kept;
    }
}

/// Most jobs a printer keeps (the oldest are dropped).
pub const MAX_JOBS: usize = 20;
/// Most text of one print job (bytes).
pub const MAX_JOB_TEXT: usize = 4 * 1024;

/// A page sent to a printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintJob {
    /// The host that sent it.
    pub from: String,
    /// How it got there: `usb` or `network`.
    pub via: String,
    pub text: String,
}

/// What a printer has printed. Empty on every other device, and then not saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Printer {
    /// The last jobs, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<PrintJob>,
    /// How many jobs it has printed in all (the list keeps the last [`MAX_JOBS`]).
    #[serde(skip_serializing_if = "is_zero")]
    pub total: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Printer {
    /// Prints a page: kept as the newest job.
    pub fn print(&mut self, from: &str, via: &str, text: &str) {
        let text: String = text.chars().take(MAX_JOB_TEXT).collect();
        self.jobs.push(PrintJob { from: from.chars().take(MAX_NAME).collect(), via: via.to_string(), text });
        while self.jobs.len() > MAX_JOBS {
            self.jobs.remove(0);
        }
        self.total = self.total.saturating_add(1);
    }

    pub(crate) fn sanitize(&mut self) {
        self.jobs.truncate(MAX_JOBS);
        for j in &mut self.jobs {
            j.text = j.text.chars().take(MAX_JOB_TEXT).collect();
            j.from = j.from.chars().take(MAX_NAME).collect();
            j.via = if j.via == "usb" { "usb".into() } else { "network".into() };
        }
        self.total = self.total.max(u32::try_from(self.jobs.len()).unwrap_or(u32::MAX));
    }
}

/// A USB data cable: a host's USB port to a USB device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbCable {
    /// The PC, laptop, server, router or switch.
    pub host: DeviceId,
    /// Which of the host's USB ports (0-based).
    #[serde(default)]
    pub host_port: u8,
    /// The printer, board, stick, tablet or smartphone.
    pub device: DeviceId,
    /// The host has ejected the device: it stays plugged in but is not mounted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ejected: bool,
}

impl Device {
    /// How many USB ports a cable of a host plugs into (0: not a host).
    pub fn usb_host_ports(&self) -> u8 {
        let m = self.catalogue_model();
        match self.kind {
            DeviceKind::Pc | DeviceKind::Laptop if m.has_rs232() => 2,
            DeviceKind::Server => 2,
            DeviceKind::Router | DeviceKind::Switch if m.has_console() => 1,
            _ => 0,
        }
    }

    /// Whether this is a USB device with a port of its own: a printer, a board, a stick, a tablet
    /// or a smartphone.
    pub fn is_usb_device(&self) -> bool {
        self.kind == DeviceKind::Printer || self.is_usb_stick() || crate::board::is_board(self) || self.catalogue_model().touch()
    }

    /// Whether this is a USB stick.
    pub fn is_usb_stick(&self) -> bool {
        self.catalogue_model().id == STICK_MODEL
    }
}

/// The USB port a name means: `USB`, `USB0` and `USB1` the first, `USB2` the second, and so on
/// (0-based), or none when the name is not a USB port.
pub fn port_index(name: &str) -> Option<u8> {
    let k: String = name.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let n = k.strip_prefix("usb")?;
    if n.is_empty() {
        return Some(0);
    }
    n.parse::<u8>().ok().map(|v| v.saturating_sub(1)).filter(|v| *v < 8)
}

impl Topology {
    /// The cable plugged into a USB device (it has one port).
    pub fn usb_of_device(&self, device: DeviceId) -> Option<&UsbCable> {
        self.usbs.iter().find(|c| c.device == device)
    }

    /// The cables plugged into a host's ports.
    pub fn usbs_of_host(&self, host: DeviceId) -> Vec<UsbCable> {
        self.usbs.iter().filter(|c| c.host == host).copied().collect()
    }

    /// The USB device a host's port holds.
    pub fn usb_in_port(&self, host: DeviceId, port: u8) -> Option<&UsbCable> {
        self.usbs.iter().find(|c| c.host == host && c.host_port == port)
    }

    /// Which end is the host and which the USB device, when two devices can be joined by a USB
    /// data cable (none: a console cable, or no cable, joins them).
    pub fn usb_pair(&self, a: DeviceId, b: DeviceId) -> Option<(DeviceId, DeviceId)> {
        let (da, db) = (self.device(a)?, self.device(b)?);
        if a == b {
            return None;
        }
        if da.usb_host_ports() > 0 && db.is_usb_device() {
            Some((a, b))
        } else if db.usb_host_ports() > 0 && da.is_usb_device() {
            Some((b, a))
        } else {
            None
        }
    }

    /// The ports of a host that nothing is plugged into (a PC's last port is the one a USB
    /// console cable uses, while it has one).
    pub fn usb_free_ports(&self, host: DeviceId) -> Vec<u8> {
        let Some(d) = self.device(host) else { return Vec::new() };
        let ports = d.usb_host_ports();
        let console = self.consoles.iter().any(|c| c.usb && c.pc == host);
        (0..ports).filter(|p| self.usb_in_port(host, *p).is_none() && !(console && p.saturating_add(1) == ports)).collect()
    }

    /// Plugs a USB device into a host's port (the first free one, or `port`).
    pub fn connect_usb_data(&mut self, a: DeviceId, b: DeviceId, port: Option<u8>) -> Result<UsbCable> {
        let name = |t: &Topology, id: DeviceId| t.device(id).map(|d| d.name.clone()).unwrap_or_default();
        if self.device(a).is_none() {
            return Err(ModelError::NoDevice(a.to_string()));
        }
        if self.device(b).is_none() {
            return Err(ModelError::NoDevice(b.to_string()));
        }
        let Some((host, device)) = self.usb_pair(a, b) else {
            return Err(ModelError::Invalid(format!(
                "a USB data cable joins a host's USB port (PC, laptop, server, router or switch) to a USB device (printer, board, USB stick, tablet or smartphone): {} and {} do not fit",
                name(self, a),
                name(self, b)
            )));
        };
        if self.usb_of_device(device).is_some() {
            return Err(ModelError::PortInUse(name(self, device), "USB".into()));
        }
        if self.usbs.len() >= MAX_USB_CABLES {
            return Err(ModelError::Invalid(format!("at most {MAX_USB_CABLES} USB cables")));
        }
        let free = self.usb_free_ports(host);
        let port = match port {
            Some(p) if free.contains(&p) => p,
            Some(p) => return Err(ModelError::PortInUse(name(self, host), format!("USB{}", u16::from(p) + 1))),
            None => match free.first() {
                Some(p) => *p,
                None => {
                    let n = self.device(host).map_or(0, Device::usb_host_ports);
                    return Err(ModelError::Invalid(format!("{} has no free USB port ({n} of {n} in use)", name(self, host))));
                }
            },
        };
        let c = UsbCable { host, host_port: port, device, ejected: false };
        self.usbs.push(c);
        Ok(c)
    }

    /// Unplugs a USB device; whether it was plugged in.
    pub fn disconnect_usb_data(&mut self, device: DeviceId) -> bool {
        let before = self.usbs.len();
        self.usbs.retain(|c| c.device != device);
        self.usbs.len() != before
    }

    /// The stick a host has mounted: the first one plugged into it and not ejected.
    pub fn stick_of_host(&self, host: DeviceId) -> Option<DeviceId> {
        self.usbs.iter().filter(|c| c.host == host && !c.ejected).map(|c| c.device).find(|d| self.device(*d).is_some_and(Device::is_usb_stick))
    }

    /// Why a USB cable carries nothing: a device at one end is off, or it is longer than 5 m.
    pub fn usb_problem(&self, c: &UsbCable) -> Option<String> {
        if [c.host, c.device].into_iter().any(|d| self.device(d).is_some_and(|x| x.powered_off)) {
            return Some("a device at one end is powered off".into());
        }
        self.run_problem(crate::cable::Cable::Usb, c.host, c.device)
    }

    /// A stick's host while it is mounted: plugged into a host that has not ejected it.
    pub fn stick_host(&self, stick: DeviceId) -> Option<DeviceId> {
        self.usb_of_device(stick).filter(|c| !c.ejected).map(|c| c.host)
    }

    /// Repairs the USB side of a hostile file: real devices of the right kinds, one cable to a
    /// device and to a host's port, and sticks that hold only what a stick can.
    pub(crate) fn sanitize_usb(&mut self) {
        self.usbs.truncate(MAX_USB_CABLES);
        let mut kept: Vec<UsbCable> = Vec::new();
        for c in std::mem::take(&mut self.usbs) {
            let ok = self.device(c.host).is_some_and(|d| c.host_port < d.usb_host_ports())
                && self.device(c.device).is_some_and(Device::is_usb_device)
                && !kept.iter().any(|k| k.device == c.device || (k.host == c.host && k.host_port == c.host_port));
            if ok {
                kept.push(c);
            }
        }
        self.usbs = kept;
        for d in &mut self.devices {
            d.usb.sanitize();
            if !d.is_usb_stick() {
                d.usb = Stick::default();
            }
            d.printer.sanitize();
            if d.kind != DeviceKind::Printer {
                d.printer = Printer::default();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{file, models};

    fn lab() -> (Topology, DeviceId, DeviceId, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let sw = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let printer = t.add_device(DeviceKind::Printer, None, 0.0, 0.0).unwrap();
        let stick = t.add_model(models::find(STICK_MODEL).unwrap(), None, 0.0, 0.0).unwrap();
        (t, pc, sw, printer, stick)
    }

    #[test]
    fn the_catalogue_says_who_hosts_and_who_plugs_in() {
        let (t, pc, sw, printer, stick) = lab();
        let host = |d: DeviceId| t.device(d).unwrap().usb_host_ports();
        assert_eq!((host(pc), host(sw), host(printer), host(stick)), (2, 1, 0, 0));
        assert!(t.device(printer).unwrap().is_usb_device() && t.device(stick).unwrap().is_usb_stick());
        assert!(!t.device(pc).unwrap().is_usb_device() && !t.device(sw).unwrap().is_usb_device());
        for (id, host_ports, device) in [
            ("Laptop-WiFi", 2, false),
            ("Server", 2, false),
            ("Tablet", 0, true),
            ("Smartphone", 0, true),
            ("Board-SBC", 0, true),
            ("Board-MCU", 0, true),
            ("Home-Router", 0, false),
            ("Hub-8", 0, false),
        ] {
            let m = models::find(id).unwrap_or_else(|| panic!("no model {id}"));
            let mut t = Topology::new();
            let d = t.add_model(m, None, 0.0, 0.0).unwrap();
            let d = t.device(d).unwrap();
            assert_eq!((d.usb_host_ports(), d.is_usb_device()), (host_ports, device), "{id}");
        }
        let model = models::find(STICK_MODEL).unwrap();
        assert_eq!(model.kind, DeviceKind::Iot);
        assert!(model.port_names().is_empty(), "a stick has no network port");
        assert_eq!(t.device(pc).unwrap().usb, Stick::default());
        assert_eq!(t.device(stick).unwrap().usb, Stick::fresh());
    }

    #[test]
    fn a_usb_data_cable_joins_a_host_port_to_a_device() {
        let (mut t, pc, sw, printer, stick) = lab();
        // Either order; the host takes its first free port.
        let c = t.connect_usb_data(printer, pc, None).unwrap();
        assert_eq!((c.host, c.host_port, c.device), (pc, 0, printer));
        let c = t.connect_usb_data(pc, stick, None).unwrap();
        assert_eq!(c.host_port, 1);
        // Two hosts, two devices, a device twice, a full host: refused, nothing changes.
        let (pc2, printer2) = (t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap(), t.add_device(DeviceKind::Printer, None, 0.0, 0.0).unwrap());
        assert!(t.connect_usb_data(pc, pc2, None).is_err() && t.connect_usb_data(printer, printer2, None).is_err());
        assert!(t.connect_usb_data(sw, pc, None).is_err(), "a PC and a switch are joined by a console cable");
        assert!(t.connect_usb_data(pc, pc, None).is_err());
        assert!(t.connect_usb_data(pc2, printer, None).is_err(), "the printer's port is taken");
        let e = t.connect_usb_data(pc, printer2, None).unwrap_err().to_string();
        assert!(e.contains("no free USB port"), "{e}");
        assert!(t.connect_usb_data(pc2, printer2, Some(5)).is_err(), "no such port");
        assert_eq!(t.usbs.len(), 2);
        // A switch has one port, for a stick.
        let stick2 = t.add_model(models::find(STICK_MODEL).unwrap(), None, 0.0, 0.0).unwrap();
        t.connect_usb_data(stick2, sw, None).unwrap();
        assert!(t.connect_usb_data(sw, printer2, None).is_err());
        // A port can be asked for, and a USB console cable keeps one of a laptop's ports.
        t.connect_usb_data(pc2, printer2, Some(1)).unwrap();
        assert_eq!(t.usb_in_port(pc2, 1).map(|c| c.device), Some(printer2));
        let laptop = t.add_model(models::find("Laptop-WiFi").unwrap(), None, 0.0, 0.0).unwrap();
        let router = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        t.connect_usb(laptop, router).unwrap();
        assert_eq!(t.usb_free_ports(laptop), vec![0], "the console cable has the last port");
        // Unplugging, and a device that goes takes its cable.
        assert!(t.disconnect_usb_data(printer) && !t.disconnect_usb_data(printer));
        t.remove_device(stick).unwrap();
        assert!(t.usb_of_device(stick).is_none());
        t.remove_device(pc2).unwrap();
        assert!(t.usbs.iter().all(|c| c.host != pc2 && c.device != printer2));
    }

    #[test]
    fn a_stick_keeps_a_small_list_of_files() {
        let mut s = Stick::fresh();
        assert_eq!(s.files.len(), 1);
        s.put("notes.txt", "hello").unwrap();
        s.put("NOTES.TXT", "hello again").unwrap();
        assert_eq!(s.files.len(), 2, "names ignore case: the second put replaced the first");
        assert_eq!(s.find("notes.txt").map(|f| f.text.as_str()), Some("hello again"));
        assert_eq!(s.used(), 25 + 11);
        assert!(s.delete("Notes.txt") && !s.delete("notes.txt"));
        for bad in ["", ".hidden", "a/b", "a\\b", "c:x", "a*", "a b ", " a", "x\u{7}y", &"n".repeat(MAX_NAME + 1)] {
            assert!(s.put(bad, "x").is_err(), "{bad:?}");
        }
        assert!(s.put("big", &"x".repeat(MAX_FILE_TEXT + 1)).is_err());
        let mut full = Stick::default();
        for i in 0..MAX_FILES {
            full.put(&format!("f{i}"), "x").unwrap();
        }
        assert!(full.put("one-more", "x").is_err(), "at most {MAX_FILES} files");
        assert!(full.put("f3", "replaced").is_ok(), "a replacement is not a new file");
        let mut heavy = Stick::default();
        for i in 0..8 {
            heavy.put(&format!("h{i}"), &"y".repeat(MAX_FILE_TEXT)).unwrap();
        }
        assert!(heavy.put("h8", "y").is_err(), "the stick is full");
        assert!(heavy.put("h0", &"z".repeat(MAX_FILE_TEXT)).is_ok());
    }

    #[test]
    fn the_cables_and_the_files_are_saved_and_a_hostile_file_is_repaired() {
        let (mut t, pc, _, printer, stick) = lab();
        t.connect_usb_data(pc, printer, None).unwrap();
        t.connect_usb_data(pc, stick, None).unwrap();
        t.usbs[1].ejected = true;
        t.device_mut(stick).unwrap().usb.put("a.txt", "one").unwrap();
        let text = file::to_json(&t).unwrap();
        assert_eq!(file::from_json(&text).unwrap(), t);
        let (plain_lab, ..) = lab();
        let plain = file::to_json(&plain_lab).unwrap();
        assert!(!plain.contains("usbs"), "a lab with no USB cable saves none");
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let cable = |h: u32, p: u32, d: u32| serde_json::json!({"host": h, "host_port": p, "device": d});
        v["topology"]["usbs"] = serde_json::json!([
            cable(pc.0, 0, printer.0),
            cable(pc.0, 0, stick.0),
            cable(pc.0, 1, printer.0),
            cable(pc.0, 7, stick.0),
            cable(printer.0, 0, stick.0),
            cable(999, 0, stick.0),
            cable(pc.0, 1, 999),
            cable(pc.0, 1, stick.0),
        ]);
        let dev = v["topology"]["devices"].as_array_mut().unwrap();
        for d in dev.iter_mut() {
            if d["id"] == serde_json::json!(stick.0) {
                d["usb"] = serde_json::json!({"files": [
                    {"name": "ok.txt", "text": "fine"},
                    {"name": "../escape", "text": "no"},
                    {"name": "OK.TXT", "text": "a duplicate"},
                    {"name": "big.bin", "text": "x".repeat(MAX_FILE_TEXT + 1)}
                ]});
            }
            if d["id"] == serde_json::json!(pc.0) {
                d["usb"] = serde_json::json!({"files": [{"name": "not-a-stick.txt", "text": "x"}]});
            }
        }
        let r = file::from_json(&v.to_string()).unwrap();
        assert_eq!(r.usbs.iter().map(|c| (c.host, c.host_port, c.device)).collect::<Vec<_>>(), vec![(pc, 0, printer), (pc, 1, stick)]);
        assert_eq!(r.device(stick).unwrap().usb.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["ok.txt"]);
        assert!(r.device(pc).unwrap().usb.files.is_empty(), "only a stick holds files");
    }

    #[test]
    fn port_names_say_which_usb_port() {
        for (name, want) in [
            ("USB", Some(0)),
            ("usb0", Some(0)),
            ("USB1", Some(0)),
            ("USB 2", Some(1)),
            ("usb8", Some(7)),
            ("USB9", None),
            ("USB x", None),
            ("Usb Console", None),
            ("Fa0/1", None),
        ] {
            assert_eq!(port_index(name), want, "{name}");
        }
    }

    #[test]
    fn a_printer_keeps_its_last_jobs_and_counts_them_all() {
        let mut p = Printer::default();
        for i in 0..MAX_JOBS + 5 {
            p.print("PC1", if i % 2 == 0 { "usb" } else { "network" }, &format!("page {i}"));
        }
        assert_eq!((p.jobs.len(), p.total as usize), (MAX_JOBS, MAX_JOBS + 5));
        assert_eq!(p.jobs[0].text, "page 5");
        p.print("PC1", "usb", &"x".repeat(MAX_JOB_TEXT + 10));
        assert_eq!(p.jobs.last().map(|j| j.text.len()), Some(MAX_JOB_TEXT));
        let (mut t, pc, _, printer, stick) = lab();
        t.device_mut(printer).unwrap().printer.print("PC1", "usb", "hello");
        t.device_mut(pc).unwrap().printer.print("x", "usb", "not a printer");
        let mut v: serde_json::Value = serde_json::from_str(&file::to_json(&t).unwrap()).unwrap();
        let _ = stick;
        let text = v.to_string();
        let r = file::from_json(&text).unwrap();
        assert_eq!(r.device(printer).unwrap().printer.jobs.len(), 1);
        assert!(r.device(pc).unwrap().printer.jobs.is_empty(), "only a printer prints");
        v["topology"]["devices"][2]["printer"] = serde_json::json!({"jobs": [{"from": "A", "via": "carrier pigeon", "text": "t"}], "total": 0});
        let r = file::from_json(&v.to_string()).unwrap();
        let job = &r.device(printer).unwrap().printer;
        assert_eq!((job.jobs[0].via.as_str(), job.total), ("network", 1));
    }
}
