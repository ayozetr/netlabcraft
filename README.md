<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/netlabcraft.png" width="128" height="128" alt="NetLabCraft icon: a laptop and a desktop PC cabled to a switch, with a server below it, on a teal tile">
</p>

<h1 align="center">NetLabCraft</h1>

<p align="center">
  <b>Network labs for learning and teaching; an open-source, clean-room network simulator built in pure Rust.</b>
</p>

<p align="center">
  Build a network, configure routers and switches by typing the commands you learn in class,
  and watch every frame cross it. It runs natively on Linux, Windows, macOS and BSD, and in the
  browser via WebAssembly.
</p>

<p align="center">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-2f6f85?style=flat-square&logo=rust&logoColor=white">
  <img alt="Runs on Linux, Windows, macOS, BSD and the web" src="https://img.shields.io/badge/runs%20on-Linux%20%C2%B7%20Windows%20%C2%B7%20macOS%20%C2%B7%20BSD%20%C2%B7%20Web-4f95ad?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-2f6f85?style=flat-square">
  <img alt="Agent-drivable over MCP" src="https://img.shields.io/badge/agents-MCP-4f95ad?style=flat-square">
  <img alt="Interface in English and Spanish" src="https://img.shields.io/badge/interface-English%20%C2%B7%20Espa%C3%B1ol-2f6f85?style=flat-square">
</p>

<br>

<p align="center">
  <img src="docs/images/hero-multilayer.png" alt="NetLabCraft's main window: a multilayer switch in the middle routes between four PCs in two VLANs and reaches a router and a server above it; menus and two toolbars on top, the device box, scenario list and PDU table at the bottom" width="100%">
  <br><sub><b>Inter-VLAN routing on a multilayer switch</b>: one of the twenty-three sample labs (<code>File → New → Sample</code>), built entirely from NetLabCraft commands.</sub>
</p>

<p align="center">
  <a href="#a-look-around">A look around</a> ·
  <a href="#why-netlabcraft">Why NetLabCraft</a> ·
  <a href="#what-it-simulates">What it simulates</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#for-agents-mcp-and-cli">For agents</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#roadmap">Roadmap</a> ·
  <a href="#downloads">Downloads</a> ·
  <a href="#license-and-credits">License and credits</a>
</p>

## A look around

<table>
<tr>
<td width="50%" valign="top"><img src="docs/images/router-cli.png" alt="A router window on its CLI tab: enable, show ip ospf neighbor listing two neighbours in FULL state, and show ip route ospf listing three OSPF routes" width="100%"><p align="center"><sub><b>The console.</b> User, privileged and configuration modes, abbreviations, <code>?</code> help, Tab completion, history and debugs.</sub></p></td>
<td width="50%" valign="top"><img src="docs/images/simulation-mode.png" alt="Simulation mode: an envelope travels along the cable between a switch and a router, and the Simulation Panel on the right lists ARP events with their time, last device, device and type" width="100%"><p align="center"><sub><b>Simulation mode.</b> Every frame in an event list, replayed on the canvas, decoded layer by layer and exportable to pcap.</sub></p></td>
</tr>
<tr>
<td width="50%" valign="top"><img src="docs/images/home-router.png" alt="A home router's window on its Home Router page: LAN address and DHCP range, static Internet settings and the Wi-Fi network; beside it the router, a PC, two Wi-Fi laptops linked by dashed lines and an ISP router" width="100%"><p align="center"><sub><b>Wireless.</b> Home routers with DHCP, NAT and Wi-Fi out of the box, access points, Wi-Fi laptops with WPA2.</sub></p></td>
<td width="50%" valign="top"><img src="docs/images/activity.png" alt="The Instructions window of a graded activity: the title Your first router, numbered steps, Completion 0% of 9 points, the time left and the Check Results and Reset Activity buttons" width="100%"><p align="center"><sub><b>Activities.</b> Instructions, checks worth points, hints, a time limit and a password lock, built with the Activity Wizard.</sub></p></td>
</tr>
<tr>
<td width="50%" valign="top"><img src="docs/images/pc-ip-configuration.png" alt="A PC's Desktop with the IP Configuration app open: IPv4 settings on top, and IPv6 set to Automatic with an address learned from the router, the link-local address and the router's link-local address as gateway" width="100%"><p align="center"><sub><b>PC desktop.</b> IP Configuration with DHCP, static and IPv6 autoconfiguration; Command Prompt and Web Browser.</sub></p></td>
<td width="50%" valign="top"><img src="docs/images/serial-wan.png" alt="Two routers joined by a red serial cable with a zigzag in the middle and a small clock next to the DCE end, each with a PC below it" width="100%"><p align="center"><sub><b>Serial WAN links.</b> DCE and DTE ends, clock rate, HDLC or PPP with PAP and CHAP; the clock marks the DCE end.</sub></p></td>
</tr>
</table>

## Why NetLabCraft

- **Familiar.** The window is arranged like the network simulator learners already know: menus and
  toolbars, the device box, Realtime and Simulation modes, device windows with Physical, Config,
  CLI, Desktop and Services tabs. Every picture and message is our own work.
- **Real frames.** Devices exchange encoded Ethernet, 802.11, ARP, IPv4, IPv6, ICMP, NDP, TCP, UDP,
  DHCP, DHCPv6, DNS, HTTP, FTP, SMTP, POP3, NTP, syslog, TFTP, SNMP, RADIUS, TACACS+, STP, OSPF,
  RIP, EIGRP, HSRP, LLDP, LACP, GRE, IKE and ESP frames. Captures open in any packet analyser, and
  a dropped frame says why.
- **Open.** Labs are readable JSON ([`.nlcraft`](docs/file-format.md)); the code is MIT or Apache-2.0.
- **Agent-native.** Every action is a command that agents can run through an **MCP server** or the
  CLI, including typing at any device's console.
- **In your language.** The interface comes in English and Spanish (Options → Language), the
  system's language by default; the consoles stay in English, as on real equipment.
- **Everywhere.** One Rust codebase for the desktop and the web. No account, no sign-in, no telemetry.
- **Never loses your work.** No panics in product code, every input treated as hostile, every
  command fuzzed, and a last-resort guard that restores the lab.

## What it simulates

- **Devices:** 25 models with our own names: fixed and modular routers (serial, Gigabit, Fast
  Ethernet and fiber cards fitted while switched off), a stateful firewall, access switches (also
  with fiber uplinks) and a multilayer switch, a home wireless router, a Wi-Fi access point, a
  wireless LAN controller with lightweight APs, a hub, PCs, laptops (also with Wi-Fi), servers, a
  printer, IP phones, and IoT sensors and devices.
  [docs/model-equivalents.md](docs/model-equivalents.md) says which one to place when a course lab
  names a vendor model.
- **Cables:** automatic, straight-through and cross-over copper (a wrong one leaves the link down
  and says why, unless auto-MDIX fixes it), fiber, console, and serial DCE/DTE; the physical view
  has cities, buildings and closets, and copper longer than 100 m goes down.
- **Switching:** MAC learning, VLANs, access and 802.1Q trunk ports, DTP, per-VLAN spanning tree
  with its timers, rapid-pvst, cost and port priority, PortFast and BPDU guard, root guard,
  EtherChannel (LACP, PAgP, static) with load balancing, VTP, port security with aging, DHCP
  snooping, dynamic ARP inspection, errdisable recovery, switch VLAN interfaces, CDP and LLDP.
- **Routing:** static and floating static routes, loopbacks, RIP, OSPF as the courses teach it
  (network statements or interface areas, timers, DR/BDR, point-to-point, several areas, stub
  areas, the link-state database), EIGRP with equal and unequal-cost load balancing,
  router-on-a-stick, multilayer switching, HSRP versions 1 and 2 with tracking; serial links with
  HDLC or PPP (PAP, CHAP).
- **IPv6:** global, EUI-64 and link-local addresses on ports, subinterfaces and VLAN interfaces,
  neighbour discovery, router advertisements, SLAAC, DHCPv6 (stateless, stateful, relay), routers
  as SLAAC and DHCPv6 clients, static routes, OSPFv3, IPv6 ACLs.
- **Security:** standard, extended and named ACLs with sequence numbers, `established` and ICMP
  types, IPv6 ACLs, `access-class`, NAT and PAT, passwords and password recovery, SSH, AAA with
  RADIUS and TACACS+, GRE tunnels, site-to-site IPsec with IKE on the wire, and a stateful
  firewall with object groups and twice NAT.
- **QoS:** class maps, policy maps, DSCP marking, policing and output queues on congested links.
- **Services:** servers with HTTP (and a web browser on the PCs), DNS, DHCP, email (SMTP and POP3,
  with an Email app on the PCs), FTP, TFTP, NTP (with authentication), syslog, AAA and IoT; DHCP
  servers on routers with leases, domain names and statistics; SNMP v2c and v3 agents with traps
  and a MIB Browser on the PCs.
- **Wireless:** home routers, access points and a controller with lightweight APs; WPA2-PSK and
  WPA2-Enterprise (checked by RADIUS), a VLAN per WLAN, roaming, and real 802.11 frames in a
  separate capture.
- **Voice and IoT:** IP phones on voice VLANs that register with call control on a router and
  call each other; smart sensors and devices that report to a server's IoT service, which runs
  simple rules.
- **Console:** every mode a CCNA course uses, `?` help with a `^` marker on errors, Tab completion,
  history, `do`, `interface range`, output filters, `running-config` / `startup-config` / `write` /
  `reload`, `copy` to and from TFTP and FTP (images too), timestamps, logging levels, debugs,
  extended ping: 802 grammar entries in all ([`docs/command-reference.md`](docs/command-reference.md)).
- **PC Command Prompt:** `ipconfig` (static or DHCP, `/all`), `ipv6config` (static or automatic,
  `/all`), `ping` (`-n`, `-l`, `-t`), `tracert`, `nslookup`, `telnet`, `ssh`, `ftp`, `netstat`,
  `dir`, `arp -a` and `-d`; also a Terminal app over a console cable.
- **Activities:** a lab can carry instructions and checks worth points (configuration lines,
  pings, addresses, command output), with sections, hints, values per student, a time limit, a
  password lock, Check Results, a comparison with the answer, Reset Activity and an Activity Wizard
  that captures the answers from a finished lab.

What it doesn't do yet is listed, ranked, in [`docs/gaps.md`](docs/gaps.md).

## Quick start

```sh
cargo run --release -p netlabcraft                            # desktop app
cargo run --release -p netlabcraft -- --sample vlans          # open a sample lab
cargo run --release -p netlabcraft -- lab.nlcraft              # open a lab
cargo run --release -p netlabcraft-cli -- samples             # list the sample labs
cargo xtask ci                                                # fmt, clippy, tests, assets, layering, wasm, parity
```

Click a device to open its window; drag to move it; scroll to pan and Ctrl+scroll to zoom.
Choose Español under Options → Language for the Spanish interface.

### Install on Linux (your user only)

```sh
packaging/linux/install.sh            # builds, then installs to ~/.local: menu entry, icon, .nlcraft files
packaging/linux/install.sh --remove   # uninstalls
```

### Web

```sh
cd apps/netlabcraft-web && trunk build --release     # → dist/web (serve it with any static server)
cd apps/netlabcraft-web && trunk serve --release     # http://127.0.0.1:8781  (?sample=vlans opens a sample)
```

You need [trunk](https://trunk-rs.github.io/trunk/) and the `wasm32-unknown-unknown` target. Open
uses the browser's file picker and Save downloads the lab; the browser asks before a tab with
unsaved changes closes.

## For agents: MCP and CLI

```sh
claude mcp add netlabcraft -- netlabcraft-cli mcp                       # headless MCP server
netlabcraft-cli exec --sample two-subnets R1 enable 'show ip route'     # type at a console
netlabcraft-cli run --sample lan --cmd 'net.ping={"from":"PC1","to":"192.168.1.13"}'
netlabcraft-cli pcap --sample vlans --ping PC1:192.168.20.10 --out vlans.pcap
netlabcraft-cli commands                                                 # every command and its params
```

The MCP server offers `netlabcraft_run` (any engine command), `netlabcraft_cli` (type lines at a
device), `netlabcraft_lab` (devices, ports, addresses and cables) and `netlabcraft_commands`. See
[`docs/mcp.md`](docs/mcp.md) and [`docs/cli.md`](docs/cli.md).

## Architecture

NetLabCraft is an engine-first Cargo workspace with enforced layering (`cargo xtask layers`). The
egui frontend is a separate crate, so the UI can be swapped without touching the simulator.

| Layer | Crates |
|---|---|
| L0 | `core` (MAC and IP addresses, networks, VLAN ids) |
| L1 | `wire` (frame formats, checksums, pcap) |
| L2 | `model` (the lab: devices, ports, configuration, cables; the `.nlcraft` format) |
| L3 | `sim` (discrete-event simulator: switching, switch security, routing, STP, OSPF, RIP, EIGRP, HSRP, IPv6, VPN, QoS, firewall, wireless, voice, IoT, services) |
| L4 | `ios` (device consoles: grammar, configuration, `show` output, host shell) |
| L5 | `engine` (session, command catalog, undo, sample labs, activities) |
| L6 | `ui-egui` (the interface and its translations) · `mcp` (MCP server) |
| L7 | `apps/netlabcraft`, `apps/netlabcraft-cli`, `apps/netlabcraft-web` |

- **Contributor and agent rules** (clean room, asset policy, never-crash, quality gates): [`AGENTS.md`](AGENTS.md)
- **How it is built:** [`docs/architecture.md`](docs/architecture.md); decisions in [`docs/decisions.md`](docs/decisions.md)
- **Bundled assets:** each one is listed with its licence in [`ATTRIBUTION.md`](ATTRIBUTION.md)
- **Releases:** on hold; the plan is in [`docs/releasing.md`](docs/releasing.md)

## Roadmap

NetLabCraft is **pre-alpha**: every core workflow works end to end (build a lab, configure it by
typing, ping, watch the frames, grade an activity, save), and the console covers the commands of
the first labs, but nobody has yet walked a full CCNA lab book through it. Weighted by what CCNA
labs rely on, we estimate it is about 84% ready for real work: about 86% for the first CCNA
course, 85% for the second and 79% for the third (estimated; the method is in
[`docs/target-app-parity.md`](docs/target-app-parity.md#audiences)). Next up: a CCNA-style lab
book run end to end, automation for the third course, a zone-based firewall and Simulation-mode
depth. Stage, numbers and estimates are in [ROADMAP.md](ROADMAP.md); the ranked work list is
[`docs/gaps.md`](docs/gaps.md).

## Downloads

There is no release, and releases are on hold for now: build from source with the
[Quick start](#quick-start), or install for your user on Linux with `packaging/linux/install.sh`.
How releases would work (versions, installers for Linux, Windows, macOS, FreeBSD and the web,
file names) is in [`docs/releasing.md`](docs/releasing.md).

## Clean room

NetLabCraft is built from public standards (IEEE, IETF RFCs) and black-box observation of how
network simulators behave. Nobody working on it reads, disassembles or decompiles any vendor's
software or file formats, and no vendor artwork, icons, sample labs or wording is used. Protocols
whose formats aren't public (CDP, DTP, PAgP) are simulated without their frames. For the same
reason it does not open `.pkt` files; see [`docs/decisions.md`](docs/decisions.md).

## License and credits

NetLabCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Copyright (c) 2026 the NetLabCraft contributors. Required notices are in [NOTICE](NOTICE).

Bundled icons, images and other assets keep their own open licences; each one is listed with its
author, source and licence in [ATTRIBUTION.md](ATTRIBUTION.md).

The app icon ([`assets/app-icon/`](assets/app-icon/)) was supplied by the project owner. Every
other icon and device picture is drawn in code and is original; the sample labs and every
screenshot in this README were made with NetLabCraft itself. NetLabCraft bundles no fonts beyond
the ones its UI toolkit ships with. Contributors: [`docs/contributors.md`](docs/contributors.md).

Cisco, Cisco IOS and Packet Tracer are trademarks of Cisco Systems, Inc. NetLabCraft is an
independent project, not affiliated with or endorsed by Cisco.
