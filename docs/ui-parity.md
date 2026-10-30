# UI parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (UI fixes from the audit; stale rows corrected; figures aligned with ROADMAP.md) · **Target:** Cisco Packet Tracer 9.0

The window, element by element, against the reference app. Part of
[target-app-parity.md](target-app-parity.md). The arrangement was observed black-box (local
`plan/packet-tracer/observed-ui.md`); artwork and wording are ours. **Workspace and windows:
~80% ready, 15–25 h; Simulation mode: ~70% ready, 18–28 h** (estimated; the same figures as
[`ROADMAP.md`](../ROADMAP.md#dimensions)).

| Element | Reference app | NetLabCraft |
|---|---|---|
| Menu bar | File, Edit, Options, View, Tools, Extensions, Window, Help | same menus, our items; File ▸ Open Recent on the desktop; File ▸ Print… makes an A4 PDF of the workspace and opens it in the system's PDF viewer to print (the web build downloads it); the web build asks before closing a tab with unsaved changes |
| Main toolbar | file, print, info, copy/paste, undo/redo, zoom, palette… ; help at right | new, open, save, network information, copy/paste devices, undo, redo, zoom in/reset/out, capture export; help at right |
| Second toolbar | select, inspect, delete, resize, note, drawing, simple/complex PDU | select, inspect, delete, place note, line, rectangle, ellipse, freehand, polygon, simple PDU, group into a cluster; resize and complex PDU shown but not available yet |
| Workspace bar | Logical / Physical, coordinates, Root, navigation, clock | Logical / Physical, coordinates, a clickable Root › cluster › … path with back, clock |
| Opening a lab | the view as saved | a lab that opens or is created is fitted to the workspace: centred and zoomed out (never past 1:1) with room for labels and notes; opening a cluster and switching views fit the same way |
| Workspace | white, icons with model + name, port labels, link triangles; red serial cables with a clock at the DCE; EtherChannel marks | same arrangement, icons drawn in code; device names on a soft plate, port labels kept clear of name blocks and of each other; parallel cables side by side, a ring around each EtherChannel, red zigzag serial cables with a clock at the DCE end, a router mark on multilayer switches |
| Time bar | time, power cycle, fast forward; Realtime / Simulation | same; the right-hand tabs are placed first and the hint and captions give way, so nothing overlaps in either language or mode (tested) |
| Device box | categories, search, subcategories, model tiles, model name box | same; 25 models of our own (routers, modular routers, a firewall, switches incl. a multilayer one and one with fiber uplinks, a home router, an access point, a wireless controller and lightweight APs, a hub, PCs, laptops incl. Wi-Fi, a server, a printer, an IP phone, IoT sensors and devices) in wrapping rows with two-line names; the cable tiles are under Connections |
| Scenario / PDU list | scenario, New/Delete, PDU table | scenario, New/Delete/Fire All, PDU table (Fire, Last Status, Source, Destination, Type, Color, Time, Num, Delete) |
| Simulation Panel | event list (Vis., Time, Last/At Device, Type, Info), Reset Simulation, play controls with speed, filters, Edit Filters / Show All/None; time bar with ms and play controls, Event List toggle | same arrangement, with columns that fit the panel; protocol frames (BPDUs, OSPF, RIP, EIGRP, HSRP, LLDP, LACP, DHCP, DNS, TCP…) listed with their own filters and colours; plus drop reasons and pcap export; no Constant Delay option |
| PDU information | OSI model and per-layer details | per-layer decode and hex dump, in a window that opens below the toolbars; not yet the reference app's OSI-model layout |
| Router/switch window | Physical, Config, CLI, Attributes | same tabs; Config: bordered list with bold sections; Global Settings with Display Name and Hostname, NVRAM Erase/Save, Startup Config Load/Export, Running Config Export/Merge; VLAN database, static routes, interfaces; console output underneath |
| PC window | Physical, Config, Desktop, Programming, Attributes | Physical, Config, Desktop (navy desktop with tiles: IP Configuration with DHCP/Static and IPv6 Off/Automatic/Static, Command Prompt, Web Browser, Email, MIB Browser, Terminal over a console cable), Attributes; only working apps are shown; no Programming tab |
| Server window | Physical, Config, Services (HTTP, DHCP, DHCPv6, TFTP, DNS, SYSLOG, AAA, NTP, EMAIL, FTP, IoT…), Desktop, Programming, Attributes | Physical, Config, Services (HTTP with file manager, DHCP pools, DNS records, SYSLOG messages, NTP, TFTP files, AAA clients and users, FTP accounts and files, EMAIL domain and users), an IoT tab for the IoT service, Desktop, Attributes; no Programming tab |
| Web Browser | URL bar with back/forward, Go, Stop; page view | same; renders headings, paragraphs, bold, lists and links |
| Activities | Instructions window with timer and score, Check Results tree, Reset Activity; Activity Wizard with answer network | Instructions window with Completion, a timer, Check Results per section (several rows visible), Compare with the Answer (each check's answer next to the lab, answers kept back when hidden) and Reset Activity; Activity Wizard (title, instructions, values per student, capture answers, ping/config/command/address checks, sections, hints, hidden answers, time limit, password lock); no answer-network tree |
| Clusters | grouped devices shown as one cloud; open, rename, uncluster | same: pick with Shift+drag or Shift+click, Ctrl+G groups, a code-drawn cloud with the device count, double-click to open, nesting, cables to the cloud at the parent level |
| Physical workspace | Intercity map, cities, buildings, wiring closets; cable lengths | same levels with code-drawn icons and a path bar (Intercity › City › Building › Closet); new places, rename, remove, move devices between places; cables between places labelled with their length, copper over 100 m goes down and says why; lengths from the places or set by hand; fiber runs up to 2 km. No wireless range by distance |
| Connections | cable types in the device box | Automatic, Console, Copper Straight-Through, Copper Cross-Over, Fiber, Serial DCE, Serial DTE tiles with drawn glyphs; the port menu greys out ports a cable doesn't fit and says why; each kind drawn its own way; the PC's Terminal app over a console cable |
| Dialogs | open over the workspace | About, Compare with the Answer, PDU information and the Activity Wizard open below the toolbars; IP Configuration and Global Settings laid out on a grid |
| Language | English plus community files | English and Spanish; every window, tab, page and mode is swept by a test that fails on English text left untranslated ([localization-parity.md](localization-parity.md)) |
| Not yet | — | complex PDUs, the Programming tab, Resize Shape, the OSI-model PDU layout |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | UI fixes from the audit: fit to view on open, time bar without overlaps, Spanish coverage with tests, dialogs below the toolbars, label placement, event list columns. Stale rows corrected: model count, cables (under Connections), protocol frames in the event list (done), server services and PC apps listed in full; figures aligned with ROADMAP.md. |
| 2026-10-30 | minor | Cable types in Connections; PC Terminal app; device box tiles in wrapping rows with two-line names. |
| 2026-10-30 | minor | Physical workspace: places, path bar, cable lengths with an effect on links. |
| 2026-10-30 | minor | File ▸ Print: the workspace as an A4 PDF for the system's viewer to print. |
| 2026-10-30 | minor | Activities: Compare with the Answer; the Activities row brought up to date. |
| 2026-10-30 | minor | Freehand and polygon drawings; multi-selection and nested clusters; About ▸ Contributors. |
| 2026-10-30 | minor | File ▸ Export Workspace Image (PNG); the PC Email app; the model count. |
| 2026-10-30 | minor | Open Recent; unsaved-changes guard on the web; Activities row. |
| 2026-10-30 | minor | Shared format; serial cables, EtherChannel rings, multilayer and WAN models, IPv6 in the PC IP Configuration and router port pages, drawings. |
| 2026-10-29 | major | Created from black-box screenshots of the main window, Simulation mode, the PC Desktop and Web Browser, the router Config tab and the server Services pages. |
