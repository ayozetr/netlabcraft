# UI parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (serial cables, multilayer and WAN models, IPv6 settings, drawings; shared format) · **Target:** Cisco Packet Tracer 9.0

The window, element by element, against the reference app. Part of
[target-app-parity.md](target-app-parity.md). The arrangement was observed black-box (local
`plan/packet-tracer/observed-ui.md`); artwork and wording are ours. **Workspace and windows:
~55% ready, 60–90 h; Simulation mode: ~55% ready, 30–45 h** (estimated).

| Element | Reference app | PacketCraft |
|---|---|---|
| Menu bar | File, Edit, Options, View, Tools, Extensions, Window, Help | same menus, our items |
| Main toolbar | file, print, info, copy/paste, undo/redo, zoom, palette… ; help at right | new, open, save, network information, copy/paste devices, undo, redo, zoom in/reset/out, capture export; help at right |
| Second toolbar | select, inspect, delete, resize, note, drawing, simple/complex PDU | select, inspect, delete, place note, line, rectangle, ellipse, simple PDU; resize, freeform and complex PDU shown but not available yet |
| Workspace bar | Logical / Physical, coordinates, Root, navigation, clock | Logical / Physical (physical not available yet), coordinates, Root, clock |
| Workspace | white, icons with model + name, port labels, link triangles; red serial cables with a clock at the DCE; EtherChannel marks | same arrangement, icons drawn in code; parallel cables side by side, a ring around each EtherChannel, red zigzag serial cables with a clock at the DCE end, a router mark on multilayer switches |
| Time bar | time, power cycle, fast forward; Realtime / Simulation | same |
| Device box | categories, search, subcategories, model tiles, model name box | same; 13 generic models (routers incl. Router-WAN with serial ports, switches incl. multilayer Switch-ML-24, hub, PC, laptop, server, printer) + one cable that is copper or serial by the ports; the port menu greys out ports of the other cable type |
| Scenario / PDU list | scenario, New/Delete, PDU table | scenario, New/Delete/Fire All, PDU table (Fire, Last Status, Source, Destination, Type, Color, Time, Num, Delete) |
| Simulation Panel | event list (Vis., Time, Last/At Device, Type, Info), Reset Simulation, play controls with speed, filters, Edit Filters / Show All/None; time bar with ms and play controls, Event List toggle | same arrangement; plus drop reasons and pcap export; no Constant Delay option |
| PDU information | OSI model and per-layer details | per-layer decode and hex dump |
| Router/switch window | Physical, Config, CLI, Attributes | same tabs; Config: bordered list with bold sections; Global Settings with Display Name and Hostname, NVRAM Erase/Save, Startup Config Load/Export, Running Config Export/Merge; VLAN database, static routes, interfaces; console output underneath |
| PC window | Physical, Config, Desktop, Programming, Attributes | Physical, Config, Desktop (navy desktop with tiles: IP Configuration with DHCP/Static and IPv6 Off/Automatic/Static, Command Prompt, Web Browser), Attributes; only working apps are shown |
| Server window | Physical, Config, Services (HTTP, DHCP, DHCPv6, TFTP, DNS, SYSLOG, AAA, NTP, EMAIL, FTP, IoT…), Desktop, Programming, Attributes | Physical, Config, Services (HTTP with file manager, DHCP pools, DNS records, SYSLOG messages, NTP, TFTP files), Desktop, Attributes |
| Web Browser | URL bar with back/forward, Go, Stop; page view | same; renders headings, paragraphs, bold, lists and links |
| Activities | Instructions window with timer and score, Check Results tree, Reset Activity; Activity Wizard with answer network | Instructions window with Completion, Check Results list and Reset Activity; Activity Wizard (title, instructions, capture answers, ping/config/command checks, hidden answers); no timer, answer-network tree or locking |
| Not yet | — | physical workspace, freeform drawing, clusters, complex PDUs, Programming tab, protocol frames (BPDU, hellos) in the event list |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Shared format; serial cables, EtherChannel rings, multilayer and WAN models, IPv6 in the PC IP Configuration and router port pages, drawings. |
| 2026-10-29 | major | Created from black-box screenshots of the main window, Simulation mode, the PC Desktop and Web Browser, the router Config tab and the server Services pages. |
