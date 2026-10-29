# UI parity

_Updated 2026-10-29. Arrangement observed black-box (`plan/packet-tracer/observed-ui.md`); artwork and wording are ours._

| Element | Reference app | PacketCraft |
|---|---|---|
| Menu bar | File, Edit, Options, View, Tools, Extensions, Window, Help | same menus, our items |
| Main toolbar | file, print, info, copy/paste, undo/redo, zoom, palette… ; help at right | new, open, save, network information, copy/paste devices, undo, redo, zoom in/reset/out, capture export; help at right |
| Second toolbar | select, inspect, delete, resize, note, drawing, simple/complex PDU | select, inspect, delete, place note (working), simple PDU; resize, line, rectangle, ellipse, freeform and complex PDU shown but not available yet |
| Workspace bar | Logical / Physical, coordinates, Root, navigation, clock | Logical / Physical (physical not available yet), coordinates, Root, clock |
| Workspace | white, icons with model + name, port labels, link triangles | same arrangement, icons drawn in code |
| Time bar | time, power cycle, fast forward; Realtime / Simulation | same |
| Device box | categories, search, subcategories, model tiles, model name box | same; 11 generic models (routers, switches, hub, PC, laptop, server, printer) + copper cable |
| Scenario / PDU list | scenario, New/Delete, PDU table | scenario, New/Delete/Fire All, PDU table (Fire, Last Status, Source, Destination, Type, Color, Time, Num, Delete) |
| Simulation Panel | event list (Vis., Time, Last/At Device, Type, Info), Reset Simulation, play controls with speed, filters, Edit Filters / Show All/None; time bar with ms and play controls, Event List toggle | same arrangement; plus drop reasons and pcap export; no Constant Delay option |
| PDU information | OSI model and per-layer details | per-layer decode and hex dump |
| Router/switch window | Physical, Config, CLI, Attributes | same tabs; Config: bordered list with bold sections; Global Settings with Display Name and Hostname, NVRAM Erase/Save, Startup Config Load/Export, Running Config Export/Merge; VLAN database, static routes, interfaces; console output underneath |
| PC window | Physical, Config, Desktop, Programming, Attributes | Physical, Config, Desktop (navy desktop with tiles: IP Configuration with DHCP/Static, Command Prompt), Attributes; only working apps are shown |
| Server window | Physical, Config, Services (HTTP, DHCP, DHCPv6, TFTP, DNS, SYSLOG, AAA, NTP, EMAIL, FTP, IoT…), Desktop, Programming, Attributes | Physical, Config, Services (HTTP with file manager, DHCP pools, DNS records), Desktop, Attributes |
| Web Browser | URL bar with back/forward, Go, Stop; page view | same; renders headings, paragraphs, bold, lists and links |
| Not yet | physical workspace, drawing tools, clusters, complex PDUs, activity wizard, Programming tab | — |
