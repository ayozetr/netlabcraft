# Hardware parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (workspace image export) · **Target:** Cisco Packet Tracer 9.0

Two kinds of hardware matter here: the **simulated** hardware (device models, ports, cables,
modules) and the **real** hardware the app runs on. Part of
[target-app-parity.md](target-app-parity.md). **Simulated hardware: ~30% ready, 140–210 h;
host hardware: ~70% ready, 10–16 h** (estimated).

## Simulated devices and cables

| Feature | Reference app | PacketCraft | Gap and estimate |
|---|---|---|---|
| Routers | Many models, modular slots (HWIC, NIM) | Router-2/3/4 (Gigabit Ethernet), Router-WAN (2 Gigabit Ethernet + 2 serial); generic names | Modules added in the Physical tab with the power off: 20–30 h |
| Switches | Access and multilayer models | Switch-8/24/48, Switch-ML-24 (multilayer) | — for CCNA labs |
| Hubs, PCs, laptops, servers, printers | yes | yes | — |
| Wireless (access points, wireless routers, wireless clients) | yes | 🟡 Home-Router (Internet port, 4 LAN ports, Wi-Fi; DHCP and NAT set up at the factory; a Home Router settings page for LAN, DHCP, static Internet and Wi-Fi, all as configuration commands), AccessPoint and Laptop-WiFi models: SSID, open or WPA2-PSK, channel, association by range (nearest access point), relay between clients; frames captured as Ethernet | WLC and lightweight APs, 802.1X/RADIUS, 802.11 frames, roaming, a DHCP client on the Internet port: 18–30 h |
| Firewalls (ASA), IoT devices, phones, cloud/modems | yes | ❌ | 40–60 h; low priority for CCNA |
| Copper cables | Straight-through and crossover (with mismatch errors) | One copper cable; every port is auto-MDIX | By design; a "strict cabling" option: 2–4 h |
| Serial cables | DCE and DTE | ✅ the first end cabled is the DCE; red cable with a clock at the DCE end | — |
| Fibre, console, coaxial, phone cables | yes | ❌ | Fibre ports and console sessions over a cable: 8–12 h |
| Physical workspace (cities, buildings, racks, cable lengths) | yes | ❌ | 40–60 h |
| Power switch and LEDs in the Physical tab | yes | 🟡 Physical tab lists ports; no power switch or module bays | 6–10 h |

## Host hardware

| Hardware feature | Reference app | PacketCraft (Linux · Windows · macOS · BSD · web) | Gap and estimate |
|---|---|---|---|
| GPU rendering | Desktop toolkit | wgpu through eframe: Vulkan/GL · DirectX 12 · Metal · Vulkan/GL · WebGPU or WebGL2 | At parity in kind; verify on each platform: 2–4 h |
| High-DPI and display scaling | yes | yes (egui scales with the display) | — |
| Multiple windows | Device windows are separate OS windows | Device windows float inside the main window | Native windows per device: 4–6 h |
| Keyboard and mouse | yes | yes; click opens a device, scroll pans, Ctrl+scroll zooms | — |
| Touch and pen | Limited | egui's touch handling, untested | Verify on a touch screen: 1–2 h |
| Multi-core | Single simulation thread | Single-threaded discrete-event simulator (deterministic) | Not needed at lab sizes |
| Accessibility (screen readers) | Partial | AccessKit tree; every widget has an accessible name (checked by the UI tests) | Canvas devices as accessible nodes: 3–5 h |
| Printing | Print the workspace | 🟡 File ▸ Export Workspace Image (PNG), desktop and web, to print or put in a report | Direct printing and PDF: 3–5 h |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Workspace image export (PNG). |
| 2026-10-30 | minor | Home wireless router. |
| 2026-10-30 | minor | Wi-Fi access point and laptop. |
| 2026-10-30 | major | Created: simulated device families and cables, host hardware. |
