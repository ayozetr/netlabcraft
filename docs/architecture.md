# Architecture

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (modules added since M0: IPv6, serial framing, STP, OSPF, RIP, VTP, EtherChannel; shared format)

How PacketCraft is built. Rules for contributors and agents are in [`AGENTS.md`](../AGENTS.md);
the reasons behind the choices are in [decisions.md](decisions.md).

## Layers

`cargo xtask layers` enforces that each crate depends only on lower layers, and that nothing below
L6 depends on a UI toolkit.

| Layer | Crate | Role |
|---|---|---|
| L0 | `packetcraft-core` | Value types: `MacAddr`, `Ipv4Net`, `Ipv6Net` (with EUI-64 and link-local helpers), `VlanId`, `VlanSet`, `DeviceId`. |
| L1 | `packetcraft-wire` | Wire formats and checksums: Ethernet II/802.1Q, ARP, IPv4, ICMP, UDP, TCP, DHCP, DNS (`dns.rs`), IPv6/ICMPv6/NDP (`ipv6.rs`), HDLC and PPP framing (`serial.rs`), the pcap writer, and the one-line summaries the event list shows. Depends on `core` only. |
| L2 | `packetcraft-model` | The lab document: devices, ports and their configuration, cables (a serial cable's first end is the DCE), the device catalogue (`models.rs`), access lists (`acl.rs`), server services (`services.rs`), interface names (`ifname.rs`), the `.pcraft` format (`file.rs`). |
| L3 | `packetcraft-sim` | The discrete-event simulator (`lib.rs`) and the protocols computed from the topology: spanning tree (`stp.rs`), EtherChannel bundles (`channel.rs`), OSPF (`ospf.rs`), RIP (`rip.rs`), VTP (`vtp.rs`); IPv6 forwarding, NDP and autoconfiguration (`v6.rs`). |
| L4 | `packetcraft-ios` | Device consoles: the command grammar (`grammar.rs`), dispatch and modes (`exec.rs`), `show` output and running-config (`render.rs`), the PC/server shell (`host.rs`). |
| L5 | `packetcraft-engine` | `Session` (lab + simulator + consoles + undo), the command catalog (`catalog.rs`, `cmd.rs`), sample labs (`sample.rs`). |
| L6 | `packetcraft-ui-egui` | The egui interface. |
| L6 | `packetcraft-mcp` | MCP server over stdio. |
| L7 | `apps/packetcraft`, `apps/packetcraft-cli`, `apps/packetcraft-web` | Shells: desktop window and file dialogs, headless CLI, browser. |

## The simulator

- **Time** is simulated, in microseconds. Nothing reads the wall clock (it traps on wasm), so runs
  are deterministic.
- **Configuration lives in the model; the simulator keeps only dynamic state** (ARP and neighbour
  caches, MAC tables, packets waiting for an address, leases, NAT translations). It reads the
  topology on every event, so a configuration change takes effect at once; `Sim::topology_changed`
  drops state the change invalidated.
- **Frames are bytes.** Devices encode and decode real frames, so the event list exports straight
  to pcap and the decoders run on every hop. Serial links carry HDLC or PPP framing; the event list
  records the equivalent Ethernet II frame, because a capture file has one link type.
- **Events:** a binary heap of deliveries ordered by (time, sequence). Every delivery is recorded
  with the receiver's drop reason, if any (capped at `TRACE_CAP`).
- **Switches** learn (VLAN, MAC) → port, age entries, flood within the VLAN, tag on trunks except
  the native VLAN, treat an EtherChannel as one port (spreading frames over its members by
  address), enforce port security, and flush learned addresses when the topology changes. A
  frame that crosses `MAX_SWITCH_HOPS` switches is a loop: dropped and reported.
- **Routers and multilayer switches** forward by longest prefix (connected, then static, OSPF and
  RIP on ties), decrement TTL or hop limit, send ICMP/ICMPv6 errors, answer proxy ARP, apply
  ACLs and NAT, and relay DHCP. On IPv4 a router drops the packet that starts ARP (hosts queue
  it); with IPv6 everyone holds the packet until neighbour discovery answers.
- **Computed protocols.** Spanning tree, EtherChannel negotiation, OSPF, RIP and VTP are computed
  from the topology the way the protocols converge, cached by a fingerprint of what they depend
  on, and recomputed when it changes. Their hello/update frames are not drawn yet.
- **Ping/traceroute** send echo requests and run the event loop until the answer arrives or the
  timeout passes; DHCP, DNS, HTTP and Telnet/SSH do the same with their own exchanges.

## The console

- `grammar.rs` is a table of command paths (keyword / argument / rest-of-line) with the modes and
  device kinds where each applies. One resolver handles abbreviations, ambiguity, `?` help and Tab
  completion; a test proves every entry is reachable by its full words.
- `exec.rs` dispatches resolved commands; a sub-mode falls back to global configuration commands
  (and leaves the sub-mode), as engineers expect.
- `running_config` renders re-playable lines; `reload` resets the device and replays its
  startup-config through the same dispatcher, so the configuration round-trips by construction.
  A switch's VLAN database and VTP settings survive a reload, as on real switches.

## The engine

- Every action is a command (`catalog.rs` lists them; `cmd.rs` runs them). Mutating commands take
  an undo snapshot of the topology.
- `Session::execute` wraps each command in `catch_unwind`; a panic restores the lab and resets the
  simulator. After each command it applies what the simulator decided (ports err-disabled by port
  security, sticky addresses) and synchronises VTP domains.
- Console lines run through `cli.exec`, which also keeps each device's scrollback.

## The UI

- `PacketApp::ui` draws menus, toolbars, the canvas, device windows, the Simulation Panel and the
  bottom device box into one `egui::Ui`.
- Host work (file dialogs, downloads, quitting) leaves as `UiRequest`s that the desktop and web
  shells serve.
- Icons and device pictures are drawn in code (`icons.rs`); colours come from `theme::Tokens`.
- `examples/snapshot.rs` renders the UI offscreen to a PNG, for checking UI work and for the
  README screenshots.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Shared format; modules added since M0 (IPv6, serial framing, STP, EtherChannel, OSPF, RIP, VTP); simulator, console and engine notes brought up to date. |
| 2026-10-29 | major | Created. |
