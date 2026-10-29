# Architecture

_Updated 2026-10-29._

## Layers

`cargo xtask layers` enforces that each crate depends only on lower layers, and that nothing below L6 depends on a UI toolkit.

| Layer | Crate | Role |
|---|---|---|
| L0 | `packetcraft-core` | Value types: `MacAddr`, `Ipv4Net`, `VlanId`, `VlanSet`, `DeviceId`. |
| L1 | `packetcraft-wire` | Wire formats (Ethernet II/802.1Q, ARP, IPv4, ICMP, UDP), Internet checksum, pcap writer. Depends on `core` only. |
| L2 | `packetcraft-model` | The lab document: devices, ports and their configuration, cables; interface-name parsing; the `.pcraft` file format. |
| L3 | `packetcraft-sim` | Discrete-event simulator: per-device dynamic state, event queue, event list (trace), ping and traceroute. |
| L4 | `packetcraft-ios` | Device consoles: the command grammar, the configuration shell, `show` output, running-config rendering, the host shell. |
| L5 | `packetcraft-engine` | `Session` (lab + simulator + consoles + undo), the command catalog, sample labs. |
| L6 | `packetcraft-ui-egui` | The egui interface. |
| L6 | `packetcraft-mcp` | MCP server over stdio. |
| L7 | `apps/packetcraft`, `packetcraft-cli`, `packetcraft-web` | Shells: desktop window and file dialogs, headless CLI, browser. |

## The simulator

- **Time** is simulated, in microseconds. Nothing reads the wall clock (it traps on wasm), so runs are deterministic.
- **Configuration lives in the model; the simulator keeps only dynamic state** (ARP caches, MAC tables, packets waiting for ARP). It reads the topology on every event, so a configuration change takes effect at once; `Sim::topology_changed` drops state the change invalidated (entries on ports that went down, frames on removed cables).
- **Frames are bytes.** Devices encode and decode real frames, so the event list exports straight to pcap and decoders are exercised on every hop.
- **Events:** a binary heap of deliveries ordered by (time, sequence). Every delivery is recorded in the trace with the receiver's drop reason, if any (capped at `TRACE_CAP`).
- **Switches** learn (VLAN, MAC) → port, age entries, flood unknown/broadcast within the VLAN, tag on trunks except the native VLAN. A frame that crosses `MAX_SWITCH_HOPS` switches is a loop: dropped and reported.
- **Routers** forward by longest prefix (connected beats static on ties), decrement TTL, send ICMP time-exceeded and unreachable, answer proxy ARP, and drop the packet that triggers an ARP request (hosts queue it instead).
- **Ping/traceroute** send echo requests and run the event loop until the matching answer arrives or the timeout passes.

## The console

- `grammar.rs` is a table of command paths (keyword / argument / rest-of-line) with the modes and device kinds where each applies. One resolver handles abbreviations, ambiguity, `?` help and Tab completion.
- `exec.rs` dispatches resolved commands; a sub-mode falls back to global configuration commands (and leaves the sub-mode), as engineers expect.
- `running_config` renders re-playable lines; `reload` resets the device and replays its startup-config through the same dispatcher, so the configuration round-trips by construction (tested).

## The engine

- Every action is a command (`catalog.rs` lists them; `cmd.rs` runs them). Mutating commands take an undo snapshot of the topology.
- `Session::execute` wraps each command in `catch_unwind`; a panic restores the lab and resets the simulator.
- Console lines run through `cli.exec`, which also keeps each device's scrollback.

## The UI

- `PacketApp::ui` draws menus, the tool palette, the canvas, device windows, the event list, notes and about windows into one `egui::Ui`.
- Host work (file dialogs, downloads, quitting) leaves as `UiRequest`s that the desktop and web shells serve.
- Icons are drawn in code (`icons.rs`); colours come from `theme::Tokens`.
