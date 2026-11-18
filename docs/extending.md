# Extending NetLabCraft

> **Last reviewed:** 2026-11-18 · **Last updated:** 2026-11-18 · **Change:** minor (the debug framework: how a protocol adds a `debug` topic) · earlier: minor (device windows: every port of the Physical tab drawn as its real connector) · earlier: minor (the closet sheets at three zooms) · earlier: minor (how to render the contact sheets through CI) · earlier: minor (first version)

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
| A `debug` topic for a protocol | `DebugKind` in `crates/sim/src/debug.rs`, the table in `crates/ios/src/exec/debug.rs`, grammar rows | see [Adding a debug topic](#adding-a-debug-topic) |
| An external program | `netlabcraft_core::process` | never `std::process::Command` directly |
| A shared mutable value behind a lock | `netlabcraft_core::sync::Mutex` | never `std::sync::Mutex` |

Docs move with the change: the page's review line and revision history, the numbers in `ROADMAP.md` and
`docs/target-app-parity.md`, and a `ROADMAP.md` progress-log entry for anything people will notice.

## Looking at drawn art without running the app

Icons, connectors, cable tiles and the closet are drawn in code, so a change to them has to be looked at. The
`sheets` example (`crates/ui-egui/examples/sheets.rs`) renders them offscreen to PNGs: a contact sheet of every
device model (large and small), one of the cable tiles, the wiring closet (a rack, a shelf, a table, patch panels, a
power strip, cables; also zoomed on the rack and at 0.5, 1.0 and 1.5), the Physical tab of one device of every family (`device-physical-<NAME>-<theme>.png`) and the logical workspace, the physical bar with the `+` menu open at each level (`create-menu-<LEVEL>-<theme>.png`) and a closet with every kind of furniture (`create-kinds-<theme>.png`), each in the light and the dark theme. The labs
are built through engine commands, so a new command or model shows up by adding a line there.

The `Snapshots` workflow (`.github/workflows/snapshots.yml`) runs it on ubuntu with a software Vulkan driver and uploads
the PNGs as the `snapshots` artifact. It is not a gate. Start it with **Actions ▸ Snapshots ▸ Run workflow**, by putting
the label `snapshots` on a pull request, or by pushing a branch named `visual/*`; download the artifact from the run page
or with `gh run download <run-id> -n snapshots -D <directory outside the repository>`. The PNGs are never committed.

## Adding a debug topic

The `debug` commands share one framework (`crates/sim/src/debug.rs`, `crates/ios/src/exec/debug.rs`): each device holds a set
of `DebugKind` flags, and a protocol reports an event through one call. A topic is added in four steps:

1. **The flag.** Add a variant to `DebugKind`, its `(label, name)` row to `DebugKind::info` (the label starts every
   message, the name is what `show debugging` prints) and the variant to `DebugKind::ALL`.
2. **The command.** Add the topic to `TOPICS` in `crates/ios/src/exec/debug.rs` (the words after `debug`, the flag, a
   short sentence about what is reported). Add the grammar rows in `grammar.rs`, one pair per topic, both with the ids
   `DebugTopic` and `NoDebugTopic` and the same words (`[K("debug"), K("crypto"), K("isakmp")]` and
   `[K("no"), K("debug"), K("crypto"), K("isakmp")]`), and the same two rows in `docs/command-reference.md` at the same
   place (`cargo xtask parity` writes them; CI checks it). The handler needs no code: it finds the topic from the
   words of the row. A topic with an argument or its own rules (OSPF's `adj|events|hello`, BGP) can keep its own ids.
3. **The event.** Where the protocol does something worth reporting, call
   `self.debug_log(device, DebugKind::CryptoIsakmp, "text")`. The message is stored as `LABEL debug: text` after a
   time stamp, always at severity 7 (debugging), and goes to `show logging`, the console, `terminal monitor` sessions
   and syslog servers, each filtered by its own `logging` level. When the text is costly to build, guard the call with
   `self.debug_on(device, kind)`. Word the text fresh: say what happened in plain sentences, with the addresses and
   names a person would look for, never in another vendor's wording. `Sim::debug_raw` is for the older messages whose
   caller already writes the whole line.
4. **The tests.** Turn the topic on, make the event happen and look at `show logging`; turn it off and check the
   silence. `every_topic_turns_on_and_off_alone` (in `crates/ios/src/tests/debug_framework.rs`) covers the topic table by
   itself, and the hostile-arguments fuzz test reaches the new rows.

Things the framework does for you: a per-device set (`undebug all`, `no debug all` and `show debugging` know the new
flag), `debug ip packet ACL` filtering (`Sim::debug_packet_wanted`), the time stamp (`service timestamps debug …`),
the severity filters (`logging console|buffered|monitor|trap LEVEL`), `terminal monitor` for remote sessions (the
engine copies a device's messages into the console of every session that has it on) and the bound: the buffer, the
console queue and the monitor queue hold at most `MAX_LOG` messages each, the oldest dropped first.
