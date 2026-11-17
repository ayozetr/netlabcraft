# UI parity

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** minor (User Profile has E-Mail and Additional Info, kept local; View ▸ View Wireless Signals (Ctrl+Alt+W) and a button on the physical bar draw each radio's coverage and a line from each associated Wi-Fi host) · also merged with minor (the stale duplicate Connections row removed) · also merged with minor (wiring closet: cables made by clicking a cable tile and two ports; device-box tiles dragged onto the workspaces) · earlier: minor (Connections: the cable tiles show connector pictures (RJ45, RJ11, duplex fiber, coaxial, D-shell, octal fan, bare wires, USB) instead of diagonal lines) · also merged with minor (Keyboard shortcuts: one table in code, 75 shortcuts, the Open Samples window and a Help page) · also merged with minor (Connections: the tiles in the reference order (Automatically Choose Connection Type first, then Console, Copper Straight-Through, Copper Cross-Over, Single Mode Fiber, Multi Mode Fiber, Phone, Coaxial, Serial DCE, Serial DTE, Octal, IoT Custom Cable, USB), fiber split in two modes, Octal and USB cables) · also merged with minor (device model count: 65, a duplicated summary row removed) · also merged with minor (Physical workspace: wiring closets draw racks, shelves and tables, front panels and cables port to port; Structured Cabling in the device box) · also merged with minor (Options ▸ Preferences, User Profile and View Command Log, with 7 engine commands and 65 catalog rows) · also merged with minor (Menus: Edit lists Copy and Paste, View gains Zoom and Toolbars submenus, the modes and views with shortcuts, Show Viewport and Show Workspace List, Tools gains Drawing Palette, Help gains pages) · earlier: minor (PC desktop: Dial-up app; the Telephone Cloud's numbers) · also merged with minor (cables as accessible nodes, native-window geometry kept between sessions) · earlier: minor (wireless security: WEP, WPA, encryption type, coverage range) · earlier: minor (the PPPoE Dialer reaches a server through switches) · earlier: minor (PC desktop: Softphone) · earlier: minor (device windows can open as native OS windows) · earlier: minor (PC desktop: Netflow Collector) · earlier: minor (PC desktop: VPN Client) · earlier: minor (smart TVs, a sniffer, a smart speaker, a water level sensor and four more router models: 62 models in the device box) · earlier: minor (PC desktop: Traffic Generator, PPPoE Dialer; the server's PPPoE page) · earlier: minor (PC desktop: Telnet/SSH Client, Text Editor, Firewall, IPv6 Firewall) · earlier: minor (the Palette window: outline and fill of drawings; File ▸ Print report pages and compressed PDF) · earlier: minor (re-scored 2026-11-13; M9.10: the cellular network in the UI) · earlier: minor (Resize Shape; canvas devices as accessible nodes, M9.8) · earlier: minor (Constant Delay, the capture filter, origin decisions and absolute start times; instructor windows; touch screens, the phone's Settings, power supplies, the board Programming tab; Programming app and WLC page additions; figures aligned with ROADMAP.md) · **Target:** Cisco Packet Tracer 9.0

The window, element by element, against the reference app. Part of
[target-app-parity.md](target-app-parity.md). The arrangement was observed black-box (local
`plan/packet-tracer/observed-ui.md`); artwork and wording are ours. **Workspace and windows:
~88% ready, 8–14 h; Simulation mode: ~90% ready, 3–6 h** (estimated; the same figures as
[`ROADMAP.md`](../ROADMAP.md#dimensions)).

| Element | Reference app | NetLabCraft |
|---|---|---|
| Menu bar | File, Edit, Options, View, Tools, Extensions, Window, Help | same menus, our items: Edit has Copy, Paste, Undo and Redo with their shortcuts; View has Zoom (in, reset, out) and Toolbars (Main Ctrl+Shift+A, Secondary Ctrl+Shift+R, Bottom Ctrl+Shift+B: shown or hidden, kept between sessions by the desktop app), Simulation Mode (Shift+S), Realtime Mode (Shift+R), Logical View (Shift+L), Physical View (Shift+P), Show Viewport (Shift+V: a small map of the workspace; click or drag in it to pan), Show Workspace List (Shift+W: every device with its name, model and type; a click selects, a double click opens) and View Wireless Signals (Ctrl+Alt+W, also a button on the physical bar: see Physical workspace); Tools has Drawing Palette (Ctrl+D); Help has Getting Started, Contents (F1), Tutorials and Accessibility (short pages of our own, linking to the repository's docs), Report an Issue (opens the issue tracker in the browser) and About; File ▸ Open Recent on the desktop; File ▸ Print… makes an A4 PDF: the workspace picture on page 1, then a report read from the model (devices grouped as routers, switches and hubs, and end devices, with each interface's IPv4 address and mask, IPv6 addresses, VLAN or switchport mode, status and MAC address, then the list of cables), with page numbers and the title, creator and date in the file's properties; it opens in the system's PDF viewer to print (the web build downloads it); the web build asks before closing a tab with unsaved changes |
| Main toolbar | file, print, info, copy/paste, undo/redo, zoom, palette… ; help at right | new, open, save, network information, copy/paste devices, undo, redo, zoom in/reset/out, capture export; help at right |
| Second toolbar | select, inspect, delete, resize, note, drawing, simple/complex PDU | select, inspect, delete, place note, line, rectangle, ellipse, freehand, polygon, simple PDU, complex PDU (also under Tools), group into a cluster; resize works on rectangles, ellipses and lines (click the drawing with Select: eight handles on a box, two on a line; the `note.resize` command, one undo step per drag, sides kept between 4 and 100000 units); a Palette window shows with a drawing tool or a selected drawing: outline on/off, outline colour and width (1-8), No fill or Fill with its colour (rectangle, ellipse, polygon; a line or freehand stroke has outline colour and width), shape buttons; it sets the default for the next drawings and edits the selected one (`note.style`, one undo step per change) |
| Workspace bar | Logical / Physical, coordinates, Root, navigation, clock | Logical / Physical, coordinates, a clickable Root › cluster › … path with back, clock |
| Opening a lab | the view as saved | a lab that opens or is created is fitted to the workspace: centred and zoomed out (never past 1:1) with room for labels and notes; opening a cluster and switching views fit the same way |
| Workspace | white, icons with model + name, port labels, link triangles; red serial cables with a clock at the DCE; EtherChannel marks | same arrangement, icons drawn in code; device names on a soft plate, port labels kept clear of name blocks and of each other; parallel cables side by side, a ring around each EtherChannel, red zigzag serial cables with a clock at the DCE end, a router mark on multilayer switches |
| Time bar | time, power cycle, fast forward; Realtime / Simulation | same; the right-hand tabs are placed first and the hint and captions give way, so nothing overlaps in either language or mode (tested) |
| Device box | categories, search, subcategories, model tiles, model name box | same; 65 models of our own (routers, modular routers, a firewall, switches incl. a multilayer one, one with fiber uplinks and two with PoE, a home router and a home gateway, an access point, a wireless controller and lightweight APs, a hub, PCs, laptops incl. Wi-Fi, a tablet and a smartphone, a server, a printer, a sniffer, an IP phone, twenty-two IoT things, two programmable boards; a WAN Emulation group with the cloud and the DSL and cable modems, a Cellular group with the cell tower and the central office server) in wrapping rows with two-line names; the cable tiles (coaxial, phone and IoT custom too) are under Connections |
| Scenario / PDU list | scenario, New/Delete, PDU table | scenario menu (Scenario 0, 1, …) with New and Delete, each scenario keeping its own PDUs in the lab file; Fire All; PDU table (Fire, Last Status, Source, Destination, Type, Color, Time, Periodic, Num, Delete) |
| Create Complex PDU | source, application, destination, ports, TTL, TOS, sequence number, size, one-shot or periodic with a time | a window from the toolbar and the Tools menu: source device and port, application (ping, DNS, HTTP, Telnet, SSH, FTP, SMTP, POP3, DHCP, TFTP, SNMP, ARP, raw UDP or TCP), destination, ports, TTL, TOS or DSCP, sequence number, size, the DNS name, page, file or community, one-shot or periodic with a start time counted from now or at an absolute simulated time (saved with the lab) |
| Simulation Panel | event list (Vis., Time, Last/At Device, Type, Info), Reset Simulation, play controls with speed, filters, Edit Filters / Show All/None; time bar with ms and play controls, Event List toggle | same arrangement, with columns that fit the panel; protocol frames (BPDUs, OSPF, RIP, EIGRP, HSRP, LLDP, LACP, DHCP, DNS, TCP…) listed with their own filters and colours; plus drop reasons and pcap export; a Constant Delay box (every hop the same simulated time, 1 ms by default) and "Capture only the visible events", which keeps only the protocols the filters show in the event list and the pcap |
| PDU information | OSI model and per-layer details | opens on an OSI Model view: In and Out layers with the decode and what the receiver decided (MAC table, routing lookup, TTL, ARP, access lists, NAT; up to 8 steps), then Inbound and Outbound PDU details and the hex dump; opens below the toolbars. The sender's own decisions (routing lookup, ARP) are recorded on the first frame of a packet it raises and shown too |
| Router/switch window | Physical, Config, CLI, Attributes | same tabs; Config: bordered list with bold sections; Global Settings with Display Name and Hostname, NVRAM Erase/Save, Startup Config Load/Export, Running Config Export/Merge; VLAN database, static routes, interfaces; console output underneath |
| PC window | Physical, Config, Desktop, Programming, Attributes | Physical, Config, Desktop (navy desktop with tiles: IP Configuration with DHCP/Static and IPv6 Off/Automatic/Static, Command Prompt, Web Browser, Email, MIB Browser, Terminal over a console cable, Programming, and on PCs, laptops and servers the Telnet/SSH Client, Text Editor, Firewall, IPv6 Firewall, Traffic Generator, PPPoE Dialer, VPN Client, Netflow Collector and Softphone), Attributes; only working apps are shown. Programming is a Desktop app rather than a tab: RESTCONF mode (method, URL, Basic login, headers, JSON body, saved requests, answers with status, headers and a folding JSON viewer), NETCONF mode (framing and operations) and Controller mode (a seed and Discover, the topology found, managed routers with model and software columns, a variables box, push a template); NETCONF has a keep-open checkbox and a close button, and the app says how a request was carried (TLS, SSH); it sends requests, it doesn't run scripts (programmable boards run BoardScript in their own Programming tab) |
| Server window | Physical, Config, Services (HTTP, DHCP, DHCPv6, TFTP, DNS, SYSLOG, AAA, NTP, EMAIL, FTP, IoT…), Desktop, Programming, Attributes | Physical, Config, Services (HTTP with file manager, DHCP pools, DNS records, SYSLOG messages, NTP, TFTP files, AAA clients and users, FTP accounts and files, EMAIL domain and users), an IoT tab for the IoT service (things listed live, webcam stills, rules in words with several conditions), Desktop with the Programming app, Attributes |
| Home gateway, phones, PoE switches, cloud, power supplies | own pages per device | the Home Gateway has the home router's page and an IoT tab drawing each thing's state; the Phone tab has a dial pad and a Settings screen (protocol, call server, extension, MAC, voice VLAN, TFTP server, DHCP or static address); Physical tabs show a powered device's adapter plug and power source, a PoE switch's budget and lit PoE ports, a cloud's port pairs, a router's switch card with link lights, and the power-supply bays of routers and switches (a fan grille and light per supply, empty bays; click to take one out or fit it) |
| Tablets and smartphones | touch home screen, apps | a touch home screen drawn in code with rounded app icons (browser, email, IP configuration, command prompt, programming and Settings), the same apps as the Desktop; the status bar names the connection in use (Wi-Fi, or cellular with the tower and the address, or no service) with its signal bars; Settings holds Cellular Data and the APN; the Physical tab draws the device from the front with its Wi-Fi radio, what it is associated with and its cellular signal |
| Cellular network | cell towers, central office, signal on phones | a cell tower and a central office server drawn in code (a lattice mast with radio waves; a flat-roofed building with an antenna); a tower's coverage is a faint orange disc with a dashed rim on the logical workspace and in the Physical view (metres on a city map, once the lab has a layout); a phone shows signal bars (1 to 4, grey with no service) beside its icon and a dashed orange line to its tower; the Config tab has a Cellular page on a central office server (pool, APN, data plan, the devices admitted and refused) and on a tower (its sectors), and the phone's Cellular0 page (Cellular Data, APN, status); the attach frames are protocol CELL in the event list |
| Programmable boards | Programming tab with a code editor, Physical tab | a Programming tab: the BoardScript editor, Run and Stop, what the program printed, its variables and pins with the IoT custom cable of each; the board drawn in its Physical tab |
| Wireless controller | web pages | the Wireless page lists WLANs with a central-switching box per WLAN; an access point's, home router's and host's Wi-Fi form has radio buttons for the authentication and hides the fields that do not apply |
| Web Browser | URL bar with back/forward, Go, Stop; page view | same; renders headings, paragraphs, bold, lists and links |
| Activities | Instructions window with timer and score, Check Results tree, Reset Activity; Activity Wizard with answer network, initial network, assessment tree, connectivity tests, locks | Instructions window in the interface language with Completion, a timer, Check Results per section, Compare with the Answer and Reset Activity (back to the initial network); Activity Wizard (title, instructions with a tab per language, values per student, capture answers, ping/config/command/address/cable checks, sections, hints, hidden answers, time limit, password lock) with Answer Network (a read-only drawing of it), Initial Network, an Assessment Tree with points and a hint per item that makes the checks, Connectivity Tests, signed results saved as JSON or CSV and a check of a student's results file, and locks (devices, cables, per-device configuration) |
| Clusters | grouped devices shown as one cloud; open, rename, uncluster | same: pick with Shift+drag or Shift+click, Ctrl+G groups, a code-drawn cloud with the device count, double-click to open, nesting, cables to the cloud at the parent level |
| Physical workspace | Intercity map, cities, buildings, wiring closets; cable lengths | same levels with code-drawn icons and a path bar (Intercity › City › Building › Closet); new places, rename, remove, move devices between places; cables between places labelled with their length, copper over 100 m goes down and says why; lengths from the places or set by hand; multi-mode fiber runs up to 550 m and single-mode fiber up to 10 km. Once a lab has a layout, a Wi-Fi host reaches only the access points of its own building (walls, not metres, decide) |
| Connections | cable types in the device box | thirteen tiles in the reference order, each a code-drawn connector picture in its own colour (RJ45-style plugs for Console, Straight-Through and Cross-Over, an RJ11-style plug for Phone, duplex fiber connectors marked S and M, a coaxial connector, D-shells with pins or sockets for Serial DTE and DCE, an octal fan, bare wires for IoT, a USB plug with a trident): Automatically Choose Connection Type (a lightning bolt), Console, Copper Straight-Through, Copper Cross-Over, Single Mode Fiber, Multi Mode Fiber, Phone, Coaxial, Serial DCE, Serial DTE, Octal, IoT Custom Cable and USB; the port menu greys out ports a cable doesn't fit and says why; each kind drawn its own way; the PC's Terminal app over a console or USB cable; an octal cable from a router's octal card (`Card-8A`) to another device's Console port; a model tile (and a Structured Cabling tile) can also be dragged from the device box onto the logical workspace, a physical place or a wiring closet, with a ghost under the pointer (on a rack, shelf or table the slot it would take lights up; Esc or letting go outside the workspace cancels), through the same commands as a click; cable tiles are clicked, not dropped |
| Physical workspace | Intercity map, cities, buildings, wiring closets; cable lengths | same levels with code-drawn icons and a path bar (Intercity › City › Building › Closet); new places, rename, remove, move devices between places; cables between places labelled with their length, copper over 100 m goes down and says why; lengths from the places or set by hand; fiber runs up to 2 km. Once a lab has a layout, a Wi-Fi host reaches only the access points of its own building (walls, not metres, decide). View Wireless Signals (Ctrl+Alt+W, View menu, or the button on this bar, kept in the `preferences` file; a UI setting, not part of the lab) draws a faint disc under the cables for every access point, wireless router and home router with a radio that offers a network, tinted by its channel and named by its SSID: the radius is the radio's coverage range in metres at the scale of the cable lengths (the logical range in workspace units while the lab has no layout), and a Wi-Fi host that is associated with an access point (the same association the simulator uses, so the walls of a building count) gets a thin line to it; the Logical workspace draws the same discs (its hosts already have their dashed association lines). Inside a wiring closet the view changes to the closet's own: racks with numbered units, shelves with levels and tables (add them from the right-click menu), devices dragged onto them (a rack stacks them by units with the name under each, shelves and tables hold them loose; dragging off takes them off), a front panel drawn in code for every device with one socket per port and a link light, patch panels, wall mounts and power distribution devices, and every cable drawn from port to port as a curve in its kind's colour (straight-through green, cross-over orange, fiber gold, console light blue, serial red, coaxial brown, phone teal), with a flagged cable haloed in red and the picked cable or the selected device's cables highlighted; drag from one port to another to patch them and from a power outlet to a device to plug it in; furniture and devices are accessible nodes and the device box has a Structured Cabling group (Ctrl+Alt+K). With a cable tile picked (Automatically Choose Connection Type or any kind), a click on a port of a front panel starts a cable (the port lights and a line in the cable's colour follows the pointer), a click on a port of another device finishes it through `link.add` (a patch panel, wall mount or fiber panel port through `patch.wire`, as the drag does); Esc or a right-click drops the cable and the tool stays picked; a busy port or a cable that doesn't fit is refused with the reason in the status line, and ports the cable cannot join are dimmed. Dragging between ports still works with Select |
| Dialogs | open over the workspace | About, Compare with the Answer, PDU information and the Activity Wizard open below the toolbars; IP Configuration and Global Settings laid out on a grid |
| Language | English plus community files | English and Spanish; every window, tab, page and mode is swept by a test that fails on English text left untranslated ([localization-parity.md](localization-parity.md)) |
| Accessibility | — | each device on the workspace is an accessible node (`Name, model` and `, selected`, role button): Tab or the arrow keys (device id order) move between them and select, Enter, Space or a screen reader's click opens the device; every cable is a node too (`Cable, kind, A port - B port`, Tab and arrows between cables, Enter or a click opens the device at its first end, the focused cable highlighted); native device windows remember their size and position by device name between sessions |
| Options menu | Preferences… (Ctrl+R), Algorithm Settings (Ctrl+Shift+M), User Profile… (Ctrl+Shift+U), View Command Log (Ctrl+Shift+V) | the same four items and shortcuts, then Dark interface, Show grid, Device windows as separate windows and Language (which also sit in Preferences); Algorithm Settings opens a note, because its contents have not been observed |
| Preferences | A window with the tabs Interface, Administrative, Show/Hide, Font, Miscellaneous, Answer Tree, Custom Interfaces, Publishers and Image Cleanup | the same nine tabs. **Interface:** Show Animation (the replayed packet moves, or sits mid-cable), Play Sound and Play Telephony Sound (short tones generated in code, played by the shell: PDU arrived or failed; IP phone call, Softphone and dial-up), Show Device Model Labels and Name Labels, Always Show Port Labels and Show Port Labels When Mouse Over (the cables of the device under the pointer), Disable Auto Cable (the Automatic cable leaves the device box), Use Metric System (physical-workspace cable lengths in metres or feet), Align logical and physical workspace objects (grid snapping), Tooltip Duration, Show Link Lights, Show QoS Stamps on Packets (DSCP and 802.1p on the replayed packet), Enable Cable Length Effects (`physical.length_effects`, saved with the lab), Use CLI as Device Default Tab (off: routers and switches open on Config), Show Cable Info Popup in Physical Workspace (a tooltip over a cable run), Enable Logging and Export Log. **Font:** interface text size 70–160%. **Show/Hide:** the same three toolbars as View ▸ Toolbars (one setting). The other six tabs say "not available yet" ([gaps.md](gaps.md) item 12). Restore Defaults resets them. Kept between sessions on the desktop (`preferences` file); the web build starts from the defaults |
| User Profile | A window with Name, E-Mail and a multi-line Additional Info, OK and Cancel | the same three fields with OK (saves) and Cancel (drops the edits), and one line saying they stay on this computer. They are kept in the `preferences` file and in the session (`profile.get`, `profile.set`, which sets the fields given); never sent anywhere and never written into a lab file. Only the name is used: it names the student in activity results when the activity was not started with a name, and the activity window offers it first; the e-mail and the info are not put in the results, and the command log writes them as `***`. An older `preferences` file without them still loads |
| Command log | View Command Log | a window listing every command the session ran (number, simulated time, command, parameters with passwords and keys hidden, result; a drag is one line), with Copy, Export Log (a text file), Clear and Enable Logging (`log.list`, `log.export`, `log.clear`, `log.enable`); the newest 2000 are kept |
| Native device windows | Device windows are separate OS windows | Options ▸ Device windows as separate windows (off by default; desktop builds only, greyed out on the web) opens each device in its own window with the device's name as title; size and position kept for the session |
| Not yet | — | scripts in the PC Programming app |

## Keyboard shortcuts

One table in code (`crates/ui-egui/src/shortcuts.rs`, `TABLE`) drives the keys, the shortcut text in the menus and the Help ▸ Keyboard shortcuts page; a new shortcut is a row there and, for a new action, a branch in `perform`. A chord with more modifiers is read before one with fewer (egui lets a shortcut without Shift match with Shift down), and a row marked *idle* is skipped while a text field or the console has the keyboard, so typing a letter never picks a tool. Counted from the table: **75 shortcuts** in 11 groups.

**File**

| Keys | Action | While typing |
|---|---|---|
| `Ctrl+N` | New | works |
| `Ctrl+O` | Open... | works |
| `Ctrl+Shift+T` | Open Samples | ignored |
| `Ctrl+S` | Save | works |
| `Ctrl+Shift+S` | Save As... | works |
| `Alt+N` | Reset Activity | ignored |
| `Ctrl+P` | Print... | works |

**Edit**

| Keys | Action | While typing |
|---|---|---|
| `Ctrl+C` | Copy | ignored |
| `Ctrl+V` | Paste | ignored |
| `Ctrl+Z` | Undo | ignored |
| `Ctrl+Shift+Z` | Redo | ignored |
| `Ctrl+Y` | Redo | ignored |
| `Ctrl+G` | Group into Cluster | ignored |
| `Shift+U` | New Cluster | ignored |
| `Delete` | Delete | ignored |

**Options**

| Keys | Action | While typing |
|---|---|---|
| `Ctrl+R` | Preferences… | ignored |
| `Ctrl+Shift+F` | Network Information | ignored |
| `Ctrl+Shift+U` | User Profile… | works |
| `Ctrl+Shift+M` | Algorithm Settings | works |
| `Ctrl+Shift+V` | View Command Log | works |
| `Shift+G` | Show grid | ignored |

**View**

| Keys | Action | While typing |
|---|---|---|
| `Ctrl+I` | Zoom In | ignored |
| `Ctrl+T` | Zoom Reset | ignored |
| `Ctrl+U` | Zoom Out | ignored |
| `Shift+S` | Simulation Mode | ignored |
| `Shift+R` | Realtime Mode | ignored |
| `Shift+L` | Logical View | ignored |
| `Shift+P` | Physical View | ignored |
| `Shift+V` | Show Viewport | ignored |
| `Shift+W` | Show Workspace List | ignored |
| `Ctrl+Alt+W` | View Wireless Signals | ignored |
| `Ctrl+Shift+A` | Main Toolbar | ignored |
| `Ctrl+Shift+R` | Secondary Toolbar | ignored |
| `Ctrl+Shift+B` | Bottom Toolbar | ignored |
| `Alt+Left` | Go Back One Level | ignored |

**Physical**

| Keys | Action | While typing |
|---|---|---|
| `Shift+C` | New City | ignored |
| `Shift+B` | New Building | ignored |
| `Shift+J` | New Wiring Closet | ignored |

**Tools**

| Keys | Action | While typing |
|---|---|---|
| `Ctrl+W` | Activity Wizard… | ignored |
| `Ctrl+D` | Drawing Palette | ignored |
| `Esc` | Select | ignored |
| `N` | Place Note | ignored |
| `I` | Inspect | ignored |
| `P` | Add Simple PDU | ignored |
| `C` | Add Complex PDU | ignored |
| `L` | Draw Line | ignored |
| `R` | Draw Rectangle | ignored |
| `E` | Draw Ellipse | ignored |
| `F` | Draw Freehand | ignored |

**Simulation**

| Keys | Action | While typing |
|---|---|---|
| `Alt+S` | Power Cycle Devices | ignored |
| `Alt+D` | Fast Forward Time | ignored |
| `Alt+B` | Simulation Back | ignored |
| `Alt+P` | Auto Capture / Play | ignored |
| `Alt+C` | Capture / Forward | ignored |
| `Alt+I` | Show Event List | ignored |

**Scenarios**

| Keys | Action | While typing |
|---|---|---|
| `Ctrl+Shift+N` | New Scenario | ignored |
| `Ctrl+Shift+E` | Delete Scenario | ignored |
| `Ctrl+Shift+O` | Toggle PDU List Window | ignored |
| `Ctrl+=` | Toggle PDU List Window | ignored |

**Device box**

| Keys | Action | While typing |
|---|---|---|
| `Ctrl+Alt+R` | Routers | works |
| `Ctrl+Alt+S` | Switches | works |
| `Ctrl+Alt+U` | Hubs | works |
| `Ctrl+Alt+N` | WAN Emulation | works |
| `Ctrl+Alt+V` | End Devices | works |
| `Ctrl+Alt+O` | Connections | works |
| `Ctrl+Alt+K` | Structured Cabling | works |

**Selection**

| Keys | Action | While typing |
|---|---|---|
| `Enter` | Open Device | ignored |
| `Ctrl+Up` | Move Device Up | ignored |
| `Ctrl+Down` | Move Device Down | ignored |
| `Ctrl+Left` | Move Device Left | ignored |
| `Ctrl+Right` | Move Device Right | ignored |
| `Shift+Up` | Select Next Device Up | ignored |
| `Shift+Down` | Select Next Device Down | ignored |
| `Shift+Left` | Select Next Device Left | ignored |
| `Shift+Right` | Select Next Device Right | ignored |

**Help**

| Keys | Action | While typing |
|---|---|---|
| `F1` | Contents | works |

Not bound, because NetLabCraft has nothing for them (the work is in [gaps.md](gaps.md) item 5): Ctrl+Shift+H (dock the Instruction window), Ctrl+Shift+I (scenario description), Ctrl+Shift+P (Port Monitor), Shift+E (Environment window), Shift+F (generic container), Shift+H (working closet), Shift+M (Move Object), Shift+N (Navigation panel), Alt+R (Resize Shape: shapes resize with their handles), Ctrl+Space and Space (rubber-band selection and selecting a device by key: Shift+drag and the accessibility focus do those), Alt+F, E, O, V, T, X and H (opening a menu by key), Y and N for confirmations (we have no confirmation dialog; N is Place Note), Alt+Left in the logical view outside a cluster, the device boxes for wireless, security, home, smart city, industrial, power grid, boards, actuators, sensors and miscellaneous (our catalogue has no such groups), multiuser, IPC, Pkz, Marvel, cluster association and the tiled background (not features here), and the Text Editor's Ctrl+N, Ctrl+O and Ctrl+S. Ctrl+Insert and Shift+Insert copy and paste in the console and in every text field because the toolkit binds them.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-17 | minor | Wiring closet: cables by clicking a cable tile and two ports (first port lit, line follows the pointer, Esc or right-click cancels, busy or misfit ports refused with the reason); device-box tiles can be dragged onto the logical workspace, a physical place and a wiring closet. Not re-scored. |
| 2026-11-17 | minor | User Profile: E-Mail and Additional Info next to Name, with OK and Cancel; View ▸ View Wireless Signals (Ctrl+Alt+W, 76 shortcuts) and a toggle button on the physical workspace bar draw the radios' coverage areas and the association lines. |
| 2026-11-17 | minor | The stale duplicate Connections row (the one without the connector pictures) removed. |
| 2026-11-17 | minor | Connections: the tiles show an upright connector per cable kind (RJ45, RJ11, duplex fiber, coaxial, D-shell with pins or sockets, octal fan, bare wires, USB) in colours from the theme tokens, instead of diagonal lines; the canvas cables are unchanged. |
| 2026-11-17 | minor | Keyboard shortcuts: one table (`shortcuts.rs`) with 75 rows replaces the scattered key handlers; Ctrl+Alt+R, S, U, N, V and O open the device box categories; tool, time, scenario, physical, selection and file chords; Open Samples (Ctrl+Shift+T); a Help ▸ Keyboard shortcuts page. The Preferences, User Profile, Algorithm Settings and Command Log keys moved into the table. |
| 2026-11-17 | minor | Connections: thirteen tiles in the reference order with the automatic one first; fiber split into Single Mode and Multi Mode; Octal and USB tiles with their glyphs and status-bar help; the automatic tile is a lightning bolt. |
| 2026-11-17 | minor | Wiring closet view: racks, shelves and tables, devices on them by slot, front panels drawn in code, cables port to port in kind colours, patch panels, wall mounts, power distribution devices, the Structured Cabling group of the device box (Ctrl+Alt+K). Not re-scored. |
| 2026-11-17 | minor | Device box: the model count is 65 (the three Dial-up models are included). |
| 2026-11-17 | minor | Options ▸ Preferences (Interface, Font and Show/Hide built, six tabs and Algorithm Settings as notes), User Profile and View Command Log, with their shortcuts; the command log, the profile and cable length effects are engine commands. Defaults keep what NetLabCraft did before (aligned logical workspace, lengths counted, CLI first). Not re-scored. |
| 2026-11-17 | minor | Menu bar: Copy and Paste in Edit; View ▸ Zoom, Toolbars (hide the main, secondary or bottom bar), Simulation and Realtime Mode, Logical and Physical View (these two replace Logical and Physical Workspace), Show Viewport, Show Workspace List; Tools ▸ Drawing Palette; Help pages, F1 and Report an Issue. Shift+S now picks Simulation (Shift+R picks Realtime) instead of toggling. Not re-scored. |
| 2026-11-16 | minor | PC desktop: Dial-up (phone number, user name, password, Dial and Hang up; the server, address, gateway and authentication of the call) for computers with a modem port, and a Telephone numbers form in the Telephone Cloud's Physical tab. Not re-scored. |
| 2026-11-16 | minor | Cables are accessible nodes on the workspace; native device windows keep their size and position between sessions. Not re-scored. |
| 2026-11-16 | minor | The Wi-Fi form of access points, home routers and wireless hosts: SSID, channel 1-11, coverage range, Authentication as radio buttons (Disabled, WEP, WPA-PSK, WPA2-PSK, WPA, WPA2), the fields each needs (WEP key, PSK pass phrase, RADIUS server settings or a login) and an Encryption Type; six new rows in the six catalogs. |
| 2026-11-16 | minor | The PPPoE Dialer no longer needs a server on its own cable: its hint says so (one row changed in the six catalogs). |
| 2026-11-16 | minor | PC desktop: Softphone (a switch, the call server, the MAC address, the IP phone's screen and dial pad with Call, Pick up, Decline, Hang up and Refresh, and the SIP checkbox) for PCs, laptops and servers. Not re-scored. |
| 2026-11-15 | minor | PC desktop: Netflow Collector (port, Start, Stop and Clear; datagram, flow, packet and byte totals; top talkers by bytes, protocols and the newest 100 flows) for PCs, laptops and servers. Not re-scored. |
| 2026-11-15 | minor | PC desktop: VPN Client (group name, group key, host IP address, user name, password; Connect and Disconnect; the server, the VPN address, the encryption and the tunnel packet counts while connected) for PCs, laptops and servers. Not re-scored. |
| 2026-11-15 | minor | Device box: 62 models (four routers, a sniffer with a Capture tab, two smart TVs, a smart speaker, a water level sensor); the Capture tab. |
| 2026-11-15 | minor | PC desktop: Traffic Generator (destination, ICMP, UDP or TCP, port, size, count, interval; Send; sent, received and lost, and a table of each packet) and PPPoE Dialer (user name, password, Connect and Disconnect; server, address, gateway and authentication of the session, or that the connection was lost) for PCs, laptops and servers. A server's Services tab gains a PPPoE page (on or off, PAP or CHAP, pool start and size, accounts). Not re-scored. |
| 2026-11-15 | minor | PC desktop: Telnet/SSH Client (protocol, host, user name; the session is the Command Prompt's console), Text Editor (file list, name, text, save, delete; 32 files of 16 KiB), Firewall and IPv6 Firewall (switch, rule form, rule table with move and remove) for PCs, laptops and servers. Not re-scored. |
| 2026-11-14 | minor | File ▸ Print: a report after the picture (devices, interfaces, cables, page numbers), a compressed picture and file properties. |
| 2026-11-14 | minor | Palette window (drawing styling): separate outline and fill for drawings, the session default for new shapes, and the selected shape edited through `note.style`; file version 4. Not re-scored. || 2026-11-13 | minor | Workspace and windows re-scored from ~85% to ~88% (Resize Shape, accessible devices); 8–14 h left. |
| 2026-11-13 | minor | Cellular network (M9.10): a Cellular group in the device box, tower and central office pictures, coverage circles, signal bars and attach lines, Config > Cellular pages, the Settings app on tablets and smartphones, the status bar's active connection. |
| 2026-11-13 | minor | Canvas devices as accessible nodes with keyboard navigation (M9.8). |
| 2026-11-13 | minor | Resize Shape (M9.8): handles on a selected rectangle, ellipse or line, backed by `note.resize`. |
| 2026-10-31 | minor | Constant Delay, "Capture only the visible events", the sender's decisions in the OSI view, absolute PDU start times; the instructor windows (Answer Network, Initial Network, assessment tree, languages, Connectivity Tests, results, Locks); tablet and smartphone touch screens, the phone's Settings screen, power-supply bays, the board Programming tab, the IoT Custom cable tile; Controller Discover and the NETCONF keep-open box; central switching on the WLC page; 51 models; figures aligned with ROADMAP.md. |
| 2026-10-30 | minor | Create Complex PDU window, scenario menu, OSI Model view of PDU details, the Programming app (RESTCONF, NETCONF, Controller), IoT tab depth, the Home Gateway, Phone tab SIP box, PoE, switch-card and cloud Physical tabs, coaxial and phone cables, Wi-Fi range by building; 37 models; figures aligned with ROADMAP.md. |
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
