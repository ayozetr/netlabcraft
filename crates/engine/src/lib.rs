//! PacketCraft L5: the session and its commands.
//!
//! **Everything is a command.** A [`Session`] holds the lab ([`Topology`]), the simulator and
//! one console per device; every user-visible action is a command in [`catalog`] with an id
//! (`device.add`, `cli.exec`, `net.ping`…), a label, a menu path and JSON parameters. The UI,
//! the CLI app and the MCP server all drive the same [`Session::execute`].
//!
//! Never crash: commands return errors, and a panic that escapes one is caught and the lab
//! restored ([`Session::execute`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::BTreeMap;

use packetcraft_ios::Cli;
use packetcraft_model::Topology;
use packetcraft_sim::Sim;
use serde_json::Value;

pub mod catalog;
mod cmd;
pub mod sample;

#[cfg(test)]
mod tests;

pub use catalog::{CATALOG, CommandInfo};
pub use packetcraft_core::DeviceId;

/// Undo steps kept.
pub const MAX_UNDO: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("{0}")]
    BadParams(String),
    #[error("{0}")]
    Failed(String),
    #[error("internal error in `{0}`; the lab was restored to its state before the command")]
    Panic(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl From<packetcraft_model::ModelError> for EngineError {
    fn from(e: packetcraft_model::ModelError) -> Self {
        EngineError::Failed(e.to_string())
    }
}

/// A device's console: its command-line session and what it printed.
#[derive(Debug, Clone, Default)]
pub struct Console {
    pub cli: Cli,
    /// Scrollback, prompt and command lines included.
    pub log: String,
}

/// Most scrollback a console keeps.
pub const MAX_CONSOLE_LOG: usize = 256 * 1024;

impl Console {
    pub fn append(&mut self, text: &str) {
        self.log.push_str(text);
        if self.log.len() > MAX_CONSOLE_LOG {
            let cut = self.log.len() - MAX_CONSOLE_LOG / 2;
            let at = (cut..self.log.len()).find(|i| self.log.is_char_boundary(*i)).unwrap_or(self.log.len());
            self.log.drain(..at);
        }
    }
}

/// The open lab.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub topology: Topology,
    pub sim: Sim,
    pub consoles: BTreeMap<DeviceId, Console>,
    undo: Vec<Topology>,
    redo: Vec<Topology>,
    /// Changed since the last save or open.
    pub dirty: bool,
    /// Where the lab was opened from or last saved to.
    pub path: Option<String>,
    /// Bumped on every change, for views that cache.
    pub revision: u64,
}

impl Session {
    pub fn new() -> Session {
        Session { topology: Topology::new(), ..Session::default() }
    }

    /// Runs a command by id. A panic inside it is caught: the lab goes back to how it was, the
    /// simulator restarts, and the command fails with [`EngineError::Panic`].
    pub fn execute(&mut self, id: &str, params: Value) -> Result<Value> {
        let before = self.topology.clone();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cmd::run(self, id, &params)));
        self.apply_sim_events();
        // A new cable or trunk can join switches into a VTP domain.
        if packetcraft_sim::vtp::sync(&mut self.topology) {
            self.sim.topology_changed(&self.topology);
        }
        match outcome {
            Ok(r) => r,
            Err(_) => {
                log::error!("command `{id}` panicked; restoring the lab");
                self.topology = before;
                self.sim.reset();
                self.revision += 1;
                Err(EngineError::Panic(id.to_string()))
            }
        }
    }

    /// Writes what the simulation decided into the lab: ports shut by port security, sticky
    /// addresses learned.
    fn apply_sim_events(&mut self) {
        let events = self.sim.take_events();
        if events.is_empty() {
            return;
        }
        for e in events {
            match e {
                packetcraft_sim::PortEvent::ErrDisabled { device, port } => {
                    if let Some(p) = self.topology.device_mut(device).and_then(|d| d.ports.get_mut(port)) {
                        p.err_disabled = true;
                    }
                }
                packetcraft_sim::PortEvent::Sticky { device, port, mac } => {
                    if let Some(ps) = self.topology.device_mut(device).and_then(|d| d.ports.get_mut(port)).and_then(|p| p.port_security.as_mut())
                        && !ps.macs.contains(&mac)
                    {
                        ps.macs.push(mac);
                    }
                }
            }
        }
        self.changed();
    }

    /// Records the current lab as an undo step (before a change).
    pub fn checkpoint(&mut self) {
        if self.undo.len() >= MAX_UNDO {
            self.undo.remove(0);
        }
        self.undo.push(self.topology.clone());
        self.redo.clear();
    }

    /// After a change: mark dirty, tell the simulator, bump the revision.
    pub fn changed(&mut self) {
        self.dirty = true;
        self.revision += 1;
        self.sim.topology_changed(&self.topology);
        let ids: Vec<DeviceId> = self.topology.devices.iter().map(|d| d.id).collect();
        self.consoles.retain(|id, _| ids.contains(id));
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.topology, prev));
        self.changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.topology, next));
        self.changed();
        true
    }

    /// Replaces the lab (open / new): history, consoles and simulation start over.
    pub fn replace(&mut self, t: Topology, path: Option<String>) {
        *self = Session { topology: t, path, revision: self.revision + 1, ..Session::default() };
    }

    /// The console of a device (created on first use, with its prompt).
    pub fn console(&mut self, dev: DeviceId) -> &mut Console {
        let t = &self.topology;
        self.consoles.entry(dev).or_insert_with(|| {
            let cli = Cli::default();
            let log = format!("{}{}", packetcraft_ios::greeting(t, dev), packetcraft_ios::prompt(t, dev, &cli));
            Console { cli, log }
        })
    }

    /// The event list as a pcap capture.
    pub fn pcap(&self) -> Vec<u8> {
        self.sim.pcap()
    }

    /// The window title's lab name.
    pub fn title(&self) -> String {
        let name = self
            .path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .and_then(|s| s.to_str())
            .map_or("Untitled lab".to_string(), str::to_string);
        if self.dirty { format!("{name} •") } else { name }
    }
}
