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
//! kept in [`Topology::consoles`].

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, End, Link, ModelError, Result, Topology, ifname, models};

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
    #[serde(rename = "fiber")]
    Fiber,
    /// A rollover cable from a PC's RS-232 port to a device's Console port.
    #[serde(rename = "console")]
    Console,
    /// A serial cable; the first end is the DCE (it gives the clock).
    #[serde(rename = "serial-dce")]
    SerialDce,
    /// A serial cable; the first end is the DTE, the far end the DCE.
    #[serde(rename = "serial-dte")]
    SerialDte,
}

impl Cable {
    pub const ALL: [Cable; 7] = [Cable::Auto, Cable::Console, Cable::Straight, Cable::Cross, Cable::Fiber, Cable::SerialDce, Cable::SerialDte];

    pub fn is_auto(&self) -> bool {
        *self == Cable::Auto
    }

    /// The id lab files and commands use.
    pub fn id(self) -> &'static str {
        match self {
            Cable::Auto => "auto",
            Cable::Straight => "straight-through",
            Cable::Cross => "cross-over",
            Cable::Fiber => "fiber",
            Cable::Console => "console",
            Cable::SerialDce => "serial-dce",
            Cable::SerialDte => "serial-dte",
        }
    }

    /// The name shown on the Connections tile.
    pub fn label(self) -> &'static str {
        match self {
            Cable::Auto => "Automatic",
            Cable::Straight => "Copper Straight-Through",
            Cable::Cross => "Copper Cross-Over",
            Cable::Fiber => "Fiber",
            Cable::Console => "Console",
            Cable::SerialDce => "Serial DCE",
            Cable::SerialDte => "Serial DTE",
        }
    }

    /// One line on what the cable is for.
    pub fn description(self) -> &'static str {
        match self {
            Cable::Auto => "Automatic: picks the right cable for the two ports you choose",
            Cable::Straight => "Copper straight-through: joins unlike ports (a PC or router to a switch or hub)",
            Cable::Cross => "Copper cross-over: joins like ports (switch to switch, router to router, PC to PC, router to PC)",
            Cable::Fiber => "Fiber: joins fiber ports (SFP uplinks and fiber cards); carries a signal up to 2 km here",
            Cable::Console => "Console (rollover): a PC's RS-232 port to a router's or switch's Console port, for the PC's Terminal",
            Cable::SerialDce => "Serial DCE: joins serial ports; the end you click first is the DCE and gives the clock",
            Cable::SerialDte => "Serial DTE: joins serial ports; the end you click first is the DTE, the other end gives the clock",
        }
    }

    /// The cable a person typed: its id, its label or a common alias.
    pub fn parse(s: &str) -> Option<Cable> {
        let k: String = s.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        Some(match k.as_str() {
            "auto" | "automatic" => Cable::Auto,
            "straight" | "straightthrough" | "copperstraightthrough" | "copper" => Cable::Straight,
            "cross" | "crossover" | "coppercrossover" => Cable::Cross,
            "fiber" | "fibre" => Cable::Fiber,
            "console" | "rollover" => Cable::Console,
            "serial" | "serialdce" | "dce" => Cable::SerialDce,
            "serialdte" | "dte" => Cable::SerialDte,
            _ => return None,
        })
    }

    pub fn is_serial(self) -> bool {
        matches!(self, Cable::SerialDce | Cable::SerialDte)
    }

    pub fn is_copper(self) -> bool {
        matches!(self, Cable::Straight | Cable::Cross)
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
    matches!(k.as_str(), "console" | "con" | "con0" | "rs232" | "com" | "com1")
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
            Cable::Fiber => need("fiber", Medium::fiber, "fiber"),
            Cable::SerialDce | Cable::SerialDte => need("serial", |m| m == Medium::Serial, "serial"),
            Cable::Console => Some(format!(
                "a console cable joins a PC's {RS232_PORT} port to a router's or switch's {CONSOLE_PORT} port, not {} and {}",
                self.short_label(a),
                self.short_label(b)
            )),
            Cable::Auto => {
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
            (Some(Medium::Fiber), _) | (_, Some(Medium::Fiber)) => Cable::Fiber,
            _ => copper_needed(self, l.a, l.b),
        }
    }

    /// Why the cable at a port carries nothing because of its kind: it doesn't fit the ports
    /// (a hostile file), or it is the wrong copper cable for ports without auto-MDIX.
    pub fn cable_mismatch(&self, end: End) -> Option<String> {
        let l = self.link_at(end)?;
        if l.cable == Cable::Auto {
            return None;
        }
        if let Some(why) = self.cable_misfit(l.cable, l.a, l.b) {
            return Some(format!("wrong cable: {why}"));
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
        let da = self.device(a).ok_or_else(|| ModelError::NoDevice(a.to_string()))?;
        let db = self.device(b).ok_or_else(|| ModelError::NoDevice(b.to_string()))?;
        let (pc, dev) = if da.has_rs232() && db.has_console() {
            (da, db)
        } else if db.has_rs232() && da.has_console() {
            (db, da)
        } else {
            let what = |d: &Device| match d.console_port_name() {
                Some(p) => format!("{} has a {p} port", d.name),
                None => format!("{} has no console or RS-232 port", d.name),
            };
            return Err(ModelError::Invalid(format!(
                "a console cable joins a PC's {RS232_PORT} port to a router's or switch's {CONSOLE_PORT} port: {} and {}",
                what(da),
                what(db)
            )));
        };
        if self.console_at(pc.id).is_some() {
            return Err(ModelError::PortInUse(pc.name.clone(), RS232_PORT.into()));
        }
        if self.console_at(dev.id).is_some() {
            return Err(ModelError::PortInUse(dev.name.clone(), CONSOLE_PORT.into()));
        }
        let c = ConsoleCable { pc: pc.id, device: dev.id };
        self.consoles.push(c);
        Ok(c)
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
        let err = t.connect_with(e(fs, 24), e(s1, 0), Cable::Fiber).unwrap_err().to_string();
        assert_eq!(err, "a fiber cable needs a fiber port on both ends: S1 Fa0/1 is copper");
        let err = t.connect_with(e(fs, 24), e(r1, 0), Cable::Straight).unwrap_err().to_string();
        assert!(err.contains("needs a copper port") && err.contains("is fiber"), "{err}");
        assert!(t.connect_with(e(r1, 0), e(r2, 0), Cable::SerialDce).unwrap_err().to_string().contains("serial port"));
        assert!(t.connect_with(e(r1, 0), e(pc1, 0), Cable::Console).is_err());
        // Combo uplinks take fiber or copper; automatic picks fiber for a fiber port.
        t.connect_with(e(fs, 24), e(s1, 24), Cable::Fiber).unwrap();
        t.connect_with(e(fs, 25), e(s1, 25), Cable::Auto).unwrap();
        assert_eq!(t.resolved_cable(t.link_at(e(fs, 25)).unwrap()), Cable::Fiber);
        assert!(t.connect_with(e(fs, 0), e(r2, 0), Cable::Fiber).is_err());
        assert!(t.connect_with(e(fs, 0), e(pc1, 0), Cable::Auto).is_ok());
        // Copper to fiber-only: nothing joins them.
        let mut u = lab().0;
        let msg = u.connect_with(e(DeviceId(6), 24), e(DeviceId(1), 0), Cable::Auto).unwrap_err().to_string();
        assert!(msg.contains("no cable joins them"), "{msg}");
        assert_eq!(u.free_port_for_cable(DeviceId(3), Cable::Fiber, None), Some(24));
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
        t.consoles.push(ConsoleCable { pc: r2, device: s1 });
        t.consoles.push(ConsoleCable { pc: DeviceId(99), device: r2 });
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
        assert_eq!(Cable::parse("coax"), None);
        assert!(is_console_port_name("RS-232") && is_console_port_name("Console") && !is_console_port_name("Fa0/1"));
    }
}
