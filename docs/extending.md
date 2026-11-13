# Extending NetLabCraft

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (first version)

Where each kind of addition goes. Layers are enforced by `cargo xtask layers`; put code at the lowest layer that can
own it.

| You are adding | It goes in | Also |
|---|---|---|
| A user-visible action | a command in `crates/engine/src/catalog.rs` and `cmd.rs` (id, label, menu, shortcut, params doc) | tests; `cargo xtask parity` regenerates `docs/command-reference.md`. The UI, `netlabcraft-cli run` and MCP reach it with no extra work |
| A console command or a mode | `crates/ios/src/grammar.rs` and the area's `crates/ios/src/exec/*.rs` | device configuration always goes through the grammar, never around it; add it to the hostile-arguments test |
| A protocol or device behaviour | `crates/sim/src/<protocol>.rs`, state types in `crates/model/src/` | frames are real (encoded in `crates/wire`); bound every loop (`MAX_SWITCH_HOPS`, `MAX_EVENTS_PER_RUN`); behaviour comes from public standards |
| A wire format | `crates/wire` (depends on `core` only) | feed the decoder every truncation |
| A sample lab | `crates/engine/src/sample.rs` | defined in code, so it is original and needs no `ATTRIBUTION.md` row |
| An MCP tool | `crates/mcp`; prefer exposing a command over a new tool | `docs/mcp.md` |
| A panel or dialog | `crates/ui-egui`, reading engine state and acting through `app.run(id, params)`; colours from `theme::Tokens` | render it with the `snapshot` example and look at the result |
| An image, icon or font | `assets/` | a row in `ATTRIBUTION.md` (`cargo xtask assets` enforces it); no vendor artwork, ever |
| A new crate | `crates/<name>`, registered in `xtask/src/layers.rs` | inherit `[lints] workspace = true`; production roots carry the deny attribute |
| An external program | `netlabcraft_core::process` | never `std::process::Command` directly |
| A shared mutable value behind a lock | `netlabcraft_core::sync::Mutex` | never `std::sync::Mutex` |

Docs move with the change: the page's review line and revision history, the numbers in `ROADMAP.md` and
`docs/target-app-parity.md`, and a `ROADMAP.md` progress-log entry for anything people will notice.
