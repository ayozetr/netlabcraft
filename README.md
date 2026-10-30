<h1 align="center">PacketCraft</h1>

<p align="center">
  <b>Network labs for learning and teaching; an open-source, clean-room network simulator built in pure Rust.</b>
</p>

<p align="center">
  Build a network, configure routers and switches by typing the commands you learn in class,
  and watch every frame cross it. It runs natively on Linux, Windows, macOS and BSD, and in the
  browser via WebAssembly.
</p>

<p align="center">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-1d5f8a?style=flat-square&logo=rust&logoColor=white">
  <img alt="Runs on Linux, Windows, macOS, BSD and the web" src="https://img.shields.io/badge/runs%20on-Linux%20%C2%B7%20Windows%20%C2%B7%20macOS%20%C2%B7%20BSD%20%C2%B7%20Web-2a7ab8?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-1d5f8a?style=flat-square">
  <img alt="Agent-drivable over MCP" src="https://img.shields.io/badge/agents-MCP-2a7ab8?style=flat-square">
</p>

<br>

<p align="center">
  <img src="docs/images/hero-multilayer.png" alt="PacketCraft's main window: a multilayer switch in the middle routes between four PCs in two VLANs and reaches a router and a server above it; menus and two toolbars on top, the device box, scenario list and PDU table at the bottom" width="100%">
  <br><sub><b>Inter-VLAN routing on a multilayer switch</b>: one of the eighteen sample labs (<code>File → New → Sample</code>), built entirely from PacketCraft commands.</sub>
</p>

<p align="center">
  <a href="#a-look-around">A look around</a> ·
  <a href="#why-packetcraft">Why PacketCraft</a> ·
  <a href="#what-it-simulates">What it simulates</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#for-agents-mcp-and-cli">For agents</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#roadmap">Roadmap</a> ·
  <a href="#license-and-credits">License</a>
</p>

## A look around

<table>
<tr>
<td width="50%" valign="top"><img src="docs/images/router-cli.png" alt="A router window on its CLI tab: the user typed enable and show ip interface brief, and the table lists GigabitEthernet0/0, two subinterfaces with addresses in VLANs 10 and 20, and two unused ports" width="100%"><p align="center"><sub><b>The console.</b> User, privileged and configuration modes, abbreviations, <code>?</code> help, Tab completion and history.</sub></p></td>
<td width="50%" valign="top"><img src="docs/images/simulation-mode.png" alt="Simulation mode: an envelope travels along the cable between a switch and a router, and the Simulation Panel on the right lists ARP events with their time, last device, device and type" width="100%"><p align="center"><sub><b>Simulation mode.</b> Every frame in an event list, replayed on the canvas, decoded layer by layer and exportable to pcap.</sub></p></td>
</tr>
<tr>
<td width="50%" valign="top"><img src="docs/images/pc-ip-configuration.png" alt="A PC's Desktop with the IP Configuration app open: IPv4 settings on top, and IPv6 set to Automatic with an address learned from the router, the link-local address and the router's link-local address as gateway" width="100%"><p align="center"><sub><b>PC desktop.</b> IP Configuration with DHCP, static and IPv6 autoconfiguration; Command Prompt and Web Browser.</sub></p></td>
<td width="50%" valign="top"><img src="docs/images/serial-wan.png" alt="Two routers joined by a red serial cable with a zigzag in the middle and a small clock next to the DCE end, each with a PC below it" width="100%"><p align="center"><sub><b>Serial WAN links.</b> DCE and DTE ends, clock rate, HDLC or PPP; the clock marks the DCE end.</sub></p></td>
</tr>
</table>

## Why PacketCraft

- **Familiar.** The window is arranged like the network simulator learners already know: menus and
  toolbars, the device box, Realtime and Simulation modes, device windows with Physical, Config,
  CLI, Desktop and Services tabs. Every icon, picture and message is our own work.
- **Real frames.** Devices exchange encoded Ethernet, ARP, IPv4, IPv6, ICMP, NDP, TCP, UDP, DHCP,
  DNS and HTTP frames. Captures open in any packet analyser, and a dropped frame says why.
- **Open.** Labs are readable JSON ([`.pcraft`](docs/file-format.md)); the code is MIT or Apache-2.0.
- **Agent-native.** Every action is a command that agents can run through an **MCP server** or the
  CLI, including typing at any device's console.
- **Everywhere.** One Rust codebase for the desktop and the web. No account, no sign-in, no telemetry.
- **Never loses your work.** No panics in product code, every input treated as hostile, every
  command fuzzed, and a last-resort guard that restores the lab.

## What it simulates

- **Devices:** routers (also one with serial WAN ports), switches (also a multilayer one that
  routes), a hub, PCs, laptops, servers and a printer: 13 generic models.
- **Switching:** MAC learning, VLANs, access and 802.1Q trunk ports, per-VLAN spanning tree,
  EtherChannel (LACP, PAgP, static), VTP, port security, switch VLAN interfaces.
- **Routing:** static, RIP and OSPF routes, router-on-a-stick, multilayer switching, ACLs, NAT and
  PAT, DHCP server and relay; serial links with HDLC or PPP.
- **IPv6:** global, EUI-64 and link-local addresses, neighbour discovery, router advertisements and
  autoconfiguration, static routes, ICMPv6.
- **Services:** servers with HTTP (and a web browser on the PCs), DNS and DHCP; Telnet and SSH
  between devices with local users.
- **Console:** every mode a CCNA course uses, `?` help, Tab completion, `do`, `interface range`,
  `running-config` / `startup-config` / `write` / `reload`, and 206 command forms
  ([`docs/command-reference.md`](docs/command-reference.md)).
- **PC Command Prompt:** `ipconfig` (static or DHCP), `ipv6config` (static or automatic), `ping`,
  `tracert`, `nslookup`, `telnet`, `ssh`, `arp`.

- **Activities:** a lab can carry instructions and checks worth points (configuration lines,
  pings, addresses, command output), with Check Results, Reset Activity and an Activity Wizard
  that captures the answers from a finished lab.

What it doesn't do yet is listed, ranked, in [`docs/gaps.md`](docs/gaps.md): wireless, HSRP and more.

## Quick start

```sh
cargo run --release -p packetcraft                            # desktop app
cargo run --release -p packetcraft -- --sample vlans          # open a sample lab
cargo run --release -p packetcraft -- lab.pcraft              # open a lab
cargo run --release -p packetcraft-cli -- samples             # list the sample labs
cargo xtask ci                                                # fmt, clippy, tests, assets, layering, wasm, parity
```

Click a device to open its window; drag to move it; scroll to pan and Ctrl+scroll to zoom.

### Web

```sh
cd apps/packetcraft-web && trunk build --release     # → dist/web (serve it with any static server)
cd apps/packetcraft-web && trunk serve --release     # http://127.0.0.1:8781  (?sample=vlans opens a sample)
```

You need [trunk](https://trunk-rs.github.io/trunk/) and the `wasm32-unknown-unknown` target. Open
uses the browser's file picker and Save downloads the lab.

## For agents: MCP and CLI

```sh
claude mcp add packetcraft -- packetcraft-cli mcp                       # headless MCP server
packetcraft-cli exec --sample two-subnets R1 enable 'show ip route'     # type at a console
packetcraft-cli run --sample lan --cmd 'net.ping={"from":"PC1","to":"192.168.1.13"}'
packetcraft-cli pcap --sample vlans --ping PC1:192.168.20.10 --out vlans.pcap
packetcraft-cli commands                                                 # every command and its params
```

The MCP server offers `packetcraft_run` (any engine command), `packetcraft_cli` (type lines at a
device), `packetcraft_lab` (devices, ports, addresses and cables) and `packetcraft_commands`. See
[`docs/mcp.md`](docs/mcp.md) and [`docs/cli.md`](docs/cli.md).

## Architecture

PacketCraft is an engine-first Cargo workspace with enforced layering (`cargo xtask layers`). The
egui frontend is a separate crate, so the UI can be swapped without touching the simulator.

| Layer | Crates |
|---|---|
| L0 | `core` (MAC and IP addresses, networks, VLAN ids) |
| L1 | `wire` (frame formats, checksums, pcap) |
| L2 | `model` (the lab: devices, ports, configuration, cables; the `.pcraft` format) |
| L3 | `sim` (discrete-event simulator, switching, routing, STP, OSPF, RIP, VTP, IPv6) |
| L4 | `ios` (device consoles: grammar, configuration, `show` output, host shell) |
| L5 | `engine` (session, command catalog, undo, sample labs) |
| L6 | `ui-egui` (the interface) · `mcp` (MCP server) |
| L7 | `apps/packetcraft`, `apps/packetcraft-cli`, `apps/packetcraft-web` |

- **Contributor and agent rules** (clean room, asset policy, never-crash, quality gates): [`AGENTS.md`](AGENTS.md)
- **How it is built:** [`docs/architecture.md`](docs/architecture.md); decisions in [`docs/decisions.md`](docs/decisions.md)
- **Bundled assets:** each one is listed with its licence in [`ATTRIBUTION.md`](ATTRIBUTION.md)
- **Releases:** not yet; the plan is in [`docs/releasing.md`](docs/releasing.md)

## Roadmap

PacketCraft is **pre-alpha**: every core workflow works end to end (build a lab, configure it by
typing, ping, watch the frames, save), but there are no installers yet, and
the console still misses commands students meet early. Weighted by what CCNA labs rely on, we
estimate it is about 50% ready for real work, and about 70% for the first CCNA course. Next up:
console coverage and CDP, protocol frames in Simulation mode, then activities with scoring.
Stage, numbers and estimates are in [ROADMAP.md](ROADMAP.md); the ranked work list is
[`docs/gaps.md`](docs/gaps.md).

## Clean room

PacketCraft is built from public standards (IEEE, IETF RFCs) and black-box observation of how
network simulators behave. Nobody working on it reads, disassembles or decompiles any vendor's
software or file formats, and no vendor artwork, icons, sample labs or wording is used. For the
same reason it does not open `.pkt` files; see [`docs/decisions.md`](docs/decisions.md).

## License and credits

PacketCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Copyright (c) 2026 the PacketCraft contributors. Required notices are in [NOTICE](NOTICE).

Every icon and device picture is drawn in code and is original; the sample labs and every
screenshot in this README were made with PacketCraft itself. Assets and their licences are listed
in [ATTRIBUTION.md](ATTRIBUTION.md). Contributors: [`docs/contributors.md`](docs/contributors.md).

Cisco, Cisco IOS and Packet Tracer are trademarks of Cisco Systems, Inc. PacketCraft is an
independent project, not affiliated with or endorsed by Cisco.
