//! Cable types and what each port takes.
//!
//! Every port has a [`Medium`] that comes from its model (or the card it sits on): copper,
//! fiber, a combo uplink that takes either, serial, or a radio. A cable joins two ports of its
//! medium. Copper cables come straight-through or cross-over: two ports wired alike (two
//! switches, two routers, a router and a PC) need a cross-over, unlike ports a straight-through,
//! unless one end swaps its pairs by itself (auto-MDIX, on by default on routers and switches
//! here, never on PCs, servers or hubs). The automatic cable is always the right kind.
//!
//! Console cables are apart: they join a PC's RS-232 port to a router's or switch's Console
//! port, which are not network interfaces (no frames, not in the interface tables), and are
//! kept in [`Topology::consoles`]. A USB cable is the same idea over USB: a PC's or laptop's USB
//! port to a router's or switch's USB console port, also kept in [`Topology::consoles`] (flagged
//! `usb`). An octal cable fans one router's octal asynchronous card out to the Console ports of up
//! to eight devices ([`Topology::octals`]).
//!
//! Fiber comes in two kinds. A multi-mode cable (850 nm light, 1000BASE-SX) reaches
//! [`FIBER_MULTI_MAX_M`] here; a single-mode cable (1310 nm, 1000BASE-LX/LX10) reaches
//! [`FIBER_SINGLE_MAX_M`]. Each fiber port holds a transceiver that is multi-mode unless its
//! `single_mode` flag is set; both ends of a fiber link need the same transceiver, and a
//! multi-mode transceiver cannot drive a single-mode cable (see [`Topology::fiber_problem`]).

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, End, Link, ModelError, Result, Topology, ifname, models};

/// How far a coaxial cable and a phone line carry a signal here (metres).
pub const COAX_MAX_M: u32 = 500;
pub const PHONE_MAX_M: u32 = 5_000;

/// How far a multi-mode fiber run carries a gigabit signal (metres; 1000BASE-SX over 50/125 um).
pub const FIBER_MULTI_MAX_M: u32 = 550;
/// How far a single-mode fiber run carries a gigabit signal (metres; 1000BASE-LX10).
pub const FIBER_SINGLE_MAX_M: u32 = 10_000;
/// The id of the octal asynchronous card of a modular router.
pub const OCTAL_CARD: &str = "Card-8A";
/// The asynchronous lines of one octal card.
pub const OCTAL_LINES: u8 = 8;
/// The name of a router's or switch's USB console port.
pub const USB_CONSOLE_PORT: &str = "USB Console";
/// The name of a PC's or laptop's USB port.
pub const USB_PORT: &str = "USB";

/// The name of a router's or switch's console port.
pub const CONSOLE_PORT: &str = "Console";
/// The name of a PC's or laptop's serial port for a console cable.
pub const RS232_PORT: &str = "RS-232";

/// A cable kind, as picked in the Connections group or given to `link.add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Cable {
    /// Picks the right kind for the two ports (what every cable was before cable types).
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "straight-through")]
    Straight,
    #[serde(rename = "cross-over")]
    Cross,
    /// A single-mode fiber cable (1310 nm light, long runs).
    #[serde(rename = "fiber-single-mode")]
    FiberSingle,
    /// A multi-mode fiber cable (850 nm light, short runs). Labs saved before the two kinds
    /// existed call their fiber cables `fiber`, which reads as this.
    #[serde(rename = "fiber-multi-mode", alias = "fiber")]
    FiberMulti,
    /// A rollover cable from a PC's RS-232 port to a device's Console port.
    #[serde(rename = "console")]
    Console,
    /// A serial cable; the first end is the DCE (it gives the clock).
    #[serde(rename = "serial-dce")]
    SerialDce,
    /// A serial cable; the first end is the DTE, the far end the DCE.
    #[serde(rename = "serial-dte")]
    SerialDte,
    /// A coaxial cable (cable modems and the cable provider's cloud).
    #[serde(rename = "coaxial")]
    Coaxial,
    /// A phone line (RJ-11: DSL modems and the phone company's cloud).
    #[serde(rename = "phone")]
    Phone,
    /// An IoT custom cable: a programmable board's pin to a smart thing (not a network link).
    #[serde(rename = "iot-custom")]
    IotCustom,
    /// An octal cable: a router's octal card to the Console ports of up to eight devices.
    #[serde(rename = "octal")]
    Octal,
    /// A USB cable: a PC's or laptop's USB port to a router's or switch's USB console port.
    #[serde(rename = "usb")]
    Usb,
}

impl Cable {
    /// The Connections tiles, in the order the group shows them.
    pub const ALL: [Cable; 13] = [
        Cable::Auto,
        Cable::Console,
        Cable::Straight,
        Cable::Cross,
        Cable::FiberSingle,
        Cable::FiberMulti,
        Cable::Phone,
        Cable::Coaxial,
        Cable::SerialDce,
        Cable::SerialDte,
        Cable::Octal,
        Cable::IotCustom,
        Cable::Usb,
    ];

    pub fn is_auto(&self) -> bool {
        *self == Cable::Auto
    }

    /// The id lab files and commands use.
    pub fn id(self) -> &'static str {
        match self {
            Cable::Auto => "auto",
            Cable::Straight => "straight-through",
            Cable::Cross => "cross-over",
            Cable::FiberSingle => "fiber-single-mode",
            Cable::FiberMulti => "fiber-multi-mode",
            Cable::Console => "console",
            Cable::SerialDce => "serial-dce",
            Cable::SerialDte => "serial-dte",
            Cable::Coaxial => "coaxial",
            Cable::Phone => "phone",
            Cable::IotCustom => "iot-custom",
            Cable::Octal => "octal",
            Cable::Usb => "usb",
        }
    }

    /// The name shown on the Connections tile.
    pub fn label(self) -> &'static str {
        match self {
            Cable::Auto => "Automatically Choose Connection Type",
            Cable::Straight => "Copper Straight-Through",
            Cable::Cross => "Copper Cross-Over",
            Cable::FiberSingle => "Single Mode Fiber",
            Cable::FiberMulti => "Multi Mode Fiber",
            Cable::Console => "Console",
            Cable::SerialDce => "Serial DCE",
            Cable::SerialDte => "Serial DTE",
            Cable::Coaxial => "Coaxial",
            Cable::Phone => "Phone",
            Cable::IotCustom => "IoT Custom Cable",
            Cable::Octal => "Octal",
            Cable::Usb => "USB",
        }
    }

    /// One line on what the cable is for.
    pub fn description(self) -> &'static str {
        match self {
            Cable::Auto => "Automatically choose the connection type: picks the right cable for the two ports you choose",
            Cable::Straight => "Copper straight-through: joins unlike ports (a PC or router to a switch or hub)",
            Cable::Cross => "Copper cross-over: joins like ports (switch to switch, router to router, PC to PC, router to PC)",
            Cable::FiberSingle => {
                "Single mode fiber: joins fiber ports whose transceivers are single mode; carries a gigabit signal up to 10 km here"
            }
            Cable::FiberMulti => "Multi mode fiber: joins fiber ports (SFP uplinks and fiber cards); carries a gigabit signal up to 550 m here",
            Cable::Console => "Console (rollover): a PC's RS-232 port to a router's or switch's Console port, for the PC's Terminal",
            Cable::SerialDce => "Serial DCE: joins serial ports; the end you click first is the DCE and gives the clock",
            Cable::SerialDte => "Serial DTE: joins serial ports; the end you click first is the DTE, the other end gives the clock",
            Cable::Coaxial => "Coaxial: joins coaxial ports (a cable modem to the cable provider's cloud); carries a signal up to 500 m here",
            Cable::Phone => "Phone line (RJ-11): joins phone ports (a DSL modem to the phone company's cloud); carries a signal up to 5 km here",
            Cable::IotCustom => {
                "IoT custom cable: joins a programmable board's pin to a smart thing, which the board's program then reads or switches"
            }
            Cable::Octal => {
                "Octal: joins one line of a router's octal card (Card-8A) to the Console port of another router or switch, for reverse-style console access"
            }
            Cable::Usb => "USB: a PC's or laptop's USB port to a router's or switch's USB console port, for the PC's Terminal",
        }
    }

    /// The cable a person typed: its id, its label or a common alias.
    pub fn parse(s: &str) -> Option<Cable> {
        let k: String = s.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        Some(match k.as_str() {
            "auto" | "automatic" => Cable::Auto,
            "straight" | "straightthrough" | "copperstraightthrough" | "copper" => Cable::Straight,
            "cross" | "crossover" | "coppercrossover" => Cable::Cross,
            "fiber" | "fibre" | "multimode" | "multimodefiber" | "multimodefibre" | "mmf" | "fibermulti" | "fibermultimode" => Cable::FiberMulti,
            "singlemode" | "singlemodefiber" | "singlemodefibre" | "smf" | "fibersingle" | "fibersinglemode" => Cable::FiberSingle,
            "octal" | "octalcable" | "async" => Cable::Octal,
            "usb" | "usbconsole" => Cable::Usb,
            "console" | "rollover" => Cable::Console,
            "serial" | "serialdce" | "dce" => Cable::SerialDce,
            "serialdte" | "dte" => Cable::SerialDte,
            "coaxial" | "coax" => Cable::Coaxial,
            "phone" | "rj11" | "phoneline" | "phonerj11" | "dsl" => Cable::Phone,
            "iotcustom" | "iotcustomcable" | "iot" | "custom" | "pin" => Cable::IotCustom,
            "automaticallychooseconnectiontype" => Cable::Auto,
            _ => return None,
        })
    }

    pub fn is_serial(self) -> bool {
        matches!(self, Cable::SerialDce | Cable::SerialDte)
    }

    pub fn is_copper(self) -> bool {
        matches!(self, Cable::Straight | Cable::Cross)
    }

    pub fn is_fiber(self) -> bool {
        matches!(self, Cable::FiberSingle | Cable::FiberMulti)
    }

    /// The cables that join a PC to a console port apart from the network links: console and USB.
    pub fn is_console_like(self) -> bool {
        matches!(self, Cable::Console | Cable::Usb)
    }

    /// The cables kept apart from `links` (console, USB, octal and the IoT custom cable).
    pub fn is_off_network(self) -> bool {
        matches!(self, Cable::Console | Cable::Usb | Cable::Octal | Cable::IotCustom)
    }
}

/// What a port takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Medium {
    /// Twisted-pair copper (RJ-45).
    Copper,
    Fiber,
    /// An uplink with both a copper socket and a fiber (SFP) cage.
    Combo,
    Serial,
    /// A router's or switch's console port.
    Console,
    /// A PC's serial port.
    Rs232,
    /// A radio: takes no cable.
    Wireless,
    /// A coaxial socket (F connector).
    Coaxial,
    /// A phone socket (RJ-11).
    Phone,
}

impl Medium {
    pub fn id(self) -> &'static str {
        match self {
            Medium::Copper => "copper",
            Medium::Fiber => "fiber",
            Medium::Combo => "combo",
            Medium::Serial => "serial",
            Medium::Console => "console",
            Medium::Rs232 => "rs-232",
            Medium::Wireless => "wireless",
            Medium::Coaxial => "coaxial",
            Medium::Phone => "phone",
        }
    }

    /// The words an error uses (`Fa0/1 is copper`).
    pub fn words(self) -> &'static str {
        match self {
            Medium::Copper => "copper",
            Medium::Fiber => "fiber",
            Medium::Combo => "copper or fiber",
            Medium::Serial => "serial",
            Medium::Console => "a console port",
            Medium::Rs232 => "an RS-232 port",
            Medium::Wireless => "a radio",
            Medium::Coaxial => "coaxial",
            Medium::Phone => "a phone line (RJ-11)",
        }
    }

    fn copper(self) -> bool {
        matches!(self, Medium::Copper | Medium::Combo)
    }

    fn fiber(self) -> bool {
        matches!(self, Medium::Fiber | Medium::Combo)
    }
}

/// A console cable: a PC's RS-232 port to a device's Console port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleCable {
    /// The PC or laptop (its RS-232 port).
    pub pc: DeviceId,
    /// The router or switch (its Console port).
    pub device: DeviceId,
    /// A USB cable (the PC's USB port to the device's USB console port) instead of a rollover.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub usb: bool,
}

/// An octal cable: one line of a router's octal card to the Console port of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OctalCable {
    /// The router with the octal card.
    pub router: DeviceId,
    /// The line of the router's octal cards (0 to 7 on the first card).
    pub line: u8,
    /// The router or switch whose Console port the line reaches.
    pub device: DeviceId,
}

impl Device {
    /// What a port takes, from the model or the card it sits on.
    pub fn port_medium(&self, port: usize) -> Option<Medium> {
        let name = &self.ports.get(port)?.name;
        if let Some(m) = self.port_slot(name).and_then(|s| self.module_in(s)) {
            return Some(m.medium.unwrap_or_else(|| models::medium_by_name(name)));
        }
        Some(self.catalogue_model().port_medium(name).unwrap_or_else(|| models::medium_by_name(name)))
    }

    pub fn has_console(&self) -> bool {
        self.catalogue_model().has_console()
    }

    pub fn has_rs232(&self) -> bool {
        self.catalogue_model().has_rs232()
    }

    /// Whether the device takes a USB cable for its console: routers and switches with a command
    /// line (their USB console port) and PCs and laptops (a USB port), the same devices as the
    /// rollover console.
    pub fn has_usb(&self) -> bool {
        self.has_console() || self.has_rs232()
    }

    /// How many asynchronous lines the octal cards fitted in the device give (8 per card).
    pub fn octal_lines(&self) -> u8 {
        let cards = self.modules.values().filter(|m| m.as_str() == OCTAL_CARD).count();
        u8::try_from(cards.saturating_mul(OCTAL_LINES as usize)).unwrap_or(u8::MAX)
    }

    /// The console-cable port this device has: `Console`, `RS-232`, or none.
    pub fn console_port_name(&self) -> Option<&'static str> {
        if self.has_console() {
            Some(CONSOLE_PORT)
        } else if self.has_rs232() {
            Some(RS232_PORT)
        } else {
            None
        }
    }
}

/// Whether a port name means the console-cable port (`Console`, `con`, `RS-232`, `rs232`, `com1`).
pub fn is_console_port_name(name: &str) -> bool {
    let k: String = name.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    matches!(k.as_str(), "console" | "con" | "con0" | "rs232" | "com" | "com1") || is_usb_port_name(name)
}

/// Whether a port name means the USB port of a PC or the USB console port of a router or switch.
pub fn is_usb_port_name(name: &str) -> bool {
    let k: String = name.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    matches!(k.as_str(), "usb" | "usb0" | "usbconsole")
}

/// Whether a port is wired as MDI-X (switch and hub ports, the phone's computer port): such a
/// port takes a straight-through cable from an MDI port (a router, a host, an access point).
pub fn mdi_x(t: &Topology, end: End) -> bool {
    let Some(d) = t.device(end.device) else { return false };
    let Some(p) = d.ports.get(end.port) else { return false };
    match d.kind {
        DeviceKind::Hub => true,
        DeviceKind::Switch => !matches!(d.catalogue_model().id, "AccessPoint" | "LAP" | "WLC") && p.name != "Internet",
        DeviceKind::Phone => end.port == 1,
        _ => false,
    }
}

/// Whether a port swaps its pairs by itself: routers and switches, unless `no mdix auto`.
pub fn auto_mdix(t: &Topology, end: End) -> bool {
    t.device(end.device)
        .filter(|d| matches!(d.kind, DeviceKind::Router | DeviceKind::Switch))
        .and_then(|d| d.ports.get(end.port))
        .is_some_and(|p| !p.mdix_off)
}

/// The copper cable two ports need without auto-MDIX: cross-over for like ports.
pub fn copper_needed(t: &Topology, a: End, b: End) -> Cable {
    if mdi_x(t, a) == mdi_x(t, b) { Cable::Cross } else { Cable::Straight }
}

impl Topology {
    /// What a port takes.
    pub fn port_medium(&self, end: End) -> Option<Medium> {
        self.device(end.device)?.port_medium(end.port)
    }

    /// `Device Gi0/1` for cable messages.
    fn short_label(&self, end: End) -> String {
        match self.device(end.device) {
            Some(d) => format!("{} {}", d.name, d.ports.get(end.port).map_or("?".to_string(), |p| ifname::short(&p.name))),
            None => format!("{} ?", end.device),
        }
    }

    /// Why `cable` can't join these two ports, if it can't.
    pub fn cable_misfit(&self, cable: Cable, a: End, b: End) -> Option<String> {
        let (ma, mb) = (self.port_medium(a)?, self.port_medium(b)?);
        for (e, m) in [(a, ma), (b, mb)] {
            if m == Medium::Wireless {
                return Some(format!("{} is a radio: it takes no cable", self.short_label(e)));
            }
        }
        let need = |name: &str, fits: fn(Medium) -> bool, port: &str| {
            [(a, ma), (b, mb)]
                .into_iter()
                .find(|(_, m)| !fits(*m))
                .map(|(e, m)| format!("a {name} cable needs a {port} port on both ends: {} is {}", self.short_label(e), m.words()))
        };
        match cable {
            Cable::Straight | Cable::Cross => need(cable.label().trim_start_matches("Copper ").to_lowercase().as_str(), Medium::copper, "copper"),
            Cable::FiberSingle => need("single mode fiber", Medium::fiber, "fiber"),
            Cable::FiberMulti => need("multi mode fiber", Medium::fiber, "fiber"),
            Cable::SerialDce | Cable::SerialDte => need("serial", |m| m == Medium::Serial, "serial"),
            Cable::Coaxial => need("coaxial", |m| m == Medium::Coaxial, "coaxial"),
            Cable::Phone => need("phone", |m| m == Medium::Phone, "phone (RJ-11)"),
            Cable::IotCustom => Some(format!(
                "an IoT custom cable joins a programmable board's pin to a smart thing, not {} and {}",
                self.short_label(a),
                self.short_label(b)
            )),
            Cable::Usb => Some(format!(
                "a USB cable joins a PC's {USB_PORT} port to a router's or switch's {USB_CONSOLE_PORT} port, not {} and {}",
                self.short_label(a),
                self.short_label(b)
            )),
            Cable::Octal => Some(format!(
                "an octal cable joins a line of a router's octal card to a router's or switch's {CONSOLE_PORT} port, not {} and {}",
                self.short_label(a),
                self.short_label(b)
            )),
            Cable::Console => Some(format!(
                "a console cable joins a PC's {RS232_PORT} port to a router's or switch's {CONSOLE_PORT} port, not {} and {}",
                self.short_label(a),
                self.short_label(b)
            )),
            Cable::Auto => {
                let line = |m: Medium| matches!(m, Medium::Coaxial | Medium::Phone);
                if line(ma) || line(mb) {
                    return (ma != mb).then(|| {
                        format!("{} is {} and {} is {}: no cable joins them", self.short_label(a), ma.words(), self.short_label(b), mb.words())
                    });
                }
                if (ma == Medium::Serial) != (mb == Medium::Serial) {
                    Some(format!(
                        "{} and {} don't fit one cable: serial ports take a serial cable, Ethernet ports a copper or fiber one",
                        self.short_label(a),
                        self.short_label(b)
                    ))
                } else if ma != Medium::Serial && !(ma.copper() && mb.copper()) && !(ma.fiber() && mb.fiber()) {
                    Some(format!("{} is {} and {} is {}: no cable joins them", self.short_label(a), ma.words(), self.short_label(b), mb.words()))
                } else {
                    None
                }
            }
        }
    }

    /// The kind a cable is in fact: an automatic one is the right kind for its ports.
    pub fn resolved_cable(&self, l: &Link) -> Cable {
        if l.cable != Cable::Auto {
            return l.cable;
        }
        let (ma, mb) = (self.port_medium(l.a), self.port_medium(l.b));
        match (ma, mb) {
            (Some(Medium::Serial), _) | (_, Some(Medium::Serial)) => Cable::SerialDce,
            (Some(Medium::Fiber | Medium::Combo), Some(Medium::Fiber | Medium::Combo))
                if self.single_mode_at(l.a) && self.single_mode_at(l.b) && (ma == Some(Medium::Fiber) || mb == Some(Medium::Fiber)) =>
            {
                Cable::FiberSingle
            }
            (Some(Medium::Fiber), _) | (_, Some(Medium::Fiber)) => Cable::FiberMulti,
            (Some(Medium::Coaxial), _) | (_, Some(Medium::Coaxial)) => Cable::Coaxial,
            (Some(Medium::Phone), _) | (_, Some(Medium::Phone)) => Cable::Phone,
            _ => copper_needed(self, l.a, l.b),
        }
    }

    /// Whether the port holds a single-mode transceiver.
    pub fn single_mode_at(&self, end: End) -> bool {
        self.device(end.device).and_then(|d| d.ports.get(end.port)).is_some_and(|p| p.single_mode)
    }

    /// Why a fiber link carries nothing because of its transceivers, if it doesn't: the two ends
    /// need the same kind (850 nm multi-mode and 1310 nm single-mode light do not meet), and a
    /// multi-mode transceiver cannot drive a single-mode cable. A single-mode transceiver over a
    /// multi-mode cable works (as with a mode-conditioning patch cord) but only as far as
    /// multi-mode fiber reaches. `None` for anything that is not a fiber cable.
    pub fn fiber_problem(&self, l: &Link) -> Option<String> {
        let kind = self.resolved_cable(l);
        if !kind.is_fiber() {
            return None;
        }
        let (sa, sb) = (self.single_mode_at(l.a), self.single_mode_at(l.b));
        let word = |single: bool| if single { "single-mode (1310 nm)" } else { "multi-mode (850 nm)" };
        if sa != sb {
            return Some(format!("transceiver mismatch: {} is {} and {} is {}", self.short_label(l.a), word(sa), self.short_label(l.b), word(sb)));
        }
        (kind == Cable::FiberSingle && !sa).then(|| {
            format!(
                "a multi-mode transceiver ({}) cannot drive a single-mode fiber cable: set the transceivers to single mode",
                self.short_label(l.a)
            )
        })
    }

    /// The longest run the cable at a link carries, in metres (`None`: no limit modelled).
    pub fn fiber_reach(kind: Cable) -> Option<u32> {
        match kind {
            Cable::FiberSingle => Some(FIBER_SINGLE_MAX_M),
            Cable::FiberMulti => Some(FIBER_MULTI_MAX_M),
            _ => None,
        }
    }

    /// Why the cable at a port carries nothing because of its kind: it doesn't fit the ports
    /// (a hostile file), or it is the wrong copper cable for ports without auto-MDIX.
    pub fn cable_mismatch(&self, end: End) -> Option<String> {
        let l = self.link_at(end)?;
        if l.cable == Cable::Auto {
            return self.fiber_problem(l);
        }
        if let Some(why) = self.cable_misfit(l.cable, l.a, l.b) {
            return Some(format!("wrong cable: {why}"));
        }
        if l.cable.is_fiber() {
            return self.fiber_problem(l);
        }
        if !l.cable.is_copper() || auto_mdix(self, l.a) || auto_mdix(self, l.b) {
            return None;
        }
        let need = copper_needed(self, l.a, l.b);
        (need != l.cable).then(|| {
            format!(
                "wrong cable: {} and {} need a {} cable, a {} one is plugged in and neither end has auto-MDIX",
                self.short_label(l.a),
                self.short_label(l.b),
                need.label().trim_start_matches("Copper ").to_lowercase(),
                l.cable.label().trim_start_matches("Copper ").to_lowercase()
            )
        })
    }

    /// Cables two free ports with a cable of the given kind. On a serial cable the first end
    /// is the DCE (Serial DCE, automatic) or the DTE (Serial DTE).
    pub fn connect_with(&mut self, a: End, b: End, cable: Cable) -> Result<()> {
        if a == b {
            return Err(ModelError::SamePort);
        }
        self.check_end(a)?;
        self.check_end(b)?;
        if let Some(why) = self.cable_misfit(cable, a, b) {
            return Err(ModelError::Invalid(why));
        }
        self.links.push(Link { a, b, cable });
        Ok(())
    }

    /// The first free port of a device a cable of this kind fits, given the medium of the
    /// port at the other end (if chosen yet).
    pub fn free_port_for_cable(&self, id: DeviceId, cable: Cable, other: Option<End>) -> Option<usize> {
        let d = self.device(id)?;
        (0..d.ports.len()).find(|&p| {
            let end = End { device: id, port: p };
            if self.link_at(end).is_some() {
                return false;
            }
            match other {
                Some(o) => self.cable_misfit(cable, o, end).is_none(),
                None => self.cable_misfit(cable, end, end).is_none(),
            }
        })
    }

    /// The device a PC's console cable leads to.
    pub fn console_peer(&self, pc: DeviceId) -> Option<DeviceId> {
        self.consoles.iter().find(|c| c.pc == pc).map(|c| c.device)
    }

    /// The console cable plugged into a device at either end.
    pub fn console_at(&self, id: DeviceId) -> Option<&ConsoleCable> {
        self.consoles.iter().find(|c| c.pc == id || c.device == id)
    }

    /// Joins a PC's RS-232 port and a router's or switch's Console port (either order).
    pub fn connect_console(&mut self, a: DeviceId, b: DeviceId) -> Result<ConsoleCable> {
        self.connect_console_with(a, b, false)
    }

    /// Joins a PC's USB port and a router's or switch's USB console port (either order).
    pub fn connect_usb(&mut self, a: DeviceId, b: DeviceId) -> Result<ConsoleCable> {
        self.connect_console_with(a, b, true)
    }

    fn connect_console_with(&mut self, a: DeviceId, b: DeviceId, usb: bool) -> Result<ConsoleCable> {
        let da = self.device(a).ok_or_else(|| ModelError::NoDevice(a.to_string()))?;
        let db = self.device(b).ok_or_else(|| ModelError::NoDevice(b.to_string()))?;
        let (host_port, dev_port) = if usb { (USB_PORT, USB_CONSOLE_PORT) } else { (RS232_PORT, CONSOLE_PORT) };
        let (pc, dev) = if da.has_rs232() && db.has_console() {
            (da, db)
        } else if db.has_rs232() && da.has_console() {
            (db, da)
        } else {
            let what = |d: &Device| match d.console_port_name() {
                Some(p) if usb && p == CONSOLE_PORT => format!("{} has a {USB_CONSOLE_PORT} port", d.name),
                Some(_) if usb => format!("{} has a {USB_PORT} port", d.name),
                Some(p) => format!("{} has a {p} port", d.name),
                None => format!("{} has no console or {} port", d.name, if usb { USB_PORT } else { RS232_PORT }),
            };
            return Err(ModelError::Invalid(format!(
                "a {} cable joins a PC's {host_port} port to a router's or switch's {dev_port} port: {} and {}",
                if usb { "USB" } else { "console" },
                what(da),
                what(db)
            )));
        };
        if self.console_at(pc.id).is_some() {
            return Err(ModelError::PortInUse(pc.name.clone(), host_port.into()));
        }
        if self.console_at(dev.id).is_some() || self.octal_at_device(dev.id).is_some() {
            return Err(ModelError::PortInUse(dev.name.clone(), dev_port.into()));
        }
        let c = ConsoleCable { pc: pc.id, device: dev.id, usb };
        self.consoles.push(c);
        Ok(c)
    }

    /// The octal cable that reaches a device's Console port.
    pub fn octal_at_device(&self, id: DeviceId) -> Option<&OctalCable> {
        self.octals.iter().find(|o| o.device == id)
    }

    /// The octal cable plugged into line `line` of a router's octal cards.
    pub fn octal_at_line(&self, router: DeviceId, line: u8) -> Option<&OctalCable> {
        self.octals.iter().find(|o| o.router == router && o.line == line)
    }

    /// Plugs an octal cable from a router's octal card (line `line`, or the first free one) to
    /// the Console port of a router or switch.
    pub fn connect_octal(&mut self, router: DeviceId, device: DeviceId, line: Option<u8>) -> Result<OctalCable> {
        let r = self.device(router).ok_or_else(|| ModelError::NoDevice(router.to_string()))?;
        let d = self.device(device).ok_or_else(|| ModelError::NoDevice(device.to_string()))?;
        let lines = r.octal_lines();
        if lines == 0 {
            return Err(ModelError::Invalid(format!(
                "{} has no octal card: fit a {OCTAL_CARD} in a bay of a modular router (device.module.add) first",
                r.name
            )));
        }
        if router == device {
            return Err(ModelError::Invalid("an octal cable needs two different devices".into()));
        }
        if !d.has_console() {
            return Err(ModelError::Invalid(format!("{} has no {CONSOLE_PORT} port: an octal cable ends at a router's or switch's console", d.name)));
        }
        if self.console_at(device).is_some() || self.octal_at_device(device).is_some() {
            return Err(ModelError::PortInUse(d.name.clone(), CONSOLE_PORT.into()));
        }
        let line = match line {
            Some(n) if n < lines => n,
            Some(n) => {
                return Err(ModelError::Invalid(format!("{} has lines 0 to {} on its octal cards, not {n}", r.name, lines.saturating_sub(1))));
            }
            None => (0..lines)
                .find(|n| self.octal_at_line(router, *n).is_none())
                .ok_or_else(|| ModelError::Invalid(format!("all {lines} lines of {}'s octal cards are cabled", r.name)))?,
        };
        if self.octal_at_line(router, line).is_some() {
            return Err(ModelError::PortInUse(r.name.clone(), format!("Async line {line}")));
        }
        let o = OctalCable { router, line, device };
        self.octals.push(o);
        Ok(o)
    }

    /// Unplugs the octal cable that reaches a device's Console port.
    pub fn disconnect_octal(&mut self, device: DeviceId) -> Option<OctalCable> {
        let i = self.octals.iter().position(|o| o.device == device)?;
        Some(self.octals.remove(i))
    }

    /// Drops octal cables a hostile file or a removed card left wrong.
    pub(crate) fn sanitize_octals(&mut self) {
        self.octals.truncate(crate::MAX_DEVICES);
        let mut lines = std::collections::BTreeSet::new();
        let mut used = std::collections::BTreeSet::new();
        let consoles: std::collections::BTreeSet<DeviceId> = self.consoles.iter().flat_map(|c| [c.pc, c.device]).collect();
        let devices = &self.devices;
        let find = |id: DeviceId| devices.iter().find(|d| d.id == id);
        self.octals.retain(|o| {
            o.router != o.device
                && find(o.router).is_some_and(|r| o.line < r.octal_lines())
                && find(o.device).is_some_and(|d| d.has_console())
                && !consoles.contains(&o.device)
                && lines.insert((o.router, o.line))
                && used.insert(o.device)
        });
    }

    /// Unplugs the console cable at a device (either end).
    pub fn disconnect_console(&mut self, id: DeviceId) -> Option<ConsoleCable> {
        let i = self.consoles.iter().position(|c| c.pc == id || c.device == id)?;
        Some(self.consoles.remove(i))
    }

    /// Drops console cables a hostile file left wrong: unknown devices, wrong kinds, a port
    /// used twice.
    pub(crate) fn sanitize_consoles(&mut self) {
        self.consoles.truncate(crate::MAX_DEVICES);
        let mut used = std::collections::BTreeSet::new();
        let devices = &self.devices;
        let ok = |id: DeviceId, pc: bool| devices.iter().find(|d| d.id == id).is_some_and(|d| if pc { d.has_rs232() } else { d.has_console() });
        self.consoles.retain(|c| ok(c.pc, true) && ok(c.device, false) && used.insert(c.pc) && used.insert(c.device));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab() -> (Topology, [DeviceId; 6]) {
        let mut t = Topology::new();
        let r1 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let r2 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let s1 = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let pc1 = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let pc2 = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let fs = t.add_model(models::find("Switch-24-SFP").unwrap(), None, 0.0, 0.0).unwrap();
        (t, [r1, r2, s1, pc1, pc2, fs])
    }

    fn e(device: DeviceId, port: usize) -> End {
        End { device, port }
    }

    #[test]
    fn ports_take_their_model_medium() {
        let (t, [r1, _, s1, pc1, _, fs]) = lab();
        assert_eq!(t.port_medium(e(r1, 0)), Some(Medium::Copper));
        assert_eq!(t.port_medium(e(s1, 0)), Some(Medium::Copper));
        assert_eq!(t.port_medium(e(s1, 24)), Some(Medium::Combo));
        assert_eq!(t.port_medium(e(fs, 24)), Some(Medium::Fiber));
        assert_eq!(t.port_medium(e(pc1, 0)), Some(Medium::Copper));
        let wan = models::find("Router-WAN").unwrap();
        assert_eq!(wan.port_medium("Serial0/0/1"), Some(Medium::Serial));
        assert_eq!(models::find("Laptop-WiFi").unwrap().port_medium("Wireless0"), Some(Medium::Wireless));
        assert!(t.device(r1).unwrap().has_console() && t.device(s1).unwrap().has_console());
        assert!(t.device(pc1).unwrap().has_rs232() && !t.device(pc1).unwrap().has_console());
        assert!(!models::find("Home-Router").unwrap().has_console());
        assert!(!models::find("Server").unwrap().has_rs232());
    }

    #[test]
    fn a_cable_joins_only_ports_of_its_medium() {
        let (mut t, [r1, r2, s1, pc1, _, fs]) = lab();
        let err = t.connect_with(e(fs, 24), e(s1, 0), Cable::FiberMulti).unwrap_err().to_string();
        assert_eq!(err, "a multi mode fiber cable needs a fiber port on both ends: S1 Fa0/1 is copper");
        let err = t.connect_with(e(fs, 24), e(r1, 0), Cable::Straight).unwrap_err().to_string();
        assert!(err.contains("needs a copper port") && err.contains("is fiber"), "{err}");
        assert!(t.connect_with(e(r1, 0), e(r2, 0), Cable::SerialDce).unwrap_err().to_string().contains("serial port"));
        assert!(t.connect_with(e(r1, 0), e(pc1, 0), Cable::Console).is_err());
        // Combo uplinks take fiber or copper; automatic picks fiber for a fiber port.
        t.connect_with(e(fs, 24), e(s1, 24), Cable::FiberMulti).unwrap();
        t.connect_with(e(fs, 25), e(s1, 25), Cable::Auto).unwrap();
        assert_eq!(t.resolved_cable(t.link_at(e(fs, 25)).unwrap()), Cable::FiberMulti);
        assert!(t.connect_with(e(fs, 0), e(r2, 0), Cable::FiberMulti).is_err());
        assert!(t.connect_with(e(fs, 0), e(pc1, 0), Cable::Auto).is_ok());
        // Copper to fiber-only: nothing joins them.
        let mut u = lab().0;
        let msg = u.connect_with(e(DeviceId(6), 24), e(DeviceId(1), 0), Cable::Auto).unwrap_err().to_string();
        assert!(msg.contains("no cable joins them"), "{msg}");
        assert_eq!(u.free_port_for_cable(DeviceId(3), Cable::FiberMulti, None), Some(24));
    }

    #[test]
    fn automatic_cables_resolve_and_wrong_copper_needs_auto_mdix() {
        let (mut t, [r1, r2, s1, pc1, pc2, _]) = lab();
        t.connect_with(e(r1, 0), e(r2, 0), Cable::Auto).unwrap();
        t.connect_with(e(pc1, 0), e(s1, 0), Cable::Auto).unwrap();
        assert_eq!(t.resolved_cable(t.link_at(e(r1, 0)).unwrap()), Cable::Cross);
        assert_eq!(t.resolved_cable(t.link_at(e(pc1, 0)).unwrap()), Cable::Straight);
        // PC to PC with a straight-through: no auto-MDIX on hosts, the link stays down.
        t.connect_with(e(pc2, 0), e(r1, 1), Cable::Straight).unwrap();
        // A router has auto-MDIX: fine.
        assert!(t.cable_mismatch(e(pc2, 0)).is_none());
        t.device_mut(r1).unwrap().ports[1].mdix_off = true;
        let why = t.cable_mismatch(e(pc2, 0)).unwrap();
        assert!(why.contains("need a cross-over cable"), "{why}");
    }

    #[test]
    fn console_cables_join_rs232_to_console_once() {
        let (mut t, [r1, r2, s1, pc1, pc2, _]) = lab();
        t.connect_console(r1, pc1).unwrap();
        assert_eq!(t.console_peer(pc1), Some(r1));
        assert!(matches!(t.connect_console(pc2, r1), Err(ModelError::PortInUse(..))));
        assert!(t.connect_console(r2, s1).is_err());
        assert!(t.connect_console(pc1, pc2).is_err());
        t.connect_console(pc2, s1).unwrap();
        t.consoles.push(ConsoleCable { pc: r2, device: s1, usb: false });
        t.consoles.push(ConsoleCable { pc: DeviceId(99), device: r2, usb: false });
        t.sanitize();
        assert_eq!(t.consoles.len(), 2);
        t.remove_device(r1).unwrap();
        assert_eq!(t.console_peer(pc1), None);
        assert_eq!(t.disconnect_console(s1).map(|c| c.pc), Some(pc2));
    }

    #[test]
    fn cable_names_parse() {
        for c in Cable::ALL {
            assert_eq!(Cable::parse(c.id()), Some(c));
            assert_eq!(Cable::parse(c.label()), Some(c));
        }
        assert_eq!(Cable::parse("crossover"), Some(Cable::Cross));
        assert_eq!(Cable::parse("rollover"), Some(Cable::Console));
        assert_eq!(Cable::parse("coax"), Some(Cable::Coaxial));
        assert_eq!(Cable::parse("fiber"), Some(Cable::FiberMulti));
        assert_eq!(Cable::parse("smf"), Some(Cable::FiberSingle));
        assert_eq!(Cable::parse("rope"), None);
        assert!(is_console_port_name("RS-232") && is_console_port_name("Console") && !is_console_port_name("Fa0/1"));
    }

    #[test]
    fn the_tiles_come_in_the_reference_order() {
        let ids: Vec<&str> = Cable::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(
            ids,
            [
                "Automatically Choose Connection Type",
                "Console",
                "Copper Straight-Through",
                "Copper Cross-Over",
                "Single Mode Fiber",
                "Multi Mode Fiber",
                "Phone",
                "Coaxial",
                "Serial DCE",
                "Serial DTE",
                "Octal",
                "IoT Custom Cable",
                "USB",
            ]
        );
    }

    #[test]
    fn an_old_fiber_cable_opens_as_multi_mode() {
        let old: Link = serde_json::from_str(r#"{"a":{"device":1,"port":0},"b":{"device":2,"port":0},"cable":"fiber"}"#).unwrap();
        assert_eq!(old.cable, Cable::FiberMulti);
        let single: Link = serde_json::from_str(r#"{"a":{"device":1,"port":0},"b":{"device":2,"port":0},"cable":"fiber-single-mode"}"#).unwrap();
        assert_eq!(single.cable, Cable::FiberSingle);
        assert!(serde_json::to_string(&old).unwrap().contains("fiber-multi-mode"));
    }

    #[test]
    fn fiber_transceivers_must_match_each_other_and_the_cable() {
        let (mut t, [_, _, s1, _, _, fs]) = lab();
        t.connect_with(e(fs, 24), e(s1, 24), Cable::FiberSingle).unwrap();
        // Multi-mode transceivers cannot drive single-mode fiber.
        assert!(t.fiber_problem(t.link_at(e(fs, 24)).unwrap()).unwrap().contains("cannot drive a single-mode fiber"));
        assert!(t.cable_mismatch(e(fs, 24)).is_some());
        for end in [e(fs, 24), e(s1, 24)] {
            let (d, p) = (end.device, end.port);
            t.device_mut(d).unwrap().ports[p].single_mode = true;
        }
        assert!(t.cable_mismatch(e(fs, 24)).is_none());
        // One end single-mode only: the wavelengths do not meet.
        t.device_mut(s1).unwrap().ports[24].single_mode = false;
        assert!(t.cable_mismatch(e(fs, 24)).unwrap().contains("transceiver mismatch"));
        // A multi-mode cable between multi-mode transceivers is fine, and an automatic cable
        // between two single-mode ones is single-mode fiber.
        t.disconnect(e(fs, 24));
        t.connect_with(e(fs, 24), e(s1, 24), Cable::Auto).unwrap();
        assert_eq!(t.resolved_cable(t.link_at(e(fs, 24)).unwrap()), Cable::FiberMulti);
        t.device_mut(s1).unwrap().ports[24].single_mode = true;
        assert_eq!(t.resolved_cable(t.link_at(e(fs, 24)).unwrap()), Cable::FiberSingle);
        assert_eq!(Topology::fiber_reach(Cable::FiberMulti), Some(550));
        assert_eq!(Topology::fiber_reach(Cable::FiberSingle), Some(10_000));
    }

    #[test]
    fn usb_cables_join_a_pc_to_a_console_like_console_cables() {
        let (mut t, [r1, r2, s1, pc1, pc2, _]) = lab();
        let c = t.connect_usb(pc1, r1).unwrap();
        assert!(c.usb);
        assert_eq!(t.console_peer(pc1), Some(r1));
        // A device has one console session: the rollover cable finds its port taken.
        assert!(matches!(t.connect_console(pc2, r1), Err(ModelError::PortInUse(..))));
        assert!(t.connect_usb(r2, s1).unwrap_err().to_string().contains("USB cable joins"));
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"usb\":true"));
        let plain = ConsoleCable { pc: pc2, device: s1, usb: false };
        assert!(!serde_json::to_string(&plain).unwrap().contains("usb"));
        let back: ConsoleCable = serde_json::from_str(&json.replace(",\"usb\":true", "")).unwrap();
        assert!(!back.usb);
    }

    #[test]
    fn octal_cables_need_a_card_and_a_free_console() {
        let mut t = Topology::new();
        let m = t.add_model(models::find("Router-M2").unwrap(), None, 0.0, 0.0).unwrap();
        let r2 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let s1 = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        assert!(t.connect_octal(m, r2, None).unwrap_err().to_string().contains("no octal card"));
        t.device_mut(m).unwrap().powered_off = true;
        t.set_module(m, "0/1", Some(OCTAL_CARD)).unwrap();
        assert_eq!(t.device(m).unwrap().octal_lines(), 8);
        assert_eq!(t.connect_octal(m, r2, None).unwrap().line, 0);
        assert_eq!(t.connect_octal(m, s1, Some(5)).unwrap().line, 5);
        assert!(t.connect_octal(m, pc, None).unwrap_err().to_string().contains("no Console"));
        assert!(matches!(t.connect_octal(m, r2, None), Err(ModelError::PortInUse(..))));
        assert!(t.connect_octal(m, s1, Some(9)).is_err());
        assert!(t.connect_console(pc, r2).is_err(), "the console is taken by the octal line");
        t.octals.push(OctalCable { router: m, line: 0, device: s1 });
        t.octals.push(OctalCable { router: m, line: 12, device: r2 });
        t.sanitize();
        assert_eq!(t.octals.len(), 2);
        // Taking the card out unplugs its lines; removing a device unplugs its cable.
        t.set_module(m, "0/1", None).unwrap();
        assert!(t.octals.is_empty());
    }
}
