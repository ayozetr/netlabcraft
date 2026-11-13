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
  <br><sub><b>Inter-VLAN routing on a multilayer switch</b>: one of the thirty-one sample labs (<code>File → New → Sample</code>), built entirely from NetLabCraft commands.</sub>
</p>

<p align="center">
  <a href="#a-look-around">A look around</a> ·
  <a href="#why-netlabcraft">Why NetLabCraft</a> ·
  <a href="#what-it-simulates">What it simulates</a> ·
  <a href="#the-ccna-lab-book">Lab book</a> ·
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
  DHCP, DHCPv6, DNS, HTTP, TLS 1.2, SSH 2.0, RESTCONF, NETCONF, FTP, SMTP, POP3, NTP, syslog,
  TFTP, SNMP, RADIUS with EAP, PEAP, CAPWAP, TACACS+, SIP, STP and RSTP, OSPF, RIP, RIPng, EIGRP,
  HSRP, VRRP, LLDP, LACP, GRE, IKEv1, IKEv2 and ESP frames, IPv4 fragments included; their
  Diffie-Hellman exchanges and AES encryption are computed for real. Captures open in any
  packet analyser, and a dropped frame says why.
- **Open.** Labs are readable JSON ([`.nlcraft`](docs/file-format.md)); the code is MIT or Apache-2.0.
- **Agent-native.** Every action is a command that agents can run through an **MCP server** or the
  CLI, including typing at any device's console.
- **In your language.** The interface comes in English and Spanish (Options → Language), the
  system's language by default; the consoles stay in English, as on real equipment.
- **Everywhere.** One Rust codebase for the desktop and the web. No account, no sign-in, no telemetry.
- **Never loses your work.** No panics in product code, every input treated as hostile, every
  command fuzzed, and a last-resort guard that restores the lab.

## What it simulates

- **Devices:** 51 models with our own names: fixed and modular routers (serial, Gigabit, Fast
  Ethernet, fiber and 4-port switch cards fitted while switched off), a stateful firewall, access
  switches (also with fiber uplinks or power over Ethernet) and a multilayer switch, a home
  wireless router and a home IoT gateway, a Wi-Fi access point, a wireless LAN controller with
  lightweight APs, a provider cloud with DSL and cable modems, a hub, PCs, laptops (also with
  Wi-Fi), tablets and smartphones (Wi-Fi only) with a touch home screen, servers, a printer, IP
  phones with their own Settings screen, eighteen IoT things (temperature sensor, fan, motion and
  smoke detectors, light, door lock, siren, webcam, and over Wi-Fi a thermostat, heater, air
  conditioner, humidity sensor, smart window, soil moisture sensor, lawn sprinkler, CO detector,
  smart plug and garage door), and two programmable boards. Routers and switches have
  power-supply bays: take out the last supply and the device switches off.
  [docs/model-equivalents.md](docs/model-equivalents.md) says which one to place when a course lab
  names a vendor model.
- **Cables:** automatic, straight-through and cross-over copper (a wrong one leaves the link down
  and says why, unless auto-MDIX fixes it), fiber, console, serial DCE/DTE, coaxial and phone
  lines, and IoT custom cables from a board's pin to a smart thing; the physical view has cities,
  buildings and closets, copper longer than 100 m goes down, and once a lab has a layout, Wi-Fi
  reaches only the access points in the same building.
- **Switching:** MAC learning, VLANs, access and 802.1Q trunk ports, DTP, per-VLAN spanning tree
  with its timers, rapid-pvst with the proposal/agreement handshake, cost and port priority,
  PortFast, BPDU guard and filter, root and loop guard, EtherChannel (LACP, PAgP, static) with load
  balancing, VTP with pruning, port security with aging, DHCP snooping with option 82, dynamic ARP
  inspection, errdisable recovery, switch VLAN interfaces, CDP and LLDP, 802.1p CoS from IP phones
  with trust and CoS-to-DSCP maps; power over Ethernet with a budget per switch; switch cards that
  make a router switch too.
- **Routing:** static and floating static routes, loopbacks, RIP with its timers, hold-down,
  triggered updates and poisoned reverse, OSPF as the courses teach it
  (network statements or interface areas, timers, DR/BDR, point-to-point, several areas, stub
  areas, the link-state database), EIGRP with equal and unequal-cost load balancing and
  summaries, router-on-a-stick, multilayer switching, HSRP versions 1 and 2 with tracking, VRRP
  versions 2 and 3 with timers and tracking, GLBP; serial links with HDLC or PPP (PAP, CHAP).
- **IPv6:** global, EUI-64 and link-local addresses on ports, subinterfaces and VLAN interfaces,
  neighbour discovery, router advertisements, SLAAC, DHCPv6 (stateless, stateful, relay), routers
  as SLAAC and DHCPv6 clients, static routes, OSPFv3, EIGRP for IPv6 (updates, queries and
  replies on the wire, variance, summaries), RIPng, VRRPv3, IPv6 ACLs.
- **Security:** standard, extended and named ACLs with sequence numbers, `established` and ICMP
  types, IPv6 ACLs, `access-class`, NAT and PAT, passwords and password recovery, SSH, AAA with
  RADIUS and TACACS+, GRE tunnels, site-to-site IPsec with IKEv1 or IKEv2 on the wire with a real
  Diffie-Hellman exchange (MODP groups 1, 2, 5, 14, 15, 16), real AES-128/192/256, DES, 3DES, AH and SHA-1,
  SHA-256 or MD5 HMACs, SA lifetimes and rekeying, a zone-based firewall and `ip inspect` (FTP,
  DNS and HTTP too) on routers, a default MTU of 1500 bytes with path MTU discovery, `ip mtu`
  with fragmentation and `ip tcp adjust-mss`, and a stateful firewall with object groups and
  twice NAT.
- **QoS:** class maps, policy maps, DSCP marking, policing, shaping and output queues on congested
  links; `mls qos trust`, CoS-to-DSCP maps and `priority extend` on switches.
- **Automation:** routers answer RESTCONF (JSON or XML, over IPv4 or IPv6, over TLS 1.2 on port
  443) and NETCONF over SSH 2.0 (running and candidate datastores, commit, validate, locks); PCs
  have a Programming app with saved requests and a controller mode that discovers routers over
  CDP and LLDP, keeps an inventory and pushes templates with per-router variables.
- **Services:** servers with HTTP (and a web browser on the PCs), DNS, DHCP, email (SMTP and POP3,
  with an Email app on the PCs), FTP, TFTP, NTP (with authentication), syslog, AAA and IoT; DHCP
  servers on routers with leases, domain names and statistics; SNMP v2c and v3 agents with traps
  and a MIB Browser on the PCs.
- **Wireless:** home routers, access points and a controller with lightweight APs; WPA2-PSK and
  WPA2-Enterprise through a real PEAP tunnel with EAP over RADIUS, group keys, a VLAN per WLAN,
  local or central switching (CAPWAP data to the controller), roaming, and real 802.11 frames in
  a separate capture.
- **Voice and IoT:** IP phones on voice VLANs, powered over Ethernet or by their adapter, that
  register with call control on a router (its own signalling or SIP) and call each other; smart
  things that report on a timer to a server's IoT service or a home gateway, whose rules join
  several conditions with AND or OR; Wi-Fi things join the home gateway's network by
  themselves; programmable boards run BoardScript, a small language of our own, reading and
  switching the things wired to their pins.
- **Simulation mode:** an event list replayed on the canvas, simple and complex PDUs (ping, DNS,
  HTTP, Telnet, SSH, FTP, email, DHCP, TFTP, SNMP, ARP, raw UDP and TCP; once or periodically),
  start times relative or absolute, PDU details in OSI layers with what the sender and each
  receiver decided, scenarios saved with the lab, Constant Delay, capturing only chosen
  protocols, drop reasons and pcap export.
- **Console:** every mode a CCNA course uses, `?` help with a `^` marker on errors, Tab completion,
  history, `do`, `interface range`, output filters, `running-config` / `startup-config` / `write` /
  `reload`, `copy` to and from TFTP and FTP (images too), timestamps, logging levels, debugs,
  extended ping: 1003 grammar entries in all ([`docs/command-reference.md`](docs/command-reference.md)).
- **PC Command Prompt:** `ipconfig` (static or DHCP, `/all`), `ipv6config` (static or automatic,
  `/all`), `ping` (`-n`, `-l`, `-t`), `tracert`, `nslookup`, `telnet`, `ssh`, `ftp`, `netstat`,
  `dir`, `arp -a` and `-d`; also a Terminal app over a console cable.
- **Activities:** a lab can carry instructions (in several languages) and checks worth points
  (configuration lines, pings, addresses, cables, command output), with sections, hints, values
  per student, a time limit, a password lock, Check Results, a comparison with the answer, Reset
  Activity and an Activity Wizard. Instructors keep the answer network in the activity (sealed
  under the lock password when locked), weigh an assessment tree built from it, set an initial
  network, run connectivity tests, export signed results and lock devices, cables or
  configurations.

What it doesn't do yet is listed, ranked, in [`docs/gaps.md`](docs/gaps.md).

## The CCNA lab book

[`docs/labs/`](docs/labs/README.md) holds twenty hands-on labs written for NetLabCraft over the
topics of the three CCNA courses: four for the first (device basics and SSH, dual-stack
addressing, VLSM, ARP, ICMP and DHCP in Simulation mode), nine for the second (VLANs and trunks,
inter-VLAN routing, STP, EtherChannel, DHCPv4 and DHCPv6, HSRP, switch security, a wireless
controller, floating static routes) and seven for the third (OSPF, ACLs, NAT, GRE over IPsec,
QoS, network management, RESTCONF). Each lab lists its topology, addressing, every command to
type and what to check. Every lab is also a test: the suite runs each one end to end as
written, and eight of them through the MCP server, so a lab that passes there works when typed
by hand.

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
| L3 | `sim` (discrete-event simulator: switching, switch security, routing, STP, OSPF, RIP, RIPng, EIGRP, HSRP, VRRP, GLBP, IPv6, VPN with IKEv1 and IKEv2, QoS, firewalls, MTU, wireless with PEAP and central switching, voice and SIP, IoT, boards and BoardScript, PoE, automation over TLS and SSH, services, complex PDUs, Simulation-mode options) |
| L4 | `ios` (device consoles: grammar, configuration, `show` output, host shell) |
| L5 | `engine` (session, command catalog, undo, sample labs, activities and instructor tools, RESTCONF, NETCONF and the controller, boards) |
| L6 | `ui-egui` (the interface and its translations) · `mcp` (MCP server) |
| L7 | `apps/netlabcraft`, `apps/netlabcraft-cli`, `apps/netlabcraft-web` |

- **Contributor and agent rules** (clean room, asset policy, never-crash, quality gates): [`AGENTS.md`](AGENTS.md); how to contribute: [`CONTRIBUTING.md`](CONTRIBUTING.md); security reports: [`SECURITY.md`](SECURITY.md)
- **How it is built:** [`docs/architecture.md`](docs/architecture.md); decisions in [`docs/decisions.md`](docs/decisions.md)
- **Bundled assets:** each one is listed with its licence in [`ATTRIBUTION.md`](ATTRIBUTION.md)
- **Releases:** installers for Linux, Windows, macOS and the browser on the [Releases page](https://github.com/ayozetr/netlabcraft/releases) (not code-signed yet); how they are built: [`docs/releasing.md`](docs/releasing.md)

## Roadmap

NetLabCraft is **pre-alpha** only because releases are on hold: every core workflow works end
to end (build a lab, configure it by typing, ping, watch the frames, grade an activity, save),
the twenty-lab [CCNA lab book](docs/labs/README.md) runs as written, and every alpha and beta
gate but the packaged build is met. Weighted by what CCNA labs rely on, we estimate it is about
91% ready for real work: about 93% for the first CCNA course, 94% for the second and 90% for the
third (estimated; the method is in
[`docs/target-app-parity.md`](docs/target-app-parity.md#audiences)). Next up: more labs walked
to find console gaps, the remaining crypto stand-ins (elliptic-curve groups) and certificates, and a
cellular network for the mobile devices. Stage, numbers and estimates are in
[ROADMAP.md](ROADMAP.md); the ranked work list is [`docs/gaps.md`](docs/gaps.md).

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
