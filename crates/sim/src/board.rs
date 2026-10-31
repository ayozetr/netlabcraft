//! Programmable boards at run time: the BoardScript machine of each board, its pins and what
//! the program printed. A board whose program is set to run starts as soon as it has power and
//! time moves on (`sim.advance`, the play controls, a ping's wait); it pauses at each `sleep`
//! and carries on when the simulated time comes. Pins wired with an IoT custom cable read the
//! smart thing (a sensor's reading, a device's state) and switch the device.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::board::{analog, is_board, pins};
use netlabcraft_model::iot::{is_sensor, thing};
use netlabcraft_model::{Device, Topology};

use crate::script::{self, Io, Machine};
use crate::{Sim, SimError};

/// What a board knows at run time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoardRun {
    /// The running (or stopped) program; `None` before it starts.
    pub machine: Option<Machine>,
    /// Levels written to pins (the last value each `write` gave).
    pub levels: BTreeMap<String, i64>,
    /// Why the program couldn't start (it doesn't compile, the board is dark).
    pub message: Option<String>,
}

/// The board's pins as its program sees them.
struct Pins<'a> {
    sim: &'a mut Sim,
    t: &'a Topology,
    board: &'a Device,
    levels: BTreeMap<String, i64>,
}

impl Pins<'_> {
    fn wired(&self, pin: &str) -> Option<&Device> {
        self.board.board.wires.iter().find(|w| w.pin == pin).and_then(|w| self.t.device(w.thing))
    }

    fn server(&self) -> Result<Ipv4Addr, String> {
        self.board.iot.server.ok_or_else(|| "no IoT server set on the board (Programming tab)".to_string())
    }
}

impl Io for Pins<'_> {
    fn has_pin(&self, pin: &str) -> bool {
        pins(self.board.catalogue_model()).iter().any(|p| p == pin)
    }

    fn read(&mut self, pin: &str) -> Result<i64, String> {
        let Some(d) = self.wired(pin) else { return Ok(self.levels.get(pin).copied().unwrap_or(0)) };
        if self.t.is_dark(d.id) {
            return Ok(0);
        }
        let level = if is_sensor(d) { i64::from(d.iot.value) } else { i64::from(self.sim.iot.things.get(&d.id).is_some_and(|s| s.on)) };
        Ok(if analog(pin) { level } else { i64::from(level > 0) })
    }

    fn write(&mut self, pin: &str, level: i64) -> Result<(), String> {
        let level = level.clamp(-1_000_000, 1_000_000);
        if let Some(d) = self.wired(pin) {
            if is_sensor(d) {
                return Err(format!("{pin} is wired to {}, a sensor: read it instead", d.name));
            }
            let id = d.id;
            self.sim.iot.things.entry(id).or_default().on = level != 0;
        }
        self.levels.insert(pin.to_string(), level);
        Ok(())
    }

    fn get(&mut self, path: &str) -> Result<String, String> {
        let server = self.server()?;
        let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
        let (status, body) = self.sim.http_get(self.t, self.board.id, server, &server.to_string(), &path).map_err(|e| e.to_string())?;
        if status.starts_with("200") { Ok(body) } else { Err(format!("the server answered {status}")) }
    }

    fn report(&mut self, value: i64) -> Result<(), String> {
        let server = self.server()?;
        let v = value.clamp(i64::from(netlabcraft_model::iot::MIN_VALUE), i64::from(netlabcraft_model::iot::MAX_VALUE));
        let path = format!("/iot/register?name={}&kind=sensor&type=board&value={v}", self.board.name);
        let (status, _) = self.sim.http_get(self.t, self.board.id, server, &server.to_string(), &path).map_err(|e| e.to_string())?;
        if status.starts_with("200") { Ok(()) } else { Err(format!("the server answered {status}")) }
    }
}

impl Sim {
    /// A board's run-time state.
    pub fn board(&self, d: DeviceId) -> Option<&BoardRun> {
        self.iot.boards.get(&d)
    }

    /// Forgets a board's run: it starts afresh (its program changed, it was stopped by hand).
    pub fn board_reset(&mut self, d: DeviceId) {
        self.iot.boards.remove(&d);
    }

    /// Starts a board's program now (from the top), running it until its first `sleep`.
    pub fn board_start(&mut self, t: &Topology, d: DeviceId) -> Result<(), SimError> {
        let dev = t.device(d).ok_or(SimError::NoDevice(d))?;
        if !is_board(dev) {
            return Err(SimError::NoSource(format!("{} is not a programmable board", dev.name)));
        }
        self.iot.boards.remove(&d);
        if t.is_dark(d) {
            let why = "the board is switched off".to_string();
            self.iot.boards.insert(d, BoardRun { message: Some(why.clone()), ..BoardRun::default() });
            return Err(SimError::NoSource(why));
        }
        match script::compile(&dev.board.program) {
            Ok(p) => {
                self.iot.boards.insert(d, BoardRun { machine: Some(Machine::new(p, self.now)), ..BoardRun::default() });
                self.board_slice(t, d);
                Ok(())
            }
            Err(e) => {
                self.iot.boards.insert(d, BoardRun { message: Some(e.clone()), ..BoardRun::default() });
                Err(SimError::NoSource(e))
            }
        }
    }

    /// Stops a board's program where it is.
    pub fn board_stop(&mut self, d: DeviceId) {
        if let Some(m) = self.iot.boards.get_mut(&d).and_then(|b| b.machine.as_mut())
            && m.due().is_some()
        {
            m.note("stopped by hand".into());
            m.state = script::RunState::Stopped(None);
        }
    }

    /// Runs one slice of a board's program, if it is due.
    fn board_slice(&mut self, t: &Topology, d: DeviceId) {
        let Some(dev) = t.device(d) else { return };
        let now = self.now;
        let Some(mut run) = self.iot.boards.remove(&d) else { return };
        let Some(mut machine) = run.machine.take() else {
            self.iot.boards.insert(d, run);
            return;
        };
        let mut pins = Pins { sim: self, t, board: dev, levels: std::mem::take(&mut run.levels) };
        machine.run(&mut pins, now);
        run.levels = pins.levels;
        run.machine = Some(machine);
        self.iot.boards.insert(d, run);
    }

    /// When the next board is due (boards set to run, with power).
    pub(crate) fn board_next_due(&self, t: &Topology) -> Option<u64> {
        t.devices
            .iter()
            .filter(|d| is_board(d) && d.board.run && !t.is_dark(d.id))
            .filter_map(|d| match self.iot.boards.get(&d.id) {
                None => Some(self.now),
                Some(r) => r.machine.as_ref().and_then(Machine::due),
            })
            .min()
    }

    /// The boards whose time has come run a slice (a board set to run starts first).
    pub(crate) fn board_timed(&mut self, t: &Topology) {
        let now = self.now;
        let ids: Vec<DeviceId> = t.devices.iter().filter(|d| is_board(d) && d.board.run && !t.is_dark(d.id)).map(|d| d.id).collect();
        for d in ids {
            match self.iot.boards.get(&d).map(|r| r.machine.as_ref().and_then(Machine::due)) {
                None => {
                    let _ = self.board_start(t, d);
                }
                Some(Some(at)) if at <= now => self.board_slice(t, d),
                _ => {}
            }
        }
    }

    /// What a thing wired to a board shows: the board and pin.
    pub fn wired_to(&self, t: &Topology, thing_id: DeviceId) -> Option<String> {
        let (b, pin) = t.board_of(thing_id)?;
        let name = t.device(b).map(|d| d.name.clone())?;
        let _ = thing(t.device(thing_id)?)?;
        Some(format!("{name} {pin}"))
    }
}
