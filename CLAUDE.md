# NetLabCraft — instructions for agents

NetLabCraft is a clean-room, open-source, pure-Rust network simulator targeting Cisco Packet Tracer parity for teaching and lab work — and superiority (real wire-format frames, pcap export, speed, openness, agent control). It runs natively on Linux, Windows, macOS and BSD, and on the web via WASM. It follows the conventions of the storytold Crafting Apps (`../gridcraft`, `../cadcraft`, `../filmcraft`, `../soundcraft`…).

Standards and learnings shared across the crafting apps live in `storytold/craftrules`. Follow its standards where they apply.

## Start every session here
1. Read `plan/STATUS.md` (current milestone, next task), then the task in `plan/execution-plan.md` and the relevant `docs/architecture.md` section. Behaviour reference: `plan/packet-tracer/*.md` (`observed-ui.md` holds black-box observations of the running reference app).
2. Pick work from `docs/gaps.md` (ranked).

`plan/` is gitignored (local only).

## Clean room — absolute rule
**This is the most important rule in this repository; breaking it is a serious failure.**
- Packet Tracer (or any vendor's software) may only be *observed* black-box: run it, use its UI with synthetic labs, take screenshots stored only under `plan/packet-tracer/screenshots/` (never committed).
- **Never** read, disassemble, decompile, debug, patch or copy anything inside the Packet Tracer installation (file names and listings only). No reverse engineering of its binaries, its `.pkt`/`.pka` files, its network protocols or its scripting interfaces. Never use third-party material derived from such work (decoders, format documents, leaked sources).
- Never copy Cisco icons, device pictures, artwork, sample labs, activities, help text, IOS help strings or output wording. Command *names* and configuration syntax (`interface`, `ip address`, `show ip route`) are the field's shared vocabulary and are used as feature names; every message, table layout and help text is written fresh.
- Protocol behaviour comes from public standards (IEEE 802.1D/802.1Q, IETF RFCs) and from observation.
- Never copy or link GPL/LGPL/AGPL code (GNS3, ns-3, Mininet, Wireshark dissectors, FRRouting, BIRD…) — don't even read their sources.
- Interoperability with Packet Tracer files is **out of scope** until a lawyer-reviewed plan exists (see `docs/decisions.md`).

## Assets and iconography
**No Cisco (or any other vendor's) iconography, images, artwork, fonts, templates or sample files — ever.**
- Every asset in the repo is original work by NetLabCraft contributors, or public domain / CC0, or open source / Creative Commons that allows redistribution.
- Every asset file has a row in [`ATTRIBUTION.md`](ATTRIBUTION.md) (author, source, licence). `cargo xtask assets` enforces it.
- Prefer art generated in code: device icons (`crates/ui-egui/src/icons.rs`), themes (`crates/ui-egui/src/theme.rs`) and sample labs (`crates/engine/src/sample.rs`) are drawn/defined in code and original.

## Never crash
People build labs for hours; a crash loses their work. **This outranks feature work.**
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`).
- **Errors are `Result<T, E>`** through the crate's error type and `?`.
- **Input-derived data is hostile** (lab files, frames, console lines, MCP params): `get()` instead of indexing, checked/saturating arithmetic, cap input-sized allocations (`MAX_DEVICES`, `MAX_ROUTES`, `TRACE_CAP`, `MAX_RANGE`…).
- **Bound loops in the simulator:** a switching loop is stopped by `MAX_SWITCH_HOPS`, a runaway run by `MAX_EVENTS_PER_RUN`.
- **Last-resort guard:** `Session::execute` catches escaped panics and restores the lab. It's a safety net, not a licence.
- **Prove it:** every crash fix lands with a regression test. `engine::tests::every_command_survives_empty_and_hostile_params` fuzzes every engine command; `ios::tests::every_command_survives_hostile_arguments` every console command in every mode; `wire` decoders are fed every truncation.
- Crates carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.

## Non-negotiables
- **Everything is a command.** User-visible behaviour = a command in `crates/engine/src/catalog.rs` + `cmd.rs` (id, label, menu, shortcut, params doc) + tests. The UI, `netlabcraft-cli run` and MCP reach all of them. Device configuration goes through the console grammar (`crates/ios/src/grammar.rs`), never around it, so everything a person can type an agent can type.
- **Programmatic calls never open dialogs**: a command with missing params returns an error.
- **Layering** is enforced by `cargo xtask layers`: `core` (L0) → `wire` (L1) → `model` (L2) → `sim` (L3) → `ios` (L4) → `engine` (L5) → `ui-egui`/`mcp` (L6) → `apps/*` (L7). Nothing below L6 depends on egui/eframe/winit/rfd. `wire` depends on `core` only. **The UI crate is swappable.**
- **The UI is thin**: panels read engine state and act through `app.run(id, params)`. Colours come from `theme::Tokens`.
- **Frames are real**: the simulator moves encoded Ethernet frames, so captures are valid pcap.
- **Rust only** (no handwritten JS/TS). **Never break wasm** (`cargo xtask wasm`).
- **Quality gates** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, doctests, assets, layers, wasm, parity). One task id per commit (`M1.3: DHCP server`).

## Running and looking at the app
- `cargo run --release -p netlabcraft -- --sample vlans` (or a `.nlcraft` file).
- Headless: `netlabcraft-cli exec --sample two-subnets PC1 'ping 10.0.2.10'`, `netlabcraft-cli run --sample lan --cmd 'net.ping={"from":"PC1","to":"192.168.1.13"}'`, `netlabcraft-cli pcap --sample vlans --ping PC1:192.168.20.10 --out cap.pcap`, `netlabcraft-cli mcp`.
- **Offscreen UI render** (no window): `cargo run -p netlabcraft-ui-egui --example snapshot -- --sample vlans --console R1 out.png`. **For UI work, look at the result.**
- Parallel agents: separate `CARGO_TARGET_DIR` per agent; edit only the crates you own.

## Roadmap and documentation
`ROADMAP.md` (committed) is the one-page summary: stage, headline numbers, audiences, dimensions, progress log. It follows craftrules `standards/progress-docs.md` and points at the detail in `docs/`: `target-app-parity.md` (authoritative assessment: method, weights, audiences), `gaps.md` (ranked work list: pick from it), `roadmap.md` (current focus, milestones, alpha and beta gates), the area checklists `protocol-parity.md`, `ui-parity.md`, `hardware-parity.md`, `localization-parity.md`, plus `architecture.md`, `decisions.md`, `file-format.md`, `cli.md`, `mcp.md`, `contributors.md`, `releasing.md`, `model-equivalents.md`, and `command-reference.md` (generated by `cargo xtask parity`).
- Update the affected docs whenever work lands: the **review line** at the top (`Last reviewed · Last updated · Change: trivial|minor|major (what) · Target`), a row in the page's **Revision history**, the numbers in `ROADMAP.md` and `target-app-parity.md` (measured numbers say how they were counted; estimates say "estimated"), and a `ROADMAP.md` progress-log entry for anything users notice.
- A finished gap leaves `gaps.md`'s ranked table and goes to its revision history.
- README screenshots live in `docs/images/`, are rendered with the snapshot example, and each has a row in `ATTRIBUTION.md`.

## Contributor credits
`contributors/people.toml` holds names people chose for themselves (see its header). Never add, edit or guess anyone else's entry.
