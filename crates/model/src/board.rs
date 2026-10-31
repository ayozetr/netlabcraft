//! Programmable boards: a single-board computer and a microcontroller board with digital and
//! analog pins. IoT custom cables join a pin to a smart thing: reading the pin gives the thing's
//! reading (a sensor) or state (a device), writing it switches the device. The board runs a
//! BoardScript program (see `netlabcraft-sim`'s `script` module) in simulated time.

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, ModelError, Result, Topology, models};

/// The longest program a board keeps (characters).
pub const MAX_PROGRAM: usize = 8_000;

/// An IoT custom cable from a board's pin to a smart thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinWire {
    /// `D0`…, `A0`….
    pub pin: String,
    pub thing: DeviceId,
}

/// A board's program and wiring.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub program: String,
    /// The program runs whenever the board has power (and starts at once when set).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub run: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<PinWire>,
}

impl Board {
    pub fn is_default(&self) -> bool {
        *self == Board::default()
    }
}

/// The pins of a board model, digital first (`D0`…), then analog (`A0`…); empty for any other
/// model.
pub fn pins(model: &models::Model) -> Vec<String> {
    let (digital, analog) = match model.id {
        "Board-SBC" => (8, 4),
        "Board-MCU" => (6, 2),
        _ => (0, 0),
    };
    (0..digital).map(|n| format!("D{n}")).chain((0..analog).map(|n| format!("A{n}"))).collect()
}

/// Whether a device is a programmable board.
pub fn is_board(d: &Device) -> bool {
    d.kind == DeviceKind::Iot && !pins(d.catalogue_model()).is_empty()
}

/// Whether a pin is analog (reads the whole reading; digital pins read 0 or 1).
pub fn analog(pin: &str) -> bool {
    pin.starts_with('A')
}

/// A pin as typed (`d0`, `A1`) in its canonical spelling, if the board has it.
pub fn find_pin(d: &Device, typed: &str) -> Option<String> {
    let t = typed.trim().to_ascii_uppercase();
    pins(d.catalogue_model()).into_iter().find(|p| *p == t)
}

impl Device {
    /// Keeps a board's program and wires sane after reading a file.
    pub(crate) fn sanitize_board(&mut self) {
        if !is_board(self) {
            self.board = Board::default();
            return;
        }
        self.board.program = self.board.program.chars().take(MAX_PROGRAM).collect();
        let pins = pins(self.catalogue_model());
        let mut seen = std::collections::BTreeSet::new();
        self.board.wires.retain(|w| pins.contains(&w.pin) && seen.insert(w.pin.clone()));
    }
}

impl Topology {
    /// The board and pin a smart thing is wired to.
    pub fn board_of(&self, thing: DeviceId) -> Option<(DeviceId, String)> {
        self.devices.iter().find_map(|d| d.board.wires.iter().find(|w| w.thing == thing).map(|w| (d.id, w.pin.clone())))
    }

    /// Wires a board's pin to a smart thing with an IoT custom cable (`Some`), or unplugs the
    /// pin's cable (`None`).
    pub fn wire_pin(&mut self, board: DeviceId, pin: &str, thing: Option<DeviceId>) -> Result<()> {
        let b = self.device(board).ok_or_else(|| ModelError::NoDevice(board.to_string()))?;
        if !is_board(b) {
            return Err(ModelError::Invalid(format!("{} is not a programmable board: an IoT custom cable starts at a board's pin", b.name)));
        }
        let Some(pin) = find_pin(b, pin) else {
            return Err(ModelError::Invalid(format!("{} has no pin {pin} (it has {})", b.name, pins(b.catalogue_model()).join(", "))));
        };
        let Some(thing) = thing else {
            let before = b.board.wires.len();
            if let Some(d) = self.device_mut(board) {
                d.board.wires.retain(|w| w.pin != pin);
                if d.board.wires.len() == before {
                    return Err(ModelError::Invalid(format!("{} {pin} has no cable", d.name)));
                }
            }
            return Ok(());
        };
        let t = self.device(thing).ok_or_else(|| ModelError::NoDevice(thing.to_string()))?;
        if crate::iot::thing(t).is_none() {
            return Err(ModelError::Invalid(format!(
                "{} is not a smart thing: an IoT custom cable joins a board's pin to a sensor or a smart device",
                t.name
            )));
        }
        if b.board.wires.iter().any(|w| w.pin == pin) {
            return Err(ModelError::PortInUse(b.name.clone(), pin));
        }
        if let Some((other, p)) = self.board_of(thing) {
            let on = self.device(other).map_or_else(String::new, |d| d.name.clone());
            return Err(ModelError::Invalid(format!("{} is already wired to {on} {p}", t.name)));
        }
        if let Some(d) = self.device_mut(board) {
            d.board.wires.push(PinWire { pin, thing });
        }
        Ok(())
    }

    /// Drops the wires to devices that are gone or are not smart things (a hostile file).
    pub(crate) fn sanitize_wires(&mut self) {
        let things: std::collections::BTreeSet<DeviceId> = self.devices.iter().filter(|d| crate::iot::thing(d).is_some()).map(|d| d.id).collect();
        let mut used = std::collections::BTreeSet::new();
        for d in &mut self.devices {
            d.board.wires.retain(|w| things.contains(&w.thing) && used.insert(w.thing));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_wire_once_to_smart_things_and_files_are_trimmed() {
        let mut t = Topology::new();
        let b = t.add_model(models::find("Board-MCU").unwrap(), None, 0.0, 0.0).unwrap();
        let s = t.add_model(models::find("IoT-Sensor").unwrap(), None, 0.0, 0.0).unwrap();
        let f = t.add_model(models::find("IoT-Device").unwrap(), None, 0.0, 0.0).unwrap();
        let pc = t.add_model(models::find("PC").unwrap(), None, 0.0, 0.0).unwrap();
        assert_eq!(pins(models::find("Board-MCU").unwrap()), ["D0", "D1", "D2", "D3", "D4", "D5", "A0", "A1"]);
        assert!(is_board(t.device(b).unwrap()) && !is_board(t.device(s).unwrap()));
        t.wire_pin(b, "a0", Some(s)).unwrap();
        t.wire_pin(b, "D0", Some(f)).unwrap();
        assert_eq!(t.board_of(s), Some((b, "A0".into())));
        assert!(t.wire_pin(b, "D0", Some(s)).is_err(), "the pin is taken");
        assert!(t.wire_pin(b, "D1", Some(s)).unwrap_err().to_string().contains("already wired"));
        assert!(t.wire_pin(b, "D9", Some(s)).unwrap_err().to_string().contains("no pin D9"));
        assert!(t.wire_pin(b, "D1", Some(pc)).unwrap_err().to_string().contains("not a smart thing"));
        assert!(t.wire_pin(s, "D1", Some(f)).unwrap_err().to_string().contains("not a programmable board"));
        assert!(analog("A0") && !analog("D0"));
        // A hostile file: a wire to a PC, an unknown pin, a pin twice.
        let text = crate::file::to_json(&t).unwrap();
        let mut back = crate::file::from_json(&text).unwrap();
        assert_eq!(back.device(b).unwrap().board.wires.len(), 2);
        let d = back.device_mut(b).unwrap();
        d.board.wires.push(PinWire { pin: "D2".into(), thing: pc });
        d.board.wires.push(PinWire { pin: "Q7".into(), thing: f });
        d.board.wires.push(PinWire { pin: "D0".into(), thing: f });
        d.board.program = "x".repeat(MAX_PROGRAM + 10);
        back.sanitize();
        let d = back.device(b).unwrap();
        assert_eq!((d.board.wires.len(), d.board.program.len()), (2, MAX_PROGRAM));
        // Removing a thing unplugs its wire; unplugging an empty pin says so.
        back.remove_device(f).unwrap();
        assert_eq!(back.device(b).unwrap().board.wires.len(), 1);
        assert!(back.wire_pin(b, "D0", None).is_err());
        back.wire_pin(b, "A0", None).unwrap();
        assert!(back.device(b).unwrap().board.wires.is_empty());
    }
}
