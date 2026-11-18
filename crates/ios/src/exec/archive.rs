//! The configuration archive and scheduled reloads: `archive` mode, `archive config`,
//! `show archive`, `configure replace` and `configure revert now`.
//!
//! Copies are kept in the device (flash), numbered, and the oldest goes past `maximum`. A
//! `time-period` makes a copy when a command next looks at the archive (`show archive`,
//! `archive config`, saving the configuration): the simulator has no timer of its own for it.

use std::fmt::Write as _;

use netlabcraft_model::archive::{DEFAULT_COPIES, MAX_COPIES};

use super::{Ctx, Mode, R, apply_config};
use crate::grammar::Id;
use crate::render;

/// A copy made because `time-period` passed since the last one.
fn due_copy(c: &mut Ctx) -> Result<(), String> {
    let now = c.sim.clock(c.dev).0;
    let cfg = render::running_config(c.d()?);
    let d = c.dm()?;
    let Some(minutes) = d.archive.time_period else { return Ok(()) };
    let last = d.archive.entries.last().map(|e| e.at);
    if last.is_none_or(|l| now.saturating_sub(l) >= i64::from(minutes) * 60) {
        d.archive.push(&cfg, now);
    }
    Ok(())
}

/// Replaces the running configuration with a text (`startup-config` or a copy).
fn replace(c: &mut Ctx, text: &str, what: &str) -> R {
    c.dm()?.reset_config();
    let failed = apply_config(c.t, c.sim, c.dev, text);
    c.cli.mode = Mode::Exec;
    let mut out = format!("The running configuration was replaced with {what}.");
    for f in failed {
        let _ = write!(out, "\n% {f}");
    }
    Ok(out)
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::Archive => {
            c.cli.mode = Mode::Archive;
            Ok(String::new())
        }
        Id::NoArchive => {
            let a = &mut c.dm()?.archive;
            a.path.clear();
            a.maximum = None;
            a.time_period = None;
            a.write_memory = false;
            Ok(String::new())
        }
        Id::ArchivePath => {
            let p = arg(0).trim();
            if p.is_empty() || p.chars().count() > 128 || !p.chars().all(|ch| ch.is_ascii_graphic()) {
                return Err("% The path is 1-128 printable characters with no spaces".into());
            }
            c.dm()?.archive.path = p.to_string();
            Ok(String::new())
        }
        Id::NoArchivePath => {
            c.dm()?.archive.path.clear();
            Ok(String::new())
        }
        Id::ArchiveMaximum => {
            let n: u16 = arg(0).trim().parse().ok().filter(|n| (1..=MAX_COPIES).contains(n)).ok_or("% The maximum is a number from 1 to 14")?;
            let a = &mut c.dm()?.archive;
            a.maximum = Some(n);
            while a.entries.len() > a.capacity() {
                a.entries.remove(0);
            }
            Ok(String::new())
        }
        Id::NoArchiveMaximum => {
            c.dm()?.archive.maximum = None;
            Ok(String::new())
        }
        Id::ArchiveTimePeriod => {
            let n: u32 =
                arg(0).trim().parse().ok().filter(|n| (1..=525_600).contains(n)).ok_or("% The period is a number of minutes from 1 to 525600")?;
            c.dm()?.archive.time_period = Some(n);
            Ok(String::new())
        }
        Id::NoArchiveTimePeriod => {
            c.dm()?.archive.time_period = None;
            Ok(String::new())
        }
        Id::ArchiveWriteMemory | Id::NoArchiveWriteMemory => {
            c.dm()?.archive.write_memory = id == Id::ArchiveWriteMemory;
            Ok(String::new())
        }
        Id::ArchiveConfig => {
            if c.d()?.archive.path.is_empty() {
                return Err("% Set the archive path first (`archive`, then `path flash:archive`)".into());
            }
            let cfg = render::running_config(c.d()?);
            let at = c.sim.clock(c.dev).0;
            let n = c.dm()?.archive.push(&cfg, at).ok_or("% No archive path")?;
            Ok(format!("Archived the running configuration as {}.", c.d()?.archive.name_of(n)))
        }
        Id::ShowArchive => {
            due_copy(c)?;
            let d = c.d()?;
            let a = &d.archive;
            if !a.configured() && a.entries.is_empty() {
                return Ok("The archive is not set up (`archive`, then `path flash:archive`).".into());
            }
            let mut s = String::new();
            let _ = writeln!(s, "Archive path:   {}", if a.path.is_empty() { "(not set)" } else { &a.path });
            let _ = writeln!(s, "Maximum copies: {}", a.maximum.unwrap_or(DEFAULT_COPIES));
            let _ = writeln!(s, "Time period:    {}", a.time_period.map_or("off".to_string(), |m| format!("{m} minutes")));
            let _ = writeln!(s, "Write memory:   {}", if a.write_memory { "on" } else { "off" });
            if a.entries.is_empty() {
                let _ = write!(s, "\nNo copies yet.");
            } else {
                let _ = writeln!(s, "\n{:<4} {:<24} Time", "No.", "Name");
                let now = c.sim.clock(c.dev).0;
                for e in &a.entries {
                    let _ = writeln!(s, "{:<4} {:<24} {} s ago", e.number, a.name_of(e.number), now.saturating_sub(e.at));
                }
            }
            Ok(s.trim_end().to_string())
        }
        Id::ConfigureReplace => {
            let name = arg(0).trim();
            let lower = name.to_ascii_lowercase();
            if lower == "startup-config" || lower == "nvram:startup-config" {
                let cfg = c.d()?.startup.clone().ok_or("% There is no saved configuration")?;
                return replace(c, &cfg, "the startup configuration");
            }
            let found = c.d()?.archive.find(name).cloned().ok_or_else(|| format!("% {name} is neither startup-config nor a copy in the archive"))?;
            let label = format!("copy {}", found.number);
            replace(c, &found.config, &label)
        }
        Id::ConfigureRevert => {
            let last = c.d()?.archive.entries.last().cloned().ok_or("% The archive has no copy to go back to")?;
            let label = format!("copy {}", last.number);
            replace(c, &last.config, &label)
        }
        _ => Err("% That command is not implemented".into()),
    }
}

/// The `archive` block of a running configuration.
pub(crate) fn running_config(s: &mut String, d: &netlabcraft_model::Device) {
    let a = &d.archive;
    if !a.configured() {
        return;
    }
    let _ = writeln!(s, "archive");
    if !a.path.is_empty() {
        let _ = writeln!(s, " path {}", a.path);
    }
    if let Some(n) = a.maximum {
        let _ = writeln!(s, " maximum {n}");
    }
    if let Some(n) = a.time_period {
        let _ = writeln!(s, " time-period {n}");
    }
    if a.write_memory {
        let _ = writeln!(s, " write-memory");
    }
    let _ = writeln!(s, "!");
}
