//! The only place NetLabCraft starts other programs.
//!
//! Every external program (a browser, a file opener, `git`…) goes through here, never through
//! `std::process::Command` directly (clippy refuses it, see `clippy.toml`), so that:
//!
//! - stdin is closed, never inherited, so nothing can sit waiting at a prompt;
//! - every run has a wall-clock deadline and its output is capped;
//! - a missing program, a bad exit status or a timeout is an `Err` with a plain message, never a
//!   panic or a hang.
//!
//! Not for the wasm build: there it returns `Err` (the platform has no processes).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The most output kept from a program (bytes, stdout and stderr each).
pub const MAX_OUTPUT: usize = 1 << 20;

/// What a finished program printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// Starts `program` and leaves it running on its own (a file opener, a browser). All three standard
/// streams are closed.
pub fn spawn_detached(program: &str, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start `{program}`: {e}"))?;
    // Reap it in the background so it never lingers as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Runs `program` to completion within `timeout`, killing it when the deadline passes. A non-zero
/// exit status is an `Err` carrying the program's stderr.
pub fn run(program: &str, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start `{program}`: {e}"))?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("`{program}` did not finish within {} s and was stopped", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("waiting for `{program}`: {e}")),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() { Ok(Output { stdout, stderr }) } else { Err(format!("`{program}` failed ({status}): {}", stderr.trim())) }
}

/// Reads a pipe on its own thread so a chatty program can never fill it and block; keeps at most
/// [`MAX_OUTPUT`] bytes and discards the rest.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        if let Some(mut pipe) = pipe {
            let mut buf = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let room = MAX_OUTPUT.saturating_sub(kept.len());
                kept.extend_from_slice(buf.get(..n.min(room)).unwrap_or_default());
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn runs_and_captures_output() {
        let out = run("echo", &["hello"], Duration::from_secs(5)).unwrap();
        assert_eq!(out.stdout.trim(), "hello");
    }

    #[test]
    fn missing_program_is_an_error_not_a_panic() {
        assert!(run("definitely-not-a-program-xyz", &[], Duration::from_secs(1)).is_err());
        assert!(spawn_detached("definitely-not-a-program-xyz", &[]).is_err());
    }

    #[test]
    fn a_hung_program_is_killed_at_the_deadline() {
        let t = Instant::now();
        let e = run("sleep", &["30"], Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("did not finish"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_failing_program_reports_its_status() {
        assert!(run("false", &[], Duration::from_secs(5)).is_err());
    }

    #[test]
    fn output_is_capped() {
        let out = run("sh", &["-c", "head -c 3000000 /dev/zero | tr '\\0' a"], Duration::from_secs(10)).unwrap();
        assert_eq!(out.stdout.len(), MAX_OUTPUT);
    }
}
