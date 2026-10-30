# UI parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (cable types, terminal) · **Target:** Cisco Packet Tracer 9.0

The window, element by element, against the reference app. Part of
[target-app-parity.md](target-app-parity.md). The arrangement was observed black-box (local
`plan/packet-tracer/observed-ui.md`); artwork and wording are ours. **Workspace and windows:
~63% ready, 40–60 h; Simulation mode: ~55% ready, 30–45 h** (estimated).

| Element | Reference app | PacketCraft |
|---|---|---|
| Menu bar | File, Edit, Options, View, Tools, Extensions, Window, Help | same menus, our items; File ▸ Open Recent on the desktop; File ▸ Print… makes an A4 PDF of the workspace and opens it in the system's PDF viewer to print (the web build downloads it); the web build asks before closing a tab with unsaved changes |
| Main toolbar | file, print, info, copy/paste, undo/redo, zoom, palette… ; help at right | new, open, save, network information, copy/paste devices, undo, redo, zoom in/reset/out, capture export; help at right |
| Second toolbar | select, inspect, delete, resize, note, drawing, simple/complex PDU | select, inspect, delete, place note, line, rectangle, ellipse, freehand, polygon, simple PDU, group into a cluster; resize and complex PDU shown but not available yet |
| Workspace bar | Logical / Physical, coordinates, Root, navigation, clock | Logical / Physical, coordinates, a clickable Root › cluster › … path with back, clock |
| Workspace | white, icons with model + name, port labels, link triangles; red serial cables with a clock at the DCE; EtherChannel marks | same arrangement, icons drawn in code; parallel cables side by side, a ring around each EtherChannel, red zigzag serial cables with a clock at the DCE end, a router mark on multilayer switches |
| Time bar | time, power cycle, fast forward; Realtime / Simulation | same |
| Device box | categories, search, subcategories, model tiles, model name box | same; 16 generic models (routers incl. Router-WAN with serial ports, switches incl. multilayer Switch-ML-24, Home-Router, AccessPoint, hub, PC, laptops incl. Wi-Fi, server, printer) + one cable that is copper or serial by the ports; the port menu greys out ports of the other cable type |
| Scenario / PDU list | scenario, New/Delete, PDU table | scenario, New/Delete/Fire All, PDU table (Fire, Last Status, Source, Destination, Type, Color, Time, Num, Delete) |
| Simulation Panel | event list (Vis., Time, Last/At Device, Type, Info), Reset Simulation, play controls with speed, filters, Edit Filters / Show All/None; time bar with ms and play controls, Event List toggle | same arrangement; plus drop reasons and pcap export; no Constant Delay option |
| PDU information | OSI model and per-layer details | per-layer decode and hex dump |
| Router/switch window | Physical, Config, CLI, Attributes | same tabs; Config: bordered list with bold sections; Global Settings with Display Name and Hostname, NVRAM Erase/Save, Startup Config Load/Export, Running Config Export/Merge; VLAN database, static routes, interfaces; console output underneath |
| PC window | Physical, Config, Desktop, Programming, Attributes | Physical, Config, Desktop (navy desktop with tiles: IP Configuration with DHCP/Static and IPv6 Off/Automatic/Static, Command Prompt, Web Browser), Attributes; only working apps are shown |
| Server window | Physical, Config, Services (HTTP, DHCP, DHCPv6, TFTP, DNS, SYSLOG, AAA, NTP, EMAIL, FTP, IoT…), Desktop, Programming, Attributes | Physical, Config, Services (HTTP with file manager, DHCP pools, DNS records, SYSLOG messages, NTP, TFTP files), Desktop, Attributes |
| Web Browser | URL bar with back/forward, Go, Stop; page view | same; renders headings, paragraphs, bold, lists and links |
| Activities | Instructions window with timer and score, Check Results tree, Reset Activity; Activity Wizard with answer network | Instructions window with Completion, a timer, Check Results per section, Compare with the Answer (each check's answer next to the lab, answers kept back when hidden) and Reset Activity; Activity Wizard (title, instructions, values per student, capture answers, ping/config/command/address checks, sections, hints, hidden answers, time limit, password lock); no answer-network tree |
| Clusters | grouped devices shown as one cloud; open, rename, uncluster | same: pick with Shift+drag or Shift+click, Ctrl+G groups, a code-drawn cloud with the device count, double-click to open, nesting, cables to the cloud at the parent level |
| Physical workspace | Intercity map, cities, buildings, wiring closets; cable lengths | same levels with code-drawn icons and a path bar (Intercity › City › Building › Closet); new places, rename, remove, move devices between places; cables between places labelled with their length, copper over 100 m goes down and says why; lengths from the places or set by hand. No wireless range by distance or fibre |
| Connections | cable types in the device box | Automatic, Console, Copper Straight-Through, Copper Cross-Over, Fiber, Serial DCE, Serial DTE tiles with drawn glyphs; the port menu greys out ports a cable doesn't fit and says why; each kind drawn its own way; the PC's Terminal app over a console cable |
| Not yet | — | complex PDUs, Programming tab, protocol frames (BPDU, hellos) in the event list |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Cable types in Connections; PC Terminal app; device box tiles in wrapping rows with two-line names. |
| 2026-10-30 | minor | Physical workspace: places, path bar, cable lengths with an effect on links. |
| 2026-10-30 | minor | File ▸ Print: the workspace as an A4 PDF for the system's viewer to print. |
| 2026-10-30 | minor | Activities: Compare with the Answer; the Activities row brought up to date. |
| 2026-10-30 | minor | Freehand and polygon drawings; multi-selection and nested clusters; About ▸ Contributors. |
| 2026-10-30 | minor | File ▸ Export Workspace Image (PNG); the PC Email app; the model count. |
| 2026-10-30 | minor | Open Recent; unsaved-changes guard on the web; Activities row. |
| 2026-10-30 | minor | Shared format; serial cables, EtherChannel rings, multilayer and WAN models, IPv6 in the PC IP Configuration and router port pages, drawings. |
| 2026-10-29 | major | Created from black-box screenshots of the main window, Simulation mode, the PC Desktop and Web Browser, the router Config tab and the server Services pages. |
