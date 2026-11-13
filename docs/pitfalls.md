# Pitfalls

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (a derived-state pitfall (M9.10)) · also merged with M9.9 (RSA, X.509, certificates in TLS and SSH) · earlier: minor (four pitfalls from M9.7: counters keyed by position, a stale checksum, a database that lists unreachable routers, a `Rest` that needs a word) · also merged with M9.5, M9.6, M9.8 and M9.11 · earlier: minor (two test pitfalls from M9.4) · also merged with M9.2 and M9.3 (routing timers; real DES, 3DES, AH and elliptic-curve Diffie-Hellman) · earlier: minor (first version)

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

- **A new field on a model struct breaks other crates' tests, and `cargo test -p one-crate` hides it.** Adding
  `log` to `AclEntry` compiled the library and the `ios` tests, while struct literals in `model`'s and `sim`'s own
  tests no longer compiled. Run `cargo test --workspace --no-run` after changing a shared struct, and build test
  literals from the constructor (`AclEntry::new(..)`) where one exists.
- **A lab-book `expect:` is a substring of NetLabCraft's own wording.** A lab walked for the first time fails on
  text (`Gi0/1` against `GigabitEthernet0/1`, `DISCOVER` against `Discover`); read the trace with
  `LAB_BOOK_TRACE=1` and fix the lab unless the output is what is wrong.
- **`assert!(x.contains("…") || true)` asserts nothing.** It was written to "just run the command" and passed
  forever. If you only need the command to run, write `let _ = x;`; if the result matters, assert it.
- **A new toolchain adds lints.** Clippy gates run with `-D warnings`, so a compiler bump can turn a green tree red
  (`nonminimal_bool` did, and CI on a rolling `stable` failed on `needless_range_loop`). The compiler is pinned in
  `rust-toolchain.toml`; bump it together with `rust-version`, and fix the code, not the gate.

## Console grammar

- **A `Rest(..)` token needs at least one word.** `spec!(X, .., [K("ip"), K("ospf"), K("authentication"), Rest("[message-digest|null]")])`
  makes the bare `ip ospf authentication` an "Incomplete command". Give a command whose last word is optional two
  specs: one without the `Rest`, one with it (as `area N stub` and `area N stub no-summary` do), and read the optional
  word with `arg(i)`, which is empty when it was not typed.
- **State kept per line is keyed by what identifies the line, never by its position.** ACL hit counters were keyed by
  the index in the list, so inserting `5 deny …` above the others moved every count onto the wrong line. They are keyed
  by the line's sequence number (`Acl::hit_key`), and a command that renumbers (`resequence`) or removes lines has to
  move or drop the counters with them (`Sim::remap_acl_hits`, `Sim::forget_acl_hit`).
- **A new console mode is edited in five places, and the tests only find some of them.** A `Mode` variant needs its
  bit in `Modes` and in `ANY_MODE`, its prompt, both lists of sub-modes in `exec.rs` (the `exit` list and the
  fall-back to configuration mode), the `every_spec_is_reachable_by_its_full_words` mode list in `grammar.rs`, and a
  setup line in `every_command_survives_hostile_arguments` so the fuzzer reaches the commands inside it. A
  `try_run` takes one line: a `\n` inside it is not a second command, it is a parse error.

## Simulator

- **Periodic control traffic is scheduled for the whole `run_for` window up front.** `schedule_control` walks every
  tick of the window at once, so a sender that reads a role or a state (a VRRP master, a held neighbour) sees it as of
  the start of the window, not as of the tick. A test that changes something and expects the new behaviour on the wire
  must run a first window for the change to settle, clear the trace, and look in a second window.
- **State derived from the lab goes stale unless every change refreshes it.** The cellular links
  (`Topology::cell_links`) are computed from positions and settings and read on every hop by `Topology::peer`.
  `device.move` does not call `Session::changed` (a drag would rebuild the simulator at every step), so a phone dragged
  out of range kept its tower and its address until the move command refreshed coverage itself. A command that changes
  where a device is or what it is set to must end in `cell_refresh` (`Session::changed` does it), and a test that
  edits a `Topology` directly must call it before it asks for a link.

- **A packet changed after it was encoded needs its checksum zeroed before the new one is computed.** Signing an OSPF
  packet with a clear-text password rewrote the authentication field and summed the packet again with the old checksum
  still in it, so the packet failed its own checksum, and no test noticed because nothing decoded it. A signer gets a
  test that decodes what it signed (`wire/src/control.rs`).
- **A view of "what this router knows" must follow what reaches it, not what is configured.** `show ip ospf database`
  listed the LSAs of every router with a segment in an area, so a router cut off by a passive interface kept its router,
  summary and external LSAs for ever. A database view starts from the viewer and follows adjacencies
  (`ospf_lsdb::flooding_domain`).
## Instructor

- **A key the student's app holds is not a secret from the student.** The results HMAC key comes from the salt in the
  file, so a signature never proved the score; what does is running the checks again against the network the file
  embeds (`activity.verify_results`). Do not add another "secret" to a file the student has: replay or move the check
  to the instructor.

## Wasm

- **`Instant::now()` and `SystemTime::now()` trap on `wasm32-unknown-unknown`.** They are disallowed in
  `clippy.toml`; simulated time lives in `netlabcraft-sim`, and the wall clock is read only off wasm.
- **`cargo xtask wasm` before you claim L0–L5 still builds there.** `process` compiles on wasm but only returns `Err`.

## Git

- **Never commit to `main`.** Work on a branch (`feature/…`), one task id per commit, and merge through a pull
  request. Check `git branch --show-current` before every commit and push by name.
- **The secret scan only protects clones that enable it.** Run `git config core.hooksPath .githooks` once per clone.
- **Deleting a branch closes its pull request.** Check open PRs before deleting a remote branch.
