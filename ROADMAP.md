# PacketCraft roadmap

_Updated 2026-10-29._

**Stage:** pre-alpha (M0 complete: foundations). One-page summary; detail in [`docs/target-app-parity.md`](docs/target-app-parity.md), [`docs/gaps.md`](docs/gaps.md) and [`docs/roadmap.md`](docs/roadmap.md).

## Headline numbers

| | |
|---|---|
| Crates | 8 libraries + 3 apps + xtask |
| Engine commands | 30 (`docs/command-reference.md`) |
| Console commands | 58 grammar entries, 6 modes |
| Device models | 11 (routers with 2/3/4 ports, switches with 8/24/48 ports, hub, PC, laptop, server, printer) |
| Sample labs | 9 |
| Tests | 111 (unit, console scripts, sample labs end to end, UI harness, fuzz of every command) |

## Dimensions (rough parity with the reference app, for a CCNA-level lab)

| Dimension | Status |
|---|---|
| Layer 2: switching, VLANs, trunks, STP | working, with port security, EtherChannel and VTP; no DTP |
| Layer 3: IPv4, IPv6, ARP, NDP, ICMP, static routing, OSPF | working, with RIP; no OSPFv3 or EIGRP |
| Services: DHCP, DNS, HTTP, FTP, email | DHCP (routers and servers), DNS, HTTP with browser; no FTP or email yet |
| Security: ACLs, NAT, SSH, passwords | ACLs, NAT/PAT, passwords, lines, banner; no SSH yet |
| Device console | core modes and commands; configuration round-trips |
| Simulation mode | event list, replay, decode, pcap export; no per-device PDU editor yet |
| Workspace: canvas, cables, notes | working; no clusters, physical view, or activity wizard |
| Files | own JSON format; no `.pkt` (out of scope, see decisions) |
| Platforms | Linux, Windows, macOS, web |
| Agent control | MCP server and CLI over the full command catalog |

## Progress log

- **2026-10-29 — Serial WAN links.** A Router-WAN model with serial ports; serial cables whose first end is the DCE; `clock rate`, `encapsulation hdlc|ppp`, line protocol down without a clock or with mismatched framing; `show controllers`; HDLC/PPP frames on the wire (listed and captured as Ethernet II); red serial cables with a clock at the DCE end; serial-wan sample lab.


- **2026-10-29 — RIP.** `router rip` with version, classful networks, passive interfaces, split horizon, 15-hop limit and default-route origination, computed like OSPF; R routes in `show ip route`, `show ip rip database`, RIP in `show ip protocols`; rip sample lab.


- **2026-10-29 — Multilayer switch.** A Switch-ML-24 model routes with `ip routing` between its VLAN interfaces and routed ports (`no switchport`), with static routes and `show ip route`; drawn with a small router mark; multilayer sample lab.


- **2026-10-29 — IPv6.** Real IPv6/ICMPv6/NDP frames; global, EUI-64 and link-local addresses; neighbour discovery; router advertisements and host autoconfiguration; static routes (link-local next hops too); ping and traceroute; `show ipv6 interface brief|route|neighbors`; IPv6 in the PC IP Configuration and router port pages; ipv6 sample lab.


- **2026-10-29 — VTP.** Servers, clients and transparent switches share VLANs over trunks by domain and password, highest revision first (including the classic overwrite by a switch with a higher revision); the VLAN database survives reloads; `show vtp status`; vtp sample lab.


- **2026-10-29 — EtherChannel.** `channel-group` with LACP, PAgP or static bundling, negotiated from both ends; a bundle is one spanning-tree port and spreads traffic over its cables; `interface port-channel`, `show etherchannel summary`; parallel cables drawn side by side with a ring around each bundle; switches flush learned addresses when the topology changes; etherchannel sample lab.


- **2026-10-29 — Port security, Telnet/SSH, one-click devices.** `switchport port-security` with maximum, violation modes and sticky addresses; ports go err-disabled and come back with `shutdown`/`no shutdown`; `show port-security`. Remote consoles over Telnet and SSH with local users. A single click opens a device, as in the reference app.


- **2026-10-29 — Server services and the web browser.** TCP, DNS and HTTP on the wire; servers run HTTP (file manager), DNS (A records) and DHCP pools from a Services tab like the reference app's; PCs browse by name in a Web Browser; ACLs and NAT understand TCP ports; services sample lab.
- **2026-10-29 — Simulation Panel, Desktop and Config like the reference app.** From black-box screenshots: the Simulation Panel table and controls, play controls in the time bar, the navy PC Desktop with tiles, the router Config tab with Global Settings (display name vs hostname, NVRAM, startup/running config load, export and merge).
- **2026-10-29 — Dynamic routing, ACLs, NAT, DHCP relay.** OSPF (single area, computed convergence); numbered and named ACLs with match counts; static NAT, NAT pools and PAT; DHCP relay with `ip helper-address`; OSPF and NAT/ACL sample labs.
- **2026-10-29 — Spanning tree and switch management.** Per-VLAN spanning tree (root election, root/designated/alternate ports, priorities, portfast, `show spanning-tree`, amber lights); switch VLAN interfaces and default gateway; spanning-tree sample lab.
- **2026-10-29 — DHCP, device security, drawings.** DHCP server on routers (pools, exclusions, bindings, conflict detection) and DHCP clients on hosts; `enable secret`, console/vty passwords with real password prompts, banner, password encryption; rectangle, ellipse and line drawings on the workspace; a DHCP and security sample lab.
- **2026-10-29 — More devices and workspace notes.** Device catalogue with generic models (routers, switches, a hub, PC, laptop, server, printer); hubs repeat frames; notes placed on the workspace; copy/paste of devices; fuller toolbars.
- **2026-10-29 — Reference-app look.** Main window rearranged like the app learners know (menus, two toolbars, navy workspace and time bars, Realtime/Simulation switch, device box, scenario PDU list, Simulation Panel); device windows with Physical / Config / CLI / Desktop / Attributes tabs; Inspect tool. All artwork drawn in code. See `docs/ui-parity.md`.
- **2026-10-29 — M0 foundations.** Workspace with the Craft layout and gates; wire formats; lab model and file format; discrete-event simulator (switching, VLANs, routing, ARP, ICMP, proxy ARP, loop guard); console shell with configuration round-trip; engine command catalog with undo/redo and panic guard; egui app (canvas, consoles, event list with replay and pcap); CLI, MCP, web build.
