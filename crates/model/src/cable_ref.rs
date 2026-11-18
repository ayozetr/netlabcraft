//! A stable name for any cable.
//!
//! A network cable is a [`Link`] and its first [`End`] names it, but the cables kept apart from
//! `links` (console and USB, octal, and the IoT custom cable) have no `End`: they join a device,
//! not a numbered port. [`CableRef`] names every kind of cable the same way, so the interface can
//! select one, open its window and delete it without caring where it is stored.
//!
//! The name is made of what is unique to the cable: a console port takes one cable, an octal
//! line ends at one Console port, and a smart thing is wired to one pin. Nothing is stored: a
//! reference to a cable that is gone resolves to nothing.

use netlabcraft_core::DeviceId;

use crate::cable::{CONSOLE_PORT, Cable, RS232_PORT, USB_CONSOLE_PORT, USB_PORT};
use crate::{End, Topology};

/// Names one cable of any kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CableRef {
    /// A network cable, by the first end of its link.
    Link(End),
    /// A console or USB cable, by the router or switch at its Console end.
    Console(DeviceId),
    /// An octal line, by the router or switch at its Console end.
    Octal(DeviceId),
    /// An IoT custom cable, by the board and the smart thing it reaches.
    Pin { board: DeviceId, thing: DeviceId },
}

/// What a cable joins, for a window or a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CableInfo {
    /// The kind of cable (console, USB, octal, IoT custom, or what the network cable resolved to).
    pub kind: Cable,
    /// The device at the first end and the device at the second.
    pub a: DeviceId,
    pub b: DeviceId,
    /// Each end as `Device Port`.
    pub a_label: String,
    pub b_label: String,
}

impl CableRef {
    /// The first end of a network cable (`None` for the other kinds).
    pub fn link(self) -> Option<End> {
        match self {
            CableRef::Link(e) => Some(e),
            _ => None,
        }
    }
}

impl Topology {
    /// The name of the network cable plugged into `end` (by its first end).
    pub fn cable_ref_at(&self, end: End) -> Option<CableRef> {
        self.link_at(end).map(|l| CableRef::Link(l.a))
    }

    /// Every cable of the lab, network cables first, then console and USB, octal, and IoT custom.
    pub fn cable_refs(&self) -> Vec<CableRef> {
        let mut out: Vec<CableRef> = self.links.iter().map(|l| CableRef::Link(l.a)).collect();
        out.extend(self.consoles.iter().map(|c| CableRef::Console(c.device)));
        out.extend(self.octals.iter().map(|o| CableRef::Octal(o.device)));
        for b in &self.devices {
            out.extend(b.board.wires.iter().map(|w| CableRef::Pin { board: b.id, thing: w.thing }));
        }
        out
    }

    /// What a cable joins, or `None` when it is gone.
    pub fn cable_info(&self, r: CableRef) -> Option<CableInfo> {
        let name = |id: DeviceId| self.device(id).map(|d| d.display_name().to_string());
        match r {
            CableRef::Link(end) => {
                let l = self.link_at(end)?;
                Some(CableInfo {
                    kind: self.resolved_cable(l),
                    a: l.a.device,
                    b: l.b.device,
                    a_label: self.end_label(l.a),
                    b_label: self.end_label(l.b),
                })
            }
            CableRef::Console(device) => {
                let c = self.consoles.iter().find(|c| c.device == device)?;
                let (host, dev) = if c.usb { (USB_PORT, USB_CONSOLE_PORT) } else { (RS232_PORT, CONSOLE_PORT) };
                Some(CableInfo {
                    kind: if c.usb { Cable::Usb } else { Cable::Console },
                    a: c.pc,
                    b: c.device,
                    a_label: format!("{} {host}", name(c.pc)?),
                    b_label: format!("{} {dev}", name(c.device)?),
                })
            }
            CableRef::Octal(device) => {
                let o = self.octals.iter().find(|o| o.device == device)?;
                Some(CableInfo {
                    kind: Cable::Octal,
                    a: o.router,
                    b: o.device,
                    a_label: format!("{} Async line {}", name(o.router)?, o.line),
                    b_label: format!("{} {CONSOLE_PORT}", name(o.device)?),
                })
            }
            CableRef::Pin { board, thing } => {
                let w = self.device(board)?.board.wires.iter().find(|w| w.thing == thing)?;
                Some(CableInfo { kind: Cable::IotCustom, a: board, b: thing, a_label: format!("{} {}", name(board)?, w.pin), b_label: name(thing)? })
            }
        }
    }

    /// The device and port name `link.remove` takes to unplug a cable.
    pub fn cable_removal(&self, r: CableRef) -> Option<(DeviceId, String)> {
        match r {
            CableRef::Link(end) => {
                let l = self.link_at(end)?;
                let port = self.device(l.a.device)?.ports.get(l.a.port)?.name.clone();
                Some((l.a.device, port))
            }
            CableRef::Console(device) => self.consoles.iter().any(|c| c.device == device).then(|| (device, CONSOLE_PORT.to_string())),
            CableRef::Octal(device) => self.octals.iter().any(|o| o.device == device).then(|| (device, CONSOLE_PORT.to_string())),
            CableRef::Pin { board, thing } => {
                let w = self.device(board)?.board.wires.iter().find(|w| w.thing == thing)?;
                Some((board, w.pin.clone()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, models};

    #[test]
    fn every_kind_of_cable_has_a_name_that_resolves_and_goes_away_with_it() {
        let mut t = Topology::new();
        let r1 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let r2 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let s1 = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let a = End { device: r1, port: 0 };
        let b = End { device: r2, port: 0 };
        t.connect_with(a, b, Cable::Auto).unwrap();
        t.connect_console(pc, s1).unwrap();
        assert_eq!(t.cable_ref_at(b), Some(CableRef::Link(a)), "either end names the cable by its first end");
        assert_eq!(t.cable_refs(), vec![CableRef::Link(a), CableRef::Console(s1)]);
        let info = t.cable_info(CableRef::Console(s1)).unwrap();
        assert_eq!((info.kind, info.a, info.b), (Cable::Console, pc, s1));
        assert!(info.a_label.ends_with(RS232_PORT) && info.b_label.ends_with(CONSOLE_PORT));
        assert_eq!(t.cable_removal(CableRef::Console(s1)), Some((s1, CONSOLE_PORT.to_string())));
        let link = t.cable_info(CableRef::Link(a)).unwrap();
        assert_eq!((link.a, link.b), (r1, r2));
        assert_eq!(t.cable_removal(CableRef::Link(a)).map(|(d, _)| d), Some(r1));
        assert_eq!(CableRef::Link(a).link(), Some(a));
        assert_eq!(CableRef::Console(s1).link(), None);
        t.disconnect_console(s1);
        assert!(t.cable_info(CableRef::Console(s1)).is_none() && t.cable_removal(CableRef::Console(s1)).is_none());
        assert_eq!(t.cable_refs(), vec![CableRef::Link(a)]);
    }

    #[test]
    fn octal_usb_and_pin_cables_are_named_too() {
        let mut t = Topology::new();
        let m = t.add_model(models::find("Router-M2").unwrap(), None, 0.0, 0.0).unwrap();
        let r2 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let s1 = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        t.device_mut(m).unwrap().powered_off = true;
        t.set_module(m, "0/1", Some(crate::cable::OCTAL_CARD)).unwrap();
        t.connect_octal(m, s1, Some(3)).unwrap();
        t.connect_usb(pc, r2).unwrap();
        let o = t.cable_info(CableRef::Octal(s1)).unwrap();
        assert_eq!((o.kind, o.a, o.b), (Cable::Octal, m, s1));
        assert!(o.a_label.ends_with("Async line 3"));
        assert_eq!(t.cable_removal(CableRef::Octal(s1)), Some((s1, CONSOLE_PORT.to_string())));
        let u = t.cable_info(CableRef::Console(r2)).unwrap();
        assert_eq!(u.kind, Cable::Usb);
        assert!(u.a_label.ends_with(USB_PORT) && u.b_label.ends_with(USB_CONSOLE_PORT));
        assert_eq!(t.cable_refs().len(), 2);
        // A name that points at nothing resolves to nothing.
        assert!(t.cable_info(CableRef::Octal(r2)).is_none());
        assert!(t.cable_info(CableRef::Pin { board: m, thing: pc }).is_none());
        assert!(t.cable_removal(CableRef::Pin { board: m, thing: pc }).is_none());
        assert!(t.cable_info(CableRef::Link(End { device: DeviceId(9999), port: 3 })).is_none());
    }
}
