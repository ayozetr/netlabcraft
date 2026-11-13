# Pitfalls

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (first version)

Mistakes already made here, and the rule that stops each one. Read this before your first change; when you make a
new mistake worth not repeating, add it in the same change.

## Std mutex and std process

- **A poisoned `std::sync::Mutex` takes everything down.** One panic while locked makes every later `lock()` fail,
  and the app catches panics at `Session::execute` and carries on. Use `netlabcraft_core::sync::Mutex`, whose `lock()`
  recovers the data. Clippy refuses the std type (`clippy.toml`).
- **`std::process::Command` inherits stdin and has no deadline.** A program waiting at a prompt hangs the caller
  forever. Start programs through `netlabcraft_core::process` (stdin closed, wall-clock deadline, capped output,
  `Err` instead of a panic). Clippy refuses `Command::new` elsewhere; developer tooling (`xtask`) and tests that
  start our own binary opt out with a comment saying why.

## Tests

- **`assert!(x.contains("…") || true)` asserts nothing.** It was written to "just run the command" and passed
  forever. If you only need the command to run, write `let _ = x;`; if the result matters, assert it.
- **A new toolchain adds lints.** Clippy gates run with `-D warnings`, so a compiler bump can turn a green tree red
  (`nonminimal_bool` did). Fix the code, not the gate.

## Wasm

- **`Instant::now()` and `SystemTime::now()` trap on `wasm32-unknown-unknown`.** They are disallowed in
  `clippy.toml`; simulated time lives in `netlabcraft-sim`, and the wall clock is read only off wasm.
- **`cargo xtask wasm` before you claim L0–L5 still builds there.** `process` compiles on wasm but only returns `Err`.

## Git

- **Never commit to `main`.** Work on a branch (`feature/…`), one task id per commit, and merge through a pull
  request. Check `git branch --show-current` before every commit and push by name.
- **The secret scan only protects clones that enable it.** Run `git config core.hooksPath .githooks` once per clone.
- **Deleting a branch closes its pull request.** Check open PRs before deleting a remote branch.
