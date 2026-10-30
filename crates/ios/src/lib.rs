//! NetLabCraft L4: the device command lines.
//!
//! Routers and switches get a configuration shell with the command names network engineers
//! already type (`enable`, `configure terminal`, `interface g0/0`, `ip address`, `show ip
//! route`…), unambiguous abbreviations, `?` help and Tab completion. Hosts get a small desktop
//! shell (`ipconfig`, `ping`, `tracert`, `arp`).
//!
//! Clean room: command *names* and configuration syntax are the shared vocabulary of the field
//! and are used as feature names; every message, table layout and help text is our own wording.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod clock;
mod exec;
pub mod grammar;
pub mod host;
mod pipe;
mod render;

#[cfg(test)]
mod tests;

pub use exec::{
    Cli, IfRef, LoginStage, MODULUS_PROMPT, Mode, Outcome, PASSWORD_PROMPT, Remote, apply_config, complete_line, connect, execute, greeting,
    help_line, prompt,
};
pub use render::running_config;
