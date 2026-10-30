# Hardware parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (cable types) · **Target:** Cisco Packet Tracer 9.0

Two kinds of hardware matter here: the **simulated** hardware (device models, ports, cables,
modules) and the **real** hardware the app runs on. Part of
[target-app-parity.md](target-app-parity.md). **Simulated hardware: ~30% ready, 140–210 h;
host hardware: ~70% ready, 10–16 h** (estimated).

## Simulated devices and cables

| Feature | Reference app | NetLabCraft | Gap and estimate |
|---|---|---|---|
| Routers | Many models, modular slots (HWIC, NIM) | Router 2GE/3GE/4GE, Router 2GE+2S, Modular Router 2GE (2 bays) and 3GE (4 bays) with serial (2 and 4 ports), Gigabit (1 and 2 ports) and Fast Ethernet cards fitted while switched off; power on and off (boots from the saved configuration); a Physical tab drawn in code; [model-equivalents.md](model-equivalents.md) | A switch card (switching inside a router), power supplies as parts: 6–10 h |
| Switches | Access and multilayer models | Switch 8FE/24FE/48FE, L3 Switch 24 (multilayer) | — for CCNA labs |
| Hubs, PCs, laptops, servers, printers | yes | yes | — |
| Wireless (access points, wireless routers, wireless clients) | yes | 🟡 Home Router, Access Point and Wi-Fi Laptop; real IEEE 802.11 frames (probe, authentication, association, reassociation, beacons every 2048 TU, data); WPA2-PSK 4-way handshake and CCMP (AES-CCM) on data frames; WLC and LAP with a CAPWAP join, WLANs pushed to joined APs, a VLAN per WLAN tagged on the AP's trunk uplink; WPA2-Enterprise via RADIUS, PEAP shown as an exchange; roaming between APs of the same SSID; a separate 802.11 capture; wlc sample | Group keys, a real PEAP/TLS tunnel, range by distance, central tunnelling: 6–10 h |
| Firewalls | yes | 🟡 Firewall 5GE: named interfaces and subinterfaces with security levels, a stateful connection table with timeouts, ICMP errors and traceroute through it, ICMP rules for the firewall itself, extended access lists with object groups and remarks bound in or out, object NAT and twice NAT, static routes with distances, a DHCP server, a global policy with `inspect icmp`; firewall sample | Route distance in path choice, more inspections: 4–6 h |
| IP phones | yes | 🟡 IP Phone with a PC port; `switchport voice vlan`; DHCP option 150; call control on routers (`telephony-service`, `ephone-dn`, `ephone`, `auto assign`, `show ephone`); registration, ringing, answer, hang up over our own documented signalling, RTP packets while a call is up; a Phone tab with a dial pad; ip-phones sample | Power over Ethernet, SIP, the phone's own setup screen: 8–12 h |
| IoT devices | yes | 🟡 Smart Sensor and Smart Device registering to a server's IoT service over HTTP; the server lists them and runs rules (if a reading passes a value, switch a device); an IoT tab; iot sample | Reports on a timer, more kinds of things, a home gateway: 8–12 h |
| Cloud, DSL and cable modems | yes | ❌ | 8–12 h; low priority for CCNA |
| Copper cables | Straight-through and crossover (with mismatch errors) | ✅ straight-through and cross-over, a wrong cable leaves the link down with the reason unless auto-MDIX (routers and switches) fixes it; automatic cable; solid and dashed lines | — |
| Serial cables | DCE and DTE | ✅ the first end cabled is the DCE; red cable with a clock at the DCE end | — |
| Fibre and console cables | yes | ✅ fiber between fiber or combo ports (switch Gigabit uplinks, Switch 24FE+2SFP, a fiber router card), orange, up to 2 km; console (rollover) from a PC's RS-232 port to a device's Console port, opened by the PC's Terminal app with no IP | — |
| Coaxial and phone cables | yes | ❌ | Only with cable/DSL modems: 4–6 h |
| Physical workspace (cities, buildings, racks, cable lengths) | yes | ✅ see [ui-parity.md](ui-parity.md) | — |
| Power switch and LEDs in the Physical tab | yes | 🟡 Physical tab lists ports; no power switch or module bays | 6–10 h |

## Host hardware

| Hardware feature | Reference app | NetLabCraft (Linux · Windows · macOS · BSD · web) | Gap and estimate |
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
| 2026-10-30 | minor | Cable types, port media, fiber ports, console cable and the PC Terminal. |
| 2026-10-30 | minor | Modular routers and cards, power, IP phones, IoT devices; wireless and firewall depth. |
| 2026-10-30 | minor | Workspace image export (PNG). |
| 2026-10-30 | minor | Home wireless router. |
| 2026-10-30 | minor | Firewall 5GE: a stateful firewall appliance. |
| 2026-10-30 | minor | Model names describe their ports; equivalents page. |
| 2026-10-30 | minor | WLC and LAP models, 802.11 frames, WPA2 handshake, WPA2-Enterprise with RADIUS. |
| 2026-10-30 | minor | Wi-Fi access point and laptop. |
| 2026-10-30 | major | Created: simulated device families and cables, host hardware. |
